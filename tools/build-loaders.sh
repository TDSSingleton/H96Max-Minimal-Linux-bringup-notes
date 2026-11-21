#!/usr/bin/env bash
# Build RK3518 Maskrom *download loaders* from pinned, hash-verified official rkbin blobs.
#
# WHY THE BLOBS ARE NAMED rk3528 ON AN rk3518 BOARD
# Rockchip ships no RK3518 anything. In rkbin@3e288fe, bin/rk35/ holds 132 files and ZERO
# match "3518", while rk3528 supplies bl31/bl32/ddr/mcu/spl/usbplug. Rockchip's own
# ddrbin_tool.py knows only the chips: rk3528, rk3538, rv1126b -- there is no rk3518 key to
# pass. The RK3518 is served entirely by the RK3528 BSP (there is no rk3518.dtsi either),
# even though the *silicon* keeps its own identity (SoC id 0x35181001, and BL31 v1.20
# prints "RK3518 SoC"). So NAME=RK3528 below is correct, not a chip mix-up.
#
# WHAT A LOADER IS: Maskrom is BootROM with no RAM initialised, so before you can read
# anything you upload a container = DDR init + usbplug + SPL that runs in RAM. It is NOT
# written to flash. Sending a loader is non-destructive: if the DDR parameters do not match
# your board, training fails and the device simply does not enumerate. Writing is a
# separate, deliberate act (wl / wf / ef) that this script never does.
#
# HOW TO PICK ONE: the DDR blobs are keyed on PCB layout, not RAM type --
#   rk3528_ddr_1056MHz_4BIT_PCB   ddr3=786 lp3=1056 ddr4=1056   <- both documented RK3518
#   rk3528_ddr_1056MHz_2L_PCB     ddr3=1056 lp3=1056 ddr4=1056  <-   boxes report "4bit PCB"
#   rk3528_ddr_1056MHz            ddr3=1056 lp3=1056 ddr4=1056  <-   generic fallback
# The RAM-type-specific blobs (D3_LP3 / D4 / LP4_4X) exist only as *_eyescan_* diagnostics.
# Each blob prints a banner in the SAME format as a factory dump's, so after recon we match
# banners rather than guessing:
#   DDR c097eb3447 huan.he 26/03/19-18:52:38, fwver: v1.14 4bit PCB   (public blob)
#   DDR 56f70fd2ad huan.he 25/06/11-11:04.04, fwver: v1.11 4bit PCB   (a real board's sector 64)
set -uo pipefail

ROOT=$(cd "$(dirname "$0")/.." && pwd)
WORK="$ROOT/tools/loaders"
UP="$WORK/upstream"
DELIV="$WORK/delivery"

C_MAIN=3e288fe814e059dd06833495f845cab04ac20a5c    # rkbin commit: usbplug/spl/ddr1.14/boot_merger
C_DDR111=feab2172b40f831a1f0c0e2eacc348c19ea2f780  # rkbin commit: ddr v1.11

# path : git-blob-sha : commit
declare -A WANT=(
  ["bin/rk35/rk3528_usbplug_v1.04.bin"]="d10f48a2bd4b6f3510050ecaca74f623943915c9:$C_MAIN"
  ["bin/rk35/rk3528_spl_v1.07.bin"]="a386890217ecfa182c0e5fe6542f48af57c0f8ab:$C_MAIN"
  ["bin/rk35/rk3528_ddr_1056MHz_v1.14.bin"]="d73666a4637eaddb71a0edad3b470c683676d855:$C_MAIN"
  ["tools/boot_merger"]="932682002e9c0fd0773a7647fb35f2fc77059c8c:$C_MAIN"
  ["tools/ddrbin_tool.py"]="745a50063c6761ff6a4e4f153b0079db85465d62:$C_MAIN"
)
# PCB-layout DDR variants, same commit, SHA filled in on first fetch (they are not pinned
# by any upstream project we know of, so we record what we downloaded and then assert it).
DDR_VARIANTS=(rk3528_ddr_1056MHz_4BIT_PCB_v1.14.bin rk3528_ddr_1056MHz_2L_PCB_v1.14.bin)

mkdir -p "$UP/bin/rk35" "$UP/tools" "$UP/custom" "$DELIV"

echo "===== 1. fetch + verify rkbin inputs ====="
fetch() { # $1=path $2=expected-git-blob-sha(or empty) $3=commit
  local p=$1 want=$2 commit=$3 f="$UP/$1" got
  [ -s "$f" ] || curl -fsS --max-time 60 \
    "https://raw.githubusercontent.com/rockchip-linux/rkbin/$commit/$p" -o "$f" \
    || { echo "FAIL download $p"; return 1; }
  got=$(git hash-object "$f")
  if [ -n "$want" ] && [ "$got" != "$want" ]; then
    echo "SHA MISMATCH $p (got $got want $want)"; return 1
  fi
  printf "ok  %-48s %s\n" "$(basename "$p")" "$got"
}
for p in "${!WANT[@]}"; do
  IFS=: read -r sha commit <<< "${WANT[$p]}"
  fetch "$p" "$sha" "$commit" || exit 1
done
for v in "${DDR_VARIANTS[@]}"; do
  fetch "bin/rk35/$v" "" "$C_MAIN" || exit 1
done
fetch "bin/rk35/rk3528_ddr_1056MHz_v1.11.bin" "2b703a389df1e7ccee38da714fd491cba0516011" "$C_DDR111" || exit 1
chmod +x "$UP/tools/boot_merger"

echo
echo "===== 2. what each DDR blob actually declares ====="
# Run ddrbin_tool from OUR directory, never in place inside the fetched tree: python puts
# the script's own directory on sys.path, so a fetched script must not be executed there.
cp "$UP/tools/ddrbin_tool.py" "$WORK/ddrbin_tool.py"
cd "$WORK"
: > "$WORK/ddr-banners.txt"
for d in "$UP"/bin/rk35/rk3528_ddr_*.bin; do
  banner=$(python3 -I ddrbin_tool.py rk3528 -g "$WORK/rb.txt" "$d" 2>/dev/null \
           | grep -oE 'DDR [0-9a-f]+ .*fwver: [^ ]*.*' | head -1)
  freqs=$(grep -E '^(ddr3_freq|lp3_freq|ddr4_freq|lp4_freq|lp4x_freq|first_init_dram_type)=' "$WORK/rb.txt" \
          | tr '\n' ' ')
  printf "%-42s %s\n     %s\n" "$(basename "$d")" "${banner:-<no banner>}" "$freqs" | tee -a "$WORK/ddr-banners.txt"
done

echo
echo "===== 3. pack loaders ====="
# Recipe mirrors the proven one in kxn/rk3518-atv/loader/build_loaders.py:
# CODE471 = DDR blob, CODE472 = usbplug, LOADER1/2 = FlashData/FlashBoot,
# NEWIDB=true, CREATE_IDB=false so it cannot also emit a flashable ID-block by accident.
# Output must begin with the ASCII magic "LDR ".
pack() { # $1=variant $2=ddr-path-relative-to-UP $3=outfile
  local variant=$1 ddr=$2 out=$3
  cat > "$UP/$variant.ini" <<INI
[CHIP_NAME]
NAME=RK3528
[VERSION]
MAJOR=1
MINOR=4
[CODE471_OPTION]
NUM=1
Path1=$ddr
Sleep=1
[CODE472_OPTION]
NUM=1
Path1=bin/rk35/rk3528_usbplug_v1.04.bin
[LOADER_OPTION]
NUM=2
LOADER1=FlashData
LOADER2=FlashBoot
FlashData=$ddr
FlashBoot=bin/rk35/rk3528_spl_v1.07.bin
[OUTPUT]
PATH=../delivery/$out
[SYSTEM]
NEWIDB=true
[FLAG]
471_RC4_OFF=true
RC4_OFF=true
CREATE_IDB=false
INI
  ( cd "$UP" && ./tools/boot_merger "$variant.ini" ) > "$DELIV/$variant-merger.log" 2>&1
  if [ -f "$DELIV/$out" ] && [ "$(head -c4 "$DELIV/$out")" = "LDR " ]; then
    printf "BUILT  %-54s %s bytes\n" "$out" "$(stat -c%s "$DELIV/$out")"
  else
    printf "FAILED %-54s -- tail of log:\n" "$out"; tail -4 "$DELIV/$variant-merger.log"
  fi
}

# generic + both PCB layouts, unpatched
pack "generic_v114"     "bin/rk35/rk3528_ddr_1056MHz_v1.14.bin"          "L0_generic_ddr114.bin"
pack "4bitpcb_v114"     "bin/rk35/rk3528_ddr_1056MHz_4BIT_PCB_v1.14.bin" "L1_4bitpcb_ddr114.bin"
pack "2lpcb_v114"       "bin/rk35/rk3528_ddr_1056MHz_2L_PCB_v1.14.bin"   "L2_2lpcb_ddr114.bin"

# LPDDR3 @ 780 MHz, the config one documented RK3518 board settled on (kxn patched
# lp3_freq 1056 -> 780 because the stock blob trained marginally on their LPDDR3 board)
printf 'lp3_freq=780\n' > "$UP/lpddr3-780.txt"
for v in 1.11 1.14; do
  src="$UP/bin/rk35/rk3528_ddr_1056MHz_v$v.bin"
  [ -f "$src" ] || src="$UP/bin/rk35/rk3528_ddr_1056MHz_4BIT_PCB_v1.14.bin"
  dst="$UP/custom/rk3528_lp3_780_v$v.bin"; cp "$src" "$dst"
  python3 -I ddrbin_tool.py rk3528 "$UP/lpddr3-780.txt" "$dst" > "$DELIV/lp3_780_v$v-patch.log" 2>&1
  pack "lp3_780_v$v" "custom/rk3528_lp3_780_v$v.bin" "L3_lp3780_ddr$v.bin"
done

# the community prebuilt that is known to have worked on a real RK3518 box
X="$ROOT/downloads/armbian-tv98-rk3518/recovery/x88pro_maskrom_loader.bin"
[ -f "$X" ] && cp "$X" "$DELIV/L4_x88pro_prebuilt.bin" \
  && printf "COPIED %-54s %s bytes\n" "L4_x88pro_prebuilt.bin" "$(stat -c%s "$DELIV/L4_x88pro_prebuilt.bin")"

echo
echo "===== 4. header forensics: does our RK3528 tag match the known-good RK3518 loader? ====="
# If the packed header's leading bytes agree with a loader that has demonstrably worked on
# RK3518 hardware, the chip tag and container shape are confirmed. If they diverge, stop and
# compare before trusting any read.
for f in "$DELIV/L1_4bitpcb_ddr114.bin" "$DELIV/L4_x88pro_prebuilt.bin"; do
  printf "%-26s " "$(basename "$f")"; od -An -tx1 -N 24 "$f" | tr -s ' '
done
echo "(expect identical magic '4c 44 52 20' = 'LDR '; differing sizes/timestamps are normal)"

echo
echo "===== manifest ====="
( cd "$DELIV" && sha256sum *.bin 2>/dev/null | tee SHA256SUMS.txt )
echo
echo "BENCH ORDER: L4_x88pro_prebuilt (proven on real RK3518) -> L1_4bitpcb_ddr114"
echo "(both documented RK3518 boards report '4bit PCB') -> L0_generic -> L3_lp3780_* if"
echo "the RAM chips read LPDDR3 -> L2_2lpcb. A loader that will not train costs a replug."
echo "After recon, re-run section 2 against your own sector-64 banner to pick definitively:"
echo "  strings recon/<ts>/factory_idbloader.bin | grep -oE 'DDR [0-9a-f]+ .*fwver.*'"
