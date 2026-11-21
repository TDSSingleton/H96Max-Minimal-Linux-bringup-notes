#!/usr/bin/env python3
"""Carve Flattened Device Trees out of Rockchip Android images and decompile them.

Why: an RK3518 TV stick ships no schematic and no mainline .dts. The authoritative
description of what is physically on YOUR board -- how much RAM, whether there is an
ethernet PHY, which WiFi chip hangs off which SDIO bus, whether there is an IR receiver,
which GPIO drives the HDMI 5V -- is already sitting in its flash, inside dtbo / boot's
resource section. Extracting it turns "this device is barely anything" from a guess into
a parts list, and gives the base to edit for a Linux port.

Usage: python3 -I carve-dtb.py <image> [<image> ...]
Output: dtb-out/<basename>-<n>.dtb and .dts
"""
import os
import struct
import subprocess
import sys

MAGIC = b"\xd0\x0d\xfe\xed"


def carve(path, outdir):
    data = open(path, "rb").read()
    base = os.path.basename(path)
    found = []
    pos = 0
    while True:
        i = data.find(MAGIC, pos)
        if i < 0:
            break
        pos = i + 4
        if i + 8 > len(data):
            continue
        totalsize = struct.unpack_from(">I", data, i + 4)[0]
        # A plausible DTB: sane size, and struct_size/offs fields inside the header.
        if not (256 < totalsize <= 4 * 1024 * 1024) or i + totalsize > len(data):
            continue
        blob = data[i:i + totalsize]
        # sanity: version field must be small
        if struct.unpack_from(">I", blob, 28)[0] > 20:
            continue
        n = len(found) + 1
        out = os.path.join(outdir, "%s-%02d.dtb" % (base, n))
        with open(out, "wb") as fh:
            fh.write(blob)
        found.append((out, totalsize))
        pos = i + 4  # allow overlapping/nested trees (Android DTBO holds several)
    return found


def decompile(blobs):
    rows = []
    for dtb, size in blobs:
        dts = dtb[:-4] + ".dts"
        try:
            subprocess.run(["dtc", "-I", "dtb", "-O", "dts", dtb, "-o", dts],
                           check=True, capture_output=True)
        except Exception as exc:
            err = getattr(exc, "stderr", b"") or str(exc).encode()
            print("  dtc failed on %s: %s" % (os.path.basename(dtb), err.decode()[:200]))
            continue
        rows.append((dtb, dts, size))
    return rows


def summarize(dts):
    txt = open(dts, "r", errors="replace").read()

    def grab(pattern):
        import re
        return re.findall(pattern, txt)

    model = grab(r'model\s*=\s*"([^"]+)"')
    compat = grab(r'compatible\s*=\s*((?:"[^"]+"(?:,\s*)?)+)')
    mem = grab(r'memory@[0-9a-f]+\s*\{[^}]*reg\s*=\s*<([^>]+)>')
    print("    model      : %s" % (model[:3],))
    print("    compatible : %s" % (compat[:4],))
    for m in mem[:4]:
        parts = [int(x, 16) if x.startswith("0x") else int(x) for x in m.split()]
        if len(parts) >= 4:
            print("    memory     : base=0x%x size=%.2f GiB (%d MiB)" % (
                (parts[0] << 32) | parts[1], parts[3] / 2**30, parts[3] // 2**20))
        elif len(parts) == 2:
            print("    memory     : base=0x%x size=%.2f GiB (%d MiB)" % (
                parts[0], parts[1] / 2**30, parts[1] // 2**20))
    for key, pat in (
        ("ethernet/phy", r'ethernet0|stmmac|snps,dwmac|realtek,r tl|rtl82|rlnx,rtl8211|ethernet@'),
        ("wifi/sdio",    r'(brcmf|ap62|aic|swt6621|syn,|seekwave|wifi|sdio-wifi|rtl88|svan)'),
        ("ir remote",    r'(rockchip,cir|ir-receiver|pwm-ir|gpio-ir|rc-map)'),
        ("bluetooth",    r'(bluetooth|uart[0-9]-bt|brcm,bt|rtk_hci)'),
        ("hdmi",         r'(dw-hdmi|hdmi-sound|display-subsystem|cec)'),
    ):
        hits = sorted(set(x.strip() for x in grab(pat)))
        if hits:
            print("    %-12s %s" % (key, ", ".join(hits)[:150]))
    return txt


def main(paths):
    outdir = "dtb-out"
    os.makedirs(outdir, exist_ok=True)
    for p in paths:
        if not os.path.isfile(p):
            print("skip (missing): %s" % p)
            continue
        print("\n########## %s (%d bytes)" % (p, os.path.getsize(p)))
        blobs = carve(p, outdir)
        if not blobs:
            print("  no DTB magic found")
            continue
        print("  carved %d candidate tree(s)" % len(blobs))
        for dtb, size, in blobs[:12]:
            print("    %-40s %9d bytes" % (os.path.basename(dtb), size))
        rows = decompile(blobs[:12])
        for dtb, dts, size in rows:
            print("\n  --- %s" % os.path.basename(dts))
            summarize(dts)


if __name__ == "__main__":
    main(sys.argv[1:])
