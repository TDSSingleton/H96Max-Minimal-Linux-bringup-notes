#!/usr/bin/env bash
# Pull the parts of the eMMC that the Rockchip loader will not serve past 32 MiB, by
# reading them from Android RECOVERY over ADB instead.
#
# WHY THIS BEATS rkdeveloptool HERE
# The vendor loader answers `rl` with 0xCC filler above LBA 65536 under both read
# sub-codes (proven by tools/read-ceiling-ab.sh against this unit). Recovery gives a root
# shell inside the box, where /dev/block/by-name/* is readable directly and the data goes
# out over the same USB-C cable. No writes, no network, and no full Android boot.
#
# NETWORK WARNING -- READ BEFORE POWERING ON
# Do NOT reach recovery by booting Android and running `adb reboot recovery` if this box
# ever joined a WiFi network: it will reconnect and dial out before you get a shell.
# Enter recovery from COLD with the chassis button instead. Recovery's adbd is USB-only;
# the recovery ramdisk has no supplicant session and joins nothing.
#
# This script only ever runs `dd if=<device> of=/dev/stdout`. There is no output path to
# the device. Review it before running if that claim matters to you.
set -uo pipefail

ROOT=$(cd "$(dirname "$0")/.." && pwd)
OUT=${OUT:-"$ROOT/recon/recovery-$(date +%Y%m%d-%H%M)"}
PARTS=${PARTS:-"recovery super"}          # space-separated names; add userdata if wanted
ADB=${ADB:-adb}
mkdir -p "$OUT/meta" "$OUT/partitions"
log() { echo "[$(date +%H:%M:%S)] $*"; }

sh -c 'command -v adb' >/dev/null || { echo "no adb on PATH"; exit 1; }

echo "===== 0. what is attached ====="
"$ADB" devices -l 2>&1 | tee "$OUT/meta/adb-devices.txt"
if ! "$ADB" get-state >/dev/null 2>&1; then
  cat <<'EOF'
!! No ADB device.
   - If the TV shows the Android recovery menu but nothing appears here, this box's
     recovery may not expose USB ADB on the C jack. Use the fallback below.
   - If it booted into Android instead, POWER IT OFF NOW (unplug) and re-enter with the
     button held from cold -- you do not want the full OS running with network access.
EOF
  exit 1
fi
"$ADB" root 2>&1 | tee "$OUT/meta/adb-root.txt"
sleep 2
echo "--- whoami:"; "$ADB" shell id 2>&1 | tee "$OUT/meta/id.txt"

echo
echo "===== 1. THE BOARD DEVICE TREE (the thing the loader ceiling hid from us) ====="
# /sys/firmware/fdt is the live, fully-resolved tree: exact RAM layout, the WiFi module and
# which SDIO/USB bus it hangs off, HDMI hotplug GPIO, whether an IR demod or an ethernet PHY
# is even wired on this board. This is the single highest-value artifact in the whole run.
if "$ADB" exec-out "cat /sys/firmware/fdt" > "$OUT/devicetree.dtb" 2>"$OUT/meta/fdt.err"; then
  log "got /sys/firmware/fdt: $(stat -c%s "$OUT/devicetree.dtb") bytes"
  dtc -I dtb -O dts "$OUT/devicetree.dtb" -o "$OUT/devicetree.dts" 2>"$OUT/meta/dtc.err" \
    && log "decompiled to $OUT/devicetree.dts" || log "dtc failed, see meta/dtc.err"
  grep -m2 -E '^\s*model|^\s*compatible' "$OUT/devicetree.dts" 2>/dev/null | tee "$OUT/meta/model.txt"
  echo "--- memory:"; grep -A1 -E '^\smemory@' "$OUT/devicetree.dts" 2>/dev/null | head -6
  echo "--- wifi/sdio/eth/ir/bluetooth present in the tree:"
  grep -oE '(brcmf|ap62|aic[0-9a-z]*|swt6621|seekwave|syna[a-z0-9]*|realtek,rtl[a-z0-9]*|stmmac|dwmac|ethernet-phy|remotectl|ir-receiver|pwm-ir|wireless-bluetooth|bluetooth-platdata|hdmi-sound|rockchip,hdmi|vop|hdmi@)' \
    "$OUT/devicetree.dts" 2>/dev/null | sort | uniq -c | sort -rn | head -20
else
  log "no /sys/firmware/fdt -- trying /proc/device-tree walk"
  "$ADB" exec-out "cat /proc/devicetree 2>/dev/null" > "$OUT/devicetree.dtb" 2>/dev/null || true
fi

echo
echo "===== 2. identity + health of the storage ====="
for c in "getprop" "ls -lZ /dev/block/by-name/" "df -h" "mount"
         "cat /sys/block/mmcblk2/device/life_time /sys/block/mmcblk2/device/pre_eol_info 2>/dev/null"
         "cat /sys/block/mmcblk*/device/name /sys/block/mmcblk*/device/csd 2>/dev/null"
         "ls -l /sys/bus/sdio/devices/ 2>/dev/null; cat /sys/bus/sdio/devices/*/vendor /sys/bus/sdio/devices/*/device 2>/dev/null"
         "lsusb 2>/dev/null; cat /sys/kernel/debug/usb/devices 2>/dev/null | head -80"
         "uname -a; cat /proc/cmdline; getenforce"; do
  f=$(echo "$c" | tr -c 'a-z0-9' '_' )
  echo "### $c" | tee -a "$OUT/meta/probe-all.txt"
  "$ADB" shell "$c" 2>&1 | tee -a "$OUT/meta/probe-all.txt" | head -25
  echo
done

echo
echo "===== 3. implant indicators the RE writeup named (read-only listing) ====="
"$ADB" shell 'for p in /data/gsdnode /data/blueberry /data/.lotso /data/.ant /data/smallp \
   /system/bin/gsdnode_android /system/bin/preinstall.sh /system/preinstall; do
     if [ -e "$p" ]; then echo "PRESENT: $p"; ls -l "$p" 2>/dev/null | head -5; else echo "absent : $p"; fi
   done
   echo "--- props:"
   for k in persist.box.customer ro.product.name ro.product.model ro.product.ota.host \
            ro.build.fingerprint ro.build.version.release; do echo "$k=$($(command -v getprop) $k)"; done' \
   2>&1 | tee "$OUT/meta/implant-check.txt"

echo
echo "===== 4. stream the partitions the loader could not give us ====="
log "partitions to capture: $PARTS"
log "expect ~25 MB/s on USB2; super is 2.3 GiB, so a few minutes each"
for p in $PARTS; do
  dest="$OUT/partitions/$p.img"
  size=$("$ADB" shell "[ -b /dev/block/by-name/$p ] && blockdev --getsize /dev/block/by-name/$p" 2>/dev/null | tr -dc '0-9')
  if [ -z "$size" ]; then log "SKIP $p (no such block device)"; continue; fi
  log "reading $p ($(( size * 512 / 1048576 )) MiB)"
  if "$ADB" exec-out "dd if=/dev/block/by-name/$p bs=1M 2>/dev/null" > "$dest"; then
    got=$(stat -c%s "$dest")
    if [ "$got" -eq $(( size * 512 )) ]; then log "  ok $dest"
    else log "  SIZE MISMATCH $dest got $got want $(( size * 512 ))"; fi
  else
    log "  FAILED $p"
  fi
done

echo
echo "===== 5. fingerprints ====="
( cd "$OUT" && find . -type f \( -name '*.img' -o -name '*.dtb' \) -exec sha256sum {} + | sort -k2 > SHA256SUMS.txt ) \
  && cat "$OUT/SHA256SUMS.txt"
du -sh "$OUT"

cat <<'EOF'

FALLBACK if recovery exposes no USB ADB (the port-on-this-box is the same one we flashed
over, so this usually works, but Android recovery builds vary):
  put a FAT32 USB stick in the USB-A host port, and in recovery use "mount USB" if the menu
  offers it, or from an ADB shell:
     mkdir -p /tmp/usb && mount -t vfat /dev/block/sda1 /tmp/usb
     dd if=/dev/block/by-name/super of=/tmp/usb/super.img bs=8M
  Note that is a write to the STICK, not to the box.
EOF
