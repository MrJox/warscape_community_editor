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
# The reference pixels come from one GPU (docs/baseline.md): this machine has two and Windows may default to either.
export WCE_GPU="${WCE_GPU:-AMD Radeon}"
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
# The offscreen capture restarts the renderer's temporal state, so it is a pure function of the scene: byte-identical.
# The whole-window shot shows the live viewport, whose auto exposure carries about 1e-6 of GPU float noise from frame to frame:
# a few pixels differ by one level (docs/baseline.md), so it is compared with a tolerance of 2 levels.
if cmp -s "$OUT/capture1.png" "$OUT/capture2.png" && [ -s "$OUT/capture1.png" ]; then echo "capture identical between runs"; else echo "capture DIFFERS between runs (or missing)"; bad=1; fi
if cmp -s "$OUT/shot1.png" "$OUT/shot2.png" && [ -s "$OUT/shot1.png" ]; then echo "shot identical between runs"
elif [ -s "$OUT/shot1.png" ] && [ -s "$OUT/shot2.png" ] && python -c "
import sys
import numpy as np
from PIL import Image
a = np.asarray(Image.open(sys.argv[1]).convert('RGB'), dtype=int); b = np.asarray(Image.open(sys.argv[2]).convert('RGB'), dtype=int)
sys.exit(0 if a.shape == b.shape and np.abs(a - b).max() <= 2 else 1)" "$OUT/shot1.png" "$OUT/shot2.png"; then echo "shot within 2 levels between runs (live exposure noise)"
else echo "shot DIFFERS between runs (or missing)"; bad=1; fi
exit $bad
