#!/bin/bash
# shot.sh <project> <scene name> <frame> <out.png> [extra env...]
export TMP='C:\MistyTry\r65tmp' TEMP='C:\MistyTry\r65tmp'
LOCK="/e/Misty Engine/Misty Engine Dev/MistyWork/gpu-timing.lock"
until mkdir "$LOCK" 2>/dev/null; do sleep 10; done; echo "r65 $(date)" > "$LOCK/owner"
proj=$1; scene=$2; frame=$3; out=$4; shift 4
env "$@" MISTY_SCENE="Assets/NodeGame/Scenes/$scene.scene" MISTY_PLAY=1 MISTY_TAB=Game MISTY_CAPTURE_FRAME=$frame MISTY_SCREENSHOT="C:/MistyTry/r65/shots/$out" MISTY_WINDOW_SIZE=${WSIZE:-1100x650} timeout 120 /c/MistyTry/r65/bin/misty.exe "C:/MistyTry/r65/$proj" 2>&1 | grep -v "^frame time\|^first editor\|scene complete\|renderer:" | tail -8
if grep -q r65 "$LOCK/owner" 2>/dev/null; then rm -rf "$LOCK"; fi
