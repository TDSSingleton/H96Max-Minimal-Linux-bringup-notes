# Status and how to resume

Part of the session log; index in [README.md](README.md). **Current state only** (2026-11-21,
late evening). How we got here: [bringup-journal.md](bringup-journal.md). Technical detail:
`research/notes/` (index `research/notes/README.md`, §1–§33).

## Where the box is

**The hardware layer is done.** A minimal arm64 Linux TV stick: our 64-bit u-boot, vendor 6.1.172
kernel (#5, with Bluetooth), Alpine 3.24.4 on the old Android `userdata`, WiFi and Bluetooth at
boot, the stick's **Bluetooth remote paired** and driving the **h96ui** console menu on HDMI,
ssh key-only. Next job: **hardware video (VPU) and maybe GPU** -- starting point NOTES §33.
Android is no longer bootable (`boot`/`uboot` replaced; `super` etc. still on the eMMC).

| eMMC area | Content | Hash / source |
|---|---|---|
| sector 64 (idbloader) | **factory**, never touched (only board-tuned DDR init) | — |
| `uboot` @0x4000 | mainline u-boot v2026.07 + patches 0001/0002 | `build/uboot/u-boot.itb` `03840459…` |
| `boot` @0xc800 | kernel #5 (BT + usbnet, no gadget), factory DTB + fbdev, initramfs stage1-2 | `build/out/linux-boot.bt-56fdbede.img` |
| `userdata` @6803456 | Alpine image `8af117b5…` (512 MiB) **plus live changes over ssh**, grown to 11.2 GiB | — |
| everything else | factory (Android `super`, `cache`, `backup`, `recovery`, …) | `recon/live-full/full.img` |

**Live changes on the box's rootfs (since the last userdata flash):** h96ui on tty1,
`h96-console` (TV-out off, 1080p, 16x32 font, `dmesg -n 4`), `h96-wifi` with the fixed
`skw6316.ko`, `h96-bt` + `skwbt.ko` + `/lib/firmware/sv6160lite.nvbin`, dbus + bluetooth (BlueZ
5.86), chronyd, no savecache, `rc_logger`, packages mpv, font-terminus, bluez, bluez-btmon,
libgpiod, v4l-utils, tools `gpio-watch`, `/var/lib/skwbt/skwbdaddr` (BT address),
BlueZ pairing of the remote (`/var/lib/bluetooth/56:6A:ED:AA:1C:D1/60:48:9C:D6:DF:C3`) and
`/etc/wpa_supplicant/wpa_supplicant.conf` (PSK hash). The last two plus the BT address exist
**only on the box**.

**Source of truth for a reflash:** `build/rootfs/h96root.img` `ba031ca2…` (1 GiB = 2,097,152
sectors, built, **not written**) reproduces everything above except the box-only secrets/pairing
(re-run `wpa_passphrase`, re-pair the remote; a new BT address would be generated).

Resources in use: ~425 MB of 11.2 GB, ~50 MB RAM of 2 GiB, CPU idle.

## Reaching and recovering it

- **ssh:** `H96=192.168.11.149 tools/h96` (WiFi) or `H96=192.168.11.145 tools/h96` (Realtek
  USB-Ethernet dongle alone on USB-A). Key `build/keys/h96max_ed25519`, project-local
  known_hosts. Copy files: `H96=… tools/h96 put LOCAL REMOTE`. If the address moved: sweep the
  /24 and match MAC `fe:fd:fc:e6:49:48` (WiFi) or `00:e0:4c:36:13:c2` (dongle) in `ip neigh`.
- **HDMI:** h96ui on tty1 (WiFi/clock bar, menus, versions); u-boot shows nothing (~5 s black).
  If the TV loses the picture: `echo detect > /sys/class/drm/card0-HDMI-A-1/status`, or replug.
- **Remote:** Bluetooth "Bluetooth remote" `60:48:9C:D6:DF:C3`; reconnects on a key press
  after boot. Re-pair: `bluetoothctl` -> `agent NoInputNoOutput`, `default-agent`, `scan on`,
  hold the remote's pairing combo, `pair/trust/connect <mac>`.
- **Maskrom:** hold the recovery button at power-on (u-boot patch 0002), or `reboot-loader`
  from Linux, or any `boot`-partition failure (automatic). Then `rkdeveloptool db
  tools/loaders/factory-loader/L5b_factory_ddr.bin`, write with read-back. The device can take
  a few seconds to enumerate (descriptor timeouts first): retry `rkdeveloptool ld`.
- **Power:** 2 A supply on USB-C. The PC's port browns out a full boot; the USB-A port cannot
  power a bus-powered hub + dongle + stick together.
- **WiFi/BT chip:** never reload one module alone. After a chip crash a warm reboot is not
  enough (boot hangs in h96-wifi, sshd never starts) -> cold power cycle, or in a running system
  rmmod skwbt skw6316 skw_sdio_v20 + unbind/bind `ffc20000.mmc` (NOTES §32).

## Rebuilding pieces

```bash
# all builds in containers (fedora:41 for kernel/u-boot/modules/h96ui, arm64 alpine for the rootfs)
ROOT=/work bash /work/tools/make-linux-boot.sh              # kernel + initramfs + boot image
ROOT=/work bash /work/tools/build-uboot.sh                  # u-boot.itb
ROOT=/work bash /work/tools/uboot/test-bootcmd-sandbox.sh   # run bootcmd.hush on the real image
ROOT=/work bash /work/tools/wifi/build-seekwave.sh          # skw_sdio_v20.ko, skw6316.ko, skwbt.ko
bash /work/tools/h96ui/build.sh                             # h96ui (needs golang)
bash tools/make-inbox.sh                                    # payload tarball (also used by the rootfs)
podman run --rm --arch arm64 -v ~/Sources/ClaudePlayground/H96Max:/work:z \
  docker.io/library/alpine:3.24.4 sh /work/tools/build-rootfs.sh   # rootfs image (1 GiB)
```
After a kernel or `/init` change: move `build/kernel/kernel` aside first (the pipeline caches it).
After a kernel rebuild: rebuild the SeekWave modules too. Iterating on the box: build in the
container, `tools/h96 put`, apply, then fold the change back into `tools/rootfs/payload`.
Kernel config changes go through `tools/build-kernel.sh` (enables + asserts); beware its
"trim" list, which runs after the enables.

## Rules

See [rules-and-lessons.md](rules-and-lessons.md): no flash writes without explicit consent, never
sector 64 / `ef` / `ul` / `gpt` / `armbian-install`; only the **original Android** must stay off
the network.

## Open items, roughly in order

1. **Hardware video (next job):** VPU decode (Rockchip MPP) + VOP2-plane display; maybe GPU
   (Mali-450, lima or libMali) for scaling/OSD. Software playback is 0.25-0.33x realtime
   (NOTES §30). Starting point and **agreed first step** (build MPP + decoder on Alpine arm64,
   VPU-decode one test stream, show it on a VOP2 plane, only then the player): NOTES §33.
   h96ui's player is one function (`play.go`).
2. Remote keys beyond navigation: map Back/Home/volume to h96ui/mpv actions (log of the keys:
   `build/wifi/remote-keys1.txt`; navigation = KEY_UP/DOWN/SELECT on Consumer Control).
3. Hygiene: fold the live changes into a userdata flash at the next structural write (keep the
   PSK, BT address and pairing!); root has no password (ssh key-only); the debug hex dump in
   skw6316 (GET_INFO) prints once per boot; `h96-wifi` should fail fast instead of hanging on a
   crashed chip; the skw_sdio recovery worker can oops after a chip assert (`build/wifi/bt-oops1.txt`).
4. Kernel cmdline still carries `loglevel=8` (console quieted at runtime by h96-console) and
   factory `androidboot.*` arguments; clean up at the next boot-image build.
5. Reclaiming Android's `super` / `cache` / `backup` (~3 GiB) would need a GPT rewrite: later.
