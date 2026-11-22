#!/bin/sh
# Build the H96Max Alpine root filesystem image for the old Android `userdata` partition.
#
# Runs INSIDE an arm64 Alpine container (qemu-user emulation on the PC), so apk, its
# post-install triggers and rc-update all run as the real target arch:
#   podman run --rm --arch arm64 -v "$PWD":/work:z \
#     docker.io/library/alpine:3.24.4 sh /work/tools/build-rootfs.sh
#
# Output: build/rootfs/h96root.img -- a 1 GiB ext4 (LABEL=h96root), grown to the whole
# 11.4 GiB partition on first boot (/etc/local.d/h96-firstboot.start). Writing 512 MiB
# instead of 11.4 GiB keeps the maskrom write short, and every byte of the image is
# written (zeros included), so no stale Android data is ever inside our filesystem.
# Touches no device.
set -eu
W=/work
OUT=$W/build/rootfs
R=/tmp/h96root                 # build inside the container: ownership is real root here
IMG=$OUT/h96root.img
SIZE_MB=1024   # content ~410 MB with mpv+bluez, ~600 MB with GStreamer; first boot grows it
REL=$(cat /etc/alpine-release)
BR=v${REL%.*}
PKGS="alpine-base busybox-mdev-openrc e2fsprogs e2fsprogs-extra
      wpa_supplicant iw wireless-regdb openssh-server chrony
      evtest htop nano less iproute2 usbutils ethtool
      mpv font-terminus bluez
      bluez-btmon libgpiod v4l-utils"
# the hardware playback stack (GStreamer etc.): one list for the image and for live boxes
PKGS="$PKGS $(grep -v '^#' $W/tools/fixup/box-packages.txt | tr '\n' ' ')"
VS=$W/build/video/video-stage.tar.gz     # tools/video/build-mpp.sh (all stages)
GPB=$(sed -n 's/^GPB_VER=//p' $W/tools/video/build-mpp.sh)
[ -f "$VS" ] || { echo "!! $VS missing: build it with tools/video/build-mpp.sh"; exit 1; }

echo "== Alpine $REL ($BR) aarch64 -> $R"
[ "$(uname -m)" = aarch64 ] || { echo "!! not an arm64 container"; exit 1; }
apk add -q e2fsprogs
rm -rf "$R"; mkdir -p "$R/etc/apk" "$OUT"
printf 'https://dl-cdn.alpinelinux.org/alpine/%s/main\nhttps://dl-cdn.alpinelinux.org/alpine/%s/community\n' "$BR" "$BR" > "$R/etc/apk/repositories"
cp -a /etc/apk/keys "$R/etc/apk/"
# shellcheck disable=SC2086
apk add -q --root "$R" --initdb --keys-dir "$R/etc/apk/keys" $PKGS

echo "== overlay"
cp -a "$W/tools/rootfs/overlay/." "$R/"
# everything the stick inbox carries (LAN dongle, sshd + key, WiFi modules and
# firmware, stick report): one source of truth, built by tools/make-inbox.sh
[ -f "$W/build/inbox/h96-inbox.tar.gz" ] || { echo "!! run tools/make-inbox.sh first"; exit 1; }
tar -xzf "$W/build/inbox/h96-inbox.tar.gz" -C "$R"
chown -R root:root "$R/etc" "$R/usr/local" "$R/root" "$R/lib/modules" "$R/lib/firmware"

echo "== video stack (NOTES §34-§38)"
# the patched kmssink replaces Alpine's: only valid for the gst-plugins-bad it was built from
have=$(chroot "$R" /sbin/apk list -I gst-plugins-bad 2>/dev/null | sed -n 's/^gst-plugins-bad-\([0-9.]*\)-r.*/\1/p')
[ "$have" = "$GPB" ] || { echo "!! Alpine now ships gst-plugins-bad $have, the kmssink patch is built for $GPB:"
                         echo "   bump GPB_VER/GPB_SHA in tools/video/build-mpp.sh and rebuild (STAGES=kms)"; exit 1; }
mkdir -p "$R/usr/local/share/h96"
cp -a "$R/usr/lib/gstreamer-1.0/libgstkms.so" "$R/usr/local/share/h96/libgstkms.so.alpine"
tar -xzf "$VS" -C "$R" --exclude=./opt      # /opt/rkffmpeg (ffmpeg-rockchip) is not used
cp -a "$R/usr/lib/gstreamer-1.0/libgstkms.so" "$R/usr/local/share/h96/libgstkms.so"
echo "$GPB" > "$R/usr/local/share/h96/libgstkms.version"
chroot "$R" /usr/bin/h96player >/dev/null 2>&1 || [ $? = 2 ] || { echo "!! h96player does not run in the image"; exit 1; }
echo "  MPP + librga + gstreamer-rockchip + h96player + patched kmssink ($GPB)"
chmod 700 "$R/root" "$R/root/.ssh"

echo "== services (OpenRC)"
svc() { chroot "$R" /sbin/rc-update add "$1" "$2" >/dev/null; echo "  $2: $1"; }
for s in devfs dmesg mdev hwdrivers; do svc $s sysinit; done
for s in bootmisc hostname swclock sysctl syslog seedrng; do svc $s boot; done
for s in local chronyd; do svc $s default; done     # chronyd: real time over the network (no RTC)
for s in mount-ro killprocs; do svc $s shutdown; done
# NOT savecache: with no RTC the clock restarts from a saved value, the saved dependency cache
# then looks newer than freshly added init scripts and they are silently skipped at boot
# (h96-wifi, 2026-11-21). Rebuilding the deptree each boot is cheap.
# networking is ours: h96-lan (USB dongles) and h96-wifi (payload); sshd is enabled by the payload

# OpenRC boot log (/var/log/rc.log): it found the skipped-service problem (research/notes §28)
grep -q '^rc_logger="YES"' "$R/etc/rc.conf" || echo 'rc_logger="YES"' >> "$R/etc/rc.conf"

echo "== accounts"
# root: "*" = no password matches, so no password login anywhere (tty2 getty, serial); the only
# way in is ssh with build/keys/h96max_ed25519. Not "!": OpenSSH (no PAM) reads that as a locked
# account and refuses the key as well. (2026-11-22, user: no root access without the key)
sed -i 's/^root:[^:]*:/root:*:/' "$R/etc/shadow"
grep -q '^root:\*:' "$R/etc/shadow" || { echo "!! could not set root's password to *"; exit 1; }
grep '^root:' "$R/etc/shadow" | cut -d: -f1-2 | sed 's/^/  shadow: /'

echo "== image: ${SIZE_MB} MiB ext4, LABEL=h96root"
rm -f "$IMG"
mkfs.ext4 -q -L h96root -m 1 -d "$R" "$IMG" "${SIZE_MB}M"
e2fsck -fn "$IMG" >/dev/null && echo "  e2fsck: clean"
dumpe2fs -h "$IMG" 2>/dev/null | grep -E 'volume name|UUID|features|Block count|Block size' | sed 's/^/  /'
chroot "$R" /sbin/apk info -v 2>/dev/null | sort > "$OUT/packages.txt"
du -sh "$R" | sed 's/^/  rootfs content: /'
echo "  packages: $(wc -l < "$OUT/packages.txt")  (build/rootfs/packages.txt)"
cd "$OUT" && sha256sum h96root.img > SHA256SUMS.txt && cat SHA256SUMS.txt
