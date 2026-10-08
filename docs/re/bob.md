# BOB headless: Phase 0 spike results (2026-10-08)

Skill with the full command-line reference: `attila-toolkit` plugin, `skills/attila-bob-cli` (1.1.1). This file only records what the tile editor project learned and still needs.

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
