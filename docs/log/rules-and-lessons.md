# Standing rules and process lessons

Part of the session log (the narrative: what happened and why); index in [README.md](README.md). Technical detail is in `research/notes/`.

## 10. Standing rules on this project

- **No writes to the device without explicit user agreement.** `db`/Download Boot is RAM-only
  and has been accepted; nothing has ever written flash. The write now on the table is
  `wl 0xc800` (the 64 MiB `boot` partition); the factory bytes for undo are already on disk at
  `build/good/boot.img` (sha256 of the whole 64 MiB region is reproducible from
  `recon/live-full/full.img`). ~~`wl 0x4000` (`uboot`) is no longer needed~~ **[superseded 2026-11-21]**: with the arm64
  kernel, `wl 0x4000` of `build/uboot/u-boot.itb` is needed too (NOTES §22/§23); undo bytes at
  `recon/20261119-1645/partitions/uboot.img`. Still only with explicit consent.
- **Never**: `rkdeveloptool ef`, `ul` (=UpgradeLoader, *writes the loader to flash* — sounds
  like a RAM upload and is the single most likely brick), `wl 0x40`, `gpt`, `prm`, `tagspl`;
  never `armbian-install`.
- **Network rule (user, 2026-11-21): keep the box's original Android off the network.** It
  does not apply to our own Linux, which we fully control; WiFi/Ethernet under our kernel is
  fine. Restoring the factory `boot`/`uboot` brings Android back, so after an undo: no network.
  Re-entry to maskrom: button at power-on (with u-boot patch 0002), `reboot loader` from our
  Linux, or the pads under the heatsink.
- Kernel target arch: **arm64 — decided by the user 2026-11-21.** Consequence (NOTES §22): the
  stock u-boot is AArch32 and has no path to AArch64, so the `uboot` partition must get a
  64-bit u-boot FIT. That write is **not agreed yet** and reverses the 2026-11-20 plan below.

## 11. Process lessons, earned the hard way

Five times tonight a check of mine validated **less than it claimed to prove**: the double-read
gate (only covered the region that already worked → declared a 15 GB dump trustworthy);
`gsda` as a bare 4-byte substring against compressed data (announced malware evidence, then 0
hits with context — retracted); "no `rk3518.dtsi`" (searched one arch dir and generalized);
the `PyString_FromString` single-symbol patch (should have enumerated the class); `scripts/config
--enable` on invented symbol names (silent no-op read as success). The fixes that stuck:
structural checks that cannot be satisfied by filler (backup-GPT CRC at the last LBA), context
grep before asserting, enumerating the whole population before patching one symptom, and
asserting requested state rather than requesting it.

Three more on 2026-11-20, all from the busybox/initramfs work, and all of them a check lying
about the artifact rather than the artifact being wrong:

- **`echo "$list" | grep -q pat` under `set -o pipefail`.** `grep -q` exits at the first match,
  the upstream `echo` dies of SIGPIPE (141), and pipefail makes *that* the pipeline's status —
  so the check reports every match found **early** as a miss. It told me 34 of 39 busybox
  applets were absent from a binary that had all 39, and the five it called present were
  exactly the alphabetically last ones (`umount wc uname usleep xargs`); that pattern is what
  gave the mechanism away instead of blaming the artifact. Fix: here-strings, which have no
  upstream process. The same bug was in three of my checks.
- **`find -name busybox | head -1` as a way to locate a binary.** Debian's `busybox-static`
  contains six paths ending in `busybox`; traversal order returned
  `usr/share/initramfs-tools/conf-hooks.d/busybox`, a 16-byte config snippet, which got copied
  out as our shell. Selecting by "biggest match" would have passed today and failed on the next
  package. Select on what the file *is* (`file -b`), and print every candidate with mode when
  none match.
- **My own repair turned a working check into a broken one.** The original
  `cpio -t | grep -qx 'init'` was correct; I replaced it with `'^\.?/+init$'` on the belief that
  GNU cpio lists names as `./init` — it strips the `./`, and `/+` then demanded a slash, so a
  good initramfs was rejected. Stated as a rule: a comment asserting what some tool's output
  looks like is not evidence. Run the tool, read the bytes, then write the pattern.
