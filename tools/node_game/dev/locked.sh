#!/bin/bash
# locked.sh <command...>: run under the shared GPU lock.
export TMP='C:\MistyTry\r65tmp' TEMP='C:\MistyTry\r65tmp'
LOCK="/e/Misty Engine/Misty Engine Dev/MistyWork/gpu-timing.lock"
until mkdir "$LOCK" 2>/dev/null; do sleep 5; done; echo "r65 $(date)" > "$LOCK/owner"
"$@"
if grep -q "^r65 " "$LOCK/owner" 2>/dev/null; then rm -rf "$LOCK"; fi
