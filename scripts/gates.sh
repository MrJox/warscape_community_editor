#!/usr/bin/env bash
# Runs every gate and writes one output file per gate to <out dir> (default build/gates).
# Compare two runs with: diff -r <run a> <run b>   (the summary lines must stay identical, see docs/baseline.md).
#
#   scripts/gates.sh [out dir] [--corpus] [--render]
#     --corpus  also runs the multi-hour tile corpus gates (roundtrip, scene_check over the whole manifest)
#     --render  also renders the reference screenshots (opens editor windows for a few seconds)
#
# Inputs are fixed so runs are comparable. Override with environment variables:
#   WCE_GAME   game folder (contains data/ and the kit: assembly_kit_battle_tilemap + assembly_kit_campaign_tilemap, or one assembly_kit)
#   WCE_CACHE  extracted-file cache of the VFS (holds the weather/lighting/vfx files the XML gates walk)
#   WCE_BIN    folder with the built wce binary for the scripted app gates (default target/release, or $CARGO_TARGET_DIR/release)
#   WCE_CBS    a dump_cbs.py output for env_vs_dump (skipped when missing)
set -u
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT="${1:-$ROOT/build/gates}"; shift || true
CORPUS=0; RENDER=0
for a in "$@"; do case "$a" in --corpus) CORPUS=1;; --render) RENDER=1;; esac; done

GAME="${WCE_GAME:-D:/SteamLibrary/steamapps/common/Total War Attila}"
CACHE="${WCE_CACHE:-$LOCALAPPDATA/awe/x}"
CBS="${WCE_CBS:-$TEMP/claude/cbs.txt}"
WB="${WCE_BIN:-${CARGO_TARGET_DIR:-$ROOT/target}/release}"
MANIFEST="$ROOT/manifest/corpus.tsv"
BKIT="$GAME/assembly_kit_battle_tilemap"
TILE="$BKIT/raw_data/terrain/tiles/battle/atlantic_mountains/custom_monte_rosa/aa"

# The kit(s) for the crate tests that read kit tiles (wce_tile_io's kit_samples).
case "${OSTYPE:-}" in msys*|cygwin*|win*) SEP=';';; *) SEP=':';; esac
if [ -d "$GAME/assembly_kit/raw_data/terrain/tiles" ]; then
    export WCE_ATTILA_KIT="${WCE_ATTILA_KIT:-$GAME/assembly_kit}"
else
    export WCE_ATTILA_KIT="${WCE_ATTILA_KIT:-$BKIT$SEP$GAME/assembly_kit_campaign_tilemap}"
fi

export WCE_ATTILA_GAME="${WCE_ATTILA_GAME:-$GAME}"   # wce_tile_io's pack_samples read the game's packs

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

ex() { # ex <crate> <example> [args...]: a Cargo example of the workspace, release build
    local crate="$1" example="$2"; shift 2
    cargo run --release -q --manifest-path "$ROOT/Cargo.toml" -p "$crate" --example "$example" -- "$@"
}

# ---- unit tests (the C++ foundation_check, editor_check, unit_tests and height_check are crate tests)
run tests             cargo test --release -q --manifest-path "$ROOT/Cargo.toml" --workspace

# ---- weather gates
run xml_roundtrip     ex wce_formats xml_roundtrip "$CACHE"
run format_check      ex wce_formats format_check "$CACHE"
run variation_check   ex wce_weather variation_check
run rmv2_check        ex wce_formats rmv2_check "$CACHE"
run vfx_check_rain    ex wce_assets vfx_check "rain heavy"
[ -f "$CBS" ] && run env_vs_dump_all ex wce_render env_vs_dump --all "$CBS"

# ---- tile gates
run perf_check        ex wce_tile_tool perf_check "$BKIT/raw_data/terrain/tiles/battle/assembly_kit/khazad_wall_test/battlefield" "$TILE"
run xml_splice_check  ex wce_formats xml_splice_check "$BKIT/raw_data/terrain/tiles/battle/assembly_kit/khazad_wall_test/battlefield/tile.ted" 60
# the tile database entry as the root of a tile: entry round trip (both kits), the index and its cache, open by entry = by folder = by tile.ted, a tile.ted outside the kit (plains/3x1/3x1_aa has a corrupt height map: it must fail the same way each time)
run tile_entry_check  ex wce_tile tile_entry_check "$GAME" "$BKIT/raw_data/terrain/tiles/battle/assembly_kit/khazad_wall_test/battlefield" "$TILE" "$BKIT/raw_data/terrain/tiles/battle/plains/3x1/3x1_aa"
# the tile round trip (docs/plans/tile_roundtrip.md): one tile through the game-neutral model and back, byte for byte, with edits and the cross check against TedScene
run tile_roundtrip    ex wce_tile_io tile_roundtrip_check "$TILE"
# the exported tile format (Phase 2 of docs/plans/tile_roundtrip.md): one settlement tile read from its pack, through the model and back
run tile_binary_roundtrip ex wce_tile_io tile_binary_roundtrip_check "$GAME/data/tiles4.pack" --filter western_city_b/small
run weather_check     ex wce_weather weather_check "$BKIT/raw_data/warscape_asset_variation_db/environment_map.xml"
# the terrain colour (Phase 3): DXT decoding, the climates of the kit, the blend map baked with the game's textures
run terrain_check     ex wce_tile_editor terrain_check "$GAME"
run tdb_build         ex wce_formats tdb_build "$BKIT/raw_data/terrain/tiles/battle/_tile_database" - "$BKIT/working_data/terrain/tiles/battle/tile_database.bin"
if [ "$CORPUS" = 1 ]; then
    run roundtrip     ex wce_formats roundtrip "$MANIFEST" "$GAME"
    run scene_check   ex wce_tile scene_check "$GAME"   # every tile.ted of both kits
    run tile_roundtrip_all ex wce_tile_io tile_roundtrip_check "$GAME"   # every TEd project of the kit(s) through the model and back (about 2 min)
    run tile_binary_roundtrip_all ex wce_tile_io tile_binary_roundtrip_check "$GAME/data"   # every exported tile folder of the packs through the model and back (about 3 min)
    run tile_entry_check_all ex wce_tile tile_entry_check "$GAME" --all   # every kit tile opens by folder, tile.ted and entry as the same document (about 80 s)
    run placement_check ex wce_tile_editor placement_check "$BKIT"   # the tool's building placement against TEd's own building_list.xml export (Phase 0 oracle of docs/plans/tile_ted_parity.md)
    run tile_open_check env WCE_BIN="$WB" bash "$ROOT/scripts/tile_open_check.sh" "$OUT/tile_open_check"   # wce.exe opens one tile of every tile set and renders a frame
fi

# ---- render gates (fixed camera and inputs; see docs/baseline.md for what is deterministic; the pixel regression is scripts/shots.sh)
if [ "$RENDER" = 1 ]; then
    run app_check     env WCE_BIN="$WB" bash "$ROOT/scripts/app_check.sh" "$OUT/app_check"   # wce.exe scripted: tools, documents, selection, gizmo edits, undo, layouts, first run, capture; two runs identical
    run weather_tool_check env WCE_BIN="$WB" bash "$ROOT/scripts/weather_check.sh" "$OUT/weather_tool_check"   # the weather tool of wce.exe scripted: documents, structural edits, save (minimal edit), weather database, capture; two runs identical
    run tile_tool_check env WCE_BIN="$WB" bash "$ROOT/scripts/tile_check.sh" "$OUT/tile_tool_check"   # the tile tool of wce.exe scripted: edits, undo, mouse and keys, save (minimal edit, byte-identical round trip, .bak); two runs identical
    run tile_entry_script env WCE_BIN="$WB" bash "$ROOT/scripts/tile_entry_run.sh" "$OUT/tile_entry_script"   # the tile tool opens tiles by entry, name, folder and tile.ted; assert_entry; the tile browser's rows
    run cross_tool_check env WCE_BIN="$WB" bash "$ROOT/scripts/cross_tool_check.sh" "$OUT/cross_tool_check"   # wce.exe scripted: a tile and a weather file open at once, the weather previewed on the tile, one edit in each, the unsaved-changes prompts, Save All writes only the two edited spans; two runs identical
    run render_check  ex wce_render render_check   # scene, picking, terrain, overlays and capture through a headless D3D11 device
fi

echo "outputs in $OUT"
if [ ${#FAILED[@]} -gt 0 ]; then echo "FAILED: ${FAILED[*]}"; exit 1; fi
