# Reading the device out: open questions, the read ceiling, verified facts

Part of the H96Max research notes; index in [README.md](README.md). Sections keep their original § numbers, so a `§N` reference anywhere resolves through the index.

## 9. Open questions to resolve before any flashing

1. Photo of the PCB both sides + **SoC silkscreen** + the three board markings listed in §2,
   eMMC part top marking, RAM stick marking, WiFi module marking (AP62xx vs AIC8800D40 is a
   chip-generation discriminator), and how many USB sockets are actually populated.
2. Does it have ethernet? (Per-board, not per-SoC. RMII `gmac0` = 100 Mbit ceiling; some
   revisions have an RGMII PHY on `gmac1` = gigabit; some have no RJ45 at all.)
3. Is there a reset pin behind the AV jack, and two pads under the heatsink?
4. What does the BootROM serial banner say — "RK3518 SoC" or "RK3528"?
5. Which end goal does the user actually want, given §1.
6. Is there a Windows machine or USB-bootable path? RKDevTool is Windows and is the only
   tool *demonstrated* on RK3518-family boxes; Linux alternatives need a newer
   rkdeveloptool plus a hand-built correct DDR loader, which is the risky part.

## 12. First read-out of the real device — and a 32 MiB read ceiling that invalidated most of it

The box enumerated on the chassis reset button as **`2207:350c`, LOADER mode**
(`bNumConfigurations 1`, iSerial `SN<redacted>`, interface `bInterfaceClass 255 / sub 6 /
proto 5` = RockUSB bulk-only: no MSC, no HID, nothing to automount). Loader mode, not
maskrom, so **no loader upload was ever needed** — the board runs its own factory loader and
its RAM trains correctly by definition. All five built loaders went unused. That is the
route the user's "recovery as a bootstrap" instinct effectively delivered.

`rfi`: **Samsung eMMC, 15028 MB, 30,777,344 sectors**, block 512 KiB, page 2 KiB → a 16 GB
part, unlike both documented boards (8 GB). `rci`: `38 32 35 33` → LE uint32 `0x33353238` =
**"3528"**, i.e. the device self-reports the *family* id, and the same four bytes sit in the
known-good X88PRO loader header — **confirming `NAME=RK3528` is correct and that `rci` cannot
arbitrate 3518-vs-3528.** `rcb`: Direct LBA / First 4m Access / Read Com Log / Read Secure
Mode / New IDB all "enabled".

Factory DDR banner, read out of sector 64 (the critical backup):
```
DDR 56f70fd2ad huan.he 25/07/21-09:51.33, fwver: v1.11 4bit PCB
U-Boot SPL 2017.09-g3aa32998d6d-250107 #lxh (Jan 08 2025 - 16:54:13), fwver: v1.06
```
Same DDR build hash `56f70fd2ad`, same `v1.11 4bit PCB` as juliovendramini's RK3518 board
(theirs `25/06/11`, ours `25/07/21`). **So this is the LPDDR3 "4bit PCB" family, NOT the X88PRO
DDR4 one** — vendramini's Armbian bring-up is the applicable template, and the X88PRO's
`factory_idbloader` is only a reference, not a spare.

GPT (from `ppt`, all offsets verified against real content): `security@0x2000 uboot@0x4000
trust@0x6000 misc@0x8000 dtbo@0xA000 vbmeta@0xC000 boot@0xC800 recovery@0x2C800
backup@0x5C800 cache@0x11C800 metadata@0x1DC800 baseparameter@0x1FC800 super@0x1FD000
userdata@0x67D000`. `uboot@0x4000` = LBA 16384, i.e. the documented `u-boot.itb` slot *is*
the GPT `uboot` partition, and the idbloader lives in the gap below `security`. `super`
starting at `0x1FD000` matches kxn's board byte-for-byte → same BSP lineage.

### The ceiling, and the mistake that hid it

A 15 GB partition-wise dump completed, head double-read gate passed — and then
`unpack_bootimg` on `boot.img` showed a header declaring a **1,344,505-byte ramdisk** at an
offset whose bytes were `0xCC`. A declared-present ramdisk cannot be filler, so the read was
a lie, not the flash. Mapping it: live `rl` matched our dump byte-for-byte at every offset,
real at LBA 60000 (29.3 MiB), `0xCC` from **LBA 65536 = exactly 32 MiB** onward; the last LBA
of the device returns `0xCC` where a **backup GPT must be**. So the loader silently serves
filler past 32 MiB and reports no error.

**Design error, worth recording: my integrity gate read sectors 0–8192 twice and compared.
That only proves the region that already worked.** The structural test that cannot be fooled
by a read window is "does the backup GPT exist at the last LBA" — now added to
`tools/dump-read-only.sh` as gate 3a, with an outward probe that reports the ceiling.

Reliability of what we captured:
- **Good (< 32 MiB):** `pi-head.bin` incl. the sector-64 DDR blob (the one-of-a-kind
  anti-brick artifact — backed up and banner-verified ✓), `security` (`SSKR`), **`uboot.img`
  (4 MiB, `d00dfeed` FIT, 99.9 % real ✓)**, `trust`, `misc`, `dtbo`, `vbmeta` (`AVB0`), and
  `boot.img`'s header + first ~7 MiB.
- **Garbage (≥ 32 MiB):** `boot`'s ramdisk, its `second`/resource (6.3 MB) and its **DTB
  (98,687 B, at ~36 MiB)** — i.e. the exact artifact a Linux port needs most; plus all of
  `recovery`, `backup`, `cache`, `metadata`, `baseparameter`, `super`, `userdata`.
- Claims I made and withdrew: "the eMMC genuinely is unwritten beyond the boot area" and "we
  have userdata" — both wrong, both were the filler.

`boot`'s kernel is an **ARM32 zImage** — magic `0x016f2818` at +0x24, `e1a00000` NOPs at +0,
verified inside the reliable window. So the stock firmware is a **32-bit** userspace/kernel
build (the `rk3518_box_32` class). Not a blocker for Linux — the SoC is Cortex-A53 and the
community builds arm64 U-Boot/Armbian on this very board — but it ends any arm64-Android-GSI
idea and means "root the Android" was never going to be an arm64 target.

Boot-image cmdline recovered (real, in-window) is unusually informative:
`console=ttyFIQ0 firmware_class.path=/vendor/etc/firmware init=/init rootwait ro
loop.max_part=7 printk.devkmsg=on kvm-arm.mode=none androidboot.console=ttyFIQ0
androidboot.wificountrycode=CN androidboot.hardware=rk30board
androidboot.boot_devices=ffbf0000.mmc` **`androidboot.selinux=permissive`** — stock firmware
ships SELinux permissive, `rk30board` HAL naming (matching the `init.rk30board.rc` root
triggers the RE report described), and `wificountrycode=CN`. Header **version 2**, os
14.0.0 / patch 2025-01, page size 2048.

### Sequencing consequence

Every community procedure makes a **verified full backup step 0**, and ours covers 32 MiB of
a 14.7 GiB device. **Do not write mainline U-Boot to `uboot@0x4000` until reads span the whole
device** — that write is the reversible one, but "reversible" assumes a faithful stock
`uboot.img` *and* an intact sector 64, and we should not be guessing about the rest of the
layout while `super`/`recovery` are unverified. Next move is to beat the ceiling: enter
maskrom (pads under the heatsink, or observe what the button alone yields on a clean
power-cycle) and `db` a loader built from **usbplug v1.04 + SPL v1.07**, which is newer than
the factory SPL v1.06 path we are currently reading through, then re-probe LBA 65536+ and the
device tail. `db` is RAM-only and fails safe.

### Retarget: GammaOS → minimal Linux (user's decision, 2026-11-19)

GammaOS is off the table (it is handheld-only) and Linux is the better fit anyway: 1–2 GB
RAM + Mali-450 + arm32 Android is badly undersized for Android 14, and a rootfs we build
removes the OTA/PCDN attack surface *structurally* instead of needing a quarantine VLAN.
Boot-chain procedure researched in detail — see §13 (sector 64 never touched; replace only
`uboot@0x4000` with `generic-rk3528_defconfig` mainline `u-boot.itb`, BL31 ≥ v1.20 (v1.21
current), `SYS_MMC_MAX_BLK_COUNT=2048`, distro_bootcmd order `mmc1 mmc0 nvme scsi usb pxe
dhcp spi` so SD precedes eMMC *once our U-Boot owns 16384*; never `armbian-install`; never
round-trip the decompiled Android DTB).

## 14. Verified device facts, the read ceiling solved, and where the source lives

All of this was measured directly this session unless marked **[agent-reported]**.

### The full capture exists and is trustworthy

`recon/live-full/full.img`, sha256 `3fc120f99b56d8888686a39e95d6ea8b5d0025c6d67adffac3f13129bcf3c471`,
15,758,000,128 B = 30,777,344 sectors = exactly the `rfi` capacity.

- primary GPT header CRC **and** 128-entry array CRC recompute correctly; backup GPT present at
  the last LBA with both CRCs correct → the check that failed all evening now passes.
- first 4 MiB byte-identical to `pi-head.bin`, taken in a *different* loader session.
- `boot` first 1 MiB identical to the standalone 64 MiB read.
- `super` unpacked with `lpunpack` into 8 ext4 sub-partitions (system 1.22 GB, product 484 MB,
  vendor 136 MB, system_ext 49 MB, vendor_dlkm 18.6 MB, odm 815 KB, system_dlkm/odm_dlkm 262 KB).

**Root cause of the 32 MiB ceiling: the factory loader in sector 64 caps its own reads.** Not a
tool bug, not an OEM lock. Proven by elimination — `rkdeveloptool` (both `RWMETHOD_IMAGE` and
`RWMETHOD_LBA`), RKDevTool on Windows (stopped at 25.25 MB), and hiifong's Rust app (full-size
file padded with 0xCC) all failed identically **while the factory loader was running**; the wall
vanished the moment a *download-mode* loader (usbplug v1.04 + SPL v1.07) ran.

Getting there needed **hiifong/rkdevtool v0.16.0** (Tauri2 + Rust, native Linux AppImage, RockUSB
implemented in Rust). Its log showed why the CLI stalled: the container carries **two `0x471`
entries — `UsbHead` first, then the DDR blob** — and the Rust code walks the entry table where
the 2019-era tool did not. Also confirmed: **`rkdeveloptool rd 3` (`RST_RESETMASKROM_SUBCODE`)
reaches maskrom in software**, no pads required. And during the download-loader session `ld`
still prints "Maskrom", because the temporary loader inherits BootROM's `bcdDevice 0100` and
`RKScan.cpp` classifies on `bNumConfigurations`/that bit — a false signal worth remembering.

### Retractions (each: a check that validated less than it claimed)

- "the eMMC is genuinely unwritten past the boot area" — **wrong**; that was the read ceiling.
- "`gsda` present in vendor/product/system" — **wrong.** A 4-byte substring matched inside
  compressed data. Re-run with context (`[!-~]{0,42}gsda[!-~]{0,42}`) → **0 hits in all three
  images.** Same for `abupdate`. **So the implant claim remains unverified in both directions**,
  and RSM ("detect secure mode") was never run — the OEM-lock question stays open.
- "there is no `rk3518.dtsi`" (§2) — **wrong**; see the source section below.
- "the double-read gate validates the capture" — it only ever tested the region that already
  worked. The structural test that cannot be fooled is the **backup GPT CRC at the last LBA**.

### What the firmware says about itself

- Kernel **6.1.118, ARMv7, `p2v8`, SMP preempt mod_unload modversions** (module vermagic).
  Android 14.0.0 / patch 2025-01 / boot header **v2**; cmdline has `androidboot.selinux=permissive`,
  `androidboot.hardware=rk30board`, `androidboot.wificountrycode=CN`,
  `androidboot.boot_devices=ffbf0000.mmc`.
- **32-bit-only firmware, confirmed twice**: `ro.vendor.product.cpu.abilist64=` is empty *and* the modules
  are ARMv7. **[§18: this is the OS build, not the silicon — the A53s, BL31 and u-boot are AArch64.]** This closes the question that gated the GammaOS-GSI idea — and GammaOS is gone anyway.
- `ro.board.platform=rk3528` (ODM naming), `ro.hardware.egl=mali`.
- Board DT: `model = "Rockchip RK3518 EVB1 DDR4 V10 Board"`,
  `compatible = "rockchip,rk3518-evb1-ddr4-v10", "rockchip,rk3518"`. The "DDR4" in the name is
  inherited EVB naming; the actual DDR init is `v1.11 4bit PCB` with `lp3_freq=666`.
- DT peripherals: `rockchip,rk3528-vop` + dw-hdmi (VP0→hdmi, VP1→TVE), **`rockchip,remotectl-pwm`
  IR enabled with an `ir_key1` key table**, `gmac0 ethernet@ffbd0000` `phy-mode="rmii"`
  **status okay** with the integrated `ethernet-phy-id0044.1400`, `sdio1` 4-bit @200 MHz carrying
  `seekwcn_boot = "seekwave,sv6160lite"` (NV `SEEKWAVE_NV_SWT6652.bin`), `bluetooth-platdata` on
  uart2 (host_wake gpio1.7, chip_en gpio1.10, chip_wake gpio1.12), and a **disabled**
  `wlan-platdata` for `ap6275s` — WiFi is neither Broadcom nor the documented AIC8800.
- **ODM SDK leaked by the .ko build paths:**
  `/home/s401/work/sdb1/rk/3518_0428_dongle/Rockchip_Android14.0_MS_SDK_RELEASE/external/wifi_driver/ea6621q/drivers/skwifi/skw_core.c`
  → the WiFi module is a **SeekWave EA6621Q** (SV6160 family).

### Ripped from the device (`vendor/seekwave/`, 105 files, 2.7 MB)

- Prebuilt modules out of `vendor_dlkm.img`: `skw.ko` 421 KB, `swt6621s_wifi.ko` 604 KB,
  `skw_sdio_lite.ko` 182 KB, `skw_sdio.ko` 95 KB, `skw_bootcoms.ko` 47 KB — all 6.1.118 ARMv7.
- ~57 firmware/NV files incl. `EA6621Q_SEEKWAVE_R00005.bin` — but that is **428 B, i.e. NV only**;
  the module's RAM image is unaccounted for (likely embedded via the `.rel.dat` references, or a
  file my name filter dropped). **Unresolved.**
- `vendor.img` manifest proves the ODM ships drivers for ea6621q (seekwaveplatform/sdio),
  swt6621s (seekwaveplatform_lite + swt6621s_wifi), aic8800 (×3), bcmdhd, infineon cyw88459,
  realtek 8822cs / 8852bs.

### Where the source actually is

- **`rockchip-linux/kernel@develop-6.1`**: `arch/arm/boot/dts/rk3518-evb1-ddr4-v10.dts` →
  **HTTP 200**, plus `arch/arm64/boot/dts/rockchip/{rk3518.dtsi, rk3518-evb.dtsi,
  rk3518-evb1-ddr4-v10.dts, .dtsi}`. The 6.1 BSP keeps 32-bit DTs in the **flat**
  `arch/arm/boot/dts/` layout — `arch/arm/boot/dts/rockchip/` is 404 — which is precisely why the
  §2 negative was wrong: arm64-only search, then a subdirectory that doesn't exist. **Absent from
  develop-5.10.** arm32 config present: `arch/arm/configs/rockchip_linux_defconfig` (no
  rk3518-specific defconfig; multiplatform).
- develop-6.1 is **6.1.172**, not 6.1.118 → `modversions` CRCs won't match, so the ripped `.ko`
  are **reference, not loadable**; rebuild from source. (Unless we build exactly 6.1.118.)
- **`github.com/daeiron/ea6x21qx_driver`** — "SeekWave EA6x21qx wifi/bt driver"; contains
  `drivers/seekwaveplatform`, `drivers/skwifi`, `firmware/{EA6521QF,EA6621QT,SWT6621_*_SDIO}.bin`
  — same tree shape as the ODM's `external/wifi_driver/ea6621q`. **Best driver-source candidate.**
- **`github.com/TinkerBoard-Android/rockchip-android-hardware-seekwave`** @`android14-rockchip`:
  `wlan/{wifi_hal, configs, wpa_supplicant_8_lib}` + `skwbt/{src, include, vendor}`.
- **`github.com/ilyakurdyukov/rk3528-tvbox`** (archived, 67★) `devicetree/orig/`:
  `rk3528-evb1-ddr4-v10.dtsi`, `rk3528-android.dtsi`, `rk3528-demo*.dtsi`,
  **`rk-stb-ir-keymap.dtsi`**, `rk3528-h96max-m1.dts` — an Armbian TV-box base that already
  carries an IR keymap.
- Neardi/FriendlyARM **[agent-reported]**: `friendlyarm/sd-fuse_rk3528` (branch `kernel-6.1.y`)
  → `friendlyarm/kernel-rockchip@nanopi6-v6.1.y`, `uboot-rockchip@nanopi5-v2017.09`, sources under
  `download.friendlyelec.com/rk3528` (Baidu Pan `1TbOAFghLurCvmkUT-Hgp2w` code `g2vf`; GDrive
  folder `1TbhufnT6NITr35uEvn_UsvnZPx6PbMEL`). Radxa: `radxa-build/radxa-rk3528`,
  `radxa-repo/rk3528a-bullseye`. `forum.neardi.com` is Flarum: HTML is a JS shell, `/api` answers
  but the guest search filter is ignored; user `Linx` = id 8; **no RK3518 threads found.**
- `research/neardi-3588-sdk-linux/` = `github.com/neardi/neardi-3588-sdk-linux`, kernel
  **5.10.110**, RK3588-only (0 rk3518/rk3528 files) but it *does* ship
  `drivers/misc/seekwaveplatform_v20/{sdio,usb,skwutil}` (SDIO table `SDIO_DEVICE(0x1FFE, 0x6316)`)
  and `drivers/bluetooth/skwbt/sv6160.nvbin` → ripped to `vendor/seekwave/`.

### Open items

1. Where the EA6621Q **RAM firmware** lives.
2. RSM / secure-boot state — never measured.
3. Whether any telemetry/OTA client is actually present (context-grep says no; `product`/`system`
   APKs and `/data` not yet examined properly).
4. RAM size — not in the DTB (U-Boot injects it); read `/proc/meminfo` at first Linux boot.
5. Does the PCB have an unpopulated RJ45 + magnetics? `gmac0` is enabled in firmware.
6. Build target: arm32 `develop-6.1` + `rockchip_linux_defconfig` +
   `rk3518-evb1-ddr4-v10.dts` (vendor-identical; at 6.1.118 the modules would even load as-is)
   vs arm64 + community DT bases (better maintained, everything rebuilt).
