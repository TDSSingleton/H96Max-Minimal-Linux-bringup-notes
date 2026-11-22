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
  Box WiFi measured ~200 Mbit/s from the LAN (64 MiB over HTTP); the host's internet ~90 Mbit/s;
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
