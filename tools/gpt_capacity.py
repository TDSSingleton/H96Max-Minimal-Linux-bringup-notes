#!/usr/bin/env python3
"""Print total LBA count + partition table from the first megabyte of a Rockchip
eMMC dump. Standalone, stdlib only, no third-party imports.

Usage: python3 -I gpt_capacity.py <head.img>
"""
import struct
import sys
import uuid

SECTORS = []


def read_head(path):
    with open(path, "rb") as fh:
        return fh.read(1 << 20)


def main(path):
    blob = read_head(path)
    if len(blob) < 512 * 34:
        sys.exit("head image too small (%d bytes) -- aborting" % len(blob))

    # Protective MBA check: LBA0 must end with the 55AA boot signature.
    if blob[0x1FE:0x200] != b"\x55\xAA":
        print("WARNING: no 55AA boot signature at LBA0 0x1FE -- sector size may not be 512")
    if blob[512:516] != b"EFI PART":
        sys.exit("no EFI PART header at LBA1 -- not a GPT device, or reads are corrupt")

    # Header layout (offsets from LBA1 start): 8 sig, 8 rev, 12 hdrsz, 16 crc,
    # 20 reserved, 24 myLBA, 32 altLBA, 40 firstUsable, 48 lastUsable, 56 diskGUID,
    # 72 entryLBA, 80 numEntries, 84 entrySize.
    # Sector size is a device property, not stored in the header; we assume 512 and
    # the 55AA / "EFI PART" checks above are what catch a wrong assumption.
    (rev, hdr_sz, crc, _, cur_lba, backup_lba, first_usable, last_usable) = struct.unpack_from(
        "<4sIIIQQQQ", blob, 512 + 8)
    ent_lba, num_ent, ent_sz = struct.unpack_from("<QII", blob, 512 + 72)

    print("GPT revision      : 0x%08x" % rev)
    print("header size       : %d" % hdr_sz)
    print("this LBA          : %d" % cur_lba)
    print("backup LBA        : %d   <- last LBA of the backup GPT" % backup_lba)
    print("first usable LBA  : %d" % first_usable)
    print("last  usable LBA  : %d" % last_usable)
    print("entry array LBA   : %d  (%d entries x %d bytes)" % (ent_lba, num_ent, ent_sz))
    print("total LBAs        : %d%s" % (backup_lba + 1,
          "  (exact; eMMC is usually slightly larger)" if ent_lba + (num_ent * ent_sz + 511) // 512 <= backup_lba else ""))
    # Machine-readable line for the dump script.
    print("TOTAL_LBAS=%d" % (backup_lba + 1))
    print()

    base = ent_lba * 512
    types = {
        "0fc63daf-8483-47c8-873c-7f4d54d60411": "linux-fs",
        "a2a0d0eb-e5b9-3344-87c0-68b6b72699c7": "MS basic data",
    }
    print("%-22s %-14s %-14s %-10s %s" % ("name", "start_lba", "end_lba", "MiB", "guid"))
    for i in range(num_ent):
        off = base + i * ent_sz
        if off + 128 > len(blob):
            break
        ent = blob[off:off + ent_sz]
        if ent[:16] == b"\x00" * 16:
            continue
        first, last = struct.unpack_from("<QQ", ent, 32)
        name = ent[56:128].decode("utf-16-le", "replace").split("\x00")[0]
        print("%-22s %-14d %-14d %-10.1f %s" % (
            name[:22] or "(unnamed)", first, last, (last - first + 1) * 512 / 1048576.0,
            str(uuid.UUID(bytes_le=ent[:16]))[:8]))


if __name__ == "__main__":
    main(sys.argv[1])
