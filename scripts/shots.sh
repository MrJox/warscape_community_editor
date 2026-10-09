#!/usr/bin/env bash
# Quick render regression: the three reference weather shots and two offscreen captures, compared byte for byte with the Phase 3 baseline (docs/baseline.md).
#   scripts/shots.sh [out dir] [reference dir]     (defaults build/shots, build/gates_base_p3)
# Environment: WCE_BIN (default build/dev/bin/$WCE_CONFIG; WCE_CONFIG defaults to RelWithDebInfo).
set -u
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT="${1:-$ROOT/build/shots}"
REF="${2:-$ROOT/build/gates_base_p3}"
BIN="${WCE_BIN:-$ROOT/build/dev/bin/${WCE_CONFIG:-RelWithDebInfo}}"
mkdir -p "$OUT"
bad=0
for env in dry rain snow; do
    "$BIN/weather_editor.exe" --open "weather/charlemagne/med_sp_land_day_$env.environment" --cam 0.6 0.35 120 --frames 20 \
        --shot "$(cygpath -w "$OUT/shot_weather_$env.png")" || bad=1
    for f in "shot_weather_$env.png"; do
        if cmp -s "$OUT/$f" "$REF/$f"; then echo "$f identical"; else echo "$f DIFFERS"; bad=1; fi
    done
done
# offscreen capture (no window shows it): another size, same determinism
for spec in "dry 1280x720" "snow 800x600"; do
    set -- $spec
    name="capture_$1_$2.png"
    "$BIN/weather_editor.exe" --open "weather/charlemagne/med_sp_land_day_$1.environment" --cam 0.6 0.35 120 --frames 20         --capture "$2" "$(cygpath -w "$OUT/$name")" || bad=1
    if cmp -s "$OUT/$name" "$REF/$name"; then echo "$name identical"; else echo "$name DIFFERS"; bad=1; fi
done
exit $bad
