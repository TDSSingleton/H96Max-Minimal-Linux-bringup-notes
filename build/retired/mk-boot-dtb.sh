#!/usr/bin/env bash
# The DTB that goes into our `boot` image: the factory one (from the template's header dtb
# area), plus our edits, each read back with fdtget. mk-bootimg.py --dtb takes the result
# (and --fbdev-on still flips fbdev_emulation in /chosen/bootargs afterwards).
#
# Edits:
#   /usbdrd/dwc3@fe500000  dr_mode = "peripheral"   USB-C port becomes a USB *device*
#                          maximum-speed = "high-speed"  (USB2 gadget; no SS link training)
#   The USB-A port (the stick) is the separate EHCI/OHCI pair at ff100000/ff140000.
set -euo pipefail
ROOT=${ROOT:-$(cd "$(dirname "$0")/../.." && pwd)}
TPL=${TPL:-$ROOT/build/good/boot.img}
OUT=${OUT:-$ROOT/build/kernel/boot.dtb}
command -v fdtput >/dev/null || { echo "!! need fdtput (dnf install dtc)"; exit 1; }
python3 -I - "$TPL" "$OUT" <<'PY'
import struct, sys
d = open(sys.argv[1], 'rb').read(0x700)
pg = struct.unpack_from('<I', d, 0x24)[0]; r = lambda x: (x + pg - 1) // pg * pg
k, rd, s = (struct.unpack_from('<I', d, o)[0] for o in (0x08, 0x10, 0x18))
n = struct.unpack_from('<I', d, 0x670)[0]
off = pg + r(k) + r(rd) + r(s)
with open(sys.argv[1], 'rb') as f:
    f.seek(off); dtb = f.read(n)
assert dtb[:4] == b'\xd0\x0d\xfe\xed', 'no FDT at the header dtb offset'
open(sys.argv[2], 'wb').write(dtb)
PY
N=/usbdrd/dwc3@fe500000
echo "  factory: dr_mode=$(fdtget -t s "$OUT" $N dr_mode)"
fdtput -t s "$OUT" $N dr_mode peripheral
fdtput -t s "$OUT" $N maximum-speed high-speed
[ "$(fdtget -t s "$OUT" $N dr_mode)" = peripheral ] || { echo "!! dr_mode edit did not stick"; exit 1; }
[ "$(fdtget -t s "$OUT" $N maximum-speed)" = high-speed ] || { echo "!! maximum-speed edit did not stick"; exit 1; }
echo "  ours:    dr_mode=$(fdtget -t s "$OUT" $N dr_mode) maximum-speed=$(fdtget -t s "$OUT" $N maximum-speed)"
echo "  $OUT  $(stat -c %s "$OUT") bytes  sha256 $(sha256sum "$OUT" | cut -c1-16)"
