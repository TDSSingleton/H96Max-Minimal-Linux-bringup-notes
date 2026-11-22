//! M3U / M3U8 (extended) playlist parsing: `#EXTINF:-1 tvg-id=".." group-title="..",Name`
//! followed by the stream URL. Plain URL lines without #EXTINF are accepted too.
//!
//! HTTP request headers a channel needs (servers that refuse other clients), from the three
//! places IPTV lists put them:
//!   #EXTINF:-1 http-user-agent="..." http-referrer="...",Name      (attributes)
//!   #EXTVLCOPT:http-user-agent=...   /  #EXTVLCOPT:http-referrer=...   (VLC options)
//!   http://host/stream.m3u8|User-Agent=...&Referer=...               (Kodi URL suffix)

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Channel {
    pub name: String,
    pub group: String,
    pub url: String,
    pub user_agent: String,
    pub referrer: String,
    pub headers: Vec<(String, String)>, // any other request headers (Kodi suffix)
    pub duration: i64, // #EXTINF:<seconds>; -1 (or 0) for live streams
}

impl Channel {
    /// On-demand video (film, episode, clip) rather than a live channel, from what IPTV lists
    /// actually carry: a positive #EXTINF duration, Xtream's /movie/ and /series/ paths, a
    /// video-file extension, or a VOD-ish group name. Everything else counts as live.
    pub fn is_vod(&self) -> bool {
        if self.duration > 0 {
            return true;
        }
        let path = self.url.split(['?', '#']).next().unwrap_or("").to_ascii_lowercase();
        if path.contains("/movie/") || path.contains("/series/") || path.contains("/vod/") {
            return true;
        }
        let ext = path.rsplit('/').next().unwrap_or("").rsplit_once('.').map(|(_, e)| e).unwrap_or("");
        if matches!(ext, "mp4" | "mkv" | "avi" | "mov" | "m4v" | "webm" | "wmv" | "flv" | "mpg" | "mpeg") {
            return true;
        }
        let g = self.group.to_ascii_lowercase();
        g.split(|c: char| !c.is_ascii_alphanumeric())
            .any(|w| matches!(w, "vod" | "movie" | "movies" | "film" | "films" | "series" | "serie" | "cinema"))
    }

    fn set_header(&mut self, name: &str, value: &str) {
        let value = value.trim().to_string();
        match name.trim().to_ascii_lowercase().as_str() {
            "user-agent" | "http-user-agent" => self.user_agent = value,
            "referer" | "referrer" | "http-referrer" | "http-referer" => self.referrer = value,
            "" => {}
            _ => self.headers.push((name.trim().to_string(), value)),
        }
    }
}

/// %XX and '+' decoding for Kodi header values.
fn url_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'%' if i + 2 < b.len() => {
                match u8::from_str_radix(std::str::from_utf8(&b[i + 1..i + 3]).unwrap_or(""), 16) {
                    Ok(v) => {
                        out.push(v);
                        i += 3;
                        continue;
                    }
                    Err(_) => out.push(b'%'),
                }
            }
            b'+' => out.push(b' '),
            c => out.push(c),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Splits Kodi's `URL|Name=value&Name=value` into the URL and its headers.
fn split_kodi(line: &str) -> (&str, Vec<(String, String)>) {
    let Some((url, rest)) = line.split_once('|') else { return (line, Vec::new()) };
    let hs = rest
        .split('&')
        .filter_map(|kv| kv.split_once('='))
        .map(|(k, v)| (url_decode(k), url_decode(v)))
        .collect();
    (url.trim(), hs)
}

pub fn parse_m3u(text: &str) -> Vec<Channel> {
    let mut out = Vec::new();
    let mut pending: Option<Channel> = None;
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        } else if line.starts_with("#EXTINF") {
            let mut c = Channel { duration: -1, ..Default::default() };
            let mut head = line;
            if let Some(i) = first_top_level_comma(line) {
                c.name = line[i + 1..].trim().to_string();
                head = &line[..i];
            }
            // "#EXTINF:-1 attrs..." / "#EXTINF:596,..." -> the number right after the colon
            let num: String = head["#EXTINF".len()..].trim_start_matches(':').trim_start()
                .chars().take_while(|c| c.is_ascii_digit() || *c == '-' || *c == '.').collect();
            c.duration = num.parse::<f64>().map(|d| d as i64).unwrap_or(-1);
            for (k, v) in attrs(head) {
                match k.to_ascii_lowercase().as_str() {
                    "group-title" => c.group = v.to_string(),
                    "tvg-name" if c.name.is_empty() => c.name = v.to_string(),
                    "http-user-agent" | "user-agent" => c.user_agent = v.to_string(),
                    "http-referrer" | "http-referer" | "referrer" | "referer" => c.referrer = v.to_string(),
                    _ => {}
                }
            }
            pending = Some(c);
        } else if let Some(g) = line.strip_prefix("#EXTGRP:") {
            if let Some(p) = pending.as_mut() {
                if p.group.is_empty() {
                    p.group = g.trim().to_string();
                }
            }
        } else if let Some(o) = line.strip_prefix("#EXTVLCOPT:") {
            if let (Some(p), Some((k, v))) = (pending.as_mut(), o.split_once('=')) {
                if matches!(k.trim(), "http-user-agent" | "http-referrer" | "http-referer") {
                    p.set_header(k, v);
                }
            }
        } else if line.starts_with('#') {
            continue;
        } else {
            let (url, kodi) = split_kodi(line);
            let mut c = pending.take().unwrap_or(Channel { duration: -1, ..Default::default() });
            c.url = url.to_string();
            for (k, v) in kodi {
                c.set_header(&k, &v);
            }
            if c.name.is_empty() {
                c.name = c.url.clone();
            }
            out.push(c);
        }
    }
    out
}

/// The comma that separates attributes from the title: the first one outside quotes.
fn first_top_level_comma(s: &str) -> Option<usize> {
    let mut in_q = false;
    for (i, ch) in s.char_indices() {
        match ch {
            '"' => in_q = !in_q,
            ',' if !in_q => return Some(i),
            _ => {}
        }
    }
    None
}

fn is_key_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b == b'-'
}

/// All `key="value"` pairs, scanned left to right without overlap
/// (same as the regex `([A-Za-z0-9_-]+)="([^"]*)"`).
fn attrs(s: &str) -> Vec<(&str, &str)> {
    let b = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        if !is_key_byte(b[i]) {
            i += 1;
            continue;
        }
        let ks = i;
        while i < b.len() && is_key_byte(b[i]) {
            i += 1;
        }
        let ke = i;
        if b.get(i) == Some(&b'=') && b.get(i + 1) == Some(&b'"') {
            let vs = i + 2;
            if let Some(len) = b[vs..].iter().position(|&c| c == b'"') {
                out.push((&s[ks..ke], &s[vs..vs + len]));
                i = vs + len + 1;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ch(n: &str, g: &str, u: &str) -> Channel {
        Channel { name: n.into(), group: g.into(), url: u.into(), duration: -1, ..Default::default() }
    }

    #[test]
    fn vod() {
        let inp = "#EXTM3U\n\
            #EXTINF:-1 group-title=\"News\",Live one\nhttp://x.example/live/u/p/1.ts\n\
            #EXTINF:5400,Film by duration\nhttp://x.example/a.m3u8\n\
            #EXTINF:-1,Xtream movie\nhttp://x.example:8080/movie/u/p/77.mkv\n\
            #EXTINF:-1 group-title=\"VOD | Action\",By group\nhttp://x.example/s/9\n\
            #EXTINF:-1 group-title=\"Filmography News\",Not a film group\nhttp://x.example/b.m3u8\n\
            #EXTINF:-1,File\nhttps://x.example/clip.mp4?token=1\n";
        let v: Vec<bool> = parse_m3u(inp).iter().map(|c| c.is_vod()).collect();
        assert_eq!(v, vec![false, true, true, true, false, true]);
        assert_eq!(parse_m3u(inp)[1].duration, 5400);
    }

    #[test]
    fn headers() {
        let inp = "#EXTM3U\n\
            #EXTINF:-1 http-referrer=\"http://r.example/\" http-user-agent=\"UA one\",Attr\n\
            http://a.example/1.m3u8\n\
            #EXTINF:-1,Vlc\n#EXTVLCOPT:http-referrer=http://v.example/\n\
            #EXTVLCOPT:http-user-agent=Mozilla/5.0 (X11)\nhttp://a.example/2.m3u8\n\
            #EXTINF:-1,Kodi\n\
            http://a.example/3.ts|User-Agent=Kodi%2F21&Referer=http%3A%2F%2Fk.example%2F&X-Token=abc\n\
            #EXTINF:-1,Plain\nhttp://a.example/4.ts\n";
        let c = parse_m3u(inp);
        assert_eq!((c[0].user_agent.as_str(), c[0].referrer.as_str()), ("UA one", "http://r.example/"));
        assert_eq!((c[1].user_agent.as_str(), c[1].referrer.as_str()), ("Mozilla/5.0 (X11)", "http://v.example/"));
        assert_eq!(c[2].url, "http://a.example/3.ts");
        assert_eq!((c[2].user_agent.as_str(), c[2].referrer.as_str()), ("Kodi/21", "http://k.example/"));
        assert_eq!(c[2].headers, vec![("X-Token".to_string(), "abc".to_string())]);
        assert_eq!(c[3], ch("Plain", "", "http://a.example/4.ts"));
        // headers never leak into the next channel
        assert!(c[3].user_agent.is_empty() && c[3].referrer.is_empty());
    }

    #[test]
    fn parse() {
        let inp = "\u{feff}#EXTM3U\n\
            #EXTINF:-1 tvg-id=\"a\" group-title=\"News, World\",Channel One, HD\n\
            http://example.invalid/one.m3u8\n\n\
            #EXTINF:-1,Plain Two\n#EXTGRP:Music\nhttps://example.invalid/two\n\
            http://example.invalid/three\n\
            #EXTINF:-1 tvg-name=\"Four\" group-title=\"Kids\",\nhttp://example.invalid/four\n";
        let got = parse_m3u(inp);
        assert_eq!(got, vec![
            ch("Channel One, HD", "News, World", "http://example.invalid/one.m3u8"),
            ch("Plain Two", "Music", "https://example.invalid/two"),
            ch("http://example.invalid/three", "", "http://example.invalid/three"),
            ch("Four", "Kids", "http://example.invalid/four"),
        ]);
    }
}
