#!/usr/bin/env bash
# Runs wce.exe on scripts/app_smoke.txt with a throw-away settings folder (the real wce.ini is copied in, layouts are not shared).
# The script ends with an offscreen capture; a second run must reproduce it byte for byte (fixed-step clock, deterministic readbacks).
#   scripts/app_check.sh [out dir]        Environment: WCE_BIN (default build/dev/bin/$WCE_CONFIG; RelWithDebInfo)
set -u
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT="$(cd "$(dirname "${1:-$ROOT/build/app_check/x}")" 2>/dev/null && pwd)/$(basename "${1:-$ROOT/build/app_check}")"
[ -n "${1:-}" ] || OUT="$ROOT/build/app_check"
BIN="${WCE_BIN:-$ROOT/build/dev/bin/${WCE_CONFIG:-RelWithDebInfo}}"
SETTINGS_SRC="${WCE_SETTINGS_DIR:-$LOCALAPPDATA/awe}"
mkdir -p "$OUT"
bad=0
for run in 1 2; do
    dir="$OUT/settings$run"
    rm -rf "$dir"; mkdir -p "$dir"
    [ -f "$SETTINGS_SRC/wce.ini" ] && cp "$SETTINGS_SRC/wce.ini" "$dir/"
    sed -e "s|%CAPTURE%|$(cygpath -m "$OUT/capture$run.png")|" -e "s|%SHOT%|$(cygpath -m "$OUT/shot$run.png")|" "$ROOT/scripts/app_smoke.txt" > "$OUT/smoke$run.txt"
    WCE_SETTINGS_DIR="$(cygpath -w "$dir")" "$BIN/wce.exe" --script "$(cygpath -w "$OUT/smoke$run.txt")" > "$OUT/run$run.log" 2>&1
    rc=$?
    echo "run $run: exit $rc"
    [ $rc = 0 ] || { bad=1; tail -5 "$OUT/run$run.log"; }
done
# first run: an empty settings folder, no WCE_GAME in the environment
ini_value() { sed -n "s/^$1=//p" "$SETTINGS_SRC/wce.ini" 2>/dev/null | head -1 | tr -d ''; }
GAME="${WCE_GAME:-$(ini_value game_dir)}"; RPFM="${WCE_RPFM_DIR:-$(ini_value rpfm_dir)}"
dir="$OUT/settings_first"; rm -rf "$dir"; mkdir -p "$dir"
text="$(cat "$ROOT/scripts/app_firstrun.txt")"; text="${text//%GAME%/$GAME}"; text="${text//%RPFM%/$RPFM}"; printf '%s
' "$text" > "$OUT/firstrun.txt"
env -u WCE_GAME -u ATTILA_DIR WCE_SETTINGS_DIR="$(cygpath -w "$dir")" "$BIN/wce.exe" --script "$(cygpath -w "$OUT/firstrun.txt")" > "$OUT/run_first.log" 2>&1
rc=$?
if [ $rc = 0 ] && [ -f "$dir/wce.ini" ]; then echo "first run: ok (wce.ini written)"; else echo "first run: FAILED (exit $rc)"; tail -5 "$OUT/run_first.log"; bad=1; fi
for f in capture shot; do
    if cmp -s "$OUT/${f}1.png" "$OUT/${f}2.png" && [ -s "$OUT/${f}1.png" ]; then echo "$f identical between runs"; else echo "$f DIFFERS between runs (or missing)"; bad=1; fi
done
exit $bad
