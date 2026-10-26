# BOB headless: Phase 0 spike results (2026-10-08)

Skill with the full command-line reference: `attila-toolkit` plugin, `skills/attila-bob-cli` (1.1.1). This file only records what the tile editor project learned and still needs.

## Command line

From the Blender buildings add-on (`total_war_cs2_addon/bob/cli.py`, `rules.py`), verified there and in the spike:
`BOB.AssemblyKit.exe /dont_stop_on_error /configuration:<name> /offline`, run with the kit's `binaries` as the working directory. The
configuration is `binaries\BOB\<name>_configuration.xml`; its entries use the logical paths `<raw>/`, `<working>/`, `<retail>/`; the
files to build go in `selected_consumers` (not the providers). Exit code 0 / 1, the reasons in `bob_error.log` (and `bob.log`). Never run
two at once; any bare argument pops a modal dialog and hangs. CA's tile example: `binaries/BOB/custom_tile_processing_configuration.xml`
(Pack processor). TEd launches BOB itself with a generated rules file (`PackFile = <retail>/data/%1.pack`).

## Verified

- Processor names follow the plugin DLL names: `Tile` (BOB_Tile), `Vegetation` (BOB_Vegetation) work for battle tiles; `Terrain` actions are named in Terry's real log.
- A scratch kit (copy of `binaries` + one tile + helpers, ~900 MB, `tile_editor/scratch/kit1`) runs BOB headless in about 40 s for a 1x1 tile. Created with `bob_run.py scratch --tile-deps --raw terrain/tiles/battle/<tile>` (the kit1 copy was made before the flag existed and then completed by hand).
- Kit requirements found by elimination: `raw_data/db` (489 MB DB schema) and `raw_data/EmpireDesignData`, otherwise BOB exits 1 after 20 s with empty logs; `working_data/terrain/vegetation/battle/grass/max_grass.xml` for Vegetation.
- Consumer list: the tile's `tile.ted` plus its raw files; processors `Tile`, `Vegetation`, `Battle` together gave 15 actions (Create tile agf, Blendmap, Prepare/Generate Vegetation x8, Generate Grass, Cleanup, Tile).

## Determinism (tile `tdd_custom_dead_marshes/1x1/aa`)

Compared with the kit's committed output and between two fresh runs:

| File | vs kit output | run vs run |
| --- | --- | --- |
| `blend/index/normal/ground_types.dds`, `hf_height_map.data`, `default.grass_list.bin`, all `*_procedural_bmd_data.xml`, 8 of 9 `.bin`, `terrain_outlines.xml` | identical | identical |
| `outfield_tents_procedural_bmd_data.bin` | 4 bytes differ (float low byte) | identical |
| `hf_water_map.data` | 635 vs 1281 bytes | not compared |
| `mesh.rigid_model_v2`, `outfield_mesh.rigid_model_v2` | 11 bytes differ at header offsets 222-233 | **differ** (uninitialised memory in the header; mask 222-233) |

So the oracle is usable: every byte of the main outputs is reproducible; the only known non-deterministic bytes are the 11 bytes in two RMV2 headers.

## Open

- Why `hf_water_map.data` differs from the kit's copy (stale raw inputs in the kit, or a river spline / water input missing in the scratch kit).
- Which consumer triggers `tree_list.bin`, `building_list`, `civilian_*.bin`, `definition.xml`, `non_terrain_outlines.xml`, `prop_marker.markers` (TEd's stage-A export or other actions).
- `Battle` processor target (`groupformations.bin`), `Campaign`, and `Terrain` (out of scope for now: tilemaps).
- Campaign tiles: same recipe with `raw:/terrain/tiles/campaign/` in the campaign kit and `TileDatabase = terrain\tiles\campaign\_tile_database`. Not run yet.

## Who reads and writes what (DLL strings, 2026-10-06)

- **BOB_Tile** reads `tile.ted`, `height_map_0.png`, `blend_map.tif`, `ground_type_map.png`, `lf_influence_map.png`, `nogo.png`,
  `building_list.xml`, `far_building_list.xml`, `terrain_outlines.xml`; writes `bmd_data.bin/.xml`, `final_*.dds`, `index.dds`,
  `ground_types.dds`, `protection_map.dds`, `tile_database.bin`.
- **BOB_Terrain** (map level): `tile_map.png`, `climate_map.png`, `lf_*.tif`, `masks.xml`, season masks, `tree_database.xml`,
  `environment_collection.xml`. Terry's real log (`binaries_terry\bob.log`) names its actions `Terry file`, `Tilemap` (7 min) and
  `Heights & Normals`.
- **BOB_Vegetation**: `default.grass_list.bin`, `_procedural_bmd_data.bin`, using `per_climate_tree_conversions.xml`.
- **BOB_Battle**: `groupformations.bin`, `LandBattle.xml`, `tree_climate_conversions.xml`.
- **TEd** loads and saves `tile.ted`, `map_info.xml`, `config.xml`, deployment polygon XML, `explicit_tiles.txt`, `protection_map.dds`.
- `tile_upgrades.xml` is not in the kits (TDD ships it in its pack).

The stage split per tile (A TEd's export, B BOB_Tile, C BOB_Vegetation) and each stage's files: docs/formats/tile_compile.md and
docs/formats/tile_binary.md.

## Files of a battle tile's raw folder

`raw_data/terrain/tiles/battle/<set>/<size>/<variation>/` (e.g. `plains/4x4/4x4_aa`):
- text / XML: `tile.ted` (the source of truth), and in legacy tiles (v15 `tile.ted` with an empty scene) the stage-A lists in
  `raw_data` instead of `working_data`: `definition.xml`, `building_list.xml`, `far_building_list.xml`, `capture_location_list.xml`,
  `deployment_areas.xml`, `ef_lines.xml`, `zones.xml`, `go_outlines.xml`, `non_terrain_outlines.xml`, `sound_emitter_list.xml`,
  `tree_random_seed.xml`; `river_splineN.xml`;
- binary: `*.markers`, `bmd.tree_list`;
- images: `height_map_0.png`, `blend_map.tif` (+ `.dds`, `.jpg`), `ground_type_map.png`, `lf_influence_map.png`, `nogo.png`,
  `custom_normal.DDS`, BOB's `tile_normal.tga`, `final_*.dds`, `protection_map.dds`, `raw/*.png` (sculpt intermediates);
- artist leftovers: World Machine `*_output.png`, `wm_in.tif`, `.tmd`, `.CS2`, `.max`.

Tile database: `_tile_database/_settings.xml`, `_tile_database/TILES/*.xml` (docs/formats/tile_database.md). Campaign tiles
(`terrain/tiles/campaign`: 1x1 to 8x8 ground sets, lakes, mountains, cliffs, with `mask` footprints; `tile.ted` v1 to v17 mixed) get
the same BOB_Tile outputs.
