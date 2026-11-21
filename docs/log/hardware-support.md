# WiFi and display support

Part of the session log (the narrative: what happened and why); index in [README.md](README.md). Technical detail is in `research/notes/`.

## 5. WiFi: the biggest unknown became a solved problem

`vendor.img`'s manifest lists the ODM's shipped drivers, and `vendor_dlkm.img` contains the
built modules: `skw.ko`, `skw_sdio.ko`, `skw_sdio_lite.ko`, `skw_bootcoms.ko`,
`swt6621s_wifi.ko` (+ aic8800×3, bcmdhd, cyw88459, 8822cs, 8852bs). All
`vermagic=6.1.118 SMP preempt mod_unload modversions ARMv7 p2v8`.
Build path leaked from the .ko: `/home/s401/work/sdb1/rk/3518_0428_dongle/
Rockchip_Android14.0_MS_SDK_RELEASE/external/wifi_driver/ea6621q/drivers/skwifi/skw_core.c`.

Ripped into `vendor/seekwave/` (105 files, 2.7 MB): 5 prebuilt .ko, ~57 firmware/NV files,
plus Neardi's `seekwaveplatform_v20/{sdio,usb,skwutil}` + `bt-skwbt` source (SDIO match
`SDIO_DEVICE(0x1FFE, 0x6316)`). Kconfig symbols: `SEEKWAVE_BSP_DRIVERS_V20`, `SKW_SDIOHAL`,
`SKW_BT`.

Earlier belief corrected: I had reported "SeekWave on SDIO probably has no Linux driver."
Wrong — Rockchip ships one, and the BT side exists too. **Unresolved:** the `EA6621Q_*.bin`
files are 428 B (NV only); the module's RAM firmware is unaccounted for (likely inside
`skw.ko` via the `.rel.dat` references).

## 7. Display/kernel truth for the plan

Mainline RK3528 has **no VOP, no dw-hdmi, no display-subsystem and no codec nodes** → mainline
= black TV. Vendor 6.1 BSP is mandatory for a *TV* box. Vendor stack: `rockchip,rk3528-vop`
(not VOP2) + `rk3528-dw-hdmi` + `rk3528-hdmi-phy` + `hpd-gpios = <&gpio0 RK_PA2 …>`; audio is a
vendor machine driver `compatible="rockchip,hdmi"` with `rockchip,cpu=<&sai3>` (not
simple-audio-card); decode is **MPP only** (`/dev/mpp_service`, `/dev/dma_heap`;
`nyanmisaka/ffmpeg-rockchip` says the vendor BSP kernel is required) — mainline `rkvdec` has no
RK3528 in its compat list, so `v4l2m2m` has no device.
No-GL playback does work: `mpv --vo=drm` uses dumb buffers + swscale + pageflip, **no EGL/GL/GBM**.
Kodi needs GBM/GL and LibreELEC's Rockchip target is RK3288/3328/3399 only → no media center
on this chip today. Lima-on-vendor-kernel is the untested experiment that could change that.
