# WiFi: SeekWave SWT6621S bring-up

Part of the H96Max research notes; index in [README.md](README.md). Earlier WiFi facts: §14, §19.

## 24. Driver port from Neardi's SDK (2026-11-21)

**The chip.** SDIO `1ffe:6621` on `mmc0` (`ffc20000.mmc`, SDR104, non-removable, pwrseq).
The box DT calls it `seekwcn_boot { compatible = "seekwave,sv6160lite"; dma_type = 1;
seekwave_nv_name = "SEEKWAVE_NV_SWT6652.bin"; gpio_chip_en/chip_wake/host_wake = gpio1 10/12/11 }`.
The ODM ran the "lite" stack (`external/wifi_driver/swt6621s/{seekwaveplatform_lite,
swt6621s_wifi}`), prebuilt 32-bit for 6.1.118, so unusable; no public source.

**Source used:** Neardi RK3588 SDK (kernel 5.10.110): `drivers/misc/seekwaveplatform_v20`
(SDIO HAL + chip boot) and `drivers/net/wireless/rockchip_wlan/skwifi` (cfg80211 fullmac,
`skw6316_dev` 2.0.240430). Kinship with the ODM binaries by shared string literals: SDIO layer
81 % of Neardi's literals present in ODM `skw_sdio_lite.ko`; WiFi 69 % in `swt6621s_wifi.ko`.
Pristine copy: `tools/wifi/neardi-orig/`; ported tree: `tools/wifi/seekwave/` (diff them).

**Firmware compatibility checked offline.** The ODM `SWT6621S_IRAM_SDIO.bin` (FW build
20250618) parses exactly as Neardi's `skw_boot_init()` expects: `kees0616` head @0xec,
`evaw0616` tail @0x150, NV block `TSVN…DEVN` @0xc8, IRAM load 0x100000, DRAM 0x20200000,
7 module entries. Whether the *message protocol* of this newer firmware matches the 2024
driver is only answerable on the box.

**Port changes (all marked `H96Max`):** SDIO id `0x6621`; DT compatible `seekwave,sv6160lite`;
firmware names as module params (defaults `SWT6621S_{IRAM,DRAM,NV}_SDIO.bin`; NV_SDIO ==
NV_SDIO_SHARE byte for byte, ALONE differs); `CHIP_ID="SWT6621S"` so the calibration file is
`SWT6621S_SEEKWAVE_R00001.bin` (the old table would say EA6652AS); 6.1 API: `mmc_sw_reset(card)`,
`netif_napi_add_weight`, `cfg80211_ch_switch_started_notify(.., link_id 0, ..)`, one
`set_default_key` call via the link_id wrapper. Built as external modules
(`tools/wifi/build-seekwave.sh`): `skw_sdio_v20.ko`, `skw6316.ko`, vermagic `6.1.172 SMP
mod_unload aarch64`, no unresolved symbols, GPL. Kernel Image untouched (`4988454b`).

**Delivery:** `tools/make-inbox.sh` -> `build/inbox/h96-inbox.tar.gz` (stick inbox), together
with the USB debug link (§25). Not loaded at boot: `/usr/local/sbin/h96-wifi-test` loads both,
scans only, logs to `/var/log/h96/wifi-test.txt` (+ stick), crash guard `/var/lib/h96/wifi-loading`.

## 25. USB debug link on the USB-C port (2026-11-21)

Kernel `USB_CONFIGFS_NCM` (ACM was already on), boot DTB `/usbdrd/dwc3@fe500000`
`dr_mode = "peripheral"`, `maximum-speed = "high-speed"` (`tools/mk-boot-dtb.sh`, read back
with fdtget). Alpine service `h96-gadget` builds a configfs composite (1d6b:0104, ACM + NCM),
usb0 = 10.96.96.1/24, busybox udhcpd leases 10.96.96.2 to the PC with no router/DNS, sshd on
10.96.96.1 only, keys only (`build/keys/h96max_ed25519`), getty on ttyGS0. The USB-A port (stick)
is the separate EHCI/OHCI pair and keeps working.

**Removed the same day (user decision).** The PC's USB-C port cannot power a full Linux boot
(the box browns out; maskrom alone is fine), and on a 2 A charger the PHY reports
`USB_DCP_CHARGER` (data lines shorted), so the gadget can never reach a PC in practice. Gone:
`USB_CONFIGFS_NCM`, the DTB edit (USB-C back to the factory `dr_mode = "host"`), the
`h96-gadget` service, udhcpd helper and the ttyGS0 getty. Remote access is the LAN dongle
instead: `h96-lan` (DHCP on any USB-host NIC) + key-only sshd; kernel gains usbnet with
AX88179/178A, AX8817x, CDC-Ether/NCM/EEM, RNDIS host, SMSC95xx (RTL8152 was already in).

## 26. WiFi works: scan on 2.4 + 5 GHz (2026-11-21, live over the LAN dongle)

Loaded by hand over ssh (logs: build/wifi/live-load1.txt, live-load2.txt, live-scan1.txt):
- `skw_sdio_v20.ko`: SDIO 1ffe:6621 bound, `check_chipid: SV6160LITE used SDIO20`; image parse as
  predicted (iram 0x100000, dram 0x20200000, 7 segments, NV 28 B); `DOWNLOAD BIN TO CP` ~0.7 s;
  loopcheck answers `trunk_W23.20.2-rev36555-rev36563-rev36560 20250618-15:38:14`. GPIO request
  for chip_en returns -16 (already held by sdio-pwrseq) -- harmless, the chip is powered.
- `skw6316.ko`: driver 2.0.240430, `WIFIREADY`, calibration `SWT6621S_SEEKWAVE_R00001.bin`
  (2372 B, 5 blocks), BW capa 0xf, STA iface `wlan0`. MAC is a placeholder `fe:fd:fc:e6:49:48`
  (driver-made; no MAC supplied). **Protocol drift seen once:** `SKW_CMD_GET_INFO expect len: 186,
  recv len: 234` -- the 2025 firmware's info struct is 48 B longer than this 2024 driver's; the
  driver uses the first 186 B. Nothing failed because of it so far; watch fields past offset 186.
- `iw dev wlan0 scan`: 39 channels, 7 BSS, 2.4 GHz and 5 GHz (5260, 5660 MHz), strongest -61 dBm.

## 27. WPA2 connects: the SWT6621S rx descriptor (2026-11-21)

**Symptom.** Association fine, then reason 15 (4-way handshake timeout) every ~3 s; wpa_supplicant
never saw an EAPOL frame, wlan0 rx_packets 0. Logs: build/wifi/live-*.txt.

**Cause.** The 2025 firmware's rx descriptor (PN-reuse mode) is laid out differently from the 2024
Neardi driver's. With the old struct: `msdu_len` read 0 (every frame trimmed away), `msdu_filter`
read 0 instead of 5 = EAP (so the pre-auth rx filter 0x30 dropped EAPOL: "rx filter: 0x30, msdu
filter: 0x0"), and the `msdu_offset` byte was read as tid ("invlid peer: 0, tid: 10").

**Method.** (1) hex-dumped the dropped frames on the box: EAPOL-Key 1/4 from the AP, intact;
(2) disassembled the ODM's own `swt6621s_wifi.ko` (ARM32, built for this firmware:
build/wifi/odm-re/swt6621s_wifi.dis) -- `skw_rx_process`, `skw_ampdu_reorder`, `skw_rx_handler`
give every field offset; (3) compiled the new struct for arm64 and decoded the captured descriptor:
size 22, msdu_len 107, msdu_offset 74, filter 5, qos 1, tid 5, peer 0, snap_match 1 -- all
consistent. Layout (bytes from desc start): +0 bit3 is_amsdu; +2 msdu_len (non-PN); +4 bits
csum_valid/-/snap_match/retry/qos/amsdu_first/amsdu_last; +5 csum u16; +7 msdu_filter; +8 u32
sn:12 frag:4 inst:2 v:1 more_frag:1 peer:5 v:1 mc:1 -:1 tid:4; +12 pn[4]; +16 msdu_len (PN reuse);
+18 msdu_offset; +19 amsdu_idx:6 need_forward:1 mac_drop_frag:1; +20 rsvd; frame at +22.
Also seen: msdu filter enum here is WAPI=4, EAP=5 (matches the driver's enum).

**Result.** WPA2-PSK/CCMP to the 5 GHz AP completes in 5 s, DHCP lease, ping 0 % loss ~1.5 ms,
ssh over WiFi. HE (WiFi 6) MCS5 40 MHz, tx 137.6 Mbit/s, -58 dBm.
Other notes: reloading skw6316.ko alone sends the chip into recovery that fails (CP RESET fail);
reboot for a clean chip. GET_INFO reply is 234 B vs the driver's 186 B (extra tail ignored).

## 28. WiFi at boot, and the clock (2026-11-21)

`h96-wifi` (OpenRC, tools/rootfs/payload/etc/init.d/h96-wifi): insmod skw_sdio_v20 -> 5 s ->
skw6316 -> wait for wlan0 -> wpa_supplicant (-s, nl80211) + udhcpc (hostname h96max). Never
unloads the modules. Verified with a cold reboot: WiFi address answers ~45 s after reboot.

**Why it was skipped at first (two boots):** files edited on the box while its clock was wrong.
No RTC: the clock restarts from the last shutdown time (swclock) and had drifted to "Jun 14 2026"
while package files say Nov 21. The new init script and runlevel link got "Jun 14" mtimes, older
than the dependency tree, and OpenRC (mtime-based change detection) left the service out of the
boot deptree although the link existed (rc-status did not even list it; gendepends.sh did emit
it). Fixed by: chronyd (NTP) enabled, savecache dropped from shutdown, and touching the two files
with a correct clock. A rootfs built on the PC has sane mtimes anyway.

**Secrets:** /etc/wpa_supplicant/wpa_supplicant.conf exists only on the box (PSK hash, mode 600,
plain-text comment removed); it is not in the repo and a rootfs rewrite drops it -- re-run
wpa_passphrase over ssh after any reflash of userdata.
