//! Xtream Codes accounts (the API most IPTV panels speak), NOTES §38.
//!
//! An account is a server ("http://host:port"), a username and a password. h96ui learns it
//! from a file on the USB stick holding either the provider's link
//!   http://host:port/get.php?username=U&password=P&type=m3u_plus&output=ts
//!   http://host:port/player_api.php?username=U&password=P
//! or three lines (":" or "=" both work; "user_agent" is optional)
//!   server: http://host:port
//!   username: U
//!   password: P
//! It is stored as subs/<name>.xtream (mode 0600: the password is in it).
//!
//! API: GET {server}/player_api.php?username=U&password=P[&action=...]
//!   (none)                     -> {user_info:{auth,status,exp_date,max_connections,
//!                                  active_cons,allowed_output_formats}, server_info:{...}}
//!   get_live_categories / get_vod_categories / get_series_categories
//!                              -> [{category_id, category_name}]
//!   get_live_streams[&category_id=C] -> [{stream_id, name, category_id, ...}]
//!   get_vod_streams[&category_id=C]  -> [{stream_id, name, container_extension, ...}]
//!   get_series[&category_id=C]       -> [{series_id, name, ...}]
//!   get_series_info&series_id=S      -> {episodes:{"<season>":[{id, episode_num, title,
//!                                        container_extension, info:{duration_secs}}]}}
//! Streams: {server}/live/U/P/<id>.<m3u8|ts>, {server}/movie/U/P/<id>.<ext>,
//!          {server}/series/U/P/<episode id>.<ext>

use crate::json::{self, Value};
use crate::m3u::Channel;

/// Many panels refuse players they do not recognise; this is what common IPTV apps send.
/// A "user_agent:" line in the account file overrides it.
pub const DEFAULT_UA: &str = "VLC/3.0.21 LibVLC/3.0.21";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Account {
    pub server: String, // scheme://host[:port], no trailing slash
    pub username: String,
    pub password: String,
    pub user_agent: String,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Live,
    Movies,
    Series,
}

impl Kind {
    fn categories_action(self) -> &'static str {
        match self {
            Kind::Live => "get_live_categories",
            Kind::Movies => "get_vod_categories",
            Kind::Series => "get_series_categories",
        }
    }
    fn list_action(self) -> &'static str {
        match self {
            Kind::Live => "get_live_streams",
            Kind::Movies => "get_vod_streams",
            Kind::Series => "get_series",
        }
    }
}

/// Percent-encoding for query values.
fn enc(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

fn dec(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Ok(v) = u8::from_str_radix(std::str::from_utf8(&b[i + 1..i + 3]).unwrap_or(""), 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(if b[i] == b'+' { b' ' } else { b[i] });
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

impl Account {
    /// An account from a provider link (get.php / player_api.php / xmltv.php with username and
    /// password in the query).
    pub fn from_url(u: &str) -> Option<Account> {
        let u = u.trim();
        let (scheme, rest) = u.split_once("://")?;
        if scheme != "http" && scheme != "https" {
            return None;
        }
        let (hostpath, query) = rest.split_once('?')?;
        let (host, path) = hostpath.split_once('/').unwrap_or((hostpath, ""));
        let file = path.rsplit('/').next().unwrap_or("");
        if !matches!(file, "get.php" | "player_api.php" | "xmltv.php" | "panel_api.php") || host.is_empty() {
            return None;
        }
        let (mut user, mut pass) = (None, None);
        for kv in query.split('&') {
            match kv.split_once('=') {
                Some(("username", v)) => user = Some(dec(v)),
                Some(("password", v)) => pass = Some(dec(v)),
                _ => {}
            }
        }
        let prefix = path.rsplit_once('/').map(|(p, _)| format!("/{p}")).unwrap_or_default();
        Some(Account {
            server: format!("{scheme}://{host}{prefix}"),
            username: user.filter(|s| !s.is_empty())?,
            password: pass.filter(|s| !s.is_empty())?,
            user_agent: DEFAULT_UA.into(),
        })
    }

    /// An account from a text file: the first provider link in it, or server/username/password
    /// lines. None if the text holds neither.
    pub fn from_text(t: &str) -> Option<Account> {
        let t = t.trim_start_matches('\u{feff}');
        let (mut server, mut user, mut pass, mut ua) = (None, None, None, None);
        for l in t.lines() {
            let l = l.trim();
            if l.starts_with('#') || l.is_empty() {
                continue;
            }
            if let Some(a) = Account::from_url(l) {
                return Some(a);
            }
            let Some((k, v)) = l.split_once([':', '=']) else { continue };
            let v = v.trim().to_string();
            match k.trim().to_ascii_lowercase().as_str() {
                "server" | "url" | "host" | "portal" | "server url" => server = Some(v),
                "username" | "user" | "login" => user = Some(v),
                "password" | "pass" => pass = Some(v),
                "user_agent" | "user-agent" | "useragent" => ua = Some(v),
                _ => {}
            }
        }
        let mut server = server.filter(|s| !s.is_empty())?;
        if !server.contains("://") {
            server = format!("http://{server}");
        }
        let server = server.trim_end_matches('/').to_string();
        let server = server.strip_suffix("/player_api.php").or(server.strip_suffix("/get.php")).unwrap_or(&server).to_string();
        Some(Account {
            server,
            username: user.filter(|s| !s.is_empty())?,
            password: pass.filter(|s| !s.is_empty())?,
            user_agent: ua.filter(|s| !s.is_empty()).unwrap_or_else(|| DEFAULT_UA.into()),
        })
    }

    pub fn to_file(&self) -> String {
        format!(
            "# h96ui Xtream account (mode 0600)\nserver={}\nusername={}\npassword={}\nuser_agent={}\n",
            self.server, self.username, self.password, self.user_agent
        )
    }

    /// "host" or "host:port" of the server, for naming the subscription.
    pub fn host(&self) -> String {
        let h = self.server.split_once("://").map(|(_, r)| r).unwrap_or(&self.server);
        h.split('/').next().unwrap_or(h).to_string()
    }

    /// Replaces the username and password in text that may be shown on the TV.
    pub fn redact(&self, s: &str) -> String {
        let mut s = s.replace(&self.password, "***");
        if self.username.len() > 2 {
            s = s.replace(&self.username, "user");
        }
        s
    }

    fn api(&self, params: &str) -> Result<Value, String> {
        let url = format!(
            "{}/player_api.php?username={}&password={}{params}",
            self.server,
            enc(&self.username),
            enc(&self.password)
        );
        let body = crate::store::fetch_with(&url, &self.user_agent).map_err(|e| self.redact(&e))?;
        let text = String::from_utf8_lossy(&body);
        if text.trim().is_empty() {
            return Err("the server sent an empty answer (wrong username/password?)".into());
        }
        json::parse(&text).map_err(|e| format!("the server's answer is not Xtream JSON ({e})"))
    }

    /// Logs in: account details, or why the server refused.
    pub fn info(&self) -> Result<Info, String> {
        let v = self.api("")?;
        let ui = v.get("user_info");
        if ui.is_null() {
            return Err("not an Xtream server (no user_info in the answer)".into());
        }
        if ui.get("auth").num() != Some(1.0) {
            return Err("login refused: check the username and password".into());
        }
        let status = ui.get("status").str();
        let formats: Vec<String> = ui.get("allowed_output_formats").items().iter().map(|f| f.str()).collect();
        Ok(Info {
            status,
            exp_date: ui.get("exp_date").num().map(|n| n as i64),
            max_connections: ui.get("max_connections").str(),
            active_cons: ui.get("active_cons").str(),
            is_trial: ui.get("is_trial").str() == "1",
            live_ext: if formats.is_empty() || formats.iter().any(|f| f == "m3u8") { "m3u8".into() } else { formats[0].clone() },
            timezone: v.get("server_info").get("timezone").str(),
        })
    }

    pub fn categories(&self, kind: Kind) -> Result<Vec<(String, String)>, String> {
        let v = self.api(&format!("&action={}", kind.categories_action()))?;
        Ok(v.items()
            .iter()
            .map(|c| (c.get("category_id").str(), c.get("category_name").str()))
            .filter(|(id, _)| !id.is_empty())
            .collect())
    }

    fn list(&self, kind: Kind, cat: Option<&str>) -> Result<Vec<Value>, String> {
        let c = cat.map(|c| format!("&category_id={}", enc(c))).unwrap_or_default();
        Ok(self.api(&format!("&action={}{c}", kind.list_action()))?.items().into_iter().cloned().collect())
    }

    fn channel(&self, name: String, group: String, url: String, duration: i64) -> Channel {
        Channel { name, group, url, user_agent: self.user_agent.clone(), duration, ..Default::default() }
    }

    /// Live channels (kind Live) or films (kind Movies) of one category, or all (cat None).
    pub fn streams(&self, kind: Kind, cat: Option<&str>, group: &str, live_ext: &str) -> Result<Vec<Channel>, String> {
        let (u, p) = (&self.username, &self.password);
        Ok(self
            .list(kind, cat)?
            .iter()
            .filter_map(|s| {
                let id = s.get("stream_id").str();
                if id.is_empty() {
                    return None;
                }
                let name = s.get("name").str();
                let url = match kind {
                    Kind::Live => format!("{}/live/{u}/{p}/{id}.{live_ext}", self.server),
                    _ => {
                        let ext = s.get("container_extension").str();
                        let ext = if ext.is_empty() { "mp4".to_string() } else { ext };
                        format!("{}/movie/{u}/{p}/{id}.{ext}", self.server)
                    }
                };
                Some(self.channel(name, group.to_string(), url, if kind == Kind::Live { -1 } else { 0 }))
            })
            .collect())
    }

    /// Series of one category (or all): (series_id, name).
    pub fn series(&self, cat: Option<&str>) -> Result<Vec<(String, String)>, String> {
        Ok(self
            .list(Kind::Series, cat)?
            .iter()
            .map(|s| (s.get("series_id").str(), s.get("name").str()))
            .filter(|(id, _)| !id.is_empty())
            .collect())
    }

    /// Episodes of a series, by season (sorted): [(season, [episode])].
    pub fn episodes(&self, series_id: &str, series_name: &str) -> Result<Vec<(String, Vec<Channel>)>, String> {
        let v = self.api(&format!("&action=get_series_info&series_id={}", enc(series_id)))?;
        let (u, p) = (&self.username, &self.password);
        let eps = v.get("episodes");
        // {"1":[...],"2":[...]} usually; some panels send [[...],[...]]
        let seasons: Vec<(String, Vec<&Value>)> = match eps {
            Value::Obj(m) => m.iter().map(|(k, v)| (k.clone(), v.items())).collect(),
            Value::Arr(a) => a
                .iter()
                .map(|s| {
                    let items = s.items();
                    let n = items.first().map(|e| e.get("season").str()).unwrap_or_default();
                    (n, items)
                })
                .collect(),
            _ => Vec::new(),
        };
        let mut out: Vec<(String, Vec<Channel>)> = seasons
            .into_iter()
            .map(|(season, items)| {
                let mut list: Vec<(f64, Channel)> = items
                    .iter()
                    .filter_map(|e| {
                        let id = e.get("id").str();
                        if id.is_empty() {
                            return None;
                        }
                        let ext = e.get("container_extension").str();
                        let ext = if ext.is_empty() { "mp4".to_string() } else { ext };
                        let num = e.get("episode_num").num().unwrap_or(0.0);
                        let title = e.get("title").str();
                        let name = if title.is_empty() { format!("Episode {num}") } else { title };
                        let dur = e.get("info").get("duration_secs").num().unwrap_or(0.0) as i64;
                        let url = format!("{}/series/{u}/{p}/{id}.{ext}", self.server);
                        Some((num, self.channel(name, format!("{series_name} S{season}"), url, dur.max(0))))
                    })
                    .collect();
                list.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
                (season, list.into_iter().map(|(_, c)| c).collect())
            })
            .filter(|(_, l): &(String, Vec<Channel>)| !l.is_empty())
            .collect();
        out.sort_by_key(|(s, _)| s.parse::<i64>().unwrap_or(i64::MAX));
        Ok(out)
    }
}

pub struct Info {
    pub status: String,
    pub exp_date: Option<i64>,
    pub max_connections: String,
    pub active_cons: String,
    pub is_trial: bool,
    pub live_ext: String, // "m3u8" when the panel allows HLS, else its first format ("ts")
    pub timezone: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accounts() {
        let a = Account::from_url("http://tv.example:8080/get.php?username=bob&password=p%40ss&type=m3u_plus&output=ts").unwrap();
        assert_eq!((a.server.as_str(), a.username.as_str(), a.password.as_str()), ("http://tv.example:8080", "bob", "p@ss"));
        let b = Account::from_url("https://tv.example/sub/player_api.php?password=x&username=y").unwrap();
        assert_eq!(b.server, "https://tv.example/sub");
        assert!(Account::from_url("http://tv.example/live/a/b/1.ts").is_none());
        assert!(Account::from_url("http://tv.example/get.php?username=&password=x").is_none());
        let c = Account::from_text("Server URL: tv.example:25461/\nUsername = bob\npassword: s3cret\nuser_agent: MyApp/1\n").unwrap();
        assert_eq!((c.server.as_str(), c.username.as_str(), c.password.as_str(), c.user_agent.as_str()),
            ("http://tv.example:25461", "bob", "s3cret", "MyApp/1"));
        assert_eq!(c.host(), "tv.example:25461");
        let d = Account::from_text(&c.to_file()).unwrap();
        assert_eq!(c, d);
        assert!(Account::from_text("https://example.invalid/list.m3u\n").is_none());
        assert_eq!(c.redact("http://tv.example/live/bob/s3cret/1.ts"), "http://tv.example/live/user/***/1.ts");
    }
}
