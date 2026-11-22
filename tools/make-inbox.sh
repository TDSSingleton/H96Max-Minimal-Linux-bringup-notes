#!/usr/bin/env bash
# Build the stick inbox: build/inbox/h96-inbox.tar.gz. Copy it to the USB stick's root; the
# initramfs unpacks it into the Alpine rootfs once (then renames it *.applied-<time>).
# Contents:
#   tools/rootfs/payload/ LAN dongle DHCP watcher (h96-lan), sshd key-only (key
#                         build/keys/h96max_ed25519), stick report, info screen
#   WiFi (not auto-loaded): skw_sdio_v20.ko + skw6316.ko, SWT6621S firmware, and the
#                         /usr/local/sbin/h96-wifi-test command
#   h96ui (Rust, build/h96ui/h96ui) and the fixup kit (tools/fixup/h96-doctor, h96-fix)
set -euo pipefail
ROOT=${ROOT:-$(cd "$(dirname "$0")/.." && pwd)}
cd "$ROOT"
S=tools/wifi/seekwave; FW=vendor/seekwave/firmware; OUT=build/inbox
KVER=$(cat downloads/kernel-rockchip/include/config/kernel.release)
mkdir -p $OUT
T=$(mktemp -d); trap 'rm -rf $T' EXIT
cp -a tools/rootfs/payload/. $T/
# h96ui: the Rust build (tools/h96ui-rs/build.sh), never a stale copy left in the payload dir
[ -f build/h96ui/h96ui ] || { echo "!! build/h96ui/h96ui missing: run tools/h96ui-rs/build.sh"; exit 1; }
install -m 0755 build/h96ui/h96ui $T/usr/local/bin/h96ui
# fixup kit: h96-doctor (read-only check) and h96-fix (repairs), docs/log/troubleshooting.md
install -m 0755 tools/fixup/h96-doctor tools/fixup/h96-fix $T/usr/local/sbin/
for k in $S/platform/skw_sdio_v20.ko $S/skwifi/skw6316.ko $S/skwbt/skwbt.ko; do
  [ "$(modinfo -F vermagic $k | xargs)" = "$KVER SMP mod_unload aarch64" ] || { echo "!! $k vermagic: $(modinfo -F vermagic $k)"; exit 1; }
  install -D -m 0644 $k $T/lib/modules/$KVER/extra/${k##*/}
done
for f in SWT6621S_IRAM_SDIO.bin SWT6621S_DRAM_SDIO.bin SWT6621S_NV_SDIO.bin SWT6621S_NV_SDIO_ALONE.bin \
         SWT6621S_SEEKWAVE_R00001.bin; do install -D -m 0644 $FW/$f $T/lib/firmware/$f; done
[ "$(stat -c %a $T/root)" = 700 ] && [ "$(stat -c %a $T/root/.ssh/authorized_keys)" = 600 ] || { echo "!! /root modes"; exit 1; }
tar --owner=0 --group=0 --numeric-owner -C $T -czf $OUT/h96-inbox.tar.gz .
tar -tvzf $OUT/h96-inbox.tar.gz | awk '$1 !~ /^d/ {print "  "$1, $3, $6, $7, $8}'
( cd $OUT && sha256sum h96-inbox.tar.gz | tee SHA256SUMS.txt )
