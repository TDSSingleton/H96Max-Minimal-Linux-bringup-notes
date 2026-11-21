#!/usr/bin/env bash
# Build a minimal bootable Linux for the RK3518 stick from Rockchip's vendor 6.1 BSP.
#
# WHY THE VENDOR TREE AND NOT MAINLINE: mainline RK3528 has no VOP, no dw-hdmi and no
# display-subsystem node at all -- a mainline build gives NOTHING on the TV. On a stick
# with no reachable UART, HDMI is our only console, so the display stack is not a
# luxury here, it is the only way to see anything. Codecs/MPP come along for free.
#
# FIRST MILESTONE IS DELIBERATELY SMALL: kernel + DTB + an embedded initramfs that
# boots, prints to HDMI, and writes a full hardware report onto a USB stick (or, if
# there is no stick, into the eMMC 'cache' partition for us to read back over
# RockUSB). Nothing is installed, nothing is flashed, nothing is made permanent.
set -uo pipefail

# ROOT is overridable for container builds:  ROOT=/work bash tools/build-kernel.sh
ROOT=${ROOT:-$(cd "$(dirname "$0")/.." && pwd)}
SRC=$ROOT/downloads/kernel-rockchip
OUT=$ROOT/build/kernel
JOBS=$(nproc)
BRANCH=${BRANCH:-develop-6.1}

command -v aarch64-linux-gnu-gcc >/dev/null || { echo "!! missing aarch64-linux-gnu-gcc"; exit 1; }
mkdir -p "$OUT"

echo "===== 0. wait for the clone if it is still running ====="
if [ ! -f "$SRC/Makefile" ]; then
  echo "kernel tree not ready yet -- waiting on the background clone (build/kernel-clone.log)"
  for _ in $(seq 1 120); do [ -f "$SRC/Makefile" ] && break; sleep 15; done
  [ -f "$SRC/Makefile" ] || { echo "!! clone still not finished; re-run when it is"; exit 1; }
fi
cd "$SRC" || exit 1
echo "  $(git log --oneline -1)  branch=$(git rev-parse --abbrev-ref HEAD)"
echo "  kernel: $(head -4 Makefile | tr '\n' ' ')"

DTB_DTS=arch/arm64/boot/dts/rockchip/rk3518-evb1-ddr4-v10.dts
[ -f "$DTB_DTS" ] || { echo "!! $DTB_DTS missing -- this branch may have moved; check develop-6.1"; exit 1; }
echo "  board DTS present: $DTB_DTS"

echo; echo "===== 1. baseline = the vendor Linux defconfig ====="
make ARCH=arm64 CROSS_COMPILE=aarch64-linux-gnu- rockchip_linux_defconfig

echo; echo "===== 2. minimal + observability fragment ====="
cfg() { ./scripts/config "$@"; }
# --- display: this IS the console on a UART-less stick ---
cfg --enable DRM
cfg --enable DRM_ROCKCHIP
cfg --enable DRM_FBDEV_EMULATION
cfg --enable TTY
cfg --enable VT              # the vendor's own 6.1.118 config has VT OFF (NOTES s20):
cfg --enable VT_CONSOLE      # without these two FRAMEBUFFER_CONSOLE silently drops out
cfg --enable FB
cfg --enable FB_CORE
cfg --enable FRAMEBUFFER_CONSOLE
cfg --enable FRAMEBUFFER_CONSOLE_DETECT_PRIMARY
cfg --disable LOGO
cfg --disable LOGO_LINUX_STROBED
# --- CMA: NOTE the factory DTB declares `cma { size = 8 MiB; linux,cma-default }`, which
#     overrides CONFIG_CMA_SIZE_MBYTES (NOTES s18) -- this value only applies if a DTB without
#     that node is used. Scanout goes through vop_mmu (IOMMU), so CMA is not expected to be
#     the limit; to override anyway, add cma=64M to BOOTARGS_ADD.
cfg --set-val CMA_SIZE_MBYTES 64
# --- payload FORM (arm64): there is no self-decompressing arm64 kernel and no
#     KERNEL_GZIP/LZ4 choice on arm64 (only arch/arm selects HAVE_KERNEL_*). `make Image.gz`
#     is just gzip(Image); whoever loads it must decompress it. The factory payload is a
#     32-bit zImage, so neither arm64 form is "the factory's form" (NOTES s18/s22).
# --- placement: RELOCATABLE lets head.S absorb a load address that is not 2 MiB aligned
#     (__primary_switch: x23 = KERNEL_START & (MIN_KIMG_ALIGN-1)); asserted below.
cfg --enable RELOCATABLE
# (USB gadget/NCM debug link: tried 2026-11-21 and removed -- the PC's USB-C port cannot power
#  a full boot, so the box browns out; see research/notes §25.)
# --- USB-Ethernet dongles on the USB-A port (the box has no RJ45). r8152 was already =y;
#     add the usbnet framework and the common chips: ASIX AX88179/178A (gigabit) and AX8817x,
#     CDC-Ethernet/NCM/EEM, RNDIS host, SMSC95xx. Built in: no module loading needed.
# --- Bluetooth (the SeekWave SWT6621S is a WiFi+BT combo on SDIO; the stick's remote is BT).
#     Core + classic + LE built in; HIDP for classic HID remotes (LE remotes use UHID, =y).
#     The chip's HCI driver (skwbt) is an external module, like the WiFi one.
for s in BT BT_BREDR BT_LE BT_HIDP; do cfg --enable $s; done
for s in USB_USBNET USB_NET_AX88179_178A USB_NET_AX8817X USB_NET_CDCETHER USB_NET_CDC_NCM \
         USB_NET_CDC_EEM USB_NET_RNDIS_HOST USB_NET_SMSC95XX; do cfg --enable $s; done
# --- bring the initramfs INTO the kernel: fewer files for u-boot to load, and it
#     removes the initrd-address class of failure entirely for milestone 1 ---
cfg --enable BLK_DEV_INITRD
[ -f "$OUT/initramfs.cpio.gz" ] && cfg --set-str INITRAMFS_SOURCE "$OUT/initramfs.cpio.gz" \
                               || echo "   (no initramfs yet -- run tools/mk-initramfs.sh, then re-run this script)"
# --- without these, /dev is EMPTY and the whole probe reads nothing ---
cfg --enable DEVTMPFS
cfg --enable DEVTMPFS_MOUNT
cfg --enable TMPFS
cfg --enable DEBUG_FS
cfg --enable SYSFS
cfg --enable PROC_FS
cfg --enable INPUT
cfg --enable INPUT_EVDEV
# --- storage + the report channels ---
cfg --enable FAT_FS
cfg --enable NLS_CODEPAGE_437
cfg --enable MMC_BLOCK
cfg --enable EXT4_FS
cfg --enable VFAT_FS
cfg --enable FAT_DEFAULT_IOCHARSET --set-str FAT_DEFAULT_IOCHARSET utf8
cfg --enable NLS_UTF8
cfg --enable USB_STORAGE
cfg --enable USB_EHCI_HCD
cfg --enable USB_XHCI_HCD
# --- blind-boot telemetry: if the box dies before HDMI comes up, pstore leaves the
#     console log on a block device we can read back over RockUSB. Experimental: it
#     needs a matching DT/chosen or pstore_blk.blkdev= on the command line.
cfg --enable PSTORE
cfg --enable PSTORE_CONSOLE
cfg --enable PSTORE_RAM
cfg --enable PSTORE_BLK
cfg --enable PRINTK
cfg --enable KALLSYMS
# --- trim: things we do not need for milestone 1 ---
# (WiFi + Bluetooth are wanted since 2026-11-21: CFG80211, MAC80211, BT, WIRELESS no longer
#  dropped here -- this list had been silently undoing the BT enables above.)
for drop in CONFIG_ANDROID_BINDER_IPC CONFIG_ASHMEM CONFIG_MEMCG_KMEM \
            CONFIG_WP_WL2551_1X1 CONFIG_RTL8822CS CONFIG_AP6XXX \
            CONFIG_WAN CONFIG_INFINIBAND \
            CONFIG_DRM_I2C_ADV7511 CONFIG_SND_SOC_ROCKCHIP_MAX98090; do
  ./scripts/config --disable "${drop#CONFIG_}" 2>/dev/null || true
done
make ARCH=arm64 CROSS_COMPILE=aarch64-linux-gnu- olddefconfig </dev/null
echo "  pre-flight: any config prompt still pending?"
if ! make ARCH=arm64 syncconfig </dev/null >/tmp/syncprobe.log 2>&1; then
  echo "    !! syncconfig failed or wanted input -- a symbol we enabled opened a question:"
  tail -25 /tmp/syncprobe.log | sed 's/^/      /'
  echo "    Drop the offending --enable line; prompts are fatal on a console-less box."
  exit 1
fi
echo "    clean"

echo; echo "===== 3. what we actually ended up with ====="
grep -E '^CONFIG_(KERNEL_GZIP|KERNEL_LZ4|KERNEL_UNCOMPRESSED|DRM|DRM_ROCKCHIP|DRM_FBDEV_EMULATION|FRAMEBUFFER_CONSOLE|BLK_DEV_INITRD|INITRAMFS_SOURCE|MMC_BLOCK|EXT4_FS|VFAT_FS|USB_STORAGE|PSTORE|PSTORE_BLK|VT)\=' .config | sed 's/^/  /'
for sym in RELOCATABLE \
           USB_RTL8152 USB_USBNET USB_NET_AX88179_178A USB_NET_AX8817X USB_NET_CDCETHER USB_NET_CDC_NCM \
           BT BT_BREDR BT_LE BT_HIDP UHID; do
  grep -qx "CONFIG_$sym=y" .config || { echo "  !! CONFIG_$sym is not =y"; exit 1; }
done
# the HDMI console chain is the only output we have: assert it, do not just request it
for sym in VT VT_CONSOLE DRM_FBDEV_EMULATION FRAMEBUFFER_CONSOLE DRM_ROCKCHIP; do
  grep -qx "CONFIG_$sym=y" .config || { echo "  !! CONFIG_$sym is not =y -- no text on HDMI; refusing to build"; exit 1; }
done
echo "  HDMI console chain: VT VT_CONSOLE DRM_FBDEV_EMULATION FRAMEBUFFER_CONSOLE DRM_ROCKCHIP all =y"
echo "  UART console driver: $(grep -E '^CONFIG_SERIAL_8250' .config | head -2 | tr '\n' ' ')"
echo "  (VOP/HDMI driver names actually built:) "
grep -E '^CONFIG_(ROCKCHIP_VOP|ROCKCHIP_VOP2|ROCKCHIP_DW_HDMI|DRM_ROCKCHIP_IT66121|DRM_ROCKCHIP_LVDS|PHY_ROCKCHIP_DP|USB_DWC3|USB_DWC2|PHY_ROCKCHIP_USB|DMA_ENGINE|ROCKCHIP_SRAM)' .config | sed 's/^/    /'

echo; echo "===== 4. build Image + dtbs (modules deferred to milestone 2) ====="
# Stream, don't buffer: `| tail -25` would hide 15 minutes of progress and make a
# stall indistinguishable from normal work. Stdin closed so a prompt aborts.
/usr/bin/time -f "  wall %e s  maxrss %MkB" \
  make -j"$JOBS" ARCH=arm64 CROSS_COMPILE=aarch64-linux-gnu- Image Image.gz dtbs </dev/null 2>&1 | tee "$OUT/build.log"
rc=${PIPESTATUS[0]}
[ "$rc" = 0 ] || { echo "!! make exited $rc -- see $OUT/build.log"; exit 1; }

echo; echo "===== 5. artifacts ====="
K=$(grep -E '^VERSION|^PATCHLEVEL|^SUBLEVEL' Makefile | tr -d ' ' | sed 's/.*=//' | paste -sd. -)
echo "  version string: $K"
ls -l arch/arm64/boot/Image arch/arm64/boot/Image.gz \
      arch/arm64/boot/dts/rockchip/rk3518-evb1-ddr4-v10.dtb 2>/dev/null | awk '{printf "  %-70s %9d\n", $9, $5}'
cp -f arch/arm64/boot/Image     "$OUT/Image"        2>/dev/null
cp -f arch/arm64/boot/Image.gz  "$OUT/kernel"       2>/dev/null
cp -f arch/arm64/boot/dts/rockchip/rk3518-evb1-ddr4-v10.dtb "$OUT/rk3518.dtb" 2>/dev/null
( cd "$OUT" && sha256sum kernel Image rk3518.dtb > SHA256SUMS.txt 2>/dev/null && cat SHA256SUMS.txt )

# $OUT/kernel = Image.gz (gzip of the arm64 Image); $OUT/Image = the raw Image.
# Which one the stock u-boot's boot_android can start is decided in NOTES s22.
k=$(file -b "$OUT/kernel" 2>/dev/null); echo "  kernel payload (Image.gz): $k"
python3 -I - "$OUT/Image" <<'PY'
import struct,sys
h=open(sys.argv[1],'rb').read(64)
to,isz,fl=struct.unpack_from('<QQQ',h,8)
ok = h[0x38:0x3c]==b'ARM\x64'
print(f"  raw Image header: magic {'ARM64 ok' if ok else 'BAD'}  text_offset {to:#x}  image_size {isz}  flags {fl:#x}")
sys.exit(0 if ok else 1)
PY
[ $? = 0 ] || { echo "  !! $OUT/Image has no arm64 Image magic"; exit 1; }
echo
echo "  assemble the boot-partition image with:"
echo "    python3 tools/mk-bootimg.py --kernel build/kernel/kernel \\"
echo "      --ramdisk build/kernel/initramfs.cpio.gz --out build/out/linux-boot.img"
echo
echo "  NOTE on vermagic: this tree is 6.1.x. The stock modules were 6.1.118 with"
echo "  modversions, so the ripped skw*.ko will NOT load unless we build exactly"
echo "  6.1.118 -- check:  grep -E '^(VERSION|PATCHLEVEL|SUBLEVEL)' Makefile"
