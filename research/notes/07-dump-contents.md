# What the full dump contains beyond the boot chain

Part of the H96Max research notes; index in [README.md](README.md). Sections keep their original § numbers, so a `§N` reference anywhere resolves through the index.

## 19. What the dump says beyond the boot chain (2026-11-21)

Everything below was read offline from `recon/live-full/full.img`: partitions were carved
with `dd`, filesystems read with `debugfs` (userdata via `full.img?offset=`), `super`
unpacked with `lpunpack`, and APKs parsed statically. Nothing was executed.

- **RAM = 2 GiB.** `cache/recovery/last_kmsg` (a factory recovery boot) shows
  `Memory: 1988420K/2086912K available … 1292380K highmem`. That closes the RAM open
  item. The 1.26 GiB of highmem is a 32-bit-kernel cost that arm64 would avoid.
- **The handoff to the kernel is AArch32:** `CPU: ARMv7 Processor [410fd034]`,
  `All CPU(s) started in SVC mode`, PSCI v1.1 from BL31.
- **u-boot rewrites the kernel cmdline.** It prepends `storagemedia=emmc androidboot.mode=…
  androidboot.serialno=…` and appends the DTB's `/chosen/bootargs`, which is
  `earlycon=uart8250,mmio32,0xff9f0000 console=ttyFIQ0 driver_async_probe=dwmmc_rockchip,rockchip-drm
  drm_kms_helper.fbdev_emulation=0`, followed by the firmware-version string.
  **`drm_kms_helper.fbdev_emulation=0` arrives after our boot.img cmdline**, so it would
  switch off the HDMI fbcon console that §17 relies on. Fix: edit `/chosen/bootargs` in the
  dtb we ship, or build with `CONFIG_DRM_FBDEV_EMULATION=y` and confirm at runtime that the
  last value wins.
- **The rest of last_kmsg matches the DTB:** VOP2 driver bound, `dw-hdmi v2.11a with HDCP
  (inno_dw_hdmi_phy2)`, CEC input device, eMMC `R1J96N` 14.7 GiB at HS200 on `ffbf0000`, and
  an SDIO card on mmc0 at SDR104, i.e. the WiFi chip is there and enumerates.
- **The HDMI mode is stored in `baseparameter`:** `DISP_0` = 148.5 MHz,
  1920/2008/2052/2200 × 1080/1084/1089/1125 @60, i.e. 1080p60. The Android UI renders at
  `persist.vendor.framebuffer.main=1280x720@60` and is scaled up. (§17's "1080p" was right,
  but inferring it from logo.bmp's size was not evidence.)
- **Android profile:** `ro.config.low_ram=true`, zram on, `dalvik.vm.heapsize=512m`. The
  32-bit userspace is part of Android's low-RAM profile.
- **Build identity:** `ro.product.*.model=H96_Max_3518_TS`, built 2025-07-21, `ro.build.type=
  userdebug`, `ro.debuggable=1`, while the fingerprint is **spoofed as a Pixel 5**
  (`google/redfin/redfin:13/TQ2A.230305.008.F1`) — an uncertified box presenting as certified.
- **The stock firmware ships with root and network ADB:**
  - `/system/xbin/su` is setuid root (mode 06755, `u:object_r:su_exec:s0`, 32-bit Bionic) and
    has no "not allowed" uid check in its strings. With `androidboot.selinux=permissive`,
    that means any app can become root.
  - `persist.sys.usb.config=adb`, and `vendor/build.prop` sets **`service.adb.tcp.port=5555`**
    with no `ro.adb.secure` anywhere, so adbd listens on the LAN, apparently without key
    authentication. Not confirmed live.
- **Telemetry/OTA question (open item 3) answered: yes.** The 2026-11-19 "0 hits for
  `abupdate`" was a grep over APKs, which are zip-compressed. Notable system apps:

  | app | identity | what it can do |
  |---|---|---|
  | `system/app/Abupdate` | **ADUPS FOTA**, `com.abupdate.fota_demo_iot`, `android.uid.system` | `INSTALL_PACKAGES`, `RECOVERY`, location (fine/coarse, cell), boot receiver, MQTT push, `installBySilent`, endpoints `iotapi.abupdate.com`, `iotapi.adups.com` (+ `-test` hosts), paths `/product/%s/%s/ota/checkVersion`, `reportBehavior`, `reportAppResult`, `productSettingInfo` |
  | `system/priv-app/HcyDeviceTest1` | Rockchip factory device test (`com.rockchip.devicetest`), system uid | `INJECT_EVENTS`, boot receiver, reads `mac_wifi`, calls `http://active.hcybox.net:31080/` (activation) |
  | `system/app/RemoteHelper` | system uid | `INJECT_EVENTS`, `WRITE_SECURE_SETTINGS`, `INTERNET`, no hosts in dex |
  | `system/priv-app/MediaCenter` | system uid | `INSTALL_PACKAGES`, `DELETE_PACKAGES`, `ACCESS_SUPERUSER` |
  | `system/priv-app/test`, `system/app/installApp1` | system uid | USB/media-mount triggered, `WRITE_SECURE_SETTINGS` (test) |
  | `product/priv-app/Tubesky` | **Google Play Store under another name** (`libphonesky_data_loader.so`) | normal Play behaviour |

  Also present: `PrebuiltGmsCorePano` (GMS for TV), `DLNA`, `LeanbackCustomizerGYS`,
  `SkyRocketFly`, `firstbt`, `SimpleAllApp`, `tvlauncehrapps`. **None of this matters for the
  Linux plan — it all lives in `super`, which Linux never runs — but it is the concrete
  reason never to let stock Android onto a network again.**
- **`userdata` is used and file-based-encrypted.** ext4 with FBE (`<encrypted>` names in
  `/data/app`, `/data/system`, `/data/misc`). `/data/unencrypted/key` is wrapped by
  Keymaster in the TEE, so the contents are not recoverable offline. The timestamps show
  use from 2025-07-21 (first boot) to app installs on 2025-12-11 and an ANR on 2025-12-28,
  so the box had a previous user.
- **Empty partitions:** `trust` and `backup` are all zero (BL31/BL32 live inside the `uboot`
  FIT). `metadata` holds vold/ota/gsi state; `misc` holds an `AB0` record and an `RKNS`
  block, but no pending boot command.
- **Disk:** `recon/live-full/chunks/` (15 GB) duplicates `full.img`. It is a cleanup
  candidate once `full.img`'s sha256 has been re-checked.
- **Likely answer to "where is the EA6621Q RAM firmware":** `vendor/etc/firmware/` ships
  `SWT6621S_IRAM_SDIO.bin` (358,776 B) and `SWT6621S_DRAM_SDIO.bin` (192,816 B), plus
  `SWT6621S_NV_SDIO*.bin`, and `vendor_dlkm` references the same names. The earlier rip into
  `vendor/seekwave/` missed them (no IRAM/DRAM files there). Which file set the EA6621Q
  module actually loads is still to be confirmed from the driver source.
