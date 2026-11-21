#!/usr/bin/env bash
# Run this FIRST, inside the container, before any long build.
#
# Why it exists: the 2026-11-20 morning u-boot build reported success from inside
# a container, and left nothing behind. Not one file in this project was newer
# than 09:00 -- and the log quoted `Makefile:1058: checkarmreloc` while in OUR
# tree line 1058 is an include flag and checkarmreloc is at 2501. That build ran
# in a checkout that is not this one, in a filesystem that is not the mount, and
# `--rm` took it away. A 20-minute build is worth 10 seconds of proof that its
# output will be where we can read it.
set -uo pipefail

 WANT=${1:-/work}
echo "=== who / where ==="
echo "  pwd            : $(pwd)"
echo "  expected root   : $WANT"
echo "  container image : $(cat /etc/os-release 2>/dev/null | grep -E '^PRETTY_NAME' || echo '(no /etc/os-release: on the host, not in the container)')"
echo "  kernel of the builder: $(uname -s -m -r)"

echo "=== is the mount real, and writable, and THIS project? ==="
if [ ! -d "$WANT" ]; then
  echo "  !! $WANT does not exist -- this container has no project mount. Start with:"
  echo "     podman run -it --rm -v \$HOME/Sources/ClaudePlayground/H96Max:$WANT:z fedora:41 bash"
  echo "     (the :z is not optional: SELinux enforcing turns it into 'not a git repository')"
  exit 1
fi
MARK="$WANT/.mount-check.$$"
echo "  sentinel write" > "$MARK" 2>/dev/null || { echo "  !! cannot write into $WANT (permissions/SELinux)"; exit 1; }
got=$(cat "$MARK" 2>/dev/null)
rm -f "$MARK"
if [ "$got" = "  sentinel write" ]; then
  echo "  OK: $WANT is writable and readable"
else
  echo "  !! wrote and could not read back -- not a real mount"; exit 1
fi
# identity: these files exist only in this project
for f in tools/mk-bootimg.py docs/log/status-and-resume.md recon/live-full/full.img; do
  [ -e "$WANT/$f" ] && echo "  OK: $WANT/$f" || echo "  !! $WANT/$f MISSING -- mounted the wrong directory"
done

echo "=== the tree we are about to build, proven not assumed ==="
for t in "$WANT/downloads/u-boot" "$WANT/downloads/kernel-rockchip"; do
  [ -d "$t" ] || { echo "  !! $t missing"; continue; }
  echo "  $t"
  echo "      Makefile L1058 : $(awk 'NR==1058{print}' "$t/Makefile" | tr -s ' ')"
  echo "      version        : $(head -4 "$t/Makefile" | tr -d '\n' | sed 's/.*VERSION = //' | tr -s ' ')"
  echo "      newest file    : $(ls -t "$t" 2>/dev/null | head -1)"
  echo "      writable       : $([ -w "$t" ] && echo yes || echo NO)"
done

echo "=== toolchain, by real execution not by 'command -v' ==="
# ccache and PATH lookups both lie about which compiler is answering; ask it.
for c in aarch64-linux-gnu-gcc aarch64-linux-gnu-objcopy aarch64-linux-gnu-ar; do
  if out=$($c --version 2>&1 | head -1); then echo "  OK  $c -> $out"
  else echo "  !!  $c -> $out"; fi
done
echo "  pylibfdt (u-boot needs it): $(python3 -c 'import os,sys; sys.path.insert(0,"/work/downloads/u-boot/scripts/dtc/pylibfdt"); import libfdt; print("importable")' 2>&1 | tail -1)"

echo
echo "If every line above is OK, the build's output will be readable from the host."
echo "If any line is not, fix it before spending 20 minutes."
