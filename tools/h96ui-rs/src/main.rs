//! h96ui: the H96Max console interface on tty1 (HDMI). Lists IPTV subscriptions, imports them
//! from a USB stick (no typing), plays channels, and keeps playback settings.
//! Driven by the Bluetooth remote: arrows/OK arrive on the console, Back/Home via evdev
//! (remote.rs); a keyboard works too (Esc/Backspace = Back).

mod m3u;
mod play;
mod remote;
mod screens;
mod store;
mod sys;
mod term;

use std::io::Write;
use std::rc::Rc;
use std::time::{Duration, Instant};
use store::Settings;
use sys::WifiState;
use term::{Key, Term};

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
pub const BUILD_DATE: &str = match option_env!("H96UI_BUILD_DATE") {
    Some(d) => d,
    None => "dev",
};

pub type Action = Rc<dyn Fn(&mut App)>;
pub type Refresh = Rc<dyn Fn(&mut App, &mut Screen)>;

pub struct Item {
    pub label: String,
    pub right: String,
    pub action: Option<Action>,
}

impl Item {
    pub fn new(label: impl Into<String>, right: impl Into<String>, f: impl Fn(&mut App) + 'static) -> Item {
        Item { label: label.into(), right: right.into(), action: Some(Rc::new(f)) }
    }
}

#[derive(Default)]
pub struct Screen {
    pub title: String,
    pub items: Vec<Item>,
    pub sel: usize,
    pub top: usize,
    pub empty: String, // shown when there are no items
    pub lines: Vec<String>,
    pub on_back: Option<Action>,
    pub refresh: Option<Refresh>, // rebuild items when the screen is shown again
    pub usb_live: bool,           // refresh when a USB stick is plugged in or pulled out
}

impl Screen {
    pub fn titled(t: impl Into<String>) -> Screen {
        Screen { title: t.into(), ..Default::default() }
    }
}

pub struct App {
    term: Option<Term>,
    rows: usize,
    cols: usize,
    stack: Vec<Screen>,
    msg: String,
    msg_until: Option<Instant>,
    wifi: WifiState,
    wifi_at: Option<Instant>,
    pub settings: Settings,
    quit_ok: bool,
    snapshot: bool,
    usb: Vec<sys::UsbDisk>,
    remote: Option<remote::Remote>,
}

impl App {
    fn top(&mut self) -> &mut Screen {
        self.stack.last_mut().expect("screen stack is never empty")
    }

    fn run_refresh(&mut self, s: &mut Screen) {
        if let Some(r) = s.refresh.clone() {
            r(self, s);
        }
    }

    pub fn push(&mut self, mut s: Screen) {
        self.run_refresh(&mut s);
        self.stack.push(s);
    }

    /// Re-runs the top screen's refresh (after a setting changed, a retry, ...).
    pub fn refresh_top(&mut self) {
        if let Some(mut s) = self.stack.pop() {
            self.run_refresh(&mut s);
            self.stack.push(s);
        }
    }

    pub fn pop(&mut self) {
        if self.stack.len() <= 1 {
            return;
        }
        if let Some(f) = self.top().on_back.clone() {
            f(self);
        }
        self.stack.pop();
        self.refresh_top();
        let s = self.top();
        if s.sel >= s.items.len() {
            s.sel = s.items.len().saturating_sub(1);
        }
    }

    pub fn say(&mut self, msg: impl Into<String>) {
        self.msg = msg.into();
        self.msg_until = Some(Instant::now() + Duration::from_secs(6));
    }

    fn msg_active(&self, slack: Duration) -> bool {
        self.msg_until.is_some_and(|t| Instant::now() < t + slack)
    }

    // ---------- rendering ----------

    fn header(&mut self) -> String {
        if self.wifi_at.is_none_or(|t| t.elapsed() > Duration::from_secs(5)) {
            self.wifi = sys::read_wifi();
            self.wifi_at = Some(Instant::now());
        }
        let mut left = "  WiFi: not connected".to_string();
        if !self.wifi.ssid.is_empty() {
            left = format!("  WiFi: {}  {}  {}", self.wifi.ssid, self.wifi.signal, self.wifi.ip);
        }
        let right = sys::local_time("%a %d %b  %H:%M") + "  ";
        let mut gap = self.cols as isize - width(&left) - width(&right);
        if gap < 1 {
            left = fit(&left, self.cols as isize - width(&right) - 1);
            gap = 1;
        }
        format!("{C_BAR}{left}{}{right}{C_RESET}", " ".repeat(gap as usize))
    }

    fn footer(&self) -> String {
        let left = format!(
            "  h96ui {VERSION} ({BUILD_DATE})  |  Alpine {}  |  Linux {}",
            sys::alpine_release(),
            sys::kernel_release()
        );
        let right = "Up/Down  OK  Back  ";
        let gap = self.cols as isize - width(&left) - width(right);
        if gap < 1 {
            return format!("{C_FOOTER}{}{C_RESET}", fit(&left, self.cols as isize));
        }
        format!("{C_FOOTER}{left}{}{right}{C_RESET}", " ".repeat(gap as usize))
    }

    fn render(&mut self) -> String {
        let (rows, cols) = (self.rows as isize, self.cols as isize);
        let header = self.header();
        let footer = self.footer();
        let crumbs: Vec<&str> = self.stack.iter().map(|s| s.title.as_str()).collect();
        let crumbs = crumbs.join("  >  ");
        let msg_on = self.msg_active(Duration::ZERO);
        let msg_text = self.msg.clone();

        let mut b = String::from("\x1b[H");
        let mut line = |t: &str| {
            b.push_str(t);
            b.push_str("\x1b[K\r\n");
        };
        line(&header);
        line("");
        line(&format!("{C_TITLE}  {}{C_RESET}", fit(&crumbs, cols - 4)));
        line(&format!("{C_DIM}  {}{C_RESET}", "-".repeat((cols - 4).max(0) as usize)));

        let s = self.stack.last_mut().expect("screen stack is never empty");
        let body = rows - 4 - 3; // header+blank+title+rule, message+blank+footer
        let mut used: isize = 0;
        for l in &s.lines {
            if used >= body {
                break;
            }
            line(&format!("  {}", fit(l, cols - 4)));
            used += 1;
        }
        if s.items.is_empty() && s.lines.is_empty() && used < body {
            line(&format!("{C_DIM}  {}{C_RESET}", fit(&s.empty, cols - 4)));
            used += 1;
        }
        let list_rows = body - used;
        if list_rows > 0 && !s.items.is_empty() {
            let lr = list_rows as usize;
            if s.sel < s.top {
                s.top = s.sel;
            }
            if s.sel >= s.top + lr {
                s.top = s.sel + 1 - lr;
            }
            let w = cols - 8;
            for i in s.top..s.items.len().min(s.top + lr) {
                let it = &s.items[i];
                let lw = w - width(&it.right) - 2;
                let txt = format!("{}  {}", fit(&it.label, lw), it.right);
                if i == s.sel {
                    line(&format!("  {C_SEL}> {} {C_RESET}", fit(&txt, w)));
                } else {
                    line(&format!("    {}", fit(&txt, w)));
                }
                used += 1;
            }
        }
        while used < body {
            line("");
            used += 1;
        }
        let msg = if msg_on {
            msg_text
        } else if !s.items.is_empty() {
            format!("{} / {}", s.sel + 1, s.items.len())
        } else {
            String::new()
        };
        line(&format!("{C_MSG}  {}{C_RESET}", fit(&msg, cols - 4)));
        line("");
        b.push_str(&footer);
        b.push_str("\x1b[K");
        b
    }

    /// Clears the console and draws the current screen (after the player used it).
    pub fn redraw_all(&mut self) {
        write_out("\x1b[0m\x1b[2J\x1b[?25l");
        self.draw();
    }

    pub fn draw(&mut self) {
        if self.snapshot {
            return;
        }
        // the console can change size under us (h96-console sets 1080p + a 16x32 font at boot)
        let mut out = String::new();
        if let Some(t) = &self.term {
            let (r, c) = t.size();
            if (r, c) != (self.rows, self.cols) {
                (self.rows, self.cols) = (r, c);
                out.push_str("\x1b[2J");
            }
        }
        out.push_str(&self.render());
        write_out(&out);
    }

    /// Waits up to `timeout` for a key from the console (arrows, OK, keyboard) or the remote's
    /// evdev device (Back, Home, ...). Key::None on timeout.
    pub fn read_key(&mut self, timeout: Duration) -> Key {
        let Some(t) = self.term.as_ref() else { return Key::None };
        let mut fds = vec![libc::pollfd { fd: t.fd(), events: libc::POLLIN, revents: 0 }];
        if let Some(r) = self.remote.as_mut() {
            fds.extend(r.fds().into_iter().map(|fd| libc::pollfd { fd, events: libc::POLLIN, revents: 0 }));
        }
        let deadline = Instant::now() + timeout;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            let ms = left.as_millis().min(i32::MAX as u128) as libc::c_int;
            if unsafe { libc::poll(fds.as_mut_ptr(), fds.len() as libc::nfds_t, ms) } <= 0 {
                return Key::None;
            }
            for p in &fds {
                if p.revents == 0 {
                    continue;
                }
                let k = if p.fd == t.fd() {
                    t.read_now()
                } else {
                    self.remote.as_mut().map_or(Key::None, |r| r.read(p.fd))
                };
                if k != Key::None {
                    return k;
                }
            }
            if left.is_zero() {
                return Key::None;
            }
            // a device went away (POLLHUP/POLLERR): drop it from this wait
            fds.retain(|p| p.revents & (libc::POLLHUP | libc::POLLERR | libc::POLLNVAL) == 0);
        }
    }

    /// Back to the main menu (the remote's Home button).
    fn go_home(&mut self) {
        while self.stack.len() > 1 {
            self.pop();
        }
    }

    /// Called once per idle tick: reacts to a USB stick being plugged in or pulled out.
    fn poll_usb(&mut self) -> bool {
        let now = sys::usb_disks();
        if now == self.usb {
            return false;
        }
        let added = now.iter().any(|d| !self.usb.iter().any(|o| o.disk == d.disk));
        self.usb = now;
        if self.top().usb_live {
            self.refresh_top();
            self.say(if added { "USB stick detected" } else { "USB stick removed" });
        } else if added {
            self.say("USB stick detected: open 'Add subscription from USB' to import playlists");
        } else if self.stack.iter().all(|s| !s.usb_live) {
            sys::unmount_usb(); // nothing should hold it, but never leave a dead mount behind
        }
        true
    }

    // ---------- input ----------

    fn handle(&mut self, k: Key) -> bool {
        let page = (self.rows as isize - 10).max(1) as usize;
        let s = self.top();
        let n = s.items.len();
        match k {
            Key::Up if n > 0 => s.sel = (s.sel + n - 1) % n,
            Key::Down if n > 0 => s.sel = (s.sel + 1) % n,
            Key::PgUp => s.sel = s.sel.saturating_sub(page),
            Key::PgDn => s.sel = (s.sel + page).min(n.saturating_sub(1)),
            Key::Home => s.sel = 0,
            Key::End => s.sel = n.saturating_sub(1),
            Key::Enter | Key::Right => {
                if let Some(f) = s.items.get(s.sel).and_then(|it| it.action.clone()) {
                    f(self);
                }
            }
            Key::Back | Key::Left => self.pop(),
            Key::MainMenu => self.go_home(),
            Key::Quit => return !self.quit_ok,
            _ => {}
        }
        true
    }

    /// Hands the terminal to another program (the player) and takes it back.
    pub fn run_external<T>(&mut self, f: impl FnOnce(&mut App) -> T) -> T {
        if let Some(t) = self.term.as_mut() {
            t.restore();
        }
        write_out("\x1b[0m\x1b[2J\x1b[H\x1b[?25h");
        let r = f(self);
        if let Some(t) = self.term.as_mut() {
            let _ = t.raw();
        }
        write_out("\x1b[?25l\x1b[2J");
        r
    }
}

// ---------- text helpers ----------

pub const C_RESET: &str = "\x1b[0m";
const C_BAR: &str = "\x1b[0;30;46m"; // black on cyan
const C_TITLE: &str = "\x1b[1;37m";
const C_SEL: &str = "\x1b[0;30;47m"; // black on white
const C_DIM: &str = "\x1b[0;37m";
const C_MSG: &str = "\x1b[1;33m";
const C_FOOTER: &str = "\x1b[0;30;46m";

fn width(s: &str) -> isize {
    s.chars().count() as isize
}

/// Pads or cuts `s` to exactly `w` characters ("~" marks a cut).
pub fn fit(s: &str, w: isize) -> String {
    if w <= 0 {
        return String::new();
    }
    let w = w as usize;
    let n = s.chars().count();
    if n <= w {
        return format!("{s}{}", " ".repeat(w - n));
    }
    if w <= 1 {
        return s.chars().take(w).collect();
    }
    let mut o: String = s.chars().take(w - 1).collect();
    o.push('~');
    o
}

fn write_out(s: &str) {
    let mut o = std::io::stdout().lock();
    let _ = o.write_all(s.as_bytes());
    let _ = o.flush();
}

// ---------- main ----------

struct Args {
    snapshot: Option<String>,
    allow_quit: bool,
    version: bool,
}

/// Accepts -flag, --flag, -flag=value and -flag value (like Go's flag package did).
fn parse_args() -> Result<Args, String> {
    let mut a = Args { snapshot: None, allow_quit: false, version: false };
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        let name = arg.trim_start_matches('-');
        let (name, val) = match name.split_once('=') {
            Some((n, v)) => (n, Some(v.to_string())),
            None => (name, None),
        };
        match name {
            "snapshot" => a.snapshot = Some(val.or_else(|| it.next()).ok_or("-snapshot needs a value")?),
            "allow-quit" => a.allow_quit = true,
            "version" => a.version = true,
            "h" | "help" => {
                return Err("usage: h96ui [-snapshot main|subs|channels|settings|about|usb] [-allow-quit] [-version]".into())
            }
            _ => return Err(format!("unknown flag: {arg}")),
        }
    }
    Ok(a)
}

/// busybox init starts us with PATH=/sbin:/usr/sbin:/bin:/usr/bin; our own tools (h96player)
/// live in /usr/local/bin, so put it first for every program we start.
fn ensure_path() {
    let cur = std::env::var("PATH").unwrap_or_default();
    if !cur.split(':').any(|d| d == "/usr/local/bin") {
        let p = if cur.is_empty() { "/usr/local/bin:/usr/bin:/bin".to_string() } else { format!("/usr/local/bin:{cur}") };
        std::env::set_var("PATH", p);
    }
}

fn main() {
    ensure_path();
    let args = match parse_args() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(2);
        }
    };
    if args.version {
        println!("h96ui {VERSION} ({BUILD_DATE})");
        return;
    }
    let mut a = App {
        term: None,
        rows: 36,
        cols: 90,
        stack: Vec::new(),
        msg: String::new(),
        msg_until: None,
        wifi: WifiState::default(),
        wifi_at: None,
        settings: Settings::load(),
        quit_ok: args.allow_quit,
        snapshot: false,
        usb: sys::usb_disks(),
        remote: None,
    };
    let m = screens::main_menu();
    a.push(m);

    if let Some(name) = args.snapshot {
        a.snapshot = true;
        screens::go_snapshot(&mut a, &name);
        let out = a.render();
        print!("{out}\n");
        return;
    }

    let mut t = match Term::new() {
        Ok(t) => t,
        Err(e) => {
            eprintln!("h96ui: not a terminal: {e}");
            std::process::exit(1);
        }
    };
    (a.rows, a.cols) = t.size();
    if let Err(e) = t.raw() {
        eprintln!("h96ui: raw mode: {e}");
        std::process::exit(1);
    }
    a.term = Some(t);
    a.remote = Some(remote::Remote::new());
    // console: never blank, hide the cursor
    write_out("\x1b[9;0]\x1b[14;0]\x1b[?25l\x1b[2J");
    let mut last_min = -1;
    a.draw();
    loop {
        let k = a.read_key(Duration::from_secs(1));
        if k != Key::None {
            if !a.handle(k) {
                break;
            }
            a.draw();
            continue;
        }
        // idle tick: USB hotplug, then redraw for the clock / WiFi / message expiry
        if a.poll_usb() {
            a.draw();
            continue;
        }
        let m = sys::local_minute();
        let wifi_old = a.wifi_at.is_none_or(|t| t.elapsed() > Duration::from_secs(5));
        if m != last_min || a.msg_active(Duration::from_secs(1)) || wifi_old {
            last_min = m;
            a.draw();
        }
    }
    if let Some(t) = a.term.as_mut() {
        t.restore();
    }
    write_out("\x1b[0m\x1b[2J\x1b[H\x1b[?25h");
}
