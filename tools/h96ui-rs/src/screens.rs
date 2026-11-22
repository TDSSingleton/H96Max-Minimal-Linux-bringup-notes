//! The menu screens. Each screen is data (items, lines) plus an optional refresh closure that
//! rebuilds it whenever it is shown again.

use crate::store::{self, Subscription};
use crate::sys::{self, ImportKind, Importable};
use crate::{play, App, Item, Screen};
use std::rc::Rc;

pub fn main_menu() -> Screen {
    let mut s = Screen::titled("H96Max");
    s.refresh = Some(Rc::new(|_a: &mut App, s: &mut Screen| {
        let n = store::list_subscriptions().len();
        s.items = vec![
            Item::new("Subscriptions", n.to_string(), |a| a.push(subs_screen())),
            Item::new("Add subscription from USB", "", |a| a.push(usb_screen())),
            Item::new("Settings", "", |a| a.push(settings_screen())),
        ];
    }));
    s
}

fn subs_screen() -> Screen {
    let mut s = Screen::titled("Subscriptions");
    s.empty = "No subscriptions yet. Use 'Add subscription from USB'.".into();
    s.refresh = Some(Rc::new(|_a: &mut App, s: &mut Screen| {
        s.items = store::list_subscriptions()
            .into_iter()
            .map(|sub| {
                let mut right = if sub.url.is_empty() { "file" } else { "url" }.to_string();
                if let Ok(ch) = sub.channels() {
                    right = format!("{} channels  {right}", ch.len());
                }
                let label = sub.name.clone();
                Item::new(label, right, move |a| a.push(sub_actions(sub.clone())))
            })
            .collect();
    }));
    s
}

fn sub_actions(sub: Subscription) -> Screen {
    let mut s = Screen::titled(sub.name.clone());
    // live channels and on-demand videos get separate lists (m3u.rs Channel::is_vod)
    let (mut live, mut vod) = (0, 0);
    if let Ok(ch) = sub.channels() {
        for c in &ch {
            if c.is_vod() { vod += 1 } else { live += 1 }
        }
    }
    let c = sub.clone();
    s.items.push(Item::new("Live channels", live.to_string(), move |a| a.push(channels_screen(c.clone(), false))));
    let c = sub.clone();
    s.items.push(Item::new("Videos", vod.to_string(), move |a| a.push(channels_screen(c.clone(), true))));
    if !sub.url.is_empty() {
        let c = sub.clone();
        s.items.push(Item::new("Refresh from URL", short_url(&sub.url), move |a| {
            a.say(format!("Downloading {} ...", short_url(&c.url)));
            a.draw();
            match c.refresh() {
                Ok(n) => a.say(format!("Refreshed: {n} channels")),
                Err(e) => a.say(format!("Refresh failed: {e}")),
            }
        }));
    }
    let c = sub.clone();
    s.items.push(Item::new("Remove", "", move |a| {
        let mut confirm = Screen::titled(format!("Remove {}?", c.name));
        let c2 = c.clone();
        confirm.items = vec![
            Item::new("No, keep it", "", |a| a.pop()),
            Item::new("Yes, remove it", "", move |a| {
                match c2.remove() {
                    Ok(()) => a.say(format!("Removed {}", c2.name)),
                    Err(e) => a.say(format!("Remove failed: {e}")),
                }
                a.pop();
                a.pop();
            }),
        ];
        a.push(confirm);
    }));
    s
}

fn short_url(u: &str) -> String {
    let u = u.strip_prefix("https://").unwrap_or(u);
    let u = u.strip_prefix("http://").unwrap_or(u);
    if u.len() > 40 {
        let mut cut = 37;
        while !u.is_char_boundary(cut) {
            cut -= 1;
        }
        return format!("{}...", &u[..cut]);
    }
    u.to_string()
}

/// One list: the live channels or the on-demand videos of a subscription.
fn channels_screen(sub: Subscription, vod: bool) -> Screen {
    let mut s = Screen::titled(if vod { "Videos" } else { "Live channels" });
    s.empty = if vod { "No on-demand videos in this playlist." } else { "No live channels in this playlist." }.into();
    let all = match sub.channels() {
        Ok(ch) => ch,
        Err(e) => {
            s.empty = format!("Cannot read the playlist: {e}");
            Vec::new()
        }
    };
    // (index in the whole playlist for mpv's --playlist-start, channel)
    let picked: Vec<(usize, crate::m3u::Channel)> =
        all.into_iter().enumerate().filter(|(_, c)| c.is_vod() == vod).collect();
    let list = Rc::new(picked.iter().map(|(_, c)| c.clone()).collect::<Vec<_>>());
    let sub = Rc::new(sub);
    for (i, (orig, c)) in picked.into_iter().enumerate() {
        let (sub, list) = (sub.clone(), list.clone());
        let right = if vod && c.duration > 0 {
            format!("{}  {}", c.group, fmt_duration(c.duration))
        } else {
            c.group.clone()
        };
        s.items.push(Item::new(c.name, right.trim().to_string(), move |a| {
            if a.settings.decoder == "software" {
                if let Err(e) = a.run_external(|a| play::play(a, &sub, orig)) {
                    a.say(format!("Playback: {e}"));
                }
                return;
            }
            match play::play_hw(a, &list, i, vod) {
                Ok(msg) if !msg.is_empty() => a.say(msg),
                Ok(_) => {}
                Err(e) => a.say(format!("Playback: {e}")),
            }
            a.redraw_all();
        }));
    }
    s
}

fn fmt_duration(secs: i64) -> String {
    if secs >= 3600 { format!("{}h{:02}", secs / 3600, secs / 60 % 60) } else { format!("{}m", (secs + 59) / 60) }
}

fn usb_screen() -> Screen {
    let mut s = Screen::titled("Add from USB");
    s.usb_live = true; // the main loop refreshes it when a stick is plugged in or pulled out
    s.on_back = Some(Rc::new(|_a: &mut App| sys::unmount_usb()));
    s.refresh = Some(Rc::new(|_a: &mut App, s: &mut Screen| {
        let keep = s.sel;
        s.items.clear();
        s.lines.clear();
        let dev = match sys::mount_usb() {
            Ok(Some(dev)) => dev,
            Ok(None) => {
                s.lines = vec![
                    "Plug in a USB stick (FAT32, exFAT, NTFS or ext4).".into(),
                    String::new(),
                    "Put .m3u/.m3u8 playlists, or .txt files with one playlist URL per line,".into(),
                    "in the stick's top folder or in a folder named iptv.".into(),
                ];
                return;
            }
            Err(e) => {
                s.lines = vec![format!("Cannot read the USB stick: {e}"), String::new()];
                s.items = vec![Item::new("Retry", "", |a| a.refresh_top())];
                return;
            }
        };
        let files = sys::find_importables();
        s.lines = vec![
            format!("USB stick {dev}: .m3u/.m3u8 playlists, and .txt/.url files with playlist URLs"),
            String::new(),
        ];
        if files.is_empty() {
            s.lines.push("Nothing importable in / or /iptv on the stick.".into());
        }
        for f in files {
            let p = f.path.to_string_lossy();
            let rel = p.strip_prefix(&format!("{}/", sys::USB_MOUNT)).unwrap_or(&p).to_string();
            let right = match f.kind {
                ImportKind::Playlist => {
                    let have = std::fs::read(&f.path).ok().and_then(|b| store::subscription_with_content(&b));
                    if have.is_some() { "playlist, imported".to_string() } else { "playlist".to_string() }
                }
                ImportKind::Urls => {
                    let urls = store::urls_in(&f.path);
                    let have = urls.iter().filter(|u| store::subscription_with_url(u).is_some()).count();
                    if have == 0 {
                        format!("{} URL(s)", urls.len())
                    } else {
                        format!("{} URL(s), {have} imported", urls.len())
                    }
                }
            };
            s.items.push(Item::new(rel, right, move |a| {
                import_one(a, &f);
                a.refresh_top(); // update the "imported" markers
            }));
        }
        s.sel = keep.min(s.items.len().saturating_sub(1));
    }));
    s
}

fn import_one(a: &mut App, f: &Importable) {
    if f.kind == ImportKind::Playlist {
        match store::import_file(&f.path) {
            Ok(store::Imported::New(name, n)) => a.say(format!("Imported '{name}': {n} channels")),
            Ok(store::Imported::Already(name)) => a.say(format!("Already imported as '{name}'")),
            Err(e) => a.say(format!("Import failed: {e}")),
        }
        return;
    }
    let (mut ok, mut had, mut bad) = (0, 0, 0);
    let mut last = String::new();
    for u in store::urls_in(&f.path) {
        a.say(format!("Downloading {} ...", short_url(&u)));
        a.draw();
        match store::import_url(&u) {
            Ok(store::Imported::New(..)) => ok += 1,
            Ok(store::Imported::Already(_)) => had += 1,
            Err(e) => {
                bad += 1;
                last = e;
            }
        }
    }
    let mut m = format!("Imported {ok} URL subscription(s)");
    if had > 0 {
        m += &format!(", {had} already there");
    }
    if bad > 0 {
        m += &format!(", {bad} failed (last error: {last})");
    }
    a.say(m);
}

fn cycle<T: PartialEq + Clone>(cur: &T, opts: &[T]) -> T {
    match opts.iter().position(|o| o == cur) {
        Some(i) => opts[(i + 1) % opts.len()].clone(),
        None => opts[0].clone(),
    }
}

/// A settings item: `change` edits the settings, then they are saved and the screen redrawn.
fn setting(label: &str, right: String, change: impl Fn(&mut store::Settings) + 'static) -> Item {
    Item::new(label, right, move |a| {
        change(&mut a.settings);
        if let Err(e) = a.settings.save() {
            a.say(format!("Could not save settings: {e}"));
        }
        a.refresh_top();
    })
}

fn settings_screen() -> Screen {
    let mut s = Screen::titled("Settings");
    s.refresh = Some(Rc::new(|a: &mut App, s: &mut Screen| {
        let st = &a.settings;
        let onoff = |b: bool| if b { "on" } else { "off" }.to_string();
        s.items = vec![
            setting("Network cache", format!("{} s", st.cache_secs), |st| {
                st.cache_secs = cycle(&st.cache_secs, &[2, 5, 10, 20])
            }),
            setting("Deinterlace", onoff(st.deinterlace), |st| st.deinterlace = !st.deinterlace),
            setting("Start volume", st.volume.to_string(), |st| {
                st.volume = cycle(&st.volume, &[50, 60, 70, 80, 90, 100])
            }),
            setting("Audio output", st.audio.to_uppercase(), |st| {
                st.audio = cycle(&st.audio, &["hdmi".into(), "spdif".into(), "analog".into()])
            }),
            setting("Picture", st.aspect.clone(), |st| {
                st.aspect = cycle(&st.aspect, &["fit".into(), "stretch".into(), "fill".into()])
            }),
            setting(
                "Video decoding",
                if st.decoder == "software" { "software (mpv)".into() } else { "hardware (VPU)".into() },
                |st| st.decoder = cycle(&st.decoder, &["hardware".into(), "software".into()]),
            ),
            Item::new("About this box", "", |a| a.push(about_screen())),
        ];
    }));
    s
}

/// Go's time.Duration.String() for whole seconds: "1h2m3s", "4m0s", "7s".
fn go_duration(secs: u64) -> String {
    let (h, m, s) = (secs / 3600, secs / 60 % 60, secs % 60);
    if h > 0 {
        format!("{h}h{m}m{s}s")
    } else if m > 0 {
        format!("{m}m{s}s")
    } else {
        format!("{s}s")
    }
}

fn about_screen() -> Screen {
    let mut s = Screen::titled("About");
    s.refresh = Some(Rc::new(|_a: &mut App, s: &mut Screen| {
        let mut up = sys::read_first_line("/proc/uptime");
        if let Some(sec) = up.split_whitespace().next().and_then(|f| f.parse::<f64>().ok()) {
            up = go_duration(sec as u64);
        }
        let mut temp = sys::read_first_line("/sys/class/thermal/thermal_zone0/temp");
        if let Ok(v) = temp.parse::<i64>() {
            temp = format!("{} C", v / 1000);
        }
        let mut mem = String::new();
        if let Ok(t) = std::fs::read_to_string("/proc/meminfo") {
            let field = |k: &str| {
                t.lines()
                    .find_map(|l| l.strip_prefix(k))
                    .and_then(|r| r.split_whitespace().next()?.parse::<i64>().ok())
                    .unwrap_or(0)
            };
            let (tot, av) = (field("MemTotal:"), field("MemAvailable:"));
            mem = format!("{} / {} MiB used", (tot - av) / 1024, tot / 1024);
        }
        let mut emmc = String::new();
        let mut blks: Vec<_> = std::fs::read_dir("/sys/block")
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.path())
            .filter(|p| {
                let n = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                n.strip_prefix("mmcblk").is_some_and(|r| r.len() == 1 && r.as_bytes()[0].is_ascii_digit())
            })
            .collect();
        blks.sort();
        for b in blks {
            if sys::read_first_line(b.join("device/type")) == "MMC" {
                emmc = format!(
                    "life_time {}  pre_eol {}",
                    sys::read_first_line(b.join("device/life_time")),
                    sys::read_first_line(b.join("device/pre_eol_info"))
                );
            }
        }
        let w = sys::read_wifi();
        let model = sys::read_first_line("/proc/device-tree/model");
        s.lines = vec![
            format!("Model      {}", model.trim_end_matches('\0')),
            format!(
                "Software   h96ui {} ({}), Alpine {}, Linux {}",
                crate::VERSION,
                crate::BUILD_DATE,
                sys::alpine_release(),
                sys::kernel_release()
            ),
            format!("Uptime     {up}"),
            format!("CPU temp   {temp}"),
            format!("Memory     {mem}"),
            format!("eMMC       {emmc}"),
            format!("WiFi       {}  {}  {}", w.ssid, w.signal, w.ip),
            format!("Data       {}", store::data_dir().display()),
        ];
    }));
    s
}

/// Navigates to a named screen for -snapshot (layout checks without the TV).
pub fn go_snapshot(a: &mut App, name: &str) {
    match name {
        "subs" => a.push(subs_screen()),
        "channels" => {
            if let Some(sub) = store::list_subscriptions().into_iter().next() {
                a.push(subs_screen());
                a.push(channels_screen(sub, false));
            }
        }
        "settings" => a.push(settings_screen()),
        "about" => {
            a.push(settings_screen());
            a.push(about_screen());
        }
        "usb" => a.push(usb_screen()),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn durations() {
        assert_eq!(go_duration(0), "0s");
        assert_eq!(go_duration(59), "59s");
        assert_eq!(go_duration(240), "4m0s");
        assert_eq!(go_duration(3725), "1h2m5s");
    }

    #[test]
    fn urls() {
        assert_eq!(short_url("https://example.invalid/a"), "example.invalid/a");
        assert_eq!(short_url(&format!("http://{}", "x".repeat(50))), format!("{}...", "x".repeat(37)));
    }
}
