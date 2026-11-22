#!/bin/sh
# seek-test.sh URL [label] -- drive h96player -i with seeks and report whether its main loop
# survives (NOTES §37). Extra h96player options in $SEEK_ARGS. Env passes through (H96PLAYER_VSINK, GST_PLUGIN_FEATURE_RANK, ...).
U=$1; L=${2:-test}; F=/tmp/seek-$$.ctl; LOG=/tmp/seek-$$.log
rm -f $F; mkfifo $F
H96PLAYER_TRACE=1 h96player -i $SEEK_ARGS "$U" < $F > $LOG 2>&1 &   # SEEK_ARGS: e.g. -u UA
P=$!; exec 3>$F
for i in $(seq 1 60); do grep -q "h96player: playing" $LOG && break; sleep 0.5; done
sleep 2; res=ok
for d in 120 -60 30 -10 10 60 -10 -30; do
  echo "seek $d" >&3; sleep 3
  n1=$(grep -c "h96player: status" $LOG); sleep 1.2; n2=$(grep -c "h96player: status" $LOG)
  pos=$(grep -o "pos=[0-9.]*" $LOG | tail -1)
  [ "$n2" -gt "$n1" ] || { res="HUNG after seek $d ($pos)"; break; }
done
exec 3>&-; sleep 3; kill -9 $P 2>/dev/null; rm -f $F
echo "$L: $res   last $(grep -o "pos=[0-9.]*" $LOG | tail -1)"
