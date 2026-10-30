#!/usr/bin/env bash
# Quick render regression: four offscreen captures of wce.exe (weather) and two of tiles, compared with the baseline (docs/baseline.md):
# identical, or within one level in under 0.05 % of the pixels (GPU float noise, and draw order: instancing draws a mesh's instances together, so surfaces at exactly the same
# depth can resolve the other way in a few pixels).
#   scripts/shots.sh [out dir] [reference dir]     (defaults build/shots, build/gates_base_perf)
# Environment: WCE_BIN (default target/release, or $CARGO_TARGET_DIR/release).
set -u
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT="${1:-$ROOT/build/shots}"
REF="${2:-$ROOT/build/gates_base_perf}"
BIN="${WCE_BIN:-${CARGO_TARGET_DIR:-$ROOT/target}/release}"
export WCE_GPU="${WCE_GPU:-AMD Radeon}"   # the reference pixels come from one GPU (docs/baseline.md)
mkdir -p "$OUT"
bad=0
# wce captures: byte-identical, or (GPU float noise: a handful of pixels one level off) within one level in under 0.05 % of the pixels
same_capture() {   # <name>: compares $OUT/<name> with $REF/<name>
    if cmp -s "$OUT/$1" "$REF/$1"; then echo "$1 identical"; return 0; fi
    local r
    r=$(python - "$OUT/$1" "$REF/$1" <<'PY'
import sys
import numpy as np
from PIL import Image
a = np.asarray(Image.open(sys.argv[1]).convert("RGB"), dtype=int); b = np.asarray(Image.open(sys.argv[2]).convert("RGB"), dtype=int)
if a.shape != b.shape: print("size differs"); sys.exit(1)
d = np.abs(a - b).max(axis=2)
n = int((d > 0).sum())
ok = d.max() <= 1 and n <= d.size * 5e-4
print("%d pixels differ, max %d level%s" % (n, d.max(), "" if ok else "  TOO MUCH"))
sys.exit(0 if ok else 1)
PY
    ) && { echo "$1 within one level ($r)"; return 0; }
    echo "$1 DIFFERS ($r)"; return 1
}
# offscreen captures through wce.exe (no window shows them): each environment opened in the weather tool, same camera and clock
for env in dry rain snow; do
    name="capture_${env}_1280x720.png"
    "$BIN/wce.exe" --tool weather --open "weather/charlemagne/med_sp_land_day_$env.environment" --cam 0.6 0.35 120 --frames 20 --capture 1280x720 "$(cygpath -w "$OUT/$name")" > "$OUT/$name.log" 2>&1 || bad=1
    same_capture "$name" || bad=1
done
name="capture_dry_800x600.png"
"$BIN/wce.exe" --tool weather --open "weather/charlemagne/med_sp_land_day_dry.environment" --cam 0.6 0.35 120 --frames 20 --capture 800x600 "$(cygpath -w "$OUT/$name")" > "$OUT/$name.log" 2>&1 || bad=1
same_capture "$name" || bad=1
# two tiles through the tile tool (terrain with polygons; a settlement with its real building models)
GAME="${WCE_GAME:-D:/SteamLibrary/steamapps/common/Total War Attila}"
KIT="$GAME/assembly_kit_battle_tilemap/raw_data/terrain/tiles/battle"
[ -d "$KIT" ] || KIT="$GAME/assembly_kit/raw_data/terrain/tiles/battle"   # one kit (assembly_kit) instead of the two tilemap kits
for t in atlantic_mountains/custom_monte_rosa/aa settlement_western_cities/western_city_rome/medium; do
    name="capture_tile_$(echo "$t" | tr '/' '_').png"
    "$BIN/wce.exe" --tile "$KIT/$t" --cam 0.8 0.7 900 --frames 3 --capture 1280x720 "$(cygpath -w "$OUT/$name")" > "$OUT/$name.log" 2>&1 || bad=1
    same_capture "$name" || bad=1
done
exit $bad
