#!/usr/bin/env bash
# Builds the PGO profile (roadmap A1).
#  1. ./gradlew :app:assembleRelease -PseedlessPgoMode=GENERATE   and install the APK (release is needed: Debug skips PGO)
#  2. Play 3-5 different games for 10+ minutes each, then close each game normally (the profile is written on close).
#  3. ./tools/pgo/collect.sh <ndk-dir>      (use the SAME NDK that built the APK)
set -euo pipefail
NDK="${1:?usage: collect.sh <path to NDK>}"
PKG="${PKG:-com.seedlessds.app}"
OUT="$(cd "$(dirname "$0")/../.." && pwd)/pgo"
TMP="$(mktemp -d)"
mkdir -p "$OUT"
adb shell "run-as $PKG ls files/pgo" | tr -d '\r' | grep '\.profraw$' | while read -r f; do
  adb exec-out "run-as $PKG cat files/pgo/$f" > "$TMP/$f"
done
ls "$TMP"/*.profraw >/dev/null
PROFDATA="$(find "$NDK" -name llvm-profdata -type f | head -n1)"
"$PROFDATA" merge -output="$OUT/recon.profdata" "$TMP"/*.profraw
echo "Wrote $OUT/recon.profdata. Commit it; release builds use it automatically (-PseedlessPgoMode=OFF to ignore)."
echo "Regenerate it when core/ or the NDK version changes."
