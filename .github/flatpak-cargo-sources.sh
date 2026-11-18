#!/usr/bin/env bash
# CI: makes packaging/flatpak/cargo-sources.json from Cargo.lock with flatpak-builder-tools' generator (a pinned commit), in a venv.
set -euo pipefail

TOOLS_COMMIT=9d86d336a39b9af38b86f114b6ce5820fe260f91
venv="${RUNNER_TEMP:-/tmp}/flatpak-cargo-generator"
python3 -m venv "$venv"
"$venv/bin/pip" install --quiet aiohttp tomlkit
curl -sSfL -o "$venv/flatpak-cargo-generator.py" \
    "https://raw.githubusercontent.com/flatpak/flatpak-builder-tools/${TOOLS_COMMIT}/cargo/flatpak-cargo-generator.py"
"$venv/bin/python" "$venv/flatpak-cargo-generator.py" Cargo.lock -o packaging/flatpak/cargo-sources.json
