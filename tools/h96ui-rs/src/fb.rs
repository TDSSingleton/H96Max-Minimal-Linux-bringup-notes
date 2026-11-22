//! EBI's own text renderer on the framebuffer. The kernel console holds at most 512 glyphs
//! (Terminus: Latin, Greek, Cyrillic), so Arabic, Hebrew, Japanese, Chinese, Korean names came
//! out as garbage. Here tty1 is put into KD_GRAPHICS (fbcon stops drawing; the keyboard and
//! the remote still reach us through the tty), and the text EBI already writes for the console
//! (a small ANSI subset: SGR colours, CUP, EL, ED, CR/LF) is interpreted into a cell grid and
//! drawn with GNU Unifont (unifont.hex: 8x16 / 16x16 bitmaps for the whole BMP), doubled to
//! 16x32 cells = the same 120x33 grid as the console font. Only changed rows are redrawn.
//! The video plane sits above this layer exactly as it sat above fbcon.
//! No font or no 32-bit framebuffer: EBI keeps writing to the console as before.

use crate::text::{char_width, visual};
use std::collections::HashMap;
use std::fs;
use std::os::fd::AsRawFd;
use std::sync::atomic::{AtomicI32, Ordering};

pub const FONT_PATHS: &[&str] = &["/usr/share/unifont/unifont.hex", "/usr/local/share/h96/unifont.hex"];
const CW: usize = 16; // cell size in pixels (glyphs are 8x16 / 16x16, doubled)
const CH: usize = 32;

const KDSETMODE: libc::c_ulong = 0x4b3a;
const KD_TEXT: libc::c_int = 0;
const KD_GRAPHICS: libc::c_int = 1;

/// The tty in KD_GRAPHICS (for the signal handler); -1 = none.
static GRAPHICS_TTY: AtomicI32 = AtomicI32::new(-1);

/// Linux console palette (0-7 normal, 8-15 bright).
const PALETTE: [u32; 16] = [
    0x000000, 0xaa0000, 0x00aa00, 0xaa5500, 0x0000aa, 0xaa00aa, 0x00aaaa, 0xaaaaaa,
    0x555555, 0xff5555, 0x55ff55, 0xffff55, 0x5555ff, 0xff55ff, 0x55ffff, 0xffffff,
];

pub struct Font {
    glyphs: HashMap<u32, (u8, Vec<u8>)>, // codepoint -> (width px 8|16, 16 rows of bits)
}

impl Font {
    pub fn load() -> Option<Font> {
        let path = std::env::var("H96UI_FONT").ok().filter(|p| !p.is_empty());
        let text = match path {
            Some(p) => fs::read_to_string(p).ok()?,
            None => FONT_PATHS.iter().find_map(|p| fs::read_to_string(p).ok())?,
        };
        Font::parse(&text)
    }

    /// unifont.hex: "XXXX:<32 hex digits = 8x16>" or "XXXX:<64 hex digits = 16x16>" per line.
    pub fn parse(text: &str) -> Option<Font> {
        let mut glyphs = HashMap::with_capacity(60000);
        for l in text.lines() {
            let Some((cp, hex)) = l.split_once(':') else { continue };
            let Ok(cp) = u32::from_str_radix(cp.trim(), 16) else { continue };
            let hex = hex.trim();
            let w = match hex.len() {
                32 => 8,
                64 => 16,
                _ => continue,
            };
            let bytes: Option<Vec<u8>> =
                (0..hex.len()).step_by(2).map(|i| u8::from_str_radix(&hex[i..i + 2], 16).ok()).collect();
            if let Some(b) = bytes {
                glyphs.insert(cp, (w, b));
            }
        }
        (!glyphs.is_empty()).then_some(Font { glyphs })
    }

    fn get(&self, c: char) -> Option<&(u8, Vec<u8>)> {
        self.glyphs.get(&(c as u32)).or_else(|| self.glyphs.get(&0xfffd))
    }
}

#[derive(Clone, Copy, PartialEq)]
struct Cell {
    ch: char,
    fg: u8,
    bg: u8,
    cont: bool, // right half of a wide character
}

const BLANK: Cell = Cell { ch: ' ', fg: 7, bg: 0, cont: false };

pub struct Fb {
    tty: libc::c_int,
    map: *mut u8,
    len: usize,
    w: usize,
    h: usize,
    stride: usize,
    pub rows: usize,
    pub cols: usize,
    grid: Vec<Cell>,
    shown: Vec<Option<Cell>>, // what is on the screen (None: unknown -> redraw)
    r: usize,
    c: usize,
    fg: u8,
    bg: u8,
    bold: bool,
    font: Font,
    pending: String, // an escape sequence split across writes
    size_key: String, // fb0 virtual_size when mapped
}

fn sysfs(p: &str) -> String {
    crate::sys::read_first_line(p)
}

impl Fb {
    /// Maps /dev/fb0 and switches `tty` to KD_GRAPHICS. None (console stays in charge) when
    /// there is no font, no 32 bpp framebuffer, or the ioctl/mmap fails.
    pub fn open(tty: libc::c_int) -> Option<Fb> {
        if std::env::var("H96UI_CONSOLE").is_ok_and(|v| v == "1") {
            return None; // escape hatch: the plain console renderer
        }
        let font = Font::load()?;
        Fb::open_with(tty, font)
    }

    fn open_with(tty: libc::c_int, font: Font) -> Option<Fb> {
        if sysfs("/sys/class/graphics/fb0/bits_per_pixel") != "32" {
            return None;
        }
        let size_key = sysfs("/sys/class/graphics/fb0/virtual_size");
        let (w, h) = size_key.split_once(',').and_then(|(a, b)| Some((a.parse().ok()?, b.parse().ok()?)))?;
        let stride: usize = sysfs("/sys/class/graphics/fb0/stride").parse().ok().filter(|s| *s >= w * 4)?;
        let f = fs::OpenOptions::new().read(true).write(true).open("/dev/fb0").ok()?;
        let len = stride * h;
        let map = unsafe {
            libc::mmap(std::ptr::null_mut(), len, libc::PROT_READ | libc::PROT_WRITE, libc::MAP_SHARED, f.as_raw_fd(), 0)
        };
        if map == libc::MAP_FAILED {
            return None;
        }
        if unsafe { libc::ioctl(tty, KDSETMODE as _, KD_GRAPHICS) } != 0 {
            unsafe { libc::munmap(map, len) };
            return None;
        }
        GRAPHICS_TTY.store(tty, Ordering::SeqCst);
        install_signal_handlers();
        let (rows, cols) = (h / CH, w / CW);
        Some(Fb {
            tty,
            map: map as *mut u8,
            len,
            w,
            h,
            stride,
            rows,
            cols,
            grid: vec![BLANK; rows * cols],
            shown: vec![None; rows * cols],
            r: 0,
            c: 0,
            fg: 7,
            bg: 0,
            bold: false,
            font,
            pending: String::new(),
            size_key,
        })
    }

    /// True if the HDMI mode (fb0 size) changed since the framebuffer was mapped.
    pub fn stale(&self) -> bool {
        sysfs("/sys/class/graphics/fb0/virtual_size") != self.size_key
    }

    /// Maps fb0 again after a mode change (new size: new grid, everything redrawn).
    pub fn remap(&mut self) -> bool {
        let key = sysfs("/sys/class/graphics/fb0/virtual_size");
        let Some((w, h)) = key.split_once(',').and_then(|(a, b)| Some((a.parse::<usize>().ok()?, b.parse::<usize>().ok()?))) else { return false };
        let Some(stride) = sysfs("/sys/class/graphics/fb0/stride").parse::<usize>().ok().filter(|s| *s >= w * 4) else { return false };
        let Ok(f) = fs::OpenOptions::new().read(true).write(true).open("/dev/fb0") else { return false };
        let len = stride * h;
        let map = unsafe {
            libc::mmap(std::ptr::null_mut(), len, libc::PROT_READ | libc::PROT_WRITE, libc::MAP_SHARED, f.as_raw_fd(), 0)
        };
        if map == libc::MAP_FAILED {
            return false;
        }
        unsafe { libc::munmap(self.map as *mut libc::c_void, self.len) };
        (self.map, self.len, self.w, self.h, self.stride, self.size_key) = (map as *mut u8, len, w, h, stride, key);
        (self.rows, self.cols) = (h / CH, w / CW);
        self.grid = vec![BLANK; self.rows * self.cols];
        self.shown = vec![None; self.rows * self.cols];
        (self.r, self.c) = (0, 0);
        true
    }

    /// Gives the screen back to the console (another program, e.g. mpv, takes it).
    pub fn suspend(&mut self) {
        unsafe { libc::ioctl(self.tty, KDSETMODE as _, KD_TEXT) };
        GRAPHICS_TTY.store(-1, Ordering::SeqCst);
    }

    /// Takes the screen again after suspend(); the next write redraws everything.
    pub fn resume(&mut self) {
        if unsafe { libc::ioctl(self.tty, KDSETMODE as _, KD_GRAPHICS) } == 0 {
            GRAPHICS_TTY.store(self.tty, Ordering::SeqCst);
        }
        self.invalidate();
    }

    fn fgc(&self) -> u8 {
        if self.bold && self.fg < 8 { self.fg + 8 } else { self.fg }
    }

    fn clear(&mut self) {
        let blank = Cell { bg: self.bg, ..BLANK };
        self.grid.iter_mut().for_each(|c| *c = blank);
    }

    fn newline(&mut self) {
        self.r += 1;
        if self.r >= self.rows {
            self.grid.drain(..self.cols);
            self.grid.extend(std::iter::repeat(Cell { bg: self.bg, ..BLANK }).take(self.cols));
            self.r = self.rows - 1;
        }
    }

    fn put_text(&mut self, t: &str) {
        let (fg, bg) = (self.fgc(), self.bg);
        for ch in visual(t).chars() {
            let w = char_width(ch);
            if w == 0 {
                continue;
            }
            if self.c + w > self.cols {
                self.c = 0;
                self.newline();
            }
            let i = self.r * self.cols + self.c;
            self.grid[i] = Cell { ch, fg, bg, cont: false };
            if w == 2 {
                self.grid[i + 1] = Cell { ch: ' ', fg, bg, cont: true };
            }
            self.c += w;
        }
    }

    fn csi(&mut self, params: &str, fin: char) {
        let p: Vec<usize> = params.trim_start_matches('?').split(';').map(|x| x.parse().unwrap_or(0)).collect();
        let p0 = p.first().copied().unwrap_or(0);
        match fin {
            'H' | 'f' => {
                self.r = p0.max(1).min(self.rows) - 1;
                self.c = p.get(1).copied().unwrap_or(1).max(1).min(self.cols) - 1;
            }
            'A' => self.r = self.r.saturating_sub(p0.max(1)),
            'B' => self.r = (self.r + p0.max(1)).min(self.rows - 1),
            'C' => self.c = (self.c + p0.max(1)).min(self.cols - 1),
            'D' => self.c = self.c.saturating_sub(p0.max(1)),
            'K' => {
                let row = self.r * self.cols;
                let blank = Cell { bg: self.bg, ..BLANK };
                let (a, b) = match p0 {
                    1 => (0, self.c + 1),
                    2 => (0, self.cols),
                    _ => (self.c, self.cols),
                };
                for c in a..b.min(self.cols) {
                    self.grid[row + c] = blank;
                }
            }
            'J' if p0 == 2 || p0 == 3 => self.clear(),
            'm' if !params.starts_with('?') => {
                for &v in if p.is_empty() { &[0][..] } else { &p[..] } {
                    match v {
                        0 => {
                            self.fg = 7;
                            self.bg = 0;
                            self.bold = false;
                        }
                        1 => self.bold = true,
                        22 => self.bold = false,
                        30..=37 => self.fg = (v - 30) as u8,
                        39 => self.fg = 7,
                        40..=47 => self.bg = (v - 40) as u8,
                        49 => self.bg = 0,
                        90..=97 => self.fg = (v - 90 + 8) as u8,
                        _ => {}
                    }
                }
            }
            _ => {} // ?25l/h, the console's private ESC[9;0] / ESC[14;0], ...
        }
    }

    /// Interprets what EBI writes (the console's ANSI subset) into the grid, then draws.
    pub fn write(&mut self, s: &str) {
        let mut data = std::mem::take(&mut self.pending);
        data.push_str(s);
        let mut text = String::new();
        let mut it = data.char_indices().peekable();
        while let Some((i, ch)) = it.next() {
            match ch {
                '\x1b' => {
                    if !text.is_empty() {
                        self.put_text(&std::mem::take(&mut text));
                    }
                    match it.peek().map(|(_, c)| *c) {
                        Some('[') => {
                            it.next();
                            let mut params = String::new();
                            let mut fin = None;
                            for (_, c) in it.by_ref() {
                                if ('\x40'..='\x7e').contains(&c) {
                                    fin = Some(c);
                                    break;
                                }
                                params.push(c);
                            }
                            match fin {
                                Some(f) => self.csi(&params, f),
                                None => {
                                    self.pending = data[i..].to_string(); // finish next write
                                    break;
                                }
                            }
                        }
                        Some(_) => {
                            it.next(); // ESC + one char (ESC 7, ESC c, ...): ignored
                        }
                        None => self.pending = "\x1b".into(),
                    }
                }
                '\r' | '\n' | '\x08' => {
                    if !text.is_empty() {
                        self.put_text(&std::mem::take(&mut text));
                    }
                    match ch {
                        '\r' => self.c = 0,
                        '\n' => self.newline(),
                        _ => self.c = self.c.saturating_sub(1),
                    }
                }
                c if (c as u32) < 0x20 || c == '\x7f' => {}
                c => text.push(c),
            }
        }
        if !text.is_empty() {
            self.put_text(&text);
        }
        self.flush();
    }

    /// Forget what is on the screen (after another program used it): next flush redraws all.
    pub fn invalidate(&mut self) {
        self.shown.iter_mut().for_each(|c| *c = None);
    }

    fn flush(&mut self) {
        for row in 0..self.rows {
            let a = row * self.cols;
            let b = a + self.cols;
            if self.grid[a..b].iter().zip(&self.shown[a..b]).all(|(g, s)| Some(*g) == *s) {
                continue;
            }
            self.draw_row(row);
            for i in a..b {
                self.shown[i] = Some(self.grid[i]);
            }
        }
    }

    fn draw_row(&mut self, row: usize) {
        let y0 = row * CH;
        let mut line = vec![0u32; self.cols * CW];
        for py in 0..CH {
            let gy = py / 2;
            let mut col = 0;
            while col < self.cols {
                let cell = self.grid[row * self.cols + col];
                let cells = if cell.cont { 1 } else { char_width(cell.ch).max(1) };
                let (fg, bg) = (PALETTE[cell.fg as usize & 15], PALETTE[cell.bg as usize & 15]);
                let x0 = col * CW;
                let tw = (cells * CW).min((self.cols - col) * CW);
                let glyph = if cell.cont || cell.ch == ' ' { None } else { self.font.get(cell.ch) };
                match glyph {
                    Some((gw, bits)) => {
                        let gw = *gw as usize;
                        let bpr = gw / 8; // bytes per glyph row
                        for px in 0..tw {
                            let gx = px * gw / (cells * CW);
                            let byte = bits.get(gy * bpr + gx / 8).copied().unwrap_or(0);
                            line[x0 + px] = if byte & (0x80 >> (gx % 8)) != 0 { fg } else { bg };
                        }
                    }
                    None => line[x0..x0 + tw].iter_mut().for_each(|p| *p = bg),
                }
                col += if cell.cont { 1 } else { cells };
            }
            let y = y0 + py;
            if y >= self.h {
                break;
            }
            let n = line.len().min(self.w);
            unsafe {
                let dst = self.map.add(y * self.stride) as *mut u32;
                std::ptr::copy_nonoverlapping(line.as_ptr(), dst, n);
            }
        }
    }
}

impl Drop for Fb {
    fn drop(&mut self) {
        unsafe {
            libc::ioctl(self.tty, KDSETMODE as _, KD_TEXT);
            libc::munmap(self.map as *mut libc::c_void, self.len);
        }
        GRAPHICS_TTY.store(-1, Ordering::SeqCst);
    }
}

extern "C" fn on_signal(sig: libc::c_int) {
    let t = GRAPHICS_TTY.load(Ordering::SeqCst);
    if t >= 0 {
        unsafe { libc::ioctl(t, KDSETMODE as _, KD_TEXT) };
    }
    unsafe { libc::_exit(128 + sig) };
}

/// SIGTERM/SIGINT/SIGHUP give the console back (a killed EBI must not leave tty1 in
/// graphics mode with a frozen picture).
fn install_signal_handlers() {
    for s in [libc::SIGTERM, libc::SIGINT, libc::SIGHUP] {
        unsafe { libc::signal(s, on_signal as *const () as libc::sighandler_t) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fb(rows: usize, cols: usize) -> Fb {
        // a grid-only Fb over a heap buffer (no tty, no /dev/fb0)
        let (w, h) = (cols * CW, rows * CH);
        let mut buf = vec![0u8; w * 4 * h].into_boxed_slice();
        let map = buf.as_mut_ptr();
        std::mem::forget(buf);
        let font = Font::parse("0041:0000000018242442427E424242420000\n4E2D:0100010001003FF8210821082108210821083FF8210801000100010001000100\nFFFD:00007E7E7E7E7E7E7E7E7E7E7E7E0000\n").unwrap();
        Fb {
            tty: -1, map, len: 0, w, h, stride: w * 4, rows, cols,
            grid: vec![BLANK; rows * cols], shown: vec![None; rows * cols],
            r: 0, c: 0, fg: 7, bg: 0, bold: false, font, pending: String::new(), size_key: String::new(),
        }
    }

    fn row_text(f: &Fb, r: usize) -> String {
        f.grid[r * f.cols..(r + 1) * f.cols].iter().filter(|c| !c.cont).map(|c| c.ch).collect::<String>().trim_end().to_string()
    }

    #[test]
    fn interprets_console_text() {
        let mut f = fb(4, 10);
        f.write("\x1b[H\x1b[0;30;46mAB\x1b[K\r\nx\x1b[0m");
        assert_eq!(row_text(&f, 0), "AB");
        assert_eq!(f.grid[0].bg, 6); // cyan bar
        assert_eq!(f.grid[5].bg, 6); // EL filled with the current background
        assert_eq!(row_text(&f, 1), "x");
        f.write("\x1b[3;4Hhi");
        assert_eq!(row_text(&f, 2), "   hi");
        f.write("\x1b[0m\x1b[2J");
        assert_eq!(row_text(&f, 2), "");
        // a wide character takes two cells
        f.write("\x1b[H中A");
        assert_eq!(f.grid[0].ch, '中');
        assert!(f.grid[1].cont);
        assert_eq!(f.grid[2].ch, 'A');
        // an escape split across writes
        f.write("\x1b[2");
        f.write(";1HZ");
        assert_eq!(row_text(&f, 1), "Z");
        // bold = bright foreground
        f.write("\x1b[1;37mB");
        assert_eq!(f.grid[f.cols + 1].fg, 15);
        // drawing: a glyph's set bits get the foreground colour
        let px = unsafe { std::slice::from_raw_parts(f.map as *const u32, f.w * f.h) };
        assert!(px.iter().any(|p| *p == 0xffffff));
    }

    /// Renders a sample screen with the real font into a raw XRGB file, for a look on the PC:
    /// H96UI_FONT=build/unifont.hex H96UI_RENDER_OUT=build/sample.raw cargo test render_sample -- --ignored
    #[test]
    #[ignore]
    fn render_sample() {
        let font = Font::load().expect("set H96UI_FONT to unifont.hex");
        let mut f = fb(12, 60);
        f.font = font;
        f.write("\x1b[H\x1b[0;30;46m  EBI  >  Favorites\x1b[K\x1b[0m\r\n\r\n");
        f.write("\x1b[0;30;47m> NHK 総合 東京                                ★ Japan \x1b[0m\r\n");
        f.write("    |AR| قناة الجزيرة 24                          ★ Arabic\r\n");
        f.write("    سلام عليكم (HD)                               Arabic\r\n");
        f.write("    כאן 11                                        ★ Israel\r\n");
        f.write("    KBS 한국방송                                  Korea\r\n");
        f.write("    CCTV 中央电视台                               China\r\n");
        f.write("    Ελληνική ΤV, Россия 1, Ça marche              Mixed\r\n");
        f.write("\r\n\x1b[1;33m  Added to Favorites: NHK 総合 東京\x1b[0m");
        let px = unsafe { std::slice::from_raw_parts(f.map as *const u8, f.stride * f.h) };
        let out = std::env::var("H96UI_RENDER_OUT").unwrap_or("sample.raw".into());
        std::fs::write(out, px).unwrap();
        println!("{}x{}", f.w, f.h);
    }

    #[test]
    fn hex_font() {
        let font = Font::parse("0041:0000000018242442427E424242420000\nbad line\n").unwrap();
        assert_eq!(font.glyphs[&0x41].0, 8);
        assert!(font.get('Z').is_none()); // no FFFD in this font
    }
}
