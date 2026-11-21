# HDMI output and the first-boot console

Part of the H96Max research notes; index in [README.md](README.md). Sections keep their original § numbers, so a `§N` reference anywhere resolves through the index.

## 17. Will there be picture on HDMI? — answered from the extracted firmware, not hope

Checked 2026-11-20 against the verified capture and the unpacked boot image, because "is it
testable" turns entirely on this.

- **The vendor tree does support this SoC's display.** `downloads/kernel-rockchip` (develop-6.1,
  6.1.172) has `rockchip,rk3528-vop` data (`formats_for_rk3528_cluster` in
  `rockchip_vop2_reg.c`), `dw_hdmi-rockchip.c` and `dw_hdmi_qp-rockchip.c`, and the built
  `.config` ended up with `CONFIG_DRM_ROCKCHIP=y ROCKCHIP_VOP2=y ROCKCHIP_DW_HDMI=y
  DRM_FBDEV_EMULATION=y FRAMEBUFFER_CONSOLE=y VT_CONSOLE=y ROCKCHIP_FIQ_DEBUGGER=y`.
- **The factory DTB drives HDMI, fully enabled**: `display-subsystem` with
  `route-hdmi { status = "okay"; logo,uboot = "logo.bmp"; logo,mode = "fullscreen";
  connect = <0x17> }`, `vop@ff840000` okay, `hdmi@ff8d0000` (`rockchip,rk3528-dw-hdmi`) okay,
  `hdmiphy@ffe00000` okay, `vp0_out_hdmi` endpoint wired, `vop_mmu` okay. No node is disabled.
- **`dtbo` is a 64-byte field of zeros.** One overlay slot, no content, and nothing in it
  touches vop/hdmi — so no overlay is required for display, and `androidboot.dtbo_idx` is inert.
- **The DTB question dissolved.** The `resource` (RSCE) blob in `second` holds three entries
  (`name[32]` at +4, 20-byte sha1 at +0xe0, hash length 0x14 at +0x100, size u32 at +0x108):

      rk-kernel.dtb      98,687 B  sha1 a37115805ed9389d5045dc2b30b505b64eaf323a
      logo.bmp        6,220,856 B  sha1 dda75e74951ef388d1e6795e58d763181ee55746
      logo_kernel.bmp    22,364 B  sha1 5b6feb18a97cf9f2fc8950036fd99aaf5db60e49

  `sha1(rk-kernel.dtb)` **equals** `sha1` of the boot image's own `dtb` field — the same
  98,687 bytes in both places. So it does not matter whether u-boot passes the boot image's
  dtb or the resource's: the kernel gets the factory EVB1 DTB either way. Our rebuilt dtb
  (98,521 B, different hash) is currently unused, which is a good thing: the bytes the vendor
  shipped are the ones proven against this PCB.
- **HDMI works before Linux, and the panel mode is now known.** u-boot draws `logo.bmp`,
  which is 6,220,856 bytes = 1920×1080×3 + 56-byte BMP header: the panel is **1080p RGB888**.
  **[§18: the factory DTB sets CMA to 8 MiB in `reserved-memory`, which overrides
  `CONFIG_CMA_SIZE_MBYTES`; only `cma=` on the cmdline would change it.]** That is also why 16 MB of CMA is not enough to reason about — `rockchip_linux_defconfig`
  ships `CONFIG_CMA_SIZE_MBYTES=16`, one 1080p buffer is 6.2 MB, and double/triple buffering
  plus the VOP's other allocations crosses 16 MB. A failed DMA alloc produces no message on
  the console we can see, just a dark screen, so `build-kernel.sh` now sets 64 MB.
- **There are two consoles, and the factory command line names only the invisible one.**
  `console=ttyFIQ0` → `fiq-debugger { rockchip,serial-id = <0>; rockchip,baudrate = <0x16e360>
  = 1,500,000; pinctrl-0 = <&uart0m0_xfer>; status = "okay" }`. Note `uart0:
  serial@ff9f0000` is `status = "disabled"` — the FIQ debugger seizes the hardware without a
  driver, which is why u-boot prints there and why an 8250 `console=ttyS*` would find nothing.
  A 1.5 Mbaud probe on uart0's pads is the best channel available (FT232R/CH340 can do it;
  CP2102 tops out at 1 Mbaud) — worth hunting for pads on a board that is already open.
  Nothing goes to HDMI unless `console=tty1` is present, so `mk-bootimg.py` gained
  `--cmdline-append` and `make-linux-boot.sh` passes `console=tty1 loglevel=8`.
- **[WRONG — see §18.]** **The header's 20-byte field at 0x240 is unidentified but provably not payload-checked.**
  It is not sha1 of kernel / ramdisk / second / dtb / header page, not of any concatenation of
  them, not sha1 of the used region or the whole 64 MiB, and it occurs nowhere else in the
  image nor in dtbo/misc/vbmeta/trust. Since the factory image boots with that value while it
  matches none of its own payloads, enforcement of payload hashes cannot be live. The builder
  now **inherits it verbatim** rather than zeroing it, which makes the offline round trip
  byte-identical to the factory partition — the strongest fidelity claim available without
  touching hardware.
- **Container builds must prove the mount before they build.** The 10:09 build left no file
  behind and quoted `Makefile:1058` for `checkarmreloc`, whereas in *our* tree line 1058 is an
  include flag and the target is at 2501 — it built a different checkout in a filesystem that
  `--rm` deleted. `tools/bench-check.sh` now asserts the mount, the tree identity and the
  cross compiler by executing them.

## 21. What HDMI will show, and the first-boot console fixes (2026-11-21)

- **u-boot:** the stock env has `stdout=serial,vidconsole`, and `rockchip_show_logo` draws
  `logo.bmp` on HDMI (`route-hdmi logo,uboot`). Expect the **logo** on screen. u-boot *text*
  appears only if Rockchip's display driver provides a vidconsole device, which is unknown,
  and u-boot probably prints little before booting anyway. **The logo appearing is the proof
  that SPL → u-boot ran and HDMI came up.**
- **Kernel:** the VT console is registered at `console_init`, and fbcon takes it over once the
  DRM stack binds (≈0.19 s in the factory log), redrawing what the VT has buffered. So the
  log should be readable from early on. Something that dies before DRM binds will leave the
  u-boot logo or a black screen.
  - **Mitigation:** `pstore`/`ramoops` (the DTB reserves `ramoops@110000`) keeps the log
    across a warm reset.
- **`/dev/console` will be `ttyFIQ0`**, not tty1: the DTB's `console=ttyFIQ0` comes after our
  `console=tty1`, and the last `console=` wins for `/dev/console`. Kernel messages still go
  to every console, but `/init`'s own output must go to `/dev/tty1` explicitly, which it
  does.
- **Fixed — `/init` would have died on its 4th line.** It ran `exec >/dev/tty1` before
  mounting devtmpfs. `DEVTMPFS_MOUNT` does not apply to initramfs boots, and busybox sh exits
  when an `exec` redirect fails (reproduced under `qemu-aarch64-static`), so the result would
  have been "Attempted to kill init". It now mounts first, waits up to 5 s for `/dev/tty1`,
  falls back to `/dev/console`, and never exits over it. `REPORT_VERSION=probe-2`.
- **Fixed — `fbdev_emulation=0`.** `mk-bootimg.py --fbdev-on` (passed by
  `make-linux-boot.sh`) rewrites it to `=1`, same length, in both dtb copies (header `dtb`
  and RSCE `rk-kernel.dtb`, whose entry sha1 at +0xe0 is recomputed). The id hash is
  recomputed as usual. The factory round trip without the flag stays byte-identical.
- **Fixed — console chain asserted.** `build-kernel.sh` now enables `TTY`, `VT`, `VT_CONSOLE`
  explicitly (the vendor config has `VT` off) and refuses to build unless `VT`,
  `VT_CONSOLE`, `DRM_FBDEV_EMULATION`, `FRAMEBUFFER_CONSOLE` and `DRM_ROCKCHIP` are all `=y`.
- **Sign of life:** `/init` prints an ASCII Tux, `Linux <release> on <model>`, and the uptime
  at which userspace started, before the probe report. (`CONFIG_LOGO` stays off; the
  graphical penguin would be a one-line change.)
- **Open:** the initramfs busybox is aarch64. A 32-bit kernel needs an armhf static busybox
  (Debian `busybox-static` `armhf`); this is tied to the arch decision in §18.
