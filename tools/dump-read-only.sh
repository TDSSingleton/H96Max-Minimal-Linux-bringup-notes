#!/usr/bin/env bash
# READ-ONLY, SAVE-ONLY extraction of every named partition from a Rockchip box in
# LOADER mode. Writes nothing to the device: rkdeveloptool is invoked through safe(),
# which hard-refuses any verb not on its allowlist. The only transfer verb used is `rl`.
#
# Output layout:
#   recon/<ts>/pi-head.bin           sectors 0..8191 -- protective MBR, GPT, and the
#                                    factory idbloader at sector 64 (the DDR blob)
#   recon/<ts>/partitions/<name>.img one file per GPT partition, sized start[i+1]-start[i]
#   recon/<ts>/meta/                 every read-only query's raw output
#   recon/<ts>/SHA256SUMS.txt        fingerprints of what we captured
#
# userdata is large (~11 GiB here) and is where a *running* implant would live, so it is
# included by default; SKIP_USERDATA=1 to leave it out and decide later.
set -uo pipefail

ROOT=$(cd "$(dirname "$0")/.." && pwd)
RK=${RK:-$ROOT/downloads/rkdeveloptool-upstream/rkdeveloptool}
OUT=${OUT:-"$ROOT/recon/$(date +%Y%m%d-%H%M)"}
CHUNK_SECTORS=${CHUNK_SECTORS:-32768}          # 16 MiB per rl() call
SKIP_USERDATA=${SKIP_USERDATA:-0}
log() { echo "[$(date +%H:%M:%S)] $*"; }
die() { echo "!! $*" >&2; exit 1; }

# The one place a device command is ever issued. Anything not listed here cannot run.
safe() {
  case "$1" in
    ld|rci|rid|rfi|rcb|ppt|rl) ;;
    *) die "safe() refused non-read-only verb: '$1'" ;;
  esac
  local first=$1
  for a in "$@"; do
    case "$a" in ef|ul|wl|wlx|gpt|prm|tagspl|db|dl|td|uc|rs) die "safe() refused write-class token '$a'";; esac
  done
  "$RK" "$@"
}

[ -x "$RK" ] || die "rkdeveloptool missing at $RK"
mkdir -p "$OUT/meta" "$OUT/partitions" || die "cannot write $OUT"
echo "$RK" | tee "$OUT/meta/tool.txt"

echo "===== 0. prove LOADER mode ====="
mode=$(safe ld 2>&1)
echo "$mode" | tee "$OUT/meta/ld.txt"
grep -qi "loader" <<<"$mode" || die "not in Loader mode -- stop"
grep -qi "maskrom" <<<"$mode" && die "Maskrom: a loader upload would be needed; aborting this read-only script"

for c in rci rid rfi rcb; do echo "--- $c"; safe "$c" 2>&1 | tee "$OUT/meta/$c.txt"; done

echo
echo "===== 1. capacity + partition table ====="
# NOTE: rkdeveloptool prints with CRLF line endings. Stripping \r here is not cosmetic --
# without it the parsed partition names carry a trailing carriage return and the output
# FILES are named "boot<CR>.img", which silently breaks every later path reference.
safe ppt 2>&1 | tr -d '\r' > "$OUT/meta/ppt.txt" || die "ppt failed"
cat "$OUT/meta/ppt.txt"
CAP=$(awk '/Flash Size:.*Sectors/{print $3}' "$OUT/meta/rfi.txt" | tr -d ' ')
[ -n "${CAP:-}" ] || die "could not parse capacity from rfi"
log "device capacity: $CAP sectors = $(( CAP * 512 / 1073741824 )) GiB"

# name:start pairs, in order, terminated by capacity
mapfile -t PARTS < <(awk '/^[0-9][0-9] /{printf "%s %s\n", $3, strtonum("0x"$2)}' "$OUT/meta/ppt.txt")
[ "${#PARTS[@]}" -gt 0 ] || die "could not parse partition names from ppt"

echo
echo "===== 2. rl() unit probe ====="
safe rl 0 1 "$OUT/meta/probe.bin" > "$OUT/meta/probe.log" 2>&1
sz=$(stat -c%s "$OUT/meta/probe.bin" 2>/dev/null || echo 0)
case "$sz" in
  512) UNIT=sector; log "rl counts SECTORS (512 B for 'rl 0 1')" ;;
  1)   UNIT=byte;   log "rl counts BYTES (pine64 build quirk)" ;;
  *)   die "unexpected probe size $sz" ;;
esac
[ "$UNIT" = sector ] || die "byte-semantics build not wired up here; set RK to the autotools build"

read_range() { # start_sectors nsectors dest
  local start=$1 n=$2 dest=$3 done_s=0 this
  rm -f "$dest" "$dest.part"
  while [ $done_s -lt $n ]; do
    this=$(( n - done_s )); [ $this -gt $CHUNK_SECTORS ] && this=$CHUNK_SECTORS
    safe rl $(( start + done_s )) "$this" "$dest.part" > "$dest.rlio" 2>&1 \
      || { echo "  read FAILED at sector $(( start + done_s )) (see $dest.rlio)"; return 1; }
    local got; got=$(stat -c%s "$dest.part")
    [ "$got" -eq $(( this * 512 )) ] || { echo "  short read $got != $(( this * 512 ))"; return 1; }
    cat "$dest.part" >> "$dest"; rm -f "$dest.part"
    done_s=$(( done_s + this ))
  done
  [ "$(stat -c%s "$dest")" -eq $(( n * 512 )) ]
}

echo
echo "===== 3. integrity gate: read the head region TWICE ====="
# Marginal DDR training returns corrupt data with no error reported. Two reads of the same
# region must hash identically or nothing captured here is trustworthy.
read_range 0 8192 "$OUT/meta/head-a.bin" || die "head read A failed"
read_range 0 8192 "$OUT/meta/head-b.bin" || die "head read B failed"
ha=$(sha256sum "$OUT/meta/head-a.bin"|cut -d' ' -f1); hb=$(sha256sum "$OUT/meta/head-b.bin"|cut -d' ' -f1)
log "head A: $ha"; log "head B: $hb"
[ "$ha" = "$hb" ] || die "DOUBLE-READ MISMATCH -- do not trust any read from this session"
cp "$OUT/meta/head-a.bin" "$OUT/pi-head.bin"
log "head gate passed"
log "idbloader DDR banner in your factory flash:"
strings -a -n 8 "$OUT/pi-head.bin" | grep -m3 -iE '^DDR |fwver' | tee "$OUT/meta/ddr-banner.txt" || echo "  (no DDR banner string found in sectors 0..8191 -- check where the loader really lives)"

echo
echo "===== 3a. STRUCTURAL GATE: does the backup GPT exist at the last LBA? ====="
# Added after a hard lesson: the head double-read above only proves reads are good in the
# region you already knew was readable. It gave false confidence on a real device where
# reads silently return 0xCC filler beyond LBA 65536 (exactly 32 MiB) -- inside the very
# boot partition whose header declares a 1.3 MB ramdisk that then reads as filler.
# A valid primary GPT MUST be mirrored at the last LBA, so its absence is proof that
# either the media or the read path did not deliver real data. This test spans the whole
# device and cannot be fooled by a read window.
LAST=$(( CAP - 33 ))
if read_range "$LAST" 33 "$OUT/meta/gpt-tail.bin"; then
  if grep -aq 'EFI PART' "$OUT/meta/gpt-tail.bin"; then
    log "backup GPT present at the last LBA -- full-span reads are working"
    READ_CEILING=$CAP
  else
    log "NO backup GPT at the last LBA (got $(od -An -tx1 -N8 "$OUT/meta/gpt-tail.bin" | tr -d ' \n'))"
    log "probing outward from 0 to find where reads stop being real..."
    ceiling=65536
    for probe in 65536 131072 262144 1048576 4194304 16777216; do
      [ $probe -ge $CAP ] && break
      read_range $probe 8 "$OUT/meta/probe-$probe.bin" 2>/dev/null || break
      if [ "$(od -An -tx1 -N8 "$OUT/meta/probe-$probe.bin" | tr -d ' \n')" = "cccccccccccccccc" ]; then
        ceiling=$probe; break
      fi
      ceiling=$probe
    done
    READ_CEILING=$ceiling
    echo
    echo "!! READ CEILING FOUND: ~$(( ceiling * 512 / 1048576 )) MiB. Beyond this the loader"
    echo "!! returns 0xCC filler with NO error reported. Partitions past that point in this"
    echo "!! dump are NOT usable backups. Do not plan a reflash from them."
    echo "!! Everything below is still trustworthy; capture the rest another way (a loader"
    echo "!! uploaded in Maskrom, or U-Boot 'mmc read')."
    echo "!! Continuing anyway so the usable part is still saved."
  fi
else
  log "could not read the device tail at all"; READ_CEILING=$CAP
fi

echo
echo "===== 4. dump each partition ====="
total=${#PARTS[@]}
for i in $(seq 0 $(( total - 1 ))); do
  name=$(cut -d' ' -f1 <<<"${PARTS[$i]}")
  start=$(cut -d' ' -f2 <<<"${PARTS[$i]}")
  if [ $(( i + 1 )) -lt $total ]; then next=$(cut -d' ' -f2 <<<"${PARTS[$(( i + 1 ))]}"); else next=$CAP; fi
  n=$(( next - start ))
  [ $n -gt 0 ] || { log "skip $name (zero/negative span)"; continue; }
  if [ "$name" = userdata ] && [ "$SKIP_USERDATA" = 1 ]; then log "SKIP userdata ($(( n*512/1048576 )) MiB) by request"; continue; fi
  dest="$OUT/partitions/$name.img"
  if [ -f "$dest" ] && [ "$(stat -c%s "$dest")" -eq $(( n * 512 )) ]; then
    log "$name already complete, reusing"; continue
  fi
  log "reading $name : sector $start .. $next  ($n sectors = $(( n*512/1048576 )) MiB)"
  read_range "$start" "$n" "$dest" && log "  ok $dest" || log "  INCOMPLETE $name -- see $dest.rlio"
done

echo
echo "===== 5. fingerprints ====="
( cd "$OUT" && find . -type f \( -name '*.img' -o -name '*.bin' \) -print0 \
    | xargs -0 -r sha256sum | sort -k2 > SHA256SUMS.txt )
cat "$OUT/SHA256SUMS.txt"
echo
du -sh "$OUT" | tee -a "$OUT/meta/total-size.txt"
log "done. Nothing was written to the device: only ld/rci/rid/rfi/rcb/ppt/rl were invoked."
