#!/usr/bin/env bash
# Execute tools/uboot/bootcmd.hush for real -- in u-boot's sandbox (u-boot built as a
# host program) -- against a disk that has the box's real GPT and build/out/linux-boot.img
# in the `boot` partition. Everything up to the final jump runs: GPT lookup, the 64 MiB
# read, abootimg, unzip, setexpr.s on the header cmdline, fdt get, bootargs assembly.
# Then booti runs separately, which parses the arm64 Image header in sandbox as well.
#
# Substitutions (the only differences from the on-device script):
#   mmc dev 0 &&        -> removed            (sandbox: `host` device instead of mmc)
#   part ... mmc 0      -> part ... host 0
#   mmc read A B C      -> read host 0:0 A B C   (0:0 = whole disk, like mmc read)
#   booti ...           -> echo WOULD booti ...   (run for real afterwards, see below)
#   mw.l / reset        -> echo                   (never poke the sandbox "SoC")
#
# Container packages: gcc make bison flex bc swig dtc perl python3-devel python3-pyelftools
#   python3-setuptools openssl-devel openssl-devel-engine gnutls-devel libuuid-devel patch diffutils
# Run in the fedora:41 container:  ROOT=/work bash /work/tools/uboot/test-bootcmd-sandbox.sh
set -uo pipefail
ROOT=${ROOT:-$(cd "$(dirname "$0")/../.." && pwd)}
UB=$ROOT/downloads/u-boot
SB=$ROOT/build/uboot-sandbox
DISK=$SB/disk.img
IMG=${IMG:-$ROOT/build/out/linux-boot.img}
FULL=$ROOT/recon/live-full/full.img
mkdir -p "$SB"
die() { echo "!! $*"; exit 1; }

echo "== sandbox build"
if [ ! -x "$SB/u-boot" ] || [ ! -f "$SB/u-boot.dtb" ] || [ "$UB/cmd/abootimg.c" -nt "$SB/u-boot" ]; then
  make -C "$UB" O="$SB" -s sandbox_defconfig </dev/null || die defconfig
  cat >>"$SB/.config" <<'EOF'
CONFIG_ANDROID_BOOT_IMAGE=y
CONFIG_ANDROID_BOOT_IMAGE_IGNORE_BLOB_ADDR=y
CONFIG_CMD_ABOOTIMG=y
CONFIG_CMD_BOOTI=y
CONFIG_CMD_UNZIP=y
CONFIG_CMD_SETEXPR=y
CONFIG_CMD_READ=y
CONFIG_SANDBOX_RAM_SIZE_MB=1024
# CONFIG_VIDEO_SANDBOX_SDL is not set
# CONFIG_EFI_CAPSULE_AUTHENTICATE is not set
EOF
  make -C "$UB" O="$SB" -s olddefconfig </dev/null || die olddefconfig
  make -C "$UB" O="$SB" -j"$(nproc)" NO_SDL=1 u-boot u-boot.dtb </dev/null >"$SB/build.log" 2>&1 || { tail -20 "$SB/build.log"; die "sandbox build failed"; }
fi
for k in CMD_ABOOTIMG CMD_BOOTI CMD_UNZIP CMD_SETEXPR CMD_READ; do
  grep -q "^CONFIG_$k=y" "$SB/.config" || die "sandbox lacks $k"; done

echo "== test disk: real GPT (primary + backup) + $(basename "$IMG") at LBA 0xc800 (sparse)"
size=$(stat -c %s "$FULL")
rm -f "$DISK"; truncate -s "$size" "$DISK"
dd if="$FULL" of="$DISK" bs=512 count=34 conv=notrunc status=none
dd if="$FULL" of="$DISK" bs=512 skip=$((size/512-33)) seek=$((size/512-33)) count=33 conv=notrunc status=none
dd if="$IMG"  of="$DISK" bs=512 seek=$((0xc800)) conv=notrunc status=none

CMD=$(grep -vE '^\s*(#|$)' "$ROOT/tools/uboot/bootcmd.hush" | paste -sd';' | sed 's/;/; /g' \
  | sed -e 's/mmc dev 0 && //' -e 's/part start mmc 0/part start host 0/' -e 's/part size mmc 0/part size host 0/' \
        -e 's/mmc read /read host 0:0 /' -e 's/booti /echo WOULD booti /' \
        -e 's/mw\.l \([^;]*\)/echo WOULD mw.l \1/' -e 's/; reset$/; echo WOULD reset/')
echo "== running"
"$SB/u-boot" -D -c "host bind 0 $DISK; setenv kernel_addr_r 0x02000000; $CMD; \
printenv h96_ok h96_blk h96_cnt h96_dtb h96_dtbsz h96_rd h96_rdsz; \
echo -- arm64 Image magic at +0x38 of the unzipped kernel:; md.b 0x02000038 4; \
echo -- now booti for real, sandbox parses the Image header:; booti \${kernel_addr_r} \${h96_rd}:\${h96_rdsz} \${h96_dtb}" 2>&1 | tee "$SB/run.log"
