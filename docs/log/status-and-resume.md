# Status and how to resume

Part of the session log; index in [README.md](README.md). **Current state only** (2026-11-22,
night). How we got here: [bringup-journal.md](bringup-journal.md). When something is broken:
[troubleshooting.md](troubleshooting.md). Technical detail: `research/notes/` (index
`research/notes/README.md`, §1–§40).

## Where the box is

**A working minimal Linux IPTV stick.** Our 64-bit u-boot, vendor 6.1.172 kernel (#5), Alpine
3.24.4 on the old Android `userdata`, WiFi + Bluetooth at boot, the stick's **Bluetooth remote**
driving **EBI 0.7.1** ("Extremely Basic IPTV", the Rust frontend; binary, paths and User-Agent
keep the name h96ui) on the HDMI console. **Access: ssh with the project key only; root has no
usable password** (`*`), so no console login either (2026-11-22). **Hardware video:** VPU decode
(Rockchip MPP) → zero-copy NV12 → a VOP2 display plane, through our own player **h96player**
(GStreamer playbin3, C). No GPU, by choice: the UI stays a text console, and video is shown on a
hardware layer above or inside it. Android is not bootable (`boot`/`uboot` replaced; `super` etc.
still on the eMMC).

What h96ui does: subscriptions from a USB stick (M3U files, URL lists, **Xtream accounts**),
live channels vs. on-demand videos split per subscription, Xtream Live TV / Movies / Series
(categories, seasons, episodes, account status), playback with Up/Down channel switching,
OK pause, Left/Right/Up/Down seek, volume and mute, an info box (the video shrinks and the console
draws title, time bar, volume around it), playlist HTTP headers (User-Agent/Referer), a
prebuffer ("Network cache"), and the remote's Back/Home/OK read straight from evdev. Hold OK
(or press Menu) on a subscription to remove it.
**New in 0.6 (2026-11-22, NOTES §41-§42):** "Add WiFi from USB" (a `.txt` with `ssid=` /
`password=` lines; saved to wpa_supplicant.conf, then h96-wifi restarts), so a reflashed box gets
WiFi without ssh. Menus: Vol- / Vol+ jump to the next / previous letter, holding Up/Down
autoscrolls (faster the longer it is held), and lists are A-Z by name with the provider's shared
leads (`|FR| `, `[Italy] `, `[IT] [Serie] `, `ITALY: `) ignored (Settings > List order goes back to
the provider's order). Playback: audio is downmixed to stereo before HDMI (Settings > Audio
channels): 5.1 tracks had failed with "Could not get/set settings from/on resource", the HDMI
codec refusing channels the TV has no speakers for. h96player errors now name the element and
the real reason; logs in `/tmp/h96ui-player.log` and `/tmp/h96player-gst.log`.
**New in 0.7 (NOTES §43):** EBI draws its own text on the framebuffer with GNU Unifont
(`unifont-misc`), so Arabic, Hebrew, Japanese, Chinese and Korean names show (Arabic joined,
right-to-left); `H96UI_CONSOLE=1` falls back to the kernel console. **Favorites:** the remote's
Google button stars a channel/video (lists, and while playing); main menu → Favorites (all),
and each subscription's own Favorites at its top (0.7.2); starred entries in a colour chosen in
Settings > Favorite color.
`/var/lib/h96ui/favorites.m3u` holds provider stream URLs (Xtream passwords): off-limits like
the subscriptions.
**WiFi:** the SeekWave driver's rx reorder buffer is switched **off** at boot by `h96-wifi`: with
it on, distant IPTV servers ran at ~0.3 Mbit/s and no live channel played (NOTES §40).

| eMMC area | Content | Hash / source |
|---|---|---|
| sector 64 (idbloader) | **factory**, never touched (only board-tuned DDR init) | — |
| `uboot` @0x4000 | mainline u-boot v2026.07 + patches 0001/0002 | `build/uboot/u-boot.itb` `03840459…` |
| `boot` @0xc800 | kernel #5 (BT + usbnet, no gadget), factory DTB + fbdev, initramfs stage1-2 | `build/out/linux-boot.bt-56fdbede.img` |
| `userdata` @6803456 | Alpine image `8af117b5…` (512 MiB) **plus live changes over ssh**, grown to 11.2 GiB | — |
| everything else | factory (Android `super`, `cache`, `backup`, `recovery`, …) | `recon/live-full/full.img` |

**Source of truth for a reflash:** `build/rootfs/h96root.img` **`3749043b…`** (1 GiB, built
2026-11-22 early, **not written**). **Out of date since the 0.6 work:** it has h96ui 0.5.1, the old
h96player (no `-C`, no detail lines), root's empty password and the old sshd drop-in. The
sources have all of it (`build-rootfs.sh` now sets root to `*`): rebuild it (make-inbox, then
build-rootfs) before it is ever written. It reproduces the live box including the video stack (GStreamer,
MPP, librga, gstreamer-rockchip, h96player, patched kmssink), the Rust h96ui and the fixup kit,
**except** the box-only items: WiFi PSK (`/etc/wpa_supplicant/wpa_supplicant.conf`), BT address
(`/var/lib/skwbt/skwbdaddr`), the remote's pairing (`/var/lib/bluetooth/…`) and the subscriptions
(`/var/lib/h96ui/`). Back those up before any userdata write.

**Live on the box but not in the image:** debugging aids `gdb` and the `*-dbg` packages
(gstreamer, gst-plugins-good/base, glib, musl: remove with `apk del` when done); the old Go
h96ui as `/usr/local/bin/h96ui-go`; `/usr/local/bin/h96play` (gst-launch prototype, superseded
by h96player); `/root/libgstkms.so.alpine` and `/root/test-streams.m3u.bak`. **Test data:** the
subscription "192.168.11.110 8766" is the **mock Xtream panel** on the PC (demo/demo123); remove
it in h96ui (or delete `/var/lib/h96ui/subs/192.168.11.110 8766.xtream`) once a real account
is in. "Test streams" has Big Buck Bunny and Apple's bipbop marked as videos.

**Fixup kit installed** (`push-to-box.sh`, last run 2026-11-22 ~20:30): EBI 0.6.4 in
`/usr/local/bin/h96ui` (now 0.7.1), `h96ui.prev` = 0.7.0 (`h96-fix ui-rollback`; any build from 0.6.4 on runs with the new
h96player, the reverse does not: the old h96player has no `-C`, and **no copy of the previous
h96player is kept**), h96player in `/usr/bin`, kmssink reference copy in `/usr/local/share/h96`,
`h96-doctor` 0 problems (it now also checks root's password and key-only sshd). Old auth files
from before the lock-down: `/root/auth-backup-20261122/` (shadow with the empty root password,
the previous h96.conf).
**The user's father's Xtream subscription ("Vodkom") is on the box: never read its account
file, call its server or play its channels** -- the user tests it.

## Reaching and recovering it

- **ssh:** `H96=192.168.11.149 tools/h96` (WiFi) or `H96=192.168.11.145 tools/h96` (Realtek
  USB-Ethernet dongle alone on USB-A). Key `build/keys/h96max_ed25519`, project-local
  known_hosts. **That key is the only way in** (no root password, no password ssh): keep a
  backup of it. Without it: maskrom + rewrite `userdata`, or edit the ext4 from the PC.
  Just after a reboot the box can answer "No route to host" for a minute or two: wait and retry. Copy files: `H96=… tools/h96 put LOCAL REMOTE`. If the address moved: sweep the
  /24 and match MAC `fe:fd:fc:e6:49:48` (WiFi) or `00:e0:4c:36:13:c2` (dongle) in `ip neigh`.
- **Health check / repairs:** `tools/h96 h96-doctor`, then `h96-fix <problem>`;
  see [troubleshooting.md](troubleshooting.md).
- **HDMI:** h96ui on tty1; u-boot shows nothing (~5 s black). Turn the TV on **before** the box:
  with the TV off, the EDID comes back empty, HDMI falls to 720p and the console is squashed
  (`h96-fix display`).
- **Remote:** Bluetooth "Bluetooth remote" `60:48:9C:D6:DF:C3`; reconnects on a key press
  after boot. Keys (`build/wifi/remote-keys2.txt`): arrows via the console; OK (KEY_SELECT 353),
  Back (158), Home (172), Menu (240), Vol+/- (115/114), Mute (113) via evdev. The Consumer
  Control device (event4) has **no EV_REP**: a held key never repeats by itself, so EBI times
  Up/Down press/release from evdev for autoscroll. Vol keys: volume while playing, letter
  jump in menus. Re-pair:
  `bluetoothctl` -> `agent NoInputNoOutput`, `default-agent`, `scan on`, hold the remote's
  pairing combo, `pair/trust/connect <mac>`.
- **Maskrom:** hold the recovery button at power-on (u-boot patch 0002), or `reboot-loader`
  from Linux, or any `boot`-partition failure (automatic). Then `rkdeveloptool db
  tools/loaders/factory-loader/L5b_factory_ddr.bin`, write with read-back. The device can take
  a few seconds to enumerate (descriptor timeouts first): retry `rkdeveloptool ld`.
- **Power:** 2 A supply on USB-C. The PC's port browns out a full boot; the USB-A port cannot
  power a bus-powered hub + Ethernet dongle + USB stick together (the stick is then not seen at all).
- **WiFi/BT chip:** never reload one module alone. After a chip crash a warm reboot is not
  enough -> `h96-fix wifi-bt` (NOTES §32), or a cold power cycle.

## Rebuilding pieces

```bash
# all builds in containers (fedora:41 for kernel/u-boot/modules/h96ui, arm64 alpine for the rest),
# started from the project root ("$PWD" is mounted as /work)
ROOT=/work bash /work/tools/make-linux-boot.sh              # kernel + initramfs + boot image
ROOT=/work bash /work/tools/build-uboot.sh                  # u-boot.itb
ROOT=/work bash /work/tools/uboot/test-bootcmd-sandbox.sh   # run bootcmd.hush on the real image
ROOT=/work bash /work/tools/wifi/build-seekwave.sh          # skw_sdio_v20.ko, skw6316.ko, skwbt.ko
podman run --rm -v "$PWD":/work:z fedora:41 \
  bash /work/tools/h96ui-rs/build.sh                        # h96ui (Rust, static arm64 musl) + tests
podman run --rm --arch arm64 -v "$PWD":/work:z \
  [-e STAGES="mpp rga ffmpeg gst kms player"] docker.io/library/alpine:3.24.4 \
  sh /work/tools/video/build-mpp.sh                         # video stack -> build/video/video-stage.tar.gz
                                                            # (STAGES=player: h96player only, ~1 min)
bash tools/make-inbox.sh                                    # payload tarball (needs build/h96ui/h96ui)
podman run --rm --arch arm64 -v "$PWD":/work:z \
  docker.io/library/alpine:3.24.4 sh /work/tools/build-rootfs.sh   # rootfs image (needs the two above)
H96=192.168.11.149 tools/fixup/push-to-box.sh               # install h96ui + video stack + fixup kit live
```
After a kernel or `/init` change: move `build/kernel/kernel` aside first (the pipeline caches it).
After a kernel rebuild: rebuild the SeekWave modules too. Iterating on the box: build in the
container, `push-to-box.sh` (or `tools/h96 put`), then fold the change back into the sources.
Kernel config changes go through `tools/build-kernel.sh` (enables + asserts); beware its
"trim" list, which runs after the enables. The video build applies `tools/video/<pkg>/*.patch`
to the submodules in place (`downloads/mpp` therefore shows as modified: expected). The kms
stage pins Alpine's exact gst-plugins-bad (`GPB_VER`/`GPB_SHA` in `build-mpp.sh`), and
`build-rootfs.sh` refuses to build if Alpine has moved on.
Testing without a real IPTV account: `python3 tools/h96ui-rs/testdata/mock-xtream.py` on the PC
(Xtream panel demo/demo123 on port 8766), `tools/video/seek-test.sh`, `seg-speed.sh`.

## Rules

See [rules-and-lessons.md](rules-and-lessons.md): no flash writes without explicit consent, never
sector 64 / `ef` / `ul` / `gpt` / `armbian-install`; only the **original Android** must stay off
the network. The UI stays a CLI/console (no GPU, no graphical UI).

## Open items, roughly in order

0. **Try 0.7.1 on the TV** (not yet confirmed by the user): favorites (Google button in a list
   and while playing), non-Latin names in real lists; and from 0.6: autoscroll on a long list, Vol-/+
   letter jumps in a French/Italian category, A-Z order, a film that had failed with the audio
   error, Add WiFi from USB with a test file.

1. **Fix the WiFi driver's rx reorder logic** (likely the sequence-number/BA fields of our rx
   descriptor port, NOTES §27/§40), then turn the buffer back on and re-measure against a distant
   server (box vs PC, same segment).
2. **Xtream with the real account:** the user tests Vodkom themselves; the mock subscription
   "192.168.11.110 8766" can be removed (hold OK on it).
3. **OSD drawn over the video** (user request: a black bar at the bottom with the time): needs a
   layer above the video, plane 75 (Esmart2-win0, ARGB with alpha), drawn by h96player, which
   must share kmssink's DRM fd (NOTES §37 end).
4. Deinterlacing for SD channels (IEP2 hardware, `mpp-iep2`); a non-blocking atomic presenter for
   clean 60p; Menu key (code 240) unused.
5. Hygiene: **rebuild `h96root.img`** (it predates 0.6 and the access lock-down), then reflash
   userdata from it at the next structural write (keep PSK, BT address, pairing,
   subscriptions!); `apk del gdb *-dbg` on the box; the debug hex dump in skw6316 (GET_INFO) prints
   once per boot; `h96-wifi` should fail fast on a crashed chip; skw_sdio recovery worker oops
   (`build/wifi/bt-oops1.txt`).
6. Kernel cmdline still carries `loglevel=8` and factory `androidboot.*` arguments; clean up at the
   next boot-image build. Reclaiming Android's `super`/`cache`/`backup` (~3 GiB) needs a GPT
   rewrite: later.
