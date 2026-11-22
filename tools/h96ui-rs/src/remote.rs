//! Remote buttons the Linux console drops. The BLE remote's arrows reach us as console escape
//! sequences, but OK (KEY_SELECT, 353), Back (KEY_BACK), Home (KEY_HOMEPAGE) and Menu (code 240)
//! do not: the console keymap only covers codes 0..255, and 158/172/240 have no entry in it.
//! So we read those straight from evdev (no double presses: the console never sees them). Devices are picked by capability
//! (any input device that can send KEY_BACK), not by number: the remote reappears as a new
//! eventN after every reconnect, so the list is rescanned every few seconds. No EVIOCGRAB --
//! the console must keep getting the arrows.

use crate::term::Key;
use std::fs::{self, File};
use std::io::Read;
use std::os::fd::AsRawFd;
use std::os::unix::fs::OpenOptionsExt;
use std::path::PathBuf;
use std::time::{Duration, Instant};

const EV_KEY: u16 = 1;
const KEY_MUTE: u16 = 113;
const KEY_VOLUMEDOWN: u16 = 114;
const KEY_VOLUMEUP: u16 = 115;
const KEY_UP: u16 = 103;
const KEY_DOWN: u16 = 108;
const KEY_BACK: u16 = 158;
const KEY_HOMEPAGE: u16 = 172;
const KEY_PLAYPAUSE: u16 = 164;
const KEY_MENU_REMOTE: u16 = 240; // the remote's Menu button (KEY_UNKNOWN in evtest)
const KEY_OK: u16 = 352;
const KEY_SELECT: u16 = 353; // the remote's OK (centre) button

/// struct input_event on 64-bit Linux: struct timeval (16) + type (2) + code (2) + value (4).
const EVENT_SIZE: usize = 24;

/// Holding OK this long is a long press (opens an item's options, e.g. Remove).
pub const LONG_PRESS: Duration = Duration::from_millis(700);

/// Holding Up/Down in a menu scrolls on its own after this long ...
pub const REPEAT_DELAY: Duration = Duration::from_millis(400);
/// ... a step every 120 ms, faster the longer it is held (repeat_interval).
fn repeat_interval(held: Duration) -> Duration {
    match held.as_millis() {
        0..=1499 => Duration::from_millis(120),
        1500..=3499 => Duration::from_millis(60),
        _ => Duration::from_millis(25),
    }
}
/// A release event lost with a dropped BLE link must not scroll forever.
const REPEAT_MAX: Duration = Duration::from_secs(60);

pub struct Remote {
    devs: Vec<(PathBuf, File)>,
    scanned: Option<Instant>,
    ok_down: Option<Instant>, // OK is held since
    ok_long_sent: bool,       // ... and its long press already fired
    arrow: Option<(Key, Instant)>, // Up/Down held since (the console sends the first step)
    next_repeat: Option<Instant>,  // when a held arrow makes its next step
}

/// Does a /sys/class/input/eventN/device/capabilities/key bitmap include `code`?
/// The file is hex words, most significant first, each one `long` (64 bits here).
fn has_key(bitmap: &str, code: u16) -> bool {
    let words: Vec<&str> = bitmap.split_whitespace().collect();
    let (w, bit) = (code as usize / 64, code as usize % 64);
    if w >= words.len() {
        return false;
    }
    let word = words[words.len() - 1 - w];
    u64::from_str_radix(word, 16).map(|v| v >> bit & 1 == 1).unwrap_or(false)
}

fn map(code: u16) -> Key {
    match code {
        KEY_PLAYPAUSE => Key::Enter, // OK (KEY_SELECT/KEY_OK) is timed in event()
        KEY_BACK => Key::Back,
        KEY_HOMEPAGE => Key::MainMenu,
        KEY_MENU_REMOTE => Key::Menu,
        KEY_VOLUMEUP => Key::VolUp,
        KEY_VOLUMEDOWN => Key::VolDown,
        KEY_MUTE => Key::Mute,
        _ => Key::None, // arrows arrive through the console
    }
}

impl Remote {
    pub fn new() -> Remote {
        let mut r = Remote { devs: Vec::new(), scanned: None, ok_down: None, ok_long_sent: false, arrow: None, next_repeat: None };
        r.rescan();
        r
    }

    fn rescan(&mut self) {
        self.scanned = Some(Instant::now());
        let Ok(ents) = fs::read_dir("/sys/class/input") else { return };
        for e in ents.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if !name.starts_with("event") {
                continue;
            }
            let dev = PathBuf::from("/dev/input").join(&name);
            if self.devs.iter().any(|(p, _)| *p == dev) {
                continue;
            }
            let caps = fs::read_to_string(e.path().join("device/capabilities/key")).unwrap_or_default();
            if !has_key(&caps, KEY_BACK) {
                continue;
            }
            if let Ok(f) = fs::OpenOptions::new().read(true).custom_flags(libc::O_NONBLOCK).open(&dev) {
                self.devs.push((dev, f));
            }
        }
    }

    /// File descriptors to poll alongside stdin (rescans every 3 s for a reconnected remote).
    pub fn fds(&mut self) -> Vec<libc::c_int> {
        if self.scanned.is_none_or(|t| t.elapsed() > Duration::from_secs(3)) {
            self.rescan();
        }
        self.devs.iter().map(|(_, f)| f.as_raw_fd()).collect()
    }

    /// One key event -> a key, with OK's short/long press decided on its timing:
    /// press = nothing yet; release before LONG_PRESS = Enter; held LONG_PRESS = LongOk (fired
    /// while still held, from autorepeat or from tick()); the release after a long press = nothing.
    fn event(&mut self, code: u16, value: i32) -> Key {
        if code == KEY_SELECT || code == KEY_OK {
            let now = Instant::now();
            return match value {
                1 => {
                    self.ok_down = Some(now);
                    self.ok_long_sent = false;
                    Key::None
                }
                2 => self.tick(false),
                _ => {
                    let held = self.ok_down.take().map(|t| now.duration_since(t));
                    match held {
                        _ if self.ok_long_sent => Key::None,
                        Some(h) if h >= LONG_PRESS => Key::LongOk,
                        Some(_) => Key::Enter,
                        None => Key::None,
                    }
                }
            };
        }
        if code == KEY_UP || code == KEY_DOWN {
            // timed only: the step itself arrives through the console
            let k = if code == KEY_UP { Key::Up } else { Key::Down };
            match value {
                1 => {
                    let now = Instant::now();
                    self.arrow = Some((k, now));
                    self.next_repeat = Some(now + REPEAT_DELAY);
                }
                0 => self.release_arrow(),
                _ => {}
            }
            return Key::None;
        }
        if value == 1 { map(code) } else { Key::None }
    }

    fn release_arrow(&mut self) {
        self.arrow = None;
        self.next_repeat = None;
    }

    /// The next step of a held Up/Down (`repeat`: only menus scroll by holding).
    fn arrow_tick(&mut self, repeat: bool) -> Key {
        let (Some((k, since)), Some(next), true) = (self.arrow, self.next_repeat, repeat) else { return Key::None };
        let now = Instant::now();
        if now.duration_since(since) > REPEAT_MAX {
            self.release_arrow();
            return Key::None;
        }
        if now < next {
            return Key::None;
        }
        self.next_repeat = Some(now + repeat_interval(now.duration_since(since)));
        k
    }

    /// True for a console arrow that only repeats a held remote arrow (kernel autorepeat):
    /// while held, the steps come from arrow_tick() instead.
    pub fn console_repeat(&self, k: Key) -> bool {
        matches!(self.arrow, Some((a, since)) if a == k && since.elapsed() >= Duration::from_millis(150))
    }

    /// LongOk once OK has been held LONG_PRESS, or the next step of a held arrow when
    /// `repeat` (call when a wait times out).
    pub fn tick(&mut self, repeat: bool) -> Key {
        match self.ok_down {
            Some(t) if !self.ok_long_sent && t.elapsed() >= LONG_PRESS => {
                self.ok_long_sent = true;
                Key::LongOk
            }
            _ => self.arrow_tick(repeat),
        }
    }

    /// How long until a held OK becomes a long press or a held arrow steps (None: nothing held).
    pub fn due(&self, repeat: bool) -> Option<Duration> {
        let ok = match self.ok_down {
            Some(t) if !self.ok_long_sent => Some(LONG_PRESS.saturating_sub(t.elapsed())),
            _ => None,
        };
        let arrow = self.next_repeat.filter(|_| repeat).map(|t| t.saturating_duration_since(Instant::now()));
        match (ok, arrow) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        }
    }

    /// Drains pending events from device `fd`; returns the first key they make.
    pub fn read(&mut self, fd: libc::c_int) -> Key {
        let Some(i) = self.devs.iter().position(|(_, f)| f.as_raw_fd() == fd) else { return Key::None };
        let mut key = Key::None;
        let mut buf = [0u8; EVENT_SIZE * 16];
        loop {
            match self.devs[i].1.read(&mut buf) {
                Ok(n) if n >= EVENT_SIZE => {
                    for ev in buf[..n - n % EVENT_SIZE].chunks_exact(EVENT_SIZE) {
                        let typ = u16::from_ne_bytes([ev[16], ev[17]]);
                        let code = u16::from_ne_bytes([ev[18], ev[19]]);
                        let value = i32::from_ne_bytes([ev[20], ev[21], ev[22], ev[23]]);
                        if typ == EV_KEY {
                            let k = self.event(code, value);
                            if key == Key::None {
                                key = k;
                            }
                        }
                    }
                }
                Ok(_) => break,
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(_) => {
                    // ENODEV: the remote disconnected; its eventN is gone
                    self.devs.remove(i);
                    self.release_arrow();
                    break;
                }
            }
        }
        key
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn remote() -> Remote {
        Remote { devs: Vec::new(), scanned: None, ok_down: None, ok_long_sent: false, arrow: None, next_repeat: None }
    }

    #[test]
    fn held_arrow_repeats() {
        let mut r = remote();
        assert_eq!(r.event(KEY_DOWN, 1), Key::None); // the console delivers the first step
        assert_eq!(r.tick(true), Key::None); // not held long enough yet
        assert!(!r.console_repeat(Key::Down)); // the console's own first step is kept
        r.arrow = Some((Key::Down, Instant::now() - REPEAT_DELAY));
        r.next_repeat = Some(Instant::now());
        assert!(r.console_repeat(Key::Down)); // kernel autorepeat copies are dropped
        assert!(!r.console_repeat(Key::Up));
        assert_eq!(r.tick(false), Key::None); // the player: no scrolling by holding
        assert_eq!(r.tick(true), Key::Down);
        assert_eq!(r.tick(true), Key::None); // next step only after the interval
        assert!(r.due(true).is_some_and(|d| d <= Duration::from_millis(120)));
        assert_eq!(r.event(KEY_DOWN, 0), Key::None);
        assert_eq!(r.tick(true), Key::None);
        assert!(r.due(true).is_none());
        r.arrow = Some((Key::Up, Instant::now() - REPEAT_MAX - Duration::from_secs(1)));
        r.next_repeat = Some(Instant::now());
        assert_eq!(r.tick(true), Key::None); // a lost release stops after REPEAT_MAX
        assert!(r.arrow.is_none());
        assert!(repeat_interval(Duration::from_secs(5)) < repeat_interval(Duration::ZERO));
    }

    #[test]
    fn ok_short_and_long() {
        let mut r = remote();
        assert_eq!(r.event(KEY_SELECT, 1), Key::None); // press: wait for the release
        assert_eq!(r.event(KEY_SELECT, 0), Key::Enter); // quick release = OK
        assert_eq!(r.event(KEY_SELECT, 1), Key::None);
        r.ok_down = Some(Instant::now() - LONG_PRESS); // held long enough
        assert_eq!(r.event(KEY_SELECT, 2), Key::LongOk); // fires while held (autorepeat)
        assert_eq!(r.event(KEY_SELECT, 2), Key::None); // only once
        assert_eq!(r.event(KEY_SELECT, 0), Key::None); // the release adds nothing
        assert_eq!(r.event(KEY_SELECT, 1), Key::None);
        r.ok_down = Some(Instant::now() - LONG_PRESS);
        assert_eq!(r.tick(false), Key::LongOk); // no autorepeat: the timer fires it
        assert_eq!(r.event(KEY_SELECT, 0), Key::None);
        assert_eq!(r.event(KEY_BACK, 1), Key::Back);
        assert_eq!(r.event(KEY_BACK, 0), Key::None);
    }

    #[test]
    fn bitmap() {
        // KEY_BACK = 158 = word 2 (from the right), bit 30
        let caps = format!("{:x} 0 0", 1u64 << 30);
        assert!(has_key(&caps, KEY_BACK));
        assert!(!has_key(&caps, KEY_HOMEPAGE));
        assert!(!has_key("ff", KEY_BACK));
        assert!(!has_key("", KEY_BACK));
    }
}
