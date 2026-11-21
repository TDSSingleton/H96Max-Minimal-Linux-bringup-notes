# Kernel choice and the factory kernel config

Part of the H96Max research notes; index in [README.md](README.md). Sections keep their original § numbers, so a `§N` reference anywhere resolves through the index.

## 13. Linux on RK3528/RK3518 — kernel choice, what works, what does not exist

**Kernel: Rockchip vendor BSP 6.1 is mandatory for a TV box, not a preference.** Mainline
`rk3528.dtsi` exists and is broad (CPUs, SCMI, CRU clocks, pinctrl/GRF, power domains,
eMMC dwcmshc, SDIO, USB2 + DWC3 USB3, PCIe, both GMACs, OTP, watchdog, and a GPU node
`"rockchip,rk3528-mali", "arm,mali-450"` for lima) — but it has **no VOP, no HDMI, no
display-subsystem, and no codec nodes**. `rockchip,dw-hdmi.yaml`'s compatible enum stops at
`rk3568`. So mainline = serial console and nothing on the TV. Vendor branch = Armbian's
`rk35xx` `vendor` source `rk-6.1-rkr7.2` (`vendor-next` = `rk-6.12-rkr1.1`); Rockchip's own
branches are `develop-5.10/6.1/6.6/6.12`.

Vendor 6.1 display/audio/codec stack (from `rk3528.dtsi` + `rk3528-evb.dtsi`):
- `vop@ff840000` = `rockchip,rk3528-vop`; VP0→hdmi, VP1→TVE/CVBS. **[§18: it is driven by the
  vendor VOP2 driver — `rockchip_vop2_reg.c:7533` — so "not VOP2" was wrong.]**
- `hdmi@ff8d0000` `rockchip,rk3528-dw-hdmi` + `hdmiphy@ffe00000` + `hdcp2@ff8c0000`, and
  crucially **`hpd-gpios = <&gpio0 RK_PA2 GPIO_ACTIVE_HIGH>`** — hotplug is a DT GPIO on this
  family, which mainline glue has no property for.
- HDMI audio is a *vendor machine driver*, not `simple-audio-card`:
  `compatible = "rockchip,hdmi"`, `rockchip,cpu = <&sai3>`, `rockchip,codec = <&hdmi>`
  (`CONFIG_SND_SOC_ROCKCHIP_HDMI=y`). Analog out = sai2 → internal `rk3528-codec`.
- CEC: `CONFIG_DRM_DW_HDMI_CEC=y`; driver is the dw-hdmi internal `dw-hdmi-cec` block (not
  s5p-cec, not cec-gpio). No cec child node found in the rk3528 DTS — needs a boot test.
- Codecs live behind `mpp_srv` (`rockchip,mpp-service`): rkvdec (`rkv-decoder-rk3528`/`-v2`),
  vdpu, avsd_plus, iep, vdpp, rga2, jpegd → userspace is **`/dev/mpp_service` +
  `/dev/dma_heap/*`**, i.e. MPP, *not* V4L2.

**Video decode: MPP only, on either kernel.** Mainline `rkvdec`'s compat list is
rk3288/rk3328/rk3399/rk3588/rk3576 — no RK3528 — so `--hwdec=v4l2m2m`, gst v4l2codecs and
`ffmpeg -v4l2m2m` have **no device** here. `rockchip-linux/mpp` README lists RK3528/RK3528A
as supported (all formats except VC1; no AV1 on this silicon). The `*_rkmpp` decoders are in
**`nyanmisaka/ffmpeg-rockchip`** (not "rkmpp" — that repo 404s) which states outright
*"Rockchip BSP/vendor kernel is necessary, 5.10 and 6.1 are two tested versions."*

**GPU: I was wrong to write off mainline GL — Lima supports Mali-450.** Mesa lists
Mali-450/Utgard as *Supported* (upstreamed Mesa 19.1 / Linux 5.2), GLES2/GL2.1 only, and
ES3.x/Vulkan will never come. But the kernel side is the catch: mainline has no display, and
the vendor defconfig enables Arm's blob `CONFIG_MALI400/MALI450` (closed userspace). A
DIY **vendor-kernel + `CONFIG_DRM_LIMA`** build is plausible on paper (the `arm,mali-450`
node is there, one must be disabled) with **no report of anyone having done it on RK3528** —
that's the highest-value experiment behind the whole "media center UI" question, because
**Kodi is GBM/GL-only (no fbdev backend) and LibreELEC's Rockchip project lists only
RK3288/RK3328/RK3399**; CoreELEC is Amlogic. No media center runs on RK3528 Linux today.

**No-GL path that does work, verified from source:** `mpv --vo=drm` uses
`drmModeCreateDumbBuffer` + libswscale + pageflip, **no EGL/GL/GBM** — fine at 1080p, rough
at 4K60 on A53. KMS via `modetest`/libdrm gives mode-set + **NV12 on VOP overlay planes**
(hardware scanout, CPU never touches pixels) — the honest 4K recipe is a small custom player:
MPP decode → dma_heap → DRM atomic NV12 plane. Vendor DRM + fbcon also gives a plain console
with zero GPU userspace.

**IR remote: not rc-core.** No `drivers/media/rc/rockchip-cir.c` in mainline (404); vendor
uses `CONFIG_ROCKCHIP_REMOTECTL(_PWM)=y` with the demod on a **pwm3 capture pin** and the
NEC table **in the DTS** (`remote_pwm_id`, `rockchip,usercode = <0x4040>`,
`rockchip,key_table`, `#include <dt-bindings/input/rk-input.h>`), emitting ordinary evdev
`KEY_*`. So `ir-keytable`/`rc_keymaps`/lirc do not apply; remap = edit DTS or use
`keyd`/`input-remapper`. (Whether our stick populates an IR demod at all: unknown until the
DTB.)

**Bluetooth/WiFi — the real risk.** BT side of the combo chips: vendor `BT_HCIUART_ROCKCHIP`
+ Android-style `wireless-bluetooth` `bluetooth-platdata` node (`bt_port=/dev/ttyS2`, reset/wake
GPIOs) and the SDK's attach daemon; **no AIC or SeekWave BT driver or firmware in mainline or
linux-firmware**. WiFi: AIC8800 is out-of-tree only (`radxa-pkg/aic8800`, "official support
for Radxa devices only", **not in linux-firmware**); SeekWave **SWT6621S has a driver only for
the USB transport** (`ohaiibuzzle/swt6621s`, WiFi-only, kernels ≤6.4, not DKMS) — **if the
module is SDIO-attached there is no Linux driver at all**. **[Superseded by §14/session log §5 (docs/log/hardware-support.md):
  Rockchip ships an SDIO driver (`seekwaveplatform_v20`, `skw_sdio`).]** AP62xx = `brcmfmac`, but needs
module-matched `nvram.txt`/`clm_blob`. → Treat WiFi as a bonus, and check whether the stock
"remote" is actually a 2.4 GHz **USB HID dongle** (works on anything) before investing in BT.

**Ethernet would have been easy — RK3528 has two GMACs and `gmac0` carries an INTEGRATED
10/100 RMII PHY** (`rmii0_phy: ethernet-phy@2`, `phy-is-integrated`,
`"ethernet-phy-id0044.1400"`, `CONFIG_ROCKCHIP_FEPHY=y`; present in mainline too), which is
why cheap boxes get RJ45 with no external PHY. **Our stick has no jack**, so the practical
answer is a mainline USB-ethernet dongle (rtl8153 / ax88772) on the single port.

**eMMC wear is readable, so measure it instead of guessing:**
`/sys/block/mmcblkN/device/life_time` and `.../pre_eol_info` (verified `MMC_DEV_ATTR` in
mainline `drivers/mmc/core/mmc.c`; 0x0B = exceeded), or `mmc extcsd read /dev/mmcblkN` from
`mmc-utils`. Host is `mmc@ffbf0000` `rk3528-dwcmshc`, HS400-ES. Mitigations for a recycled
part: read-only squashfs root + overlay (`CONFIG_OVERLAY_FS` is **absent** from
`rockchip_linux_defconfig`, enable it), `noatime`, no swap (zram is already `=y`), volatile
journald, no mount-time discard, A/B rootfs so an update is one `dd` not an in-place OTA.
`root=PARTUUID=` is already the vendor scheme, and `reboot-mode` gives
`mode-bootloader`/`mode-loader` → software path back to flashing.

**Cross-agent contradiction, resolved:** the userspace agent flagged "no evidence mainline
U-Boot supports RK3528". That is wrong — the boot-chain agent fetched
`configs/rock-2-rk3528_defconfig` from u-boot master, and `generic-rk3528_defconfig` is what
both RK3518 box projects build (`u-boot` v2026.07, binman-produced `u-boot.itb`). Mainline
U-Boot is available; mainline *kernel display* is not. Those are different questions and only
the second is a no.

**Recommended shape (pending the DTB):** vendor 6.1 BSP kernel + board DT derived from
`rk3528-evb1-ddr4-v10`/rock-2f and grafted with *our* board's pins; Buildroot rootfs; system
in eMMC (≈13.7 GiB of `super`+`userdata` space) so the single USB port stays free; KMS
console + `mpv --vo=drm` now, MPP→NV12-plane player for 4K; USB-ethernet dongle for network,
plugged in only when wanted (air-gap by construction). Lima-on-vendor-kernel is the one
experiment that could upgrade this to a real Kodi-class UI.

## 20. The factory kernel's own `.config` (2026-11-21)

- The `boot.img` kernel is a 32-bit zImage whose payload is **LZ4** (legacy magic at +0x6761
  inside the zImage; `CONFIG_KERNEL_LZ4=y`). Decompressed, it is 23,643,228 B, banner
  `Linux version 6.1.118 (s401@R730-2) (Android clang 17.0.2 r487747c, LLD 17.0.2) SMP PREEMPT`.
- **`CONFIG_IKCONFIG=y`**: the full vendor config is embedded (`IKCFG_ST`). Saved as
  **`research/vendor-kernel-6.1.118.config`** (6,734 lines, sha256 `10669e37…6f59`). This is
  the exact config of a kernel known to drive this board, and the best starting point for
  a 32-bit build (`make ARCH=arm olddefconfig` on the 6.1.172 tree).
- Key values: `ARM=y`, `VMSPLIT_3G`, `HIGHMEM=y`, no LPAE, `CPU_RK3528`, `ROCKCHIP_VOP2`,
  `ROCKCHIP_DW_HDMI`, `MMC_SDHCI_OF_DWCMSHC`, `ROCKCHIP_FIQ_DEBUGGER`,
  `ROCKCHIP_REMOTECTL_PWM`, `MALI450`, `MODVERSIONS`, `MODULES=y`, `DEVTMPFS=y`,
  `ANDROID_BINDER_IPC=y`.
- **`# CONFIG_VT is not set`**, and there is no `DRM_FBDEV_EMULATION` or
  `FRAMEBUFFER_CONSOLE`: the vendor kernel has no text console on HDMI at all. Our config
  must add `VT`, `VT_CONSOLE`, `DRM_FBDEV_EMULATION` and `FRAMEBUFFER_CONSOLE`, and the
  DTB's `fbdev_emulation=0` must be removed (§19).
- `KALLSYMS_ALL` is off, but the plain kallsyms table is present, so function-level symbol
  recovery from the binary is possible if ever needed.
