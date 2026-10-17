#!/usr/bin/env bash
# The UI editor, end to end. Every check is about a rule, not about what a particular installation or mod happens to contain; the few
# facts of the game's own layouts that are used are named where they are.
#  - wce_ui_format: every UI file of the game's packs converts to XML and back to the same bytes;
#  - wce_ui_model: the tree prints back the same XML, entries and edits work on every layout; every field has a name and a control and an edit that
#    writes the value a field has changes nothing; docked components move with the screen by the share their dock point says;
#  - making, removing, reordering and reparenting components keep every layout whole and put each component where it was asked (structure_check);
#  - which files are read: release packs only plus chosen mods, loose folders first, the skin list (sources_check);
#  - wce-ui: render size, XML round trip, pack save read back, properties set by name, the MCP stdio server;
#  - wce.exe (scripts/ui_smoke.txt): the tool's edits, undo, previews, skins of your own, pictures and layouts that change on disk, export,
#    and the pack the export wrote read back.
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

echo "== layout (docked components follow the screen)"
cargo run --release -q -p wce_ui_model --example layout_check -- --game "$GAME" > "$OUT/layout.log" 2>&1 || fail "layout_check (see $OUT/layout.log)"
tail -2 "$OUT/layout.log"

echo "== structure (make, remove, reorder, reparent)"
cargo run --release -q -p wce_ui_model --example structure_check -- --game "$GAME" > "$OUT/structure.log" 2>&1 || fail "structure_check (see $OUT/structure.log)"
tail -1 "$OUT/structure.log"

echo "== properties (game names and controls for every field)"
cargo run --release -q -p wce_ui_model --example props_check -- --game "$GAME" > "$OUT/props.log" 2>&1 || fail "props_check (see $OUT/props.log)"
head -1 "$OUT/props.log"

echo "== which files are read"
cargo run --release -q -p wce_ui_model --example sources_check -- --game "$GAME" > "$OUT/sources.log" 2>&1 || fail "sources_check (see $OUT/sources.log)"
grep -E "SKIPPED|FAILED|all good" "$OUT/sources.log"

# a small PNG of one colour
png() { python -I - "$1" "$2" "$3" "$4" <<'EOP'
import struct, sys, zlib
path, r, g, b = sys.argv[1], int(sys.argv[2]), int(sys.argv[3]), int(sys.argv[4])
w = h = 16
raw = b"".join(b"\x00" + bytes([r, g, b, 255]) * w for _ in range(h))
def chunk(t, d):
    c = struct.pack(">I", len(d)) + t + d
    return c + struct.pack(">I", zlib.crc32(t + d))
open(path, "wb").write(b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", w, h, 8, 6, 0, 0, 0)) + chunk(b"IDAT", zlib.compress(raw)) + chunk(b"IEND", b""))
EOP
}
png_size() { python -I -c "import struct,sys; d=open(sys.argv[1],'rb').read(24); print('%dx%d' % struct.unpack('>II', d[16:24]))" "$1"; }
# a value of the JSON that follows the last line "> <tool>" of a script's output: json_after <log> <tool> <python expression on j>
json_after() { python -I - "$1" "$2" "$3" <<'EOP'
import json, sys
t = open(sys.argv[1], encoding="utf8").read()
i = t.rindex("\n> " + sys.argv[2])
j = json.JSONDecoder().raw_decode(t[t.index("{", i):])[0]
print(eval(sys.argv[3]))
EOP
}

echo "== wce-ui"
cargo build --release -q -p wce_ui_cli || fail "build wce-ui"
cargo build --release -q -p wce_ui_model --example pack_dump || fail "build pack_dump"
W="$TARGET/release/wce-ui.exe"
PD="$TARGET/release/examples/pack_dump.exe"
L="ui/campaign ui/settlement_captured"
mkdir -p "$OUT/cli"; C="$OUT/cli"

# the picture is as large as the layout's root, and as large as the screen asked for
root="$("$W" --game "$GAME" tree "$L" | sed -n '1s/.*@0,0 \([0-9]*x[0-9]*\).*/\1/p')"
"$W" --game "$GAME" render "$L" --out "$C/render.png" --outlines > /dev/null || fail "render"
[ "$(png_size "$C/render.png")" = "$root" ] || fail "render is $(png_size "$C/render.png"), the root is $root"
"$W" --game "$GAME" render "ui/frontend ui/battle_setup" --out "$C/screen.png" --screen 1920x1080 > /dev/null || fail "render at a screen size"
[ "$(png_size "$C/screen.png")" = "1920x1080" ] || fail "render at 1920x1080 is $(png_size "$C/screen.png")"

# XML out and back is stable
"$W" --game "$GAME" to-xml "$L" --out "$C/layout.xml" > /dev/null || fail "to-xml"
"$W" --game "$GAME" from-xml "$C/layout.xml" --out "$C/layout.bin" > /dev/null || fail "from-xml"
"$W" --game "$GAME" to-xml "$C/layout.bin" --out "$C/layout2.xml" > /dev/null || fail "to-xml of the rebuilt layout"
cmp -s "$C/layout.xml" "$C/layout2.xml" || fail "XML -> binary -> XML is not the same XML"

# a change saved into a pack comes back out of it
"$W" --game "$GAME" set_entry "$L" --entry 1 --x 77 --save --pack "$C/check.pack" > /dev/null || fail "pack save"
"$PD" list "$C/check.pack" | cut -f1 | grep -qx "$L" || fail "the pack does not hold $L"
"$PD" get "$C/check.pack" "$L" "$C/from_pack.bin" || fail "read the layout back from the pack"
[ "$("$W" --game "$GAME" entry "$C/from_pack.bin" --entry 1 | python -I -c "import json,sys; print(json.load(sys.stdin)['offset'][0])")" = "77" ] || fail "the layout in the pack does not have the saved offset"

# properties by name: set, then read back under the same name
cat > "$C/props_state.txt" <<'EOS'
open "ui/frontend ui/battle_setup"
set_property --entry 1 --name "Horizontal alignment" --value Centre --scope state
set_property --entry 1 --name "Interactive" --value no --scope state
state --entry 1
EOS
cat > "$C/props_entry.txt" <<'EOS'
open "ui/frontend ui/battle_setup"
set_property --entry 1 --name "Visible" --value no
set_property --entry 1 --name "Dock point" --value "Bottom centre"
entry --entry 1
EOS
"$W" --game "$GAME" script "$C/props_state.txt" > "$C/props_state.log" 2>&1 || fail "set_property on a state (see $C/props_state.log)"
"$W" --game "$GAME" script "$C/props_entry.txt" > "$C/props_entry.log" 2>&1 || fail "set_property on a component (see $C/props_entry.log)"
prop() { json_after "$1" "$2" "[p['value'] for p in j['properties'] if p['name'] == '$3'][0]"; }
[ "$(prop "$C/props_state.log" state "Horizontal alignment")" = "Centre" ] || fail "Horizontal alignment did not stick"
[ "$(prop "$C/props_state.log" state "Interactive")" = "no" ] || fail "Interactive did not stick"
[ "$(prop "$C/props_entry.log" entry "Visible")" = "no" ] || fail "Visible did not stick"
[ "$(prop "$C/props_entry.log" entry "Dock point")" = "Bottom centre" ] || fail "Dock point did not stick"
if "$W" --game "$GAME" set_property "$L" --entry 1 --name "No such property" --value 1 > /dev/null 2>&1; then fail "an unknown property name was accepted"; fi

printf '%s\n' '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}' '{"jsonrpc":"2.0","id":2,"method":"tools/list"}' '{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"ui_open","arguments":{"source":"'"$L"'"}}}' \
    | "$W" --game "$GAME" mcp > "$C/mcp.log"
grep -q '"name":"ui_set_property"' "$C/mcp.log" && grep -q 'settlement_captured' "$C/mcp.log" || fail "mcp"

echo "== wce.exe"
D="$OUT/app"; rm -rf "$D"; mkdir -p "$D/settings" "$D/fixtures" "$D/project/ui/skins/gate_skin"; cp "$SETTINGS_SRC/wce.ini" "$D/settings/" 2>/dev/null
# the settings of the machine decide which mod packs are chosen: none here, so that nothing but the game's release packs is read
sed -i '/^mod_packs=/d' "$D/settings/wce.ini" 2>/dev/null
png "$D/fixtures/red.png" 255 0 0
png "$D/fixtures/blue.png" 0 0 255
png "$D/project/ui/skins/gate_skin/gate.png" 0 255 0
"$W" --game "$GAME" set_entry "ui/campaign ui/messageicon" --entry 1 --x 222 --save --out "$D/fixtures/x222.bin" > /dev/null || fail "make the layout fixture"
sed -e "s|%OUT%|$(cygpath -m "$D")|g" "$ROOT/scripts/ui_smoke.txt" > "$D/script.txt"
CARGO_TARGET_DIR="$TARGET" cargo build --release -q -p wce_app || fail "build wce_app"
WCE_SETTINGS_DIR="$(cygpath -w "$D/settings")" timeout 300 "$TARGET/release/wce.exe" --script "$(cygpath -w "$D/script.txt")" > "$D/run.log" 2>&1
rc=$?; echo "wce.exe script: exit $rc"; [ $rc = 0 ] || { fail "script"; grep -E "error|FAILED" "$D/run.log" | head -5; }
cmp -s "$D/before.xml" "$D/undone.xml" || fail "undo did not give back the same XML"
cmp -s "$D/before.xml" "$D/dragged.xml" && fail "the drag did not change the layout"

# the pack the editor saved: the layout with the offset that was set
"$PD" get "$D/ui_smoke.pack" "$L" "$D/smoke_layout.bin" || fail "read the layout back from ui_smoke.pack"
[ "$("$W" --game "$GAME" entry "$D/smoke_layout.bin" --entry 1 | python -I -c "import json,sys; print(json.load(sys.stdin)['offset'][0])")" = "300" ] || fail "ui_smoke.pack does not have the offset that was set"

# the exported pack: exactly what is in the project folder, byte for byte, and the changed layout as it was in the editor
[ -d "$D/project/ui/skins/fresh_skin" ] || fail "the new skin has no folder"
[ -s "$D/project.pack" ] || fail "the project was not exported to a pack"
"$PD" list "$D/project.pack" | cut -f1 > "$D/exported.txt"
printf '%s\n' "ui/campaign ui/messageicon" "ui/skins/default/wce_gate_picture.png" "ui/skins/gate_skin/gate.png" | sort > "$D/expected.txt"
sort "$D/exported.txt" | cmp -s - "$D/expected.txt" || { fail "the exported pack holds the wrong files: $(tr '\n' ' ' < "$D/exported.txt")"; }
"$PD" get "$D/project.pack" "ui/skins/gate_skin/gate.png" "$D/out_gate.png" && cmp -s "$D/out_gate.png" "$D/project/ui/skins/gate_skin/gate.png" || fail "a picture in the pack is not the one in the folder"
"$PD" get "$D/project.pack" "ui/skins/default/wce_gate_picture.png" "$D/out_pic.png" && cmp -s "$D/out_pic.png" "$D/fixtures/blue.png" || fail "the changed picture in the pack is not the latest one"
"$PD" get "$D/project.pack" "ui/campaign ui/messageicon" "$D/out_layout.bin" || fail "read the exported layout"
[ "$("$W" --game "$GAME" entry "$D/out_layout.bin" --entry 1 | python -I -c "import json,sys; print(json.load(sys.stdin)['offset'][0])")" = "5" ] || fail "the exported layout is not the one that was in the editor"

[ $bad = 0 ] && echo "ui_check: all good" || { echo "ui_check: FAILED"; exit 1; }
