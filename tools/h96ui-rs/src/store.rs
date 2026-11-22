//! Subscriptions and settings under /var/lib/h96ui (override with H96UI_DATA for tests).
//!   subs/<name>.m3u   the playlist (copied from USB, or downloaded)
//!   subs/<name>.url   only for URL subscriptions: the source URL (used by Refresh)
//!   settings.json     playback settings

use crate::m3u::{parse_m3u, Channel};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

pub fn data_dir() -> PathBuf {
    match std::env::var("H96UI_DATA") {
        Ok(d) if !d.is_empty() => PathBuf::from(d),
        _ => PathBuf::from("/var/lib/h96ui"),
    }
}

fn subs_dir() -> PathBuf {
    data_dir().join("subs")
}

fn read_text(p: &Path) -> std::io::Result<String> {
    Ok(String::from_utf8_lossy(&fs::read(p)?).into_owned())
}

#[derive(Clone)]
pub struct Subscription {
    pub name: String,
    pub path: PathBuf, // .m3u, or .xtream for an Xtream account
    pub url: String,   // empty for file subscriptions
    pub xtream: Option<crate::xtream::Account>,
}

fn with_ext(p: &Path, ext: &str) -> PathBuf {
    p.with_extension(ext)
}

pub fn list_subscriptions() -> Vec<Subscription> {
    let mut out: Vec<Subscription> = fs::read_dir(subs_dir())
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let p = e.path();
            let fname = p.file_name()?.to_str()?.to_string();
            if let Some(name) = fname.strip_suffix(".xtream") {
                let acc = crate::xtream::Account::from_text(&read_text(&p).ok()?)?;
                return Some(Subscription { name: name.to_string(), path: p, url: String::new(), xtream: Some(acc) });
            }
            let name = fname.strip_suffix(".m3u")?.to_string();
            let url = read_text(&with_ext(&p, "url")).map(|s| s.trim().to_string()).unwrap_or_default();
            Some(Subscription { name, path: p, url, xtream: None })
        })
        .collect();
    out.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()).then(a.name.cmp(&b.name)));
    out
}

impl Subscription {
    pub fn channels(&self) -> Result<Vec<Channel>, String> {
        if self.xtream.is_some() {
            return Err("an Xtream account has no playlist file".into());
        }
        read_text(&self.path).map(|t| parse_m3u(&t)).map_err(|e| io_text(&e, &self.path))
    }

    pub fn refresh(&self) -> Result<usize, String> {
        if self.url.is_empty() {
            return Err("not a URL subscription".into());
        }
        let b = fetch(&self.url)?;
        let n = parse_m3u(&String::from_utf8_lossy(&b)).len();
        if n == 0 {
            return Err("no channels in the downloaded playlist".into());
        }
        fs::write(&self.path, b).map_err(|e| io_text(&e, &self.path))?;
        Ok(n)
    }

    pub fn remove(&self) -> Result<(), String> {
        let _ = fs::remove_file(with_ext(&self.path, "url"));
        fs::remove_file(&self.path).map_err(|e| io_text(&e, &self.path))
    }
}

/// Error text in the style the Go version showed ("open <path>: no such file or directory").
fn io_text(e: &std::io::Error, p: &Path) -> String {
    let msg = match e.kind() {
        std::io::ErrorKind::NotFound => "no such file or directory".to_string(),
        std::io::ErrorKind::PermissionDenied => "permission denied".to_string(),
        _ => e.to_string(),
    };
    format!("open {}: {}", p.display(), msg)
}

/// Replaces each run of characters outside [A-Za-z0-9._ -] with "_", trims, and makes the
/// name unique among existing subscriptions.
fn unique_name(base: &str) -> String {
    let mut clean = String::new();
    let mut in_bad = false;
    for c in base.chars() {
        if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | ' ' | '-') {
            clean.push(c);
            in_bad = false;
        } else if !in_bad {
            clean.push('_');
            in_bad = true;
        }
    }
    let base = match clean.trim() {
        "" => "subscription".to_string(),
        s => s.to_string(),
    };
    let mut name = base.clone();
    let mut i = 2;
    while subs_dir().join(format!("{name}.m3u")).exists() || subs_dir().join(format!("{name}.xtream")).exists() {
        name = format!("{base} ({i})");
        i += 1;
    }
    name
}

fn file_stem_no_ext(p: &Path) -> String {
    let f = p.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    match f.rfind('.') {
        Some(i) => f[..i].to_string(),
        None => f,
    }
}

/// The subscription whose playlist has exactly these bytes (an earlier import of the same file).
pub fn subscription_with_content(b: &[u8]) -> Option<String> {
    list_subscriptions()
        .into_iter()
        .find(|s| s.url.is_empty() && fs::read(&s.path).map(|x| x == b).unwrap_or(false))
        .map(|s| s.name)
}

/// The URL subscription that already tracks this URL.
pub fn subscription_with_url(url: &str) -> Option<String> {
    list_subscriptions().into_iter().find(|s| s.url == url).map(|s| s.name)
}

/// Logs in to an Xtream account and stores it (mode 0600) as subs/<host>.xtream.
pub fn import_xtream(acc: &crate::xtream::Account) -> Result<Imported, String> {
    if let Some(s) = list_subscriptions().into_iter().find(|s| {
        s.xtream.as_ref().is_some_and(|x| x.server == acc.server && x.username == acc.username)
    }) {
        return Ok(Imported::Already(s.name));
    }
    let info = acc.info()?;
    fs::create_dir_all(subs_dir()).map_err(|e| e.to_string())?;
    let name = unique_name(&acc.host().replace(':', " "));
    let dst = subs_dir().join(format!("{name}.xtream"));
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let mut f = fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(&dst).map_err(|e| io_text(&e, &dst))?;
    f.write_all(acc.to_file().as_bytes()).map_err(|e| io_text(&e, &dst))?;
    let st = if info.status.is_empty() { "ok".to_string() } else { info.status.to_lowercase() };
    Ok(Imported::Account(name, st))
}

pub enum Imported {
    New(String, usize),     // playlist: name, channel count
    Account(String, String), // Xtream: name, account status
    Already(String),
}

/// Copies a playlist file into the store; returns the subscription name and channel count.
pub fn import_file(src: &Path) -> Result<Imported, String> {
    let b = fs::read(src).map_err(|e| io_text(&e, src))?;
    if let Some(name) = subscription_with_content(&b) {
        return Ok(Imported::Already(name));
    }
    let n = parse_m3u(&String::from_utf8_lossy(&b)).len();
    let base = src.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    if n == 0 {
        return Err(format!("no channels found in {base}"));
    }
    fs::create_dir_all(subs_dir()).map_err(|e| e.to_string())?;
    let name = unique_name(&file_stem_no_ext(src));
    let dst = subs_dir().join(format!("{name}.m3u"));
    fs::write(&dst, b).map_err(|e| io_text(&e, &dst))?;
    Ok(Imported::New(name, n))
}

/// The http(s) URLs in a .txt/.url file (one per line, # comments allowed).
pub fn urls_in(src: &Path) -> Vec<String> {
    let Ok(t) = read_text(src) else { return Vec::new() };
    t.split('\n')
        .map(|l| l.trim().trim_start_matches('\u{feff}').trim())
        .filter(|l| l.starts_with("http://") || l.starts_with("https://"))
        .map(String::from)
        .collect()
}

const MAX_DOWNLOAD: usize = 64 << 20;

/// HTTP(S) download via busybox wget (+ ssl_client), 60 s timeout, 64 MiB cap.
fn fetch(url: &str) -> Result<Vec<u8>, String> {
    fetch_with(url, &format!("h96ui/{}", crate::VERSION))
}

/// fetch() with a given User-Agent (Xtream panels often refuse unknown players).
pub fn fetch_with(url: &str, ua: &str) -> Result<Vec<u8>, String> {
    let o = Command::new("wget")
        .args(["-q", "-T", "60", "-U", ua, "-O", "-", "--", url])
        .output()
        .map_err(|e| format!("wget: {e}"))?;
    if !o.status.success() {
        let err = String::from_utf8_lossy(&o.stderr);
        let last = err.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("").trim();
        let last = last.strip_prefix("wget: ").unwrap_or(last);
        return Err(if last.is_empty() { format!("download failed ({})", crate::sys::exit_text(&o.status)) } else { last.to_string() });
    }
    let mut b = o.stdout;
    b.truncate(MAX_DOWNLOAD);
    Ok(b)
}

/// Downloads a playlist and stores it as a refreshable URL subscription.
pub fn import_url(url: &str) -> Result<Imported, String> {
    if let Some(name) = subscription_with_url(url) {
        return Ok(Imported::Already(name));
    }
    let b = fetch(url)?;
    let n = parse_m3u(&String::from_utf8_lossy(&b)).len();
    if n == 0 {
        return Err("no channels in the downloaded playlist".into());
    }
    fs::create_dir_all(subs_dir()).map_err(|e| e.to_string())?;
    let path = url.split('?').next().unwrap_or("").trim_end_matches('/'); // like Go filepath.Base
    let base = path.rsplit('/').next().unwrap_or("");
    let base = base.strip_suffix(".m3u8").unwrap_or(base);
    let base = base.strip_suffix(".m3u").unwrap_or(base);
    let name = unique_name(base);
    let dst = subs_dir().join(format!("{name}.m3u"));
    fs::write(&dst, b).map_err(|e| io_text(&e, &dst))?;
    let u = subs_dir().join(format!("{name}.url"));
    fs::write(&u, format!("{url}\n")).map_err(|e| io_text(&e, &u))?;
    Ok(Imported::New(name, n))
}

// ---- settings ----

#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    pub cache_secs: i64,
    pub deinterlace: bool,
    pub volume: i64,
    pub audio: String,  // hdmi | spdif | analog
    pub aspect: String, // fit | stretch | fill
    pub decoder: String, // hardware (VPU + display plane) | software (mpv)
    pub order: String,   // az (names.rs az_order) | provider (as the playlist/panel lists them)
    pub channels: String, // stereo (downmix: HDMI refuses 5.1 on a stereo TV) | source
}

impl Default for Settings {
    fn default() -> Self {
        Settings { cache_secs: 5, deinterlace: false, volume: 80, audio: "hdmi".into(), aspect: "fit".into(), decoder: "hardware".into(), order: "az".into(), channels: "stereo".into() }
    }
}

impl Settings {
    fn path() -> PathBuf {
        data_dir().join("settings.json")
    }

    pub fn load() -> Settings {
        let mut s = Settings::default();
        if let Ok(t) = read_text(&Self::path()) {
            s.apply_json(&t);
        }
        s
    }

    /// Reads the flat settings object; unknown keys and malformed values are ignored, so a
    /// missing field keeps its default (like Go's json.Unmarshal into defaults).
    fn apply_json(&mut self, t: &str) {
        for (k, v) in json_flat_object(t) {
            match (k.as_str(), v) {
                ("cache_secs", Json::Num(n)) => self.cache_secs = n,
                ("volume", Json::Num(n)) => self.volume = n,
                ("deinterlace", Json::Bool(b)) => self.deinterlace = b,
                ("audio", Json::Str(s)) => self.audio = s,
                ("aspect", Json::Str(s)) => self.aspect = s,
                ("decoder", Json::Str(s)) => self.decoder = s,
                ("order", Json::Str(s)) => self.order = s,
                ("channels", Json::Str(s)) => self.channels = s,
                _ => {}
            }
        }
    }

    pub fn to_json(&self) -> String {
        format!(
            "{{\n  \"cache_secs\": {},\n  \"deinterlace\": {},\n  \"volume\": {},\n  \"audio\": {},\n  \"aspect\": {},\n  \"decoder\": {},\n  \"order\": {},\n  \"channels\": {}\n}}\n",
            self.cache_secs, self.deinterlace, self.volume, json_str(&self.audio), json_str(&self.aspect),
            json_str(&self.decoder), json_str(&self.order),
            json_str(&self.channels)
        )
    }

    pub fn save(&self) -> Result<(), String> {
        fs::create_dir_all(data_dir()).map_err(|e| e.to_string())?;
        fs::write(Self::path(), self.to_json()).map_err(|e| io_text(&e, &Self::path()))
    }
}

enum Json {
    Str(String),
    Num(i64),
    Bool(bool),
    Other,
}

fn json_str(s: &str) -> String {
    let mut o = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            c if (c as u32) < 0x20 => o.push_str(&format!("\\u{:04x}", c as u32)),
            c => o.push(c),
        }
    }
    o.push('"');
    o
}

/// Key/value pairs of a flat JSON object (string, integer, bool values; others skipped).
fn json_flat_object(t: &str) -> Vec<(String, Json)> {
    let c: Vec<char> = t.chars().collect();
    let mut i = 0;
    let mut out = Vec::new();
    let ws = |i: &mut usize| while *i < c.len() && c[*i].is_whitespace() { *i += 1 };
    let string = |i: &mut usize| -> Option<String> {
        if c.get(*i) != Some(&'"') {
            return None;
        }
        *i += 1;
        let mut s = String::new();
        while *i < c.len() {
            match c[*i] {
                '"' => { *i += 1; return Some(s); }
                '\\' => {
                    *i += 1;
                    match c.get(*i)? {
                        'n' => s.push('\n'),
                        't' => s.push('\t'),
                        'u' => {
                            let h: String = c.get(*i + 1..*i + 5)?.iter().collect();
                            s.push(char::from_u32(u32::from_str_radix(&h, 16).ok()?)?);
                            *i += 4;
                        }
                        x => s.push(*x),
                    }
                }
                x => s.push(x),
            }
            *i += 1;
        }
        None
    };
    ws(&mut i);
    if c.get(i) != Some(&'{') {
        return out;
    }
    i += 1;
    loop {
        ws(&mut i);
        let Some(k) = string(&mut i) else { break };
        ws(&mut i);
        if c.get(i) != Some(&':') {
            break;
        }
        i += 1;
        ws(&mut i);
        let v = if c.get(i) == Some(&'"') {
            match string(&mut i) { Some(s) => Json::Str(s), None => break }
        } else {
            let st = i;
            while i < c.len() && !matches!(c[i], ',' | '}') && !c[i].is_whitespace() {
                i += 1;
            }
            let tok: String = c[st..i].iter().collect();
            match tok.as_str() {
                "true" => Json::Bool(true),
                "false" => Json::Bool(false),
                n => n.parse().map(Json::Num).unwrap_or(Json::Other),
            }
        };
        out.push((k, v));
        ws(&mut i);
        if c.get(i) == Some(&',') {
            i += 1;
        } else {
            break;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Removing works for all three kinds (playlist file, URL subscription, Xtream account),
    /// in a scratch data dir (H96UI_DATA), never the box's real subscriptions.
    #[test]
    fn remove_each_kind() {
        let d = std::env::temp_dir().join(format!("h96ui-test-{}", std::process::id()));
        let subs = d.join("subs");
        fs::create_dir_all(&subs).unwrap();
        std::env::set_var("H96UI_DATA", &d);
        fs::write(subs.join("Plain list.m3u"), "#EXTM3U\n#EXTINF:-1,A\nhttp://a.example/1\n").unwrap();
        fs::write(subs.join("online.m3u"), "#EXTM3U\n#EXTINF:-1,B\nhttp://b.example/1\n").unwrap();
        fs::write(subs.join("online.url"), "https://b.example/list.m3u\n").unwrap();
        fs::write(subs.join("tv.example 8080.xtream"), "server=http://tv.example:8080\nusername=u\npassword=p\n").unwrap();
        let all = list_subscriptions();
        assert_eq!(all.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(), vec!["online", "Plain list", "tv.example 8080"]);
        assert_eq!(all[0].url, "https://b.example/list.m3u");
        assert!(all[2].xtream.is_some());
        for s in &all {
            s.remove().unwrap();
        }
        assert!(list_subscriptions().is_empty());
        assert_eq!(fs::read_dir(&subs).unwrap().count(), 0, "every file of every kind is gone");
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn settings_roundtrip() {
        let mut s = Settings::default();
        s.audio = "sp\"dif".into();
        s.deinterlace = true;
        s.volume = 60;
        let mut t = Settings::default();
        t.apply_json(&s.to_json());
        assert_eq!(s, t);
    }

    #[test]
    fn settings_partial_keeps_defaults() {
        let mut t = Settings::default();
        t.apply_json("{\"volume\": 90, \"bogus\": [1,2], \"aspect\": \"fill\"}");
        // the array value stops the scan (flat objects only): volume applied, defaults kept
        assert_eq!(t.volume, 90);
        assert_eq!(t.cache_secs, 5);
    }

    #[test]
    fn settings_go_format() {
        let s = Settings::default();
        assert_eq!(s.to_json(), "{\n  \"cache_secs\": 5,\n  \"deinterlace\": false,\n  \"volume\": 80,\n  \"audio\": \"hdmi\",\n  \"aspect\": \"fit\",\n  \"decoder\": \"hardware\",\n  \"order\": \"az\",\n  \"channels\": \"stereo\"\n}\n");
    }
}
