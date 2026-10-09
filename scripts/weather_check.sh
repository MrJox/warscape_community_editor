#!/usr/bin/env bash
# The weather tool of wce.exe, scripted (scripts/weather_smoke.txt) twice in throw-away settings and output folders. Checks that the saved file differs
# from the pack file in exactly the edited line, and that a capture of the edited scene is the same in both runs.
#   scripts/weather_check.sh [out dir]        Environment: WCE_BIN (default build/dev/bin/$WCE_CONFIG; RelWithDebInfo)
set -u
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT="${1:-$ROOT/build/weather_check}"; mkdir -p "$OUT"; OUT="$(cd "$OUT" && pwd)"
BIN="${WCE_BIN:-$ROOT/build/dev/bin/${WCE_CONFIG:-RelWithDebInfo}}"
SETTINGS_SRC="${WCE_SETTINGS_DIR:-$LOCALAPPDATA/awe}"
ENV_PATH=weather/charlemagne/med_sp_land_day_dry.environment
CAND="${WCE_WEATHER_CANDIDATES:-2}"
# The reference pixels come from one GPU (docs/baseline.md): this machine has two and Windows may default to either.
export WCE_GPU="${WCE_GPU:-AMD Radeon}"
bad=0
# the element count of the pack file, from the file itself (tags that open an element)
SRC="$(ls "$SETTINGS_SRC"/x/*/$ENV_PATH 2>/dev/null | head -1)"
[ -n "$SRC" ] || { WCE_SETTINGS_DIR="$(cygpath -w "$SETTINGS_SRC")" "$BIN/wce.exe" --open $ENV_PATH --cmd quit >/dev/null 2>&1; SRC="$(ls "$SETTINGS_SRC"/x/*/$ENV_PATH | head -1)"; }
ELEMENTS=$(grep -o '<[A-Za-z_]' "$SRC" | wc -l)
for run in 1 2; do
    dir="$OUT/settings$run"; rm -rf "$dir"; mkdir -p "$dir"; cp "$SETTINGS_SRC/wce.ini" "$dir/" 2>/dev/null
    save="$OUT/saved$run"; rm -rf "$save"
    text="$(cat "$ROOT/scripts/weather_smoke.txt")"
    text="${text//%OUT%/$(cygpath -w "$save")}"; text="${text//%ELEMENTS_PLUS_1%/$((ELEMENTS + 1))}"; text="${text//%ELEMENTS%/$ELEMENTS}"; text="${text//%CANDIDATES%/$CAND}"
    printf '%s\n' "$text" > "$OUT/smoke$run.txt"
    WCE_SETTINGS_DIR="$(cygpath -w "$dir")" "$BIN/wce.exe" --script "$(cygpath -w "$OUT/smoke$run.txt")" > "$OUT/run$run.log" 2>&1
    rc=$?
    echo "run $run: exit $rc"
    [ $rc = 0 ] || { bad=1; tail -5 "$OUT/run$run.log"; }
done
# the saved file: only the edited attribute differs from the pack file
if [ -f "$OUT/saved1/$ENV_PATH" ]; then
    changed=$(diff <(tr -d '\r' < "$SRC") <(tr -d '\r' < "$OUT/saved1/$ENV_PATH") | grep -c '^>')
    if [ "$changed" = 1 ] && grep -q "sun_direction_elevation='41'" "$OUT/saved1/$ENV_PATH"; then echo "saved file differs from the pack file in exactly the edited line"; else echo "saved file differs in $changed lines (expected 1)"; bad=1; fi
    cmp -s "$OUT/saved1/$ENV_PATH" "$OUT/saved2/$ENV_PATH" && echo "both runs saved the same bytes" || { echo "runs saved different bytes"; bad=1; }
else echo "no saved file"; bad=1; fi
# the saved weather database: XML and FASTBIN convert into each other byte for byte (the old variation_e2e check)
conv="$BIN/variation_convert.exe"
if [ -f "$OUT/saved1/warscape_asset_variation_db/warscape_asset_variation_db.bin" ] && [ -x "$conv" ]; then
    d="$OUT/saved1/warscape_asset_variation_db"
    "$conv" "$(cygpath -w "$d/warscape_asset_variation_db.bin")" "$(cygpath -w "$OUT/from_bin.xml")" > /dev/null
    "$conv" "$(cygpath -w "$d/warscape_asset_variation_db.xml")" "$(cygpath -w "$OUT/from_xml.bin")" > /dev/null
    if cmp -s "$OUT/from_bin.xml" "$d/warscape_asset_variation_db.xml" && cmp -s "$OUT/from_xml.bin" "$d/warscape_asset_variation_db.bin" \
       && grep -q "ROME_W5" "$d/warscape_asset_variation_db.xml" && grep -q "weather.w5.test.environment' flags='4294967295'" "$d/warscape_asset_variation_db.xml"; then
        echo "database: saved XML and FASTBIN convert into each other byte for byte, the new value and variation are in"
    else echo "database check FAILED"; bad=1; fi
else echo "no saved database"; bad=1; fi
exit $bad
