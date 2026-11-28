#!/bin/bash
# The editor's river shader as a movie pack (scripts/river_shader.py; docs/re/water.md, "The editor's river shader"):
#   scripts/river_shader.sh <Attila's folder> <out.pack>
# Builds vfs_ls and pack_write (release), takes the game's own battle river pixel shaders (vanilla packs only), patches them and writes
# them into a movie-type pack: the game reads shaders before it mounts mod packs, so a mod-type pack's are never used.
set -euo pipefail
GAME="$1"; OUT="$2"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cargo build --release -q -p wce_vfs --example vfs_ls --example pack_write --manifest-path "$ROOT/Cargo.toml"
WORK="$(mktemp -d)"
trap 'rm -r -- "$WORK"' EXIT
mkdir -p "$WORK/in"
for f in $("$ROOT/target/release/examples/vfs_ls" "$GAME" fxc/ --vanilla | awk '{print $1}' | grep -E '^fxc/dx11.*rigidriver\.hlsl_ps_ps30_main_battle_river\.fxc$'); do
  "$ROOT/target/release/examples/vfs_ls" "$GAME" x --vanilla --extract "$f" "$WORK/in/$(basename "$f")" > /dev/null
done
python3 -I "$ROOT/scripts/river_shader.py" "$WORK/in" "$WORK/out" > /dev/null
args=()
for f in "$WORK"/out/*.fxc; do args+=("fxc/$(basename "$f")=$f"); done
"$ROOT/target/release/examples/pack_write" --movie "$OUT" "${args[@]}"
