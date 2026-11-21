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
