#!/bin/sh
# Hardware-video userspace for the box (Alpine arm64, musl), built in the arm64 Alpine container:
#   podman run --rm --arch arm64 -v ~/Sources/ClaudePlayground/H96Max:/work:z \
#     -e STAGES="mpp rga ffmpeg gst" docker.io/library/alpine:3.24.4 sh /work/tools/video/build-mpp.sh
# Stages (default all, in order; each installs into build/video/stage, which persists):
#   mpp     librockchip_mpp + mpi_dec_test & co   (downloads/mpp, rockchip-linux develop)
#   rga     librga from source                    (downloads/librga, nyanmisaka/rk-mirrors jellyfin-rga;
#                                                   airockchip/librga is prebuilt glibc only)
#   ffmpeg  ffmpeg-rockchip (h264_rkmpp/hevc_rkmpp, RGA filters) into /opt/rkffmpeg, so it never
#           replaces Alpine's libavcodec that mpv links against
#   gst     gstreamer-rockchip: mppvideodec (+ rga/dmaheap)
#   kms     kmssink (libgstkms.so) from gst-plugins-bad = Alpine's exact version, sha256-pinned,
#           + tools/video/gst-plugins-bad/*.patch (anamorphic SD fix, NOTES §35). Replaces
#           Alpine's /usr/lib/gstreamer-1.0/libgstkms.so on the box: re-apply after an apk upgrade
#           of gst-plugins-bad (and bump GPB_VER/GPB_SHA to the new version).
#   player  h96player (tools/video/h96player.c): the playbin front-end h96ui runs (headers,
#           prebuffer, parseable status)
# Sources are built out of tree; our patches (tools/video/<pkg>/*.patch) are applied, idempotently. Output: build/video/stage + video-stage.tar.gz
set -eu
ROOT=${ROOT:-/work}
OUT=$ROOT/build/video
STAGE=$OUT/stage
STAGES=${STAGES:-mpp rga ffmpeg gst kms player}
GPB_VER=1.28.3
GPB_SHA=4213f43ddb875bb141e5040e97735579d74665bec3d17b51052aade395b83f00
J=$(nproc)
mkdir -p "$STAGE"

apk add --no-cache build-base cmake meson ninja pkgconf linux-headers libdrm-dev bsd-compat-headers >/dev/null
# the container is throwaway: earlier stages' output goes into its / so later stages just find it
cp -a "$STAGE/." /

for s in $STAGES; do echo "=== stage $s"; case $s in
mpp)
  apk add --no-cache git >/dev/null
  for p in "$ROOT"/tools/video/mpp/*.patch; do   # applied to the submodule, skipped if already in
    git -C "$ROOT/downloads/mpp" apply --reverse --check "$p" 2>/dev/null ||
      git -C "$ROOT/downloads/mpp" apply "$p"
  done
  B=$OUT/mpp-build; rm -rf "$B"
  cmake -S "$ROOT/downloads/mpp" -B "$B" -G Ninja -DCMAKE_BUILD_TYPE=Release \
    -DCMAKE_INSTALL_PREFIX=/usr -DBUILD_TEST=ON -DBUILD_SHARED_LIBS=ON
  ninja -C "$B" -j"$J"; DESTDIR=$STAGE ninja -C "$B" install; ninja -C "$B" install >/dev/null ;;
rga)
  B=$OUT/rga-build; rm -rf "$B"
  meson setup "$B" "$ROOT/downloads/librga" --prefix=/usr --libdir=lib --buildtype=release \
    -Dlibdrm=true -Dlibrga_demo=false -Dcpp_args="['-Dmmap64=mmap','-include','sys/cdefs.h']"  # musl: no mmap64 (>=1.2.4), no implicit cdefs
  ninja -C "$B" -j"$J"; DESTDIR=$STAGE ninja -C "$B" install; ninja -C "$B" install >/dev/null ;;
ffmpeg)
  apk add --no-cache nasm openssl-dev alsa-lib-dev zlib-dev >/dev/null
  B=$OUT/ffmpeg-build; rm -rf "$B"; mkdir -p "$B"; cd "$B"
  "$ROOT/downloads/ffmpeg-rockchip/configure" --prefix=/opt/rkffmpeg \
    --extra-ldflags="-Wl,-rpath,/opt/rkffmpeg/lib" \
    --enable-gpl --enable-version3 --enable-shared --disable-static --disable-doc --disable-ffplay \
    --enable-libdrm --enable-rkmpp --enable-rkrga --enable-openssl --enable-alsa
  make -j"$J"; make DESTDIR=$STAGE install; make install >/dev/null; cd "$ROOT" ;;
gst)
  apk add --no-cache gstreamer-dev gst-plugins-base-dev >/dev/null
  B=$OUT/gst-build; rm -rf "$B"
  meson setup "$B" "$ROOT/downloads/gstreamer-rockchip" --prefix=/usr --libdir=lib --buildtype=release \
    -Drockchipmpp=enabled -Drga=enabled -Ddmaheap=enabled \
    -Drkximage=disabled -Dgpu=disabled -Dkmssrc=disabled -Dvpxalphadec=disabled
  ninja -C "$B" -j"$J"; DESTDIR=$STAGE ninja -C "$B" install; ninja -C "$B" install >/dev/null ;;
kms)
  apk add --no-cache gstreamer-dev gst-plugins-base-dev xz curl >/dev/null
  apk add --no-cache gst-plugins-bad >/dev/null
  ver=$(apk list -I gst-plugins-bad 2>/dev/null | sed -n 's/^gst-plugins-bad-\([0-9.]*\)-r.*/\1/p' | head -1)
  [ "$ver" = "$GPB_VER" ] || { echo "Alpine has gst-plugins-bad $ver, script pins $GPB_VER: update GPB_VER/GPB_SHA"; exit 1; }
  T=$OUT/src/gst-plugins-bad-$GPB_VER.tar.xz; mkdir -p "$OUT/src"
  [ -f "$T" ] || curl -sSfL -o "$T" "https://gstreamer.freedesktop.org/src/gst-plugins-bad/gst-plugins-bad-$GPB_VER.tar.xz"
  echo "$GPB_SHA  $T" | sha256sum -c -
  S=$OUT/src/gst-plugins-bad-$GPB_VER; rm -rf "$S"; tar -C "$OUT/src" -xf "$T"
  for p in "$ROOT"/tools/video/gst-plugins-bad/*.patch; do patch -d "$S" -p1 < "$p"; done
  B=$OUT/kms-build; rm -rf "$B"
  meson setup "$B" "$S" --prefix=/usr --libdir=lib --buildtype=release \
    -Dauto_features=disabled -Dkms=enabled -Dexamples=disabled -Dtests=disabled -Dnls=disabled
  ninja -C "$B" -j"$J" sys/kms/libgstkms.so
  install -Dm755 "$B/sys/kms/libgstkms.so" "$STAGE/usr/lib/gstreamer-1.0/libgstkms.so" ;;
player)
  apk add --no-cache gstreamer-dev gst-plugins-base-dev >/dev/null
  mkdir -p "$STAGE/usr/bin"
  cc -O2 -Wall -Wextra -Wno-unused-parameter -o "$STAGE/usr/bin/h96player" "$ROOT/tools/video/h96player.c" \
    $(pkg-config --cflags --libs gstreamer-1.0 gstreamer-video-1.0) ;;
*) echo "unknown stage $s"; exit 2 ;;
esac; done

# --- package (runtime files only) --------------------------------------------------------
P=$OUT/pkg; rm -rf "$P"; mkdir -p "$P"
tar -C "$STAGE" --exclude=./usr/include --exclude=./opt/rkffmpeg/include --exclude=pkgconfig \
  --exclude='*.a' --exclude=./opt/rkffmpeg/share -cf - . | tar -C "$P" -xf -
find "$P" -type f \( -name '*.so*' -o -path '*/bin/*' \) -exec strip --strip-unneeded {} + 2>/dev/null || true
tar -C "$P" -czf "$OUT/video-stage.tar.gz" .
ls -l "$OUT/video-stage.tar.gz"; du -sh "$P"
