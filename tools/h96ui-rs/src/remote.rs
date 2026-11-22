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
const KEY_BACK: u16 = 158;
const KEY_HOMEPAGE: u16 = 172;
const KEY_PLAYPAUSE: u16 = 164;
const KEY_MENU_REMOTE: u16 = 240; // the remote's Menu button (KEY_UNKNOWN in evtest)
const KEY_OK: u16 = 352;
const KEY_SELECT: u16 = 353; // the remote's OK (centre) button

/// struct input_event on 64-bit Linux: struct timeval (16) + type (2) + code (2) + value (4).
const EVENT_SIZE: usize = 24;

pub struct Remote {
    devs: Vec<(PathBuf, File)>,
    scanned: Option<Instant>,
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
        KEY_SELECT | KEY_OK | KEY_PLAYPAUSE => Key::Enter,
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
        let mut r = Remote { devs: Vec::new(), scanned: None };
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

    /// Drains pending events from device `fd`; returns the first mapped key press.
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
                        if typ == EV_KEY && value == 1 && key == Key::None {
                            key = map(code);
                        }
                    }
                }
                Ok(_) => break,
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(_) => {
                    // ENODEV: the remote disconnected; its eventN is gone
                    self.devs.remove(i);
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
