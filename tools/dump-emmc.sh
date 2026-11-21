#!/usr/bin/env bash
# READ-ONLY full eMMC image of an RK3518/RK3528 box, over RockUSB in loader mode.
#
# This script contains no write command. VERB SEMANTICS READ FROM usage() IN main.cpp:
#   SAFE / RAM-only : db <loader>  = DownloadBoot (runs in RAM, never touches flash)
#   SAFE / read-only: ld rci rid rfi rcb ppt rl
#   DESTRUCTIVE     : ef (EraseFlash)  wl wlx (WriteLBA)  gpt (WriteGPT)
#                     prm (WriteParameter)  tagspl (TagSPL)
#                     ul (UpgradeLoader) <- TRAP: this WRITES THE LOADER TO FLASH.
#                       It sounds like "upload loader into RAM". It is not. It is the
#                       single most likely way to permanently brick this box, because it
#                       overwrites the sector-64 factory idbloader that holds the only
#                       copy of the board-tuned DDR init.
#   AVOID           : td (TestDevice) -- exercises the media; unneeded and unwelcome on
#                       an eMMC already reported at ~70% of its write lifecycle.
# Never run ef or ul on this device for any reason.
#
# WHY THIS IS THE RECON STEP: in Maskrom/loader mode the box runs only its
# unmodifiable BootROM plus a RAM-resident loader. No Android, no network stack, no
# OTA client, no daemon can wake up. It is the closest thing to zero-runtime that
# exists on this hardware, and it yields everything at once: the forensic image, the
# DDR-blob backup, and the boot.img we would patch for root.
#
# Usage: tools/dump-emmc.sh            # full run
#        RK=/path/to/rkdeveloptool tools/dump-emmc.sh
set -uo pipefail

ROOT=$(cd "$(dirname "$0")/.." && pwd)
OUT=${OUT:-"$ROOT/recon/$(date +%Y%m%d-%H%M)"}
CHUNK_BYTES=${CHUNK_BYTES:-16777216}      # 16 MiB per read
log() { echo "[$(date +%H:%M:%S)] $*"; }
die() { echo "!! $*" >&2; exit 1; }

RK=${RK:-}
if [ -z "$RK" ]; then
  for c in "$ROOT/downloads/rkdeveloptool-upstream/rkdeveloptool" \
           "$ROOT/tools/rkbt/bin/rkdeveloptool" "$(command -v rkdeveloptool 2>/dev/null)"; do
    [ -n "$c" ] && [ -x "$c" ] && RK=$c && break
  done
fi
[ -n "$RK" ] || die "no rkdeveloptool found; run tools/setup-workstation.sh first"

mkdir -p "$OUT/meta" "$OUT/chunks" || die "cannot create $OUT"
IMG="$OUT/emmc-full.img"

# read_at <start_sector> <nbytes> <dest>   -- adapts to this build's rl() units
read_at() {
  local start=$1 nbytes=$2 dest=$3 size_arg
  if [ "$UNIT" = sector ]; then
    size_arg=$(( nbytes / 512 ))
  else
    size_arg=$nbytes
  fi
  "$RK" rl "$start" "$size_arg" "$dest"
}

echo "===== 0. device state ====="
lsusb -d 2207: | tee "$OUT/meta/lsusb.txt"
"$RK" -v 2>&1 | tee "$OUT/meta/version.txt"
grep -q 2207: <<<"$(lsusb -d 2207:)" || echo ">> Nothing on USB as 2207: the box is not in Maskrom/loader mode. Stop and enter Maskrom."
echo

echo "===== 1a. read-only device identity (no flash access yet) ====="
# These verbs only query. `rci` is the interesting one: it asks the device for its own
# chip id. Expect it to answer RK3528 even on genuine RK3518 silicon -- RKDevTool is
# reported to do exactly that -- so treat it as family confirmation, not SKU proof.
"$RK" ld  2>&1 | tee "$OUT/meta/cmd-ld.txt"
"$RK" rci 2>&1 | tee "$OUT/meta/cmd-rci.txt"
"$RK" rid 2>&1 | tee "$OUT/meta/cmd-rid.txt"
"$RK" rfi 2>&1 | tee "$OUT/meta/cmd-rfi.txt"
"$RK" rcb 2>&1 | tee "$OUT/meta/cmd-rcb.txt"
echo

echo "===== 1b. confirm LOADER mode (not Maskrom) ====="
# VERB IS `ppt` (PrintPartition) IN THIS BUILD. There is NO `pt` -- see usage() in
# main.cpp. An earlier draft of this script called `pt`, which fails and therefore
# masquerades as "you are not in loader mode". Do not chase that ghost again.
"$RK" ppt > "$OUT/meta/partition-table.txt" 2>&1
rc=$?
sed -n '1,40p' "$OUT/meta/partition-table.txt"
if [ $rc -ne 0 ]; then
  cat <<'EOF'

`ppt` failed -- you are almost certainly still in Maskrom, which has no RAM yet.
Upload a loader. This runs in RAM only; it does NOT write flash, and a loader with
the wrong DDR parameters simply fails to train (replug and try the next one).

  RK="db" ->   rkdeveloptool db <loader.bin>      # then wait ~3 s and re-run me

  USE `db`. NEVER `ul` -- `ul` is UpgradeLoader and WRITES THE LOADER TO FLASH.

Try in this order (see tools/build-loaders.sh to produce them):
  1. tools/loaders/delivery/x88pro_maskrom_loader.bin          proven on an RK3518 box
  2. tools/loaders/delivery/rk3518_stock_ddr114_usb104_spl107.bin
  3. tools/loaders/delivery/rk3518_lp3_780_ddr1.11_usb104_spl107.bin   LPDDR3 boards
  4. tools/loaders/delivery/rk3518_lp3_780_ddr1.14_usb104_spl107.bin   LPDDR3 boards
EOF
  exit 2
fi
echo

echo "===== 2. PROBE rl() UNITS -- do not skip this ====="
# A widely-used Debian/pine64 build of rkdeveloptool treats the size argument of `rl`
# as BYTES rather than SECTORS. Getting this backwards silently yields a truncated or
# oversized image, so we measure it instead of assuming.
"$RK" rl 0 1 "$OUT/meta/probe1.bin" > "$OUT/meta/probe.log" 2>&1
sz=$(stat -c%s "$OUT/meta/probe1.bin" 2>/dev/null || echo 0)
log "probe: 'rl 0 1' produced $sz bytes"
case "$sz" in
  512) UNIT=sector; log "this build counts SECTORS" ;;
  1)   UNIT=byte;   log "this build counts BYTES (the pine64 quirk)" ;;
  *)   die "unexpected probe size $sz -- read $OUT/meta/probe.log and stop" ;;
esac
CHUNK_SECTORS=$(( CHUNK_BYTES / 512 ))
echo

echo "===== 3. integrity self-test: read the head TWICE and compare ====="
# Marginal DDR training returns *corrupt data with no error reported* -- the single
# most dangerous failure mode here, because a corrupt backup is worse than none.
# Two reads of the same region, hashed, is the cheap detector.
read_at 0 "$CHUNK_BYTES" "$OUT/meta/head-a.bin" > "$OUT/meta/head-a.log" 2>&1 \
  || die "head read failed -- see $OUT/meta/head-a.log"
read_at 0 "$CHUNK_BYTES" "$OUT/meta/head-b.bin" > /dev/null 2>&1
ha=$(sha256sum "$OUT/meta/head-a.bin" | cut -d' ' -f1)
hb=$(sha256sum "$OUT/meta/head-b.bin" | cut -d' ' -f1)
log "head pass A: $ha"
log "head pass B: $hb"
[ "$ha" = "$hb" ] || die "DOUBLE-READ MISMATCH -- this loader's DDR training is suspect. Try another loader before anything else."
log "double-read identical -- reads from this loader can be trusted"
cp "$OUT/meta/head-a.bin" "$OUT/head.img"
python3 -I "$ROOT/tools/gpt_capacity.py" "$OUT/head.img" | tee "$OUT/meta/gpt.txt"
echo

echo "===== 4. full sequential dump (resumable) ====="
TOTAL=$(awk -F= '/^TOTAL_LBAS=/{print $2}' "$OUT/meta/gpt.txt")
[ -n "${TOTAL:-}" ] && [ "$TOTAL" -gt 1000 ] || die "could not read capacity from GPT; inspect $OUT/meta/gpt.txt"
log "capacity: $TOTAL sectors = $(( TOTAL * 512 / 1073741824 )) GiB"

idx=0
[ -f "$IMG" ] && idx=$(( $(stat -c%s "$IMG") / CHUNK_BYTES ))
[ "${idx:-0}" -gt 0 ] && log "existing image present, resuming at chunk $idx"

while [ $(( idx * CHUNK_SECTORS )) -lt "$TOTAL" ]; do
  off=$(( idx * CHUNK_SECTORS ))
  part=$(printf "%s/chunks/chunk_%05d.bin" "$OUT" "$idx")
  if [ -s "$part" ] && [ "$(stat -c%s "$part")" -eq "$CHUNK_BYTES" ]; then
    log "chunk $idx cached"
  else
    read_at "$off" "$CHUNK_BYTES" "$part" > "$part.log" 2>&1 || { log "read failed at sector $off -- stopping cleanly (re-run to resume)"; break; }
    got=$(stat -c%s "$part")
    if [ "$got" -ne "$CHUNK_BYTES" ]; then log "short read at $off ($got of $CHUNK_BYTES) -- end of device"; break; fi
    log "chunk $idx @ sector $off ok"
  fi
  idx=$(( idx + 1 ))
  [ $(( idx % 32 )) -eq 0 ] && log "progress: $(( idx * CHUNK_BYTES / 1073741824 )) GiB"
done

echo
echo "===== 5. assemble + fingerprint ====="
cat "$OUT"/chunks/chunk_*.bin > "$IMG" 2>/dev/null || die "no chunks to assemble"
log "image: $IMG  $(stat -c%s "$IMG") bytes"
( cd "$OUT" && sha256sum emmc-full.img meta/head-a.bin chunks/chunk_00000.bin \
    > SHA256SUMS.txt 2>/dev/null ) && cat "$OUT/SHA256SUMS.txt"

echo
echo "Anti-brick insurance -- the board-tuned DDR blob exists ONLY here:"
echo "  dd if=$IMG of=$OUT/factory_idbloader.bin bs=512 skip=64   count=2048   # idbloader (DDR+SPL)"
echo "  dd if=$IMG of=$OUT/factory_uboot.itb     bs=512 skip=16384 count=8192   # u-boot.itb"
echo
echo "Next: send me $OUT/meta/gpt.txt, $OUT/meta/partition-table.txt and $OUT/meta/lsusb.txt"
echo "and I will map the partitions and pull out boot.img for the root step."
