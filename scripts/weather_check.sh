#!/usr/bin/env bash
# The weather tool of wce.exe, scripted (scripts/weather_smoke.txt) twice in throw-away settings and projects folders. Checks that the saved file (in
# the project's loose folder) differs from the pack file in exactly the edited line, and that a capture of the edited scene is the same in both runs.
#   scripts/weather_check.sh [out dir]        Environment: WCE_BIN (default target/release, or $CARGO_TARGET_DIR/release)
set -u
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT="${1:-$ROOT/build/weather_check}"; mkdir -p "$OUT"; OUT="$(cd "$OUT" && pwd)"
BIN="${WCE_BIN:-${CARGO_TARGET_DIR:-$ROOT/target}/release}"
SETTINGS_SRC="${WCE_SETTINGS_DIR:-$LOCALAPPDATA/wce/config}"
ENV_PATH=weather/charlemagne/med_sp_land_day_dry.environment
CAND="${WCE_WEATHER_CANDIDATES:-2}"
# The reference pixels come from one GPU (docs/baseline.md): this machine has two and Windows may default to either.
export WCE_GPU="${WCE_GPU:-AMD Radeon}"
bad=0
# the pack file as the packs have it, written by the editor (game_file --packs), in throw-away settings and projects folders
pre="$OUT/pack"; rm -rf "$pre"; mkdir -p "$pre"; cp "$SETTINGS_SRC/settings.toml" "$pre/" 2>/dev/null
SRC="$OUT/pack_original.environment"; rm -f "$SRC"
printf 'game_file %s "%s" --packs\nquit\n' "$ENV_PATH" "$(cygpath -m "$SRC")" > "$OUT/pack.txt"
WCE_SETTINGS_DIR="$(cygpath -w "$pre")" WCE_PROJECTS="$(cygpath -w "$pre/projects")" "$BIN/wce.exe" --script "$(cygpath -w "$OUT/pack.txt")" > "$OUT/pack.log" 2>&1
[ -f "$SRC" ] || { echo "cannot read $ENV_PATH from the packs"; tail -5 "$OUT/pack.log"; exit 1; }
# the element count of the pack file, from the file itself (tags that open an element)
ELEMENTS=$(grep -o '<[A-Za-z_]' "$SRC" | wc -l)
for run in 1 2; do
    dir="$OUT/settings$run"; rm -rf "$dir"; mkdir -p "$dir"; cp "$SETTINGS_SRC/settings.toml" "$dir/" 2>/dev/null
    projects="$OUT/projects$run"; rm -rf "$projects"
    text="$(cat "$ROOT/scripts/weather_smoke.txt")"
    text="${text//%ELEMENTS_PLUS_1%/$((ELEMENTS + 1))}"; text="${text//%ELEMENTS%/$ELEMENTS}"; text="${text//%CANDIDATES%/$CAND}"
    printf '%s\n' "$text" > "$OUT/smoke$run.txt"
    WCE_SETTINGS_DIR="$(cygpath -w "$dir")" WCE_PROJECTS="$(cygpath -w "$projects")" "$BIN/wce.exe" --script "$(cygpath -w "$OUT/smoke$run.txt")" > "$OUT/run$run.log" 2>&1
    rc=$?
    echo "run $run: exit $rc"
    [ $rc = 0 ] || { bad=1; tail -5 "$OUT/run$run.log"; }
done
# the saved file: only the edited attribute differs from the pack file
S1="$OUT/projects1/loose"; S2="$OUT/projects2/loose"
if [ -f "$S1/$ENV_PATH" ]; then
    changed=$(diff <(tr -d '\r' < "$SRC") <(tr -d '\r' < "$S1/$ENV_PATH") | grep -c '^>')
    if [ "$changed" = 1 ] && grep -q "sun_direction_elevation='41'" "$S1/$ENV_PATH"; then echo "saved file differs from the pack file in exactly the edited line"; else echo "saved file differs in $changed lines (expected 1)"; bad=1; fi
    cmp -s "$S1/$ENV_PATH" "$S2/$ENV_PATH" && echo "both runs saved the same bytes" || { echo "runs saved different bytes"; bad=1; }
else echo "no saved file"; bad=1; fi
# the saved weather database: XML and FASTBIN convert into each other byte for byte (the old variation_e2e check)
conv="$BIN/examples/variation_convert.exe"
[ -x "$conv" ] || (cd "$ROOT" && cargo build --release -q -p wce_weather --example variation_convert)
if [ -f "$S1/warscape_asset_variation_db/warscape_asset_variation_db.bin" ] && [ -x "$conv" ]; then
    d="$S1/warscape_asset_variation_db"
    "$conv" "$(cygpath -w "$d/warscape_asset_variation_db.bin")" "$(cygpath -w "$OUT/from_bin.xml")" > /dev/null
    "$conv" "$(cygpath -w "$d/warscape_asset_variation_db.xml")" "$(cygpath -w "$OUT/from_xml.bin")" > /dev/null
    if cmp -s "$OUT/from_bin.xml" "$d/warscape_asset_variation_db.xml" && cmp -s "$OUT/from_xml.bin" "$d/warscape_asset_variation_db.bin" \
       && grep -q "ROME_W5" "$d/warscape_asset_variation_db.xml" && grep -q "weather.w5.test.environment' flags='4294967295'" "$d/warscape_asset_variation_db.xml"; then
        echo "database: saved XML and FASTBIN convert into each other byte for byte, the new value and variation are in"
    else echo "database check FAILED"; bad=1; fi
else echo "no saved database"; bad=1; fi
exit $bad
