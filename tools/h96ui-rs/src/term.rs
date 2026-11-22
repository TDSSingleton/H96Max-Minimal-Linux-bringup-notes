//! Minimal terminal handling for the Linux console (tty1): raw mode via termios, size via
//! TIOCGWINSZ, and key decoding. Waiting for input (console + remote) is App::read_key.

use std::io::{self, Read};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    None,
    Up,
    Down,
    Left,
    Right,
    Enter,
    Back, // Esc, Backspace
    PgUp,
    PgDn,
    Home,
    End,
    Quit, // 'q' (keyboard only; the remote has no q)
    // remote buttons the console drops (read from evdev, see remote.rs)
    MainMenu, // the remote's Home button
    Menu,
    LongOk,   // OK held LONG_PRESS (remote.rs): an item's options
    VolUp,
    VolDown,
    Mute,
}

pub struct Term {
    fd: libc::c_int,
    saved: libc::termios,
    raw: bool,
}

impl Term {
    /// Wraps stdin; fails if it is not a terminal.
    pub fn new() -> io::Result<Term> {
        let fd = libc::STDIN_FILENO;
        let mut saved: libc::termios = unsafe { std::mem::zeroed() };
        if unsafe { libc::tcgetattr(fd, &mut saved) } != 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(Term { fd, saved, raw: false })
    }

    pub fn raw(&mut self) -> io::Result<()> {
        let mut r = self.saved;
        r.c_iflag &= !(libc::IGNBRK | libc::BRKINT | libc::PARMRK | libc::ISTRIP
            | libc::INLCR | libc::IGNCR | libc::ICRNL | libc::IXON);
        r.c_lflag &= !(libc::ECHO | libc::ECHONL | libc::ICANON | libc::ISIG | libc::IEXTEN);
        r.c_cflag &= !(libc::CSIZE | libc::PARENB);
        r.c_cflag |= libc::CS8;
        r.c_cc[libc::VMIN] = 1;
        r.c_cc[libc::VTIME] = 0;
        if unsafe { libc::tcsetattr(self.fd, libc::TCSANOW, &r) } != 0 {
            return Err(io::Error::last_os_error());
        }
        self.raw = true;
        Ok(())
    }

    pub fn restore(&mut self) {
        if self.raw {
            unsafe { libc::tcsetattr(self.fd, libc::TCSANOW, &self.saved) };
            self.raw = false;
        }
    }

    /// (rows, cols); falls back to the box's original 90x36 console.
    pub fn size(&self) -> (usize, usize) {
        let mut ws: libc::winsize = unsafe { std::mem::zeroed() };
        if unsafe { libc::ioctl(self.fd, libc::TIOCGWINSZ, &mut ws) } != 0 || ws.ws_row == 0 {
            return (36, 90);
        }
        (ws.ws_row as usize, ws.ws_col as usize)
    }

    pub fn fd(&self) -> libc::c_int {
        self.fd
    }

    /// One read() worth of console input, decoded (call when poll() says it is readable).
    pub fn read_now(&self) -> Key {
        let mut buf = [0u8; 16];
        match io::stdin().lock().read(&mut buf) {
            Ok(n) if n > 0 => decode_key(&buf[..n]),
            _ => Key::None,
        }
    }
}

/// Turns one read() worth of bytes into a key. The Linux console sends each escape sequence in
/// a single write, so a lone ESC in a read is the Esc key itself.
pub fn decode_key(b: &[u8]) -> Key {
    match b {
        b"\x1b[A" | b"\x1bOA" => Key::Up,
        b"\x1b[B" | b"\x1bOB" => Key::Down,
        b"\x1b[C" | b"\x1bOC" => Key::Right,
        b"\x1b[D" | b"\x1bOD" => Key::Left,
        b"\r" | b"\n" | b" " => Key::Enter,
        b"\x1b" | b"\x7f" | b"\x08" => Key::Back,
        b"\x1b[5~" => Key::PgUp,
        b"\x1b[6~" => Key::PgDn,
        b"\x1b[1~" | b"\x1b[H" | b"\x1bOH" => Key::Home,
        b"\x1b[4~" | b"\x1b[F" | b"\x1bOF" => Key::End,
        b"q" | b"Q" => Key::Quit,
        _ => Key::None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys() {
        let cases: &[(&[u8], Key)] = &[
            (b"\x1b[A", Key::Up), (b"\x1b[B", Key::Down), (b"\r", Key::Enter),
            (b"\x1b", Key::Back), (b"\x7f", Key::Back), (b"\x1b[5~", Key::PgUp),
            (b"\x1b[6~", Key::PgDn), (b"x", Key::None),
        ];
        for (inp, want) in cases {
            assert_eq!(decode_key(inp), *want, "{:?}", inp);
        }
    }
}
