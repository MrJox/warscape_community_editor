#!/usr/bin/env bash
# Quick render regression: the three reference weather shots (legacy window) and four offscreen captures of wce.exe, compared byte for byte with the baseline (docs/baseline.md).
#   scripts/shots.sh [out dir] [reference dir]     (defaults build/shots, build/gates_base_p5)
# Environment: WCE_BIN (default build/dev/bin/$WCE_CONFIG; WCE_CONFIG defaults to RelWithDebInfo).
set -u
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT="${1:-$ROOT/build/shots}"
REF="${2:-$ROOT/build/gates_base_p5}"
BIN="${WCE_BIN:-$ROOT/build/dev/bin/${WCE_CONFIG:-RelWithDebInfo}}"
export WCE_GPU="${WCE_GPU:-AMD Radeon}"   # the reference pixels come from one GPU (docs/baseline.md)
mkdir -p "$OUT"
bad=0
for env in dry rain snow; do
    "$BIN/weather_editor.exe" --open "weather/charlemagne/med_sp_land_day_$env.environment" --cam 0.6 0.35 120 --frames 20 \
        --shot "$(cygpath -w "$OUT/shot_weather_$env.png")" || bad=1
    for f in "shot_weather_$env.png"; do
        if cmp -s "$OUT/$f" "$REF/$f"; then echo "$f identical"; else echo "$f DIFFERS"; bad=1; fi
    done
done
# offscreen captures through wce.exe (no window shows them): each environment opened in the weather tool, same camera and clock
for env in dry rain snow; do
    name="capture_${env}_1280x720.png"
    "$BIN/wce.exe" --tool weather --open "weather/charlemagne/med_sp_land_day_$env.environment" --cam 0.6 0.35 120 --frames 20 --capture 1280x720 "$(cygpath -w "$OUT/$name")" > "$OUT/$name.log" 2>&1 || bad=1
    if cmp -s "$OUT/$name" "$REF/$name"; then echo "$name identical"; else echo "$name DIFFERS"; bad=1; fi
done
name="capture_dry_800x600.png"
"$BIN/wce.exe" --tool weather --open "weather/charlemagne/med_sp_land_day_dry.environment" --cam 0.6 0.35 120 --frames 20 --capture 800x600 "$(cygpath -w "$OUT/$name")" > "$OUT/$name.log" 2>&1 || bad=1
if cmp -s "$OUT/$name" "$REF/$name"; then echo "$name identical"; else echo "$name DIFFERS"; bad=1; fi
exit $bad
