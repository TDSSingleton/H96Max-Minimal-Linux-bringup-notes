# RK3518 vs RK3528: what the chip is

Part of the H96Max research notes; index in [README.md](README.md). Sections keep their original § numbers, so a `§N` reference anywhere resolves through the index.

## 2. RK3518 is an RK3528-family part, not its own chip

- No Rockchip product page for RK3518; no public datasheet trail. No `rk3518.dtsi` in
  the vendor kernel — RK3518 rides on `rk3528.dtsi`.
- Stock RK3518 box DT: `compatible = "rockchip,rk3518-evb1-ddr4-v10-r6b", "rockchip,rk3528"`.
- 4× Cortex-A53, arm64, GIC-400, PSCI. **No visible Cortex-M0** (clock/PM go through
  SCMI-over-SMC + `rk3528-dmc`; the mailbox node is `status="disabled"`). So the usual
  "A53 + M0" description of RK3518 is not evidenced on these boards.
- GPU: **Mali-450 (Utgard)** @ 300–800 MHz. Codecs: rkvdec(v2), rkvenc, vdpu, avs-plus,
  jpegd, rga2. HDMI 4K@60 + CVBS/TVE out present.
- **PCIe is unusable on RK3518** (`rk_pcie` external abort) → disable the node.
- CNX Software has **never** published an RK3518 article (`?s=RK3518` = Nothing Found)
  while RK3528 has full coverage. Only 9 GitHub repos match `RK3518`, none is an H96 Max.
- `rk3518_box_32` appears in a Hugsun FOTA device list → RK3518 boxes ship as OEM/ODM
  generics (Hugsun HS86Pro14, "Mortal T1"), **not** under the "H96 Max" name.
- RKDevTool's ChipInfo reports "RK3528" for boards whose owner calls them RK3518. The
  only reliable discriminator is the BootROM/BL31 serial banner at 1500000 baud.

**Board silkscreens the community keys on:** `M16-R6B-L3-V1.0-260121`,
`XR8223518K-V1.0`, `RK3528_DDR3_8X4_V12`. Get the user's marking.

## 11. RK3518 vs RK3528 — resolved from Rockchip's own repo, not forum writeups

The user challenged the RK3528 naming ("our device is a 3518, no?"). Right challenge: my
"same family" claim had been resting on self-published, partly AI-declared community
writeups. Checked it against primary sources instead, and it holds — plus it produced two
corrections and one much better method.

**RK3518 has no software line at all.** In `rockchip-linux/rkbin@3e288fe`, `bin/rk35/` holds
**132 files, zero matching "3518"**, while `rk3528` supplies `bl31`/`bl32`/`ddr`/`mcu`/`spl`/
`usbplug`. Rockchip's own `tools/ddrbin_tool.py` recognises exactly three chips:
**`rk3528`, `rk3538`, `rv1126b`** — there is no `rk3518` key to pass. Combined with the
absent `rk3518.dtsi` and the `compatible = "rockchip,rk3518-evb1-...", "rockchip,rk3528"`
declaration, this means the RK3528 names in `tools/build-loaders.sh` (blob paths *and*
`[CHIP_NAME] NAME=RK3528`) are **mandatory, not a chip mix-up**.

The chip is still real and distinct: SoC id **`0x35181001`**, and BL31 **v1.20 prints
"RK3518 SoC"**. Coherent story: RK3518 = RK3528 silicon with its own ID code and features
downbinned off (PCIe aborts and must be disabled), served entirely by the RK3528 BSP.
Operationally: **software names cannot arbitrate 3518-vs-3528** — RKDevTool reportedly says
"RK3528" for boards reading "RK3518". The ID register (serial banner, or derivable from the
dump) is the only arbiter.

**Correction to §2 (my "no Cortex-M0" claim).** `rk3528_mcu_v1.00.bin` exists in rkbin, so
an MCU is part of the family; the RK3518 box device tree simply doesn't expose it. My
earlier "do not assume an M0" was an over-claim from a DT-only search.

**Correction to §10 (loader selection).** The DDR blobs are keyed on **PCB layout, not RAM
type**. `ddrbin_tool.py rk3528 -g` readback of the three multi-type blobs:

| blob | banner | ddr3 | lp3 | ddr4 | lp4/4x |
|---|---|---|---|---|---|
| `rk3528_ddr_1056MHz_4BIT_PCB_v1.14` | `DDR c097eb3447 huan.he 26/03/19-18:52:38, fwver: v1.14 4bit PCB` | **786** | 1056 | 1056 | 1056 |
| `rk3528_ddr_1056MHz_2L_PCB_v1.14` | `…18:52:37, fwver: v1.14 2L PCB` | 1056 | 1056 | 1056 | 1056 |
| `rk3528_ddr_1056MHz_v1.14` | `…18:52:39, fwver: v1.14` | 1056 | 1056 | 1056 | 1056 |

All three report `first_init_dram_type=3` and cover DDR3/DDR4/LPDDR3/LPDDR4 in one blob
(the tool's own comment: *"backward compatible, supports DDR3, DDR4, LPDDR3"*). RAM-type-
specific blobs (`D3_LP3`, `D4`, `LP4_4X`) exist **only as `_eyescan_` diagnostics**. So
§10's "DDR4 → prebuilt, LPDDR3 → the 780 loaders" framing was wrong; **4BIT_PCB vs 2L_PCB**
is the axis, and both documented RK3518 boards report **"4bit PCB"**.

**The method this unlocks (better than try-in-order).** `-g` prints a banner in *exactly*
the format a factory dump's banner has:
```
DDR c097eb3447 huan.he 26/03/19-18:52:38, fwver: v1.14 4bit PCB   <- public blob
DDR 56f70fd2ad huan.he 25/06/11-11:04.04, fwver: v1.11 4bit PCB   <- a real box's sector 64
```
So after the recon dump: `strings factory_idbloader.bin | grep -oE 'DDR .*fwver.*'`, compare
against `tools/loaders/ddr-banners.txt`, and pick the closest public blob by fwver + PCB
string. Deterministic, and it works with the device in a drawer.

`rk3528_bl31_v1.21.elf` is in rkbin — clears the documented `bl31 >= v1.19` floor for later
`u-boot.itb` work (Maskrom loaders don't carry BL31 at all; DDR+usbplug+SPL only).

`tools/build-loaders.sh` rewritten: builds **L0** generic / **L1** 4BIT_PCB / **L2** 2L_PCB /
**L3** lp3_780 (v1.11, v1.14) / **L4** the X88PRO prebuilt, prints every blob's banner, and
step 4 **byte-diffs our packed header against the known-good RK3518 loader** — because the
name→chip-id mapping lives inside the closed-source `boot_merger` and cannot be read from
source (`boot_merger.h` only defines the `[CHIP_NAME]` INI key). Bench order: L4 → L1 → L0 →
L3 → L2. Fixed two self-inflicted bugs before handoff: a `WANT` entry with an empty commit
(would have built a bad URL) and a `sha2566sum` typo whose failure was being masked by `tee`
returning success through `||`.

**Still not verified:** whether `boot_merger` emits a distinct chip id for RK3518 vs
RK3528, i.e. whether the L-series headers will match L4 byte-for-byte in the tag field.
Section 4 answers that on first run.
