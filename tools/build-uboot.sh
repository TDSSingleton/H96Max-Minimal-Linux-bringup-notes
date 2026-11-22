#!/usr/bin/env bash
# Build a 64-bit mainline u-boot FIT (u-boot.itb) for the H96Max `uboot` partition.
#
# WHY: the kernel is arm64 (user decision 2026-11-21) and the factory u-boot is AArch32
# with no execution-state switch, so it cannot start it (research/notes §22).
#
# WHAT IS BUILT: u-boot v2026.07, generic-rk3528 base + tools/uboot/:
#   rk3518-h96max.dts         generic eMMC DT + SARADC (factory recovery button, ch1)
#   0001-*.patch              Linux `reboot loader` -> BootROM download mode
#   0002-*.patch              find the upstream `adc@` node; env breadcrumbs for Linux
#   0003-*.patch              pylibfdt Py2->Py3 shims (swig >= 4 dropped pycompat.i)
#   h96max.config             config fragment, every line verified after olddefconfig
#   bootcmd.hush              boot `boot` (AOSP v2) via abootimg/unzip/booti; on failure
#                             -> BootROM download mode
# BL31 = rkbin rk3528_bl31_v1.21.elf (RK3518 needs >= v1.20). No OP-TEE.
#
# WHAT IS NOT: SPL/TPL/idbloader. The factory idbloader at sector 64 holds the only
# board-tuned DDR init and stays untouched forever. binman insists on a TPL blob to
# assemble idbloader.img, so one is supplied and the resulting idbloader.img /
# u-boot-rockchip.bin are DELETED after the build so they cannot be written by mistake.
#
# Run inside the fedora:41 container, project mounted at /work with :z:
#   podman run --rm -v "$PWD":/work:z fedora:41 \
#     bash -c 'dnf -y -q install <see NEED below> && ROOT=/work bash /work/tools/build-uboot.sh'
# Touches no device. Prints the write commands; does not run them.
set -uo pipefail

ROOT=${ROOT:-$(cd "$(dirname "$0")/.." && pwd)}
UB=$ROOT/downloads/u-boot
RKBIN=$ROOT/downloads/rkbin/bin/rk35
MINE=$ROOT/tools/uboot
OUT=$ROOT/build/uboot-out            # out-of-tree build dir (disposable)
ART=$ROOT/build/uboot                # artifacts we keep
LOG=$ART/build.log
BL31=$RKBIN/rk3528_bl31_v1.21.elf
TPL=$RKBIN/rk3528_ddr_1056MHz_v1.14.bin   # binman fodder only; its output is deleted
export ARCH=arm CROSS_COMPILE=aarch64-linux-gnu-   # u-boot: 64-bit lives in arch/arm
mkdir -p "$ART"

die() { echo "!! $*"; exit 1; }
step() { echo; echo "=================== $* ==================="; }

step "0. environment"
for t in make gcc aarch64-linux-gnu-gcc bison flex bc swig dtc python3 patch sha256sum; do
  command -v $t >/dev/null || die "missing: $t   (NEED: dnf install gcc-aarch64-linux-gnu \
binutils-aarch64-linux-gnu gcc make bison flex bc swig dtc python3-devel python3-pyelftools \
python3-setuptools openssl-devel openssl-devel-engine gnutls-devel patch git diffutils)"
done
python3 -c 'import elftools' 2>/dev/null || die "missing python3-pyelftools"
echo "  toolchain: $(aarch64-linux-gnu-gcc --version | head -1)"
echo "  u-boot   : $(sed -n 's/^VERSION = //p;s/^PATCHLEVEL = //p' $UB/Makefile | paste -sd.)"
for f in "$BL31" "$TPL" "$MINE/rk3518-h96max.dts" "$MINE/h96max.config" "$MINE/bootcmd.hush"; do
  [ -f "$f" ] || die "missing $f"; done
echo "  BL31     : $(basename $BL31)  sha256 $(sha256sum $BL31 | cut -c1-16)…"
strings "$BL31" | grep -m1 -E 'v1\.2[0-9]' | sed 's/^/             /'

step "1. our board files into the tree (idempotent)"
cp "$MINE/rk3518-h96max.dts" "$MINE/rk3518-h96max-u-boot.dtsi" "$UB/arch/arm/dts/"
for p in "$MINE"/0*.patch; do
  if patch -d "$UB" -p1 -R --dry-run -s <"$p" >/dev/null 2>&1; then echo "  already applied: $(basename $p)"
  else patch -d "$UB" -p1 -s <"$p" || die "patch failed: $p"; echo "  applied: $(basename $p)"; fi
done
# Out-of-tree builds refuse a source tree that holds an old in-tree build.
if [ -f "$UB/.config" ] || [ -f "$UB/include/config/auto.conf" ]; then
  echo "  source tree has an old in-tree build -> make mrproper (build outputs only)"
  make -C "$UB" -s mrproper </dev/null || die "mrproper failed"
fi

step "2. configure"
rm -rf "$OUT"; mkdir -p "$OUT"
make -C "$UB" O="$OUT" -s generic-rk3528_defconfig </dev/null || die defconfig
grep -vE '^\s*(#|$)' "$MINE/h96max.config" >>"$OUT/.config"
grep -E '^# CONFIG_.* is not set' "$MINE/h96max.config" >>"$OUT/.config"
BOOTCMD=$(grep -vE '^\s*(#|$)' "$MINE/bootcmd.hush" | paste -sd';' | sed 's/;/; /g')
case "$BOOTCMD" in *'"'*) die 'bootcmd.hush must not contain double quotes';; esac
echo "CONFIG_BOOTCOMMAND=\"$BOOTCMD\"" >>"$OUT/.config"
make -C "$UB" O="$OUT" -s olddefconfig </dev/null || die olddefconfig
bad=0
while read -r line; do
  case "$line" in
    '# CONFIG_'*' is not set') s=${line#\# }; s=${s% is not set}
       grep -q "^$s=" "$OUT/.config" && { echo "  !! still on : $s"; bad=1; } ;;
    CONFIG_*) grep -qxF "$line" "$OUT/.config" || { echo "  !! not taken: $line  -> got: $(grep "^${line%%=*}=" "$OUT/.config" || echo unset)"; bad=1; } ;;
  esac
done < <(grep -E '^(CONFIG_|# CONFIG_.* is not set)' "$MINE/h96max.config")
grep -qF "CONFIG_BOOTCOMMAND=\"$BOOTCMD\"" "$OUT/.config" || { echo "  !! BOOTCOMMAND not taken"; bad=1; }
[ $bad = 0 ] || die "config fragment did not fully apply"
echo "  all $(grep -cE '^(CONFIG_|# CONFIG_)' "$MINE/h96max.config") fragment lines + BOOTCOMMAND confirmed"
for k in ARM64 SYSRESET ROCKCHIP_BOOT_MODE_REG; do grep -E "^CONFIG_$k=" "$OUT/.config" | sed 's/^/  /'; done
grep -qE '^CONFIG_ENV_IS_IN_(MMC|FAT|EXT4|SPI_FLASH)=y' "$OUT/.config" && die "an env-on-storage option is on (saveenv could write the eMMC)"
cp "$OUT/.config" "$ART/u-boot.config"

step "3. build"
t0=$(date +%s)
make -C "$UB" O="$OUT" -j"$(nproc)" BL31="$BL31" ROCKCHIP_TPL="$TPL" </dev/null >"$LOG" 2>&1
rc=$?
tail -5 "$LOG" | sed 's/^/  /'
echo "  make rc=$rc  ($(( $(date +%s) - t0 )) s, log: build/uboot/build.log)"
[ -f "$OUT/u-boot.itb" ] || die "no u-boot.itb -- see $LOG ($(grep -m3 -E 'Error|error:|No rule' $LOG | tr '\n' ' '))"
[ $rc = 0 ] || echo "  (make rc!=0 but u-boot.itb exists -- the reason is printed above; judge it before trusting the file)"

step "4. remove what must never be written"
for f in idbloader.img u-boot-rockchip.bin u-boot-rockchip-spi.bin; do
  [ -f "$OUT/$f" ] && { rm -f "$OUT/$f"; echo "  deleted $f (would overwrite the factory DDR init at sector 64)"; }
done

step "5. verify the FIT"
ITB=$ART/u-boot.itb; cp "$OUT/u-boot.itb" "$ITB"
sz=$(stat -c %s "$ITB")
echo "  size $sz bytes (uboot partition = 4194304)"; [ "$sz" -le 4194304 ] || die "does not fit"
[ "$(od -An -tx1 -N4 "$ITB" | tr -d ' ')" = d00dfeed ] || die "not an FDT/FIT"
"$OUT/tools/dumpimage" -l "$ITB" >"$ART/dumpimage.txt" 2>&1
grep -E '^ Image|Architecture|Load Address|Compression' "$ART/dumpimage.txt" | sed 's/^/  /'
awk '/\(u-?boot\)/{f=1} f&&/Architecture/{print; exit}' "$ART/dumpimage.txt" | grep -q AArch64 \
  || die "the u-boot image in the FIT is not AArch64"
echo "  OK  u-boot image is AArch64"
grep -q 'bl31\|atf' "$ART/dumpimage.txt" && echo "  OK  BL31 present"
# the u-boot DTB that ended up in the FIT: our board, saradc on
idx=$(grep -E '^ Image [0-9]+ \(fdt' "$ART/dumpimage.txt" | head -1 | sed -E 's/ Image ([0-9]+).*/\1/')
"$OUT/tools/dumpimage" -T flat_dt -p "$idx" -o "$ART/u-boot-own.dtb" "$ITB" >/dev/null
dtc -I dtb -O dts "$ART/u-boot-own.dtb" 2>/dev/null >"$ART/u-boot-own.dts"
grep -q 'model = "H96Max RK3518"' "$ART/u-boot-own.dts" && echo "  OK  FIT carries our DT (model H96Max RK3518)" || die "FIT DT is not ours"
awk '/adc@ffae0000/{f=1} f&&/status/{print; exit}' "$ART/u-boot-own.dts" | grep -q okay && echo "  OK  saradc enabled" || die "saradc not enabled"
strings "$OUT/u-boot.bin" | grep -cF 'h96: no bootable kernel'>/dev/null && echo "  OK  bootcmd compiled in" || die "bootcmd missing from u-boot.bin"
strings "$OUT/u-boot.bin" | grep -cF 'reboot loader: entering BootROM'>/dev/null && echo "  OK  reboot-loader patch compiled in" || die "patch missing"
strings "$OUT/u-boot.bin" | grep -cxF 'h96_bootmode'>/dev/null && echo "  OK  breadcrumb patch compiled in" || die "patch 0002 missing"
( cd "$ART" && sha256sum u-boot.itb >SHA256SUMS.txt && cat SHA256SUMS.txt | sed 's/^/  /' )

step "6. the write -- NOT EXECUTED, your decision"
cat <<EOF
  target : uboot partition, LBA 0x4000, 0x2000 sectors (4 MiB). Sector 64 untouched.
  file   : build/uboot/u-boot.itb
  undo   : rkdeveloptool wl 0x4000 recon/20261119-1645/partitions/uboot.img  (= full.img @0x4000)

  Do it in the SAME maskrom session as the boot-partition write:
    - new u-boot + factory boot  -> unzip of the 32-bit zImage fails -> straight to maskrom
    - stock u-boot + linux boot  -> stock u-boot runs our arm64 kernel in AArch32 -> hang
      (stock button recovery still works there, but don't go there on purpose)

    rkdeveloptool wl 0x4000 build/uboot/u-boot.itb
    rkdeveloptool rl 0x4000 0x2000 /tmp/echo-uboot.bin
    cmp -n $sz build/uboot/u-boot.itb /tmp/echo-uboot.bin && echo UBOOT VERIFIED
    rkdeveloptool wl 0xc800 build/out/linux-boot.img
    rkdeveloptool rl 0xc800 0x20000 /tmp/echo-boot.img
    sha256sum /tmp/echo-boot.img build/out/linux-boot.img
    rkdeveloptool rd

  Ways back to maskrom with this u-boot: (1) any boot-partition failure -> automatic;
  (2) hold the recovery button at power-on (saradc ch1, as the factory u-boot);
  (3) from running Linux: reboot loader. If u-boot itself fails to load, the factory
  SPL falls back to the BootROM. Only a u-boot that loads and then hangs before reading
  the button needs the eMMC CLK short.
EOF
