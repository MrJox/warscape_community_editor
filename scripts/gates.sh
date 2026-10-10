#!/usr/bin/env bash
# Runs every gate of both editors and writes one output file per gate to <out dir> (default build/gates).
# Compare two runs with: diff -r <run a> <run b>   (the summary lines must stay identical, see docs/baseline.md).
#
#   scripts/gates.sh [out dir] [--corpus] [--render]
#     --corpus  also runs the multi-hour tile corpus gates (roundtrip, scene_check over the whole manifest)
#     --render  also renders the reference screenshots (opens editor windows for a few seconds)
#
# Inputs are fixed so runs are comparable. Override with environment variables:
#   WCE_GAME   game folder (contains the assembly_kit_* folders and data/)
#   WCE_CACHE  extracted-file cache of the VFS (holds the weather/lighting/vfx files the XML gates walk)
#   WCE_BIN    folder with the built executables (default build/dev/bin/$WCE_CONFIG; WCE_CONFIG defaults to RelWithDebInfo)
#   WCE_CBS    a dump_cbs.py output for env_vs_dump (skipped when missing)
set -u
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT="${1:-$ROOT/build/gates}"; shift || true
CORPUS=0; RENDER=0
for a in "$@"; do case "$a" in --corpus) CORPUS=1;; --render) RENDER=1;; esac; done

GAME="${WCE_GAME:-D:/SteamLibrary/steamapps/common/Total War Attila}"
CACHE="${WCE_CACHE:-$LOCALAPPDATA/awe/x}"
CBS="${WCE_CBS:-$TEMP/claude/cbs.txt}"
WB="${WCE_BIN:-$ROOT/build/dev/bin/${WCE_CONFIG:-RelWithDebInfo}}"   # one folder for every executable since Phase 1
TB="$WB"
MANIFEST="$ROOT/manifest/corpus.tsv"
BKIT="$GAME/assembly_kit_battle_tilemap"
TILE="$BKIT/raw_data/terrain/tiles/battle/atlantic_mountains/custom_monte_rosa/aa"

# The reference pixels come from one GPU (docs/baseline.md): this machine has two and Windows may default to either.
export WCE_GPU="${WCE_GPU:-AMD Radeon}"
mkdir -p "$OUT"
FAILED=()
run() { # run <name> <command...>: output (stdout+stderr) to <name>.txt, exit code appended
    local name="$1"; shift
    local t0=$SECONDS
    "$@" > "$OUT/$name.txt" 2>&1
    local rc=$?
    echo "exit $rc" >> "$OUT/$name.txt"
    printf '%-20s exit %-3s %4ss\n' "$name" "$rc" "$((SECONDS - t0))"
    [ "$rc" = 0 ] || FAILED+=("$name")
}

# ---- weather gates
run xml_roundtrip     "$WB/xml_roundtrip.exe" "$CACHE"
run foundation_check  "$WB/foundation_check.exe"
run editor_check      "$WB/editor_check.exe"
run vfs_compare       "$WB/vfs_probe.exe" --compare   # cold mount through rpfm_server and rpfm_cli: same pack order, winners and bytes (needs a running or startable rpfm_server)
run format_check      "$WB/format_check.exe" "$CACHE"
run variation_check   "$WB/variation_check.exe"
run rmv2_check        "$WB/rmv2_check.exe"
run vfx_check_rain    "$WB/vfx_check.exe" "rain heavy"
[ -f "$CBS" ] && run env_vs_dump_all "$WB/env_vs_dump.exe" --all "$CBS"

# ---- tile gates
# monte_rosa's tile.ted has no SCENE_GRAPH (it crashed unit_tests before the Phase 1 fix, docs/baseline.md): the second file checks that
run perf_check        "$WB/perf_check.exe" "$BKIT/raw_data/terrain/tiles/battle/assembly_kit/khazad_wall_test/battlefield" "$TILE"
run xml_splice_check  "$WB/xml_splice_check.exe" "$BKIT/raw_data/terrain/tiles/battle/assembly_kit/khazad_wall_test/battlefield/tile.ted" 60
run unit_tests        "$TB/unit_tests.exe" "$BKIT/raw_data/terrain/tiles/battle/assembly_kit/khazad_wall_test/battlefield/tile.ted" "$TILE/tile.ted"
run height_check      "$TB/height_check.exe" "$TILE/height_map_0.png"
# the tile database entry as the root of a tile: entry round trip (both kits), the index and its cache, open by entry = by folder = by tile.ted, a tile.ted outside the kit (plains/3x1/3x1_aa has a corrupt height map: it must fail the same way each time)
run tile_entry_check  "$TB/tile_entry_check.exe" "$GAME" "$BKIT/raw_data/terrain/tiles/battle/assembly_kit/khazad_wall_test/battlefield" "$TILE" "$BKIT/raw_data/terrain/tiles/battle/plains/3x1/3x1_aa"
run weather_check     "$TB/weather_check.exe" "$BKIT/raw_data/warscape_asset_variation_db/environment_map.xml"
# the terrain colour (Phase 3): DXT decoding, the climates of the kit, the blend map baked with the game's textures
run terrain_check     "$TB/terrain_check.exe" "$GAME"
run tdb_build         "$TB/tdb_build.exe" "$BKIT/raw_data/terrain/tiles/battle/_tile_database" - "$BKIT/working_data/terrain/tiles/battle/tile_database.bin"
if [ "$CORPUS" = 1 ]; then
    run roundtrip     "$TB/roundtrip.exe" "$MANIFEST" "$GAME"
    run scene_check   "$TB/scene_check.exe" "$MANIFEST" "$GAME"
    run tile_entry_check_all "$TB/tile_entry_check.exe" "$GAME" --all   # every kit tile opens by folder, tile.ted and entry as the same document (about 80 s)
    run placement_check "$TB/placement_check.exe" "$BKIT"   # the tool's building placement against TEd's own building_list.xml export (Phase 0 oracle of docs/plans/tile_ted_parity.md)
    run tile_open_check env WCE_BIN="$WB" bash "$ROOT/scripts/tile_open_check.sh" "$OUT/tile_open_check"   # wce.exe opens one tile of every tile set and renders a frame
fi

# ---- render gates (fixed camera and inputs; see docs/baseline.md for what is deterministic; the pixel regression is scripts/shots.sh)
if [ "$RENDER" = 1 ]; then
    run app_check     env WCE_BIN="$WB" bash "$ROOT/scripts/app_check.sh" "$OUT/app_check"   # wce.exe scripted: tools, documents, selection, gizmo edits, undo, layouts, first run, capture; two runs identical
    run weather_tool_check env WCE_BIN="$WB" bash "$ROOT/scripts/weather_check.sh" "$OUT/weather_tool_check"   # the weather tool of wce.exe scripted: documents, structural edits, save (minimal edit), weather database, capture; two runs identical
    run tile_tool_check env WCE_BIN="$WB" bash "$ROOT/scripts/tile_check.sh" "$OUT/tile_tool_check"   # the tile tool of wce.exe scripted: edits, undo, mouse and keys, save (minimal edit, byte-identical round trip, .bak); two runs identical
    run tile_entry_script env WCE_BIN="$WB" bash "$ROOT/scripts/tile_entry_run.sh" "$OUT/tile_entry_script"   # the tile tool opens tiles by entry, name, folder and tile.ted; assert_entry; the tile browser's rows
    run cross_tool_check env WCE_BIN="$WB" bash "$ROOT/scripts/cross_tool_check.sh" "$OUT/cross_tool_check"   # wce.exe scripted: a tile and a weather file open at once, the weather previewed on the tile, one edit in each, the unsaved-changes prompts, Save All writes only the two edited spans; two runs identical
    run render_check  "$WB/render_check.exe"   # scene, picking, terrain, overlays and capture through a headless D3D11 device
fi

echo "outputs in $OUT"
if [ ${#FAILED[@]} -gt 0 ]; then echo "FAILED: ${FAILED[*]}"; exit 1; fi
