#!/usr/bin/env bash
# Build a Maskrom download loader from THIS BOARD'S OWN factory DDR blob, carved out of
# the verified pi-head.bin backup. No USB access whatsoever -- the device is not touched.
#
# WHY THIS BEATS THE PUBLIC-BLOB LOADERS
# rkbin's public rk3528 DDR blobs are documented to mis-train on RK3518 boxes: v1.11/v1.13
# "4BIT" builds fail write-training (wrtrn err) and 2L builds train "marginally", producing
# random Synchronous Aborts whose faulting address decodes to ASCII text. My L0-L3 loaders
# are built from those public blobs. But the board's *factory* DDR init -- the exact binary
# that trains this board's LPDDR3 correctly, since it is what boots the box today -- is
# sitting inside pi-head.bin starting ~1956 bytes after sector 64. Packing THAT into a
# download loader removes the guesswork entirely: it is the same DDR code the working boot
# path already runs.
#
# The carve is SELF-VALIDATING: Rockchip's own ddrbin_tool.py is used to parse each
# candidate back, and a candidate is only accepted if the banner it reports is byte-identical
# to the banner found in the flash image. A wrong offset does not silently produce a wrong
# loader; it produces no accepted candidate.
set -uo pipefail

ROOT=$(cd "$(dirname "$0")/.." && pwd)
WORK=$ROOT/tools/loaders
UP=$WORK/upstream
OUT=$WORK/factory-loader
HEAD=${HEAD:-$ROOT/recon/20261119-1645/pi-head.bin}
mkdir -p "$OUT"
die() { echo "!! $*" >&2; exit 1; }

[ -f "$HEAD" ] || die "no factory head image at $HEAD"
[ -f "$UP/tools/boot_merger" ] || die "boot_merger missing -- run tools/build-loaders.sh first"
[ -f "$WORK/ddrbin_tool.py" ] || cp "$UP/tools/ddrbin_tool.py" "$WORK/ddrbin_tool.py"

echo "===== 1. the banner we are hunting ====="
FACT_BANNER=$(strings -a -n 8 "$HEAD" | grep -m1 -oE 'DDR [0-9a-f]{8,}[^ ]* .*fwver: [^ ]*.*')
echo "factory banner: $FACT_BANNER"
FWVER=$(sed -n 's/.*fwver: \(v[0-9.]*\).*/\1/p' <<<"$FACT_BANNER")
echo "factory fwver : $FWVER"

echo
echo "===== 2. calibrate where the banner sits inside a DDR blob ====="
# The banner is near the END of the blob (offset ~56230 of 60840 in v1.14), not the start, so
# blob_start = banner_file_offset - banner_in_blob_offset. Use a public blob of the SAME
# fwver when one exists, else fall back to a searched window.
python3 -I - "$HEAD" "$FWVER" "$WORK" "$OUT" <<'PY'
import glob, os, re, struct, subprocess, sys
head_path, fwver, work, out = sys.argv[1], sys.argv[2], sys.argv[3], sys.argv[4]
pi = open(head_path, 'rb').read()

fac = re.search(rb'DDR [0-9a-f]{8,}.{0,80}?fwver: \S+', pi)
if not fac:
    sys.exit('no DDR banner in head image')
fac_start, fac_bytes = fac.start(), fac.group()
print('factory banner at byte %d: %s' % (fac_start, fac_bytes.decode('latin1')))
idb = 32768                       # sector 64
print('idblock magic at sector64: %r' % pi[idb:idb+4])

# banner offset inside a public blob, preferring the same fwver
cands = sorted(glob.glob(os.path.join(work, 'upstream/bin/rk35/rk3528_ddr_*.bin')))
def boff(p):
    d = open(p, 'rb').read()
    m = re.search(rb'DDR [0-9a-f]{8,}', d)
    return (m.start() if m else None), len(d), d[:4].hex(' ')
pref = [p for p in cands if fwver.replace('v','') in os.path.basename(p)] or cands
offsets = {}
for p in pref:
    o, n, h = boff(p)
    print('  public %-46s size %6d banner@%s' % (os.path.basename(p), n, o))
    if o:
        offsets.setdefault(o, []).append(p)
print('distinct banner offsets: %s' % sorted(offsets))

# search candidate blob starts and accept only parses whose banner matches the factory one
accepted = []
starts = sorted({fac_start - o for o in offsets} | set(range(idb + 512, idb + 8192, 512)))
for st in starts:
    if st < idb or st > fac_start:
        continue
    for size in (60904, 60840, 58672, 58680, 60816, 60832):
        blob = pi[st:st + size]
        if len(blob) != size:
            continue
        p = os.path.join(out, 'cand_%d_%d.bin' % (st, size))
        open(p, 'wb').write(blob)
        rb = os.path.join(out, 'cand.readback.txt')
        r = subprocess.run(['python3', '-I', os.path.join(work, 'ddrbin_tool.py'), 'rk3528',
                            '-g', rb, p], capture_output=True, text=True)
        txt = ''
        if os.path.isfile(rb):
            txt = open(rb, errors='replace').read()
        got = re.search(r'DDR [0-9a-f]{8,}.{0,80}?fwver: \S+', txt)
        ok = bool(got) and got.group().encode('latin1', 'replace').strip() == fac_bytes.strip()
        print('  start %6d size %6d -> %-6s %s' % (st, size, 'MATCH' if ok else 'no',
              (got.group()[:58] if got else (r.stdout or r.stderr or '').strip()[:58])))
        if ok:
            accepted.append((st, size))
            break
    if accepted:
        break

if not accepted:
    print('\nNO CANDIDATE PARSED BACK AS THE FACTORY BLOB.')
    print('Fall back to the public-blob loaders (L1_4bitpcb_ddr114 / L3_lp3780_ddr1.11) and')
    print('re-examine the idblock layout before packing anything.')
    sys.exit(3)

st, size = accepted[0]
final = os.path.join(out, 'factory_ddr_%s.bin' % fwver)
open(final, 'wb').write(pi[st:st + size])
print('\nACCEPTED: factory DDR blob at byte %d, %d bytes -> %s' % (st, size, final))
print('  (idblock header before it: %d bytes)' % (st - idb))
# record what parsed back, for the loader manifest
os.replace(os.path.join(out, 'cand.readback.txt'), os.path.join(out, 'factory-ddr-params.txt'))
open(os.path.join(out, 'carve.txt'), 'w').write('%d %d %s\n' % (st, size, os.path.basename(final)))
PY
rc=$?
[ $rc -eq 0 ] || { echo; echo "carve step returned $rc -- see notes above"; exit $rc; }

echo
echo "===== 3. pack the loader: factory DDR + usbplug v1.04 + SPL v1.07 ====="
CARVE="$OUT/$(awk '{print $3}' "$OUT/carve.txt")"
REL=$(python3 -Ic "import os,sys; print(os.path.relpath(sys.argv[1], sys.argv[2]))" "$CARVE" "$UP")
# place it under the upstream tree so boot_merger's relative INI paths resolve
cp "$CARVE" "$UP/custom/$(basename "$CARVE")"
REL="custom/$(basename "$CARVE")"

cat > "$UP/factory.ini" <<INI
[CHIP_NAME]
NAME=RK3528
[VERSION]
MAJOR=1
MINOR=4
[CODE471_OPTION]
NUM=1
Path1=$REL
Sleep=1
[CODE472_OPTION]
NUM=1
Path1=bin/rk35/rk3528_usbplug_v1.04.bin
[LOADER_OPTION]
NUM=2
LOADER1=FlashData
LOADER2=FlashBoot
FlashData=$REL
FlashBoot=bin/rk35/rk3528_spl_v1.07.bin
[OUTPUT]
PATH=../factory-loader/L5_factory_ddr.bin
[SYSTEM]
NEWIDB=true
[FLAG]
471_RC4_OFF=true
RC4_OFF=true
CREATE_IDB=false
INI
( cd "$UP" && ./tools/boot_merger factory.ini ) 2>&1 | tail -3
LOADER="$OUT/L5_factory_ddr.bin"
[ -f "$LOADER" ] || die "boot_merger produced no output"
[ "$(head -c4 "$LOADER")" = "LDR " ] || die "output lacks the LDR magic"
echo "built: $LOADER  $(stat -c%s "$LOADER") bytes"

echo
echo "===== 4. cross-check the container header against a loader known to work on RK3518 ====="
for f in "$LOADER" "$WORK/delivery/L4_x88pro_prebuilt.bin"; do
  [ -f "$f" ] && printf "%-34s %s\n" "$(basename "$f")" "$(od -An -tx1 -N24 "$f" | tr -d '\n' | tr -s ' ')"
done
echo "(expect identical magic 4c445220 and the same chip-tag tail; sizes differ with the DDR blob)"
( cd "$WORK/factory-loader" && sha256sum L5_factory_ddr.bin ) | tee -a "$WORK/delivery/SHA256SUMS.txt"

cat <<'EOF'

===== 5. what to do with it -- LATER, on your say-so, not now =====
Nothing above this line touched the device. When you decide to try it:

  rkdeveloptool rd 3          # RST_RESETMASKROM_SUBCODE: ask the loader to reboot to BootROM
                              # IF this is unsupported the box may reboot into Android instead,
                              # which means it can reach your WiFi -- block its MAC first, or use
                              # the pads under the heatsink and skip this command entirely.
  lsusb -d 2207:              # should reappear with bNumConfigurations 0  (maskrom)
  rkdeveloptool db <L5_factory_ddr.bin>     # RAM only; writes nothing
  rkdeveloptool ld                           # expect "Loader" again
  rkdeveloptool rl 65536 8 /tmp/t.bin && od -An -tx1 -N8 /tmp/t.bin
  rkdeveloptool rl $((30777344-33)) 8 /tmp/g.bin && grep -a EFI\ PART /tmp/g.bin

If that last pair returns real data and 'EFI PART', the ceiling was the vendor loader's and
we re-dump the whole device properly. If it still reads 0xCC with the factory DDR packed in,
then "something is deliberately limiting access" graduates from hypothesis to leading
candidate -- and the extraction path becomes recovery-over-ADB instead.
EOF
