#!/bin/sh
# seg-speed.sh URL... -- for each HLS playlist URL: follow a master playlist to its first
# variant, download the newest media segment and print its size, time and throughput.
# Tells a slow server/network apart from a player problem (NOTES §35). Runs on the box.
abs() { case $2 in http*) echo "$2" ;; /*) echo "${1%%://*}://$(echo "${1#*://}" | cut -d/ -f1)$2" ;; *) echo "${1%/*}/$2" ;; esac; }
for U in "$@"; do
  P=$(wget -q -T 15 -O - "$U") || { echo "$U: playlist fetch failed"; continue; }
  V=$(echo "$P" | grep -v '^#' | grep . | head -1 | tr -d '\r')
  if echo "$P" | grep -q EXT-X-STREAM-INF; then     # master -> first variant
    U=$(abs "$U" "$V"); P=$(wget -q -T 15 -O - "$U") || { echo "$U: variant fetch failed"; continue; }
  fi
  S=$(echo "$P" | grep -v '^#' | grep . | tail -1 | tr -d '\r'); S=$(abs "$U" "$S")
  D=$(echo "$P" | sed -n 's/^#EXTINF:\([0-9.]*\).*/\1/p' | tail -1)
  t0=$(date +%s.%N 2>/dev/null || date +%s)
  B=$(wget -q -T 30 -O - "$S" | wc -c)
  t1=$(date +%s.%N 2>/dev/null || date +%s)
  awk -v b="$B" -v a="$t0" -v z="$t1" -v d="$D" -v u="$U" 'BEGIN { t = z - a; if (t <= 0) t = 0.001;
    printf "%-60.60s seg %.1fs  %7.0f KB in %5.2fs = %6.2f Mbit/s (stream needs %5.2f)\n",
      u, d, b/1024, t, b*8/t/1e6, (d > 0 ? b*8/d/1e6 : 0) }'
done
