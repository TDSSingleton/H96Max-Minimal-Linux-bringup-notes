# Recon runbook — H96Max / RK3518

Read-only plan of record. Nothing here writes to the device, touches the network, or
requires the stock OS to boot. Print this and work at the bench.

## The governing idea

**Maskrom is the safest possible state to be in with this device.** Maskrom is the SoC's
unmodifiable BootROM with no RAM initialised. Android never runs, so the network stack,
the OTA client and any implant stay asleep. A "loader" uploaded from the PC runs in RAM
only — it is *not* written to flash. You can therefore read the entire eMMC with zero
runtime on the target OS and zero network exposure, and get three things in one pass:

1. your only backup of the board-tuned DDR blob (sector 64) — the anti-brick insurance
2. the forensic image to analyse the compromise claim properly
3. `boot.img`, which is what we would patch for root

## Network policy for the whole recon phase

Nothing in phases 0–4 requires a network. Keep the box's WiFi unprovisioned and no
ethernet cable attached. If a later phase needs ADB over the box's network stack, it
happens on a dedicated NIC in hostapd AP mode or a direct cable to an unrouted interface
with no gateway and no DHCP — never the LAN. Note this device family's stock firmware is
claimed to run a bandwidth-selling PCDN client, so "just letting it sync for a minute" is
the one habit to break now.

## Phase 0 — parts on the bench

| Item | Why |
|---|---|
| **C-to-C or A-to-C cable — probably sufficient** | This unit is powered through a **Type-C** jack, and a C socket that carries 5 V almost always carries the USB 2.0 D+/D− pair too. If so, one cable into the PC gives power *and* the flashing link, and the male-to-male adapter below is not needed. Try this first. |
| USB **male-to-male** (A↔A) cable — fallback | Only if the C jack turns out to be power-only. The flashing controller is an OTG port that these boards often wire as *host* and populate with a Type-A socket, and a normal cable will not let your PC be the host of a device that thinks it is the host. Standard practice here, but see the back-feed note in Phase 2 route B. |
| A second, short USB cable (A↔C / A↔micro) | In case the OTG socket is a smaller connector, or wired as device-side. |
| Something to short two pads: tweezers or a probe | Maskrom entry. Insulated tip preferred. |
| Bright light + macro phone camera | Silkscreen reading, and pad identification. |
| ≥ 40 GB free on the workstation | Full eMMC image (8 GB nominal, but these parts report ~7.3 GiB usable; userdata dump dominates). |
| Later, if we write: 1.8 V-tolerant USB-UART | These GPIO banks are a **1.8 V domain** at **1500000 8N1**. A 3.3 V adapter gave one person unreadable output. Not needed for recon. |

Before plugging anything in, turn off automount on the workstation. In Maskrom the device
exposes only a vendor-specific bulk endpoint (no mass storage, no HID, no MTP — so no
BadUSB keystroke path and nothing to auto-open), but once the box boots its own OS over
that cable it *can* present MTP/ADB. Cheap insurance, GNOME:

    gsettings set org.gnome.desktop.media-handling automount false
    gsettings set org.gnome.desktop.media-handling automount-open false

KDE Plasma: System Settings → Removable Storage → uncheck "Mount automatically".
Headless is fine too — udisks2 only automounts for a logged-in graphical session.
Tell me your DE if you want this set precisely rather than guessed at.

## Phase 1 — identify the board (power disconnected)

Open it and record, with photos:

1. **SoC silkscreen.** Read it under raking light. RK3518 vs RK3528 matters less than you
   would think — they are the same family and RKDevTool has reported "RK3528" for boards
   whose owners read "RK3518" — but it is the anchor for everything else.
2. **PCB marking.** The three markings the community keys on are
   `M16-R6B-L3-V1.0-260121`, `XR8223518K-V1.0`, `RK3528_DDR3_8X4_V12`. If yours matches
   one, the Maskrom advice for that board transfers directly.
3. **RAM chips.** Marking + count → LPDDR3 (1 GB / 1.5 GB, "4BIT PCB") vs DDR4 vs DDR3 8x4.
   This decides which loader to build first. DDR4 → X88PRO-style prebuilt.
   LPDDR3 → the 780 MHz LPDDR3 loaders.
4. **eMMC marking.** Also tells you the wear risk: one teardown found these eMCP parts at
   **~70% of lifecycle**, apparently harvested from older phones.
5. **WiFi/BT module.** `AIC8800D40` (enumerates as D80, SDIO id `c8a1:0082`) or
   `SWT6621S` (SeekWave) ⇒ this is the documented RK3518 generation.
   `AP62xx`/Broadcom ⇒ older/different SoC generation; re-examine the chip ID.
6. **Ports actually populated** — count USB sockets, are any labelled OTG, is there an
   RJ45, a microSD slot, an AV jack?
7. **Reset button** — look for a recessed pushbutton *behind the AV/headphone jack*; that
   hiding place is documented on this chassis family.
8. **Two pads under the main heatsink.** Directly reported for this board: "the board is
   the same as mine — you have to remove the heatsink, you will see the two pins."
   Photograph them before disturbing the heatsink.

Also note whether ethernet exists at all: it is per-board, not per-SoC. Some variants have
no RJ45 fitted; `gmac0` on this family is RMII-only (100 Mbit ceiling) while some revisions
have an RGMII PHY on `gmac1`.

## Phase 2 — enter a flashing mode

### First: which mode did you actually get? (this decides everything downstream)

`rkdeveloptool` decides maskrom-vs-loader by one test only — `RKScan.cpp` reads
`bNumConfigurations & 0x1`: **0 configs = Maskrom** (BootROM, no RAM, needs a loader
uploaded), **1 config = Loader** (a loader is already running, ready to read). You can see
the same thing straight off sysfs, so check this before anything else:

    lsusb -d 2207:                                  # is it there at all?
    lsusb -d 2207: -v | grep -E 'idVendor|idProduct|iProduct|bNumConfigurations'
    rkdeveloptool ld                                # tool's own verdict

| What you see | State | Next step |
|---|---|---|
| 2207 present, `bNumConfigurations 0` | **Maskrom** | `rkdeveloptool db <loader.bin>`, wait 3 s, it re-enumerates as Loader |
| 2207 present, `bNumConfigurations 1` | **Loader** | **Best case — go straight to `tools/dump-emmc.sh`. No loader upload, no DDR-blob guessing, because the board is running its own factory loader, which by definition trains your RAM correctly.** |
| Nothing on USB | Not in any download mode | Try the other port / other cable / the pads |
| Presents mass storage | It booted something with UMS | **Do not mount it.** Power off, use another route |

Arriving at **Loader** directly is strictly better than arriving at Maskrom: it skips the
whole "which public DDR blob trains your board" problem, and it means the reads come
through the vendor's own code path.

### Routes, easiest first. Stop at the first that works.

**A. Power from the PC over the USB-C jack, holding the reset button.** If this box is
powered through its Type-C socket, that connector very likely carries the USB 2.0 data
pair too — meaning **one C-to-C (or A-to-C) cable between PC and the power jack gives both
power and the flashing link, and no male-to-male adapter is needed.** Sequence: cable
disconnected → hold reset → plug into the PC → keep holding ~10 s → run the check above.
Caveat: a PC USB-C port supplies 5 V only. If the box expects 12 V (via PD negotiation) it
will not come up from a port — no harm, just nothing appears. Maskrom draws little, so 5 V
is often enough even for a box that wants more when running Android.

**B. Its own supply for power + the Type-A port for data, holding reset.** This is the
classic route on these boxes and needs the male-to-male cable if the socket is Type-A.
Honest risk note, because it cuts against my usual advice: an A↔A cable ties two 5 V rails
together. It is standard practice in this scene and the box draws almost nothing in
maskrom, but a cheap board can back-feed into your PC. If you have a hub with its own
supply, or a spare PCIe/front-port you do not mind, prefer that.

**C. Short the two pads under the heatsink** for 2–3 s with power applied, then release.
Directly reported for this board family. This is the route that definitely reaches BootROM
maskrom rather than relying on whatever the button is wired to.

**What the reset button probably is, and why it still helps.** A button a user can reach is
normally sampled by **U-Boot** as *recovery*, not by BootROM as *maskrom* — BootROM's
maskrom input is the internal test pad. So route A/B may well land in Android **recovery**
rather than a flashing mode. That is still a win: recovery is a minimal ramdisk, and from it

    adb devices && adb reboot loader

requests the BootROM download path (`syscon-reboot-mode`, `mode-bootloader = <0x5242c309>`
is present in this chip's DT). That gets you to Loader **without booting the full OS**.
Check `adb devices` alongside the `lsusb` test above — an ADB serial means recovery, not a
download mode.

**D. Last resort — boot stock Android, air-gapped.** If nothing above works, booting the
compromised OS with no ethernet and WiFi never joined is a defensible fallback, not a
forbidden one: the reported implant (bandwidth-selling PCDN client + OTA poller) needs an
uplink to do anything, and with no network it gets none. Residual risk is local only — it
can write its own flash and it can sit on a network once you later connect one, which is
why this route ends in `adb reboot loader` and a reflash rather than in "leave it running."

What a successful enumeration looks like:

    lsusb -d 2207:
    Bus 001 Device 0NN: ID 2207:350b Rockchip Electronics ...

The product id is a guess until we see it — the RK3518/RK3528 Maskrom PID is **not
documented** anywhere we found; upstream's rules stop at `0x350a` (RK3566) and `0x350b` is
only confirmed for RK3588-class parts. Match on vendor `2207`, not on product. That is why
`tools/99-rockchip.rules` keys on the vendor id.

If nothing enumerates: try the other USB socket, then the A↔A cable, then the other
direction of the cable, then route B. Also try a USB 2.0 port on the PC with no hub in
between — ASMedia hubs and front-panel headers are a known nuisance for Maskrom.

## Phase 3 — read everything

    tools/build-loaders.sh        # once; fetches+hash-checks rkbin, packs 4 loaders
    tools/dump-emmc.sh            # probe units -> double-read verify -> full image

Two gates inside `dump-emmc.sh` that you must not skip or patch around:

- **the `rl` units probe** — a common Debian/pine64 build of rkdeveloptool counts *bytes*
  where upstream counts *sectors*. Silent truncation is how you end up with a useless
  backup you believe is good.
- **the double-read hash comparison** — marginal DDR training returns corrupt data with no
  error. If the two hashes differ, that loader is untrustworthy and you try the next one.
  A corrupt backup of sector 64 is worse than no backup, because you would trust it later.

If `pt` fails, you are in Maskrom without a loader. `rkdeveloptool db <loader.bin>`, wait
three seconds, rerun. A loader that cannot train your RAM costs a replug, nothing more.

### The verb list — read this before typing anything

Semantics taken from `usage()` in rkdeveloptool's `main.cpp`, not from memory or forum
posts:

| Verdict | Verbs | What they do |
|---|---|---|
| **Safe, RAM only** | `db <loader>` | DownloadBoot — runs the loader in RAM. Never touches flash. Power off and it is gone. |
| **Safe, read only** | `ld` `rci` `rid` `rfi` `rcb` `ppt` `rl` | enumerate, chip info, flash id, flash geometry, capability, partition table, read LBAs. |
| **NEVER** | **`ul <loader>`** | **UpgradeLoader — writes the loader INTO FLASH.** It reads like "upload loader to RAM". It is not, and it is the most likely single way to permanently brick this box: it overwrites the sector-64 idbloader that holds the only copy of the board-tuned DDR init. |
| **NEVER** | `ef` | EraseFlash. |
| **NEVER** | `wl` `wlx` `gpt` `prm` `tagspl` | WriteLBA / write-by-partition-name / WriteGPT / WriteParameter / TagSPL. |
| **AVOID** | `td` | TestDevice — exercises the media. Unnecessary on eMMC already reported at ~70% write lifecycle. |

Also never `armbian-install` (writes an older loader; the X88PRO author's own warning).

Why `ef` and `ul` are unrecoverable here rather than merely risky: they destroy the
sector-64 idbloader, the only copy of the tuned DDR init that exists for this board. Public
rkbin DDR blobs either fail write-training (`wrtrn err`) or train *marginally* and then
crash with a Synchronous Abort whose faulting address is ASCII text. One person wrote
mainline U-Boot to a box of this family and it never enumerated again — red LED, no HDMI,
"No Devices Found" in Maskrom. That is the actual risk on this device, not the malware.

**Script bug found and fixed while writing this:** my first `dump-emmc.sh` called
`rkdeveloptool pt`. There is no `pt` verb in this build — it is `ppt` (PrintPartition). It
would have failed and masqueraded as "you are not in loader mode", sending us chasing
Maskrom entry forever. The script now uses `ppt`.

`rci` is worth running first: it asks the device what chip it thinks it is. Expect it to
answer **RK3528 even on genuine RK3518 silicon** (RKDevTool does exactly that), so treat it
as family confirmation, not SKU proof.

## Phase 4 — analysis, on the workstation, offline

Send me these and I will do the rest:

    recon/<ts>/meta/lsusb.txt            recon/<ts>/meta/gpt.txt
    recon/<ts>/meta/partition-table.txt  recon/<ts>/SHA256SUMS.txt
    the log tail of tools/build-loaders.sh   (it prints each DDR blob's real config)

What we then determine, all from the image:

1. **chip identity + DDR type/frequency** from the idbloader's DDR banner string
   (e.g. `DDR 56f70fd2ad huan.he 25/06/11-11:04.04, fwver: v1.11 4bit PCB`) — this is the
   decisive answer to RK3518-vs-RK3528 and to which loader is *provably* right for you.
2. **bitness** — `ro.product.name` and the ELF headers inside `super` tell us whether this
   is the 32-bit `rk3518_box_32` build or arm64. This gates the entire GammaOS-GSI idea.
3. **boot.img format** — `unpack_bootimg` on the extracted `boot` partition resolves the
   header version and ramdisk compression (gzip/lz4/lz4o), i.e. whether "fix the ramdisk"
   is even the right shape of root, and whether AVB/dm-verity is in play.
4. **ODM and implant indicators** — `getprop`-equivalent strings in `system`/`vendor` for
   `persist.box.customer`, `ro.product.ota.host`, plus `strings`/path search for
   `gsdnode_android`, `/data/gsdnode`, `/data/blueberry`, `/data/.lotso`, `niulink`,
   `wxedge`, `hgsun`/`hugsun`, `ota.rupa.tv`, `tws.gsda.com.cn`. `userdata` is where a
   live implant would actually be running from, and we have it.
5. **partition map** for the eventual root/reflash step.

## Phase 5 — then, and only then, choose

The fork we deferred: root-in-place and game on stock / replace boot+super with something
we built / try the GammaOS TV GSI / write off the box. The dump answers which of those are
physically possible on your silicon and how compromised the firmware actually is, which is
the information the decision needs.
