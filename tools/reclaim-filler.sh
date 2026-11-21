#!/usr/bin/env bash
# Find and reclaim capture files that are provably worthless, WITHOUT deleting anything
# that merely looks suspicious.
#
# A partition dump is eligible for deletion only if BOTH hold:
#   1. its start LBA is at or beyond the proven read ceiling (LBA 65536 = 32 MiB), so no
#      byte of it could have been real, and
#   2. every single byte of the file is the same value (verified by reading the whole file,
#      not a sample).
# Files starting below the ceiling are never eligible even if they are 100% filler --
# boot.img starts at LBA 51200 and its first ~7 MiB is the only copy of the stock kernel
# header and zImage start we have.
#
# Usage:  bash tools/reclaim-filler.sh              # dry run, prints the table
#         bash tools/reclaim-filler.sh --delete     # actually remove + write a manifest
set -uo pipefail

ROOT=$(cd "$(dirname "$0")/.." && pwd)
RUN=${1:-}
DIR=$(ls -d "$ROOT"/recon/*/ 2>/dev/null | tail -1)
[ -n "$DIR" ] || { echo "no recon/ capture directory found"; exit 1; }
echo "examining: $DIR"
[ "$RUN" = "--delete" ] && echo "MODE: DELETE" || echo "MODE: dry run (pass --delete to remove)"
echo

python3 -I - "$DIR" "$RUN" <<'PY'
import os, re, sys

d, mode = sys.argv[1], (sys.argv[2] == "--delete")
CEILING = 65536            # sectors; 65536 * 512 == 32 MiB, measured on this device
CHUNK = 8 << 20

# partition start LBAs, from the saved read-only `ppt` output
starts = {}
meta = os.path.join(d, "meta", "ppt.txt")
if os.path.isfile(meta):
    for line in open(meta, "r", errors="replace"):
        line = line.replace("\r", "").strip()
        m = re.match(r'^\d+\s+([0-9A-Fa-f]{6,})\s+(\S+)$', line)
        if m:
            starts[m.group(2)] = int(m.group(1), 16)
else:
    print("!! no meta/ppt.txt; cannot classify by LBA -- nothing will be deleted")

def uniform(path):
    """True only if every byte in the file equals the first byte."""
    with open(path, "rb") as fh:
        first = fh.read(1)
        if not first:
            return None
        pat = first * CHUNK
        seen = 0
        while True:
            b = fh.read(CHUNK)
            if not b:
                return True
            if len(b) == CHUNK and b != pat:
                return False
            if any(x != first[0] for x in b):
                return False
            seen += len(b)

hdr = "%-22s %12s %-9s %-14s %s" % ("file", "bytes", "startLBA", "position", "verdict")
print(hdr); print("-" * len(hdr))
freed = 0
victims = []
pdir = os.path.join(d, "partitions")
for name in sorted(os.listdir(pdir)) if os.path.isdir(pdir) else []:
    if not name.endswith(".img"):
        continue
    p = os.path.join(pdir, name)
    size = os.path.getsize(p)
    part = name[:-4]
    lba = starts.get(part)
    if lba is None:
        print("%-22s %12d %-9s %-14s %s" % (name, size, "?", "unknown", "KEEP (no LBA)"))
        continue
    pos = "%.1f MiB" % (lba * 512 / 1048576.0)
    if lba < CEILING:
        print("%-22s %12d %-9s %-14s %s" % (name, size, hex(lba), pos,
              "KEEP (starts inside readable window)"))
        continue
    u = uniform(p)
    if u is None:
        print("%-22s %12d %-9s %-14s %s" % (name, size, hex(lba), pos, "empty file -> removable"))
        victims.append((p, size, "empty")); freed += size
    elif u:
        with open(p, "rb") as fh:
            v = fh.read(1).hex()
        print("%-22s %12d %-9s %-14s %s" % (name, size, hex(lba), pos,
              "UNIFORM 0x%s beyond ceiling -> removable" % v))
        victims.append((p, size, "uniform-0x%s" % v)); freed += size
    else:
        print("%-22s %12d %-9s %-14s %s" % (name, size, hex(lba), pos,
              "MIXED beyond ceiling -> KEEP, inspect"))

# the .rlio progress logs and the stale checksum file
logs = [os.path.join(pdir, f) for f in os.listdir(pdir)] if os.path.isdir(pdir) else []
logs = [f for f in logs if f.endswith(".rlio")]
print("\n%-22s %12d %s" % ("progress logs", sum(os.path.getsize(f) for f in logs),
      "(%d files, pure tool chatter)" % len(logs)))
print("potentially reclaimable: %.2f GiB" % (freed / 1073741824.0))

if not mode:
    print("\ndry run -- nothing removed. Re-run with --delete.")
    sys.exit(0)

manifest = os.path.join(d, "DELETED-manifest.txt")
with open(manifest, "w") as out:
    for p, size, why in victims:
        os.remove(p)
        out.write("%s\t%d\t%s\n" % (os.path.basename(p), size, why))
    for f in logs:
        os.remove(f)
        out.write("%s\t%d\ttool-progress-log\n" % (os.path.relpath(f, d), os.path.getsize(f) if False else 0))
sha = os.path.join(d, "SHA256SUMS.txt")
if os.path.isfile(sha):
    os.remove(sha)
    print("removed stale SHA256SUMS.txt (it fingerprinted the filler files)")
print("removed %d files, %.2f GiB; manifest at %s" % (len(victims), freed / 1073741824.0, manifest))
print("what remains is the trustworthy part of the capture. Re-dump the rest once the")
print("read ceiling is beaten, into a NEW recon/<ts> directory, so the verified 32 MiB")
print("baseline stays intact until the new capture passes its own gates.")
PY
