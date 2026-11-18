#!/usr/bin/env bash
# Builds the Flatpak bundle as the release does: target/flatpak/wce-<version>-linux-x86_64.flatpak (packaging/flatpak/README.md).
#
#   scripts/flatpak.sh [--skip-cargo-sources] [--install]
#
# --skip-cargo-sources  keep packaging/flatpak/cargo-sources.json (else made again from Cargo.lock: a few minutes)
# --install             install the bundle for the user afterwards (`flatpak run io.github.mrjox.WarscapeCommunityEditor`)
#
# Needs flatpak, flatpak-builder and flatpak-cargo-generator (flatpak-builder-tools); the SDK, its Rust extension and the runtime are
# installed from Flathub (for the user) when missing.

set -euo pipefail

APP_ID=io.github.mrjox.WarscapeCommunityEditor
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DIR="$ROOT/packaging/flatpak"
OUT="${WCE_FLATPAK_OUT:-$ROOT/target/flatpak}"

skip_sources=false
install=false
for arg in "$@"; do
    case "$arg" in
        --skip-cargo-sources) skip_sources=true ;;
        --install) install=true ;;
        *) echo "unknown option: $arg" >&2; exit 1 ;;
    esac
done

cd "$ROOT"
version=$(cargo metadata --no-deps --format-version 1 | python3 -c 'import json,sys; print(next(p["version"] for p in json.load(sys.stdin)["packages"] if p["name"] == "wce_app"))')
bundle="$OUT/wce-$version-linux-x86_64.flatpak"
echo "Version $version"

if [ "$skip_sources" = false ] || [ ! -f "$DIR/cargo-sources.json" ]; then
    generator=$(command -v flatpak-cargo-generator || command -v flatpak-cargo-generator.py || true)
    if [ -z "$generator" ]; then
        echo "flatpak-cargo-generator is not installed: https://github.com/flatpak/flatpak-builder-tools/tree/master/cargo" >&2
        exit 1
    fi
    echo "Making cargo-sources.json from Cargo.lock"
    "$generator" Cargo.lock -o "$DIR/cargo-sources.json"
fi

flatpak remote-add --user --if-not-exists flathub https://dl.flathub.org/repo/flathub.flatpakrepo

# The version being built goes into the metainfo for the build only; the file is put back afterwards.
metainfo="$DIR/$APP_ID.metainfo.xml"
backup=$(mktemp)
cp "$metainfo" "$backup"
trap 'cp "$backup" "$metainfo"; rm -f "$backup"' EXIT
sed -i "s|<release version=\"[^\"]*\" date=\"[^\"]*\"|<release version=\"$version\" date=\"$(date -u +%Y-%m-%d)\"|" "$metainfo"

mkdir -p "$OUT"
flatpak-builder --user --install-deps-from=flathub --force-clean --state-dir="$OUT/state" --repo="$OUT/repo" "$OUT/build" "$DIR/$APP_ID.yaml"
flatpak build-bundle --runtime-repo=https://dl.flathub.org/repo/flathub.flatpakrepo "$OUT/repo" "$bundle" "$APP_ID"
echo "Bundle: $bundle"

if [ "$install" = true ]; then
    flatpak install --user -y --noninteractive --bundle "$bundle"
fi
