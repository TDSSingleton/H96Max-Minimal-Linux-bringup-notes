#!/usr/bin/env bash
# ONE script: verified dump  ->  flashable 'boot'-partition image.
#
# It never touches the device. There is no rkdeveloptool call in this file, and the last
# thing it prints is the command *you* would run, if you decide to. See
# docs/log/bringup-journal.md (appendix 1, old §9) and research/notes/ §16 (superseded for arm64 by §22) for why the target is `boot` at 0xc800
# and not `uboot` at 0x4000: the stock bootloader's bootcmd already starts with
# `boot_android ${devtype} ${devnum}`, and nothing verifies the payload.
#
#   ROOT=/work bash tools/make-linux-boot.sh             # container (mounted at /work)
#   bash tools/make-linux-boot.sh                        # host
#   bash tools/make-linux-boot.sh --dry-run              # check the environment, build nothing
#   bash tools/make-linux-boot.sh --stop-after=initramfs # everything cheap, skip the kernel
#   bash tools/make-linux-boot.sh --fresh                # ignore cached artifacts
#   BUSYBOX=/path/to/aarch64/busybox bash tools/make-linux-boot.sh
#
# Steps, in the order the dependencies require:
#   0  environment          0b factory template     1  static aarch64 busybox
#   2  probe initramfs      3  kernel Image.gz      4  assemble + verify the image
#   5  print the flash command (never runs it)
#
# Order matters and is encoded here: the initramfs must exist before the kernel build,
# because build-kernel.sh embeds it via CONFIG_INITRAMFS_SOURCE.
set -uo pipefail

ROOT=${ROOT:-$HOME/Sources/ClaudePlayground/H96Max}
[ -d "$ROOT" ] || ROOT=$(cd "$(dirname "$0")/.." && pwd)
cd "$ROOT" || { echo "!! cannot cd to ROOT=$ROOT"; exit 1; }

LOGDIR=$ROOT/build/make-linux-boot
mkdir -p "$LOGDIR"
BBDIR=$ROOT/build/busybox-aarch64
BBVER=1.37.0
TPL=build/good/boot.img          # the template, carved from the verified capture
BOOT_LBA=0xc800
BOOT_SECT=0x20000                # 64 MiB

ORD="env template busybox initramfs kernel assemble summary"
stop_after=assemble
FRESH=0
for a in "$@"; do case "$a" in
  --dry-run) stop_after=env ;;
  --fresh) FRESH=1 ;;
  --stop-after=*) stop_after=${a#*=} ;;
  *) echo "note: ignoring unknown argument '$a'"; ;;
esac; done

# --dry-run must mean what it says: check the environment, touch nothing, build nothing.
at() { local i idx=0 lim=0
  for i in $ORD; do [ "$i" = "$1" ] && break; idx=$((idx+1)); done
  for i in $ORD; do [ "$i" = "$stop_after" ] && break; lim=$((lim+1)); done
  [ "$idx" -le "$lim" ]; }
stopped() { echo; echo "== --$([ "$stop_after" = env ] && echo dry-run || echo "stop-after=$stop_after"): not running [$*] =="; exit 0; }

die() { echo; echo "!! $*" >&2; exit 1; }
# Truncate this step's log when it starts. The logs append across runs, and the failure
# report tails them -- so a *fixed* error from an earlier run keeps being quoted back as
# though it were the current one (that is exactly what happened with the pool 404).
step() { echo; echo "=================== $* ==================="
         [ -n "$CUR" ] && : > "$LOGDIR/$CUR.log" 2>/dev/null; }
ok()   { echo "  OK   $*"; }
bad()  { echo "  !!   $*"; }

# Any failure says which step died and where its log is. On a machine whose console output
# we cannot see later, "it failed" is not an error message.
CUR="startup"
trap 'rc=$?; if [ $rc -ne 0 ]; then
        echo; echo "!! step [${CUR}] failed (rc=$rc).";
        [ -f "$LOGDIR/$CUR.log" ] && echo "   log: $LOGDIR/$CUR.log" && tail -20 "$LOGDIR/$CUR.log" | sed "s/^/     /";
      fi' EXIT

echo "project root : $ROOT"
echo "log dir      : $LOGDIR"

# ---------------------------------------------------------------- 0. environment
CUR=env
step "0. environment: check the things that make this take 20 minutes to fail"
miss=0
for c in file cpio gzip sha256sum sed awk python3 make curl; do
  command -v "$c" >/dev/null 2>&1 && ok "$c" || { bad "$c not found"; miss=1; }
done
# The cross compiler must ANSWER, not merely be on PATH: PATH lookups and ccache both lie.
# And note `cmd | head -1` reports head's exit status, always 0 -- so the verdict has to be
# read out of the text, not out of $?.
out=$(aarch64-linux-gnu-gcc --version 2>&1 | head -1)
case "$out" in
  *"aarch64"*"GCC"*|*"(GCC)"*) ok "aarch64-linux-gnu-gcc -> $out" ;;
  *) bad "aarch64-linux-gnu-gcc did not answer: ${out:-(no output)}"
     bad "no aarch64 cross compiler here. Run this inside the container:"
     bad "   podman run -it --rm -v $HOME/Sources/ClaudePlayground/H96Max:/work:z fedora:41 bash"
     bad "   bash /work/tools/bench-check.sh /work && ROOT=/work bash /work/tools/make-linux-boot.sh"
     miss=1 ;;
esac
for f in tools/mk-initramfs.sh tools/build-kernel.sh tools/mk-bootimg.py; do
  [ -f "$f" ] && ok "$f" || { bad "$f missing -- this chain has no fallback"; miss=1; }
done
[ $miss = 0 ] || die "environment is not ready; fix the lines above first"

SRC=downloads/kernel-rockchip
if [ -d "$SRC" ]; then
  echo "  kernel source: $(awk -F'= *' '/^(VERSION|PATCHLEVEL|SUBLEVEL|EXTRAVERSION) =/{printf "%s.", $2}' \
      $SRC/Makefile | sed 's/\.$//')  ($SRC)"
else
  # not fatal: build-kernel.sh clones the vendor tree itself, and a first clone of a
  # 6.1 tree is a few minutes we would rather spend than argue about here
  echo "  $SRC absent -- step 3 will clone it (network, several minutes)"
fi

# ------------------------------------------------------- 0b. the template image
at template || stopped "the template carve"
CUR=template
step "0b. the factory boot image, which is the template for everything we build"
if [ ! -f "$TPL" ]; then
  if [ -f recon/live-full/full.img ]; then
    echo "  carving $TPL from the verified capture (LBA $BOOT_LBA, $BOOT_SECT sectors)"
    mkdir -p build/good
    dd if=recon/live-full/full.img of="$TPL" bs=512 skip=$((BOOT_LBA)) count=$((BOOT_SECT)) status=none \
      || die "carve failed"
  else
    die "no $TPL and no recon/live-full/full.img -- the template has to come from the dump"
  fi
fi
head -c 8 "$TPL" | grep -q ANDROID || die "$TPL does not start with ANDROID! -- wrong template"
ok "$TPL  $(stat -c%s "$TPL") bytes, sha256 $(sha256sum "$TPL" | cut -c1-16)…"

# ------------------------------------------------------- 1. an aarch64 busybox
at busybox || stopped "the busybox build"
CUR=busybox
step "1. an aarch64 busybox for the initramfs"
# What this step has to produce: ONE statically linked aarch64 binary.
# A dynamic busybox needs ld.so + libc inside the initramfs as well, and mk-initramfs.sh
# does not copy them; the symptom is `Failed to execute /init (error -2)`, which looks
# exactly like a DTB or kernel bug. That is why "is it static" is a gate, not a preference.
#
# And why the toolchain assumption was wrong: Fedora's gcc-aarch64-linux-gnu is a
# FREESTANDING cross compiler. It was enough for u-boot (which builds -nostdinc) and it is
# enough for the kernel (same), but busybox is a *hosted* program and there is no target
# libc: `aarch64-linux-gnu-gcc -print-sysroot` says /usr/aarch64-linux-gnu/sys-root and that
# directory contains zero headers, so `#include <byteswap.h>` dies on file 3 of 400.
# The probe below asks the compiler the question instead of assuming the answer.
BB_APPLETS="sh mount umount cat echo printf grep sed awk cut head tail wc ls lsmod insmod
            dmesg uname date usleep poweroff reboot halt mknod mkdir sleep dd stat basename
            find md5sum sha256sum tr xargs readlink blockdev losetup mdev"

cc_can_link_libc() {
  # Compile AND link a hosted program. Linking matters: the headers may exist while
  # crt1.o/libc.a do not, and that fails at a different, later, more confusing file.
  local dir=build/ccprobe && mkdir -p "$dir"
  printf '#include <byteswap.h>\n#include <stdio.h>\nint main(void){puts("ok");return 0;}\n' > "$dir/p.c"
  ( aarch64-linux-gnu-gcc -static "$dir/p.c" -o "$dir/p.elf" >>"$LOGDIR/busybox.log" 2>&1 \
      && file -b "$dir/p.elf" | grep -q 'ARM aarch64' )
}

bb_validate() {
  local b=$1 f
  [ -x "$b" ] || die "no executable busybox at $b (mode $(stat -c%a "$b" 2>/dev/null || echo 'missing'), umask $(umask))"
  f=$(file -b "$b")
  echo "  $b"
  echo "     $f"
  case "$f" in
    *"ARM aarch64"*) ;;
    *ARM*)  die "$b is 32-bit ARM, not aarch64: $f  (would need CONFIG_COMPAT and a compat
            VDSO in the initramfs; the vendor's own toybox is exactly this, see below)" ;;
    *) die "$b is not an aarch64 binary at all: $f" ;;
  esac
  case "$f" in *static*) ok "statically linked" ;; *) die "$b is dynamically linked: $f" ;; esac
  # Applet presence, asserted rather than assumed. busybox keeps its applet names as one
  # NUL-separated .rodata string, so `strings` shows each on its own line. A missing applet
  # otherwise surfaces as a line on a screen nobody is watching.
  if command -v strings >/dev/null; then
    local have miss=""
    have=$(strings -a -n 1 "$b" 2>/dev/null | sort -u)
    # here-strings, not `echo "$have" | grep -q`: grep -q exits the moment it matches, its
    # upstream echo then dies of SIGPIPE (141), and pipefail promotes that to the pipeline's
    # status -- so every name found EARLY reads as missing. Here-strings have no upstream.
    for a in $BB_APPLETS; do grep -qx "$a" <<<"$have" || miss="$miss $a"; done
    if [ -n "$miss" ]; then
      echo "  !! applets missing from this busybox:$miss"
      echo "     the /init in mk-initramfs.sh would fail on those, silently, on the device."
      die "busybox lacks required applets"
    fi
    ok "all $(echo $BB_APPLETS | wc -w) initramfs applets present"
  else
    echo "  (no 'strings' -- cannot verify the applet table)"
  fi
}

busybox_hint() {
  echo "   Any of these produce the one thing this step needs -- a static aarch64 busybox:"
  echo "   (a) give the cross compiler a target libc, then step 1 compiles busybox $BBVER"
  echo "       from upstream source itself (best provenance; what the probe above tests for):"
  echo "         dnf search 'glibc*aarch64*'   # Fedora 45 has no cross glibc in the repos;"
  echo "         dnf search 'aarch64*glibc'    # if nothing turns up, use (b) or (c)."
  echo "   (b) a musl cross toolchain, which ships its own libc + headers and is self-contained:"
  echo "         https://musl.cc/aarch64-linux-musl-cross.tgz   (then CROSS_COMPILE=...)"
  echo "   (c) Debian's busybox-static arm64 package -- no compiler at all, one file:"
  echo "         (this is what the step falls back to automatically)"
  echo "   (d) run an aarch64 container under qemu and copy the binary out:"
  echo "         podman run --rm --arch arm64 fedora:41 cat /usr/bin/busybox > /tmp/bb"
  echo "         (Fedora's is dynamic, so this only helps if you also copy the libs in)"
  echo "   NOT usable, and worth knowing why: the vendor's own toybox, which is sitting in"
  echo "   the dump's recovery ramdisk (build/tpl/rec/system/bin/toybox) -- it is"
  echo "   ELF 32-bit ARM, dynamically linked, interpreter /system/bin/linker: Bionic. Using"
  echo "   it means hauling Android's linker and libs into our initramfs and enabling"
  echo "   CONFIG_COMPAT, i.e. running the vendor code we are trying not to run."
}
bb_fetch_static() {
  # Debian's busybox-static for arm64: one file, no compiler, no libc needed.
  # Resolved through the package INDEX rather than a guessed pool path, for two reasons:
  # pool/ directories are keyed by SOURCE package (pool/main/b/busybox/, not
  # busybox-static/), which is why the first version of this 404'd; and the index
  # publishes SHA256 and Size, so the download can be *verified* against what Debian
  # says it is instead of merely hashed for the record. Still not GPG-verified -- no apt
  # keyring is involved. The source build is the better-provenance path; it just needs
  # a target libc, which this machine does not have.
  local dest=$1 base=http://deb.debian.org/debian dir=build/busybox-deb
  mkdir -p "$dir" || return 1
  local idx=$dir/Packages.gz
  if [ ! -s "$idx" ]; then
    echo "   fetching the arm64 package index (a few MB)"
    curl -fsSL --retry 2 -o "$idx" "$base/dists/stable/main/binary-arm64/Packages.gz" \
      >>"$LOGDIR/busybox.log" 2>&1 || { echo "   index fetch failed"; return 1; }
  fi
  local stanza fn sha sz arch deb got
  stanza=$(zcat "$idx" 2>/dev/null | awk '/^Package: busybox-static$/{f=1} f{print} f&&/^$/{exit}')
  [ -n "$stanza" ] || { echo "   busybox-static is not in the arm64 index"; return 1; }
  fn=$(echo "$stanza" | awk '/^Filename:/{print $2}')
  sha=$(echo "$stanza" | awk '/^SHA256:/{print $2}')
  sz=$(echo "$stanza" | awk '/^Size:/{print $2}')
  arch=$(echo "$stanza" | awk '/^Architecture:/{print $2}')
  echo "   index: $fn  arch=$arch  size=$sz"
  [ "$arch" = arm64 ] || { echo "   index architecture is $arch, not arm64"; return 1; }
  [ -n "$fn" ] && [ -n "$sha" ] || { echo "   index stanza has no Filename/SHA256"; return 1; }
  deb=$dir/$(basename "$fn")
  [ -s "$deb" ] || curl -fsSL --retry 2 -o "$deb" "$base/$fn" >>"$LOGDIR/busybox.log" 2>&1 \
      || { echo "   download failed"; return 1; }
  got=$(sha256sum "$deb" | cut -d' ' -f1)
  if [ "$got" != "$sha" ]; then
    echo "   !! sha256 $got does not match the index ($sha) -- not using it"
    return 1
  fi
  ok "download verified against Debian's index: $sha"
  rm -f "$dir"/data.tar.*
  # a .deb is an `ar` archive; a container that only installed the *cross* binutils may
  # have no `ar` at all, so take the two routes that work and say which one was used
  if command -v ar >/dev/null 2>&1; then
    echo "   unpacking with ar"
    ( cd "$dir" && ar x "$(basename "$deb")" ) >>"$LOGDIR/busybox.log" 2>&1 \
      || { echo "   'ar x' failed"; return 1; }
  elif command -v bsdtar >/dev/null 2>&1; then
    echo "   unpacking with bsdtar (no 'ar' here)"
    bsdtar -xf "$deb" -C "$dir" >>"$LOGDIR/busybox.log" 2>&1 || { echo "   bsdtar failed"; return 1; }
  else
    echo "   need 'ar' or 'bsdtar' to unpack a .deb: dnf install binutils libarchive"
    return 1
  fi
  local data
  data=$(ls "$dir"/data.tar.* 2>/dev/null | head -1)
  [ -n "$data" ] || { echo "   no data.tar.* in the deb"; return 1; }
  tar -xf "$data" -C "$dir" >>"$LOGDIR/busybox.log" 2>&1 || { echo "   could not unpack $data (needs xz/zstd support in tar)"; return 1; }
  # Pick by CONTENT, not by name or size. `find -name busybox` in this package matches six
  # files -- usr/bin/busybox plus a 16-byte initramfs-tools conf hook, a lintian override,
  # a man page and a doc directory -- and "first match wins" copied the 16-byte config
  # snippet out as our shell. Taking the biggest match would have worked today by luck;
  # asking the file what it is works regardless of what else the package ships.
  local bb="" c
  while IFS= read -r c; do
    if file -b "$c" 2>/dev/null | grep -q 'ARM aarch64'; then bb=$c; break; fi
  done < <(find "$dir" -type f -name busybox | sort)
  if [ -z "$bb" ]; then
    echo "   no aarch64 ELF named 'busybox' in the package; every candidate was:"
    find "$dir" -type f -name busybox -printf '     %10s %m %p\n' | sort -k3
    return 1
  fi
  # GNU tar extracts as an ordinary user with --no-same-permissions, i.e. the umask is
  # applied to the archive's mode, and the exec bit is exactly the bit that can vanish.
  # The trap printed "no busybox at $dest" below while the copy had plainly succeeded --
  # that is a missing x bit, not a missing file. Set it on both sides and report it.
  echo "   found $bb ($(stat -c%s "$bb") bytes, mode $(stat -c%a "$bb"), umask $(umask))"
  chmod 0755 "$bb" 2>/dev/null || echo "   !! could not chmod the extracted file"
  mkdir -p "$(dirname "$dest")" && cp -f "$bb" "$dest" || { echo "   copy to $dest failed"; return 1; }
  chmod 0755 "$dest" 2>/dev/null
  local m=$(stat -c%a "$dest" 2>/dev/null)
  echo "  extracted $dest (mode $m)"
  [ -x "$dest" ] || { echo "   !! $dest is still not executable (mode $m) -- $dest lives on a"
                     echo "      filesystem mounted noexec, or the chmod was refused"
                     return 1; }
  return 0
}

ensure_busybox() {
  if [ -n "${BUSYBOX:-}" ] && [ -x "$BUSYBOX" ]; then
    f=$(file -b "$BUSYBOX")
    case "$f" in *"ARM aarch64"*) ;; *) die "BUSYBOX=$BUSYBOX is not aarch64: $f" ;; esac
    case "$f" in *"statically"*) ok "using provided static busybox: $BUSYBOX" ;;
      *) echo "  !! provided busybox is dynamic ($f): its libraries must be in the initramfs" ;;
    esac
    return 0
  fi
  local B=$BBDIR/bin/busybox
  if [ $FRESH = 0 ] && [ -x "$B" ]; then
    echo "  cached busybox:"; bb_validate "$B"; BUSYBOX=$B; return 0
  fi
  # Ask the compiler whether it can build a *hosted* program, in one file, before starting
  # a 400-file build that dies on the third. This is the check that was missing: Fedora's
  # cross gcc is freestanding (empty sysroot), which is invisible until byteswap.h.
  if cc_can_link_libc; then
    ok "cross compiler can compile and link a static hosted aarch64 program"
    echo "     -> building busybox $BBVER from upstream source"
  else
    bad "cross compiler cannot link a hosted program (log: $LOGDIR/busybox.log)"
    bad "no target libc for aarch64 on this machine: not compiling from source."
    if bb_fetch_static "$B"; then
      echo "  using the prebuilt static package:"; bb_validate "$B"
      BUSYBOX=$B; return 0
    fi
    echo "  ...and that fallback failed as well. Options:"
    busybox_hint
    die "cannot obtain a static aarch64 busybox by any route"
  fi
  command -v aarch64-linux-gnu-ld >/dev/null || die "no cross binutils to link busybox with"

  local tar=build/busybox-$BBVER.tar.bz2
  if [ ! -f "$tar" ]; then
    echo "  downloading busybox $BBVER source from busybox.net"
    curl -fsSL --retry 3 -o "$tar" "https://www.busybox.net/downloads/busybox-$BBVER.tar.bz2" \
      >"$LOGDIR/busybox.log" 2>&1 || { bad "download failed, see $LOGDIR/busybox.log"
        busybox_hint; die "cannot obtain busybox"; }
  fi
  [ -s "$tar" ] || die "$tar is empty"
  rm -rf build/busybox-$BBVER
  tar -xf "$tar" -C build || die "busybox source extract failed"

  # Inside a subshell `die` only exits the subshell, so this block uses set -e and
  # returns its status to the caller, which checks it. Silent continuation here would
  # mean the next step builds an initramfs around a binary that does not exist.
  ( set -e
    cd build/busybox-$BBVER
    echo "  configure"            | tee -a "$LOGDIR/busybox.log"
    make defconfig >>"$LOGDIR/busybox.log" 2>&1
    # static, and no -Werror: distro gcc is newer than busybox's test matrix, and a
    # warning-turned-error is not a reason to lose the whole chain
    sed -i 's/^# CONFIG_STATIC is not set/CONFIG_STATIC=y/; s/^CONFIG_WERROR=y/# CONFIG_WERROR is not set/' .config
    grep -q '^CONFIG_STATIC=y' .config || { echo "!! CONFIG_STATIC did not take"; exit 1; }
    echo "  compile (-j$(nproc))" | tee -a "$LOGDIR/busybox.log"
    make CROSS_COMPILE=aarch64-linux-gnu- -j"$(nproc)" >>"$LOGDIR/busybox.log" 2>&1
    mkdir -p "$BBDIR/bin"
    cp -f busybox "$BBDIR/bin/busybox"
  ) || { echo "   build failed; tail of $LOGDIR/busybox.log:"
        tail -20 "$LOGDIR/busybox.log" | sed 's/^/     /'
        echo "   most likely cause: this distro's gcc (16.x on this host) is newer than"
        echo "   busybox $BBVER's test matrix. Trying the prebuilt static package."
        if bb_fetch_static "$B"; then
          echo "  using the prebuilt static binary:"; bb_validate "$B"
          BUSYBOX=$B; return 0
        fi
        echo "   and that failed too. Options:"
        busybox_hint
        die "busybox build failed -- full log: $LOGDIR/busybox.log"; }
  echo "  built from source:"; bb_validate "$B"
  BUSYBOX=$B
}
ensure_busybox
[ -x "${BUSYBOX:-}" ] || die "BUSYBOX is unset or not executable after step 1"
export BUSYBOX
echo "  initramfs will contain: $BUSYBOX"

# ------------------------------------------------------- 2. the probe initramfs
at initramfs || stopped "the initramfs"
CUR=initramfs
step "2. probe initramfs (what the box reports back, and where)"
if [ $FRESH = 0 ] && [ -s build/kernel/initramfs.cpio.gz ]; then
  ok "cached: build/kernel/initramfs.cpio.gz ($(stat -c%s build/kernel/initramfs.cpio.gz) bytes)"
  echo "     (rebuild with --fresh, or after editing the /init in tools/mk-initramfs.sh)"
else
  ROOT=$ROOT bash tools/mk-initramfs.sh 2>&1 | tee "$LOGDIR/initramfs.log" | tail -20
  [ -s build/kernel/initramfs.cpio.gz ] || die "mk-initramfs.sh produced nothing"
fi
# The archive must contain an /init we can actually execute. cpio lists it as "./init",
# because mk-initramfs.sh packs it with `find .`, so match either spelling -- `grep -x init`
# here would fail forever and read like a broken initramfs rather than a broken check.
LIST=$(gzip -dc build/kernel/initramfs.cpio.gz 2>/dev/null | cpio -t 2>/dev/null)
# GNU cpio prints names WITHOUT the "./" that `find .` fed in, so accept both spellings.
# My previous pattern was '^\.?/+init$' -- "/+" demands at least one slash, which matches
# "./init" and "init"... no, it matches neither as printed: it needs a slash, so it rejected
# a perfectly good initramfs. The original `grep -x init` was correct and I "fixed" it into
# a bug. Verify a pattern against the real output before trusting a comment about it.
grep -qxE '^(\./)?init$' <<<"$LIST" \
  || die "initramfs has no /init at the archive root -- the kernel would panic 'No working init found'"
ok "initramfs contains /init"

# ------------------------------------------------------- 3. the kernel
at kernel || stopped "the kernel build (~15 min)"
CUR=kernel
step "3. vendor kernel 6.1.172 (arm64, Image.gz, initramfs embedded)"
if [ $FRESH = 0 ] && [ -s build/kernel/kernel ] && grep -qx "$(sha256sum build/kernel/kernel | cut -d' ' -f1)" <(cut -d' ' -f1 build/kernel/SHA256SUMS.txt 2>/dev/null); then
  ok "cached: build/kernel/kernel ($(stat -c%s build/kernel/kernel) bytes)"
  echo "     a full rebuild takes ~15 min; --fresh forces it"
else
  ROOT=$ROOT bash tools/build-kernel.sh 2>&1 | tee "$LOGDIR/kernel.log" | tail -40
fi
[ -s build/kernel/kernel ] || die "no build/kernel/kernel -- kernel step did not emit the payload"
k=$(file -b build/kernel/kernel); echo "  payload form: $k"
case "$k" in
  *"Linux kernel ARM boot executable zImage"*) ok "zImage stub, the same form as the factory payload" ;;
  *"gzip compressed"*) ok "gzip payload (u-boot copies it to 0x10008000 and jumps to the stub)" ;;
  *) bad "unexpected form. The factory boot.img holds a gzip zImage; a raw Image at"
     bad "0x10008000 is only 32 KiB aligned, which the arm64 boot protocol does not promise."
     echo "   check that CONFIG_KERNEL_GZIP took effect in $LOGDIR/kernel.log" ;;
esac

# ------------------------------------------------------- 4. assemble
at assemble || stopped "image assembly"
CUR=assemble
# What to add to the factory command line. Both consoles are wanted, and the factory
# cmdline already opens with console=ttyFIQ0, so we only append the second one:
#   console=tty1   -> the HDMI text console. Without it kernel messages go ONLY to uart0
#                     at 1,500,000 baud (fiq-debugger, serial-id 0, uart0m0_xfer; the
#                     uart0 8250 node itself is status "disabled" -- FIQ seizes the
#                     hardware directly, which is also where u-boot prints).
#   loglevel=8     -> probe failures are the thing we actually need to read.
# The LAST console= wins for /dev/console, so tty1 ends up on the screen.
BOOTARGS_ADD=${BOOTARGS_ADD:-console=tty1 loglevel=8}

step "4. assemble the boot-partition image (and verify it by re-reading the file)"
python3 tools/mk-bootimg.py \
  --template "$TPL" \
  --kernel build/kernel/kernel \
  --ramdisk build/kernel/initramfs.cpio.gz \
  --out build/out/linux-boot.img \
  --part-sectors $BOOT_SECT --start-lba $BOOT_LBA \
  --cmdline-append "$BOOTARGS_ADD" \
  --fbdev-on \
  2>&1 | tee "$LOGDIR/assemble.log"
IMG=build/out/linux-boot.img
[ -s "$IMG" ] || die "no $IMG produced"
[ "$(stat -c%s "$IMG")" = "$((BOOT_SECT*512))" ] || die "$IMG is not exactly the partition size"

# ------------------------------------------------------- 5. what to do next
CUR=summary
step "5. ready. Nothing has been written to the device."
echo "  image : $IMG"
echo "  sha256: $(sha256sum "$IMG")"
echo "  kernel: $(stat -c%s build/kernel/kernel) bytes   initramfs: $(stat -c%s build/kernel/initramfs.cpio.gz) bytes"
echo "  factory template kept untouched at $TPL (undo bytes)"
echo
echo "  The console for this first boot is HDMI (/dev/tty1). The initramfs prints its probe
  report there and copies it to a USB stick as rk3518-report.txt + BOOT_OK; with no stick
  present it writes nothing anywhere unless 'emmc_report=1' is on the command line."
echo
cat <<EOF
  When, and only when, you decide to write -- button -> maskrom -> Download Boot L5b, then:

      rkdeveloptool wl $BOOT_LBA $IMG
      rkdeveloptool rl $BOOT_LBA $BOOT_SECT /tmp/echo.img
      sha256sum /tmp/echo.img $IMG            # must match, or the write is a lie

  Undo, byte-exact, whenever you want it back:

      rkdeveloptool wl $BOOT_LBA $TPL

  This script deliberately contains no rkdeveloptool call: the standing rule on this project
  is that no write happens without your explicit agreement, and a script that could not make
  one is a stronger guarantee than one that promises not to.
EOF
