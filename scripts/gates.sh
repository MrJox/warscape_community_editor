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

# ---- weather_editor gates
run xml_roundtrip     "$WB/xml_roundtrip.exe" "$CACHE"
run format_check      "$WB/format_check.exe" "$CACHE"
run variation_check   "$WB/variation_check.exe"
run rmv2_check        "$WB/rmv2_check.exe"
run vfx_check_rain    "$WB/vfx_check.exe" "rain heavy"
[ -f "$CBS" ] && run env_vs_dump_all "$WB/env_vs_dump.exe" --all "$CBS"

# ---- tile_editor gates
# monte_rosa's tile.ted has no SCENE_GRAPH (it crashed unit_tests before the Phase 1 fix, docs/baseline.md): the second file checks that
run unit_tests        "$TB/unit_tests.exe" "$BKIT/raw_data/terrain/tiles/battle/assembly_kit/khazad_wall_test/battlefield/tile.ted" "$TILE/tile.ted"
run height_check      "$TB/height_check.exe" "$TILE/height_map_0.png"
run weather_check     "$TB/weather_check.exe" "$BKIT/raw_data/warscape_asset_variation_db/environment_map.xml"
run tdb_build         "$TB/tdb_build.exe" "$BKIT/raw_data/terrain/tiles/battle/_tile_database" - "$BKIT/working_data/terrain/tiles/battle/tile_database.bin"
if [ "$CORPUS" = 1 ]; then
    run roundtrip     "$TB/roundtrip.exe" "$MANIFEST" "$GAME"
    run scene_check   "$TB/scene_check.exe" "$MANIFEST" "$GAME"
fi

# ---- reference screenshots (fixed camera and inputs; see docs/baseline.md for what is deterministic)
if [ "$RENDER" = 1 ]; then
    for env in dry rain snow; do
        case $env in
            dry)  f="weather/charlemagne/med_sp_land_day_dry.environment" ;;
            rain) f="weather/charlemagne/med_sp_land_day_rain.environment" ;;
            snow) f="weather/charlemagne/med_sp_land_day_snow.environment" ;;
        esac
        run "shot_weather_$env" "$WB/weather_editor.exe" --open "$f" --cam 0.6 0.35 120 --frames 20 --shot "$(cygpath -w "$OUT/shot_weather_$env.png")"
    done
    run shot_tile         "$TB/tile_studio.exe" --tile "$(cygpath -w "$TILE")" --cam 0.6 0.6 400 --frames 10 --shot "$(cygpath -w "$OUT/shot_tile.png")"
fi

echo "outputs in $OUT"
if [ ${#FAILED[@]} -gt 0 ]; then echo "FAILED: ${FAILED[*]}"; exit 1; fi
