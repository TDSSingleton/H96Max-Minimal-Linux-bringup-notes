# Workstation tooling and maskrom loaders

Part of the H96Max research notes; index in [README.md](README.md). Sections keep their original § numbers, so a `§N` reference anywhere resolves through the index.

## 8. Local workstation state (Fedora 45, x86_64)

- Installed: `adb` 1.0.41 (37.0.0p1), `fastboot`, `mkbootimg`, `unpack_bootimg`, `7z`,
  `python3`, kernel modules for ch341/cp210x/ftdi_sio, `wine`.
- **Missing: `rkdeveloptool`** — available in the repos as `rkdeveloptool-1.32-14.fc45`
  (upstream predates RK3518; its udev rules stop at `0x350a`, no RK3518/RK3528 PID, and
  there are **no `/etc/udev/rules.d` rockchip rules**). Will likely need a newer
  `radxa/rkdeveloptool` or a hand-written rule for `0x2207:*`.
- Missing: `binwalk`, `afptool_android`, `img_repack`, `bootimg.py` (Python `pyusb` absent;
  `libusb-1.0` present), `qemu-system-arm`.
- Nothing Rockchip on the bus right now; no serial devices present.

## 10. Second pass — tooling built, and the Hugsun X88PRO as our Rosetta stone

User answered: **recon-first**, workstation is **this Fedora box (Linux only preferred)**
with a **Windows VM available**, and **no UART adapter yet** (so recon must not depend on
serial). That steered the whole plan to *Maskrom read-out*, which needs no serial, no
network and no OS boot.

### The load-breaking find: `ohaiibuzzle/armbian-tv98-rk3518` (branch `senpai`)
A working Armbian bring-up for a **Hugsun X88PRO = RK3518 box**, shipped with exactly the
artifacts we were told did not exist:
- `recovery/x88pro_maskrom_loader.bin` (477,632 B) — **a prebuilt RK3518 Maskrom loader.**
  Removes "must hand-build a DDR loader blind" as the first attempt.
- `armbian/userpatches/x88pro/factory_idbloader_x88pro.bin` (2 MiB) — a real dumped
  factory sector-64 blob, for offline DDR-banner comparison.
- `recovery/stock_uboot.img` (4 MiB), `recovery/x88pro-restore-sd.img.xz`, `restore-boot.cmd`.
- Its eMMC table: sector 64 = factory idbloader **DDR v1.13** (`3d08567393 huan.he 26/04/02`,
  **DDR4**) + SPL v1.06; sector 16384 = mainline U-Boot 2026.07 `u-boot.itb` (BL31 **v1.21**,
  `SYS_MMC_MAX_BLK_COUNT=2048`); rootfs from 32768 with `rk3518-hugsun-x88pro.dtb`.
- **"Do NOT use `armbian-install`"** — it writes a much older loader and bricks; they ship
  `x88pro-install-emmc` instead. Independently corroborates §3's sector-64 rule.
- **eMCP wear: "~70% lifecycle used, likely due to the chips being pulled from older
  mobile devices."** Their advice: `blkdiscard` the eMMC, restore idbloader + u-boot, run
  Armbian from external storage *exclusively* to avoid wear. New constraint: the flash may
  be near end-of-life regardless of us, which argues for imaging now and for a design that
  does not hammer eMMC with writes.
- WiFi here is **SeekWave SWT6621S**, not AIC8800D80 → at least two WiFi variants on
  RK3518 boards, so the "AP62xx means older SoC" test in §2 is weaker than stated
  (AIC **or** SWT6621S both = RK3518 generation).
- Its working list includes **Ethernet**, so some RK3518 boards do have RJ45 — per-board.

### rkdeveloptool: both public trees are macOS-broken
`rockchip-linux/rkdeveloptool` **and** `radxa/rkdeveloptool` both ship a CMakeLists with
`PROJECT_NAME "rkDevelopTool_Mac"`, hardcoded `/usr/local/Cellar/libusb/1.0.22/...` include
and link paths, and a `.dylib` link target → cmake fails on Linux with
`libusb.h: No such file or directory`. The tree is otherwise normal Linux code
(`main.cpp`, `RKBoot.cpp`, `configure.ac`), so **build with autotools**:
`autoreconf -fi && ./configure && make`. Fedora's `rkdeveloptool-1.32-14.fc45` is the
fallback. CMake 4.x needs `-DCMAKE_POLICY_VERSION_MINIMUM=3.5` regardless. Also: the
bundled `99-rk-rockusb.rules` stops at `0x350a` (RK3566) and the bundled `config.ini`
loader recipe is **empty** → authored our own rule (`tools/99-rockchip.rules`, vendor `2207`
only, `TAG+="uaccess"`) and our own INI.

### Loader packing recipe, reproduced in `tools/build-loaders.sh`
Pinned official rkbin blobs, verified by **git-blob SHA** computed locally with
`git hash-object` (same assertion style kxn's script uses, so a swapped blob is caught):
- commit `3e288fe814e059dd06833495f845cab04ac20a5c`: `rk3528_usbplug_v1.04.bin` `d10f48a2…`,
  `rk3528_spl_v1.07.bin` `a3868902…`, `rk3528_ddr_1056MHz_v1.14.bin` `d73666a4…`,
  `tools/boot_merger` `93268200…`, `tools/ddrbin_tool.py` `745a5006…`
- commit `feab2172b40f831a1f0c0e2eacc348c19ea2f780`: `rk3528_ddr_1056MHz_v1.11.bin` `2b703a38…`

`boot_merger` INI: `[CODE471_OPTION] Path1=<ddr>` (0x471 = DDR/init),
`[CODE472_OPTION] Path1=<usbplug>` (0x472 = usbplug/SPL), `[LOADER_OPTION]
LOADER1=FlashData=<ddr>, LOADER2=FlashBoot=<spl>`, `[SYSTEM] NEWIDB=true`,
`[FLAG] 471_RC4_OFF=true, RC4_OFF=true, CREATE_IDB=false`, `[VERSION] MAJOR=1 MINOR=4`,
`[CHIP_NAME] NAME=RK3528`; output must begin with ASCII magic **`LDR `**.
LPDDR3 780 MHz variant = copy the v1.11/v1.14 blob then
`ddrbin_tool.py rk3528 params.txt <blob>` with `lp3_freq=780`. **`ddrbin_tool.py rk3528 -g
out.txt <blob>` reads a blob's DDR type and frequency offline** — so once we have any dump
we can prove which DDR family the board wants without the device in hand. Run fetched
Python from *our* directory with `python3 -I`, never in place.
Four loaders to try, in order: X88PRO prebuilt → stock v1.14 → lp3_780 v1.11 → lp3_780 v1.14.
**A loader that mis-trains costs only a replug** — it runs in RAM and is never written.

### Two gates added to the read-out, because silent corruption is the real hazard
1. **`rl` unit probe** (`rkdeveloptool rl 0 1 f` → 512 B means sectors, 1 B means the
   pine64 byte-count quirk). Wrong assumption = a truncated backup you believe is good.
2. **double-read hash compare** of the head region. Marginal DDR training returns corrupt
   data *with no error*, so differing SHA256s mean reject the loader and try the next.

### Deliverables written this pass
`tools/99-rockchip.rules`, `tools/setup-workstation.sh`, `tools/build-loaders.sh`,
`tools/gpt_capacity.py`, `tools/dump-emmc.sh` (read-only; `wl`/`wf`/`wc`/`ef` appear
nowhere), `docs/RECON-RUNBOOK.md`. Already on the host, confirmed: `simg2img` + `img2simg`
(`android-tools-37.0.0p1`), `mkbootimg`/`unpack_bootimg`, `dtc`, `e2fsprogs`,
`libusb1-devel`, autotools + gcc-c++ — so no installs are strictly required
(`binwalk`/`lz4` optional). **Not yet executed:** the build scripts — the auto-mode Bash
classifier blocked `autoreconf` and `rm -rf`-adjacent commands, so they are queued for the
user to run.

### Still the top unknowns
Chip/DDR identity and board silkscreen; **arm32 vs arm64 userspace** (gates the whole
GammaOS-GSI idea); stock `boot.img` header version, ramdisk compression and whether AVB is
enforced; which physical socket is the OTG one; and whether Maskrom enumerates at all on a
board this old — the one documented failure case never recovered.
