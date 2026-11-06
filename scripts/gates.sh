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
#   WCE_ROME2  Rome 2's folder: its gates run too (skipped when not set)
#   WCE_ROME2_DB  Rome 2's tile database in the kit's format (the Terrain tool's packs tree: <cache>/neighbours/rome_2_*/raw_data/terrain/tiles/battle/_tile_database)
#   WCE_BOB_KIT  a scratch Assembly Kit BOB_Terrain ran in: the Heights & Normals and Tilemap gates compare with its outputs (skipped when not set)
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

ex_rome2() { # ex_rome2 <crate> <example> [args...]: the same for Rome 2 (WCE_GAME_KEY=rome_2)
    WCE_GAME_KEY=rome_2 ex "$@"
}

# ---- unit tests (the C++ foundation_check, editor_check, unit_tests and height_check are crate tests)
run tests             cargo test --release -q --manifest-path "$ROOT/Cargo.toml" --workspace

# ---- weather gates
run xml_roundtrip     ex wce_formats xml_roundtrip "$CACHE"
run format_check      ex wce_formats format_check "$CACHE"
run variation_check   ex wce_weather variation_check
run rmv2_check        ex wce_formats rmv2_check "$CACHE"
run vfx_check_rain    ex wce_assets vfx_check "rain heavy"
run unit_formats      ex wce_assets unit_formats_check "$GAME"
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
# binary -> TEd project (Phase 2 step 3): the settlement tile decompiled, written, read back; exits 1 on a tile that does not decompile or read back
run tile_decompile    ex wce_tile_io tile_decompile_check "$GAME" --filter western_city_b/small
run weather_check     ex wce_weather weather_check "$BKIT/raw_data/warscape_asset_variation_db/environment_map.xml"
# the terrain colour (Phase 3): DXT decoding, the climates of the kit, the blend map baked with the game's textures
run terrain_check     ex wce_tile_editor terrain_check "$GAME"
run tech_check ex wce_tile_editor tech_check "$GAME"   # every building logic file reads into the technology overlays' shapes (the building technology chip)
run tdb_build         ex wce_formats tdb_build "$BKIT/raw_data/terrain/tiles/battle/_tile_database" - "$BKIT/working_data/terrain/tiles/battle/tile_database.bin"
# ---- map gates (docs/plans/terrain_editor.md): a kit map folder through the Terrain tool's model and back (unedited, made again, minimal
# edits), and the packs' built map files (tile_map, tile_list, locations.blm, CHMF, deployment XML) read and written back, byte for byte
MAPRAW="$GAME/assembly_kit/raw_data"; [ -d "$MAPRAW/terrain/battles" ] || MAPRAW="$BKIT/raw_data"
run map_roundtrip     ex wce_terrain_io map_roundtrip_check "$MAPRAW" --filter settlement_western_city_b_small
run map_built_roundtrip ex wce_terrain_io map_built_roundtrip_check --game "$GAME" --vanilla
# BOB_Terrain's Heights & Normals outputs made from a kit map's LF, against a BOB run's (a scratch kit: docs/re/bob.md; skipped without one)
BOBKIT="${WCE_BOB_KIT:-}"
if [ -n "$BOBKIT" ] && [ -d "$BOBKIT/working_data/terrain/battles" ]; then
    for m in $(ls "$BOBKIT/working_data/terrain/battles"); do
        [ -f "$BOBKIT/working_data/terrain/battles/$m/lf_height_map.data" ] && run "heights_build_$m" ex wce_terrain_io heights_build_check "$BOBKIT/raw_data/terrain/battles/$m" "$BOBKIT/working_data/terrain/battles/$m"
        # The fill (tile map and battle field) of the map as BOB filled it (an edited map's too: wce_edit, docs/plans/terrain_editor.md Phase 4)
        [ -f "$BOBKIT/working_data/terrain/battles/$m/tile_map.index" ] && run "tilemap_$m" ex wce_tile tile_map_check "$BOBKIT/raw_data/terrain" "$m" "$BOBKIT/working_data/terrain/battles/$m"
        # Tilemap's metadata (save_meta_data_map): the deployment files, locations.blm and its masks from the map's .terry and images
        [ -f "$BOBKIT/working_data/terrain/battles/$m/locations.blm" ] && run "built_$m" ex wce_terrain built_check "$BOBKIT/raw_data" "$m" "$BOBKIT/working_data/terrain/battles/$m"
        # The whole build as the Terrain tool exports it (the folder through the model, every built file), against every file BOB wrote
        [ -f "$BOBKIT/working_data/terrain/battles/$m/tile_map.index" ] && run "build_$m" ex wce_terrain_io build_check "$BOBKIT/raw_data" "$m" "$BOBKIT/working_data/terrain/battles/$m"
    done
fi
# The same of the kit's campaign maps BOB built (Terry file on terrain/campaigns/<map>: the markers, environment, LF and the fill)
if [ -n "$BOBKIT" ] && [ -d "$BOBKIT/working_data/terrain/campaigns" ]; then
    for m in $(ls "$BOBKIT/working_data/terrain/campaigns"); do
        [ -f "$BOBKIT/working_data/terrain/campaigns/$m/tile_map.index" ] && run "build_campaign_$m" ex wce_terrain_io build_check "$BOBKIT/raw_data" "campaigns/$m" "$BOBKIT/working_data/terrain/campaigns/$m"
    done
fi
# Terry's BCA generator (wce_terrain::bca) against a .terry Thrones' Terry generated (docs/re/terry.md, "The BCA generator"): WCE_BCA_ORACLE a
# folder with <map>_generated.terry and tile_map/ (the tile map Terry read: the game packs' tile_map.index and .tiles), WCE_BCA_KIT its kit,
# WCE_BCA_MAP the map (skipped without them)
if [ -n "${WCE_BCA_ORACLE:-}" ] && [ -n "${WCE_BCA_KIT:-}" ] && [ -n "${WCE_BCA_MAP:-}" ]; then
    run "bca_$WCE_BCA_MAP" env BCA_TILEMAP="$WCE_BCA_ORACLE/tile_map" bash -c "$(declare -f ex); ROOT='$ROOT'; ex wce_terrain bca_check '$WCE_BCA_KIT/raw_data/terrain' '$WCE_BCA_MAP' '$WCE_BCA_KIT/editor_working_data/Terry/bca_generation_spec.xml' '$WCE_BCA_ORACLE/${WCE_BCA_MAP}_generated.terry'"
fi
if [ "$CORPUS" = 1 ]; then
    run roundtrip    ex wce_formats roundtrip "$MANIFEST" "$GAME"
    run map_roundtrip_all ex wce_terrain_io map_roundtrip_check "$MAPRAW"   # every battle and campaign map folder of the kit (about 25 s)
    run scene_check   ex wce_tile scene_check "$GAME"   # every tile.ted of both kits
    run tile_roundtrip_all ex wce_tile_io tile_roundtrip_check "$GAME"   # every TEd project of the kit(s) through the model and back (about 2 min)
    run tile_binary_roundtrip_all ex wce_tile_io tile_binary_roundtrip_check "$GAME/data"   # every exported tile folder of the packs through the model and back (about 3 min)
    run tile_decompile_all ex wce_tile_io tile_decompile_check "$GAME"   # every exported tile folder of the packs decompiled and read back; the kit agreement is in the output, not gated (about 8 min)
    run tile_entry_check_all ex wce_tile tile_entry_check "$GAME" --all   # every kit tile opens by folder, tile.ted and entry as the same document (about 80 s)
    run placement_check ex wce_tile_editor placement_check "$BKIT"   # the tool's building placement against TEd's own building_list.xml export (Phase 0 oracle of docs/plans/tile_ted_parity.md)
    run tile_open_check env WCE_BIN="$WB" bash "$ROOT/scripts/tile_open_check.sh" "$OUT/tile_open_check"   # wce.exe opens one tile of every tile set and renders a frame
fi

# ---- Rome 2 gates (docs/formats/rome2_tiles.md): the XML tile databases written back byte for byte, a settlement tile's folders
# decompiled and compiled again (the multi-list capture file of the release pack's copy is a known loss), the editor's round trip
if [ -n "${WCE_ROME2:-}" ]; then
    run rome2_tile_database ex wce_tile_io rome2_tile_database_check "$WCE_ROME2"
    run rome2_unit_formats ex_rome2 wce_assets unit_formats_check "$WCE_ROME2"
    run rome2_tile_decompile ex_rome2 wce_tile_io tile_decompile_check "$WCE_ROME2" --filter roads_stone/2x2
    run rome2_tile_workflow ex_rome2 wce_tile_io tile_workflow_check "$WCE_ROME2" "$OUT/rome2_workflow" terrain/tiles/battle/settlement_roman_cities/roman_city_a/small
    # Its maps' built files (the only form they have) read and written back, and the CHMF an LF edit makes (Terrain tool, Phase 9)
    run rome2_map_built_roundtrip ex_rome2 wce_terrain_io map_built_roundtrip_check --game "$WCE_ROME2" --vanilla
    run rome2_built_lf ex_rome2 wce_terrain_io built_lf_check "$WCE_ROME2" --vanilla
    run rome2_built_scene ex_rome2 wce_terrain_io built_scene_check "$WCE_ROME2" --vanilla
    # The LF normal chunks an LF edit makes from the shipped ones: a campaign battle map, a settlement map, one with a layout kept
    for m in main_gaul settlement_greek_city_d alesia; do
        run "rome2_built_normals_$m" ex_rome2 wce_terrain_io built_normals_check "$WCE_ROME2" "terrain/battles/$m"
    done
    # The campaign battle maps' tile maps made again from their placements (the database: the Terrain tool's packs tree, WCE_ROME2_DB)
    [ -n "${WCE_ROME2_DB:-}" ] && run rome2_tile_map_rebuild ex_rome2 wce_tile tile_map_rebuild_check "$WCE_ROME2" "$WCE_ROME2_DB"
    if [ "$CORPUS" = 1 ]; then
        run rome2_tile_binary_roundtrip_all ex wce_tile_io tile_binary_roundtrip_check "$WCE_ROME2/data"
        run rome2_tile_decompile_all ex_rome2 wce_tile_io tile_decompile_check "$WCE_ROME2"
    fi
fi

# ---- render gates (fixed camera and inputs; see docs/baseline.md for what is deterministic; the pixel regression is scripts/shots.sh)
if [ "$RENDER" = 1 ]; then
    run app_check     env WCE_BIN="$WB" bash "$ROOT/scripts/app_check.sh" "$OUT/app_check"   # wce.exe scripted: tools, documents, selection, gizmo edits, undo, layouts, first run, capture; two runs identical
    run weather_tool_check env WCE_BIN="$WB" bash "$ROOT/scripts/weather_check.sh" "$OUT/weather_tool_check"   # the weather tool of wce.exe scripted: documents, structural edits, save (minimal edit), weather database, capture; two runs identical
    run tile_tool_check env WCE_BIN="$WB" bash "$ROOT/scripts/tile_check.sh" "$OUT/tile_tool_check"   # the tile tool of wce.exe scripted: edits, undo, mouse and keys, save (minimal edit, byte-identical round trip, .bak); two runs identical
    run tile_entry_script env WCE_BIN="$WB" bash "$ROOT/scripts/tile_entry_run.sh" "$OUT/tile_entry_script"   # the tile tool opens tiles by entry, name, folder and tile.ted; assert_entry; the tile browser's rows
    run cross_tool_check env WCE_BIN="$WB" bash "$ROOT/scripts/cross_tool_check.sh" "$OUT/cross_tool_check"   # wce.exe scripted: a tile lit by a weather file of the packs, then that file open at once and edited live, one edit in each, the unsaved-changes prompts, Save All writes only the two edited spans; two runs identical
    run render_check  ex wce_render render_check   # scene, picking, terrain, overlays and capture through a headless D3D11 device
fi

echo "outputs in $OUT"
if [ ${#FAILED[@]} -gt 0 ]; then echo "FAILED: ${FAILED[*]}"; exit 1; fi
