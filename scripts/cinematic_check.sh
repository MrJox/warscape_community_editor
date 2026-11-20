#!/usr/bin/env bash
# The Cinematic tool's gate (render): scripts/cinematic_smoke.txt twice, each in a throw-away settings folder (a copy of
# settings.toml only); both runs must pass and give identical shots.
#
#   scripts/cinematic_check.sh [out dir]
#
# Run it with no other wce.exe open: an editor in use at the same time made the movie and the preview shot differ (2026-11-20).
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
    # The 3D preview and a movie export, in a projects folder of their own.
    proj="$OUT/projects_preview$run"; rm -rf "$proj"; mkdir -p "$proj"
    sed -e "s|%SHOT4%|$(cygpath -m "$OUT/preview$run.png")|" -e "s|%MOVIE%|$(cygpath -m "$OUT/movie$run.pack")|"         "$ROOT/scripts/cinematic_preview.txt" > "$OUT/preview$run.txt"
    WCE_PROJECTS="$(cygpath -w "$proj")" WCE_SETTINGS_DIR="$(cygpath -w "$dir")" "$BIN/wce.exe" --script "$(cygpath -w "$OUT/preview$run.txt")" > "$OUT/preview_run$run.log" 2>&1
    rc=$?
    echo "preview run $run: exit $rc"
    [ $rc = 0 ] || { bad=1; grep -i -E "fail|error" "$OUT/preview_run$run.log" | tail -5; }
    [ -s "$OUT/movie$run.pack" ] || { echo "preview run $run: no movie pack"; bad=1; }
    # The battle logic graph.
    proj="$OUT/projects_logic$run"; rm -rf "$proj"; mkdir -p "$proj"
    sed -e "s|%SHOT5%|$(cygpath -m "$OUT/logic$run.png")|" "$ROOT/scripts/cinematic_logic.txt" > "$OUT/logic$run.txt"
    WCE_PROJECTS="$(cygpath -w "$proj")" WCE_SETTINGS_DIR="$(cygpath -w "$dir")" "$BIN/wce.exe" --script "$(cygpath -w "$OUT/logic$run.txt")" > "$OUT/logic_run$run.log" 2>&1
    rc=$?
    echo "logic run $run: exit $rc"
    [ $rc = 0 ] || { bad=1; grep -i -E "fail|error" "$OUT/logic_run$run.log" | tail -5; }
    # Movie editing: a tone and a PNG sequence made here first.
    media="$OUT/media"; mkdir -p "$media/pngs" "$OUT/r$run"
    python -c "
import sys, wave, math, struct
from PIL import Image
d = sys.argv[1]
w = wave.open(d + '/tone.wav', 'wb'); w.setnchannels(2); w.setsampwidth(2); w.setframerate(48000)
w.writeframes(b''.join(struct.pack('<hh', v, v) for v in (int(9000 * math.sin(i * 2 * math.pi * 330 / 48000)) for i in range(48000 * 2))))
w.close()
for f in range(10):
    Image.new('RGB', (64, 48), (f * 20, 80, 160)).save(d + '/pngs/frame_%04d.png' % f)
" "$media"
    sed -e "s|%SUBS%|$(cygpath -m "$OUT/subs$run.pack")|" -e "s|%WAV%|$(cygpath -m "$media/tone.wav")|" -e "s|%SOUND%|$(cygpath -m "$OUT/sound$run.pack")|"         -e "s|%PNGS%|$(cygpath -m "$media/pngs")|" -e "s|%IMPORT%|$(cygpath -m "$OUT/r$run/wce_imported.pack")|"         "$ROOT/scripts/cinematic_movie_edit.txt" > "$OUT/edit$run.txt"
    WCE_SETTINGS_DIR="$(cygpath -w "$dir")" "$BIN/wce.exe" --script "$(cygpath -w "$OUT/edit$run.txt")" > "$OUT/edit_run$run.log" 2>&1
    rc=$?
    echo "movie edit run $run: exit $rc"
    [ $rc = 0 ] || { bad=1; grep -i -E "fail|error" "$OUT/edit_run$run.log" | tail -5; }
    for f in subs$run sound$run r$run/wce_imported; do [ -s "$OUT/$f.pack" ] || { echo "movie edit run $run: no $f pack"; bad=1; }; done
done
for f in subs sound; do
    if cmp -s "$OUT/${f}1.pack" "$OUT/${f}2.pack"; then echo "$f pack identical between runs"; else echo "$f pack DIFFERS between runs"; bad=1; fi
done
if cmp -s "$OUT/r1/wce_imported.pack" "$OUT/r2/wce_imported.pack"; then echo "import pack identical between runs"; else echo "import pack DIFFERS between runs"; bad=1; fi
if cmp -s "$OUT/movie1.pack" "$OUT/movie2.pack"; then echo "movie pack identical between runs"; else echo "movie pack DIFFERS between runs"; bad=1; fi
if cmp -s "$OUT/projects1/cinematics/wce_scene_test/project.json" "$OUT/projects2/cinematics/wce_scene_test/project.json"; then echo "project.json identical between runs"
else echo "project.json DIFFERS between runs"; bad=1; fi
if cmp -s "$OUT/scene1.pack" "$OUT/scene2.pack"; then echo "battle pack identical between runs"; else echo "battle pack DIFFERS between runs"; bad=1; fi
for shot in intro event scene logic; do
    if cmp -s "$OUT/${shot}1.png" "$OUT/${shot}2.png" && [ -s "$OUT/${shot}1.png" ]; then echo "$shot shot identical between runs"
    else echo "$shot shot DIFFERS between runs (or missing)"; bad=1; fi
done
# The live 3D viewport: within 2 levels, as app_check.sh allows its live shots (exposure noise); the movie above is exact.
if cmp -s "$OUT/preview1.png" "$OUT/preview2.png" && [ -s "$OUT/preview1.png" ]; then echo "preview shot identical between runs"
elif [ -s "$OUT/preview1.png" ] && [ -s "$OUT/preview2.png" ] && python -c "
import sys
import numpy as np
from PIL import Image
a = np.asarray(Image.open(sys.argv[1]).convert('RGB'), dtype=int); b = np.asarray(Image.open(sys.argv[2]).convert('RGB'), dtype=int)
sys.exit(0 if a.shape == b.shape and np.abs(a - b).max() <= 2 else 1)" "$OUT/preview1.png" "$OUT/preview2.png"; then echo "preview shot within 2 levels between runs (live exposure noise)"
else echo "preview shot DIFFERS between runs (or missing)"; bad=1; fi
[ $bad = 0 ] && echo OK || { echo FAILED; exit 1; }
