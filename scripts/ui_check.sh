#!/usr/bin/env bash
# The UI editor, end to end:
#  - wce_ui_format: every UI file of the game's packs converts to XML and back to the same bytes;
#  - wce_ui_model: the tree prints back the same XML, the entries and edit operations work on every layout;
#  - wce-ui: tree, render, to-xml / from-xml, pack save, and the MCP stdio server answer;
#  - wce.exe (scripts/ui_smoke.txt): the tool opens a layout, the mouse drag is one undo entry and undo gives back the same XML,
#    edits and structural operations, a pack save.
#   scripts/ui_check.sh [out dir]      Environment: WCE_GAME, CARGO_TARGET_DIR (a SHORT path on Windows: MSBuild's 260 character limit breaks the SDL3 build
#                                      in a deep folder), WCE_SETTINGS_DIR (a folder with wce.ini; default %LOCALAPPDATA%/awe)
set -u
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT="${1:-$ROOT/build/ui_check}"; mkdir -p "$OUT"; OUT="$(cd "$OUT" && pwd)"
GAME="${WCE_GAME:-D:/SteamLibrary/steamapps/common/Total War Attila}"
TARGET="${CARGO_TARGET_DIR:-C:/wt}"
SETTINGS_SRC="${WCE_SETTINGS_DIR:-$LOCALAPPDATA/awe}"
export WCE_GPU="${WCE_GPU:-AMD Radeon}"
bad=0
fail() { echo "FAILED: $*"; bad=1; }
cd "$ROOT"

echo "== format round trip"
cargo run --release -q -p wce_ui_format --example ui_roundtrip -- --game "$GAME" > "$OUT/roundtrip.log" || fail "format round trip (see $OUT/roundtrip.log)"
tail -1 "$OUT/roundtrip.log"

echo "== model"
cargo run --release -q -p wce_ui_model --example model_check -- --game "$GAME" > "$OUT/model.log" 2>&1 || fail "model_check (see $OUT/model.log)"
tail -1 "$OUT/model.log"

echo "== layout (docking against the stored offsets)"
cargo run --release -q -p wce_ui_model --example layout_check -- --game "$GAME" > "$OUT/layout.log" 2>&1 || fail "layout_check (see $OUT/layout.log)"
tail -3 "$OUT/layout.log"

echo "== wce-ui"
cargo build --release -q -p wce_ui_cli || fail "build wce-ui"
W="$TARGET/release/wce-ui.exe"
L="ui/campaign ui/settlement_captured"
"$W" --game "$GAME" tree "$L" | head -3
"$W" --game "$GAME" render "$L" --out "$OUT/render.png" --outlines > /dev/null || fail "render"
[ -s "$OUT/render.png" ] || fail "render wrote nothing"
"$W" --game "$GAME" to-xml "$L" --out "$OUT/layout.xml" > /dev/null || fail "to-xml"
"$W" --game "$GAME" from-xml "$OUT/layout.xml" --out "$OUT/layout.bin" > /dev/null || fail "from-xml"
"$W" --game "$GAME" set_entry "$L" --entry 1 --x 77 --save --pack "$OUT/check.pack" > /dev/null || fail "pack save"
[ -s "$OUT/check.pack" ] || fail "pack missing"
"$W" --game "$GAME" render "ui/frontend ui/battle_setup" --out "$OUT/screen1080.png" --screen 1920x1080 --skin rohan > /dev/null || fail "render at a screen size"
"$W" --game "$GAME" tree "ui/frontend ui/battle_setup" --max_depth 1 --screen 1920x1080 | grep -q "screen 480,90" || fail "docking at 1920x1080"
"$W" --game "$GAME" related "ui/frontend ui/battle_setup" | grep -q "land_unit_card" || fail "related layouts"
"$W" --game "$GAME" render "ui/frontend ui/battle_setup" --out "$OUT/related.png" --related --skin rohan > /dev/null || fail "render with related layouts"
"$W" --game "$GAME" animations "ui/campaign ui/popup_game_saved" | grep -q "OnCreate" || fail "animations"
for t in 0 300; do "$W" --game "$GAME" render "ui/campaign ui/popup_game_saved" --animation OnCreate --animation_entry 1 --time $t --out "$OUT/anim_$t.png" > /dev/null || fail "render an animation"; done
cmp -s "$OUT/anim_0.png" "$OUT/anim_300.png" && fail "the animation did not change the picture"
printf '%s\n' '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}' '{"jsonrpc":"2.0","id":2,"method":"tools/list"}' '{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"ui_open","arguments":{"source":"'"$L"'"}}}' \
    | "$W" --game "$GAME" mcp > "$OUT/mcp.log"
grep -q '"name":"ui_render"' "$OUT/mcp.log" && grep -q 'settlement_captured' "$OUT/mcp.log" || fail "mcp"

echo "== wce.exe"
D="$OUT/app"; rm -rf "$D"; mkdir -p "$D/settings"; cp "$SETTINGS_SRC/wce.ini" "$D/settings/" 2>/dev/null
sed -e "s|%OUT%|$(cygpath -m "$D")|g" "$ROOT/scripts/ui_smoke.txt" > "$D/script.txt"
CARGO_TARGET_DIR="$TARGET" cargo build --release -q -p wce_app || fail "build wce_app"
WCE_SETTINGS_DIR="$(cygpath -w "$D/settings")" timeout 300 "$TARGET/release/wce.exe" --script "$(cygpath -w "$D/script.txt")" > "$D/run.log" 2>&1
rc=$?; echo "wce.exe script: exit $rc"; [ $rc = 0 ] || { fail "script"; grep -E "error|FAILED" "$D/run.log" | head -5; }
cmp -s "$D/before.xml" "$D/undone.xml" || fail "undo did not give back the same XML"
cmp -s "$D/before.xml" "$D/dragged.xml" && fail "the drag did not change the layout"
[ -s "$D/ui_smoke.png" ] || fail "no screenshot"

[ $bad = 0 ] && echo "ui_check: all good" || { echo "ui_check: FAILED"; exit 1; }
