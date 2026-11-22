//! Playback, two engines:
//! - hardware (default): h96player (tools/video/h96player.c: GStreamer playbin -> mppvideodec
//!   (VPU) -> NV12 dmabuf -> kmssink on a VOP2 plane above the console, hardware-scaled; audio to
//!   ALSA; playlist HTTP headers; prebuffer). h96ui keeps the terminal and reads the remote
//!   itself: Up/Down = channel -/+, Back = stop. (NOTES §34-§36)
//! - software: hand the screen to mpv (CPU decode + scaling; slow, kept as a fallback).

use crate::m3u::Channel;
use crate::store::{data_dir, Settings, Subscription};
use crate::term::Key;
use crate::{write_out, App};
use std::io::{Read, Write};
use std::os::fd::AsRawFd;
use std::process::{Child, ChildStderr, ChildStdin, Command, Stdio};
use std::time::{Duration, Instant};

const MPV_INPUT_CONF: &str = r#"# h96ui: keys while playing (the IR remote will map onto these)
ESC quit
BS quit
q quit
UP playlist-prev
DOWN playlist-next
PGUP playlist-prev
PGDWN playlist-next
LEFT seek -10
RIGHT seek 10
ENTER show-text "${playlist-pos-1}/${playlist-count}  ${media-title}" 3000
SPACE cycle pause
"#;

fn audio_device(a: &str) -> &'static str {
    match a {
        "spdif" => "alsa/plughw:CARD=ROCKCHIPSPDIF,DEV=0",
        "analog" => "alsa/plughw:CARD=rk3528acodec,DEV=0",
        _ => "alsa/plughw:CARD=rockchiphdmi,DEV=0",
    }
}

fn mpv_args(s: &Settings, playlist: &str, index: usize, input_conf: &str) -> Vec<String> {
    let mut a: Vec<String> = [
        "--vo=drm", "--hwdec=no", "--fullscreen",
        "--input-terminal=yes", "--input-default-bindings=no",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    a.push(format!("--input-conf={input_conf}"));
    a.extend(
        ["--term-osd=no", "--msg-level=all=error", "--really-quiet", "--osd-level=1", "--osd-duration=2500", "--cache=yes"]
            .iter()
            .map(|s| s.to_string()),
    );
    a.push(format!("--cache-secs={}", s.cache_secs));
    a.push(format!("--demuxer-readahead-secs={}", s.cache_secs));
    a.push("--network-timeout=20".into());
    a.push(format!("--volume={}", s.volume));
    a.push(format!("--audio-device={}", audio_device(&s.audio)));
    a.push(format!("--playlist={playlist}"));
    a.push(format!("--playlist-start={index}"));
    if s.deinterlace {
        a.push("--deinterlace=yes".into());
    }
    match s.aspect.as_str() {
        "stretch" => a.push("--keepaspect=no".into()),
        "fill" => a.push("--panscan=1.0".into()),
        _ => {}
    }
    a
}

fn which(cmd: &str) -> bool {
    std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).any(|d| d.join(cmd).is_file()))
        .unwrap_or(false)
}

/// Runs mpv in the foreground on our terminal. The caller restores/re-enters raw mode.
/// mpv reads the console keys itself; the remote's Back/Home (evdev only) stop it from here.
pub fn play(a: &mut App, sub: &Subscription, index: usize) -> Result<(), String> {
    let s = a.settings.clone();
    let dir = data_dir();
    let conf = dir.join("mpv-input.conf");
    let _ = std::fs::create_dir_all(&dir);
    std::fs::write(&conf, MPV_INPUT_CONF).map_err(|e| e.to_string())?;
    if !which("mpv") {
        return Err("mpv is not installed (apk add mpv)".into());
    }
    let mut child = Command::new("mpv")
        .args(mpv_args(&s, &sub.path.to_string_lossy(), index, &conf.to_string_lossy()))
        .spawn()
        .map_err(|e| e.to_string())?;
    let st = loop {
        if let Ok(Some(st)) = child.try_wait() {
            break st;
        }
        if let Some(r) = a.remote.as_mut() {
            let fds = r.fds();
            let mut p: Vec<libc::pollfd> =
                fds.iter().map(|&fd| libc::pollfd { fd, events: libc::POLLIN, revents: 0 }).collect();
            if unsafe { libc::poll(p.as_mut_ptr(), p.len() as libc::nfds_t, 200) } > 0 {
                for q in &p {
                    if q.revents != 0 && matches!(r.read(q.fd), Key::Back | Key::MainMenu) {
                        stop(&mut child);
                        return Ok(());
                    }
                }
            }
        } else {
            std::thread::sleep(Duration::from_millis(200));
        }
    };
    if st.success() {
        Ok(())
    } else {
        Err(crate::sys::exit_text(&st))
    }
}

// ---------- hardware engine ----------

/// VOP2 plane for video: Esmart1-win0, an independent layer on VP0 (HDMI). Not Esmart0-win1..3:
/// those share the console's format and scaler (NV12 there shows green garbage).
const PLANE: &str = "172";
const CONNECTOR: &str = "371"; // HDMI-A-1
const PLAYER_LOG: &str = "/tmp/h96ui-player.log"; // copy of the last player's status lines

fn alsa_device(a: &str) -> &'static str {
    match a {
        "spdif" => "plughw:CARD=ROCKCHIPSPDIF,DEV=0",
        "analog" => "plughw:CARD=rk3528acodec,DEV=0",
        _ => "plughw:CARD=rockchiphdmi,DEV=0",
    }
}

/// h96player arguments (tools/video/h96player.c): the channel's HTTP headers from the
/// playlist, the prebuffer ("Network cache"), plane/connector, audio device, volume.
fn player_args(s: &Settings, ch: &Channel) -> Vec<String> {
    let plane = std::env::var("H96UI_PLANE").unwrap_or_else(|_| PLANE.into());
    let conn = std::env::var("H96UI_CONNECTOR").unwrap_or_else(|_| CONNECTOR.into());
    let mut a: Vec<String> = vec![
        "-b".into(), s.cache_secs.max(1).to_string(),
        "-p".into(), plane,
        "-c".into(), conn,
        "-a".into(), alsa_device(&s.audio).into(),
        "-v".into(), format!("{:.2}", s.volume.clamp(0, 100) as f64 / 100.0),
    ];
    if !ch.user_agent.is_empty() {
        a.extend(["-u".into(), ch.user_agent.clone()]);
    }
    if !ch.referrer.is_empty() {
        a.extend(["-r".into(), ch.referrer.clone()]);
    }
    for (k, v) in &ch.headers {
        a.extend(["-H".into(), format!("{k}: {v}")]);
    }
    a.push("--".into());
    a.push(ch.url.clone());
    a
}

// ---------- the player process ----------

#[derive(Default)]
struct Status {
    pos: f64,
    dur: f64, // -1 = unknown / live
    seekable: bool,
    paused: bool,
    playing: bool,  // first frame is up
    buffering: i32, // last buffering percentage
    error: Option<String>,
    ended: bool,
}

/// A running h96player in control mode (-i): commands go to its stdin, status lines come back
/// on its stderr (non-blocking pipe, also copied to PLAYER_LOG for diagnosis).
struct Player {
    child: Child,
    stdin: Option<ChildStdin>,
    stderr: ChildStderr,
    buf: Vec<u8>,
    log: Option<std::fs::File>,
    st: Status,
}

impl Player {
    fn spawn(s: &Settings, ch: &Channel) -> Result<Player, String> {
        let mut args = vec!["-i".to_string()];
        args.extend(player_args(s, ch));
        let mut child = Command::new("h96player")
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("h96player: {e} (install build/video/video-stage.tar.gz)"))?;
        let stdin = child.stdin.take();
        let stderr = child.stderr.take().ok_or("h96player: no stderr pipe")?;
        unsafe {
            let fd = stderr.as_raw_fd();
            libc::fcntl(fd, libc::F_SETFL, libc::fcntl(fd, libc::F_GETFL) | libc::O_NONBLOCK);
        }
        let log = std::fs::File::create(PLAYER_LOG).ok();
        Ok(Player { child, stdin, stderr, buf: Vec::new(), log, st: Status { dur: -1.0, ..Default::default() } })
    }

    fn send(&mut self, cmd: &str) {
        if let Some(w) = self.stdin.as_mut() {
            let _ = writeln!(w, "{cmd}");
            let _ = w.flush();
        }
    }

    /// Reads whatever status lines arrived; true if the status changed.
    fn poll(&mut self) -> bool {
        let mut tmp = [0u8; 4096];
        let mut changed = false;
        loop {
            match self.stderr.read(&mut tmp) {
                Ok(0) => break,
                Ok(n) => self.buf.extend_from_slice(&tmp[..n]),
                Err(_) => break, // WouldBlock
            }
        }
        while let Some(i) = self.buf.iter().position(|&b| b == b'\n') {
            let line: Vec<u8> = self.buf.drain(..=i).collect();
            let line = String::from_utf8_lossy(&line).trim_end().to_string();
            if let Some(l) = self.log.as_mut() {
                let _ = writeln!(l, "{line}");
            }
            changed |= self.parse(&line);
        }
        changed
    }

    fn parse(&mut self, line: &str) -> bool {
        let Some(m) = line.split_once("h96player: ").map(|(_, m)| m) else { return false };
        let st = &mut self.st;
        if let Some(rest) = m.strip_prefix("status ") {
            for kv in rest.split_whitespace() {
                match kv.split_once('=') {
                    Some(("pos", v)) => st.pos = v.parse().unwrap_or(st.pos),
                    Some(("dur", v)) => st.dur = v.parse().unwrap_or(st.dur),
                    Some(("seekable", v)) => st.seekable = v == "1",
                    Some(("paused", v)) => st.paused = v == "1",
                    _ => {}
                }
            }
        } else if m == "playing" {
            st.playing = true;
        } else if let Some(p) = m.strip_prefix("buffering ") {
            st.buffering = p.trim_end_matches('%').parse().unwrap_or(st.buffering);
        } else if let Some(e) = m.strip_prefix("error: ") {
            st.error = Some(e.to_string());
        } else if m == "end of stream" {
            st.ended = true;
        } else {
            return false;
        }
        true
    }

    fn exited(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(Some(_)))
    }

    /// Closing stdin makes h96player stop cleanly (NULL state releases the display plane).
    fn stop(mut self) {
        self.stdin = None;
        let t = Instant::now();
        while t.elapsed() < Duration::from_millis(1500) {
            if let Ok(Some(_)) = self.child.try_wait() {
                return;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        stop(&mut self.child);
    }
}

fn stop(child: &mut Child) {
    unsafe { libc::kill(child.id() as libc::pid_t, libc::SIGTERM) };
    let t = Instant::now();
    while t.elapsed() < Duration::from_millis(1500) {
        if let Ok(Some(_)) = child.try_wait() {
            return;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let _ = child.kill();
    let _ = child.wait();
}

// ---------- console drawing (the OSD lives on the console, around a shrunk video) ----------

/// Display size in pixels (the console framebuffer covers the whole HDMI mode).
fn screen_px() -> (i64, i64) {
    let v = crate::sys::read_first_line("/sys/class/graphics/fb0/virtual_size");
    match v.split_once(',').map(|(w, h)| (w.parse(), h.parse())) {
        Some((Ok(w), Ok(h))) if w > 0 && h > 0 => (w, h),
        _ => (1920, 1080),
    }
}

fn centered(a: &App, row: isize, text: &str, color: &str) -> String {
    let cols = a.cols as isize;
    let t: String = text.chars().take(cols.max(0) as usize).collect();
    let col = ((cols - t.chars().count() as isize) / 2).max(0) + 1;
    format!("\x1b[{};{}H{color}{t}{}", row.max(1), col, crate::C_RESET)
}

/// Full-screen card: shown while a channel loads (the video plane covers it once the first
/// frame is up) and after a stream fails: title, status, key help.
fn card(a: &App, title: &str, status: &str, help: &str) -> String {
    let mid = (a.rows as isize / 2 - 2).max(1);
    let mut b = String::from("\x1b[0m\x1b[2J");
    b += &centered(a, mid, title, "\x1b[1;37m");
    b += &centered(a, mid + 2, status, "\x1b[0;37m");
    b += &centered(a, mid + 5, help, "\x1b[0;36m");
    b
}

fn hms(t: f64) -> String {
    let t = t.max(0.0) as u64;
    if t >= 3600 {
        format!("{}:{:02}:{:02}", t / 3600, t / 60 % 60, t % 60)
    } else {
        format!("{}:{:02}", t / 60, t % 60)
    }
}

/// OSD geometry: the video box (pixels, for h96player's "rect") and the first text row below it.
fn osd_box(a: &App) -> ((i64, i64, i64, i64), isize) {
    let (w, h) = screen_px();
    let (rows, cols) = (a.rows.max(1) as i64, a.cols.max(1) as i64);
    let cell_h = h / rows;
    let _cell_w = w / cols;
    let bw = w * 62 / 100;
    let bh = bw * 9 / 16;
    let (bx, by) = ((w - bw) / 2, cell_h * 2);
    let text_row = ((by + bh) / cell_h.max(1) + 2) as isize;
    ((bx, by, bw, bh), text_row)
}

/// Volume line for the OSD: "Volume 65%  [=============-------]" or "Muted".
fn volume_line(volume: i64, muted: bool) -> String {
    if muted {
        return "Muted   (Mute: sound on)".into();
    }
    let w = 20usize;
    let done = (volume.clamp(0, 100) as usize * w + 50) / 100;
    format!("Volume {:>3}%  [{}{}]", volume, "=".repeat(done), "-".repeat(w - done))
}

/// The OSD text: title, position bar (videos) or LIVE line, volume (after a volume key),
/// key help.
fn osd(a: &App, title: &str, pos_line: &str, st: &Status, vod: bool, vol: Option<&str>) -> String {
    let (_, row) = osd_box(a);
    let mut b = String::from("\x1b[0m\x1b[2J");
    b += &centered(a, row, title, "\x1b[1;37m");
    if let Some(v) = vol {
        b += &centered(a, row + 1, v, "\x1b[1;33m");
    }
    if vod {
        let state = if st.paused { "||  paused" } else { ">   playing" };
        let w = (a.cols as isize - 30).clamp(10, 80) as usize;
        let frac = if st.dur > 0.0 { (st.pos / st.dur).clamp(0.0, 1.0) } else { 0.0 };
        let done = (frac * w as f64).round() as usize;
        let bar = format!("[{}{}]", "=".repeat(done), "-".repeat(w - done.min(w)));
        let dur = if st.dur > 0.0 { hms(st.dur) } else { "--:--".into() };
        b += &centered(a, row + 2, &format!("{}  {bar}  {dur}", hms(st.pos)), "\x1b[0;37m");
        b += &centered(a, row + 3, state, "\x1b[0;33m");
        b += &centered(a, row + 5, "OK: pause/play   Left/Right: -/+10 s   Up/Down: +/-1 min   Vol: volume   Back: menu", "\x1b[0;36m");
    } else {
        b += &centered(a, row + 2, pos_line, "\x1b[0;37m");
        b += &centered(a, row + 5, "Up/Down: channel   OK: info   Vol: volume   Back: menu", "\x1b[0;36m");
    }
    b
}

// ---------- the playback loop ----------

enum Next {
    Item(usize),
    Menu(String), // back to the menu, with a message
}

/// After a stream stopped by itself: keep its reason on screen until the user picks another
/// item (live) or goes back.
fn after_failure(a: &mut App, name: &str, why: &str, index: usize, n: usize, vod: bool) -> Next {
    let help = if vod { "Back: menu" } else { "Up/Down: other channel   Back: menu" };
    write_out(&card(a, name, &format!("Stopped: {why}"), help));
    loop {
        match a.read_key(Duration::from_secs(1)) {
            Key::Up | Key::PgUp if !vod => return Next::Item((index + n - 1) % n),
            Key::Down | Key::PgDn if !vod => return Next::Item((index + 1) % n),
            Key::Back | Key::Left | Key::Quit | Key::MainMenu | Key::Enter => {
                return Next::Menu(format!("{name}: {why}"))
            }
            _ => {}
        }
    }
}

const OSD_SECS: u64 = 4;

/// Plays items[index] on the hardware path and returns the message for the menu.
///   live channels: Up/Down = channel -/+, OK = info for a few seconds, Back = menu
///   videos (vod):  OK = pause/play, Left/Right = -/+10 s, Up/Down = +/-1 min, Back = menu
/// The OSD shrinks the video into a box ("rect") and draws on the console around it.
pub fn play_hw(a: &mut App, items: &[Channel], mut index: usize, vod: bool) -> Result<String, String> {
    let n = items.len();
    if n == 0 {
        return Err("nothing to play".into());
    }
    write_out("\x1b[?25l");
    let mut muted = false; // kept across channel switches; the level lives in a.settings.volume
    loop {
        let ch = &items[index];
        let counter = format!("{} / {}", index + 1, n);
        let help = if vod { "Back: menu" } else { "Up/Down: channel   Back: menu" };
        write_out(&card(a, &ch.name, &format!("Loading ...   {counter}"), help));
        let mut p = Player::spawn(&a.settings, ch)?;
        if muted {
            p.send("mute 1");
        }
        let mut osd_until: Option<Instant> = None; // Some = OSD shown (until, unless paused)
        let mut vol_until: Option<Instant> = None; // volume line visible in the OSD until
        let mut last_buf = -1;
        let next = loop {
            let k = a.read_key(Duration::from_millis(200));
            let changed = p.poll();
            if p.st.ended && vod {
                break Next::Menu(format!("Finished: {}", ch.name));
            }
            if p.st.error.is_some() || p.st.ended || p.exited() {
                p.poll();
                let why = p.st.error.clone().unwrap_or_else(|| "the stream ended".into());
                break after_failure(a, &ch.name, &why, index, n, vod);
            }
            if !p.st.playing && p.st.buffering != last_buf {
                last_buf = p.st.buffering;
                write_out(&card(a, &ch.name, &format!("Buffering {}%   {counter}", p.st.buffering.max(0)), help));
            }
            let mut show_osd = false;
            match k {
                Key::Back | Key::Quit | Key::MainMenu => break Next::Menu(String::new()),
                Key::Left if !vod => break Next::Menu(String::new()),
                Key::Up | Key::PgUp if !vod => break Next::Item((index + n - 1) % n),
                Key::Down | Key::PgDn if !vod => break Next::Item((index + 1) % n),
                Key::Enter if vod && p.st.playing => {
                    p.send("toggle");
                    p.st.paused = !p.st.paused; // status confirms within a second
                    show_osd = true;
                }
                Key::Enter if p.st.playing => show_osd = true,
                Key::VolUp | Key::VolDown | Key::Mute => {
                    // not saved: Settings > Start volume stays the level each session starts at
                    match k {
                        Key::VolUp => { a.settings.volume = (a.settings.volume + 5).min(100); muted = false; }
                        Key::VolDown => { a.settings.volume = (a.settings.volume - 5).max(0); muted = false; }
                        _ => muted = !muted,
                    }
                    p.send(&format!("volume {:.2}", a.settings.volume as f64 / 100.0));
                    p.send(if muted { "mute 1" } else { "mute 0" });
                    vol_until = Some(Instant::now() + Duration::from_secs(OSD_SECS));
                    show_osd = p.st.playing;
                }
                Key::Left | Key::Right | Key::Up | Key::Down if vod && p.st.playing => {
                    let d = match k { Key::Left => -10, Key::Right => 10, Key::Up => 60, _ => -60 };
                    p.send(&format!("seek {d}"));
                    p.st.pos = (p.st.pos + d as f64).clamp(0.0, if p.st.dur > 0.0 { p.st.dur } else { f64::MAX });
                    show_osd = true;
                }
                _ => {}
            }
            if show_osd {
                if osd_until.is_none() {
                    let ((x, y, w, h), _) = osd_box(a);
                    p.send(&format!("rect {x} {y} {w} {h}"));
                }
                osd_until = Some(Instant::now() + Duration::from_secs(OSD_SECS));
            }
            if let Some(t) = osd_until {
                if !p.st.paused && Instant::now() >= t {
                    p.send("full");
                    write_out("\x1b[0m\x1b[2J");
                    osd_until = None;
                } else if show_osd || changed {
                    let vl = vol_until.filter(|t| Instant::now() < *t).map(|_| volume_line(a.settings.volume, muted));
                    write_out(&osd(a, &ch.name, &format!("LIVE   {counter}   {}", ch.group), &p.st, vod, vl.as_deref()));
                }
            }
        };
        p.stop();
        match next {
            Next::Item(i) => index = i,
            Next::Menu(msg) => return Ok(msg),
        }
    }
}
