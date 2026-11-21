#!/usr/bin/env bash
# Build a tiny initramfs whose only purpose is to BOOT, LOOK AROUND, AND TALK BACK
# on a device that has no usable serial console.
#
# Output channels, in order of preference:
#   1. /dev/tty1      -- the HDMI text console (the vendor DRM stack; our real console)
#   2. a USB stick     -- FAT/ext4 partition on the single USB-A port; you unplug it
#                        and read the report on the PC. No network, no device access.
#   3. eMMC 'cache' partition -- if no stick is present, the raw report is dd'ed to
#      /dev/disk/by-partlabel/cache at offset 0, and we pull it back later over
#      RockUSB with `rl`. 'cache' is Android scratch we are never going to keep.
#
# Nothing here writes to the eMMC unless channel 2 is absent, and even then only to
# one named scratch partition.
set -uo pipefail

# ROOT is overridable for container builds:  ROOT=/work bash tools/mk-initramfs.sh
ROOT=${ROOT:-$(cd "$(dirname "$0")/.." && pwd)}
OUT=$ROOT/build/kernel
BUILD=$OUT/initramfs-root
REPORT_VERSION=probe-1

# The initramfs runs on the TARGET. A host (x86-64) busybox inside it fails as
#   Failed to execute /init (error -2)
# which looks exactly like a kernel/DTB bug. So: accept an explicit aarch64 binary,
# and refuse anything that is not ARM aarch64.
BB=${BUSYBOX:-}
[ -n "$BB" ] || BB=$(command -v busybox || true)
if [ -z "$BB" ]; then
  echo "!! no busybox found. Provide an AARCH64 one, e.g. on the host:"
  echo "   mkdir -p /tmp/armbb && dnf install --installroot=/tmp/armbb --releasever=41 \\"
  echo "     --forcearch=aarch64 --downloadonly --downloaddir=/tmp/armbb busybox"
  echo "   cd /tmp/armbb && rpm2cpio busybox-*.rpm | cpio -idmv && ls -l bin/busybox"
  echo "   then: BUSYBOX=/tmp/armbb/bin/busybox bash tools/mk-initramfs.sh"
  exit 1
fi
if command -v file >/dev/null; then
  ft=$(file -L "$BB")
  echo "busybox: $BB"
  echo "  $ft"
  case "$ft" in
    *"ARM aarch64"*) : ;;
    *) echo "!! NOT an aarch64 binary. An x86-64 /init fails as 'error -2' and looks";
       echo "   like a kernel bug. Use BUSYBOX=/path/to/aarch64/busybox (see hint above)."; exit 1 ;;
  esac
  # Same failure mode, same symptom, different cause: a dynamic busybox can't find its
  # ld.so inside an initramfs that contains no libraries, and the kernel reports 'error -2'.
  case "$ft" in
    *static*) echo "  statically linked: no library dependencies to carry into the initramfs" ;;
    *) echo "!! this busybox is DYNAMICALLY linked: $ft";
       echo "   /init will fail as 'Failed to execute /init (error -2)' unless the initramfs";
       echo "   also contains ld-linux-aarch64.so.1 and libc.so.6 -- nothing here copies them.";
       echo "   Use tools/make-linux-boot.sh (it builds or fetches a static one), or point";
       echo "   BUSYBOX= at a static binary. Refusing to build a knowingly broken initramfs.";
       exit 1 ;;
  esac
else
  echo "   (no 'file' command -- cannot verify the binary's architecture; install file)"
  exit 1
fi

rm -rf "$BUILD"; mkdir -p "$BUILD"/{bin,sbin,dev,proc,sys,usb,mmcroot,etc,lib,root,tmp,var/log}
cp "$BB" "$BUILD/bin/busybox"
# cp/tar both let the umask bite when creating a new file, and the mode stored IN the cpio
# is what the kernel honours when it runs /init. An init that is not executable gives
# "No working init found" / "Failed to execute /init" -- indistinguishable from a bad DTB.
chmod 0755 "$BUILD/bin/busybox" || { echo "!! could not chmod busybox"; exit 1; }

# applet symlinks -- only what the report needs, keeps it small
cd "$BUILD"
for a in sh mount umount cat echo printf grep sed awk cut head tail wc ls lsmod insmod dmesg \
         uname date usleep poweroff reboot halt mknod mkdir sleep dd stat basename find \
         md5sum sha256sum tr xargs readlink blockdev losetup mdev diff od timeout findfs switch_root tar gunzip; do
  ln -sf /bin/busybox "bin/$a" 2>/dev/null || true
done
ln -sf /bin/busybox sbin/init 2>/dev/null || true
cd "$ROOT"

cat > "$BUILD/init" <<'INIT_EOF'
#!/bin/busybox sh
# initramfs hardware probe. See mk-initramfs.sh for the channel design.
REPORT_VERSION=stage1-2
PATH=/bin:/sbin
# /dev must be mounted BEFORE any redirect to /dev/tty1. CONFIG_DEVTMPFS_MOUNT does not
# apply to initramfs boots, so /dev starts empty, and busybox sh EXITS when `exec >file`
# cannot open its target (tested under qemu-aarch64: "can't create ...", status 1).
# /init exiting = "Kernel panic - Attempted to kill init" before a single line of ours.
mount -t devtmpfs devtmpfs /dev  2>/dev/null
mount -t proc     proc     /proc 2>/dev/null
mount -t sysfs    sysfs    /sys  2>/dev/null
# fbcon creates tty1 only once DRM has bound; give it a few seconds, never die over it.
i=0; while [ ! -c /dev/tty1 ] && [ $i -lt 50 ]; do usleep 100000; i=$((i+1)); done
if   [ -c /dev/tty1 ];    then exec >/dev/tty1 2>&1
elif [ -c /dev/console ]; then exec >/dev/console 2>&1
fi

# ---- sign of life (the penguin comes at the END, so it is what stays on screen) ----
echo ">>> userspace reached at $(cut -d' ' -f1 /proc/uptime)s -- probing..."
echo
echo "=== RK3518 stick initramfs probe $REPORT_VERSION ==="
echo
mount -t tmpfs    tmpfs    /tmp  2>/dev/null
mount -t debugfs  debugfs  /sys/kernel/debug 2>/dev/null && DBG=1 || DBG=0

LOG=/tmp/report.txt
{
  echo "### initramfs probe $REPORT_VERSION"
  echo "### date      : $(date -u 2>/dev/null)"
  echo "### uname     : $(uname -a)"
  echo "### cmdline   : $(cat /proc/cmdline 2>/dev/null)"
  echo

  echo "## DEVICE TREE (the questions the whole evening hinged on)"
  for n in model compatible; do
    echo "  $n        : $(tr -d '\0' < /proc/device-tree/$n 2>/dev/null)"
  done
  echo "  memory    :"
  for m in /proc/device-tree/memory*; do
    [ -e "$m" ] || continue
    r=$(od -An -tx1 -j 8 -N 16 "$m/reg" 2>/dev/null | tr -d ' \n')
    echo "    $m reg=$r"
  done
  echo "  /proc/meminfo MemTotal: $(grep -m1 MemTotal /proc/meminfo 2>/dev/null)"
  echo "  dt children of interest: $(ls /proc/device-tree/ 2>/dev/null | grep -iE 'serial|ethernet|vop|hdmi|gpu|sdio|mmc|display|remotectl|wireless|seekw' | tr '\n' ' ')"
  echo

  echo "## CPU"
  grep -E 'processor|model|Features|CPU part|Hardware|BogoMIPS' /proc/cpuinfo 2>/dev/null | sort -u | head -12
  echo "  online: $(cat /sys/devices/system/cpu/online 2>/dev/null)  possible: $(cat /sys/devices/system/cpu/possible 2>/dev/null)"
  echo

  echo "## MMC / eMMC  (health: recycled-part suspicion)"
  for d in /sys/block/mmcblk*; do
    [ -e "$d" ] || continue
    dev=$(basename "$d")
    echo "  --- $dev  type=$(cat $d/device/type 2>/dev/null)"
    echo "      name  : $(cat $d/device/name 2>/dev/null)   cid: $(cat $d/device/cid 2>/dev/null)"
    echo "      life_time    : $(cat $d/device/life_time 2>/dev/null)   (0x1..0xa=0-100%, 0xb=exceeded)"
    echo "      pre_eol_info : $(cat $d/device/pre_eol_info 2>/dev/null)   (0x1 ok,0x2 warn,0x3 urgent)"
    echo "      sectors      : $(cat $d/size 2>/dev/null)"
    echo "      hwpart        : $(ls -d $d/boot0 $d/boot1 2>/dev/null | tr '\n' ' ')"
    [ "$DBG" = 1 ] && echo "      extcsd key bytes:" && \
        sed -n 's/^.*\((EXT_CSD[^)]*)\)/      \1/p' /sys/kernel/debug/mmc0/mmc0:0001/extcsd 2>/dev/null | head -8
    echo "      partitions  :"
    ls /dev/disk/by-partlabel/ 2>/dev/null | sed 's/^/          /' | head -20
  done
  echo

  echo "## SDIO  (WiFi identification: is it really SeekWave SV6160L/EA6621Q?)"
  for c in /sys/bus/sdio/devices/*; do
    [ -e "$c" ] || continue
    echo "  $(basename $c) vendor=0x$(cat $c/vendor 2>/dev/null) device=0x$(cat $c/device 2>/dev/null) class=0x$(cat $c/class 2>/dev/null) drv=$(basename $(readlink -f $c/driver 2>/dev/null) 2>/dev/null)"
  done
  echo "  sdhci/dw-mmc hosts: $(ls -d /sys/class/mmc_host/* 2>/dev/null | tr '\n' ' ')"
  echo

  echo "## USB  (the one host port must show the stick, or nothing works)"
  for u in /sys/bus/usb/devices/*; do
    [ -e "$u/idVendor" ] || continue
    echo "  $(basename $u) $(cat $u/idVendor 2>/dev/null):$(cat $u/idProduct 2>/dev/null) $(cat $u/product 2>/dev/null) spd=$(cat $u/speed 2>/dev/null)"
  done
  echo "  block devices: $(ls /sys/block 2>/dev/null | tr '\n' ' ')"
  echo

  echo "## DISPLAY / DRM  (this is the console -- if empty, we are blind)"
  for cd in /sys/class/drm/*; do
    [ -e "$cd/status" ] || continue
    echo "  $(basename $cd) status=$(cat $cd/status 2>/dev/null) modes=$(cat $cd/modes 2>/dev/null | tr '\n' ',' | cut -c1-90)"
  done
  echo "  fbdev: $(ls /sys/class/graphics 2>/dev/null | tr '\n' ' ')"
  echo "  tty1 exists: $([ -e /dev/tty1 ] && echo yes || echo NO)"
  echo

  echo "## ETHERNET  (gmac0 + integrated PHY is enabled in the stock DT -- is there a PHY?)"
  for n in /sys/class/net/*; do
    [ -e "$n" ] || continue
    echo "  $(basename $n) addr=$(cat $n/address 2>/dev/null) state=$(cat $n/operstate 2>/dev/null) driver=$(basename $(readlink -f $n/device/driver 2>/dev/null) 2>/dev/null)"
  done
  echo "  mdio devices: $(ls -d /sys/bus/mdio_bus/devices/* 2>/dev/null | tr '\n' ' ')"
  for p in /sys/bus/mdio_bus/devices/*; do
    [ -e "$p/phy_id" ] && echo "    $(basename $p) phy_id=0x$(cat $p/phy_id 2>/dev/null)"
  done
  echo

  echo "## INPUT  (remotectl-pwm IR: if it probes, we have a remote without the USB port)"
  grep -A4 -E '^I:|^N:|^P:' /proc/bus/input/devices 2>/dev/null | head -40
  echo "  event devices: $(ls /dev/input 2>/dev/null | tr '\n' ' ')"
  echo

  echo "## KERNEL: modules + problems"
  echo "  loaded: $(lsmod 2>/dev/null | awk 'NR>1{print $1}' | tr '\n' ' ')"
  echo "  dmesg error/warn/probe-fail lines:"
  dmesg 2>/dev/null | grep -iE 'error|fail|unable|not found|deferred|probe|warn' | tail -40 | sed 's/^/    /'
  echo "  dmesg last 25:"
  dmesg 2>/dev/null | tail -25 | sed 's/^/    /'
} > $LOG 2>&1

echo; echo "---- report ----"; cat $LOG; echo "----------------"

# The recovery button is settled (saradc ch1, u-boot patch 0002), so no button test here.
say() { echo "$*"; echo "$*" >> $LOG; }

# ---- the USB stick: report channel + inbox ----
# Wait for a USB disk (up to 20 s; probe-2 lost a 1 s race). sd* is USB-only on this box;
# the eMMC (mmcblk*) is never a candidate.
i=0; while ! ls /dev/sd[a-z] >/dev/null 2>&1 && [ $i -lt 200 ]; do usleep 100000; i=$((i+1)); done
[ -n "$(ls /dev/sd[a-z] 2>/dev/null)" ] && { echo ">>> USB disk seen after $((i/10)).$((i%10)) s wait"; usleep 1000000; }
DEST=""
for b in /dev/sda1 /dev/sdb1 /dev/sda2 /dev/sda /dev/sdb; do
  [ -b "$b" ] || continue
  mkdir -p /usb
  if mount -t vfat "$b" /usb 2>/dev/null || mount -t ext4 "$b" /usb 2>/dev/null; then
    DEST=/usb; break
  fi
done

# ---- IR remote test: which keys does the box's own remote produce? ----
# The remotectl-pwm driver only emits key events for remotes whose usercode is in one of the
# 9 key tables of the vendor DT; an unknown remote shows up (if at all) in dmesg instead.
# Each evdev reader gets its own copy of every event, so one reader logs raw records to a
# file and a second prints them live on screen. arm64 input_event = 24 bytes:
#   u64 sec, u64 usec, u16 type, u16 code, s32 value  ->  od -tu2 fields 9,10,11 (+12)
# opt-in only (h96_remote=1 on the cmdline): it costs 30 s per boot
if grep -q 'h96_remote=1' /proc/cmdline; then
  say ""; say "## IR REMOTE TEST"
  EVS=""
  for e in /sys/class/input/event*; do
    n=$(cat $e/device/name 2>/dev/null)
    say "  /dev/${e##*/}: $n"
    case "$n" in *pwm*|*remote*|*cec*|*adc-keys*) EVS="$EVS ${e##*/}";; esac
  done
  dmesg -c >/dev/null 2>&1        # so the dmesg tail below holds only what the test caused
  say ">>> PRESS EVERY BUTTON ON THE REMOTE, one at a time, slowly (30 s)"
  say "    (up, down, left, right, OK, back, home, menu, vol+/-, mute, power, numbers...)"
  for ev in $EVS; do
    timeout 30 cat /dev/input/$ev > /tmp/ir-$ev.bin 2>/dev/null &
    timeout 30 od -An -v -tu2 -w24 /dev/input/$ev 2>/dev/null \
      | awk -v ev=$ev '$9==1 && $11==1 {print "    " ev ": key code " $10 " pressed"; fflush()}' &
  done
  wait
  say ">>> remote test done"
  for ev in $EVS; do
    say "  $ev key events (code value):"
    od -An -v -tu2 -w24 /tmp/ir-$ev.bin 2>/dev/null \
      | awk '$9==1 {printf "    code %d value %d\n", $10, $11}' >> $LOG
    say "    total records: $(( $(stat -c %s /tmp/ir-$ev.bin 2>/dev/null || echo 0) / 24 ))"
  done
  say "  dmesg during the test:"
  dmesg 2>/dev/null | tail -40 | sed 's/^/    /' >> $LOG
fi

# ---- USB tree, now that the stick wait is over (the early listing only sees root hubs) ----
# port path, ids, speed, product, and the driver of every interface: shows hubs, dongles,
# and whether anything bound to them.
{
  echo ""; echo "## USB TREE (after the stick wait, $(cut -d' ' -f1 /proc/uptime)s)"
  for d in /sys/bus/usb/devices/*; do
    [ -e "$d/idVendor" ] || continue
    drv=""
    for i in "$d"/*:*; do
      [ -e "$i/bInterfaceClass" ] || continue
      drv="$drv ${i##*/}=$(basename "$(readlink -f "$i/driver" 2>/dev/null)" 2>/dev/null)"
    done
    echo "  ${d##*/} $(cat $d/idVendor):$(cat $d/idProduct) spd=$(cat $d/speed) $(cat $d/product 2>/dev/null) |$drv"
  done
  echo "## net interfaces"
  for n in /sys/class/net/*; do echo "  ${n##*/} $(cat $n/address) $(cat $n/operstate) driver=$(basename "$(readlink -f $n/device/driver 2>/dev/null)" 2>/dev/null)"; done
  echo "## dmesg usb/hub/r8152 lines"
  dmesg | grep -iE 'usb [0-9]|hub|r8152|eth[0-9]|over-current|power' | tail -40 | sed 's/^/    /'
} >> $LOG 2>&1

# ---- the report goes to the stick ----
if [ -n "$DEST" ]; then
  cp $LOG $DEST/rk3518-report.txt
  echo "$REPORT_VERSION ok $(uname -r)" > $DEST/BOOT_OK
  sync
  echo ">>> REPORT WRITTEN TO USB STICK ($DEST/rk3518-report.txt)."
else
  echo ">>> no USB stick: the report is only on this screen (and in the rootfs, below)."
fi

# ---- our root filesystem in the old Android `userdata` partition ----
# Mounted ONLY if its ext4 label is h96root: findfs reads the superblock label, and Android's
# own userdata ext4 (FBE) has no such label, so it is never mounted (let alone read-write).
ROOTDEV=$(findfs LABEL=h96root 2>/dev/null)
mkdir -p /newroot
if [ -n "$ROOTDEV" ] && mount -t ext4 -o rw,noatime "$ROOTDEV" /newroot 2>/dev/null \
   && [ -x /newroot/bin/busybox ] && [ -e /newroot/etc/alpine-release ]; then
  # (not `-x /newroot/sbin/init`: that is a symlink to /bin/busybox and, tested from here,
  #  would resolve against the INITRAMFS root and pass even for a broken rootfs)
  echo ">>> root filesystem: $ROOTDEV (LABEL=h96root)"
  mkdir -p /newroot/var/log/h96
  cp $LOG /newroot/var/log/h96/initramfs-report.txt
  # inbox: a tar on the stick is unpacked into the rootfs, then renamed so it applies once.
  # This is how files (WiFi firmware, modules, config) reach the box without maskrom.
  if [ -n "$DEST" ] && [ -f $DEST/h96-inbox.tar.gz ]; then
    if tar -xzf $DEST/h96-inbox.tar.gz -C /newroot 2>/tmp/inbox.err; then
      mv $DEST/h96-inbox.tar.gz $DEST/h96-inbox.applied-$(date +%s).tar.gz
      echo ">>> inbox applied: h96-inbox.tar.gz -> rootfs"
    else
      echo "!! inbox FAILED, left in place:"; sed 's/^/   /' /tmp/inbox.err
    fi
  fi
  [ -n "$DEST" ] && { sync; umount /usb 2>/dev/null; }
  cat <<'TUX'

         .--.
        |o_o |
        |:_/ |
       //   \ \
      (|     | )
     /'\_   _/`\
     \___)=(___/

TUX
  echo "  Linux $(uname -r): kernel bring-up done at $(cut -d' ' -f1 /proc/uptime)s -- starting Alpine"
  sleep 2
  umount /sys/kernel/debug 2>/dev/null; umount /tmp 2>/dev/null
  mount --move /dev  /newroot/dev
  mount --move /proc /newroot/proc
  mount --move /sys  /newroot/sys
  exec switch_root /newroot /sbin/init
fi

# ---- no usable rootfs: stay in the initramfs, safely ----
[ -n "$DEST" ] && { sync; umount /usb 2>/dev/null; }
echo "!! no h96root filesystem ($([ -n "$ROOTDEV" ] && echo "found $ROOTDEV but the mount failed or it holds no Alpine" || echo "no partition labelled h96root"))"
cat <<'TUX'

         .--.
        |o_o |
        |:_/ |
       //   \ \
      (|     | )
     /'\_   _/`\
     \___)=(___/

TUX
echo "  Linux $(uname -r) on $(tr -d '\0' < /proc/device-tree/model 2>/dev/null)"
echo "  initramfs only. Recovery: hold the button at power-on -> maskrom."
while :; do sleep 3600; done
INIT_EOF

chmod +x "$BUILD/init"
# /dev/console must exist IN the archive: the kernel opens it for init's stdin/out/err
# BEFORE /init runs (probe-2 printed "unable to open an initial console"). We are not root,
# so mknod is out; append a second newc archive holding just the node (the kernel unpacks
# concatenated archives in order).
console_node() { python3 -I - <<'PY'
import sys
def ent(name, mode, rmaj=0, rmin=0):
    n = name.encode() + b'\0'
    h = b'070701' + b''.join(b'%08X' % v for v in
        (0, mode, 0, 0, 1, 0, 0, 0, 0, rmaj, rmin, len(n), 0))
    b = h + n
    return b + b'\0' * (-len(b) % 4)
out = ent('dev/console', 0o020600, 5, 1) + ent('TRAILER!!!', 0)
out += b'\0' * (-len(out) % 512)
sys.stdout.buffer.write(out)
PY
}
cd "$BUILD" && { find . | cpio --create --format='newc' 2>/dev/null; console_node; } | gzip -9 > "$OUT/initramfs.cpio.gz"
cd "$ROOT"
rm -rf "$BUILD"

SZ=$(stat -c%s "$OUT/initramfs.cpio.gz")
# Verify the archive itself, not the directory it came from: the modes stored IN the cpio
# are what the kernel applies, and `ln -sf` above has `|| true` on it, so a missing applet
# would only be discovered by whichever line of /init needs it -- on the device, quietly.
#
# Two listings, because they answer different questions. `-t` gives plain names to match
# exactly; `-t -v` gives modes but renders a symlink as "bin/sh -> /bin/busybox", so a
# pattern anchored at end-of-line matches NOTHING (my first version reported all 39 applets
# missing from an archive that had all 39).
NAMES=$(gzip -dc "$OUT/initramfs.cpio.gz" | cpio -t 2>/dev/null)
LIST=$(gzip -dc "$OUT/initramfs.cpio.gz" | cpio -t -v 2>/dev/null)
bad=""
for a in sh mount umount cat echo printf grep sed awk cut head tail wc ls lsmod insmod dmesg \
         uname date usleep poweroff reboot halt mknod mkdir sleep dd stat basename find \
         md5sum sha256sum tr xargs readlink blockdev losetup mdev diff od timeout findfs switch_root tar gunzip; do
  grep -qxF "bin/$a" <<<"$NAMES" || bad="$bad $a"
done
if [ -n "$bad" ]; then
  echo "!! applet links missing from the archive:$bad"; exit 1
fi
ok="$(echo $NAMES | wc -w) entries, all applet links present"
# GNU cpio -t stops at the first trailer, so check the appended console node by hand
gzip -dc "$OUT/initramfs.cpio.gz" | python3 -I -c '
import sys; d = sys.stdin.buffer.read(); i = d.rfind(b"dev/console\0")
h = d[i-110:i] if i >= 110 else b""
ok = h[:6] == b"070701" and int(h[14:22], 16) == 0o020600 and int(h[78:86], 16) == 5 and int(h[86:94], 16) == 1
print("  initramfs /dev/console: " + ("char 5:1 mode 0600" if ok else "MISSING/WRONG")); sys.exit(0 if ok else 1)' \
  || { echo "!! /dev/console node missing from the archive"; exit 1; }
# /init must be marked executable *in the archive*
ic=$(awk '$1 ~ /^-/ && ($NF == "init" || $NF == "./init"){print $1; exit}' <<<"$LIST")
case "$ic" in
  ?*x*) echo "  initramfs /init mode: $ic" ;;
  *) echo "!! initramfs /init mode is '$ic' -- no exec bit; the kernel would refuse to run it"; exit 1 ;;
esac
echo
echo "built: $OUT/initramfs.cpio.gz   ($SZ bytes, $(printf '%.1f' "$(echo "$SZ/1024" | bc -l)") kB)"
echo
echo "To embed it in the kernel (recommended -- u-boot then loads only Image + dtb):"
echo "  CONFIG_INITRAMFS_SOURCE=\"$OUT/initramfs.cpio.gz\""
echo "(tools/build-kernel.sh picks this up automatically on the next run)"
echo
echo "To load it as a separate initrd instead, use in the u-boot script:"
echo "  fatload usb 0:1 \$filesize initrd.img   # and pass rdaddr/rdsize"
