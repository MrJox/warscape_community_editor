#!/usr/bin/env bash
# Runs the Phase 0 repro scripts (scripts/repro/*.txt) of the Tile tool parity plan on a kit tile, unattended (non-activating window, own settings folder),
# and leaves the screenshots and logs in <out dir>/<script>/.   scripts/tile_repro.sh [out dir] [script names...]
# Environment: WCE_BIN (default target/release, or $CARGO_TARGET_DIR/release), WCE_GAME, WCE_TILE (default: the kit's assembly_kit/osgiliath_test/battlefield).
set -u
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
. "$ROOT/scripts/lib.sh"
OUT="${1:-$ROOT/build/repro}"; shift || true; mkdir -p "$OUT"; OUT="$(cd "$OUT" && pwd)"
BIN="${WCE_BIN:-${CARGO_TARGET_DIR:-$ROOT/target}/release}"
TILE="${WCE_TILE:-$GAME/assembly_kit_battle_tilemap/raw_data/terrain/tiles/battle/assembly_kit/osgiliath_test/battlefield}"
NAMES=("$@"); [ ${#NAMES[@]} = 0 ] && NAMES=(layout layers gizmo terrain_water)
for name in "${NAMES[@]}"; do
    dir="$OUT/$name"; rm -rf "$dir"; mkdir -p "$dir/settings"; cp "$SETTINGS_SRC/settings.toml" "$dir/settings/" 2>/dev/null
    sed -e "s|%TILE%|$(mixpath "$TILE")|g" -e "s|%OUT%|$(mixpath "$dir")|g" "$ROOT/scripts/repro/$name.txt" > "$dir/script.txt"
    WCE_SETTINGS_DIR="$(winpath "$dir/settings")" "$BIN/wce$EXE" --script "$(winpath "$dir/script.txt")" > "$dir/run.log" 2>&1
    echo "$name: exit $?"
done
