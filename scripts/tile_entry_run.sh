#!/usr/bin/env bash
# Runs scripts/tile_entry.txt (Phase 0b: open by entry / name / folder / tile.ted, assert_entry) in a throw-away settings folder.  scripts/tile_entry_run.sh <out dir>
set -u
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT="${1:-$ROOT/build/tile_entry_run}"; mkdir -p "$OUT"; OUT="$(cd "$OUT" && pwd)"
BIN="${WCE_BIN:-${CARGO_TARGET_DIR:-$ROOT/target}/release}"
export WCE_GPU="${WCE_GPU:-AMD Radeon}"
GAME="${WCE_GAME:-D:/SteamLibrary/steamapps/common/Total War Attila}"
KIT="$GAME/assembly_kit_battle_tilemap/raw_data/terrain/tiles/battle"
rm -rf "$OUT/run"; mkdir -p "$OUT/run/settings"; cp "${WCE_SETTINGS_DIR:-$LOCALAPPDATA/awe}/wce.ini" "$OUT/run/settings/" 2>/dev/null
sed -e "s|%KIT%|$KIT|g" -e "s|%OUT%|$(cygpath -m "$OUT/run")|g" "$ROOT/scripts/tile_entry.txt" > "$OUT/run/script.txt"
WCE_SETTINGS_DIR="$(cygpath -w "$OUT/run/settings")" "$BIN/wce.exe" --script "$(cygpath -w "$OUT/run/script.txt")" > "$OUT/run/run.log" 2>&1
rc=$?
echo "tile_entry script: exit $rc"
[ $rc = 0 ] || grep -E "error|FAILED" "$OUT/run/run.log" | head -5
exit $rc
