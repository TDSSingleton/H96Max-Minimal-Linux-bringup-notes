#!/usr/bin/env python3
"""
Build the thing we actually want to flash: a 'boot'-partition image that the
STOCK u-boot will boot, carrying our kernel and our initramfs.

Why this and not a new u-boot (all of it verified offline from
recon/live-full/full.img, the 14.68 GiB capture, on 2026-11-20):

  * The factory bootloader's bootcmd is
        boot_android ${devtype} ${devnum};boot_fit;bootrkp;run distro_bootcmd;
    so it boots the `boot` partition as an Android boot image. Replacing the
    bootloader is not on the critical path.
  * vbmeta.img is an AVB0 shell: authentication-data len 0, hash-data len 0,
    zero descriptors -> AVB verifies nothing.
  * The header id[] at 0x240 IS the standard AOSP v2 mkbootimg SHA-1
    (kernel,len, ramdisk,len, second,len, recovery_dtbo_size, dtb,len) --
    reproduced for both factory boot.img and recovery.img (2026-11-21) -- and
    the stock u-boot contains "Hash from header" / "Hash real" /
    "ANDROID: sha1(...) + OK", so it computes and compares it. We therefore
    RECOMPUTE it; inheriting the factory bytes would present a mismatch.
  * `boot` = GPT LBA 0xc800, 64 MiB. Contents are backed up byte-exactly, so
    the write is undoable.

The header is a plain AOSP boot_img_hdr_v2 (tags_addr 0x20, page_size 0x24,
header_version=2 0x28, header_size=1660 0x66c, dtb_size 0x670, dtb_addr 0x674;
recovery_dtbo_size/offset = 0 for `boot`). An earlier note here called it
non-conformant; that was a misread of the offsets. We still copy the template
header verbatim and patch only the size fields and the id hash.

We keep the factory `second` (RSCE resource, 6344704 B) and the factory `dtb`
(98687 B, compatible = "rockchip,rk3518-evb1-ddr4-v10") unchanged: they are the
board description that this exact PCB shipped with, and the vendor tree we are
building has that same .dts, so we have a known-good reference either way.

This script only writes files on the PC. It prints the flash command; it never
runs it.
"""
import argparse, hashlib, os, struct, sys

ANDROID = b'ANDROID!'
F_OK, F_BAD = "\033[32mOK\033[0m", "\033[31mFAIL\033[0m"

def die(msg):
    print(f"!! {msg}", file=sys.stderr)
    sys.exit(1)

def u32(b, o):
    return struct.unpack('<I', b[o:o+4])[0]

def align(n, p):
    return (n + p - 1) // p * p

def boot_id(kern, ramd, second, dtb):
    """AOSP mkbootimg id[] for header v2 with no recovery_dtbo, padded to 32 bytes.
    Verified to reproduce the factory boot.img id 65fc3577...fda2."""
    h = hashlib.sha1()
    for blob in (kern, ramd, second):
        h.update(blob); h.update(struct.pack('<I', len(blob)))
    h.update(struct.pack('<I', 0))                # recovery_dtbo: empty, size only
    h.update(dtb); h.update(struct.pack('<I', len(dtb)))
    return h.digest() + bytes(12)

def sha256(path):
    h = hashlib.sha256()
    with open(path, 'rb') as f:
        for chunk in iter(lambda: f.read(1 << 22), b''):
            h.update(chunk)
    return h.hexdigest()

def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('--template', default='build/good/boot.img',
                    help='factory boot partition image (source of the header)')
    ap.add_argument('--kernel', required=True, help='ARM zImage / arm64 Image.gz / arm64 Image')
    ap.add_argument('--ramdisk', required=True, help='aarch64 initramfs cpio.gz')
    ap.add_argument('--second', default=None, help='resource blob; default: template\'s own')
    ap.add_argument('--dtb', default=None, help='dtb blob; default: template\'s own')
    ap.add_argument('--out', default='build/out/linux-boot.img')
    ap.add_argument('--part-sectors', type=lambda s: int(s, 0), default=0x20000,
                    help='partition size in 512B sectors (boot = 0x20000 = 64MiB)')
    ap.add_argument('--start-lba', default='0xc800', help='for the printed wl command')
    ap.add_argument('--cmdline-append', default='',
                    help='extra kernel cmdline terms, e.g. "console=tty1 loglevel=8"')
    ap.add_argument('--fbdev-on', action='store_true',
                    help='rewrite drm_kms_helper.fbdev_emulation=0 -> =1 in the dtb\'s /chosen/bootargs '
                         '(both copies: header dtb + RSCE rk-kernel.dtb, whose sha1 is updated). '
                         'u-boot appends those bootargs AFTER ours, so without this there is no HDMI console.')
    args = ap.parse_args()

    for p in (args.template, args.kernel, args.ramdisk):
        if not os.path.isfile(p):
            die(f"missing input: {p}")

    tpl = open(args.template, 'rb').read()
    if tpl[:8] != ANDROID:
        die(f"{args.template} does not start with ANDROID! -- wrong source, refusing")

    page = u32(tpl, 0x24)
    if page not in (512, 1024, 2048, 4096, 8192, 16384):
        print(f"note: template page field = {page}, using 2048 as measured from the "
              f"factory image's own payload offsets")
        page = 2048

    kern = open(args.kernel, 'rb').read()
    ramd = open(args.ramdisk, 'rb').read()

    al = lambda n: align(n, page)          # page arithmetic for this template
    # second + dtb: reuse the factory blobs unless told otherwise
    if args.second:
        second = open(args.second, 'rb').read()
    else:
        off = al(al(page + u32(tpl, 0x08)) + u32(tpl, 0x10))
        second = tpl[off:off + u32(tpl, 0x18)]
        if second[:4] != b'RSCE':
            die(f"template's `second` at {hex(off)} is not RSCE ({second[:4]!r}); "
                f"pass --second explicitly")
    if args.dtb:
        dtb = open(args.dtb, 'rb').read()
    else:
        off = al(al(al(page + u32(tpl, 0x08)) + u32(tpl, 0x10)) + u32(tpl, 0x18))
        if tpl[off:off+4] != b'\xd0\x0d\xfe\xed':
            die(f"template's dtb at {hex(off)} has no FDT magic; pass --dtb explicitly")
        dtb = tpl[off:off + struct.unpack('>I', tpl[off+4:off+8])[0]]  # FDT totalsize is BE

    if args.fbdev_on:
        OFF, ON = b'drm_kms_helper.fbdev_emulation=0', b'drm_kms_helper.fbdev_emulation=1'
        if dtb.count(OFF) != 1:
            die(f"--fbdev-on: expected exactly one '{OFF.decode()}' in the dtb, found {dtb.count(OFF)}")
        dtb = dtb.replace(OFF, ON)
        second = bytearray(second)
        n_ent = 0
        for e in range(0, 0x4000, 0x200):              # RSCE index: 512-byte ENTR records
            if second[e:e+4] != b'ENTR':
                continue
            name = bytes(second[e+4:e+0xe0]).split(b'\0')[0]
            blk, size = struct.unpack_from('<II', second, e + 0x104)
            if not name.endswith(b'.dtb'):
                continue
            body = bytes(second[blk*512: blk*512 + size])
            if body.count(OFF) != 1:
                die(f"--fbdev-on: RSCE {name.decode()} does not contain the bootargs string once")
            body = body.replace(OFF, ON)
            second[blk*512: blk*512 + size] = body
            if u32(second, e + 0x100) == 20:          # hash_size: sha1
                second[e+0xe0:e+0xe0+20] = hashlib.sha1(body).digest()
            n_ent += 1
        if n_ent != 1:
            die(f"--fbdev-on: expected one .dtb entry in RSCE, patched {n_ent}")
        second = bytes(second)
        print("dtb: /chosen/bootargs fbdev_emulation=0 -> =1 (header dtb + RSCE rk-kernel.dtb, sha1 updated)")

    limit = args.part_sectors * 512
    total = align(page, page) + align(len(kern), page) + align(len(ramd), page) \
            + align(len(second), page) + align(len(dtb), page)
    print(f"payload total {total} bytes ({total/1048576:.1f} MiB) of a "
          f"{limit/1048576:.0f} MiB partition")
    if total > limit:
        die("does not fit; shrink the initramfs/kernel or use a bigger partition")

    if u32(tpl, 0x28) != 2 or u32(tpl, 0x66c) != 1660:
        die("template is not a boot_img_hdr_v2 (header_version 2, header_size 1660)")
    if u32(tpl, 0x660) != 0:
        die("template carries a recovery_dtbo (recovery-style image); this builder "
            "only handles `boot` images with recovery_dtbo_size == 0")

    def kernel_form(b):
        if b[0x24:0x28] == b'\x18\x28\x6f\x01': return 'ARM (32-bit) zImage'
        if b[0x38:0x3c] == b'ARM\x64':         return 'arm64 Image (uncompressed)'
        if b[:2] == b'\x1f\x8b':               return 'gzip (arm64 Image.gz?)'
        return None
    kform = kernel_form(kern)
    if not kform:
        die(f"kernel starts with {kern[:8].hex()!r} -- not a zImage, arm64 Image or gzip")
    print(f"kernel form: {kform}")
    if not kform.startswith('ARM (32-bit)'):
        print("  note: the factory payload is a 32-bit ARM zImage; the stock u-boot's "
              "handling of\n  an arm64 kernel in boot_android is NOT verified on this box.")

    for name, blob, magic in (
                              ('ramdisk', ramd, (b'\x1f\x8b\x08', b'\x04\x22\x4d\x18', b'BZh', b'\x28\xb5\x2f\xfd')),
                              ('dtb', dtb, (b'\xd0\x0d\xfe\xed',))):
        if not any(blob[:len(m)] == m for m in magic):
            die(f"{name} starts with {blob[:8].hex()!r} -- not a recognised "
                f"{name}; wrong file or wrong compression")

    # ---- header: template verbatim, then patch only the fields we change ----
    head = bytearray(tpl[:page])
    struct.pack_into('<I', head, 0x08, len(kern))
    struct.pack_into('<I', head, 0x10, len(ramd))
    struct.pack_into('<I', head, 0x18, len(second))
    struct.pack_into('<I', head, 0x670, len(dtb))
    # id[] at 0x240: AOSP v2 mkbootimg SHA-1, recomputed (see module docstring).
    head[0x240:0x260] = boot_id(kern, ramd, second, dtb)
    if args.cmdline_append:
        cur = head[0x40:0x240].split(b'\x00')[0]
        new = (cur.decode('latin1') + ' ' + args.cmdline_append).encode('latin1')
        if len(new) >= 0x200:
            die(f"appended cmdline is {len(new)} bytes; the header field holds 511")
        head[0x40:0x240] = new + b'\x00' * (0x200 - len(new))

    out = bytearray(head)
    def put(blob):
        out.extend(blob)
        pad = align(len(blob), page) - len(blob)
        out.extend(bytes(pad))
    put(kern); put(ramd); put(second); put(dtb)
    if len(out) < limit:
        out.extend(bytes(limit - len(out)))

    os.makedirs(os.path.dirname(args.out) or '.', exist_ok=True)
    with open(args.out, 'wb') as f:
        f.write(out)

    # ---- verification: re-read the FILE and locate payloads by their own
    # signatures, independently of the math used above. If a check here passes,
    # a later reader can trust the bytes, not just the intent. ----
    back = open(args.out, 'rb').read()
    bs = u32(back, 0x08), u32(back, 0x10), u32(back, 0x18)
    o_k = page
    o_r = align(o_k + bs[0], page)
    o_s = align(o_r + bs[1], page)
    o_d = align(o_s + bs[2], page)
    checks = [
        ("magic",            back[:8] == ANDROID),
        ("kernel_size field", bs[0] == len(kern)),
        ("ramdisk_size field", bs[1] == len(ramd)),
        ("second_size field", bs[2] == len(second)),
        ("kernel sha256",     hashlib.sha256(back[o_k:o_k+len(kern)]).hexdigest()
                              == hashlib.sha256(kern).hexdigest()),
        ("ramdisk sha256",    hashlib.sha256(back[o_r:o_r+len(ramd)]).hexdigest()
                              == hashlib.sha256(ramd).hexdigest()),
        ("second is RSCE",    back[o_s:o_s+4] == b'RSCE'),
        ("dtb FDT magic",     back[o_d:o_d+4] == b'\xd0\x0d\xfe\xed'),
        ("dtb totalsize",     struct.unpack('>I', back[o_d+4:o_d+8])[0] == len(dtb)),
        ("header cmdline",    (back[0x40:0x240].split(b'\x00')[0] == tpl[0x40:0x240].split(b'\x00')[0])
                              if not args.cmdline_append
                              else back[0x40:0x240].split(b'\x00')[0].decode('latin1').endswith(
                                   args.cmdline_append)),
        ("dtb_size field",    u32(back, 0x670) == len(dtb)),
        ("id = sha1 of payloads (re-read)", back[0x240:0x260] == boot_id(
            back[o_k:o_k+bs[0]], back[o_r:o_r+bs[1]], back[o_s:o_s+bs[2]],
            back[o_d:o_d+u32(back, 0x670)])),
        ("addresses unchanged", all(u32(back, o) == u32(tpl, o)
                                    for o in (0x0c, 0x14, 0x1c, 0x20, 0x24, 0x28, 0x66c, 0x674))),
        ("size == partition",  len(back) == limit),
        ("tail zero-filled",   back[o_d + align(len(dtb), page):].count(0)
                               == len(back) - (o_d + align(len(dtb), page))),
    ]
    print("\nverification (re-read from the written file):")
    bad = 0
    for name, ok in checks:
        print(f"  {F_OK if ok else F_BAD:16} {name}")
        bad += (not ok)
    print(f"\noffsets: kernel {hex(o_k)}  ramdisk {hex(o_r)}  "
          f"resource {hex(o_s)}  dtb {hex(o_d)}")
    if bad:
        die(f"{bad} verification check(s) failed -- do NOT flash this")

    print(f"\nbuilt {args.out}  ({len(back)} bytes, sha256 {sha256(args.out)})")
    print(f"  kernel  {args.kernel}   {len(kern)} bytes")
    print(f"  ramdisk {args.ramdisk} {len(ramd)} bytes")
    print(f"  resource: {'--second ' + args.second if args.second else 'factory (unchanged)'}")
    print(f"  dtb:      {'--dtb ' + args.dtb if args.dtb else 'factory'}"
          f"{' + fbdev_emulation=1 in /chosen/bootargs (both copies)' if args.fbdev_on else ' (unchanged)'}")
    print(f"\nThis script does not touch the device. When you decide to write, the "
          f"command is:")
    print(f"  rkdeveloptool wl {args.start_lba} {args.out}")
    print(f"and the undo is (factory bytes, from the verified capture):")
    print(f"  rkdeveloptool wl {args.start_lba} {args.template}")
    print(f"Verify after writing, from the device, without relying on the write "
          f"to have succeeded:")
    print(f"  rkdeveloptool rl {args.start_lba} {args.part_sectors} /tmp/echo.img "
          f"&& sha256sum /tmp/echo.img {args.out}")

if __name__ == '__main__':
    main()
