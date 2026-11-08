#!/usr/bin/env bash
# Cross-tool composition of wce.exe, scripted (scripts/cross_tool.txt) twice in throw-away settings and projects folders: a vanilla kit tile
# (dense_forest_rugged/1x1/aa: 1x1 cells, five of the game's rock buildings, no rivers) lit by a weather file of the packs, then the same
# .environment open at once, one edit in each document, the unsaved-changes prompts, Save All, File > Export > For Assembly Kit, Revert to
# game's version. Checks that
#  - Save All saved into the project, never into the kit: the kit's copy of the tile is untouched until the export;
#  - only the two edited spans changed: the .environment (saved into the project's loose folder) differs from the pack file in the one
#    attribute value; the tile exported for the kit (tile.ted) differs from the original in the one moved building's position, every other
#    file of the tile folder is untouched, and tile.ted.bak is the original;
#  - the tile's weather lights the tile (the capture differs), follows an edit of the open file, ends with `tile_weather none` (the capture equals the
#    one without it), and lights the tile with the saved file once the document is closed (the capture differs from the default light), and
#    Revert to game's version deletes the project's copy;
#  - the two runs write the same bytes and the same captures.
#   scripts/cross_tool_check.sh [out dir]        Environment: WCE_BIN (default target/release, or $CARGO_TARGET_DIR/release), WCE_GAME,
#                                                WCE_KIT_TILES (the kit's terrain/tiles/battle; default: the game's battle tile map kit, else its kit)
set -u
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT="${1:-$ROOT/build/cross_tool_check}"; mkdir -p "$OUT"; OUT="$(cd "$OUT" && pwd)"
BIN="${WCE_BIN:-${CARGO_TARGET_DIR:-$ROOT/target}/release}"
SETTINGS_SRC="${WCE_SETTINGS_DIR:-$LOCALAPPDATA/wce/config}"
export WCE_GPU="${WCE_GPU:-AMD Radeon}"   # the reference pixels come from one GPU (docs/baseline.md)
GAME="${WCE_GAME:-D:/SteamLibrary/steamapps/common/Total War Attila}"
KIT="${WCE_KIT_TILES:-}"
if [ -z "$KIT" ]; then
    for k in "$GAME/assembly_kit_battle_tilemap" "$GAME/assembly_kit"; do
        [ -d "$k/raw_data/terrain/tiles/battle" ] && { KIT="$k/raw_data/terrain/tiles/battle"; break; }
    done
fi
TILE_REL=dense_forest_rugged/1x1/aa
ENTRY=dense_forest_rugged_1x1.xml
SRC="$KIT/$TILE_REL"
[ -f "$SRC/tile.ted" ] && [ -f "$KIT/_tile_database/TILES/$ENTRY" ] || { echo "no kit tile $TILE_REL under '$KIT' (set WCE_GAME or WCE_KIT_TILES)"; exit 1; }
ENV_PATH=weather/charlemagne/med_sp_land_day_dry.environment
# the pack file as the packs have it, written by the editor (game_file --packs), in throw-away settings and projects folders
pre="$OUT/pack"; rm -rf "$pre"; mkdir -p "$pre"; cp "$SETTINGS_SRC/settings.toml" "$pre/" 2>/dev/null
ENV_SRC="$OUT/pack_original.environment"; rm -f "$ENV_SRC"
printf 'game_file %s "%s" --packs\nquit\n' "$ENV_PATH" "$(cygpath -m "$ENV_SRC")" > "$OUT/pack.txt"
WCE_SETTINGS_DIR="$(cygpath -w "$pre")" WCE_PROJECTS="$(cygpath -w "$pre/projects")" "$BIN/wce.exe" --script "$(cygpath -w "$OUT/pack.txt")" > "$OUT/pack.log" 2>&1
[ -f "$ENV_SRC" ] || { echo "cannot read $ENV_PATH from the packs"; tail -5 "$OUT/pack.log"; exit 1; }
bad=0
fail() { echo "FAILED: $*"; bad=1; }

for run in 1 2; do
    dir="$OUT/run$run"; rm -rf "$dir"; mkdir -p "$dir/settings"; cp "$SETTINGS_SRC/settings.toml" "$dir/settings/" 2>/dev/null
    # a copy of the kit tile in a scratch kit (with its tile database entry, as the editor expects it): the script opens it and exports into it
    copy="$dir/kit/terrain/tiles/battle"
    mkdir -p "$copy/$(dirname "$TILE_REL")" "$copy/_tile_database/TILES"
    cp -r "$SRC" "$copy/$TILE_REL"
    cp "$KIT/_tile_database/TILES/$ENTRY" "$copy/_tile_database/TILES/"
    tile="$copy/$TILE_REL"
    sed -e "s|%TILE%|$(cygpath -m "$tile")|g" -e "s|%KIT%|$(cygpath -m "$dir/kit")|g" -e "s|%OUT%|$(cygpath -m "$dir")|g" "$ROOT/scripts/cross_tool.txt" > "$dir/script.txt"
    WCE_SETTINGS_DIR="$(cygpath -w "$dir/settings")" WCE_PROJECTS="$(cygpath -w "$dir/projects")" "$BIN/wce.exe" --script "$(cygpath -w "$dir/script.txt")" > "$dir/run.log" 2>&1
    rc=$?
    echo "run $run: exit $rc"
    [ $rc = 0 ] || { bad=1; grep -E "error|FAILED" "$dir/run.log" | head -5; }
done
D1="$OUT/run1"; D2="$OUT/run2"
T1="$D1/kit/terrain/tiles/battle/$TILE_REL"; T2="$D2/kit/terrain/tiles/battle/$TILE_REL"

# Save All went into the project (the kit's copy was untouched until the export, `kit_untouched` in the script's log)
grep -q "kit copy untouched by Save All" "$D1/run.log" && echo "Save All saved the tile into the project, not into the kit" || fail "Save All wrote into the kit"
ls "$D1/projects/assembly_kit/raw_data/terrain/tiles/battle/$TILE_REL/tile.wce" >/dev/null 2>&1 && echo "the project has the tile (tile.wce)" || fail "no tile.wce in the project"

# only the two edited spans changed
python "$ROOT/scripts/span_diff.py" "$ENV_SRC" "$D1/saved.environment" --expect 26 41 && echo "the saved .environment (read through the editor's files) differs from the pack file in exactly the edited attribute value" || fail "the saved .environment is not the pack file with one value changed"
python "$ROOT/scripts/span_diff.py" "$SRC/tile.ted" "$T1/tile.ted" && echo "the exported tile.ted differs from the original in exactly one span, inside one tag (the moved building)" || fail "the exported tile.ted is not the original with one span changed"
for f in $(cd "$SRC" && find . -type f | sed 's|^\./||'); do
    [ "$f" = tile.ted ] && continue
    cmp -s "$SRC/$f" "$T1/$f" || fail "tile file $f changed"
done
for f in $(cd "$T1" && find . -type f | sed 's|^\./||'); do
    [ -f "$SRC/$f" ] || [ "$f" = tile.ted.bak ] || fail "unexpected new tile file $f"
done
cmp -s "$SRC/tile.ted" "$T1/tile.ted.bak" && echo "tile.ted.bak is the original" || fail "tile.ted.bak is not the original"
[ ! -e "$D1/projects/loose/$ENV_PATH" ] && echo "Revert to game's version deleted the project's copy" || fail "the project's copy is still there after Revert to game's version"

# the preview
cmp -s "$D1/tile_default.png" "$D1/tile_previewed.png" && fail "the previewed weather did not change the picture" || echo "the previewed weather lights the tile (the capture differs)"
cmp -s "$D1/tile_previewed.png" "$D1/tile_edited_light.png" && fail "editing the previewed file did not change the picture" || echo "the preview follows the edits of the weather file"
cmp -s "$D1/tile_default.png" "$D1/tile_none.png" && echo "preview off: the tile is lit by the default light again, bytes equal to before" || fail "preview off differs from the default light"
cmp -s "$D1/tile_moved_default.png" "$D1/tile_after_close.png" && fail "after closing the document the tile lost its weather" || echo "closing the weather document keeps the tile's weather (the saved file)"

# both runs
for f in tile_default.png tile_previewed.png tile_edited_light.png tile_none.png tile_moved_default.png tile_after_close.png saved.environment; do
    cmp -s "$D1/$f" "$D2/$f" && [ -s "$D1/$f" ] && echo "$f identical between runs" || fail "$f differs between runs (or missing)"
done
cmp -s "$T1/tile.ted" "$T2/tile.ted" && echo "tile.ted identical between runs" || fail "tile.ted differs between runs"
exit $bad
