#!/usr/bin/env bash
# The tile tool of wce.exe, scripted (scripts/tile_smoke.txt, tile_roundtrip.txt, tile_save.txt) on a small kit tile, in throw-away settings and output folders:
#  - the edits and their undo / redo behave (assert_* lines), twice, with an identical offscreen capture and identical saved files;
#  - the saved tile.ted differs from the original in exactly the edited line, the saved height map only inside the brush;
#  - the mouse and key paths work (scripts/tile_mouse.txt: click select, G grab, brush drag, polygon drawing, handle drag);
#  - everything undone and saved equals the originals byte for byte;
#  - save in place keeps a .bak, and undo + save puts the original bytes back;
#  - procedural walls (scripts/tile_walls.txt): drawn, attached to a fort building's ends, corners, undo, saved.
#   scripts/tile_check.sh [out dir]        Environment: WCE_BIN (default target/release, or $CARGO_TARGET_DIR/release), WCE_GAME
set -u
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT="${1:-$ROOT/build/tile_check}"; mkdir -p "$OUT"; OUT="$(cd "$OUT" && pwd)"
BIN="${WCE_BIN:-${CARGO_TARGET_DIR:-$ROOT/target}/release}"
SETTINGS_SRC="${WCE_SETTINGS_DIR:-$LOCALAPPDATA/wce/config}"
export WCE_GPU="${WCE_GPU:-AMD Radeon}"   # the reference pixels come from one GPU (docs/baseline.md)
GAME="${WCE_GAME:-D:/SteamLibrary/steamapps/common/Total War Attila}"
KIT="$GAME/assembly_kit_battle_tilemap/raw_data/terrain/tiles/battle"
SRC="$KIT/assembly_kit/khazad_wall_test/battlefield"
bad=0
fail() { echo "FAILED: $*"; bad=1; }

run_script() { # run_script <script name> <run> <tile dir> [second tile dir] -> $OUT/<name><run>/
    local name="$1" run="$2" tile="$3" tile2="${4:-}" dir="$OUT/$1$2"
    rm -rf "$dir"; mkdir -p "$dir/settings"; cp "$SETTINGS_SRC/settings.toml" "$dir/settings/" 2>/dev/null
    sed -e "s|%TILE2%|$([ -n "$tile2" ] && cygpath -m "$tile2")|g" -e "s|%TILE%|$(cygpath -m "$tile")|g" -e "s|%OUT%|$(cygpath -m "$dir")|g" "$ROOT/scripts/$name.txt" > "$dir/script.txt"
    WCE_SETTINGS_DIR="$(cygpath -w "$dir/settings")" "$BIN/wce.exe" --script "$(cygpath -w "$dir/script.txt")" > "$dir/run.log" 2>&1
    local rc=$?
    echo "$name run $run: exit $rc"
    [ $rc = 0 ] || { bad=1; grep -E "error|FAILED" "$dir/run.log" | head -5; }
}

run_script tile_gizmo 1 "$SRC"
# the ground colour (Phase 3): blended ground of the lothlorien 1x1 tile, another climate, flat, blended again = the first capture; all different from each other
LOTH="$KIT/tdd_custom_lothlorien/1x1/aa"
run_script tile_terrain 1 "$LOTH"
run_script tile_terrain 2 "$LOTH"
T1="$OUT/tile_terrain1"; T2="$OUT/tile_terrain2"
for f in blend.png desert.png flat.png blend2.png groups.png protection.png blend3.png detail.png; do
    cmp -s "$T1/$f" "$T2/$f" && [ -s "$T1/$f" ] && echo "terrain $f identical between runs" || fail "terrain $f differs between runs (or missing)"
done
cmp -s "$T1/blend.png" "$T1/blend3.png" && cmp -s "$T1/blend.png" "$T1/blend2.png" && echo "terrain: blended again = the first capture" || fail "terrain: blended again differs from the first capture"
for f in desert.png flat.png groups.png protection.png; do
    cmp -s "$T1/blend.png" "$T1/$f" && fail "terrain: $f equals the blended capture" || echo "terrain: $f differs from the blended capture"
done
# the ground shows its textures (the shader reads the layer's uv scale from ps_texture_array_cb[mapping + 1] = array element [mapping]: layer 0 once got scale 0 and was one flat colour):
# in the close capture no more than 1 % of the lit 8 x 8 pixel blocks may be flat (the broken build: 6 %, the right one: 0 %)
python - "$T1/detail.png" <<'PY' || bad=1
import sys
import numpy as np
from PIL import Image
a = np.asarray(Image.open(sys.argv[1]).convert("L"), dtype=float)
h, w = a.shape
b = a[:h // 8 * 8, :w // 8 * 8].reshape(h // 8, 8, w // 8, 8).transpose(0, 2, 1, 3).reshape(-1, 64)
lit = b.mean(1) > 40
flat = float((b.std(1)[lit] < 2.5).mean()) if lit.any() else 1.0
print("terrain detail: %d lit blocks, %.1f %% flat" % (lit.sum(), flat * 100))
sys.exit(0 if lit.sum() > 1000 and flat <= 0.01 else 1)
PY
# water (Phase 4): river ribbons of the ford tile, switched off and on, a stroke, the lake plane; captures identical between two runs, on differs from off
OSG_MIN="$KIT/tdd_settlement_dunedain_cities/dunedain_city_osgiliath/minor"
FORD="$KIT/river_ford/2x2_lb_rb_ford/aa"; LAKE="$KIT/lakes/10x10/aa"
run_script tile_water 1 "$FORD" "$LAKE"
run_script tile_water 2 "$FORD" "$LAKE"
for f in water_on.png water_off.png water_on2.png lake.png; do
    cmp -s "$OUT/tile_water1/$f" "$OUT/tile_water2/$f" && [ -s "$OUT/tile_water1/$f" ] && echo "water $f identical between runs" || fail "water $f differs between runs (or missing)"
done
cmp -s "$OUT/tile_water1/water_on.png" "$OUT/tile_water1/water_on2.png" && echo "water: on again = the first capture" || fail "water: on again differs from the first capture"
cmp -s "$OUT/tile_water1/water_on.png" "$OUT/tile_water1/water_off.png" && fail "water: off equals on" || echo "water: off differs from on"
# tile links (Phase 6a, read-only): the entry's targets and rules drawn on the cells they name (osgiliath minor: 2 targets, 2 rules)
run_script tile_links 1 "$OSG_MIN"
run_script tile_links 2 "$OSG_MIN"
for f in links_off.png links_on.png links_off2.png; do
    cmp -s "$OUT/tile_links1/$f" "$OUT/tile_links2/$f" && [ -s "$OUT/tile_links1/$f" ] && echo "links $f identical between runs" || fail "links $f differs between runs (or missing)"
done
cmp -s "$OUT/tile_links1/links_off.png" "$OUT/tile_links1/links_off2.png" && echo "links: off again = the first capture" || fail "links: off again differs from the first capture"
cmp -s "$OUT/tile_links1/links_off.png" "$OUT/tile_links1/links_on.png" && fail "links: on equals off" || echo "links: on differs from off"
# placement modes (Phase 2): landscape follows the terrain, freeform is free, a height stroke carries the landscape ones; all undone and saved equals the original
DESERT="$KIT/terrain_rugged_hills_desert/1x1/aa"
run_script tile_placement 1 "$DESERT"
for f in tile.ted height_map_0.png; do
    cmp -s "$DESERT/$f" "$OUT/tile_placement1/restored/$f" && echo "placement: undo all + save: $f equals the original" || fail "placement: undo all + save: $f differs from the original"
done
# layers: flags, structure, undo all, and the saved tile.ted equals the original (osgiliath_test: 55 MB)
OSG="$KIT/assembly_kit/osgiliath_test/battlefield"
# procedural walls, phase 1: drawn, ends attached, corners, undo; the saved tile.wce keeps the wall and the undone corner mark is gone
run_script tile_walls 1 "$DESERT"
W="$OUT/tile_walls1/saved/tile.wce"
[ "$(grep -c '<WALL_SPLINE' "$W" 2>/dev/null)" = 1 ] && ! grep -q 'sharp=' "$W" && echo "walls: the saved tile.wce has the wall, without the undone sharp mark" || fail "walls: the saved tile.wce lacks the wall or keeps an undone sharp mark"

run_script tile_layers 1 "$OSG"
run_script tile_scene_ui 1 "$OSG"   # the Scene panel with the scripted mouse: eye, cursor arrow, drag and drop, F2, Del
cmp -s "$OSG/tile.ted" "$OUT/tile_layers1/layers_restored/tile.ted" && echo "layers: undo all + save equals the original tile.ted" || fail "layers: undo all + save differs from the original tile.ted"
for run in 1 2; do run_script tile_smoke $run "$SRC"; done
for f in saved/tile.ted saved/height_map_0.png capture.png; do
    cmp -s "$OUT/tile_smoke1/$f" "$OUT/tile_smoke2/$f" && [ -s "$OUT/tile_smoke1/$f" ] && echo "$f identical between runs" || fail "$f differs between runs (or missing)"
done
# the mouse and the keys (select, grab, brush, polygon drawing, handle drag)
run_script tile_mouse 1 "$SRC"
# minimal edit
changed=$(diff <(tr -d '\r' < "$SRC/tile.ted") <(tr -d '\r' < "$OUT/tile_smoke1/saved/tile.ted") | grep -c '^>')
[ "$changed" = 1 ] && echo "saved tile.ted differs from the original in exactly the moved building's line" || fail "saved tile.ted differs in $changed lines (expected 1)"
python - "$SRC/height_map_0.png" "$OUT/tile_smoke1/saved/height_map_0.png" <<'PY' || bad=1
import sys
import numpy as np
from PIL import Image
a = np.asarray(Image.open(sys.argv[1])).astype(int); b = np.asarray(Image.open(sys.argv[2])).astype(int)
d = a != b
ys, xs = np.nonzero(d)
# the stroke is at (440, 470) m = pixel (568, 598) with a 12 px brush; the earlier raise at (457, 478) was undone
ok = d.any() and xs.min() >= 568 - 13 and xs.max() <= 568 + 13 and ys.min() >= 598 - 13 and ys.max() <= 598 + 13
print("saved height map: %d samples changed, inside the brush: %s" % (d.sum(), "yes" if ok else "NO"))
sys.exit(0 if ok else 1)
PY
# everything undone: the saved files equal the originals
run_script tile_roundtrip 1 "$SRC"
for f in tile.ted height_map_0.png; do
    cmp -s "$SRC/$f" "$OUT/tile_roundtrip1/restored/$f" && echo "undo all + save: $f equals the original" || fail "undo all + save: $f differs from the original"
done
[ -s "$OUT/tile_roundtrip1/edited/tile.ted" ] && echo "edited tile written and opened again" || fail "no edited tile"
# save in place on a copy of the kit tile
COPY="$OUT/kit/terrain/tiles/battle"
rm -rf "$OUT/kit"; mkdir -p "$COPY/assembly_kit/khazad_wall_test" "$COPY/_tile_database/TILES"
cp -r "$SRC" "$COPY/assembly_kit/khazad_wall_test/battlefield"
cp "$KIT/_tile_database/TILES/assembly_kit_khazad_wall_test.xml" "$COPY/_tile_database/TILES/"
run_script tile_save 1 "$COPY/assembly_kit/khazad_wall_test/battlefield"
for f in tile.ted height_map_0.png; do
    cmp -s "$SRC/$f" "$COPY/assembly_kit/khazad_wall_test/battlefield/$f" && echo "save in place, undo, save: $f equals the original" || fail "save in place, undo, save: $f differs from the original"
    cmp -s "$SRC/$f" "$COPY/assembly_kit/khazad_wall_test/battlefield/$f.bak" && echo "$f.bak is the original" || fail "$f.bak is not the original"
done
exit $bad
