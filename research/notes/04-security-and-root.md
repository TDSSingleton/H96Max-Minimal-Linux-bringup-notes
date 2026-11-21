# Root routes and the security posture of the stock firmware

Part of the H96Max research notes; index in [README.md](README.md). Sections keep their original § numbers, so a `§N` reference anywhere resolves through the index.

## 5. Root-by-ramdisk — the premise needs updating

The "unpack boot.img, put `su` in the ramdisk, repack" trick is the Android ≤9 flow. On an
RK3518 box shipping **Android 14 / kernel 6.1**, the ramdisk is (or is close to) first-stage
init only, and root normally comes from a Magisk patch of boot.img/vendor_boot. What is
confirmed for RK3518 Android: AOSP `android-14.0.0_r75`, kernel 6.1, classic boot.img +
8-partition super, arm64 (one board variant is 32-bit userspace). What is **not** found in
any source: the stock boot.img header version, ramdisk compression (gzip/lz4/lz4o), whether
AVB/dm-verity is *enforced* on stock H96 firmware, and any published Magisk-on-RK3518 recipe.

Unverified-but-specific alternative root doors, from the RK3518 firmware RE report (single
source, see §6): stock `init.rk30board.rc` registers property-triggered root services —
`persist.abc_switch=1` + `persist.abc_path` → `/system/bin/sh <path>` as root in
`u:r:su:s0`, and `sys.flash.img=1` → root `dd` of logo/boot/recovery from `/sdcard`; plus
world-writable TEE nodes (`chmod 0666 /dev/tee0 /dev/opteearmtz00`, `0777 /dev/rkvtunnel`).
If those are real on this board, root is trivial — and so is the implant that used them.

## 6. Security posture — separating fact from folklore

**Documented by named analysts** (agent could not re-verify URLs this session; treat as
pointers): ESET "969" (Nov 2024) — Android STBs/TVs in a residential-proxy/DDoS botnet,
infected mainly via pirate-IPTV APKs. Check Point "Rise of AiChuang" (Sep 2022) — ~1.2M
Android devices, mostly TV boxes, via backdoored "UpdateService"-style APKs in pirate IPTV
bundles. Fortinet "RedFire" (May 2023) + ESET/Unit42 "ADB.Miner" (2018) — Mirai-style
entry via **open ADB over TCP 5555** on boxes. Factory-preinstall precedents on cheap
Android generally: Kaspersky XHelper/HiddenAds, Check Point CopyCat, iVerify/Krebs
"EasterEgg" (mostly phones/watches, not TV boxes).
**Folklore, not supported:** "Emotet on TV boxes" (Windows malware), XorDDoS-on-boxes
(x86/ARM Linux via telnet/SSH brute force), a "Linux.TVRat" family, and Mozi as a TV-box
botnet (Mozi is routers/DVRs/IP cams/x86/ARM Linux; Android boxes are a negligible slice).
⇒ "TV boxes in botnets is real" = substantiated. "This box shipped as a botnet node" =
plausible but unproven; the realistic mechanism is the **seller reflashing stock firmware
with a pirated-IPTV bundle**, which is close enough to treat as compromised.

**One directly relevant, single-source, unverified writeup:** `freddy-newandyke/rk3518-malware-quarantine`
(1 star, created 2026-11-05, ships live malware; repo self-declares AI-assisted; no CVE, no
CERT, no press, zero corroboration). Its claims about `rk3518_box_32` Android 14 firmware,
ODM = Hugsun (`persist.box.customer=hugsun`): `/system/bin/gsdnode_android` v2.2.0 respawned
every 10 s by a root `init.sh`, registering MAC/serial as a node to `tws.gsda.com.cn:7704`
(`115.29.212.137`), 5-min heartbeat, AES-128-ECB key `JS9faiTH1B24aTA1`, remote root shell,
silent `pm install -r`, `reboot loader`; nine PCDN/bandwidth-selling payloads (`niulink`,
`wxedge`, `dianxin`, `ksc-kepler`, `ant`, `baidu`, `ross_atv`, Airship) = the box's uplink
sold as a residential CDN node; a second loader hidden in the stock Rockchip OTA app
(`RKUpdateService$HgsHandler` polling `ota.rupa.tv:2300/BlueberryTask/data` every 30 min →
`/data/blueberry/blueberry.sh` as UID 1000); FOTA clients `com.chihi.fota` →
`api.hugsun.com.cn` / `api.fota.rupa.tv`, plus Abupdate APKs; `/system/bin/preinstall.sh`
disabling Play Protect at boot and sideloading `/system/preinstall/*.apk`.
Worth checking locally, cheap and decisive: `getprop` for `persist.box.customer`,
`ro.product.name`, `ro.product.ota.host`; existence of `/data/gsdnode/`, `/data/blueberry/`,
`/data/.lotso/`, `/system/bin/gsdnode_android`; SDIO vendor `c8a1:` (AIC8800).

**Persistence / minimum reflash set.** A factory reset wipes `/data` (+`/cache`) only.
Everything survives: `super` (system/vendor), `boot`, `dtbo`/`vendor_boot`, `uboot`, `trust`,
and the idbloader at LBA 64. An implant in uboot/trust/idbloader sits below the OS and no
Android-side action removes it. Minimum for confidence: reflash idbloader/MiniLoaderAll +
uboot + trust + boot + dtbo/vendor_boot + super from a package you control, and wipe
userdata/cache. Not rewritten by any of that: SoC eFuse/OTP, and the protected vendor
storage block holding MAC/Widevine IDs (not a plausible code-exec site). MAC-based
re-infection: no documented precedent. **OTA re-push is a real documented risk** — unauth
HTTP OTA can reinstall APKs, so the post-flash enemy is the update client, not the MAC.

**Flashing is fully offline.** Maskrom is unmodifiable BootROM presenting USB OTG: loader
upload over the cable, zero network needed. In maskrom the device enumerates only as a
Rockchip vendor-specific bulk USB device (`0x2207:xxxx`) via libusb — not mass storage, not
HID, not MTP — so no auto-mount surface and no BadUSB keystroke path during flashing.
Residual: once the box boots its own OS it can present MTP/ADB. Disable the desktop's
automount before cabling a suspect box, and keep it powered off / in maskrom when plugged
into the workstation. No maskrom-protocol CVE found (reported as not found, not as absent).

**First-boot window is the actual risk**, not flash time. Close it with L2 control, not
racing `ifconfig down`: no ethernet cable, WiFi never joined, or an isolated switch port /
VLAN with no gateway and DHCP off. After: permanent quarantine VLAN, default-deny egress,
never enable wireless ADB (the documented RedFire door), sinkhole the OTA domains. A 2.4 GHz
gamepad dongle lives on the TV side, so "allow nothing outbound" is compatible with gameplay.
