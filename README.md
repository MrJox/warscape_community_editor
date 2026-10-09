# tile_editor

Reader / writer / editor for Total War: Attila battle and campaign tiles (TEd + BOB replacement). Plan: [PLAN.md](PLAN.md). Format notes: `docs/formats`, reverse-engineering notes: `docs/re`.

## Build

```
cmake -S . -B build -G "Visual Studio 17 2022" -A x64
cmake --build build --config Release
```
It compiles a few sources of `../weather_editor` in place (lossless XML, Dear ImGui, PNG screenshot helper); zlib 1.3.1 is vendored in `third_party/zlib`.

## Tools (all under build/Release)

| Tool | What it does |
| --- | --- |
| `roundtrip <manifest> <kits parent>` | byte-identity gate over the corpus manifest (`tools/corpus_inventory.py` builds it) for every registered format |
| `scene_check` | tile.ted gate: parse/save, no-op edits, number formatting |
| `tdb_build <_tile_database dir> <extra tiles dir or -> <shipped bin or ->` | rebuilds `tile_database.bin` from `_settings.xml` + `TILES/*.xml` and compares with a shipped file |
| `height_check <height_map_0.png>...` | height field conversions, brushes, undo, kit-exact PNG save |
| `tile_studio [--tile <folder>]` | the viewer / editor (below) |
| `png_debug`, `order_test` | investigation helpers |

## Tile Studio

`tile_studio.exe --tile <.../raw_data/terrain/tiles/battle/<set>/<size>/<variation>>` opens a tile: terrain from `height_map_0.png`, every tile.ted instance as a box (types colour coded), polygon outlines, layer tree, per-type visibility, inspector.

Mouse: RMB orbit, MMB pan, wheel zoom, LMB select (Select tool) or paint (Raise / Lower / Smooth / Flatten / Level). `G` grabs the selected instance onto the ground under the cursor (LMB confirm, Esc / RMB cancel). Ctrl+Z / Ctrl+Y undo / redo (terrain strokes and instance moves share one history). Ctrl+S saves `tile.ted` (only the changed attribute text is rewritten) and `height_map_0.png` (kit-exact encoder) next to the originals, keeping a one-time `.bak`.

Scripted use (same code paths as the mouse): `--select <type or element index> --move <dx> <dz> --paint <raise|lower|smooth|flatten|level> <x m> <z m> <steps> --undo <n> --save-to <folder> --cam <yaw> <pitch> <dist> --shot <png> --frames <n>`.

Scene units: x / z are half metres (unit scale 2), y metres; the tile is `cells * 128` m square (see `docs/formats/tile_ted.md`). Known gap: for settlement tiles the instance heights do not line up with `height_map_0.png` (they sit on the LF terrain), so buildings may float or sink in the viewer.
