#!/usr/bin/env bash
# Build the SeekWave SWT6621S modules (tools/wifi/seekwave, ported from Neardi's SDK)
# against our vendor 6.1.172 kernel tree. Container only:
#   podman run --rm -v ~/Sources/ClaudePlayground/H96Max:/work:z fedora:41 bash -c \
#     'dnf -y -q install gcc-aarch64-linux-gnu binutils-aarch64-linux-gnu gcc make bison flex bc perl openssl-devel elfutils-libelf-devel patch diffutils >/dev/null; ROOT=/work bash /work/tools/wifi/build-seekwave.sh'
set -uo pipefail
ROOT=${ROOT:-$(cd "$(dirname "$0")/../.." && pwd)}
K=$ROOT/downloads/kernel-rockchip
S=$ROOT/tools/wifi/seekwave
# the port is Neardi's sources + our patch; (re)create it if needed
bash "$ROOT/tools/wifi/prepare-seekwave.sh" || exit 1
OUT=$ROOT/build/wifi
mkdir -p "$OUT"
export ARCH=arm64 CROSS_COMPILE=aarch64-linux-gnu-
# modpost wants the kernel's exported symbols; this tree was built with `make Image`,
# which leaves them in vmlinux.symvers
[ -f "$K/Module.symvers" ] || cp "$K/vmlinux.symvers" "$K/Module.symvers"
# ...and scripts/module.lds, which only `modules_prepare` generates (does not touch vmlinux/Image)
[ -f "$K/scripts/module.lds" ] || make -C "$K" -s modules_prepare || { echo "!! modules_prepare failed"; exit 1; }
rc=0
make -C "$K" M="$S/platform" -j"$(nproc)" modules 2>&1 | tee "$OUT/platform.log" | grep -E 'error|warning: implicit|Error|WARNING' | head -40
[ "${PIPESTATUS[0]}" = 0 ] || rc=1
make -C "$K" M="$S/skwifi" -j"$(nproc)" KBUILD_EXTRA_SYMBOLS="$S/platform/Module.symvers" modules 2>&1 \
  | tee "$OUT/skwifi.log" | grep -E 'error|warning: implicit|Error|WARNING' | head -60
[ "${PIPESTATUS[0]}" = 0 ] || rc=1
make -C "$K" M="$S/skwbt" -j"$(nproc)" KBUILD_EXTRA_SYMBOLS="$S/platform/Module.symvers" modules 2>&1 \
  | tee "$OUT/skwbt.log" | grep -E 'error|warning: implicit|Error|WARNING' | head -40
[ "${PIPESTATUS[0]}" = 0 ] || rc=1
ls -la "$S"/platform/*.ko "$S"/skwifi/*.ko "$S"/skwbt/*.ko 2>/dev/null
exit $rc
