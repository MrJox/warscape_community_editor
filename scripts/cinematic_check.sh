#!/usr/bin/env bash
# The Cinematic tool's gate (render): scripts/cinematic_smoke.txt twice, each in a throw-away settings folder (a copy of
# settings.toml only); both runs must pass and give identical shots.
#
#   scripts/cinematic_check.sh [out dir]
set -u
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT="${1:-$ROOT/build/cinematic_check}"
BIN="${WCE_BIN:-${CARGO_TARGET_DIR:-$ROOT/target}/release}"
SETTINGS_SRC="${WCE_SETTINGS_DIR:-$LOCALAPPDATA/wce/config}"
mkdir -p "$OUT"
bad=0
for run in 1 2; do
    dir="$OUT/settings$run"
    rm -rf "$dir"; mkdir -p "$dir"
    [ -f "$SETTINGS_SRC/settings.toml" ] && cp "$SETTINGS_SRC/settings.toml" "$dir/"
    sed -e "s|%SHOT1%|$(cygpath -m "$OUT/intro$run.png")|" -e "s|%SHOT2%|$(cygpath -m "$OUT/event$run.png")|" \
        "$ROOT/scripts/cinematic_smoke.txt" > "$OUT/smoke$run.txt"
    WCE_SETTINGS_DIR="$(cygpath -w "$dir")" "$BIN/wce.exe" --script "$(cygpath -w "$OUT/smoke$run.txt")" > "$OUT/run$run.log" 2>&1
    rc=$?
    echo "run $run: exit $rc"
    [ $rc = 0 ] || { bad=1; grep -i -E "fail|error" "$OUT/run$run.log" | tail -5; }
    # The scene editor, with its own projects folder.
    proj="$OUT/projects$run"; rm -rf "$proj"; mkdir -p "$proj"
    sed -e "s|%SHOT3%|$(cygpath -m "$OUT/scene$run.png")|" -e "s|%PACK%|$(cygpath -m "$OUT/scene$run.pack")|"         "$ROOT/scripts/cinematic_scene.txt" > "$OUT/scene$run.txt"
    WCE_PROJECTS="$(cygpath -w "$proj")" WCE_SETTINGS_DIR="$(cygpath -w "$dir")" "$BIN/wce.exe" --script "$(cygpath -w "$OUT/scene$run.txt")" > "$OUT/scene_run$run.log" 2>&1
    rc=$?
    echo "scene run $run: exit $rc"
    [ $rc = 0 ] || { bad=1; grep -i -E "fail|error" "$OUT/scene_run$run.log" | tail -5; }
    [ -s "$proj/cinematics/wce_scene_test/project.json" ] || { echo "scene run $run: no project.json saved"; bad=1; }
    [ -s "$OUT/scene$run.pack" ] || { echo "scene run $run: no battle pack"; bad=1; }
done
if cmp -s "$OUT/projects1/cinematics/wce_scene_test/project.json" "$OUT/projects2/cinematics/wce_scene_test/project.json"; then echo "project.json identical between runs"
else echo "project.json DIFFERS between runs"; bad=1; fi
if cmp -s "$OUT/scene1.pack" "$OUT/scene2.pack"; then echo "battle pack identical between runs"; else echo "battle pack DIFFERS between runs"; bad=1; fi
for shot in intro event scene; do
    if cmp -s "$OUT/${shot}1.png" "$OUT/${shot}2.png" && [ -s "$OUT/${shot}1.png" ]; then echo "$shot shot identical between runs"
    else echo "$shot shot DIFFERS between runs (or missing)"; bad=1; fi
done
[ $bad = 0 ] && echo OK || { echo FAILED; exit 1; }
