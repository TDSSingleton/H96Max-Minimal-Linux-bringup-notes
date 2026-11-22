# Troubleshooting: symptom → cause → fix

Part of the session log; index in [README.md](README.md). Every failure seen so far on the box,
with its cause and the fix. Most fixes are one command from the **fixup kit**
(`tools/fixup/`, installed on the box as `/usr/local/sbin/h96-doctor` and `h96-fix`).

**First step, always:** `H96=192.168.11.149 tools/h96 h96-doctor`. It changes nothing; each
problem line names its fix. Then `h96-fix <problem>`. With no network at all, plug the
Realtek USB-Ethernet dongle into the USB-A port (`H96=192.168.11.145`). A USB keyboard
also works: tty2 runs a getty and root has an **empty password** (`shadow: root:` in the rootfs
build log) -- local recovery is easy, ssh stays key-only; setting a password is an open item.

| Tool | Where | What |
|---|---|---|
| `h96-doctor [-q]` | box | read-only health check (UI, player, GStreamer, kmssink patch, MPP, video plane, console size, WiFi, BT, remote, clock, disk, Xtream file modes) |
| `h96-fix player` | box | stop stuck players, restart h96ui |
| `h96-fix display` | box | HDMI re-detect, console to the HDMI mode + 16x32 font, restart h96ui |
| `h96-fix ui` / `ui-rollback` | box | restart h96ui / swap in the previous build (`h96ui.prev`) |
| `h96-fix kmssink [alpine]` | box | re-install our patched kmssink / put Alpine's back |
| `h96-fix usb` | box | unmount a stick left at `/mnt/h96usb` |
| `h96-fix wifi-reorder` | box | turn off the WiFi driver's rx reorder buffer (streams from distant servers crawl) |
| `h96-fix wifi-bt` | box | reset the WiFi/BT chip without a power cycle (detaches itself) |
| `tools/fixup/push-to-box.sh` | PC | install the current h96ui, video stack and fixup kit over ssh |
| `tools/video/seek-test.sh URL` | box | 8 seeks through `h96player -i`; reports a hung main loop |
| `tools/video/seg-speed.sh URL…` | box | HLS segment throughput per server (slow server vs. slow box) |
| `h96ui -xtream-check FILE` | box | log in to an Xtream account and list categories (prints no password) |
| `tools/h96ui-rs/testdata/mock-xtream.py` | PC | fake Xtream panel (demo/demo123) for testing without a real account |

## Picture and playback

| Symptom | Cause | Fix |
|---|---|---|
| Frozen picture over the menu; remote does nothing | a player still holds the video plane (crashed/hung h96player, an old mpv) | `h96-fix player` |
| Stuck in a stream, Back does nothing (old builds) | Go h96ui + mpv: the remote's Back (KEY_BACK) never reaches the console, mpv only quits on Esc | `push-to-box.sh` (Rust h96ui reads Back via evdev); now: `h96-fix player` |
| OK button does nothing | KEY_SELECT (353) is outside the console keymap (0–255); only h96ui ≥ 0.4.0 reads it from evdev | `push-to-box.sh` |
| Screen blinks green when video starts | video put on a sub-window of the console's layer (`Esmart0-win1..3`): they share its XR24 format and scaler | use plane **172** (Esmart1-win0); h96player defaults to it (`-p`) |
| SD (576p) channel: "GStreamer resource error", log `drmModeSetPlane failed: No space left on device` | kmssink 1.28 bug: uses the pixel-aspect-corrected width (1024) as the source rectangle of a 720x576 frame | patched kmssink: `h96-fix kmssink`; after an apk upgrade of gst-plugins-bad rebuild it (`STAGES=kms`, bump `GPB_VER/GPB_SHA`) |
| Every channel fails with "h96player: No such file or directory" | busybox init starts h96ui with `PATH=/sbin:/usr/sbin:/bin:/usr/bin` | h96ui ≥ 0.3.2 adds `/usr/local/bin`; h96player now lives in `/usr/bin` |
| **No live channel plays** (stays on Loading/Buffering, 0 frames), nearby CDNs fine | the WiFi driver's rx reorder buffer is on: distant servers crawl at ~0.3 Mbit/s (NOTES §40) | `h96-fix wifi-reorder`; `h96-wifi` does it at boot (check: `h96-doctor`) |
| Picture freezes every few seconds on some channels | the server sends too slowly (185.9.2.18: ~12 Mbit/s per connection for an 8.8 Mbit/s stream) -- first rule out the reorder buffer above | Settings → Network cache 10 s (or 20); compare `seg-speed.sh` on the box with the same segment on the PC (`curl -w %{speed_download}`) |
| Seeking freezes the video and the player ignores stop | playbin deadlock: post-seek pad re-creation vs. a query on playbin (NOTES §37) | fixed in h96player (playbin3, async state changes, no queries during seeks); `h96-fix player` frees a hung one |
| "Stopped: …" right after Loading on a channel that needs special headers | server wants the playlist's User-Agent/Referer | h96ui ≥ 0.3.1 sends `http-user-agent`/`http-referrer`/`#EXTVLCOPT`/Kodi `|Header=` values |
| Combing on motion (SD channels) | interlaced source, shown woven (no deinterlacer yet) | open item (IEP2 hardware deinterlacer) |
| 60 fps files show ~51 fps | blocking legacy SetPlane per frame | open item (non-blocking atomic presenter); IPTV is 25–50 fps |
| Software playback crawls (old builds, Settings → Video decoding = software) | mpv decodes and scales on the CPU (0.25–0.33×) | Settings → Video decoding → hardware (default) |
| "Could not get/set settings from/on resource" (some films), dmesg `hdmi-audio-codec: Not able to map channels to speakers (-22)` | a 5.1 audio track; the TV reports stereo speakers and HDMI refuses 6 channels (NOTES §42) | EBI ≥ 0.6.4 downmixes: Settings → Audio channels = stereo (default) |
| Any other playback error: what failed? | the TV shows `<message> [<element>: <reason>]` (≥ 0.6.3) | on the box: `cat /tmp/h96ui-player.log` (status + `detail:` lines), `/tmp/h96player-gst.log` (GStreamer warnings + ALSA negotiation), `dmesg \| tail` |
| After `h96-fix ui-rollback` to an EBI older than 0.6.4: every channel fails at once | none here: the new h96player still runs old EBI builds; but the old h96player cannot run EBI ≥ 0.6.4 (`-C` unknown) | keep EBI and h96player from the same push (`push-to-box.sh` installs both) |

## Screen and console

| Symptom | Cause | Fix |
|---|---|---|
| Menu squashed / wrong size / only a corner of the screen | the TV returned an empty EDID at boot (TV off or slow), HDMI fell back to 720p; the console size is set only once at boot | `h96-fix display`; for good: turn the TV on before the box |
| No picture at all | HDMI lost the link | `h96-fix display`, or replug HDMI |
| Analog TV-out mode shown / console 720x576 | TV-out (VP1) still on | `h96-fix display` (sets TV-1 off) |

## Network, Bluetooth, remote

| Symptom | Cause | Fix |
|---|---|---|
| No WiFi / no `wlan0`, or Bluetooth gone after a chip crash | the SWT6621S firmware asserted; a warm reboot does not reset it | `h96-fix wifi-bt` (unbind/bind `ffc20000.mmc`); otherwise unplug the power |
| Boot hangs in h96-wifi, no ssh | crashed chip from the previous run | cold power cycle |
| Remote ignored after boot | the BLE remote reconnects only on a key press | press any key, wait ~2 s; doctor shows "remote connected" |
| Remote never connects | pairing lost (userdata reflash, new BT address) | re-pair: `docs/log/status-and-resume.md` → Remote |
| Box not at .149 | DHCP gave another address | sweep the /24, match MAC `fe:fd:fc:e6:49:48` (WiFi) / `00:e0:4c:36:13:c2` (dongle) |
| "No route to host" at .149 right after a reboot | still booting (WiFi + DHCP take a while) | wait a minute or two and retry |
| A reflashed box has no WiFi | the image carries no PSK on purpose | main menu → **Add WiFi from USB**: a `.txt` with `ssid=…` / `password=…` lines on the stick (NOTES §41) |
| Add WiFi from USB: "Saved, but not connected … after 30 s" | wrong password/name, out of range, or the network is WPA3-only / 802.1X | fix the file and press OK again (it replaces the saved entry); the previous conf is `/etc/wpa_supplicant/wpa_supplicant.conf.bak` |
| Holding Up/Down moves only one step (EBI < 0.6.0) | the remote's input has no key autorepeat (no EV_REP) | EBI ≥ 0.6.0 autoscrolls from evdev timing |
| ssh: "Permission denied (publickey)" | the box only accepts `build/keys/h96max_ed25519` (no passwords anywhere since 2026-11-22) | use `tools/h96`; if the key is lost: maskrom + userdata rewrite, or edit `/root/.ssh/authorized_keys` on the eMMC from the PC |
| USB stick not seen at all (no `sda` in dmesg) | the USB-A port cannot power hub + Ethernet dongle + stick | unplug the dongle (WiFi keeps ssh) or plug the stick in directly |
| exFAT stick: "exFAT needs fuse-exfat" | the kernel has no exFAT | `apk add fuse-exfat` (in `box-packages.txt`) |

## Subscriptions and Xtream

| Symptom | Cause | Fix |
|---|---|---|
| A get.php link imported as a huge playlist | old builds treated it as an M3U URL | h96ui ≥ 0.5.0 recognises Xtream links and adds an account instead |
| "login refused: check the username and password" | Xtream `user_info.auth` is 0 | check the file; `h96ui -xtream-check FILE` |
| "not an Xtream server" / "not Xtream JSON" / HTTP 403 | wrong server address, or the panel refuses the User-Agent | try `user_agent: <what the provider's app uses>` in the account file |
| Account file readable by everyone | copied by hand | `chmod 600 /var/lib/h96ui/subs/*.xtream` (doctor checks) |
| Vol-/Vol+ jumps land on "F" (or one letter) in a whole category | a provider prefix the list shares | EBI ≥ 0.6.3 strips shared leads (`|FR| `, `[Italy] `, `[IT] [Serie] `, `ITALY: `); a new prefix style: `names.rs` `lead()` + a test |
| A list is not in the order the provider shows | lists are A-Z by default (≥ 0.6.2) | Settings → List order → as the provider lists them |
| Xtream: does it need logging in again? | no: every request carries the account; there is no session | only after an expiry or a password change: import the account again |

## Boot and system

| Symptom | Cause | Fix |
|---|---|---|
| A service silently not started after editing files | no RTC: files got mtimes "in the future" relative to the boot clock, OpenRC skipped them | chronyd on (default), `touch` the files; never re-enable `savecache` |
| Box will not boot our kernel | boot partition broken | it falls into maskrom automatically (u-boot patch); recovery button at power-on also works; write with read-back (`status-and-resume.md` → Maskrom) |
| Can't find how to delete a subscription | it was only at the bottom of the subscription's own menu | h96ui ≥ 0.5.1: **hold OK** (or press **Menu**) on it in Subscriptions → Remove |
| Need everything back as built | live changes drifted | reflash userdata from `build/rootfs/h96root.img` (built by `tools/build-rootfs.sh`: includes the video stack, Rust h96ui, fixup kit) -- **only with explicit consent**, and keep the PSK, BT address and pairing (`status-and-resume.md`) |
