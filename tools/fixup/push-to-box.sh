#!/usr/bin/env bash
# push-to-box.sh -- install the current builds on a running box over ssh (no flash write):
#   build/h96ui/h96ui                  -> /usr/local/bin/h96ui (previous one kept as h96ui.prev)
#   build/video/video-stage.tar.gz     -> /  (MPP, librga, gstreamer-rockchip, h96player,
#                                         patched kmssink; /opt/rkffmpeg is left out: unused)
#   tools/fixup/h96-doctor, h96-fix    -> /usr/local/sbin
#   tools/fixup/box-packages.txt       -> apk add (only what is missing; the box needs internet)
# then restarts h96ui (unless --no-restart) and runs h96-doctor -q.
# Usage: H96=192.168.11.149 tools/fixup/push-to-box.sh [--no-restart]
# Builds: tools/h96ui-rs/build.sh, tools/video/build-mpp.sh (see docs/log/status-and-resume.md)
set -euo pipefail
ROOT=$(cd "$(dirname "$0")/../.." && pwd)
: "${H96:?set H96=<box ip>, e.g. H96=192.168.11.149}"
RESTART=1; [ "${1:-}" = --no-restart ] && RESTART=0
UI=$ROOT/build/h96ui/h96ui; VS=$ROOT/build/video/video-stage.tar.gz
for f in "$UI" "$VS"; do [ -f "$f" ] || { echo "missing $f: build it first"; exit 1; }; done
# list once, then grep a here-string: "tar | grep -q" fails under pipefail when grep exits
# early and tar dies of SIGPIPE (rules-and-lessons.md, 2026-11-20 -- and again 2026-11-22)
LIST=$(tar -tzf "$VS")
for m in ./usr/bin/h96player ./usr/lib/gstreamer-1.0/libgstkms.so ./usr/lib/gstreamer-1.0/libgstrockchipmpp.so; do
  grep -qx -- "$m" <<<"$LIST" || { echo "$VS lacks $m: run tools/video/build-mpp.sh (all stages)"; exit 1; }
done
grep -q "ARM aarch64" <<<"$(file "$UI")" || { echo "$UI is not an arm64 binary"; exit 1; }
GPB=$(sed -n 's/^GPB_VER=//p' "$ROOT/tools/video/build-mpp.sh")
H="$ROOT/tools/h96"
echo "== copying to $H96"
"$H" put "$UI" /tmp/h96ui.new
"$H" put "$VS" /tmp/video-stage.tar.gz
"$H" put "$ROOT/tools/fixup/h96-doctor" /tmp/h96-doctor
"$H" put "$ROOT/tools/fixup/h96-fix" /tmp/h96-fix
"$H" put "$ROOT/tools/fixup/box-packages.txt" /tmp/box-packages.txt
echo "== installing"
"$H" "RESTART=$RESTART GPB=$GPB sh -s" <<'REMOTE'
set -e
S=/usr/local/share/h96; mkdir -p $S
pk=$(grep -v '^#' /tmp/box-packages.txt | grep . | while read -r p; do apk info -e "$p" >/dev/null || echo "$p"; done)
[ -z "$pk" ] || { echo "apk add:" $pk; apk add -q $pk; }
K=/usr/lib/gstreamer-1.0/libgstkms.so
# keep Alpine's own kmssink once (h96-fix kmssink alpine)
if [ -f $K ] && [ ! -f $S/libgstkms.so.alpine ]; then
  if [ -f /root/libgstkms.so.alpine ]; then cp -a /root/libgstkms.so.alpine $S/; else cp -a $K $S/libgstkms.so.alpine; fi
fi
tar -xzf /tmp/video-stage.tar.gz -C / --exclude=./opt
cp -a $K $S/libgstkms.so; echo "$GPB" > $S/libgstkms.version
rm -rf /root/.cache/gstreamer-1.0
# h96player now comes from the tarball (/usr/bin); an older hand-copied one would win in PATH
[ -f /usr/local/bin/h96player ] && rm -f /usr/local/bin/h96player && echo "removed stale /usr/local/bin/h96player"
install -m 755 /tmp/h96-doctor /tmp/h96-fix /usr/local/sbin/
B=/usr/local/bin
if ! cmp -s /tmp/h96ui.new $B/h96ui; then
  [ -f $B/h96ui ] && cp -a $B/h96ui $B/h96ui.prev
  install -m 755 /tmp/h96ui.new $B/h96ui
  echo "h96ui: $($B/h96ui.prev -version 2>/dev/null || echo none) -> $($B/h96ui -version)"
else
  echo "h96ui unchanged: $($B/h96ui -version)"
fi
rm -f /tmp/h96ui.new /tmp/video-stage.tar.gz /tmp/h96-doctor /tmp/h96-fix /tmp/box-packages.txt
[ "$RESTART" = 1 ] && h96-fix player
h96-doctor -q || true
REMOTE
