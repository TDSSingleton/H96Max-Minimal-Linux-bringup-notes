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
- **IPTV provider files on the box are off-limits (user, 2026-11-22)**, the one exception being
  `ma.m3u`. That covers everything under `/var/lib/h96ui/subs/` (`<name>.m3u`, `<name>.url`,
  `<name>.xtream` with its credentials) and any other provider playlist/account file on the
  device: Claude does not read, edit, rename, move, refresh, delete or copy them off the box.
  That includes `/var/lib/h96ui/favorites.m3u` (EBI ≥ 0.7): copies of provider stream URLs,
  Xtream passwords inside.
  Tests use a scratch `H96UI_DATA` dir or `ma.m3u`, never the real subscriptions.
- **Access (user, 2026-11-22): no root access without the project's ssh key.** sshd is key-only
  (`AuthenticationMethods publickey`), root's password is `*` (never empty, never `!`: OpenSSH
  without PAM reads `!` as a locked account and refuses the key too). Change auth only behind an
  automatic revert, and prove a *fresh* key login before cancelling it.
- **Pushes to the box** (`push-to-box.sh`): only with the user's go-ahead for that build.
- Frontend name (user, 2026-11-22): the CLI frontend is **EBI (Extremely Basic IPTV)**. The
  on-screen text says EBI; the binary, `/var/lib/h96ui` and the `h96ui/` User-Agent keep their
  names so deployment and the fixup kit are unaffected.
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

Lessons from the video/player work (2026-11-21/22):

- **Test from the same environment the program runs in.** h96player worked from every ssh
  shell and failed for every channel from h96ui: busybox init gives children
  `PATH=/sbin:/usr/sbin:/bin:/usr/bin`. My tests never went through init. Programs started by
  init must not depend on the caller's PATH (h96ui now sets its own).
- **`pkill -f <pattern>` matches the shell running it.** `pkill -f mock-xtream.py` inside a
  command line containing "mock-xtream.py" killed my own shell (exit 144). Anchor the pattern
  (`pgrep -f '^python3 …'`) or kill by PID.
- **A plane is not a layer.** On VOP2, `EsmartN-win1..3` share format and scaler with `win0`;
  "it lists NV12" was true and still meaningless. Read the driver's plane names before choosing.
- **Measure each link before blaming one.** "The stream hangs" was neither the decoder, the
  display nor the WiFi: per-hop measurements (fakesink decode, LAN download, the same segment
  on the PC) put it on one remote server in four commands.
- **A deadlock needs symbols, not guesses.** Two rounds of plausible fixes (async state changes)
  missed it; Alpine's `-dbg` packages + `gdb -batch -ex "thread apply all bt"` showed the cycle
  (playbin lock vs. input-selector) in one capture. Install the debug symbols at the first hang.
- **A scripted test can wedge the user's TV.** A leftover test player kept playing for minutes
  after my fifo test "ended". After any test that starts a player: check `pidof h96player` and
  kill leftovers before reporting.
- **The safety classifier blocked sending spoofed browser headers to a third-party CDN myself.**
  The feature (honouring a playlist's own User-Agent/Referer) went in only after the user asked
  for it; the manual check was handed to the user, not routed around.

Two more from the "streaming is gone" hunt (2026-11-22, NOTES §40):

- **A fast LAN test does not clear the network path.** I had written "not the box (WiFi ~200
  Mbit/s)" from a 1 ms LAN copy; the real fault (a driver reorder stall) only bites at 74 ms. To
  clear the box's network, download *the same object from the same distant server* on the box and
  on a wired PC at the same time. Then flip one variable (the debugfs switch) and measure again.
- **The pipefail/`grep -q` trap came back** in push-to-box.sh (`tar -tzf | grep -qx`), two days
  after it was written up. Grep a here-string, every time; the lesson is now in the script.

Two more on 2026-11-22 evening:
- **A generic error message is not a diagnosis.** "Could not get/set settings from/on resource"
  is GStreamer's stock text for an element that gave none; h96player dropped the debug string
  that held the reason. Print the source element and the debug text first, then reason about
  it: the first real failure named the cause (HDMI channel mapping) in one line, where "more
  codecs?" would have been a guess.
- **`a && b` can skip an edit silently.** A failing `grep` in front of an in-place edit stopped
  the edit, the next commands still ran, and the doctor was pushed without the new checks; only
  the missing output showed it. After an edit, check that the change is actually in the file
  (or don't chain edits behind probes).
