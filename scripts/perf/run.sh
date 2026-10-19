#!/usr/bin/env bash
# scripts/perf/run.sh <nav|edit|load> [tile folder]   Environment: WCE_BIN (default target/release, or $CARGO_TARGET_DIR/release), WCE_GAME
# Runs a performance scenario with the live clock and prints the frame statistics.
set -u
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
BIN="${WCE_BIN:-${CARGO_TARGET_DIR:-$ROOT/target}/release}"
GAME="${WCE_GAME:-D:/SteamLibrary/steamapps/common/Total War Attila}"
TILE="${2:-$GAME/assembly_kit_battle_tilemap/raw_data/terrain/tiles/battle/tdd_settlement_dunedain_cities/dunedain_city_osgiliath/minor}"
OUT="$ROOT/build/perf"; mkdir -p "$OUT"
sed -e "s|%TILE%|$TILE|g" "$ROOT/scripts/perf/$1.txt" > "$OUT/$1.script.txt"
"$BIN/wce.exe" --live --script "$(cygpath -w "$OUT/$1.script.txt")" 2>&1 | grep -E "^\[info\] (perf|wait_loaded|tile loaded)|draw set|^  (frames|GPU)"
