# Interface (h96ui) and video playback

Part of the H96Max research notes; index in [README.md](README.md).

## 29. h96ui: the console interface (2026-11-21)

Go (stdlib only), static arm64, 6.0 MB: `tools/h96ui/` (`build.sh` in fedora:41: vet + tests +
cross-build -> `build/h96ui/h96ui`). Runs on tty1 from inittab (replaces h96-info). Layout:
top bar WiFi SSID / signal / IP + clock, breadcrumbs, list with highlight, message line,
footer `h96ui <ver> (<date>) | Alpine | Linux` + key hints. Keys: arrows, Enter, Esc/Backspace,
PgUp/PgDn -- what the IR remote sends once mapped (the IR receiver has the `kbd` input handler,
so its keys reach the console directly; HDMI-CEC does not).
- Main menu: **Subscriptions** (count) / **Add subscription from USB** / **Settings**.
- Subscriptions: per item Channels / Refresh from URL / Remove (confirm). Channels list
  name + group; Enter plays with mpv using the subscription as playlist (Up/Down/PgUp/PgDn
  = channel, Enter = OSD title, Esc/BS = back to the menu).
- USB import (no typing): mounts the stick read-only (vfat/exfat/ext4), offers `.m3u/.m3u8`
  (copied) and `.txt/.url` with http(s) playlist URLs (downloaded, refreshable), from `/` and
  `/iptv`. Data in `/var/lib/h96ui/{subs,settings.json}`.
- Settings: network cache 2/5/10/20 s, deinterlace, start volume, audio HDMI/SPDIF/analog
  (ALSA cards rockchiphdmi / ROCKCHIPSPDIF / rk3528acodec), picture fit/stretch/fill, About.
- `h96ui --snapshot main|subs|channels|settings|about|usb` renders a screen as text (layout
  checks over ssh). Console is 90x36 at 1080p (readable on a TV, no font change needed).

## 30. Video: software playback is not viable; the hardware path (2026-11-21)

mpv 0.41 (Alpine, +350 MB) with `--vo=drm`, Mux Big Buck Bunny HLS test stream, measured on the box:
- 1920x1080@60 H.264, decode only (`--vo=null --untimed`): 10 s of video in 30 s (~0.33x).
- 320x184@30, decode only: 10 s in 16 s (network-bound).
- **320x184 displayed full-screen via `--vo=drm`: 10 s in 39 s (~0.25x)** -- the CPU-side
  scaling + colour conversion to 1080p is the worst part.
- mpv's GPU outputs are unavailable: Mali-450 has only the vendor kernel driver, no userspace
  (vendor libMali is closed/glibc; Mesa lima needs the upstream lima kernel driver).

**Plan:** decode on the VPU (Rockchip MPP: `mpp_rkvdec2`, `mpp_vdpu2`… are probed, via
`/dev/mpp_service`) and scan out on a VOP2 hardware plane (hardware scaling, NV12, zero-copy);
no GPU needed. Candidates: GStreamer `mppvideodec` (gstreamer-rockchip) + `kmssink`; or
ffmpeg-rockchip (rkmpp) with a DRM-plane presenter. h96ui keeps the player as one function
(`play.go`), so the player can be swapped without touching the menus.

**Console geometry fix (same day).** h96ui first appeared squashed in the top-left: the DRM
fbdev helper drives HDMI (1080p, VP0) and the analog TV-out (720x576i, VP1) from one framebuffer
and sizes the console for the smaller mode -> 90x36 cells of 8x16 = a 720x576 corner. Fix:
OpenRC `h96-console` (default runlevel, before inittab starts h96ui): TV-1 connector `off`,
`fbset -g 1920 1080 1920 1080 32`, `setfont ter-v32n` (font-terminus) on tty1 -> 120x33.
h96ui 0.1.1 re-reads the console size on every redraw. Verified across a reboot. A cleaner
long-term option is `video=TV-1:d` on the kernel cmdline (boot image) or disabling the `tve`
node in the boot DTB.

## 33. Starting point for the VPU / GPU job (2026-11-21)

What the running kernel (#5, vendor 6.1.172) already provides, read on the box
(`build/vpu-gpu-starting-point.txt`):
- **`/dev/mpp_service`** (Rockchip MPP kernel service, `mpp_service mpp-srv: probe success`) with
  `/proc/mpp_service/{rkvdec0, rkvenc0, avsd_plus, iep, jpegd, supports-device, …}`.
  Probed blocks: `mpp_rkvdec2 ff740100.rkvdec` (H.264/H.265/VP9-class decoder; link mode, SRAM
  0xfe480000/48 KiB, rcb iova 0x10000000, min_width 512; harmless "shared_niu_* not found",
  "failed on clk_get clk_core/cabac"), `mpp_vdpu2 ff7c0400.vdpu` + `mpp_vdpu1 ff7c1000.avsd_plus`
  (older codecs / AVS+), `mpp_jpgdec ff870000.jpegd`, `mpp_rkvenc2 ff780000.rkvenc` (encoder;
  no OPP table), `mpp-iep2 ff860000.iep` (deinterlace), `mpp_vdpp ff861000.vdpp`. Each has its
  own IOMMU group.
- **`/dev/rga`**: RGA2, hw_version 3.7.93215 (2D blit, scaling, colour conversion).
- **Display:** VOP2 `ff840000.vop`, VP0 -> HDMI (1080p60, YUV444 limited by default),
  VP1 -> TV-out (now forced off by h96-console). Planes Esmart0..3 (Esmart2 = cursor on VP0),
  AFBC NV12 noted as unsupported at probe. `/dev/dri/card0` only (no render node).
- **GPU:** Mali-450 (Utgard) at ff700000, vendor `mali400` kernel driver loaded (rk_ko_ver 5);
  only PP0/PP1 IRQs exist in the DT (the PP2..PP7 "not found" errors are expected -> an MP2
  configuration); devfreq `ff700000.gpu` 700/800 MHz. **No userspace driver**: vendor libMali is
  closed (glibc/Android builds); Mesa `lima` needs the upstream `lima` kernel driver instead of
  `mali400`. Utgard = OpenGL ES 2.0 only, no compute.

Candidate approaches for hardware video (to evaluate next):
1. **ffmpeg with Rockchip MPP** (e.g. the ffmpeg-rockchip fork: `h264_rkmpp`, `hevc_rkmpp`
   decoders + RGA filters) -> frames as DRM PRIME (dmabuf) -> shown on a VOP2 plane via KMS
   (zero-copy, hardware scaling). Needs librockchip_mpp + librga built for musl/arm64, and a
   presenter (mpv cannot show DRM PRIME without a GPU; a small KMS-plane presenter, or
   GStreamer `kmssink`).
2. **GStreamer**: `mppvideodec` (gstreamer-rockchip) + `kmssink` (plane, zero-copy) +
   hlsdemux/souphttpsrc; heavier but mostly off-the-shelf.
3. **GPU for some acceleration** (secondary): with Mesa lima (kernel `lima` driver instead of
   vendor `mali400`), mpv `--vo=gpu` (GLES2) could do scaling/colour conversion; or libMali via
   a glibc container. Useful for the UI/OSD, not required for decode+display if planes are used.
h96ui calls the player through one function (`play.go`), so swapping mpv for any of these keeps
the menus unchanged.

**First step agreed for the VPU job (2026-11-21):** before touching the player or h96ui, prove
the hardware path in isolation:
1. Check which ready-made pieces build on **Alpine arm64 (musl)**: Rockchip's MPP userspace
   library (`librockchip_mpp`, talks to `/dev/mpp_service`), librga, and an MPP-capable decoder
   front-end (ffmpeg-rockchip's `h264_rkmpp`/`hevc_rkmpp`, or gstreamer-rockchip's
   `mppvideodec`). Build in a container, like everything else.
2. Decode **one test stream** with the VPU (e.g. the Mux Big Buck Bunny HLS 1080p60 H.264 used in
   §30) and check realtime speed + CPU load against the software numbers (0.33x decode only).
3. Show the decoded frames **directly on a VOP2 plane** (KMS, DRM PRIME / dmabuf, hardware
   scaling) -- e.g. GStreamer `kmssink` or a minimal KMS-plane presenter -- and check smooth 1080p
   on HDMI.
4. Only then: wire the chosen pipeline into h96ui's `play.go` (channel up/down, back to the
   menu), and decide whether the GPU (Mali-450 via lima, or libMali) is worth it for the OSD.

## 34. Hardware video works: VPU decode + VOP2 plane, zero-copy (2026-11-21)

**Userspace, built for Alpine arm64/musl** (`tools/video/build-mpp.sh`, arm64 alpine:3.24.4
container, stages `mpp rga ffmpeg gst`, output `build/video/video-stage.tar.gz`, untar at `/`):
- **MPP** (`downloads/mpp`, rockchip-linux develop 9b56348) + `tools/video/mpp/0001-mpp_soc-add-rk3518.patch`:
  MPP's SoC table has no `rockchip,rk3518` -> "can not found match soc name", generic caps,
  "client N driver is not ready". The patch aliases rk3518 to the plain **rk3528** entry
  (vdpu382 H.264/H.265, vdpu2, jpegd, avsd, vepu540c). The DT decoder is
  `rockchip,rkv-decoder-rk3528`; VP9 (the rk3528a entry) is not claimed.
- **librga** from source: `airockchip/librga` is prebuilt glibc binaries only; `downloads/librga` =
  nyanmisaka/rk-mirrors `jellyfin-rga`. musl fixes via meson cpp_args: `-Dmmap64=mmap` (musl
  >= 1.2.4 dropped it) and `-include sys/cdefs.h` (+ apk `bsd-compat-headers`).
- **gstreamer-rockchip** (`downloads/gstreamer-rockchip` = JeffyCN/mirrors branch
  `gstreamer-rockchip`): `mppvideodec` (rank primary+1, so playbin picks it), rga, dmaheap.
  Against Alpine's GStreamer 1.28.3. Box packages: gstreamer gstreamer-tools gst-plugins-base
  gst-plugins-good gst-plugins-bad (kmssink, HLS) gst-libav (AAC), libdrm-tests (modetest).
- **ffmpeg-rockchip** (`downloads/ffmpeg-rockchip`): `h264_rkmpp`/`hevc_rkmpp` + RGA, installed
  into `/opt/rkffmpeg` so Alpine's libavcodec (mpv) is untouched. (Build under emulation is slow.)
- Submodule gotcha: `git submodule add --depth 1 -b <branch>` on the multi-branch mirror repos
  fails ("unable to checkout"); fetch the branch inside the submodule and check out FETCH_HEAD.

**Measured on the box:**
- `mpi_dec_test -t 7`, Mux BBB 1080p60 High-profile H.264, 30 s (1802 frames): **10.8 s = 166 fps =
  2.8x realtime, ~6% CPU** (software: 0.33x at 100%).
- `mppvideodec ! fakesink`: steady 60.0 fps.
- **Live HLS over WiFi** (`h96play https://test-streams.mux.dev/x36xhzz/x36xhzz.m3u8`, playbin):
  MppVideoDec + avdec_aac, adaptive 720p -> 1080p, **60 fps, 2 drops / 700 frames, ~8% CPU,
  audio on HDMI** (`alsasink device=plughw:0`, card 0 `rockchiphdmi`).

**Display -- the plane matters:**
- DRM planes: 59 Esmart0-win0 (primary, the console, XR24), 172 **Esmart1-win0**, 75 Esmart2-win0
  (cursor), 101 Esmart3-win0 (VP1), 332 Cluster0-win0; the others are `-win1..3` sub-windows.
  All list NV12. HDMI connector 371, CRTC 91 (VP0).
- **Sub-windows (`EsmartN-win1..3`) share their win0's format and scaler**: NV12 on plane 124
  (Esmart0-win1, over the XR24 console) flashed **green** on the TV, then on a re-run would not
  negotiate. Use an independent win0 layer: **plane 172**. Debug view:
  `/sys/kernel/debug/dri/0/summary` (src/dst rect, format, pitch, buffer offsets).
- With plane 172: NV12 1920x1080 (pitch 1920, UV at 1920x1088) hardware-scaled to the mode,
  zero-copy dmabuf, ~2% CPU -- but **26 fps**: the vendor kernel's legacy SetPlane already blocks
  to vblank and kmssink waits for vsync again. `skip-vsync=true` -> ~51 fps on a file at 60p
  (live HLS: 60). A presenter using non-blocking atomic commits would make 60p clean.
- **Video inside the console UI:** the VOP composites the console (primary plane) and the video
  layer (zpos above), so `kmssink render-rectangle=<x,y,w,h>` puts video in a box while h96ui
  stays visible around it -- no CPU cost. `RECT=x,y,w,h h96play URL`.
- `tools/video/h96play` (on the box as /usr/local/bin/h96play) = the prototype player.

**HDMI side note:** this boot the TV returned an all-zero EDID at 39 s; at 100 s the link renegotiated
to **1280x720** (console became 45x18). Not caused by video; h96-console's 1080p fbset only runs once.

## 35. Real IPTV channels: anamorphic SD, server throughput, request headers (2026-11-21)

h96ui 0.3.0 (Rust) plays through the hardware path (`play.rs` play_hw: gst-launch playbin ->
mppvideodec -> kmssink plane 172); remote Back/Home are read from evdev (`remote.rs`), because
the Linux console drops KEY_BACK (158), KEY_HOMEPAGE (172) and the Menu key (240). Remote key
map: `build/wifi/remote-keys2.txt`. First tests with an iptv-org country list:

- **576p (2M) failed: `drmModeSetPlane failed: No space left on device`.** DRM KMS debug
  (`echo 0x04 > /sys/module/drm/parameters/debug`): `drm_framebuffer_check_src_coords: Invalid
  source coordinates 1024x576 (fb 720x576)`. kmssink 1.28.3 bug: without a crop meta it uses
  the pixel-aspect-corrected *display* size (720x576 PAR 64:45 -> 1024) as the *source*
  rectangle. Patch `tools/video/gst-plugins-bad/0001-kmssink-source-rect-is-the-frame-size.patch`,
  built by stage `kms` of `tools/video/build-mpp.sh` (pinned to Alpine's exact
  gst-plugins-bad, sha256), installed over `/usr/lib/gstreamer-1.0/libgstkms.so` (Alpine's
  original: `/root/libgstkms.so.alpine`; an apk upgrade of gst-plugins-bad reverts it).
  After: src 720x576 -> dst 1920x1080, 50 fps, 0 drops. All HD (square pixel) streams were fine.
- The SD stream is **interlaced** (`interlace-mode=mixed`); the hardware path shows it woven
  (no deinterlacer yet; IEP2 exists).
- **1080p "hangs" = data starvation, not decode/display.** Headless (fakesink) 40 s runs: 2M Monde
  +1 rendered ~11.5 s of video, Al Maghribia (8.8 Mbit/s 1080i) ~3 s; 0 dropped frames.
  Box WiFi measured ~200 Mbit/s from the LAN (64 MiB over HTTP) -- **correction (§40): a LAN copy
  (1 ms RTT) cannot reveal the driver's reorder stall that cripples distant servers**; the host's internet ~90 Mbit/s;
  but server 185.9.2.18 gives ~12 Mbit/s per connection to any client -> no margin for an
  8.8 Mbit/s stream with sequential segment fetches from the live edge. Needs a prebuffer
  (playbin shows `use-buffering = false` for HLS: h96ui's "Network cache" setting has no effect
  on the hardware path yet). `tools/video/seg-speed.sh URL...` measures per-segment throughput
  on the box (busybox `date` has 1 s resolution: coarse).
- **360p (2M Monde) refused:** the playlist entry carries `http-referrer` / `http-user-agent`
  attributes and `#EXTVLCOPT:` lines (VLC convention); h96ui sends neither.

## 36. h96player: playlist HTTP headers and a real prebuffer (2026-11-21)

gst-launch cannot reach playbin's source element or react to buffering, so h96ui (0.3.1) now
runs **`h96player`** (`tools/video/h96player.c`, ~250 lines of C on the GStreamer API, stage
`player` of `tools/video/build-mpp.sh`, installed as /usr/local/bin/h96player):
- **Headers:** `-u UA -r referer -H 'Name: value'` set on souphttpsrc in `source-setup`.
  hlsdemux2 (gst-plugins-good 1.28 `ext/adaptivedemux2`, rank primary+1, so playbin's choice)
  copies **User-Agent and Referer** from the source's `http-headers` event (request headers,
  which include souphttpsrc's extra-headers) into its download helper -> every playlist and
  segment fetch carries them. Other custom headers reach only the first request.
  h96ui's parser (`m3u.rs`) collects them from `#EXTINF` attributes (`http-user-agent`,
  `http-referrer`), `#EXTVLCOPT:` lines and Kodi's `URL|User-Agent=..&Referer=..` suffix
  (%-decoded). Groundwork for Xtream panels, many of which refuse non-player User-Agents.
- **Prebuffer:** hlsdemux2 `high-watermark-time` = `-b` seconds (h96ui "Network cache"); the
  pipeline starts PAUSED and goes PLAYING at buffering 100%; after that it pauses again only
  below `-L` percent (default 2). hlsdemux2 posts its level continuously (8%<->100% jitter within
  milliseconds once playing), so "pause below 100%" paused constantly. ASYNC_DONE without any
  buffering message (files) -> play at once.
- Status lines on stderr: `h96player: error: <text>` (h96ui shows it), `buffering N%`,
  `playing`, `end of stream`; `H96PLAYER_TRACE=1` adds timestamps and rendered/dropped frame
  counts every 5 s.
- **Result on the slowest server (185.9.2.18, ~12 Mbit/s per connection, 8.8 Mbit/s 1080i50):**
  start after ~4 s (10 s prebuffer), then 250 frames per 5 s for 52 s, 0 dropped. With
  gst-launch the same channel showed ~3 s of video in 35 s.

## 37. On-demand videos, OK button, and the seek deadlock (2026-11-22)

- **OK button:** the remote's OK is KEY_SELECT (353). The Linux console keymap only covers
  keycodes 0..255, so it can never reach the tty; h96ui reads it from evdev with Back/Home
  (`remote.rs`: KEY_SELECT, KEY_OK 352, KEY_PLAYPAUSE 164 -> Enter). No double presses possible.
- **Live vs on-demand** (`m3u.rs` `Channel::is_vod`): positive `#EXTINF` duration, Xtream
  `/movie/` `/series/` (and `/vod/`) paths, video-file extensions (mp4 mkv avi mov m4v webm
  wmv flv mpg mpeg; *not* .ts, which Xtream uses for live), or a group word vod/movie(s)/film(s)/
  series/serie/cinema. h96ui 0.4.0: subscription -> "Live channels (n)" / "Videos (n)".
- **h96player -i** (control from stdin): `toggle|pause|play`, `seek <+-s>`, `rect x y w h`,
  `full`; `status pos= dur= seekable= paused=` once a second. EOF on stdin = stop.
  h96ui's OSD: the video shrinks into a box (`rect`, kmssink render-rectangle + expose) and the
  console draws title / position bar / keys around it; full screen again 4 s after a seek,
  stays while paused. Videos: OK pause, Left/Right -/+10 s, Up/Down +/-1 min. Live: OK = info.
- **Seek deadlock (playbin, gst 1.28.3), found with Alpine -dbg packages + gdb:** after a
  flushing seek hlsdemux2 re-creates its src pads; playbin's `pad_removed_cb` (holding playbin's
  dyn lock) resets the input-selector and waits for its GRWLock writer lock, while the main
  thread's position/duration query blocked in `gst_play_bin_query` on the same playbin lock ->
  main loop dead (no status, no commands, SIGTERM ignored). Same with avdec + fakesink, so not
  mppvideodec/kmssink. Fixes: **playbin3** (no input-selector), all runtime state changes and
  seeks via `gst_element_call_async`, no pipeline queries while a seek is in flight (position
  from the video sink, duration/seekable cached), seeks serialized (merged while one is
  pending, 5 s timeout), and a 2 s `alarm()` watchdog on quit. `tools/video/seek-test.sh`
  (8 seeks, checks the status keeps ticking): passes with VPU+kmssink+HDMI audio; live
  Al Maghribia still 50 fps / 0 dropped on playbin3.
- **Drawing over the video** (user request: a black bar with the time at the bottom): needs a
  layer *above* the video with alpha -- the console is below it. Free candidate: plane 75
  Esmart2-win0 (AR24, per-pixel alpha, zpos up to 17, VP0+VP1). Only the DRM master may set
  planes, and during playback that is kmssink inside h96player, so h96player would open the DRM
  device itself, hand the fd to kmssink (`fd` property) and put a small ARGB dumb buffer with
  the bar (built-in bitmap font) on plane 75. Not done yet.
- **Volume (h96ui 0.4.1):** remote Vol+/Vol-/Mute (evdev) during playback -> h96player
  `volume <0..1>` / `mute <0|1>` (playbin3 software volume); +/-5 % steps, shown as a line in the
  OSD box for 4 s. Level kept across channel switches in memory only (Settings > Start volume
  stays the per-session default); mute survives a channel switch.

## 38. Xtream Codes accounts (2026-11-22)

h96ui 0.5.0 speaks the Xtream API natively (`tools/h96ui-rs/src/xtream.rs`, own JSON reader
`json.rs`, downloads via busybox wget). The API, stream URL forms and the accepted account
file formats are documented at the top of `xtream.rs`.
- **Getting an account in (no typing):** a `.txt`/`.url`/`.xtream` file on the USB stick with the
  provider's `get.php` / `player_api.php` link, or `server:` / `username:` / `password:` lines
  (optional `user_agent:`). Account files win over "list of playlist URLs" (a get.php link would
  otherwise be downloaded as one huge flat M3U). Import logs in first, refuses duplicates
  (same server + user), stores `subs/<host port>.xtream` mode 0600. Over ssh:
  `h96ui -xtream-check FILE` (login + categories, prints no password) and `h96ui -xtream-add FILE`.
- **Menus:** Live TV / Movies / Series -> categories ("All ..." first, fetched per category) ->
  channels / films / series -> seasons -> episodes (sorted; one season = straight to episodes);
  Account (status, expiry, connections, live format, User-Agent). Live URLs use .m3u8 when the
  panel allows HLS (`allowed_output_formats`), else its first format (ts).
- **User-Agent:** default `VLC/3.0.21 LibVLC/3.0.21` (many panels refuse unknown players), used
  for the API and passed to h96player for every stream. Passwords are registered as secrets:
  `App::say` and the player's failure card replace them with `***`.
- **Tested** against `tools/h96ui-rs/testdata/mock-xtream.py` (account demo/demo123; 403 unless the
  User-Agent starts with "VLC"; live = HLS redirects, films/episodes = Blender's progressive
  MP4s with range requests): link and line formats log in, a wrong password says "login
  refused", every API and stream request carried the UA; live channel 870 frames / 0 dropped;
  `seek-test.sh` (8 seeks) passes on a progressive MP4 film. Not yet tried on a real panel.

## 39. Fixup kit and the reproducible image (2026-11-22)

- `tools/fixup/h96-doctor` (box, read-only; exit status = number of FAILs): h96ui running and
  which build, console cells vs. framebuffer (expected 120x33 on 1920x1080), h96player present,
  leftover player processes, GStreamer elements (playbin3 mppvideodec kmssink hlsdemux2 avdec_aac
  alsasink souphttpsrc), kmssink = our patched build (compared with
  `/usr/local/share/h96/libgstkms.so`), `/dev/mpp_service`, librockchip_mpp knows the RK3518
  (`mpp_info_test`), video plane Esmart1-win0 active without a player (= frozen frame), wlan0 +
  address, hci0, the remote's input device and whether h96ui has it open, clock (no RTC), USB
  stick left mounted, free space, memory, temperature, Xtream account files mode 600, BT address
  and WiFi credentials present.
- `tools/fixup/h96-fix` (box): `player`, `display`, `ui`, `ui-rollback`, `kmssink [alpine]`,
  `usb`, `wifi-bt` (NOTES §32 chip reset, detached via setsid because it drops WiFi).
- `tools/fixup/push-to-box.sh` (PC): installs build/h96ui/h96ui (keeps h96ui.prev),
  build/video/video-stage.tar.gz (without /opt/rkffmpeg), the fixup kit and missing packages from
  `tools/fixup/box-packages.txt`; stores the kmssink reference copy, Alpine's original and the
  version it was built for; restarts h96ui; runs `h96-doctor -q`.
- `tools/build-rootfs.sh` now installs `box-packages.txt` and the video stage (saving Alpine's
  kmssink first, refusing to build when Alpine's gst-plugins-bad differs from `GPB_VER`, and
  running h96player once inside the image); `tools/make-inbox.sh` takes h96ui from the Rust build
  and adds the fixup kit. Image `h96root.img` `3749043b…`: 330 packages, 533 MB content; root's
  shadow entry is empty (passwordless local login on tty2).
- Verified: the scripts pass `sh -n`; doctor and the hardware-free fix paths run correctly under
  busybox in an arm64 Alpine container. **Not yet run on the box** (offline at the time).
- Symptom-to-fix table: `docs/log/troubleshooting.md`.

## 40. "Streaming is gone": the WiFi driver's rx reorder buffer (2026-11-22)

**Symptom (user):** after a reboot no live channel played any more, the public 2M (576p) included;
h96player stayed in PAUSED (no buffering message, no error, 0 frames). The Mux test stream and
Medi1TV still played.
**Ruled out, in order:** h96player/playbin3 (the old playbin failed the same way), the decoder
(2M is now MPEG-2 + MP2; `mpi_dec_test -t 2` 505 fps; a downloaded segment plays from a file),
the box clock (NTP-synced to the µs), the HLS playlist (normal sequence, 4 x 5.4 s window),
packet loss (0 % ping loss at 74 ms RTT, 6 TCP retransmits total), WiFi signal (-59 dBm, HE-MCS 11).
**Cause:** downloads from distant servers ran at a steady **0.30 Mbit/s** on the box vs ~11.5
Mbit/s on the PC (same segment, same minute): a window/stall limit, not loss. TCP counters showed
the received segments landing in the out-of-order queue. A/B with the driver's runtime switch
`/sys/kernel/debug/skwifi/chip1.sdio/rx_reorder` (Neardi skw6316 A-MPDU reorder buffer,
`skw_rx.c` `skw_ampdu_reorder`, enabled at load): **disable -> 6.59 Mbit/s, enable -> 0.30**.
With it off, 2M, Al Maghribia and Chada TV play again (0 dropped).
**Fix:** `h96-wifi` turns it off right after the driver loads (payload + box; the old script is
`/root/h96-wifi.before-reorder-fix`); `h96-doctor` FAILs if it is on; `h96-fix wifi-reorder`.
**Why the earlier tests missed it:** the LAN test (§35: ~200 Mbit/s) runs at 1 ms RTT, where a
reorder stall costs nothing; on a 74 ms path each stall makes the sender back off. The earlier
11.5 Mbit/s from the same server (before the reboot) suggests the stall depends on the AP's
aggregation for the session. **Open:** fix the reorder logic itself (likely our rx descriptor
port: sequence-number / BA-window fields, §27) and re-enable it; until then the stack sees
occasional reordering, which TCP absorbs.

## 41. EBI 0.6: WiFi from USB, letter jump, autoscroll, A-Z lists (2026-11-22)

The frontend is called **EBI (Extremely Basic IPTV)** from 0.6 on (user decision). Only the
on-screen text changed: title, footer, About, `-version`. The binary `h96ui`, `/var/lib/h96ui`,
`H96UI_*` variables and the `h96ui/<ver>` User-Agent keep the old name so that inittab, the
rootfs build and the fixup kit keep working.

- **Add WiFi from USB** (`wifi.rs`). The rootfs image deliberately has no WiFi PSK, so a
  reflashed box could only get WiFi through ssh. Now: a `.txt`/`.wifi` file on the stick (top
  folder, `wifi/` or `iptv/`) with `ssid=` and `password=` lines (also `wifi`/`network`/`name`,
  `pass`/`psk`/`key`; `=` or `:`; several networks per file; no password line = open; a file
  named `wifi*` may be just two plain lines). Checked against wpa_supplicant's limits (passphrase
  8..63, or 64 hex digits = raw PSK; SSID 1..32 bytes). OK on a network: the conf is parsed into
  header + `network={}` blocks, a block with the same SSID is replaced, the new one gets
  `priority` = highest + 1 (counted *before* the old block is dropped, so a re-add stays on
  top), the rest of the file is kept, the old file goes to `wpa_supplicant.conf.bak`, the new
  one is written 0600 and renamed into place, then `rc-service h96-wifi restart` (its stop keeps
  the driver loaded: the safe reset, §28) and a 30 s wait for SSID + IPv4 (Back stops waiting).
  Passwords go into the redaction list, never on screen. Such files are neither URL lists nor
  Xtream accounts, so the subscription import ignores them (and the reverse: tests cover it).
  An on-screen keyboard with a network scan was started and dropped at the user's request:
  the USB file is enough.
- **Autoscroll.** The remote's Consumer Control input (event4) has no EV_REP: a held arrow
  produced exactly one step. `remote.rs` now times KEY_UP/KEY_DOWN press/release from evdev
  (the step itself still arrives through the console): after 400 ms a step every 120 ms, every
  60 ms after 1.5 s, every 25 ms after 3.5 s; menus only (`App::menu_repeat`; in the player
  Up/Down zap or seek). Console arrows that only repeat a held remote arrow (a device with
  kernel autorepeat) are dropped while the evdev timer drives. A lost release (BLE drop) stops
  after 60 s; a vanished device clears the state.
- **Letter jump and A-Z order** (`names.rs`, one module so the two can never disagree). Per
  list: (1) find a **lead** on each name: encased at any length (`[Italy] `, `|ITALIA| `,
  `(Serie TV) `, also `{} <> «»`), one word of up to 12 letters/digits before `:`/`|`/` - `
  (`ITALY: `), up to 4 before any symbol (`FR- `, `4K/`), or two capitals before a space
  (`FR `, `UK `); plain words are names (`BBC One`, `France 2`, `Le Mans`, `Italy-Rai`). (2) A
  lead is stripped only if at least 2 names and 60 % of the list share it, twice over
  (`[IT] [Serie] Gomorra` -> `Gomorra`), so a mixed-language list keeps its country codes as the
  useful letter, and a UK list that is mostly BBC stays under B. (3) Only letters and digits
  count, Latin accents folded (`Équipe` under E), digits = `#`. The sort key is runs of folded
  letters and numbers by value (`Canal 2` < `Canal 10`); `#` first, nameless last, stable. Sorted
  (Settings > List order, default A-Z): live channels, videos, films, series, Xtream categories
  (`All` stays first); never episodes. Zapping follows the shown order. Vol- = next letter
  (down), Vol+ = previous (user: more natural than the first mapping); an unsorted list jumps
  one block of equal letters at a time.

## 42. "Could not get/set settings from/on resource": HDMI audio channel mapping (2026-11-22)

- **Symptom:** some films stopped at once with GStreamer's stock text for
  `GST_RESOURCE_ERROR_SETTINGS`. That text appears when the element gives no message of its own;
  kmssink always does (`Could not find a plane for crtc`, ...), so the bare text pointed at
  `alsasink` (or rtspsrc). More codecs would not help: a missing codec is a different error.
- **Verbosity first** (h96player): error and warning lines are now `error: <msg> [<element>:
  <last line of the debug string>]`, plus `detail: <full debug string>`; h96ui sets
  `GST_DEBUG=2,alsa:4 GST_DEBUG_FILE=/tmp/h96player-gst.log` for every player run (unless
  GST_DEBUG is already set). First real hit: `[alsasink0: Unable to set hw params for playback:
  Invalid argument]`; dmesg: `hdmi-audio-codec ...: Not able to map channels to speakers (-22)`,
  `ASoC: error at snd_soc_dai_hw_params on i2s-hifi: -22`.
- **Cause:** a multichannel (5.1) track. `plughw` advertises everything (4 kHz-2 MHz, 1-n
  channels) and passes the channel count through; the HDMI codec looks it up in the speaker
  allocation the TV reports (stereo) and refuses. Reproduced without any stream:
  `audiotestsrc wave=silence ! audio/x-raw,channels=6,rate=48000 ! alsasink device=plughw:CARD=rockchiphdmi,DEV=0`
  fails the same way; with `audioconvert ! audioresample ! capsfilter caps=audio/x-raw,channels=2`
  in front it plays.
- **Fix:** h96player `-C N` (default 2; 0 = as the source) wraps alsasink in a bin with that
  converter chain (audioconvert's downmix matrix, not a channel drop); EBI passes it from
  Settings > Audio channels (stereo default, "as the source" for a TV/amplifier that takes
  5.1); mpv gets `--audio-channels=stereo`. Not done: reading the TV's ELD to choose
  automatically (`/proc/asound/card0` has no `eld#` file on this kernel).
- Side note in the same log: `Unknown PCM plughw:...:{AES0 ...}` is alsasink probing IEC958
  passthrough on a device string that already has arguments: harmless.

## 43. EBI 0.7: its own text renderer (Unifont) and favorites (2026-11-22)

- **Why:** names in Arabic, Hebrew, Japanese, Chinese, Korean showed as garbage. The kernel
  console holds at most 512 glyphs (Terminus: Latin/Greek/Cyrillic), so no console font can fix
  CJK; Alpine ships no fbterm/kmscon.
- **How (`fb.rs`, `text.rs`):** tty1 goes to `KD_GRAPHICS` (fbcon stops drawing; keys still
  arrive on the tty), `/dev/fb0` (1920x1080, 32 bpp, stride 7680) is mmapped, and everything EBI
  already wrote for the console (SGR colours, CUP, EL, ED, CR/LF; the player's cards and OSD
  too) is interpreted into a 120x33 cell grid and drawn with **GNU Unifont** (`unifont.hex` from
  Alpine's `unifont-misc`: 8x16 / 16x16 bitmaps for the BMP, doubled to 16x32 cells). Only rows
  that changed are redrawn. CJK takes two cells (`char_width`, also used by `fit()`), combining
  marks are skipped, U+FFFD for missing glyphs. Arabic: letters mapped to their joined
  presentation forms (isolated/final/initial/medial + lam-alef ligatures, Persian letters
  included); RTL runs reversed in a left-to-right line, numbers after RTL text join the run,
  brackets mirrored, width kept (a ligature is padded). Not a full UBA/HarfBuzz: good enough
  for names (the whole method, with worked examples: [14-text-rendering.md](14-text-rendering.md)). Falls back to the plain console if the font or a 32 bpp fb0 is missing;
  `H96UI_CONSOLE=1` forces the console. SIGTERM/SIGINT/SIGHUP and a normal exit put tty1 back
  in `KD_TEXT`; mpv (software path) gets the console via suspend/resume. RSS 17 MB with the
  font. Checked by reading fb0 back into a PNG; a multi-script sample renders with
  `H96UI_FONT=… H96UI_RENDER_OUT=… cargo test render_sample -- --ignored`.
- musl's `ioctl()` takes an `int` request (glibc: `unsigned long`): `KDSETMODE as _`.
- **Favorites:** the remote's Google button sends **KEY_SEARCH (217)** on the Consumer Control
  input (captured with raw `cat /dev/input/event4`: evtest piped through grep wrote nothing,
  its stdout is block-buffered and the timeout killed it). In any channel/video list it stars or
  unstars the selection (★ in the right column), during playback the current channel (OSD
  note), in Favorites it removes. Main menu → **Favorites**: A-Z, live ones zap among each other
  with Up/Down, videos play alone; hold OK = remove. Stored as `/var/lib/h96ui/favorites.m3u`
  (a normal playlist, headers as `#EXTVLCOPT` and the Kodi `|` suffix, round-trip tested),
  **mode 0600: Xtream URLs carry the account password** -- a provider file for the rules.
  Keyboard: `*` or `f`.
- **0.7.2:** each subscription has its own "Favorites (n)" at the top, picked by stream URL (an
  M3U subscription's playlist entries; an Xtream account's `server/…/<user>/…` URLs), so nothing
  is tagged in favorites.m3u and favorites starred earlier are included; the main menu still
  lists all. Starred entries are drawn in Settings > Favorite color (cyan = the bars' colour,
  yellow, green, magenta, red; bright text, and a bar of that colour when selected).
