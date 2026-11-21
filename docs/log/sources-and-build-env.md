# Source availability and the build environment

Part of the session log (the narrative: what happened and why); index in [README.md](README.md). Technical detail is in `research/notes/`.

## 6. Source availability — the user's hunch was right

- `rockchip-linux/kernel@develop-6.1`: **`arch/arm/boot/dts/rk3518-evb1-ddr4-v10.dts` → HTTP 200**
  plus `arch/arm64/boot/dts/rockchip/{rk3518.dtsi, rk3518-evb.dtsi, rk3518-evb1-ddr4-v10.dts/.dtsi}`.
  The 6.1 BSP keeps 32-bit DTs in the **flat** `arch/arm/boot/dts/` — no `rockchip/` subdir —
  which is why my earlier "there is no rk3518.dtsi" was wrong twice over (wrong arch dir, then
  wrong layout assumption). Absent in `develop-5.10`.
- `daeiron/ea6x21qx_driver` — "SeekWave EA6x21qx wifi/bt driver", tree mirrors the ODM path.
- `TinkerBoard-Android/rockchip-android-hardware-seekwave@android14-rockchip` — `wlan/`
  (wifi_hal, configs, wpa_supplicant_8_lib) + `skwbt/src`.
- `ilyakurdyukov/rk3528-tvbox` — Armbian TV-box base with `rk3528-evb1-ddr4-v10.dtsi`,
  **`rk-stb-ir-keymap.dtsi`**, `rk3528-h96max-m1.dts`.
- FriendlyARM `sd-fuse_rk3528` (kernel-6.1.y) + `kernel-rockchip@nanopi6-v6.1.y` — but
  `ARCH=arm64`, RK3518 never named. `forum.neardi.com` is Flarum (JS shell; `/api` works but
  guest search filter ignored) — **no RK3518 threads found**.
- Kernel target: `develop-6.1` is 6.1.**172** ≠ the vendor's 6.1.118, and modversions means the
  ripped .ko won't load → driver must be rebuilt (or pin 6.1.118).

## 8. Build environment: Fedora 45 is hostile to current u-boot

Five collisions, **none of them about the board**:
1. `swig 4.5` removed `pycompat.i` → vendored `libfdt.i` still calls `PyString_FromString`,
   `PyString_AsString`, `PyInt_AsLong`. Fix: 3 macro shims, **enumerated** not guessed.
   (Fedora 41 ships swig 4.2 → the shim is still needed there.)
2. **OpenSSL 4.0.3 deleted `ENGINE_*`** (`nm -D libcrypto.so | grep -c ' T ENGINE_'` → 0) →
   `rsa-sign.c` won't link. Root cause was *not* signing: `TOOLS_KWBIMAGE` (`default y`, Marvell
   Kirkwood!) `select TOOLS_LIBCRYPTO`. `TOOLS_FIT_SIGNATURE` is `def_bool` depending on it and
   can't be disabled directly. Breaking the chain at kwbimage fixed it.
3. **I invented `DOS_LABEL` and `CMD_EXT4`** — they don't exist. `scripts/config --enable` on a
   nonexistent name writes a line `olddefconfig` silently drops: indistinguishable from success.
   Script now asserts all 11 requested symbols are really `=y`.
4. `mkeficapsule` needed gnutls headers → disabled `TOOLS_MKEFICAPSULE/MXSSDR/BITFAKE`.
5. `ARCH=arm64` for u-boot → `arch/arm64/Makefile: No such file`. u-boot uses **`ARCH=arm`**
   for 64-bit too (`select ARM64` under `ROCKCHIP_RK3528`); `ARCH=arm64` is the *Linux*
   convention and `build-kernel.sh` correctly uses it.
Also: `make | tail -N` **buffered all output**, so a Kconfig prompt waiting on stdin looked
like a hang. Lesson encoded: every `make` gets `</dev/null`, output is streamed with `tee`,
exit status via `PIPESTATUS`, and a `syncconfig`/`scripts_dtc`/`tools` pre-flight runs before
the long build.

**Container:** `podman run -it --rm -v ~/Sources/ClaudePlayground/H96Max:/work:z fedora:41`.
`:z` is mandatory — the tree is `user_home_t` under **Enforcing** SELinux and container labels
can't read it ("not a git repository" was really a label denial). Undo with
`restorecon -RF`. Rootless podman maps container root → uid 1000, so outputs stay user-owned.
Scripts now accept `ROOT=/work`. `busybox` in an initramfs must be **aarch64** — the script
refuses a non-ARM binary, because x86-64 `/init` fails as `error -2` and looks like a DTB bug.
