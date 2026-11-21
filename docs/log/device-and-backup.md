# What the device is, the read ceiling, and the backup

Part of the session log (the narrative: what happened and why); index in [README.md](README.md). Technical detail is in `research/notes/`.

## 2. What the device actually is (all verified from the device itself)

| | |
|---|---|
| SoC | RK3518 = RK3528-family SKU. SoC id `0x35181001`; `rci` reports **`3528`** (family id) |
| Board DT | `model = "Rockchip RK3518 EVB1 DDR4 V10 Board"`, `compatible = "rockchip,rk3518-evb1-ddr4-v10", "rockchip,rk3518"` |
| CPU / arch | 4× Cortex-A53 (AArch64-capable; BL31/u-boot are AArch64), but the **firmware is 32-bit**: stock kernel is an ARM zImage, module vermagic `ARMv7 p2v8`, `ro.vendor.product.cpu.abilist64=` empty |
| Kernel / OS | Linux **6.1.118**, Android 14.0.0, patch 2025-01, boot header **v2**, `androidboot.selinux=permissive`, `rk30board`, `wificountrycode=CN` |
| RAM | **unknown** — not in the DTB (U-Boot injects it). DDR init: `v1.11 4bit PCB`, `lp3_freq=666` |
| Storage | **Samsung eMMC**, 15028 MB = 30,777,344 sectors = 14.68 GiB. Block-addressed, not MTD/SPI |
| GPU | Mali-450 (Utgard). Lima *does* support it (Mesa docs), but mainline has no display |
| WiFi | **SeekWave EA6621Q** (`"seekwave,sv6160lite"`, NV `SEEKWAVE_NV_SWT6652.bin`) on SDIO. Broadcom `ap6275s` node exists but is **disabled** |
| BT | `bluetooth-platdata` on uart2; `skwbt` driver + `sv6160.nvbin` shipped in firmware |
| IR | `rockchip,remotectl-pwm` on pwm3, **status okay**, key table in DTS → remote works with no USB port used. NOT rc-core |
| Ethernet | `gmac0` `phy-mode="rmii"`, **status okay**, integrated PHY `ethernet-phy-id0044.1400` — but the stick has no RJ45 |
| Ports | one USB-C = **power + data + flashing**; one USB-A = host. Button next to the C jack |
| Serial no. | `SN<redacted>` |

## 3. The read ceiling — the real story of the evening

**Symptom:** every read past LBA 65536 (exactly 32 MiB) returned `0xCC` filler **with a
success status**. Silent fabrication, not an error.

**Ruled out, one by one, each with evidence rather than assumption:**
- *the eMMC is unwritten there* — wrong. `boot`'s header declares a 1,344,505-byte ramdisk and
  Android boots, so real data must exist there.
- *rkdeveloptool's read method* — patched `RWMETHOD_IMAGE`(0) vs `RWMETHOD_LBA`(1) selectable
  via `RK_DIRECT_LBA=1`; **both** returned filler. (Device advertised "Direct LBA: enabled".)
- *a USB hub* — the box is on Bus 001 Port 002, directly on the xHCI root hub.
- *an OEM lock* — it returns data rather than refusing, the boundary is exactly 2^16 sectors,
  and the first 32 MiB came freely to any host including the `SSKR` key partition and `vbmeta`.
- *my container packing* — the field-proven `x88pro_maskrom_loader.bin` from an independent
  build chain stalled identically.

**Cause:** the **factory loader in sector 64 caps its own reads at 32 MiB.** Proven by
elimination and by the fix: reading through a *download-mode* loader returns real data
everywhere.

**The fix, in the order that worked:**
1. `rkdeveloptool rd 3` (`RST_RESETMASKROM_SUBCODE`) → maskrom **in software**, no pads, case
   stays shut. (Held-reset-button also reaches loader/maskrom directly.)
2. hiifong/rkdevtool v0.16.0 (Tauri2 + Rust, native RockUSB) → Advanced → **Download Boot** →
   `L5_factory_ddr.bin`. Its log showed the reason the CLI failed: the container has **two
   `0x471` entries, `UsbHead` first, then the DDR blob**, and the Rust code walks the entry
   table while the 2019-era C tool does not.
3. Reads then span the whole device. Quirk to remember: `rkdeveloptool ld` still prints
   **"Maskrom"** in that session (the temp loader keeps BootROM's `bcdDevice 0100`; the tool
   classifies on `bNumConfigurations`), but RockUSB reads work.

## 4. Backup state — everything needed exists, and is cross-validated

`recon/live-full/full.img` — 15,758,000,128 B = exactly capacity.
`sha256 3fc120f99b56d8888686a39e95d6ea8b5d0025c6d67adffac3f13129bcf3c471`

- primary **and** backup GPT: header CRC32 and 128-entry array CRC32 both recompute ✅ (the
  backup GPT was the tell that all earlier "successes" were fake)
- first 4 MiB byte-identical to `recon/20261119-1645/pi-head.bin`, captured through a
  *different* loader session
- `boot` first 1 MiB identical to the standalone 64 MiB read
- `super` unpacked via `lpunpack` → 8 ext4 sub-images (system 1.22 GB, product 484 MB,
  vendor 136 MB, system_ext 49 MB, vendor_dlkm 18.6 MB, odm 815 KB, ±262 KB dlkms)

Anti-brick artifacts, all verified real: **`pi-head.bin`** (factory DDR banner
`DDR 56f70fd2ad huan.he 25/07-21-09:51.33, fwver: v1.11 4bit PCB` + `SPL ... fwver: v1.06`),
`partitions/uboot.img` (4 MiB, `d00dfeed` FIT, 99.9 % non-filler), `live-2138/boot.img`
(dtb `d00dfeed` 98,687 B decompiled to 4,641 lines; ramdisk gzip; second = `RSCE` resource).

**Worth deleting (~44 GB of verified filler):** `recon/20261119-1645/partitions/` beyond
32 MiB, `recon/rkdevtool_dump_20261119-210716.img` (25 MB, truncated), and
`recon/rkdevtool_dump_20261119-211003.img` (14.7 GB, correctly sized, padded with filler).
`tools/reclaim-filler.sh` dry-run lists candidates (start LBA beyond the ceiling **and** every
byte verified uniform) before `--delete`.
