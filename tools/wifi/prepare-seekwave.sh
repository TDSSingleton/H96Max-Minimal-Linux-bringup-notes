#!/usr/bin/env bash
# Recreate our SeekWave SWT6621S port (tools/wifi/seekwave/) from Neardi's original sources.
#
# The driver code is Neardi's (submodule research/neardi-3588-sdk-linux); only our changes are
# kept in this repo, as tools/wifi/seekwave-h96max.patch. This script:
#   1. copies the original driver directories out of the submodule into the port's layout
#   2. applies our patch (H96Max changes: SDIO id, DT compatible, rx descriptor, BT lite NV, Kbuild)
#   3. installs the Bluetooth NV files from the same submodule:
#        skwbt/sv6316.nvbin -> tools/wifi/seekwave/skwbt/sv6160lite.nvbin
#                              tools/rootfs/payload/lib/firmware/sv6160lite.nvbin
#        skwbt/sv6160.nvbin -> tools/rootfs/payload/lib/firmware/sv6160.nvbin
# Safe to re-run: an already-prepared tree is left alone. --force rebuilds it (drops build products).
#
# Needs: the Neardi submodule (git submodule update --init --depth 1 research/neardi-3588-sdk-linux),
# patch, diffutils. Called automatically by build-seekwave.sh.
set -euo pipefail
ROOT=${ROOT:-$(cd "$(dirname "$0")/../.." && pwd)}
K=$ROOT/research/neardi-3588-sdk-linux/kernel
S=$ROOT/tools/wifi/seekwave
PATCH=$ROOT/tools/wifi/seekwave-h96max.patch
FW=$ROOT/tools/rootfs/payload/lib/firmware
die() { echo "!! $*" >&2; exit 1; }

[ -d "$K/drivers/misc/seekwaveplatform_v20" ] || \
  die "Neardi sources missing: git submodule update --init --depth 1 research/neardi-3588-sdk-linux"
command -v patch >/dev/null || die "missing: patch"

install -D -m 0644 "$K/drivers/bluetooth/skwbt/sv6160.nvbin" "$FW/sv6160.nvbin"
install -D -m 0644 "$K/drivers/bluetooth/skwbt/sv6316.nvbin" "$FW/sv6160lite.nvbin"

if [ -d "$S" ] && [ "${1:-}" != --force ]; then
  if (cd "$S" && patch -p1 -R --dry-run -s -f <"$PATCH" >/dev/null 2>&1); then
    echo "  already prepared: $S"; exit 0
  fi
  die "$S exists but is not the patched Neardi tree; move it aside or re-run with --force"
fi

T=$(mktemp -d "$ROOT/tools/wifi/.seekwave.XXXXXX"); trap 'rm -rf "$T"' EXIT
mkdir -p "$T/platform" "$T/include/linux/platform_data"
cp -a "$K/drivers/misc/seekwaveplatform_v20/sdio" "$K/drivers/misc/seekwaveplatform_v20/skwutil" "$T/platform/"
cp -a "$K/drivers/misc/seekwaveplatform_v20/Makefile" "$T/platform/Makefile.neardi"
cp -a "$K/drivers/bluetooth/skwbt" "$K/drivers/net/wireless/rockchip_wlan/skwifi" "$T/"
cp -a "$K/include/linux/platform_data/skw6316_config.h" \
      "$K/include/linux/platform_data/skw_platform_data.h" "$T/include/linux/platform_data/"

(cd "$T" && patch -p1 -E -s <"$PATCH") || die "patch failed: $PATCH"
install -m 0644 "$K/drivers/bluetooth/skwbt/sv6316.nvbin" "$T/skwbt/sv6160lite.nvbin"

rm -rf "$S"; mv "$T" "$S"; trap - EXIT
echo "  prepared: $S (Neardi sources + $(basename "$PATCH"))"
