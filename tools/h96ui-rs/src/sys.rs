//! Things the UI reads from the system: WiFi state, the USB stick, version strings, local time.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Clone, Default)]
pub struct WifiState {
    pub ssid: String,
    pub signal: String, // e.g. "-58 dBm"
    pub ip: String,
}

fn output(cmd: &str, args: &[&str]) -> Option<String> {
    let o = Command::new(cmd).args(args).output().ok()?;
    if !o.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&o.stdout).into_owned())
}

pub fn read_wifi() -> WifiState {
    let mut w = WifiState::default();
    if let Some(out) = output("iw", &["dev", "wlan0", "link"]) {
        for l in out.lines() {
            let l = l.trim();
            if let Some(v) = l.strip_prefix("SSID:") {
                w.ssid = v.trim().to_string();
            } else if let Some(v) = l.strip_prefix("signal:") {
                w.signal = v.trim().to_string();
            }
        }
    }
    if let Some(out) = output("ip", &["-4", "-o", "addr", "show", "wlan0"]) {
        let f: Vec<&str> = out.split_whitespace().collect();
        for i in 0..f.len() {
            if f[i] == "inet" && i + 1 < f.len() {
                w.ip = f[i + 1].split('/').next().unwrap_or("").to_string();
            }
        }
    }
    w
}

pub fn read_first_line(p: impl AsRef<Path>) -> String {
    match fs::read(p) {
        Ok(b) => String::from_utf8_lossy(&b).lines().next().unwrap_or("").trim().to_string(),
        Err(_) => String::new(),
    }
}

pub fn kernel_release() -> String {
    read_first_line("/proc/sys/kernel/osrelease")
}

pub fn alpine_release() -> String {
    read_first_line("/etc/alpine-release")
}

/// strftime() in the local time zone (std has no local time).
pub fn local_time(fmt: &str) -> String {
    let fmt = std::ffi::CString::new(fmt).unwrap_or_default();
    unsafe {
        let t = libc::time(std::ptr::null_mut());
        let mut tm: libc::tm = std::mem::zeroed();
        libc::localtime_r(&t, &mut tm);
        let mut buf = [0u8; 64];
        let n = libc::strftime(buf.as_mut_ptr() as *mut libc::c_char, buf.len(), fmt.as_ptr(), &tm);
        String::from_utf8_lossy(&buf[..n]).into_owned()
    }
}

/// strftime() of a Unix time in the local time zone.
pub fn format_time(t: i64, fmt: &str) -> String {
    let fmt = std::ffi::CString::new(fmt).unwrap_or_default();
    unsafe {
        #[allow(deprecated)] // 64-bit on aarch64 musl already
        let t = t as libc::time_t;
        let mut tm: libc::tm = std::mem::zeroed();
        libc::localtime_r(&t, &mut tm);
        let mut buf = [0u8; 64];
        let n = libc::strftime(buf.as_mut_ptr() as *mut libc::c_char, buf.len(), fmt.as_ptr(), &tm);
        String::from_utf8_lossy(&buf[..n]).into_owned()
    }
}

pub fn local_minute() -> i32 {
    unsafe {
        let t = libc::time(std::ptr::null_mut());
        let mut tm: libc::tm = std::mem::zeroed();
        libc::localtime_r(&t, &mut tm);
        tm.tm_min
    }
}

// ---- USB stick ----

pub const USB_MOUNT: &str = "/mnt/h96usb";

/// USB block devices now present: whole disks under /sys/block whose device path runs through
/// the USB bus, with their partitions. A disk with no medium (size 0) is skipped.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct UsbDisk {
    pub disk: String,       // "sda"
    pub parts: Vec<String>, // ["sda1", ...], numerically sorted
}

pub fn usb_disks() -> Vec<UsbDisk> {
    let mut out = Vec::new();
    for e in fs::read_dir("/sys/block").into_iter().flatten().flatten() {
        let disk = e.file_name().to_string_lossy().into_owned();
        if !disk.starts_with("sd") {
            continue;
        }
        let base = e.path();
        let real = fs::canonicalize(&base).unwrap_or_default();
        if !real.to_string_lossy().contains("/usb") || read_first_line(base.join("size")) == "0" {
            continue;
        }
        let mut parts: Vec<String> = fs::read_dir(&base)
            .into_iter()
            .flatten()
            .flatten()
            .map(|p| p.file_name().to_string_lossy().into_owned())
            .filter(|n| n.starts_with(&disk) && n[disk.len()..].bytes().all(|b| b.is_ascii_digit()) && n.len() > disk.len())
            .collect();
        parts.sort_by_key(|n| n[disk.len()..].parse::<u32>().unwrap_or(0));
        out.push(UsbDisk { disk, parts });
    }
    out.sort_by(|a, b| a.disk.cmp(&b.disk));
    out
}

/// Filesystem type from busybox blkid ("vfat", "exfat", "ntfs", "ext4", ...), "" if unknown.
fn fs_type(dev: &str) -> String {
    blkid_type(&output("blkid", &[dev]).unwrap_or_default())
}

fn blkid_type(out: &str) -> String {
    out.split_whitespace()
        .find_map(|f| f.strip_prefix("TYPE=\"")?.strip_suffix('"').map(String::from))
        .unwrap_or_default()
}

fn run_ok(cmd: &str, args: &[&str]) -> Result<(), String> {
    match Command::new(cmd).args(args).output() {
        Ok(o) if o.status.success() => Ok(()),
        Ok(o) => {
            let e = String::from_utf8_lossy(&o.stderr);
            let e = e.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("").trim().to_string();
            Err(if e.is_empty() { exit_text(&o.status) } else { e })
        }
        Err(e) => Err(format!("{cmd}: {e}")),
    }
}

/// Mounts one filesystem read-only. exFAT is not in the kernel: it goes through fuse-exfat.
fn mount_one(dev: &str, fstype: &str) -> Result<(), String> {
    match fstype {
        "exfat" => run_ok("mount.exfat-fuse", &["-o", "ro", dev, USB_MOUNT])
            .map_err(|e| if e.contains("No such file") || e.starts_with("mount.exfat-fuse:") {
                "exFAT needs fuse-exfat (apk add fuse-exfat)".to_string()
            } else { e }),
        "vfat" | "ext4" | "ext3" | "ext2" | "ntfs" | "iso9660" => {
            run_ok("mount", &["-t", fstype, "-o", "ro", dev, USB_MOUNT])
        }
        "" => Err("no recognizable filesystem".into()),
        t => Err(format!("unsupported filesystem {t}")),
    }
}

/// Mounts the first readable filesystem on a USB stick, read-only: partitions first, then the
/// whole disk (sticks formatted without a partition table). Ok(None) if no stick is plugged in.
pub fn mount_usb() -> Result<Option<String>, String> {
    unmount_usb();
    let _ = fs::create_dir_all(USB_MOUNT);
    let mut last_err = None;
    for d in usb_disks() {
        let mut cands = d.parts.clone();
        cands.push(d.disk.clone());
        for c in cands {
            let dev = format!("/dev/{c}");
            let t = fs_type(&dev);
            if t.is_empty() && c == d.disk && !d.parts.is_empty() {
                continue; // a partition table, not a filesystem
            }
            match mount_one(&dev, &t) {
                Ok(()) => return Ok(Some(dev)),
                Err(e) => last_err = Some(format!("{dev}: {e}")),
            }
        }
    }
    match last_err {
        Some(e) => Err(e),
        None => Ok(None),
    }
}

/// Same wording as Go's exec.ExitError ("exit status 32"), which the UI shows.
pub fn exit_text(st: &std::process::ExitStatus) -> String {
    match st.code() {
        Some(c) => format!("exit status {c}"),
        None => st.to_string(),
    }
}

/// Lazy unmount: also works after the stick was pulled out while mounted.
pub fn unmount_usb() {
    let _ = Command::new("umount").args(["-l", USB_MOUNT]).output();
}

#[derive(Clone, PartialEq, Eq)]
pub enum ImportKind {
    Playlist,
    Urls,
    Xtream(crate::xtream::Account),
}

#[derive(Clone)]
pub struct Importable {
    pub path: PathBuf,
    pub kind: ImportKind,
}

/// Playlist files and URL lists on the stick (root and /iptv).
pub fn find_importables() -> Vec<Importable> {
    let mut out = Vec::new();
    let root = Path::new(USB_MOUNT);
    for dir in [root.to_path_buf(), root.join("iptv"), root.join("IPTV")] {
        let Ok(ents) = fs::read_dir(&dir) else { continue };
        for e in ents.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') || e.file_type().map(|t| t.is_dir()).unwrap_or(true) {
                continue;
            }
            let p = e.path();
            let ext = p.extension().map(|x| x.to_string_lossy().to_lowercase()).unwrap_or_default();
            // an Xtream account (provider link or server/username/password lines) wins over
            // "a list of playlist URLs": its get.php link would otherwise be downloaded as a
            // (huge) flat M3U
            if matches!(ext.as_str(), "txt" | "url" | "xtream") {
                if let Some(acc) = fs::read(&p).ok().and_then(|b| crate::xtream::Account::from_text(&String::from_utf8_lossy(&b))) {
                    out.push(Importable { path: p, kind: ImportKind::Xtream(acc) });
                    continue;
                }
            }
            match ext.as_str() {
                "m3u" | "m3u8" => out.push(Importable { path: p, kind: ImportKind::Playlist }),
                "txt" | "url" if !crate::store::urls_in(&p).is_empty() => {
                    out.push(Importable { path: p, kind: ImportKind::Urls })
                }
                _ => {}
            }
        }
    }
    out.sort_by(|a, b| a.path.cmp(&b.path));
    out
}

#[cfg(test)]
mod tests {
    #[test]
    fn blkid() {
        let l = "/dev/sda1: LABEL=\"MY STICK\" UUID=\"1A2B-3C4D\" TYPE=\"vfat\"\n";
        assert_eq!(super::blkid_type(l), "vfat");
        assert_eq!(super::blkid_type("/dev/sda: PTTYPE=\"dos\"\n"), "");
        assert_eq!(super::blkid_type(""), "");
    }
}
