# eMMC layout / maskrom, and the single USB port

Part of the H96Max research notes; index in [README.md](README.md). Sections keep their original § numbers, so a `§N` reference anywhere resolves through the index.

## 3. Storage / boot / flash

- These boxes boot from **eMMC with a GPT**, not MTD/SPI NOR. `sfc` SPI controller is
  enabled in DT but has no flash child node.
- eMMC geometry: **sector 64** = factory idbloader (board-tuned DDR blob + SPL),
  **sector 16384** = `u-boot.itb` (BL31 + U-Boot FIT), OS partitions from sector 32768.
- CRITICAL: the **only copy of the tuned DDR init blob lives in the stock idbloader at
  sector 64**. Public rkbin DDR blobs (v1.11/v1.13) fail write-training (`wrtrn err`) or
  train "marginal" and produce a Synchronous Abort whose faulting address is ASCII
  (`far 0x6566782d316d3170` = `efx-1m1p`). Community rule: *never let an install overwrite
  sector 64 with a non-factory idbloader.* `armbian-install` is reported to soft-brick
  RK3518 boxes exactly this way.
- ⇒ **`dd` a full image of the original eMMC before touching anything.** Unanimous advice.
- Boot order: **eMMC boots first**; a working box ignores SD until the eMMC loader is
  erased/overwritten. SD-boot is possible but requires that destructive step.
- `super` on one tested board: LBA `0x001FD000`, length 2,516,582,400 B. Android builds
  emit *sparse* super → `simg2img` to raw first. Updating apps/HAL usually needs only
  `super`, userdata can be kept.
- Maskrom entry on this family: (a) recessed reset button hidden behind the AV/headphone
  jack, (b) **two test pins under the main heatsink** ("remove the heatsink, you will see
  the two pins"), (c) `adb reboot loader` / `reboot loader` — the DT has
  `syscon-reboot-mode` with `mode-bootloader = <0x5242c309>`, so download mode is
  implemented. The classic "short eMMC CLK to GND" trick is **not** documented for RK3518.
- Maskrom is *not* a guaranteed fallback once the loader area is corrupt — one open issue
  reports a bricked box that never reappears in RKDevTool ("No Devices Found", red LED).
- Loader blob floor for RK3518: needs **rk3528 rkbin**, `bl31 >= v1.19`, `usbplug >= v1.04`
  (BL31 v1.17 fails "SoC not recognized"; v1.20 prints "RK3518 SoC"). kxn's loader
  generator packs DDR 1.11 + USB plug 1.04 + SPL 1.07 for LPDDR3 780 MHz — parameters
  valid **only for that board**.
- `2207:350b` is confirmed for RK3588-class only; the RK3518/RK3528 maskrom PID is
  **unverified** in every source found. Stock rkdeveloptool rules stop at `350a` (RK3566).
- The maskrom download "loader" is a DDR+USB/SPL container, **not** a raw partition image —
  don't write it as an ID block.

## 4. USB — why the single port hurts

Vendor SoC DT exposes exactly two USB port domains:
- `usbdrd` DWC3 `usb@fe500000`, upstream `dr_mode = "otg"`, phys = `u2phy_otg` + the
  **shared** naneng combo PHY (`combphy_pu`) that PCIe also uses.
- one USB2-only host pair: EHCI `usb@ff100000` + OHCI `usb@ff140000`, phys = `u2phy_host`.

There is **one** OTG/device-capable controller — that is the only path to maskrom. On RK3518
the box's Android DT sets that controller to `dr_mode = "host"`. So: flashing needs the one
OTG-capable socket to be in device mode, and while it is, you have exactly zero host ports
for a keyboard/controller/SD reader. Best documented match for "H96 Max with one USB port"
is the RK3528 **H96 Max M7** (CNX): 1× USB 2.0 port, microSD slot, HDMI, **no ethernet**.

Console UART: **1500000 8N1**, GPIO1.8/GPIO1.9 (uart2 @ `0xffa00000`, fiq-debugger
serial-id 2) — and **GPIO banks are a 1.8 V domain**, so a 3.3 V USB-UART adapter is the
wrong level (one user tried an ESP32-C3 bridge and got garbage). uart0 @ `0xff9f0000` also
exists; use address-based earlycon, not `ttySx`, because the alias differs between DTs.
UART buys you: BL31/DDR banner/SPL/U-Boot/kernel log, chip ID, and a U-Boot `mmc` prompt
for recovery. Whether a stock user build drops to a shell on console: **not documented**;
do not assume it.
