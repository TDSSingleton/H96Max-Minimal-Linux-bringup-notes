#!/usr/bin/env bash
# Build a Linux rkdeveloptool and record what it supports.
# Read-only w.r.t. any device. Safe to re-run.
#
# Why from source: the Fedora package is 1.32 (2021-era, predates RK3518/RK3528).
# Note both GitHub trees named rkdeveloptool ship a *macOS* CMakeLists.txt
# (PROJECT_NAME rkDevelopTool_Mac, /usr/local/Cellar paths, .dylib link) --
# use autotools, not cmake.
set -uo pipefail

ROOT=$(cd "$(dirname "$0")/.." && pwd)
SRC="$ROOT/downloads/rkdeveloptool-upstream"
OUT="$ROOT/tools/rkbt"
LOG="$ROOT/tools/setup.log"

{
  echo "===== $(date -Is) rkdeveloptool build ====="
  [ -d "$SRC/.git" ] || git clone --depth 1 https://github.com/rockchip-linux/rkdeveloptool.git "$SRC"

  cd "$SRC" || exit 1
  echo "--- autoreconf"
  autoreconf -fi
  echo "--- configure"
  ./configure --prefix="$OUT"
  echo "--- make"
  make -j"$(nproc)"

  echo
  echo "===== usage / version ====="
  ./rkdeveloptool -v
  # This dump is what I need to read to confirm the exact read-command semantics.
  ./rkdeveloptool -h > "$ROOT/tools/rkdeveloptool-help.txt" 2>&1 || true
  cat "$ROOT/tools/rkdeveloptool-help.txt"

  echo
  echo "===== read-command semantics (sector vs byte count) ====="
  # A historical Debian/pine64 build of rkdeveloptool treated the 3rd argument of
  # `rl` as a BYTE count instead of a sector count. We must know which build we
  # have before trusting any offset math, so surface the source of truth.
  grep -nE '\b(rl|rr)\b' main.cpp | head -20

  echo
  echo "===== 2207 product ids referenced in source ====="
  grep -ohE '0x3[0-9a-fA-F]{3}' DefineHeader.h | sort -u | tr '\n' ' '
  echo
} 2>&1 | tee "$LOG"

echo
echo "Now install the udev rule (needs root, run yourself):"
echo "  sudo cp $ROOT/tools/99-rockchip.rules /etc/udev/rules.d/"
echo "  sudo udevadm control --reload && sudo udevadm trigger"
echo
echo "Optional but useful for offline analysis of the dump:"
echo "  sudo dnf install -y binwalk lz4"
