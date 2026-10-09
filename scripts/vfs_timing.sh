#!/usr/bin/env bash
# Cold and warm start of the weather tool through each pack backend: the same command as the gates' dry screenshot
# (weather_editor --open ... --frames 20 --shot), so the "startup set" is exactly what that run mounts and extracts.
#   scripts/vfs_timing.sh [out dir]        (default build/vfs_timing)
# Cold = an empty cache folder (pack indexes and extracted files rebuilt); warm = the same folder again.
# Environment: WCE_BIN (default build/dev/bin/$WCE_CONFIG), WCE_CONFIG (RelWithDebInfo), WCE_TIMING_CACHE (cache root, short path).
set -u
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT="${1:-$ROOT/build/vfs_timing}"
BIN="${WCE_BIN:-$ROOT/build/dev/bin/${WCE_CONFIG:-RelWithDebInfo}}"
CACHE_ROOT="${WCE_TIMING_CACHE:-${LOCALAPPDATA:-/tmp}/awe_timing}"
mkdir -p "$OUT"
now() { date +%s.%N; }
for backend in server cli; do
    cache="$CACHE_ROOT/$backend"
    rm -rf "$cache"
    for phase in cold warm; do
        t0=$(now)
        WCE_RPFM_BACKEND=$backend WCE_CACHE_DIR="$(cygpath -w "$cache")" "$BIN/weather_editor.exe" \
            --open weather/charlemagne/med_sp_land_day_dry.environment --cam 0.6 0.35 120 --frames 20 \
            --shot "$(cygpath -w "$OUT/shot_${backend}_${phase}.png")" > "$OUT/log_${backend}_${phase}.txt" 2>&1
        rc=$?
        t1=$(now)
        printf '%-7s %-5s exit %d  %6.1f s\n' "$backend" "$phase" "$rc" "$(awk -v a="$t1" -v b="$t0" 'BEGIN{print a-b}')"
    done
done
