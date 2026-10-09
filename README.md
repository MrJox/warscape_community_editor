# Warscape Community Editor

One editor for Total War: Attila (Warscape engine) content: weather and lighting, battle and campaign tiles, and later
units, VFX, battle maps and cinematics. It shares one viewport and one renderer built on the game's own shaders. Plan
and status: [PLAN.md](PLAN.md).

**State: the two former editors are retired (Phase 8).** `wce.exe` is the one application: the weather tool, the tile tool
(the old Tile Studio) and the shared shell, with the gates under `cli/` and `scripts/`. The earlier plans are in
[docs/plans/](docs/plans/).

## Build

```
cmake -S . -B build -G "Visual Studio 17 2022" -A x64
cmake --build build --config Release
```

Executables land in `build/bin/<config>` (`wce.exe` and the gates). Use `--config RelWithDebInfo` for test builds while an editor
is open (a Release build cannot overwrite a running exe).

## Gates

`scripts/gates.sh [out dir] [--corpus] [--render]` runs every gate with fixed inputs and writes one
output file per gate. `scripts/imgcmp.py <run a> <run b>` compares the reference screenshots of two runs. The recorded
baseline and the comparison rules are in [docs/baseline.md](docs/baseline.md). Every migration step must reproduce it.

Requirements: the game, RPFM (`rpfm_cli`), the battle/campaign tilemap assembly kits for the tile gates, and
`manifest/corpus.tsv` (built by `scripts/corpus_inventory.py`; not in git).
