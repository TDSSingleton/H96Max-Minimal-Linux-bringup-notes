# Input (IR, CEC) and Bluetooth

Part of the H96Max research notes; index in [README.md](README.md). Earlier input facts: §29
(h96ui keys). WiFi side of the same chip: §24–§28.

## 31. Input: there is no IR receiver; CEC is not available (2026-11-21)

**Conclusion first:** this H96Max is a TV **stick** with no IR receiver. Its remote is a
Bluetooth LE HID remote (§32) with an IR emitter only for the TV's power/volume (which is why it
switched the test TV off). The vendor DT nevertheless describes an IR receiver
(`pwm3: pwm@ffa90030`, `rockchip,remotectl-pwm`, nine NEC key tables, pin group `pwm3m0` =
GPIO4_C6, `pcfg_pull_none`), because it is the generic RK3518 "EVB1" tree.

How it was established (useful method for any "is this pin alive?" question):
- remotectl driver has runtime params `code_print` / `dbg_level`
  (`/sys/module/rockchip_pwm_remotectl/parameters/`): `code_print=1` logs `USERCODE=` /
  `RMC_GETDATA=` for any NEC frame, even unknown remotes. Result: nothing.
- `/proc/interrupts`: `rk_pwm_irq` / `rk_pwm_pwr_irq` stayed at 0 during presses.
- PWM3 registers via `devmem` (busybox): CTRL `0x6005` (enabled, capture mode), INT_EN `0x88`,
  all counters 0 -> the capture unit never saw an edge. Pinmux debugfs: pin 150 (gpio4-22) is
  correctly muxed to `ffa90030.pwm` `pwm3m0`.
- Pin level: GPIO4 EXT_PORT (`0xffb20070`) bit 22 = 0, also as a plain GPIO with pull-up
  (`gpioget -b pull-up -c gpiochip4 22`) -> held low (nothing driving it high).
- **`gpio-watch`** (`tools/initramfs/gpio-watch.c`, static, no libc, ~4.8 KB): maps the five
  GPIO banks' EXT_PORT registers through /dev/mem and samples ~480 k samples/s/bank, counting
  transitions per pin. Baseline: only GPIO4_C4 toggles (~390 kHz, the vdd_cpu PWM regulator).
  During 12 remote presses: only eMMC (GPIO1_C4…D4) and SDIO-WiFi (GPIO3_A4…B4) traffic.
  Two porting bugs found while writing it: aarch64 `O_SYNC` is `04010000` (`010000000` is
  `O_PATH` -> EBADF on mmap); read argc/argv via an asm `_start` stub, not inside `_start`.
- Ruled out along the way: factory dtbo (only touches reboot_mode), header vs RSCE DTB (byte-
  identical), kernel config (ours is a superset of the factory's for IR/PWM/pinctrl/GPIO), Android
  init scripts (no IR setup), `vcc_sd` rail (toggled via GPIO4 DR bit 1: no change), GPIO hogs
  (none), io-domains (no node).

**HDMI-CEC:** the box has `/dev/cec0` (dw-hdmi, CEC 2.0) and an `hdmi_cec_key` input (no `kbd`
handler). Claimed playback LA 4 with `cec-ctl --playback --osd-name H96Max` (v4l-utils); the TV
sent nothing in 75 s, and the TV's menu says CEC is not supported (old model). Released again
with `cec-ctl --clear`. Note: around this test the TV lost the HDMI picture; it came back on the
next kernel boot (cause not isolated).

## 32. Bluetooth: SWT6621S BT side, the remote paired, survives reboots (2026-11-21)

**Kernel** (`tools/build-kernel.sh`): `BT`, `BT_BREDR`, `BT_LE`, `BT_HIDP` (UHID was already =y),
asserted. An old "trim" list from milestone 1 had been `--disable`-ing `BT`, `CFG80211`,
`MAC80211`, `WIRELESS` after the enables (CFG80211/MAC80211 survived only because something
selects them) -> removed from the list. Boot image `linux-boot.bt-56fdbede.img`, kernel `#5`.

**Driver** `tools/wifi/seekwave/skwbt/` (Neardi `drivers/bluetooth/skwbt`, pristine copy in
`tools/wifi/neardi-orig/skwbt/`), external module `skwbt.ko`. It binds to the platform device
that `skw_sdio_v20` creates (`btseekwave`), opens SDIO ports cmd 2 / data 5 / audio 3, starts the
BT service in the chip (`BTREADY`), downloads an NV blob, registers `hci0`. Changes:
1. 6.1: `set_fs`/`force_uaccess_begin`/`mm_segment_t` are gone -> no-op helpers on >= 5.18
   (the file I/O already uses `kernel_read`/`kernel_write`).
2. BD address persisted in `/var/lib/skwbt/skwbdaddr` (was `/devinfo/skwbt`, absent here) ->
   stable controller address `56:6A:ED:AA:1C:D1` across reboots, so pairings survive.
3. **NV for the lite chip:** BT reports `hci_rev 0x5302`. Neardi only knew 0x5301 (6316, short
   NV) and sent everything else the 354-byte `sv6160.nvbin` -> the chip stopped answering
   (`cp response timeout`), the firmware asserted, WiFi died with it, and the SDIO recovery
   worker oopsed (`complete()` on a bad pointer in `skw_sdio_exception_work`;
   `build/wifi/bt-oops1.txt`). The factory BT helper `libbt-vendor-seekwave.so` (vendor
   partition) chooses `sv6160lite.nvbin` / `sv6160.nvbin` / `sv6316.nvbin`;
   `/vendor/etc/bluetooth/sv6160lite.nvbin` (37 B) is byte-identical to `sv6316.nvbin`. Now:
   0x5302 -> `sv6160lite.nvbin`, sent in the 6316 packetised format.
4. **Codec query crash:** with the right NV, init ran until the Linux core sent
   `Read Local Supported Codec Capabilities` (0x100e) -> `BSPASSERT: hci_tl.c-386` (btmon trace:
   commands #1-#80 all succeed, #81 never answered; `build/wifi/bt-btmon1.txt`). The chip
   advertises the command but crashes on it; 6.1 has no quirk for it. The driver now clears
   octet 45 bits 2/3 and octet 29 bit 5 in the Read Local Supported Commands reply, so the core
   never sends the codec queries (irrelevant for a remote). Logged as `skwbt: masked codec
   commands ...`.

**Chip reset without a power cycle:** after a firmware crash a *warm* reboot does not reset the
chip (h96-wifi then hangs and OpenRC never starts sshd). In a running system:
`rmmod skwbt skw6316 skw_sdio_v20; echo ffc20000.mmc > /sys/bus/platform/drivers/dwmmc_rockchip/unbind;
echo ffc20000.mmc > .../bind` -> the SDIO power sequence pulses the chip reset; the card
re-enumerates fresh (1ffe:6621, SDR104). Then load the modules again. Cold power cycle otherwise.

**Userspace:** BlueZ 5.86 (`bluetoothd` via OpenRC `bluetooth`, needs `dbus`), `btmon`
(bluez-btmon). OpenRC `h96-bt` (after h96-wifi, before bluetooth): mkdir /var/lib/skwbt,
insmod skwbt, wait for hci0. Firmware file `/lib/firmware/sv6160lite.nvbin`.

**The remote:** "Bluetooth remote" `60:48:9C:D6:DF:C3`, BLE HID-over-GATT (Battery + HID
services), modalias `usb:v2B54p1600`, battery 91 %. Paired with `bluetoothctl` (agent
`NoInputNoOutput`; pair, trust, connect) while it blinked in its pairing mode. Linux inputs:
"Bluetooth remote Keyboard" (kbd), "… Consumer Control" (kbd), "… Mouse", base. Navigation
arrives on Consumer Control: KEY_UP 103, KEY_DOWN 108, KEY_SELECT 353 (OK)
(`build/wifi/remote-keys1.txt`). Because those inputs have the `kbd` handler, keys reach the
console and **h96ui works with the remote directly** (confirmed by the user). After a reboot the
remote is still bonded/trusted and reconnects on the first key press (confirmed).
