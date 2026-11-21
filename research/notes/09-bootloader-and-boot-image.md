# u-boot and the boot image format

Part of the H96Max research notes; index in [README.md](README.md). Sections keep their original § numbers, so a `§N` reference anywhere resolves through the index.

## 15. u-boot, the build environment, and the community FIT we rejected

Narrative and resume steps are in `docs/log/` (start at `status-and-resume.md`); the durable findings:

- **`CONFIG_ARM64` for rk3528 comes from Kconfig, not the defconfig** (`arch/arm/mach-rockchip/
  Kconfig`: `config ROCKCHIP_RK3528 ... select ARM64`). It also `imply BOOTSTD_FULL`, so distro
  boot is free. u-boot uses **`ARCH=arm`** for 64-bit; `ARCH=arm64` fails with
  `Makefile:832: arch/arm64/Makefile: No such file or directory` (that name is Linux's).
- **Fedora 45 breaks u-boot v2026.07 five ways**, all environmental, none about the board:
  swig 4.5 dropped `pycompat.i` (vendored `libfdt.i` still calls `PyString_FromString`,
  `PyString_AsString`, `PyInt_AsLong`); OpenSSL 4.0.3 deleted `ENGINE_*` and the dependency
  arrives via **`TOOLS_KWBIMAGE` default y → select TOOLS_LIBCRYPTO**, not via signing
  (`TOOLS_FIT_SIGNATURE` is `def_bool` on libcrypto so it cannot be disabled directly);
  `mkeficapsule` wants gnutls headers; `scripts/config --enable` on invented names
  (`DOS_LABEL`, `CMD_EXT4`) is a silent no-op; and `make | tail` buffering turned a Kconfig
  **prompt waiting on stdin** into what looked like a hang. Fixes now in `tools/build-uboot.sh`:
  enumerated shim, kwbimage/libcrypto chain broken, real symbol names with a state assertion,
  `</dev/null` on every make, streamed output via `tee` + `PIPESTATUS`, and `syncconfig` /
  `scripts_dtc` / `tools` pre-flights.
- **Container is the build environment:** `podman run -it --rm -v <proj>:/work:z fedora:41`.
  `:z` is mandatory — the tree is `user_home_t` under Enforcing SELinux, and the container's
  label cannot read it (manifests as `fatal: not a git repository` and `ls: Permission
  denied`). Rootless uid map keeps outputs owned by the user. `restorecon -RF` undoes labels.
  Fedora 41 still ships swig 4.x, so the pylibfdt shim stays necessary there; OpenSSL 3.x there
  has `ENGINE_*`, so the crypto chain would link but we keep it off.
- **Our v2026.07 build compiles u-boot fully** (`u-boot.bin` 913,088 B) then dies on
  `make[1]: No rule to make target 'checkarmreloc'` — a relocation lint added to `INPUTS-y` by
  `arch/arm/config.mk:143` (`ifneq ($(CONFIG_XPL_BUILD),y)`) with its rule in the main Makefile
  at 2501. Compounding it: `CONFIG_BINMAN_DTB=""` and **`u-boot.itb` has no rule**, so the FIT
  was only reachable through `all` — the path the lint blocks. Unresolved; next attempt uses the
  community-proven `generic-rk3528_defconfig` base instead of my `rock-2` detour.
- **Community `u-boot.itb` (juliovendramini/rk3518_armbian) inspected and rejected for our use.**
  FIT: AArch64 u-boot `2026.07-rc4-g1ab49f6a91ea-dirty` @`0x00800000`, `atf-1` = **bl31-v1.21**
  (clears the documented `bl31 >= v1.19/1.20` RK3518 floor; v1.17 fails "SoC not recognized"),
  833,024 B so it fits the 4 MiB `uboot` partition. `sha256 1ea0200d47b55e19d5749248ac407cfb4faa3f19813a7e77998853e47dced91d`.
  **Why rejected:** it is `generic-rk3528_defconfig`, which has **zero USB options** — and this
  stick has no SD slot. A bootloader with no input channel and no output channel, on hardware
  with no reachable serial console. `idbloader.img` (176,128 B, `sha256 e06631c0aeb7f6…`) must
  never be written: it carries a public DDR blob.
- **Architecture change that removes the console dependency:** build one FIT (kernel +
  `rk3518-evb1-ddr4-v10.dtb` + embedded initramfs + fixed cmdline), write it to the eMMC **`boot`
  partition** (`0xC800`, 64 MiB), and compile `CONFIG_BOOTCOMMAND` as
  `mmc read $loadaddr <sec> <count>; bootm $loadaddr`. No stick, no keyboard, no prompt.
  Linux's console is HDMI (vendor DRM + fbcon), which is observable; the blind window shrinks to
  SPL→u-boot→kernel-start. Optional extra channel: `rockusb` gadget in u-boot.
- **`busybox` in an initramfs must be an aarch64 binary** — the host/container package is
  x86-64 and an x86 `/init` fails as `Failed to execute /init (error -2)`, which looks exactly
  like a DTB or kernel bug. `tools/mk-initramfs.sh` now refuses a non-`ARM aarch64` binary; get
  one with `dnf install --installroot=… --forcearch=aarch64 --downloadonly … busybox` then
  `rpm2cpio | cpio -idmv`.

## 16. The bootloader question dissolved: the stock u-boot already boots our kernel

> **Superseded by §22** for the arm64 kernel: the stock u-boot is AArch32 and cannot start it.
> The boot-image layout findings below still hold (as corrected in §18).

Everything below was read out of `recon/live-full/full.img` (the verified 14.68 GiB capture)
on 2026-11-20 morning. It is why replacing u-boot is **no longer on the critical path**.

- **The factory bootloader's boot command chain**, from strings in `uboot.img`:

      bootcmd=boot_android ${devtype} ${devnum};boot_fit;bootrkp;run distro_bootcmd;
      boot_targets=mmc1 mmc0 mtd2 mtd1 mtd0 usb0 pxe dhcp
      bootcmd_usb0=setenv devnum 0; run usb_boot

  `boot_android` loads the **`boot` partition as an Android boot image** and boots it. So a
  replacement kernel does not need a replacement bootloader — it needs a replacement *image*,
  and the vendor chain already runs it first. `boot_fit` ("Boot FIT Image from memory or
  boot/recovery partition") is a fallback if the Android image is not bootable, which is what
  makes the write survivable: a bad payload in `boot` degrades to the next chain entry rather
  than to a dead box.
- **Nothing verifies the payload.** Three independent reasons: **[§18: reason 2 is WRONG — the id
  is the standard mkbootimg SHA-1 and u-boot checks it. Only reason 1 (empty vbmeta) stands.]**
  1. `vbmeta.img` is an `AVB0` shell — authentication-data length 0, hash-data length 0,
     **zero descriptors**, no public key. AVB is present in the partition table and asserts
     nothing.
  2. The boot image's own header hash slots (0x240, `id[8][16]`) hold
     `65fc3577537b593169a845e1bf6843ea2967fda2`, which is **not** `sha1` of the kernel, the
     ramdisk, `second` or the dtb — and that image boots today. If `ANDROID_BOOT_IMAGE_HASH`
     enforcement were live, the factory image would already fail it.
  3. `security` partition is not a boot signature either: it starts `SSKR` and matches the
     `rkss_read_multi_sections` / `TEEC: rkss_*` / `RPMB INIT` strings — that is Rockchip Secure
     Storage (TEE key/Widevine material), ~1 MiB of payload in a 4 MiB partition, unrelated to
     kernel verification.
- **Real GPT from the capture** (my earlier `partitions=` string was the u-boot *default*
  table, not this device's): `uboot` 0x4000 (4 MiB), `trust` 0x6000, `misc` 0x8000,
  `dtbo` 0xa000, `vbmeta` 0xc000, **`boot` 0xc800 = 64 MiB**, `recovery` 0x2c800 = 96 MiB,
  `backup` 0x5c800, `cache` 0x11c800, `metadata` 0x1dc800, `baseparameter` 0x1fc800,
  `super` 0x1fd000 = 2304 MiB, `userdata` 0x67d000 = 11.4 GiB. There is no separate
  `kernel`/`resource`/`system` partition — those are inside `super`; `resource` (RSCE) lives
  *inside* boot.img as `second`.
- **The factory boot.img layout, measured not assumed** (page size 2048):

      header @0x0000 | kernel 10714184 @0x0800 | ramdisk 1344505 @0xa38800
      second(RSCE) 6344704 @0xb81000 | dtb 98687 @0x118e000 | zeros to 64 MiB

  **[WRONG — see §18: the header is a plain AOSP v2 header.]** The header is **not AOSP-conformant**: `tags_addr` (0x10000100) sits where AOSP has
  `tags_size` (0x20), `page_size` (2048) where AOSP has `tags_addr` (0x24), and 2 where AOSP
  has `page_size` (0x28); `header_size`, `dtb_size`, `recovery_dtbo_*` are all zero. Payload
  offsets therefore come out of page arithmetic on the three size fields only — which is why
  kernel and ramdisk sizes can be changed freely. The right way to build a replacement is to
  **copy the template header verbatim and patch only what must change**, not to re-derive it
  from the AOSP spec.
- `kernel` is a **gzip self-decompressing zImage** (`00 00 a0 e1` stub, magic `0x016f2818`),
  i.e. `arch/arm64/boot/Image.gz`, *not* a raw `Image`. **[WRONG — see §18: that is a 32-bit ARM
  zImage; arm64 `Image.gz` is a bare gzip stream with no stub, and arm64 has no `KERNEL_GZIP`.]** That matters: the load address is
  `0x10008000`, which is only 32 KiB aligned, and a raw arm64 `Image` expects ≥64 KiB
  alignment per the boot protocol while the gzip stub does not care. `build-kernel.sh` now
  enables `KERNEL_GZIP` (and disables the other compressors) and emits `build/kernel/kernel`.
- The factory DTB inside boot.img is `compatible = "rockchip,rk3518-evb1-ddr4-v10",
  "rockchip,rk3518"` — **the vendor's own generic EVB board file**, which exists in our clone
  at both `arch/arm64/boot/dts/rockchip/rk3518-evb1-ddr4-v10.dts` and `arch/arm/boot/dts/`.
  The OEM did not write a board DTB; they flashed Rockchip's EVB1-DDR4-V10 default. So reusing
  the factory DTB is legitimate, and building the same one from source is a cross-check.
- Factory cmdline (kept verbatim by the builder):
  `console=ttyFIQ0 firmware_class.path=/vendor/etc/firmware init=/init rootwait ro
  loop.max_part=7 printk.devkmsg=on kvm-arm.mode=none androidboot.console=ttyFIQ0
  androidboot.wificountrycode=CN androidboot.hardware=rk30board
  androidboot.boot_devices=ffbf0000.mmc androidboot.selinux=permissive`
  — `init=/init` means our initramfs `/init` runs unmodified, and the `androidboot.*` terms are
  inert without Android init. `ffbf0000.mmc` is the eMMC controller.
- Stock modules are **6.1.118**; our tree is 6.1.172. Same series, so the factory DTB is
  compatible; only loading the ripped `skw*.ko` would need the exact 6.1.118.
- `tools/mk-bootimg.py` assembles the replacement and **verifies it by re-reading the written
  file** and locating each payload by its own signature rather than by the arithmetic that
  placed it. Round-trip test with the factory's own kernel+ramdisk reproduces the factory
  `boot.img` **byte-for-byte except the 20 stale header-hash bytes** — that is the proof the
  layout model is right, and it is a stronger check than anything asserted about intent.
- **First-write target is therefore `wl 0xc800`, not `wl 0x4000`.** Undo is
  `wl 0xc800 build/good/boot.img` (factory bytes, 64 MiB, already carved and hashed).
- u-boot trivia, recorded so it is not rediscovered: `u-boot.itb` is a **binman** product, not
  a Makefile one — `arch/arm/dts/rockchip-u-boot.dtsi` has a `fit` entry with
  `filename = "u-boot.itb"` inside `&binman`, and the only rule that runs binman in the
  top-level build is `.binman_stamp` (`all: .binman_stamp`, Makefile ~1392). So
  `BL31=<rk3528_bl31_v1.21.elf> make ARCH=arm .binman_stamp` produces the itb while skipping
  `checkarmreloc`, which is only a post-hoc relocation lint
  (`readelf -r u-boot | grep R_A` must be exactly `R_AARCH64_RELATIVE`). Moot for this plan.
- **Fedora's `gcc-aarch64-linux-gnu` is a FREESTANDING cross compiler: no target libc.**
  Measured, not inferred:
  - `aarch64-linux-gnu-gcc -print-sysroot` → `/usr/aarch64-linux-gnu/sys-root`, and that
    directory **contains zero headers** (`find … -name '*.h' | wc -l` → 0). Owned by
    `gcc-aarch64-linux-gnu` / `binutils-aarch64-linux-gnu`; `dnf -q repoquery --available
    'glibc-*aarch64*' 'glibc-*linux-gnu*'` returned no candidates, so there is no target
    libc to install here (that query returning empty is the observation — it does not prove
    no such package exists anywhere in Fedora).
  - Its include search path is only `…/gcc/aarch64-linux-gnu/16/include{,-fixed}`.
  - `-print-file-name=libc.a|crt1.o|byteswap.h` echoes the **bare name back**, which is gcc's
    "not found" answer.
  Consequence, and it is a good one: **u-boot and the kernel are unaffected**, because both are
  freestanding — u-boot compiles with `-nostdinc`, and the 6.1 kernel tree references no
  `libgcc`/`print-file-name` at all and adds `-nostdinc` (Makefile:1029), while `scripts/kconfig`
  etc. build with the **host** gcc. Only *hosted* programs like busybox hit it, and they hit it
  as `fatal error: byteswap.h: No such file or directory` three files into a 400-file build.
  My earlier "the cross compiler works, it produced `ELF 64-bit LSB relocatable, ARM aarch64`"
  was true and proved nothing about libc availability — assembling freestanding objects was
  the only thing that evidence covered.
- **The vendor's own userspace binaries are unusable as our initramfs shell, and it is worth
  knowing exactly why.** The recovery ramdisk does contain a multi-call binary
  (`system/bin/toybox`, 399,844 B), but `file` says:
  `ELF 32-bit LSB pie executable, ARM, EABI5, dynamically linked, interpreter
  /system/bin/linker, for Android 34` — 32-bit **Bionic**, needing Android's linker and
  `/system/lib/*.so`, plus `CONFIG_COMPAT` in our kernel. That is running the vendor userspace
  we are trying not to run, so `make-linux-boot.sh` rejects it and says why in the help text.
- **`set -o pipefail` turns "found" into "failed"** in the `echo "$big" | grep -q pat` idiom:
  `grep -q` exits at the first match, the upstream `echo` then dies of SIGPIPE (141), and
  pipefail promotes that to the pipeline's status. So a check reports every hit **early in the
  list** as a miss — `bb_validate` claimed 34 of 39 busybox applets were absent from a binary
  that had all of them, and the only survivors were the alphabetically-late names
  (`umount wc uname usleep xargs`), which is what pointed at the mechanism instead of at
  busybox. Use here-strings (`grep -q pat <<<"$var"` — no upstream process) for any
  "is this name in this captured list" test. Audited every `| grep -q` in `tools/`; the three
  that could bite are fixed.
- **Never select a file by `find -name`.** Debian's `busybox-static` contains **six** paths
  ending in `busybox`, and the first one traversal reached was
  `usr/share/initramfs-tools/conf-hooks.d/busybox` — a 16-byte config snippet, mode 644, which
  got silently copied out as our shell. "Take the biggest match" would have worked today by
  luck; select on what the file *is* (`file -b … | grep 'ARM aarch64'`) and, if nothing
  matches, print every candidate with size and mode.
- The fetch is resolved through `dists/stable/main/binary-arm64/Packages.gz` rather than a
  guessed pool directory (pool/ is keyed by **source** package: `pool/main/b/busybox/`, which
  is why the first attempt 404'd), and is **verified against the SHA256 the index publishes**:
  `busybox-static_1.37.0-6+b9_arm64.deb`, 847,552 B,
  `c833be48abfa16bc19c4966ec93e289ff1ce5d2f1476cad3a57bd105378cd15c`, extracting
  `usr/bin/busybox` = 1,975,064 B, mode 755, `ELF 64-bit LSB executable, ARM aarch64,
  statically linked, for GNU/Linux 3.7.0`.
- **Static-ness is a gate, not a preference.** A dynamically linked busybox in an initramfs
  with no libraries produces `Failed to execute /init (error -2)` — the same message as a bad
  DTB or a wrong-arch /init, which is how this class of mistake keeps presenting as a kernel
  bug. `mk-initramfs.sh` now refuses a dynamic binary; `make-linux-boot.sh` probes the compiler
  (`cc_can_link_libc`: compile **and link** a two-line hosted program) before committing to a
  source build, falls back to Debian's `busybox-static` arm64 package when it cannot, and
  verifies whichever binary it ends up with for arch, static-ness and the presence of every
  applet `/init` calls (busybox stores its applet names as one NUL-separated `.rodata` string,
  so `strings -a -n1` lists them one per line and they can be asserted).

## 18. Corrections (2026-11-21) — re-checked against the bytes on disk

- **The boot image header is plain AOSP `boot_img_hdr_v2`.** At their AOSP offsets:
  `tags_addr` 0x20 = 0x10000100, `page_size` 0x24 = 2048, `header_version` 0x28 = 2,
  `os_version` 0x2c, `recovery_dtbo_size` 0x660 = 0, `header_size` 0x66c = **1660**,
  `dtb_size` 0x670 = **98,687**, `dtb_addr` 0x674 = 0x11f00000. §16 assumed a non-existent
  `tags_size` field and misread the v1/v2 fields as zero.
- **The id at 0x240 is the standard mkbootimg SHA-1** over kernel, len, ramdisk, len, second,
  len, recovery_dtbo len, dtb, len (each len is u32 LE). It reproduces `boot.img`'s
  `65fc3577…fda2`, and `recovery.img`'s `01361854…c3e9` once its embedded 4 MiB dtbo is
  included. The factory u-boot contains `Hash from header`, `Hash real` and
  `ANDROID: sha1(...) + OK`, so it computes and compares the hash; whether a mismatch is
  fatal is not known. `tools/mk-bootimg.py` now recomputes the id, patches `dtb_size`, and
  checks both by re-reading its output. Its factory-payload round trip is now
  **byte-identical over the whole 64 MiB**, hash included.
- **The factory kernel is a 32-bit ARM zImage** (eight `00 00 a0 e1` NOPs, branch, magic
  `0x016f2818` at +0x24, end = 0xa37c48). An arm64 `Image.gz` is a bare gzip stream with no
  stub, and arm64 has no `HAVE_KERNEL_GZIP`, so `cfg --enable KERNEL_GZIP` in
  `build-kernel.sh` is a silent no-op (its own check printed "KERNEL_GZIP is not set" in
  `build/make-linux-boot/kernel.log`). The stock u-boot has only ever started an AArch32
  kernel; whether `boot_android` will start an arm64 one, and decompress a gzip one, is
  **unverified**.
- **The 32-bit route is buildable from the same tree:** `CPU_RK3528` (`drivers/soc/rockchip/
  Kconfig.cpu`) has no `ARM64` dependency, `ROCKCHIP_VOP2` depends only on `CPU_RK3528`, and
  `arch/arm/boot/dts/rk3518-evb1-ddr4-v10.dts` just `#include`s the arm64 board file, so the
  DTB is the same either way. `arch/arm/configs/rockchip_linux_defconfig` does **not** enable
  `CPU_RK3528`, `ROCKCHIP_VOP2`, the dwcmshc eMMC host or the FIQ debugger, so it needs a
  fragment.
- **CMA:** `rk3528-android.dtsi`, and therefore the factory DTB, declares `cma { size = 8 MiB;
  linux,cma-default; }`, which takes precedence over `CONFIG_CMA_SIZE_MBYTES`. The 64 MB
  setting does nothing; `cma=64M` on the cmdline is what overrides it. `vop_mmu` is enabled,
  so scanout buffers may not need CMA at all.
- **The SoC is 64-bit; the firmware is 32-bit.** "32-bit only" (§2, §14) describes the
  Android build, not the A53 cores. BL31 is AArch64; ~~u-boot is AArch64~~ **[corrected §22]**
  the factory u-boot is AArch32 (Thumb-2) with no execution-state switch.
- **§13's mainline status** (no RK3528 VOP/HDMI, no RK3528 in `rkvdec`) was a 2026-11-19
  snapshot and is fast-moving; a 2026-11-21 web search could neither confirm nor refute it.
  It does not affect the plan, which uses the vendor BSP.

## 22. arm64 decided — and the stock u-boot cannot start an arm64 kernel (2026-11-21)

Decision (user): **the kernel is arm64.** Read offline from
`recon/20261119-1645/partitions/uboot.img` (FIT header at 0, u-boot data at 0x1200):

- **The stock u-boot is 32-bit.** FIT `uboot { arch = "arm"; load = <0x200000>; }`, while
  `atf-*` are `arch = "arm64"`. The code starts `ea0000b8` + `e59ff014` (an A32 vector
  table), the body is Thumb-2, and it prints `CPU: AArch32`. ATF therefore launched BL33 in
  AArch32. Its env (`kernel_addr_r=0x00208000`, `kernel_addr_c=0x02008000`) follows 32-bit
  conventions, and it has `bootz` but **no `booti`** (no "ARM64 Image magic" / "Moving Image"
  strings).
- **It has no way back to AArch64.** From AArch32 non-secure the CPU can only reach AArch64
  through EL3. The complete set of SMC function IDs this u-boot issues is:
  - `0x82000009` (Rockchip SIP share-mem), `0x82000025` (Rockchip SIP), `0x84000008`/`0x84000009`
    (PSCI SYSTEM_OFF/RESET), and SCMI-over-SMC (ID from DT).
  - Found by: every literal-pool word, every Thumb `movw/movt` pair, every caller of the two
    SMC trampolines (`0x2004f8` A32, `0x2036f8`), and the ops-table pointer to the generic
    wrapper.
  - **There is no PSCI `CPU_ON` and no execution-state-switch SiP (`0x82000020`).**
  - So an arm64 `Image`/`Image.gz` in `boot` would be entered in A32 state and fault on its
    first instruction, leaving the u-boot logo on screen and no fallback (`boot_android`
    "succeeds" at jumping).
- **Consequence: a 64-bit kernel needs a 64-bit BL33.** The `uboot` partition (0x4000, 4 MiB)
  has to carry an AArch64 u-boot FIT. The factory SPL picks the BL33 execution state from
  the FIT's `arch`, which is how Rockchip's arm64 products boot with the same SPL. This
  reverses §16's "do not replace the bootloader". The `boot`-partition image work still
  applies to whatever the new u-boot loads.
- **Recovery net, from the factory SPL in `pi-head.bin`** (sector 64, which stays untouched):
  it has Rockchip's back-to-BootROM path (`# Reset the board to bootrom #`,
  `back_to_bootrom()`, `ctrl+b: Bootrom download!`). A u-boot FIT that **fails to load or
  verify** drops the box into maskrom by itself, so USB recovery remains. A FIT that loads
  but whose u-boot **hangs** has no such net, and the held-button path lives in u-boot
  itself; the remaining route would be the eMMC-CLK short.
- **Secure boot looks off.** The factory FIT `conf` has `signature { algo = "sha256,rsa2048";
  key-name-hint = "dev"; }` with **no `value`**, and the SPL boots it. So signatures are not
  enforced. That answers the long-open RSM/secure-boot question by inference; it was not
  measured directly.
- **Kernel side for arm64**, all in `build-kernel.sh`:
  - the `KERNEL_GZIP/LZ4` lines are gone (no-ops on arm64);
  - `RELOCATABLE=y` is asserted, so `head.S` absorbs a non-2 MiB-aligned load address;
  - the raw `Image` header is checked (magic `ARM\x64`, text_offset 0, flags `0xa` = LE, 4K
    pages, may be placed anywhere);
  - the console chain from §21 is asserted;
  - the CMA comment is corrected.
  - Board drivers were compared against the vendor config: nothing the board needs is missing.
    (`ROCKCHIP_CLK_PVTM/INV` depend only on RK3368/RK3288 clocks and were not added; the
    real `ROCKCHIP_PVTM`, `ROCKCHIP_OPP` and `ROCKCHIP_SYSTEM_MONITOR` are already on.)
- **Rebuilt 2026-11-21 in the fedora:41 container (gcc 14.2.1 cross), 186 s.** The 2026-11-20
  host artifacts were being served from cache (`make-linux-boot.sh` skips stage 3 when
  `build/kernel/kernel` exists, and that kernel embedded the old `/init`). They were moved to
  `build/kernel/prev-20261120/`. New: `Image` 40,444,416 B (`4988454b…`), `Image.gz`
  15,080,024 B (`eb5868de…`). Verified: the embedded initramfs is byte-identical to
  `initramfs.cpio.gz` (probe-2, Tux, mounts before redirect); `linux-boot.img`
  (sha256 `630bd3bd…`) carries it with
  `fbdev_emulation=1` in both dtb copies. **Not flashable as a working boot yet**: the
  stock u-boot cannot start it (above).

## 23. Our 64-bit u-boot, built and its boot script executed (2026-11-21)

**Artifact:** `build/uboot/u-boot.itb`, 801,792 B, sha256 `238c31af…c588`. u-boot v2026.07,
AArch64 at 0x00800000, BL31 = rkbin `rk3528_bl31_v1.21.elf` (atf-1/2/3), no OP-TEE, our DT.
Built by `tools/build-uboot.sh` in the fedora:41 container (needs `openssl-devel-engine` on
F41; the old `checkarmreloc` failure did not recur in a clean out-of-tree build). The
`idbloader.img` / `u-boot-rockchip.bin` binman also produces are **deleted** by the script:
sector 64 stays factory.

**Inputs, all in `tools/uboot/`:**
- `rk3518-h96max.dts`: `rk3528-generic.dts` (the base of the community RK3518 itb) plus the
  SARADC. The factory u-boot's *own* DTB (FIT image 5 of the factory `uboot.img`) has
  `adc-keys { io-channels = <&saradc 1>; keyup 1.8 V; press ~0 V }`, and mainline's
  `rockchip_dnl_key_pressed()` reads exactly saradc channel 1, raw ≤ 30 → BootROM download.
  So **the factory recovery button keeps working** with our u-boot.
- `0001-…reboot-loader-enters-maskrom.patch`: the vendor kernel's reboot-mode writes
  `0x5242c301` (loader) to 0xff370200 = mainline's `ROCKCHIP_BOOT_MODE_REG`; mainline
  ignored it. Now `reboot loader` from Linux → BootROM download mode.
- `bootcmd.hush` → `CONFIG_BOOTCOMMAND`. The `boot` partition owns the kernel, ramdisk, DTB
  **and** the cmdline; u-boot never needs reflashing to change them. Reads the GPT `boot`
  partition (64 MiB to 0x0a000000), `abootimg` for the DTB and ramdisk (in place,
  `ANDROID_BOOT_IMAGE_IGNORE_BLOB_ADDR`), `unzip` the Image.gz at +0x800 to `kernel_addr_r`
  0x02000000, bootargs = DTB `/chosen/bootargs` **then** the header cmdline (so the header's
  `console=tty1` is last and wins, the reverse of the factory u-boot), `booti`. Any failure
  before the jump → write `0xef08a53c` to 0xff370200 and reset = **automatic maskrom**.
- `h96max.config`: every line verified after `olddefconfig`; `ENV_IS_NOWHERE` asserted
  (u-boot can never `saveenv` onto the eMMC).

**Executed, not just read:** `tools/uboot/test-bootcmd-sandbox.sh` runs the same script in
u-boot's sandbox against a disk with the box's real GPT and our image at 0xc800
(`build/uboot/sandbox-linux-boot.log`): ramdisk at +0xe62800 size 1,019,385, DTB at
+0x1568800 size 98,687, Image unzipped to 40,444,416 B with `ARMd` at +0x38, bootargs as
designed. With the factory `boot.img` instead (`sandbox-factory-boot.log`): `Bad gzipped
data` → maskrom path. What sandbox cannot test: the jump itself, the arm64 header checks in
`booti_setup`, eMMC/SARADC drivers on the real SoC.

**Ways back to maskrom with this u-boot:** (1) bad/foreign `boot` → automatic; (2) recovery
button at power-on; (3) `reboot loader` from Linux; (4) FIT fails to load → factory SPL
falls back to BootROM. Remaining eMMC-short cases: u-boot loads then hangs before
`board_late_init`, or the kernel hangs *and* the button read fails.

**Not verified (precedent only):** that the factory SPL loads a mainline FIT with u-boot at
0x00800000 and that mainline sizes DRAM from its ATAGs. The Hugsun X88PRO (§10) runs this
exact combination (factory idbloader + mainline 2026.07 itb + BL31 v1.21). The vendor
kernel DT's OP-TEE node will fail to probe without BL32 (expected harmless; Linux doesn't
need it here).

**Write order:** `uboot` and `boot` in the **same** maskrom session. New u-boot + factory
`boot` lands in maskrom by design; stock u-boot + our `boot` would run an arm64 kernel in
AArch32 and hang.

**Addendum 2026-11-21 (after first boots):** the factory SPL does load the mainline FIT and
DRAM sizing works (Linux sees 0x7fe00000 = 2 GiB minus u-boot's 2 MiB ATF hole). The button
did not work at first: not the channel (Linux measured ch1, ~1019 released / ~10 held) but a
name match, since mainline looks for an ADC device called `saradc*` and the upstream
rk3528.dtsi calls it `adc@ffae0000`. Patch 0002 fixes that and makes u-boot report
`h96.adc` / `h96.bootmode` on the kernel cmdline.
