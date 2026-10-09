#!/usr/bin/env bash
# Opens one tile of every tile set of the assembly kits (the first by name; the battle and the campaign kit) in wce.exe, renders a frame offscreen and checks that it neither crashes nor logs an error.
# Several minutes: part of the corpus gates.
#   scripts/tile_open_check.sh [out dir]        Environment: WCE_BIN (default build/dev/bin/$WCE_CONFIG; RelWithDebInfo), WCE_GAME
set -u
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT="${1:-$ROOT/build/tile_open_check}"; mkdir -p "$OUT"; OUT="$(cd "$OUT" && pwd)"
BIN="${WCE_BIN:-$ROOT/build/dev/bin/${WCE_CONFIG:-RelWithDebInfo}}"
export WCE_GPU="${WCE_GPU:-AMD Radeon}"
GAME="${WCE_GAME:-D:/SteamLibrary/steamapps/common/Total War Attila}"
ROOTS=("$GAME/assembly_kit_battle_tilemap/raw_data/terrain/tiles" "$GAME/assembly_kit_campaign_tilemap/raw_data/terrain/tiles")
bad=0; n=0; instances=0
# one tile per tile set: <kit>/.../tiles/<battle|campaign>/<set>/<...>/tile.ted, the first in sorted order
declare -A seen
while IFS= read -r ted; do
    dir="${ted%/tile.ted}"
    set_key="$(echo "${dir#$GAME/}" | sed "s|/raw_data/terrain/tiles||" | cut -d/ -f1-3)"
    [ -n "${seen[$set_key]:-}" ] && continue
    seen[$set_key]=1
    n=$((n + 1))
    log="$OUT/open_$n.log"
    timeout 300 "$BIN/wce.exe" --tile "$dir" --frames 3 --capture 320x180 "$(cygpath -w "$OUT/open.png")" > "$log" 2>&1
    rc=$?
    errors=$(grep -c "\[error\]" "$log")
    if [ $rc != 0 ] || [ "$errors" != 0 ]; then echo "FAILED ($rc, $errors errors): ${dir#$GAME/}"; grep "\[error\]" "$log" | head -3; bad=1; fi
    c=$(sed -n 's/.*tile loaded: \([0-9]*\) instances.*/\1/p' "$log" | head -1); instances=$((instances + ${c:-0}))
done < <(find "${ROOTS[@]}" -name tile.ted 2>/dev/null | sort)
echo "opened $n tiles (one per tile set), $instances instances, failures: $bad"
exit $bad
