# Original goal (GammaOS) and what firmware exists

Part of the H96Max research notes; index in [README.md](README.md). Sections keep their original § numbers, so a `§N` reference anywhere resolves through the index.

*Original framing (2026-11-19, before the device was connected): root the box, then install GammaOS. Superseded by the retarget to minimal Linux (§12).*

## 1. GammaOS does not support this device — VERIFIED MYSELF

Pulled the full repo list of `TheGammaSqueeze` (100 repos, 2026-11-19). Every GammaOS
branch targets **retro handhelds**, not TV boxes:

| Branch | What it is | Silicon |
|---|---|---|
| `GammaOS` (archived 2026-05-02) | LOS 19.1 / A12 for Anbernic | Unisoc T618 |
| `GammaOS-RK3566` (archived) | LOS for Anbernic RK3566 handhelds | RK3566 |
| `GammaOSCore` (beta3, 2025-02) | desc: *"A lightweight Android TV 13 based distribution **for handheld gaming consoles**"* | RK3566, Allwinner A133P |
| `GammaOSNext` (active, Nano v1.4.4) | *"Android 14 **GSI** distribution tailored for **handheld** gaming consoles"* | Unisoc T820, MTK, Allwinner A523/A527 |

Device matrix is ~30 named handhelds (RG557/RG477/RG Cube/RG405M/TrimUI Brick/MagicX
Zero 40/GKD 350H/AYANEO Pocket/Retroid Pocket 4 Pro/KT-R1/ZPG A1/GameMT E6 Max...).
Zero TV boxes. GitHub issue search `org:TheGammaSqueeze RK3518 OR "TV box" OR RK3568`
→ 0 results. Hackaday search → nothing.

Install methods it uses: whole-disk Rockchip SDBoot `.img` to microSD (SDDiskTool /
RKDevTool), Unisoc `.pac` via ResearchDownload, MTK SP Flash, `fastboot flashing unlock`
+ FlashPartitions scripts. Root is pre-included (Magisk). ~1.5–2.6 GB images, 32 GB A2
microSD recommended, SD-boot keeps eMMC untouched.

Interesting adjacent fact: `GammaOSNextDistribution-14` ships `buildtv.sh` which builds
`lineage_tv_arm64_bvN` — a **PHH-Treble Android TV GSI** (LineageOS 21 / A14), branded
"GammaOS Core". So GammaOS *is* generated as a generic arm64 TV GSI upstream. That GSI
concept could in principle boot on a Treble-compatible arm64 TV box, but that is **my
inference**, not a supported GammaOS configuration; it would depend on the box's vendor
HALs (Mali-450 + HIDL Composer 2.1) and on the box userspace being arm64 rather than the
32-bit `rk3518_box_32` variant.

If the user's actual want is "retro gaming box", the TV-box ecosystem is EmuELEC /
Batocera / ArkOS-style Linux images, or just RetroArch + Daijisho + ES-DE on stock Android.

## 7. Firmware availability — the real blocker

- **No stock `update.img` for RK3518 boxes is publicly available.** "Original Android
  firmware? I could not find any." kxn's project explicitly refuses to publish
  原厂完整固件 (full factory firmware), userdata, BT keys or OEM signing keys. Only
  `factory_idbloader.bin` is hosted anywhere. `h96max.com` is unreachable (HTTP 522).
- Beware aggregator name collisions, documented first-hand: the NeedROM "Mortal T1" entry
  is actually an **Allwinner H313** image (`IMAGEWTY` header, ~1800 `sun50iw9` strings) —
  wrong SoC entirely.
- Community paths that exist for RK3518 hardware:
  - `github.com/kxn/rk3518-atv` — AOSP **14 arm64 Android TV** port for RK3518/Mali-450/
    1.5 GB LPDDR3/AIC8800D80. userdebug, dev test-keys, **SELinux permissive**, recovery
    partition *not fixed*, Wi-Fi **ADB 5555 with no screen authorization** on (their choice
    for a home dev box; a liability for us). Published as `source-preview-v8`: the author
    states a clean build from scratch and a cold-boot acceptance test of the resulting
    images are **not done**; shipped v7 super SHA256 `75f509fc…a28e`, v8 package
    `8b1b6ed4…e6f8`. Target use is AllStream casting, not gaming.
  - `github.com/juliovendramini/rk3518_armbian` — Armbian bring-up + the DDR/idbloader
    survival guide. Note: several of these writeups self-declare as AI-assisted.
  - `github.com/ohaiibuzzle/armbian-tv98-rk3518` — Hugsun X88PRO RK3518 Armbian SD image
    + maskrom loader + restore kit (eMCP was already ~70% worn).
  - RK3528 H96 Max (different chip, same family): `superromchik1/h96max-rk3528-armbian`,
    `Daninet/rk3528-h96max-armbian`, `jwagnervaz/RK3528-H96MAX-ANDROIDTV13PORT`.
