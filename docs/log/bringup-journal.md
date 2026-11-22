# Bring-up journal: from first flash to a networked box (2026-11-21)

Part of the session log; index in [README.md](README.md). This file is **history** (what was done,
why, and what came out). The current state and how to resume are in
[status-and-resume.md](status-and-resume.md); deep technical detail is in `research/notes/`
(§22–§28). The timestamped entries written during the day are kept verbatim in the appendix.

## Milestones

| # | Milestone | Evidence |
|---|---|---|
| 1 | 64-bit u-boot built from mainline v2026.07, boot script executed in u-boot sandbox | NOTES §23, `build/uboot/sandbox-*.log` |
| 2 | First flash (`uboot` + `boot`), **first boot**: factory SPL -> our u-boot -> arm64 kernel -> HDMI | photo `20261121_124624.jpg` |
| 3 | Recovery button works with our u-boot (patch 0002), `reboot loader` -> maskrom (patch 0001) | user test, `runattempts/1` |
| 4 | Alpine 3.24.4 rootfs in the old Android `userdata` (11.2 GiB), info screen on HDMI | photo `20261121_144939.jpg` |
| 5 | LAN via USB-Ethernet dongle, ssh key-only | box at 192.168.11.145 |
| 6 | WiFi driver ported (SeekWave SWT6621S), chip boots, scan on 2.4 + 5 GHz | NOTES §26 |
| 7 | **WPA2 connects** after rewriting the rx descriptor from the ODM driver | NOTES §27, `build/wifi/live-*.txt` |
| 8 | WiFi starts at boot; box runs on WiFi alone at 192.168.11.149 | NOTES §28 |
| 9 | h96ui console menu (Go) on HDMI; full 1080p console via h96-console | NOTES §29, photos |
| 10 | Software video measured: not viable (0.25-0.33x realtime) | NOTES §30 |
| 11 | No IR receiver exists (TV stick); CEC unavailable on the TV | NOTES §31 |
| 12 | Bluetooth up; the stick's BLE remote paired, drives h96ui, survives reboots | NOTES §32 |

## A. Bootloader and first boot

- **Why a new u-boot:** the kernel is arm64 by decision; the factory u-boot is AArch32 and cannot
  switch the CPU to AArch64 (NOTES §22). Mainline v2026.07, `generic-rk3528` base, our DT
  `tools/uboot/rk3518-h96max.dts` (eMMC + SARADC), BL31 v1.21, no OP-TEE. Built by
  `tools/build-uboot.sh` in fedora:41 (needs `openssl-devel-engine`); idbloader outputs deleted
  so sector 64 (the only board-tuned DDR init) can never be overwritten.
- **Boot script** (`tools/uboot/bootcmd.hush`): reads the GPT `boot` partition, `abootimg` for
  DTB/ramdisk, `unzip` the Image.gz, bootargs = DTB `/chosen/bootargs` + header cmdline (+ u-boot
  breadcrumbs `h96.adc=`/`h96.bootmode=`), `booti`. Any failure -> BootROM download (maskrom).
  Executed for real in u-boot sandbox against a disk with the box's GPT
  (`tools/uboot/test-bootcmd-sandbox.sh`) before every flash.
- **First boot** worked straight away: the factory SPL loads a mainline FIT and passes DRAM size
  (2 GiB). HDMI console via fbcon; probe initramfs report to a USB stick.
- **Recovery button:** at first it did nothing. Linux measured it (saradc ch1: ~1019 released,
  ~10 held), proving u-boot's channel/threshold right; the bug was mainline's
  `rockchip_dnl_key_pressed()` only accepting an ADC device named `saradc*` while the upstream DT
  names it `adc@ffae0000` -> patch 0002 (+ breadcrumbs). Patch 0001 maps Linux `reboot loader`
  to BootROM download.
- **Initramfs** (`tools/mk-initramfs.sh`): fixed a 1 s race with the USB stick (wait up to 20 s),
  added `/dev/console` (appended newc archive), probe report + USB tree to the stick, IR test
  opt-in (`h96_remote=1`), mounts our rootfs **only if** its label is `h96root` (Android's old
  ext4 can never be mounted rw), stick inbox (`h96-inbox.tar.gz` unpacked once), `switch_root`.

## B. Alpine on the old `userdata`

- `tools/build-rootfs.sh` in an arm64 Alpine container: 512 MiB ext4 image LABEL=h96root,
  written to the start of `userdata` (LBA 6803456) over maskrom with read-back, grown to the
  whole 11.2 GiB on first boot (online resize2fs). No partition-table change.
- Overlay/payload: `tools/rootfs/overlay` (fstab, hostname, first-boot resize) and
  `tools/rootfs/payload` (inittab, info screen `h96-info`, stick report, LAN, WiFi, sshd key-only
  with `build/keys/h96max_ed25519`). `tools/make-inbox.sh` packs the payload; the rootfs build
  unpacks the same tarball (one source of truth).
- Lessons: busybox `ip` has no `-br` (installed iproute2); a background job started from
  OpenRC `local` can be reaped (stick report now runs from inittab `::once`).

## C. Getting a remote link (three tries)

1. **USB gadget on the USB-C port** (ACM + NCM, ssh on 10.96.96.1): worked on the box side
   (DWC3 in device mode), but the PC's USB-C cannot power a full boot (brown-out), and on a 2 A
   charger the PHY sees a dedicated charging port (no data). **Removed** at the user's request.
2. **USB-Ethernet dongle** (Realtek RTL8152 combo with a built-in hub): enumerated and bound
   `r8152` -> `eth1`, DHCP running, but **NO-CARRIER** while the USB stick shared its hub. Same
   dongle on the PC: carrier in 2 s. Dongle **alone** on the box: link + DHCP -> ssh worked.
   Conclusion: power budget of the box's USB-A port. Kernel since then also has usbnet +
   AX88179/178A, AX8817x, CDC-Ether/NCM/EEM, RNDIS, SMSC95xx.
3. **WiFi** (below) made the dongle optional.

## D. WiFi: SeekWave SWT6621S

- **Source:** Neardi RK3588 SDK (kernel 5.10): `seekwaveplatform_v20` (SDIO + boot) and
  `rockchip_wlan/skwifi` (cfg80211), 81 % / 69 % string kinship with the ODM's binaries.
  Ported to 6.1 as external modules (`tools/wifi/build-seekwave.sh`): SDIO id 0x6621, DT
  `seekwave,sv6160lite`, firmware names as module params (ODM `SWT6621S_{IRAM,DRAM,NV}_SDIO.bin`),
  `CHIP_ID="SWT6621S"` for the calibration file, four 6.1 API fixes. Pristine copy in
  `tools/wifi/neardi-orig/` for diffing.
- **Firmware compatibility** was checked offline first (image markers, load addresses, 7
  segments) and held on the box: chip boots, firmware 20250618, `WIFIREADY`, wlan0, scan.
- **The handshake failure** (reason 15) was the newer firmware's 22-byte rx descriptor: with the
  driver's 20-byte layout, msdu_len read 0, the EAP filter class read 0 (frames dropped by the
  pre-auth filter) and a length byte was taken as TID 10. Found by hex-dumping dropped frames on
  the box (EAPOL-Key 1/4, intact) and **disassembling the ODM's own `swt6621s_wifi.ko`** (ARM32) to
  read every field offset; the new struct decodes the captured frame consistently. Note: a first
  "non-QoS TID" hypothesis was wrong and was removed.
- **Boot integration:** OpenRC `h96-wifi` (insmod both, wpa_supplicant, udhcpc). It was skipped
  at boot twice: files edited while the box clock (no RTC) said "Jun 14" had mtimes older than the
  dependency tree. chronyd + no savecache + correct mtimes fixed it.
- Live debug loop that made this fast: build module in the container, `tools/h96 put`, reboot
  (never reload only the WiFi module: chip recovery fails), insmod, read dmesg.

## E. The console interface (h96ui)

- Lightest option chosen with the user: a compiled console menu + mpv as player (measured
  options: dialog ~0 MB, mpv +352 MB, Kodi +189 MB on top; LVGL not in Alpine). Go, stdlib only,
  static arm64 6 MB (`tools/h96ui/`): top bar (WiFi SSID/signal/IP, clock), Subscriptions,
  Add from USB (no typing: .m3u/.m3u8 files and URL lists), Settings (mpv cache, deinterlace,
  volume, audio HDMI/SPDIF/analog, picture), About; footer with versions. `--snapshot` renders
  screens as text, which is how the layout was checked over ssh.
- First run was squashed into the top-left: the DRM fbdev console was sized for the analog
  TV-out (720x576) sharing the framebuffer with HDMI. `h96-console` turns TV-out off, sets
  1920x1080 and a 16x32 font (120x33) -- and later `dmesg -n 4`, because kernel messages
  (cmdline `loglevel=8`) flooded the menu.
- mpv works (H.264 1080p60 decodes, audio to HDMI) but far too slowly in software; the
  `--vo=drm` scaling path is the worst part. -> the VPU job.

## F. Input: the IR receiver that did not exist

- Vendor DT describes an IR receiver on PWM3 (GPIO4_C6). Zero frames, zero interrupts, PWM3
  capture counters 0, pin held low even with a pull-up. A custom `gpio-watch` (samples every
  GPIO pin ~480 k/s) showed no pin toggling during remote presses. Every software explanation was
  checked and ruled out (dtbo, DTB copies, kernel config, Android init, power rails, hogs).
- The user then confirmed: it is a TV stick with no IR window; the remote is Bluetooth (with an
  IR emitter for the TV only). CEC was tried as a fallback (box joined the CEC bus as a playback
  device); the TV does not support CEC. Lesson: ask about the physical device early -- an hour of
  careful elimination ended in a hardware fact the user could see.

## G. Bluetooth

- Kernel: BT was silently disabled by an old milestone-1 "trim" list in `build-kernel.sh`; the
  build's own assertion caught it. Fixed, kernel #5 flashed (`56fdbede…`).
- Driver `skwbt` (Neardi) ported; first load crashed the chip's firmware and took WiFi down with
  it (wrong NV blob for the lite chip). The factory vendor partition had `sv6160lite.nvbin` and
  the factory BT helper's strings showed the selection logic. Second load got further and crashed
  on one HCI command; a `btmon` trace pinned it (#81 Read Local Supported Codec Capabilities,
  0x100e); the driver now masks the codec commands in the supported-commands reply.
- A chip reset without a power cycle (unbind/bind the SDIO host) made the crash/retry loop
  self-service. Pairing with bluetoothctl (`NoInputNoOutput` agent) while the remote blinked;
  BD address persisted so the pairing survives reboots (verified with a reboot).
- Side effects handled: a crashed chip makes the next warm boot hang in h96-wifi (needs a cold
  cycle); BT was disabled at boot while debugging and re-enabled once stable.

## H. Lessons from the day

- Verify on the device what a tool claims: a sandbox run of the boot script, a decoded frame,
  a disassembled reference driver beat reasoning about layouts.
- Power is a first-class suspect on this box (PC USB-C, bus-powered hubs on USB-A).
- No RTC: time-dependent tooling (OpenRC mtimes, TLS later) needs NTP early.
- Prefer maskrom writes with read-back for anything structural (user preference); live ssh
  changes for iteration, then fold them into the build sources.
- Ask early about what the user can see or touch (connectors, windows, LEDs, the remote's
  pairing mode) before deep software elimination.
- A build that asserts what it asked for (kernel options, module vermagic, image contents)
  catches the silent failures; three of today's bugs were found that way.
- Wireless firmware for a combo chip is shared: a crash on the BT side takes WiFi down. Bring up
  the risky half by hand (not at boot) until it is proven.

---

## Appendix 1: background carried over from the session log

## 1. Goal drift, and why it was correct

Started as: root the box by fixing the ramdisk, install **GammaOS**.

- **minimal Linux TV box**. This is the better goal on three counts
  — ~1–2 GB RAM + Mali-450 is badly undersized for Android 14; a rootfs we build removes the
  OTA/PCDN attack surface *structurally* instead of needing a quarantine VLAN; and Armbian on
  this family is proven ("Working: Ethernet, HDMI console, SD, eMMC, USB, Wi-Fi, Bluetooth").
- "Root by fixing the ramdisk" is also the wrong era: stock is Android 14 / boot header v2 /
  kernel 6.1.118. Root there means a Magisk patch of boot.img, not ramdisk `su` injection.
  Moot now — Android is abandoned by user decision ("Android can go die in a ditch").

## 9. Where the u-boot build stands, and the architecture change

- Our own v2026.07 build compiles u-boot fully (`LD u-boot`, `u-boot.bin` 913,088 B) then dies
  on `make[1]: No rule to make target 'checkarmreloc'`, a relocation lint whose prerequisite is
  added by `arch/arm/config.mk` (`ifneq ($(CONFIG_XPL_BUILD),y)`) while the rule sits in the
  main `Makefile`. `CONFIG_BINMAN_DTB=""` and `u-boot.itb` has **no rule**, so the FIT was only
  reachable through `all` — the path the lint blocks. Unresolved.
- Community `u-boot.itb` (juliovendramini, RK3518) inspected and **rejected for our use**:
  it's `generic-rk3528_defconfig` → **no USB host at all**, and our stick has no SD slot, so it
  would be a bootloader with no input channel and no output channel. Contents verified anyway:
  AArch64 u-boot 2026.07-rc4 at `0x00800000`, **atf-1 = bl31-v1.21** (meets the ≥v1.20 RK3518
  floor), fits the 4 MiB `uboot` partition. `sha256 1ea0200d47b5…`.
- ~~Plan of record (2026-11-20 morning): do not replace the bootloader at all.~~ **Superseded
  2026-11-21 (NOTES §22):** the kernel is arm64 by decision and the stock u-boot is AArch32, so a
  64-bit u-boot is required. Kept below for the reasoning and the boot-image facts. Reading the
  factory `uboot.img` strings settled it — the stock bootcmd is

      bootcmd=boot_android ${devtype} ${devnum};boot_fit;bootrkp;run distro_bootcmd;

  i.e. the stock bootloader *already* boots the `boot` partition as an Android image, so our
  kernel needs a replacement image and not a replacement bootloader. The chain degrades safely:
  an unbootable `boot` falls through to `boot_fit`, `bootrkp`, then distro boot. `vbmeta.img` is an
  AVB0 shell with zero descriptors, but the boot header **does** carry the standard mkbootimg
  SHA-1 and u-boot checks it, so our builder recomputes it (NOTES §18). Full details and the measured layout are in NOTES §16.
  1. Build the kernel from the vendor 6.1.172 tree. **Open (NOTES §18):** the factory payload
     is a 32-bit ARM zImage; an arm64 `Image.gz` is a different form, and the stock u-boot
     starting an arm64 kernel is unverified. Build + `rk3518-evb1-ddr4-v10.dtb` + our probe initramfs.
  2. `tools/mk-bootimg.py` synthesises a 64 MiB `boot` image: template header copied verbatim
     with the size fields (incl. `dtb_size`) patched and the AOSP v2 SHA-1 id **recomputed**
     (u-boot checks it — NOTES §18), factory `resource` (RSCE) and dtb reused unchanged.
     Round-tripped against the factory payloads it reproduces `build/good/boot.img`
     **byte-for-byte over all 64 MiB**.
  3. Write `wl 0xc800` (**not** `0x4000`), verify by reading it back, undo = `wl 0xc800
     build/good/boot.img`. Needs explicit consent; it would be the first write ever.
  4. **Linux's console is HDMI** (`/dev/tty1` via DRM fbdev) — the report is written there and
     to a USB stick. No serial, no keyboard, no typing at a prompt nobody can see.
- u-boot therefore became moot *before* it was finished. For the record: `u-boot.itb` is a
  binman output, only reachable via `make .binman_stamp` (`all` dies on the `checkarmreloc`
  relocation lint, which is a post-hoc `readelf` check, not a build failure) —
  `BL31=<rk3528_bl31_v1.21.elf> make ARCH=arm .binman_stamp` if we ever want it.
- The 10:09 container build reported success but **left nothing in the mount**, and its log
  quoted `Makefile:1058` for `checkarmreloc` while our tree has that target at line 2501: it
  built a different checkout inside `--rm`'s disposable filesystem. `tools/bench-check.sh` now
  proves the mount, the tree identity and the cross compiler *before* a long build.
- `tools/mk-initramfs.sh` already builds the probe initramfs that reports RAM size, eMMC
  `life_time`/`pre_eol_info`, SDIO vendor:device, MDIO PHY presence, IR input devices, DRM
  connector status/modes, USB tree, and dmesg failures — to `/dev/tty1` and a USB stick (and
  eMMC `cache` **only** if `emmc_report=1` is on the cmdline, to honour the no-writes rule).

## Appendix 2: timestamped entries as written during the day (verbatim)

### 2026-11-21: first writes done (user consent given: "device is in Maskrom, we can try to flash")
Maskrom 2207:350c → `db` L5b_factory_ddr.bin (RAM only). Pre-write read-back of `uboot` and
`boot` matched the undo copies (`d4e00dae…`, `ccfd74cf…`). Written and verified by read-back:
`wl 0x4000 build/uboot/u-boot.itb` (cmp OK, 801,792 B) and `wl 0xc800 build/out/linux-boot.img`
(sha256 `630bd3bd…` echoed). Sector 64..0x4000 re-read after the writes: unchanged vs full.img.
First boot: pending.

### 2026-11-21 12:46: FIRST BOOT WORKED (photo 20261121_124624.jpg)
Factory SPL → our u-boot.itb → `booti` → vendor 6.1.172 arm64 kernel → `/init` at 5.49 s,
all on HDMI (fbcon). Also seen: USB stick enumerated (usb 3-1, sda1), SDIO card at mmc2
(the WiFi module), rk3528-acodec + SPDIF sound cards. So the factory-SPL-loads-mainline-FIT
and DRAM-sizing questions (NOTES §23) are answered: yes.
Problems: (1) report not written: /init checked for the stick at 5.56 s, sda1 appeared at
6.53 s; (2) "unable to open an initial console": no /dev/console in the cpio.
Fixed in probe-3 (`tools/mk-initramfs.sh`): waits up to 20 s for /dev/sd*, eMMC no longer a
candidate, /dev/console node appended as a second newc archive, penguin moved to the end,
halt after 10 min. Kernel unchanged (the boot.img ramdisk overrides the built-in one).
`build/out/linux-boot.img` sha256 `ac30b857…` (probe-2 kept as linux-boot.probe2.img).
Not yet written: needs maskrom, i.e. the first real test of the recovery button.

### 2026-11-21 ~13:00: recovery button does NOT reach maskrom with our u-boot
Held at power-on: box boots Linux as normal. Our u-boot reads saradc ch1 (raw <= 30), taken
from the factory u-boot's DTB -- which is the generic "RK3528 EVB" tree, so the channel is
probably not this board's wiring. Unknown without serial. Consequence: no route back to
maskrom from the probe-2 image except the hardware one (maskrom pads / eMMC CLK short).
probe-3 additions: a button test (samples all saradc channels + GPIO state released vs held,
into the report), and after the report it calls `/bin/reboot-loader` (tools/initramfs/
reboot-loader.c, raw reboot(RESTART2,"loader")) -> reboot-mode 0x5242c301 -> our u-boot patch
-> BootROM download. So the box returns to maskrom by itself ~3 min after each boot
(`h96_stay=1` on the cmdline disables). Image: build/out/linux-boot.img sha256 `ab0329ab…`.
Probe-3 written 13:08, read-back sha256 ab0329ab… OK; uboot re-read = ours. Entered maskrom via the hardware route.

### 2026-11-21 ~13:20: probe-3 report (runattempts/1/rk3518-report.txt) and the button bug
Report written to the stick (USB wait fix works). Button = **saradc ch1**: released ~1019,
held ~10 (10-bit). So u-boot's channel/threshold were right; the bug: mainline
`rockchip_dnl_key_pressed()` only accepts an ADC device named `saradc*`, and upstream
rk3528.dtsi names the node `adc@ffae0000` -> never found -> "not pressed".
Fix: tools/uboot/0002-*.patch (also matches `adc@`, and leaves breadcrumbs: env h96_adc /
h96_adc_err / h96_bootmode, appended by bootcmd to bootargs as h96.adc= h96.adcerr=
h96.bootmode= so the Linux report shows what u-boot saw). New build/uboot/u-boot.itb sha256
`03840459…` (802,304 B), not yet written. Whether the reboot-loader self-return works is still
unknown (the box was on TV power, where maskrom would be invisible anyway).
Other report facts: RAM 2 GiB (memory node 0x7fe00000 from u-boot); eMMC Samsung R1J96N
life_time 0x01/0x01, pre_eol 0x01 (healthy, not the worn eMCP feared); WiFi SDIO 1ffe:6621
(SeekWave, as predicted); HDMI connected 1080p, fb0+fbcon; eth0 + MDIO PHY id 0x00441400
present (RJ45 question: the MAC/PHY exist); IR remote (pwm remotectl) and adc-keys inputs
registered; OP-TEE probe fails (no BL32, expected).
13:25 wrote u-boot.itb 03840459… (patch 0002) to 0x4000, read-back cmp OK. Maskrom entry for this write: see user's answer (software return vs pads).

### 2026-11-21: stage 1 built (not written): Alpine on `userdata` + IR remote test
- Network rule narrowed by the user: only the original Android must stay off the network.
- u-boot patch 0002 on the box; the button reaches maskrom (confirmed by the user).
- `tools/build-rootfs.sh` (arm64 Alpine 3.24.4 container) -> build/rootfs/h96root.img, 512 MiB
  ext4 LABEL=h96root, 57 packages (packages.txt), sha256 `e87f1647…`. Overlay in
  tools/rootfs/overlay: system-info screen on tty1 (inittab), getty on tty2, first-boot
  resize2fs to the full 11.4 GiB, reboot-loader in /usr/local/sbin. root has NO password
  (console only; must change before sshd/WiFi). Networking/sshd/chrony installed, not enabled.
- initramfs stage1-1: stick wait + report, 30 s IR remote test (raw evdev, decoder tested on
  synthetic events), stick inbox (h96-inbox.tar.gz -> rootfs, applied once), mounts
  LABEL=h96root only (Android's ext4 has no such label), switch_root into Alpine; fallback
  stays in initramfs. boot image sha256 `25d455dc…`.
- Planned writes: boot @0xc800 (linux-boot.img), userdata start @6803456 (h96root.img,
  1048576 sectors). Undo for userdata = its bytes from recon/live-full/full.img (11.4 GiB).
13:49 WRITTEN: boot @0xc800 = linux-boot.img 25d455dc… (read-back OK); userdata @6803456 = h96root.img e87f1647… 512 MiB (32 s write, read-back OK). GPT on device re-read == dump before writing.

### 14:06 WiFi test package ready (not yet run)
IR remote: stage1 report shows 0 events from the box remote (unknown usercode; revisit via the
remotectl code_print param). WiFi: Neardi stack ported to 6.1 (NOTES §24), packaged as
build/wifi/h96-inbox.tar.gz (a87f42a8…) for the stick inbox; scan-only test, log -> stick wifi-test.txt.

### 2026-11-21 ~14:20: USB debug link built (NOTES §25), not yet on the box
Kernel rebuilt with USB_CONFIGFS_NCM (Image.gz in build/kernel/kernel; WiFi modules rebuilt
against it). boot.dtb: USB-C DWC3 dr_mode=peripheral, high-speed. boot image
build/out/linux-boot.img sha256 `94eab59f…` (bootcmd sandbox OK). Stick inbox
build/inbox/h96-inbox.tar.gz `11cbf9b9…`: h96-gadget (ACM+NCM, usb0 10.96.96.1, udhcpd gives
the PC 10.96.96.2 with no router/DNS), sshd on 10.96.96.1 key-only (build/keys/h96max_ed25519),
getty on ttyGS0, WiFi modules+firmware + /usr/local/sbin/h96-wifi-test (not auto-run).
PC side: tools/h96 (ssh/scp wrapper, project-local known_hosts).
14:21 WRITTEN boot @0xc800 = linux-boot.img 94eab59f… (gadget kernel+DTB), read-back OK.

### 14:33 run 3 (charger power, gadget kernel #3): DWC3 in device mode (xHCI gone), PHY saw
USB_DCP_CHARGER (so no PC link possible on a charger); u-boot breadcrumbs work (h96.adc=1018
released, bootmode=0); MMC numbering moves between boots (eMMC mmcblk1, WiFi on mmc0).
PC USB-C power browns out a full Linux boot -> plan: 2 A on USB-C + RTL815x USB-Ethernet dongle
on USB-A. Inbox v2 (build/inbox): h96-lan (udhcpc on r8152 ifs, hostname h96max), sshd on all
ifs key-only, info screen shows the LAN IP + eMMC found by type. tools/h96 takes H96=<addr>.

### 14:52 photo 20261121_144939.jpg: Alpine up with inbox v2 (ssh line shown), root resized
to 11.2G, usb0 10.96.96.1 (gadget bound on charger), RTC-less clock. Dongle (RTL8152, USB 10/100
LAN, MAC 00:E0:4C:36:13:C2) got no address shown -- cause unknown (screen only listed addressed
ifs). Inbox v3 (a9c0683a…): hot-plug LAN watcher (h96-lan-watch), alpine-report.txt to the stick
~60 s after boot, info screen lists all links. Plan: box USB-A -> user's USB2 hub -> stick + dongle.

### 14:53 user prefers maskrom writes over the stick inbox. Rootfs rebuilt with the inbox
(v3) baked in: build/rootfs/h96root.img sha256 `9a63407e…` (36.6 MB content, e2fsck clean, modes
verified). To write: userdata @6803456. boot unchanged (94eab59f…).
15:00 WRITTEN userdata @6803456 = h96root.img 9a63407e… (read-back OK)

### 15:27 run 4: only the initramfs report (no alpine-report: local.d background job likely
reaped/too early); its dmesg shows the stick as usb 1-1 dev #2 on the root port, i.e. NO hub and
NO RTL8152 enumerated on the box although the user had the stick in the dongle's hub ports.
Also found: busybox 'ip' has no -br, so v3's links line was broken. Built for one maskrom session:
boot 48d9df05… (initramfs stage1-2: IR test opt-in via h96_remote=1, late USB tree with drivers),
userdata h96root.img fa5824e0… (+iproute2 usbutils ethtool, h96-stick-report from inittab
::once at 30/90/180 s -> alpine-report.txt + alpine-dmesg.txt).
15:31 WRITTEN boot 48d9df05… and userdata fa5824e0…, both read-back OK.

### 15:39 USB gadget removed (user: useless if the box browns out on PC power). Kernel being
rebuilt without NCM gadget and with usbnet + AX88179 etc. (user has an ASIX AX88179 as fallback);
factory DTB again; rootfs payload moved to tools/rootfs/payload (h96-lan any USB NIC, sshd
key-only, stick report, info screen, WiFi). Deployed images kept as *.deployed-* in build/.

### 15:44 run 5 (runattempts/5): hub+stick+RTL8152 enumerate on the box, r8152 -> eth1, udhcpc
running, but NO-CARRIER for 3 min (r8152 never logged 'carrier on'). Same dongle+cable on the PC:
carrier on in 2 s (combo hub flaky there too: -71 retries) => box-side, power suspected.
Fallback set built (not written): boot 657b74d0… (no gadget, factory DTB, usbnet+AX88179/178A/
AX8817x/CDC), userdata h96root.img 8af117b5… (no gadget, h96-lan any USB NIC, stick report +
ethtool/carrier). Next test asked: dongle alone (no stick) on the box, read TV links line.
15:46 WRITTEN boot 657b74d0… + userdata 8af117b5… (no-gadget fallback set), read-back OK

### 15:52 NETWORK ACCESS: box reachable at 192.168.11.145 (Realtek RTL8152 dongle as eth1,
DHCP via h96-lan, default route 192.168.11.1); ssh key-only works (H96=192.168.11.145 tools/h96).
Kernel #4 (gadget-free). User reports no HDMI picture; box side: HDMI-A-1 connected, VP0 ACTIVE
1920x1080p60 YUV8 148.5 MHz scanning the fbcon framebuffer, TV-out 576i on VP1 too, no
reset/undervoltage/oops in dmesg, uptime monotonic -> not a brown-out. Forced HDMI re-detect +
console reset at 142 s.

### 15:54 WIFI SCAN WORKS (NOTES §26): ported Neardi stack loads, ODM firmware boots
(SV6160LITE, fw 20250618), wlan0 scans 2.4+5 GHz, 7 BSS. Loaded by hand; not yet persistent.
Next: credentials -> wpa_supplicant + DHCP, then an OpenRC service to load at boot.

### 16:15 WIFI CONNECTED (NOTES §27): rx descriptor rewritten from the ODM driver's code;
WPA2 COMPLETED in 5 s, DHCP 192.168.11.149, ping/ssh over WiFi OK. Still loaded by hand.

### 16:34 WiFi PERMANENT: h96-wifi starts at boot (cold reboot verified, .149 in ~45 s).
Root cause of the earlier skip: mtimes from the box's wrong clock (no RTC) -> chronyd enabled,
savecache dropped, files touched. Box can now run on WiFi alone (dongle optional).
Sources updated: payload h96-wifi + runlevel link, build-rootfs (chronyd, no savecache),
driver skw6316 with the SWT6621S rx descriptor. Box rootfs was patched live (not reflashed).

### 17:32 Bluetooth: IR ruled out (TV stick, no receiver; remote is BT, user found the pairing combo;
TV has no CEC). Kernel rebuilt with BT/BR-EDR/LE/HIDP (an old trim list in build-kernel.sh had been
dropping BT/CFG80211/MAC80211/WIRELESS); boot image 56fdbede… (not yet written). skwbt ported from
Neardi (no-op set_fs on 5.18+, BD address in /var/lib/skwbt). Staged on the box: 3 modules,
sv6160.nvbin, h96-bt, bluez + dbus + bluetooth services.
17:42 WRITTEN boot @0xc800 = linux-boot.bt-56fdbede.img (BT kernel), read-back OK (was 657b74d0).

## 2026-11-21 evening → 11-22 night: hardware video, the Rust UI, a real IPTV player

### VPU and display (NOTES §33-§34)
Built Rockchip MPP for Alpine/musl in the arm64 container. MPP did not know the RK3518 ->
patch aliasing it to rk3528. `mpi_dec_test`: 1080p60 H.264 at **166 fps, ~6 % CPU** (software was
0.33×). librga from source (airockchip's is glibc binaries only; musl fixes). gstreamer-rockchip
`mppvideodec` + kmssink: the first plane I picked (Esmart0-win1) shares the console layer's
format and blinked **green** on the user's TV; plane 172 (Esmart1-win0) is independent. kmssink
waited for vblank twice (26 fps) -> `skip-vsync`. Live HLS over WiFi with HDMI audio: 60 fps, 8 % CPU.
Video in a rectangle over the console (render-rectangle) confirmed on the TV; the user chose:
**UI stays a CLI, no GPU**.

### h96ui rewritten in Rust (user request)
1:1 port of the Go UI (0.2.0), checked byte-for-byte against the Go `-snapshot` output on the
box; 541 KB instead of 6 MB. Then: USB hotplug detection (/sys/block poll; no hotplug daemon runs,
devtmpfs makes the nodes), exFAT via fuse-exfat, NTFS, duplicate-import detection (0.2.1).

### Playback integration and the bugs real channels found (NOTES §35-§37)
- The user got **stuck in mpv**: the remote's Back is KEY_BACK, which the console drops. Recorded
  all remote keys with evtest; h96ui now reads Back/Home (and later OK = KEY_SELECT 353, beyond the
  0-255 console keymap) from evdev, chosen by capability.
- 576p channel: `drmModeSetPlane: No space left on device` = kmssink uses the PAR-corrected
  width as the source rectangle -> patched kmssink, pinned to Alpine's gst-plugins-bad 1.28.3.
- 1080p "hangs": not the box (WiFi ~200 Mbit/s) but a server that gives ~12 Mbit/s per
  connection for an 8.8 Mbit/s stream. -> **h96player** (C, GStreamer API): prebuffer with
  hysteresis, playlist HTTP headers on every HLS request (source-setup; hlsdemux2 copies UA and
  Referer), parseable status. Slowest channel: 3 s of video in 35 s before, 52 s at 50 fps after.
- h96ui could not find h96player: busybox init's PATH lacks /usr/local/bin (0.3.2).
- On-demand videos as their own list per subscription; h96player control protocol (pause, seek,
  rect, volume); the box OSD. Seeking **deadlocked** playbin; found with gdb and Alpine's -dbg
  packages: a lock cycle between playbin's input-selector reset after hlsdemux2 re-created its
  pads, and my status query. -> playbin3, async state changes, no queries during seeks; 8-seek
  test passes on the TV. Volume/mute keys with an on-screen level (0.4.1).

### Xtream (NOTES §38) and the fixup kit (NOTES §39)
Native Xtream Codes support (0.5.0): accounts from a USB file (provider link or
server/username/password lines), mode 0600, Live/Movies/Series with categories and seasons,
account status, VLC-style User-Agent. Tested against a mock panel that refuses other
User-Agents; progressive-MP4 films seek fine. Then the fixup kit: `h96-doctor`, `h96-fix`,
`push-to-box.sh`, `docs/log/troubleshooting.md`; the rootfs build now includes the whole video
stack and the Rust UI (`h96root.img` `3749043b…`, built, not written).
No flash writes in this phase; everything was installed live over ssh.

### 2026-11-22 01:35: "streaming is gone" + "can't delete subscriptions" (NOTES §40)
After a reboot no live channel played. Not the player, decoder (2M had switched to MPEG-2,
which the VPU decodes at 505 fps), clock or playlist: the box downloaded from the distant server
at 0.30 Mbit/s against the PC's 11.5, with no loss. A/B with the SeekWave driver's debugfs
`rx_reorder` switch: 6.59 Mbit/s off vs 0.30 on. Turned off at boot in h96-wifi; all three
failing channels play again. Deleting subscriptions: Remove existed but only at the bottom of
the subscription's menu -> h96ui 0.5.1: hold OK (timed from evdev press/release) or Menu on a
subscription opens Open/Remove. First real push-to-box run (found and fixed its own pipefail
bug); doctor: 0 problems. The user's father's Xtream subscription was not touched.

### 2026-11-22 evening: EBI 0.6 (NOTES §41-§42) and the access lock-down
- **Name:** the frontend is **EBI (Extremely Basic IPTV)**; on-screen text only, the binary and
  paths stay `h96ui`. New standing rule: provider files on the box are off-limits except
  `ma.m3u` (rules-and-lessons §10).
- **WiFi without ssh** (0.6.0): first designed as a network scan + on-screen keyboard, cut down at
  the user's request to "Add WiFi from USB" (a `.txt` with `ssid=`/`password=`). A test caught a
  priority bug before it shipped (a re-added network got a lower priority than the others).
- **Faster lists:** held Up/Down did nothing because the remote's input has no EV_REP -> evdev-timed
  autoscroll. Vol keys as a letter jump; then, from the user's feedback, shared provider leads
  are stripped (0.6.1: short codes; 0.6.3: encased words of any length such as `[Italy]`,
  `[Serie]`), lists are sorted A-Z by the same names, and Vol-/Vol+ were flipped (0.6.2).
- **"Could not get/set settings from/on resource":** a generic GStreamer message whose reason
  h96player threw away. Step 1 was verbosity (element + debug reason on the error line, a
  per-run GStreamer log); the first real hit then said `alsasink: Unable to set hw params` and
  dmesg `Not able to map channels to speakers`: 5.1 audio on a stereo TV. Reproduced with a
  6-channel silent test source, fixed with a stereo downmix in front of alsasink (0.6.4,
  h96player `-C`).
- **Access:** ssh was already key-only, but root's password was empty (Alt+F2 on a USB keyboard =
  root shell). Now `*` (not `!`, which this OpenSSH treats as locked and refuses keys too),
  `AuthenticationMethods publickey`, `MaxAuthTries 3`; applied behind a 120 s auto-revert that a
  fresh key login cancelled; `h96-doctor` checks both; `build-rootfs.sh` sets `*` too.
- Five pushes with `push-to-box.sh` (0.6.0 → 0.6.4), each with consent; doctor 0 problems each
  time. No flash writes; no provider file read.
