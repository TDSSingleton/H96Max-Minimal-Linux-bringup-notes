#!/usr/bin/env bash
# Build the locally-patched rkdeveloptool and A/B test the two read sub-codes against the
# live device, to find out whether the 32 MiB read ceiling is the TOOL's choice of
# RWMETHOD_IMAGE or the loader's own limit.
#
# Background: main.cpp's read_lba() historically called RKU_ReadLBA() with the default
# bySubCode = RWMETHOD_IMAGE (0). RWMETHOD_LBA (1) is declared in RKComm.h but never passed
# anywhere in upstream. Our device's `rcb` output says "Direct LBA: enabled". The patch makes
# it selectable with RK_DIRECT_LBA=1. READ PATH ONLY -- write_lba() is untouched, so this
# binary cannot write any more than the stock one could, and we are not going to.
#
# The decisive probe is the LAST LBA of the device: a valid primary GPT must be mirrored
# there, so seeing "EFI PART" proves reads span the whole device. A single good offset
# elsewhere could be coincidence; this one cannot.
set -uo pipefail

ROOT=$(cd "$(dirname "$0")/.." && pwd)
SRC=$ROOT/downloads/rkdeveloptool-upstream
RK=$SRC/rkdeveloptool
CAP=${CAP:-30777344}          # sectors, from `rfi` on this unit
LIM=$((65536))                # the observed ceiling: exactly 2^16 sectors = 32 MiB

echo "===== build ====="
cd "$SRC" || exit 1
PATCH=$ROOT/tools/rkdeveloptool/0001-read_lba-selectable-direct-lba.patch
if patch -p1 -R --dry-run -s <"$PATCH" >/dev/null 2>&1; then echo "  already applied: $(basename "$PATCH")"
else patch -p1 -s <"$PATCH" || { echo "!! patch failed: $PATCH"; exit 1; }; echo "  applied: $(basename "$PATCH")"; fi
if ! make -j"$(nproc)" 2>&1 | tail -3; then
  echo "!! build failed. If configure output is missing, run: autoreconf -fi && ./configure"
  exit 1
fi
[ -x "$RK" ] || { echo "!! no binary at $RK"; exit 1; }

echo; echo "===== device state ====="
"$RK" ld 2>&1 | tee "$ROOT/tools/ab-ld.txt"
grep -qi loader "$ROOT/tools/ab-ld.txt" || echo ">> not in Loader mode -- nothing else in this script will work"
"$RK" rcb 2>&1 | tee "$ROOT/tools/ab-rcb.txt"

probe() { # label  env  lba
  local label=$1 env=$2 lba=$3 out=/tmp/ab.bin
  rm -f "$out"
  # shellcheck disable=SC2086
  env $env "$RK" rl "$lba" 8 "$out" >/dev/null 2>&1
  local h="MISSING"
  [ -s "$out" ] && h=$(od -An -tx1 -N8 "$out" | tr -d ' \n')
  local verdict=FILLER
  case "$h" in
    cccccccccccccccc) verdict="FILLER (0xCC)" ;;
    0000000000000000) verdict="all-zero" ;;
    MISSING)          verdict="no data" ;;
    *)                verdict="REAL DATA" ;;
  esac
  printf "  %-18s LBA %-9d %-16s %s\n" "$label" "$lba" "$verdict" "$h"
  LAST_VERDICT=$verdict
}

echo; echo "===== A/B at and above the ceiling ====="
echo "--- RWMETHOD_IMAGE (upstream default)"
probe IMAGE  ""                $LIM
probe IMAGE  ""                $((LIM * 2))
probe IMAGE  ""                1048576
probe IMAGE  ""                $((CAP - 33))
echo "--- RWMETHOD_LBA (RK_DIRECT_LBA=1)"
probe LBA    RK_DIRECT_LBA=1   $LIM
probe LBA    RK_DIRECT_LBA=1   $((LIM * 2))
probe LBA    RK_DIRECT_LBA=1   1048576
probe LBA    RK_DIRECT_LBA=1   $((CAP - 33))

echo; echo "===== direct-method check of the backup GPT (the proof that matters) ====="
rm -f /tmp/abgpt.bin
RK_DIRECT_LBA=1 "$RK" rl $((CAP - 33)) 33 /tmp/abgpt.bin >/dev/null 2>&1
if [ -s /tmp/abgpt.bin ] && grep -qa 'EFI PART' /tmp/abgpt.bin; then
  echo ">>> FOUND 'EFI PART' at the end of the device using RWMETHOD_LBA."
  echo ">>> The ceiling was the tool's read method. Re-dump like this:"
  echo ">>>   RK_DIRECT_LBA=1 bash $ROOT/tools/dump-read-only.sh"
else
  echo ">>> No backup GPT via RWMETHOD_LBA either."
  head -c 8 /tmp/abgpt.bin 2>/dev/null | od -An -tx1
  echo ">>> Both sub-codes are capped by the running loader. Next variable is the loader"
  echo ">>> itself: go to Maskrom (pads under the heatsink) and upload a loader built from"
  echo ">>> usbplug v1.04 + SPL v1.07 -- try L1_4bitpcb_ddr114.bin first, since this board's"
  echo ">>> factory blob is 'v1.11 4bit PCB' (L3_lp3780_ddr1.11.bin is the alternate)."
fi
