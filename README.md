# H96 Max (RK3518): a minimal Linux TV stick

This project replaces the stock Android 14 firmware on an **H96 Max RK3518** TV stick with a
small, self-built **arm64 Linux** system. The result boots our own 64-bit u-boot and the Rockchip
vendor kernel, runs Alpine Linux, and drives a console interface on HDMI that you operate with the
stick's Bluetooth remote.

**Status:** the hardware layer is complete. The next milestone is hardware video decoding (VPU).

## Preface

This is all AI. I have some tokens on me that I use to passively do things for my family. My father always complained about IPTVs on Android so this is, in response, a minimal stack that literally contains nothing but the exact obligatory components for a device of the sort to function.
As usual with Chinese Rockchip devices, they are maybe the easiest thing to work with so I just said what the hell and gave this a whirl.
It does have something that you would maybe find interesting, and it's that I traced back a copy of a **Seekwave SWT6621S Driver** that is not upstreamed. It should actually be available in an old Neardi repo that's normally available as a submodule here.

Take control back of your devices, it is time.

## Highlights

- **Device identified from the hardware itself.** The RK3518 is an RK3528-family SKU: a quad
  Cortex-A53 whose factory firmware runs entirely in 32-bit mode. It has Samsung eMMC, a SeekWave
  SWT6621S WiFi/BT combo chip on SDIO, and a Mali-450 GPU.
- **Full eMMC backup before any write.** The stock loader silently returns filler beyond 32 MiB.
  We traced this to the read sub-code that rkdeveloptool uses, added a direct-LBA read mode to the
  tool, and verified a complete image of the device against its backup GPT.
- **64-bit mainline u-boot (v2026.07).** The factory u-boot is AArch32 and cannot start an arm64
  kernel. Our u-boot has its own board DT and a scripted AOSP boot-image path, and was tested in
  the u-boot sandbox before it was ever flashed. Its patches add two ways back to maskrom:
  `reboot loader` from Linux and the recovery button. A failed boot also falls back to maskrom.
- **Vendor 6.1 kernel, built for arm64.** Mainline has no display, HDMI or codec support for this
  SoC, so we use Rockchip's BSP kernel. The config is asserted at build time, and the kernel
  boots with the factory device tree and HDMI console.
- **Alpine Linux 3.24 root filesystem.** It is installed in the former Android `userdata`
  partition (11 GiB), is reproducible from a build script, and is reachable over SSH with key-only
  login.
- **WiFi (2.4 and 5 GHz, WPA2).** We ported the SeekWave driver from a Neardi RK3588 SDK and
  rewrote its RX descriptor handling from the ODM's shipped driver. WiFi comes up at boot.
- **Bluetooth and remote control.** We ported the `skwbt` driver and diagnosed a firmware
  codec-query crash. The stick's BLE remote pairs and survives reboots. The stick has no IR
  receiver, and CEC is unavailable.
- **h96ui console interface.** A small Go menu on a full-resolution 1080p console, operated with
  the remote.
- **Video groundwork.** We measured software playback at 0.25–0.33× realtime, which is not
  viable, and documented a plan for hardware decode (Rockchip MPP, VOP2 planes, optional Mali-450).

## Repository layout

| Path | Contents |
|---|---|
| `tools/` | Build, flash-preparation and recovery scripts; u-boot DT, config and patches (`tools/uboot/`); rkdeveloptool patch (`tools/rkdeveloptool/`); rootfs overlay; h96ui source; SeekWave WiFi/BT port as a patch on Neardi's driver (`tools/wifi/seekwave-h96max.patch`) |
| `vendor/` | Reference u-boot images. The SeekWave firmware (`vendor/seekwave/`) comes from the stick's own Android partitions and is not tracked |
| `docs/log/` | Project log: current status, bring-up journal, standing rules and lessons. Start with [`docs/log/status-and-resume.md`](docs/log/status-and-resume.md) |
| `research/notes/` | Technical notes (§1–§33). Index: [`research/notes/README.md`](research/notes/README.md) |
| `downloads/`, `research/neardi-3588-sdk-linux` | Upstream sources, tracked as **git submodules** (see `.gitmodules`) |
| `build/` | Build outputs, logs and device captures (local only, not tracked) |

Device dumps (`recon/`), keys and build artifacts are kept out of version control on purpose
(see `.gitignore`).

## Getting started

```bash
git clone --recurse-submodules <this repo>
# or, in an existing clone:
git submodule update --init --depth 1
```

Our changes to upstream trees are stored here as patches, not in the submodules. The build
scripts apply them to a clean checkout and skip any patch that is already applied. The
SeekWave driver port is recreated from the Neardi submodule by `tools/wifi/prepare-seekwave.sh`
(`build-seekwave.sh` runs it for you). The WiFi chip's firmware isn't redistributed: it has to be
extracted into `vendor/seekwave/firmware/` from the device's own Android `vendor` partition.

All builds run in containers: `fedora:41` for the kernel, u-boot, modules and h96ui, and arm64
`alpine:3.24.4` for the root filesystem. For the exact commands, see the *Rebuilding pieces*
section of [`docs/log/status-and-resume.md`](docs/log/status-and-resume.md).

## Safety rules

- **No write to the device's flash without explicit consent.** Never write sector 64 (the
  factory DDR-init loader), and never run erase, GPT or loader-upgrade commands.
- **Writes are always read back and verified.**
- **The original Android firmware must stay off the network.** Our own Linux system may connect.

## Roadmap

1. Hardware video decode: Rockchip MPP on Alpine, output to a VOP2 plane, then integration into
   the h96ui player.
2. Map the remaining remote keys (Back, Home, volume) to h96ui and player actions.
3. Fold the changes made live on the device back into a reproducible rootfs image.
4. Clean up the kernel command line, then possibly reclaim Android's leftover partitions (about
   3 GiB).
