#!/bin/bash
# cb.sh <log name> <cargo args...>: wait for a build slot (same slot folders as build-slot.sh), run cargo, release.
cd "/e/Misty Engine/Misty Engine Dev/MistyWork/r65-node-game" || exit 1
export TMP='C:\MistyTry\r65tmp' TEMP='C:\MistyTry\r65tmp' CARGO_INCREMENTAL=0 CARGO_TARGET_DIR='E:\MistyTargets\r65-node-game'
SLOT_DIR=/e/MistyTargets/slots
log=/c/MistyTry/r65tmp/$1.log; shift
MINE=""
while [ -z "$MINE" ]; do
  for n in 1 2 3; do
    if mkdir "$SLOT_DIR/slot-$n" 2>/dev/null; then echo "r65 $(date)" > "$SLOT_DIR/slot-$n/owner"; MINE="$SLOT_DIR/slot-$n"; break; fi
  done
  [ -z "$MINE" ] && sleep 3
done
echo "slot taken $(date)" > "$log"
cargo "$1" -j ${JOBS:-4} "${@:2}" >> "$log" 2>&1; code=$?
echo "exit $code $(date)" >> "$log"
if grep -q "^r65 " "$MINE/owner" 2>/dev/null; then rm -rf "$MINE"; fi
tail -25 "$log"
