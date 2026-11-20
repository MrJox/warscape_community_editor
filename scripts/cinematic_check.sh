#!/usr/bin/env bash
# The Cinematic tool's gate (render): scripts/cinematic_smoke.txt twice, each in a throw-away settings folder (a copy of
# settings.toml only); both runs must pass and give identical shots.
#
#   scripts/cinematic_check.sh [out dir]
set -u
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT="${1:-$ROOT/build/cinematic_check}"
BIN="${WCE_BIN:-${CARGO_TARGET_DIR:-$ROOT/target}/release}"
SETTINGS_SRC="${WCE_SETTINGS_DIR:-$LOCALAPPDATA/wce/config}"
mkdir -p "$OUT"
bad=0
for run in 1 2; do
    dir="$OUT/settings$run"
    rm -rf "$dir"; mkdir -p "$dir"
    [ -f "$SETTINGS_SRC/settings.toml" ] && cp "$SETTINGS_SRC/settings.toml" "$dir/"
    sed -e "s|%SHOT1%|$(cygpath -m "$OUT/intro$run.png")|" -e "s|%SHOT2%|$(cygpath -m "$OUT/event$run.png")|" \
        "$ROOT/scripts/cinematic_smoke.txt" > "$OUT/smoke$run.txt"
    WCE_SETTINGS_DIR="$(cygpath -w "$dir")" "$BIN/wce.exe" --script "$(cygpath -w "$OUT/smoke$run.txt")" > "$OUT/run$run.log" 2>&1
    rc=$?
    echo "run $run: exit $rc"
    [ $rc = 0 ] || { bad=1; grep -i -E "fail|error" "$OUT/run$run.log" | tail -5; }
done
for shot in intro event; do
    if cmp -s "$OUT/${shot}1.png" "$OUT/${shot}2.png" && [ -s "$OUT/${shot}1.png" ]; then echo "$shot shot identical between runs"
    else echo "$shot shot DIFFERS between runs (or missing)"; bad=1; fi
done
[ $bad = 0 ] && echo OK || { echo FAILED; exit 1; }
