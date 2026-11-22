#!/usr/bin/env bash
# Build h96ui (Rust) for the box: static aarch64-unknown-linux-musl binary, only the libc crate.
# Container (the rustup toolchain and cargo cache persist under build/rust/):
#   podman run --rm -v ~/Sources/ClaudePlayground/H96Max:/work:z fedora:41 \
#     bash /work/tools/h96ui-rs/build.sh
set -euo pipefail
cd "$(dirname "$0")"
ROOT=$(cd ../.. && pwd)
export RUSTUP_HOME=$ROOT/build/rust/rustup CARGO_HOME=$ROOT/build/rust/cargo
export CARGO_TARGET_DIR=$ROOT/build/rust/target PATH=$CARGO_HOME/bin:$PATH
if ! command -v cargo >/dev/null; then
  command -v cc >/dev/null || dnf -y -q install gcc >/dev/null   # host linker for tests/build scripts
  curl -sSf https://sh.rustup.rs | sh -s -- -y -q --profile minimal --no-modify-path
fi
command -v cc >/dev/null || dnf -y -q install gcc >/dev/null
rustup target add aarch64-unknown-linux-musl >/dev/null 2>&1
T=aarch64-unknown-linux-musl
# rust ships musl's crt + libc for this target ("self-contained"): link with its bundled lld
export CARGO_TARGET_AARCH64_UNKNOWN_LINUX_MUSL_LINKER=rust-lld
export H96UI_BUILD_DATE=$(date -u +%Y-%m-%d)
cargo test --quiet
cargo build --quiet --release --target $T
OUT=$ROOT/build/h96ui; mkdir -p "$OUT"
install -m755 "$CARGO_TARGET_DIR/$T/release/h96ui" "$OUT/h96ui"
file "$OUT/h96ui" 2>/dev/null || true
ls -la "$OUT/h96ui" && sha256sum "$OUT/h96ui"
echo "h96ui $(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml) built"
