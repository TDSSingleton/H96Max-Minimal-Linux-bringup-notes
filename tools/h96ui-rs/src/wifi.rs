//! WiFi networks from a USB stick: .txt files with a network name and password are added to
//! wpa_supplicant.conf, then h96-wifi is restarted (its stop keeps the driver loaded, which is
//! the safe way to reset only wpa_supplicant + DHCP).
//!
//! File format (one or more networks per file; `=` or `:`; a new ssid line starts a new one):
//!   ssid=My network        also: wifi, network, name
//!   password=secret        also: pass, psk, key; no password line = open network
//! A file whose name starts with "wifi" may also be just two plain lines: name, then password.

use std::fs;
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Net {
    pub ssid: String,
    pub psk: Option<String>, // None: open network
}

impl Net {
    /// wpa_supplicant's limits: a passphrase is 8..63 characters, or 64 hex digits (a raw PSK).
    pub fn check(&self) -> Result<(), String> {
        if self.ssid.is_empty() || self.ssid.len() > 32 {
            return Err("the network name must be 1..32 bytes".into());
        }
        if let Some(p) = &self.psk {
            let raw = p.len() == 64 && p.bytes().all(|b| b.is_ascii_hexdigit());
            if !raw && !(8..=63).contains(&p.len()) {
                return Err("the password must be 8..63 characters".into());
            }
            if p.contains(['\n', '\r']) {
                return Err("the password has a line break in it".into());
            }
        }
        Ok(())
    }
}

/// The networks in one file (see the module comment for the format).
pub fn parse_file(file_name: &str, text: &str) -> Vec<Net> {
    let text = text.trim_start_matches('\u{feff}');
    let mut out: Vec<Net> = Vec::new();
    let mut keyed = false;
    for l in text.lines() {
        let l = l.trim_end_matches('\r');
        if l.trim().is_empty() || l.trim_start().starts_with('#') {
            continue;
        }
        let Some((k, v)) = l.split_once(['=', ':']) else { continue };
        // values keep inner spaces; only the space right after the separator is dropped
        let v = v.strip_prefix(' ').unwrap_or(v).trim_end_matches([' ', '\t']).to_string();
        match k.trim().to_ascii_lowercase().as_str() {
            "ssid" | "wifi" | "network" | "name" | "wifi name" | "network name" => {
                keyed = true;
                out.push(Net { ssid: v, psk: None });
            }
            "password" | "pass" | "psk" | "key" | "wifi password" => {
                keyed = true;
                if let Some(n) = out.last_mut().filter(|n| n.psk.is_none()) {
                    n.psk = Some(v).filter(|p| !p.is_empty());
                }
            }
            _ => {}
        }
    }
    if !keyed && file_name.to_ascii_lowercase().starts_with("wifi") {
        let mut lines = text.lines().map(|l| l.trim_end_matches('\r')).filter(|l| !l.trim().is_empty());
        if let Some(ssid) = lines.next() {
            out.push(Net { ssid: ssid.trim().to_string(), psk: lines.next().map(String::from) });
        }
    }
    out.retain(|n| !n.ssid.is_empty());
    out
}

/// .txt/.wifi files on the mounted stick (top folder, /wifi, /iptv) that hold networks.
pub fn find_on_usb() -> Vec<(PathBuf, Vec<Net>)> {
    let root = Path::new(crate::sys::USB_MOUNT);
    let mut out = Vec::new();
    for dir in ["", "wifi", "WiFi", "WIFI", "iptv", "IPTV"] {
        let Ok(ents) = fs::read_dir(root.join(dir)) else { continue };
        for e in ents.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            let ext = Path::new(&name).extension().map(|x| x.to_string_lossy().to_lowercase()).unwrap_or_default();
            if name.starts_with('.') || !matches!(ext.as_str(), "txt" | "wifi") || !e.path().is_file() {
                continue;
            }
            let Ok(b) = fs::read(e.path()) else { continue };
            if b.len() > 64 << 10 {
                continue;
            }
            let nets = parse_file(&name, &String::from_utf8_lossy(&b));
            if !nets.is_empty() {
                out.push((e.path(), nets));
            }
        }
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

// ---------- wpa_supplicant.conf ----------

pub fn conf_path() -> PathBuf {
    match std::env::var("H96UI_WPA_CONF") {
        Ok(p) if !p.is_empty() => PathBuf::from(p),
        _ => PathBuf::from("/etc/wpa_supplicant/wpa_supplicant.conf"),
    }
}

/// A conf file split into the text outside network blocks and the blocks themselves.
struct Conf {
    head: Vec<String>,
    blocks: Vec<Vec<String>>, // lines between "network={" and "}"
}

fn parse_conf(text: &str) -> Conf {
    let mut c = Conf { head: Vec::new(), blocks: Vec::new() };
    let mut cur: Option<Vec<String>> = None;
    for l in text.lines() {
        let t = l.trim();
        match cur.as_mut() {
            None if t.replace(' ', "").starts_with("network={") => cur = Some(Vec::new()),
            None => c.head.push(l.to_string()),
            Some(_) if t == "}" => c.blocks.push(cur.take().unwrap_or_default()),
            Some(b) => b.push(t.to_string()),
        }
    }
    if let Some(b) = cur {
        c.blocks.push(b); // unterminated block: keep it rather than lose it
    }
    c
}

fn block_value<'a>(b: &'a [String], key: &str) -> Option<&'a str> {
    b.iter().find_map(|l| l.strip_prefix(key)?.strip_prefix('='))
}

/// ssid="name" (quoted, up to the last quote) or ssid=6e616d65 (hex).
fn block_ssid(b: &[String]) -> Option<String> {
    let v = block_value(b, "ssid")?;
    if let Some(q) = v.strip_prefix('"') {
        return Some(q.rfind('"').map_or(q, |i| &q[..i]).to_string());
    }
    let bytes: Option<Vec<u8>> =
        (0..v.len()).step_by(2).map(|i| v.get(i..i + 2).and_then(|h| u8::from_str_radix(h, 16).ok())).collect();
    bytes.map(|b| String::from_utf8_lossy(&b).into_owned())
}

/// Network names already in the conf, in file order.
pub fn saved(text: &str) -> Vec<String> {
    parse_conf(text).blocks.iter().filter_map(|b| block_ssid(b)).collect()
}

/// The conf with `net` added (replacing a block with the same name) at the highest priority,
/// so wpa_supplicant prefers it over the other saved networks.
pub fn upsert(text: &str, net: &Net) -> String {
    let mut c = parse_conf(text);
    // counted before the old block goes, so a re-added network still ends up on top
    let prio = c.blocks.iter().filter_map(|b| block_value(b, "priority")?.parse::<i32>().ok()).max().unwrap_or(0) + 1;
    c.blocks.retain(|b| block_ssid(b).as_deref() != Some(net.ssid.as_str()));
    let mut b = vec![format!("ssid=\"{}\"", net.ssid), "scan_ssid=1".to_string()];
    match &net.psk {
        Some(p) if p.len() == 64 && p.bytes().all(|x| x.is_ascii_hexdigit()) => b.push(format!("psk={p}")),
        Some(p) => b.push(format!("psk=\"{p}\"")),
        None => b.push("key_mgmt=NONE".to_string()),
    }
    b.push(format!("priority={prio}"));
    c.blocks.push(b);

    let mut out = String::new();
    let mut head = c.head;
    while head.last().is_some_and(|l| l.trim().is_empty()) {
        head.pop();
    }
    if head.is_empty() {
        head.push("# WiFi networks (EBI: Add WiFi from USB adds to this file)".into());
    }
    for l in head {
        out.push_str(&l);
        out.push('\n');
    }
    for b in c.blocks {
        out.push_str("\nnetwork={\n");
        for l in b {
            out.push('\t');
            out.push_str(&l);
            out.push('\n');
        }
        out.push_str("}\n");
    }
    out
}

/// Adds `net` to the conf (old file kept as .bak; new one written 0600 and renamed into place)
/// and restarts h96-wifi.
pub fn add(net: &Net) -> Result<(), String> {
    net.check()?;
    let path = conf_path();
    let old = match fs::read(&path) {
        Ok(b) => String::from_utf8_lossy(&b).into_owned(),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(format!("{}: {e}", path.display())),
    };
    let new = upsert(&old, net);
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    if !old.is_empty() {
        let bak = path.with_extension("conf.bak");
        write_private(&bak, old.as_bytes()).map_err(|e| format!("{}: {e}", bak.display()))?;
    }
    let tmp = path.with_extension("conf.new");
    write_private(&tmp, new.as_bytes()).map_err(|e| format!("{}: {e}", tmp.display()))?;
    fs::rename(&tmp, &path).map_err(|e| format!("{}: {e}", path.display()))?;
    restart()
}

fn write_private(p: &Path, data: &[u8]) -> std::io::Result<()> {
    let mut f = fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(p)?;
    f.write_all(data)?;
    f.sync_all()
}

/// rc-service h96-wifi restart (skipped when H96UI_WPA_CONF points at a test file).
fn restart() -> Result<(), String> {
    if std::env::var("H96UI_WPA_CONF").is_ok_and(|p| !p.is_empty()) {
        return Ok(());
    }
    match Command::new("rc-service").args(["h96-wifi", "restart"]).output() {
        Ok(o) if o.status.success() => Ok(()),
        Ok(o) => {
            let e = String::from_utf8_lossy(&o.stderr);
            let e = e.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("").trim().to_string();
            Err(format!("restarting WiFi: {}", if e.is_empty() { crate::sys::exit_text(&o.status) } else { e }))
        }
        Err(e) => Err(format!("rc-service: {e}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn net(s: &str, p: Option<&str>) -> Net {
        Net { ssid: s.into(), psk: p.map(String::from) }
    }

    #[test]
    fn files() {
        assert_eq!(parse_file("x.txt", "ssid=Home Net\npassword=pa ss:word\n"), vec![net("Home Net", Some("pa ss:word"))]);
        assert_eq!(parse_file("x.txt", "\u{feff}WiFi: Cafe\r\nPassword: 12345678\r\n"), vec![net("Cafe", Some("12345678"))]);
        assert_eq!(
            parse_file("x.txt", "# two\nssid=A\npsk=aaaaaaaa\n\nssid=Guest\n"),
            vec![net("A", Some("aaaaaaaa")), net("Guest", None)]
        );
        assert_eq!(parse_file("wifi.txt", "Home\nsecret99\n"), vec![net("Home", Some("secret99"))]);
        assert_eq!(parse_file("wifi-guest.txt", "Guest\n"), vec![net("Guest", None)]);
        // not WiFi: a URL list, an Xtream account, a random note
        assert!(parse_file("lists.txt", "http://a.example/list.m3u\n").is_empty());
        assert!(parse_file("acc.txt", "server=http://x:8080\nusername=u\npassword=p\n").is_empty());
        assert!(parse_file("notes.txt", "Home\nsecret99\n").is_empty());
    }

    #[test]
    fn checks() {
        assert!(net("A", Some("12345678")).check().is_ok());
        assert!(net("A", None).check().is_ok());
        assert!(net("A", Some("short")).check().is_err());
        assert!(net("A", Some(&"ab".repeat(32))).check().is_ok()); // raw PSK
        assert!(net("", Some("12345678")).check().is_err());
    }

    #[test]
    fn conf() {
        let old = "ctrl_interface=/run/wpa_supplicant\ncountry=FR\n\nnetwork={\n\tssid=\"Home\"\n\tpsk=\"oldpass1\"\n\tpriority=3\n}\nnetwork={\n  ssid=4361666565\n  key_mgmt=NONE\n}\n";
        assert_eq!(saved(old), vec!["Home", "Cafee"]);
        let new = upsert(old, &net("Home", Some("newpass1")));
        assert!(new.starts_with("ctrl_interface=/run/wpa_supplicant\ncountry=FR\n"));
        assert!(!new.contains("oldpass1"));
        assert!(new.contains("ssid=\"Home\"\n\tscan_ssid=1\n\tpsk=\"newpass1\"\n\tpriority=4\n}"));
        assert_eq!(saved(&new), vec!["Cafee", "Home"]); // the other network is kept
        let open = upsert("", &net("Guest \"x\"", None));
        assert!(open.contains("ssid=\"Guest \"x\"\"\n\tscan_ssid=1\n\tkey_mgmt=NONE\n\tpriority=1\n"));
        assert_eq!(saved(&open), vec!["Guest \"x\""]);
    }

    #[test]
    fn add_writes_file() {
        let d = std::env::temp_dir().join(format!("h96ui-wifi-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        let p = d.join("wpa.conf");
        std::env::set_var("H96UI_WPA_CONF", &p);
        add(&net("Home", Some("secret99"))).unwrap();
        add(&net("Cafe", None)).unwrap();
        let text = fs::read_to_string(&p).unwrap();
        assert_eq!(saved(&text), vec!["Home", "Cafe"]);
        assert!(fs::read_to_string(d.join("wpa.conf.bak")).unwrap().contains("secret99"));
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(fs::metadata(&p).unwrap().permissions().mode() & 0o777, 0o600);
        assert!(add(&net("Bad", Some("short"))).is_err());
        let _ = fs::remove_dir_all(&d);
    }
}
