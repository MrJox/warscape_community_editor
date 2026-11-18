#!/usr/bin/env bash
# CI (nightly.yml): writes the nightly version into [workspace.package] version, so the built binary knows it.
#   .github/nightly-version.sh <base version> <nightly version>
set -euo pipefail

base="$1"
nightly="$2"
sed -i "s/^version = \"${base}\"/version = \"${nightly}\"/" Cargo.toml
if ! grep -q "^version = \"${nightly}\"" Cargo.toml; then
    echo "::error::Cargo.toml version update failed: no version = \"${nightly}\" after replacing ${base}"
    exit 1
fi
