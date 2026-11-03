# Terry: the kit's terrain tool (survey, 2026-11-02)

What the Assembly Kit's map-level terrain tool does, for the Terrain tool's plan (docs/plans/terrain_editor.md). Black box only so far:
the strings and exported symbols of the binaries (no decompilation), the kit's data files and the projects it saved. Nothing here was run:
Attila's Terry crashes at launch (user, 2026-11-02); Thrones of Britannia's runs (slowly) and is the one to take references from.

## Where it is

- Terry is not an executable of its own: it is a plugin of **TWeak** (`binaries/TWeak.AssemblyKit.exe`), the DLL
  `TWeak_TerrainMetadataEditor.AssemblyKit.dll` (namespace `TerrainMetadataEditor`, menu `&Terry`, project extension `.terry`), on Qt 4
  and `QtToolUtility.AssemblyKit.dll` (namespace `QTU`: the entity / component model, the brushes, the grid painter). The same files ship
  in Thrones' kit (1 253 376 bytes against Attila's 1 252 352). Rome 2's kit has no Terry.
- Its data: `editor_working_data/Terry/bca_generation_spec.xml` (the battle catchment area generator's rules), `layout_map_legend.xml`
  (campaign layout colours -> guide models), `Terry/bca_generation_errors.txt` (written).
- The 3D view is the engine's own scene (`WARSCAPE::TW_SCENE::terrain`, `rigid_v2`, `vfx`, `weather`; `Weather/default_battle.environment`
  / `default_campaign.environment`), with `toggle_terrain_vtex` (the virtual-texture path, docs/re/terrain.md) and a water plane switch.

## Projects

`raw_data/terrain/<battles|campaigns>/<map>/<map>.terry`: XML, `<project version="3"><scene version="10">`, a flat list of `<entity id name>`
(64-bit hex ids) with `<components>` and `<children>` (child ids). The map's images sit next to it.

| Project | Entities | Components (count) |
|---|---|---|
| `battles/main_attila_map` (32.8 MB) | 53 782 | ECTransform 52 016, ECRectangle 49 814, ECLandscapeAbove 49 814, ECTileAlignment 24 907, ECBattleCatchmentArea 24 907, ECProcedural 24 882, ECDeploymentZoneRegion 2 202, ECPolyline 2 202, ECDeploymentZone 1 766 |
| `campaigns/main_attila_map` (12.3 MB) | 14 644 | ECTransform 14 644, ECSeasonMask 14 632, ECMesh 14 322, ECLandscapeAbove 14 322, ECPropMesh 14 322, ECVFX 310, ECEnvironmentVolume 12 |
| a settlement map (`battles/settlement_*`) | 0 | (`<project version="3"><scene version="10"/></project>`, 62 bytes) |

- A battle catchment area (BCA): an entity named after the battle (`Thermopylae`), `ECRectangle` 2048 x 2048, `ECTransform` (position in
  world units, y on the LF), `ECLandscapeAbove`, `ECTileAlignment`, `ECBattleCatchmentArea catchment_type` (`land_normal`, `land_ambush`,
  ...), and a child `Catchment Region` (a larger `ECRectangle`). `ECProcedural` marks the generated ones (24 882 of 24 907: almost all).
- Deployment zones: `ECDeploymentZone` (alliance, category, configuration, type) with `ECDeploymentZoneRegion` children holding an
  `ECPolyline`.
- Campaign props: `ECMesh model_path="RigidModels/campaign/..." opacity glow_amount burn burn_amount`, `ECPropMesh
  visible_inside/outside_snow_zone, visible_inside/outside_destruction_zone`, `ECSeasonMask spring summer autumn winter`.
- Other components in the DLLs: `ECRiver` (a spline, `Flow_frequency`), `ECSpline`, `ECCircle`, `ECVFX` / `ECVFXSignature`,
  `ECEnvironmentVolume` (inner / outer radius), `ECCamera`, `ECSignature`, `ECRiverSignature`.

## Features (from the UI strings, slots and class names)

- **Tile map** (`TileMap`, a 2D view `TileMapView` and the 3D one): tile set layers (`TileSetLayer`, `add_layer`, layers inserted and
  removed; `Paint Tile Set`, `Fill Tile Set`, `Command_PaintTileSet`) and a tile instance layer (`TileInstanceLayer`: `Place Tile`,
  `Add Tile`, `Move Tile(s)`, `Remove Tile(s)`, rotation; `instance_fits_on_map`, `instances_occupying`, `subtile_masked_valid`): the
  `tile_map.png` and the `explicit_tiles.txt`. "There are elements in this map that are missing from the database." `Import Map`.
- **Tile window** (`Tool_TerrainWindow`: `Tile Window Enabled / Centre / Size`, `set_tile_window`): the part of the map whose real tiles the
  3D view loads; the rest is the LF ground.
- **LF heights** (`Tool_LFHeightMap`, one operation `OpSimpleChannel0` with `Subtract`): painting `LF Height (Normal)` and
  `LF Height (Sea)` (`lfhm`, `lfhm_sea`; `Editable Low Frequency`); brushes `FalloffBrush` (hardness, falloff models `Radius Sin`,
  `Radius Sin Sin` in a `FalloffModelDialog`), `ImageBrush` (a grey float image, `Load Brush`), `UnitDiskBrush`, `UnitSquareBrush`; size,
  scale, aspect ratio, rotation. `Allow Undo Of Terrain Painting` is an option (their undo of a stroke was costly). `Apply LF Patches`,
  `Apply Per-Tile LF Patches` (the custom-mesh tiles' `lf_mask.tif`, docs/re/battle_maps.md).
- **Entities**: create modes spot, circle, rectangle, polyline, spline (`New Battle Catchment Area`, `New Deployment Zone`, `New River`,
  prop mesh, VFX, environment volume); select / translate / rotate / scale gizmo, clone, multi-selection; a scene tree, the entity
  properties, the undo stack view; a quadtree spatial manager.
- **Generate Battle Catchment Areas** (`BattleCatchmentAreaGenerator`, `Zonifier`): from `bca_generation_spec.xml` (per battle type the
  tiles / sets that make its infield and those it must not contain, its boundary, its deployment zones' categories and restrictions;
  globals `deployment_min_area`, `deployment_split_width`, `deployment_coarseness` 0.05, `map_border_for_outfield` 32, `delete_invalid`)
  and `deployment_mask.png`; messages "Removing old BCAs", "Generating deployment zones", "Less than two zones created", "More than two
  zones created - just kept largest two", "Infield contains restricted tiles". `on_dump_locations_blm`: the BCAs are what
  `locations.blm` lists (docs/re/battle_maps.md).
- **Campaign layout** (`CampaignLayoutDrawer`, `Layout Map Configuration`): `layout_map.png` drawn over the map (opacity, relative scale,
  tile extension), its colours placed as guide models (`layout_map_legend.xml`); `db/campaign_map_playable_areas.xml`.
- **Seasons** (`SeasonManager`, `season_changed`, `update_valid_for_season`): what shows per season (the season masks).
- **BOB**: Terry runs BOB_Terrain's actions `Terry file`, `Tilemap` (7 minutes on the campaign battle map) and `Heights & Normals`
  (`binaries_terry\bob.log`, docs/re/bob.md).

## The maps' files (Attila kit)

| File | `battles/main_attila_map` | `campaigns/main_attila_map` |
|---|---|---|
| `tile_map.png`, `climate_map.png` | 2032 x 1437 cells | yes |
| `lf_heights.tif` (16-bit grey) | 8128 x 5748 (4 px a cell, 93 MB) | 16256 x 11528 (375 MB) |
| `lf_sea_heights.tif` | 23 MB | 23 MB |
| `normal.dds` | 250 MB | 250 MB |
| others | `culture_map.png`, `snow_map*.png`, `nogo.dds`, `explicit_tiles.txt`, `config.xml`, `tile_map.tiles` / `.index` (an old version 7 pair) | `layout_map.png`, `lf_sea_colour*.tif`, `masks.xml`, `season_mask_*.png`, `snow_mask/` |

## BOB_Terrain's outputs (Phase 0, 2026-11-02)

Fresh BOB runs on a scratch kit (how: docs/re/bob.md, "The Terrain processor headless"): `main_attila_map` (14 minutes) and `wce_relief`, a
copy of `settlement_western_city_b_small` with synthetic relief written into its `lf_heights.tif` (563 .. 4160) and `lf_sea_heights.tif`
(every TEd-made map in the kit has a flat LF, 436 or 655: useless for this). The 42 BOB outputs already in the user's `working_data` were
built with an older tile database: they are not fill oracles (33 of 43 differ, all by the custom tiles; a fresh run agrees).

| File | Written by | What it is | Status |
|---|---|---|---|
| `terrain.tile_list` | Tilemap | the placements | the port's fill exact (`wce_relief` 73 / 73, docs/re/battle_maps.md for the rest) |
| `tile_map.index` / `.tiles` | Tilemap, with `save_final_tile_map` | the final tile map (v9) | the port's `fill_kit_tile_map` + writer: `main_attila_map` byte-identical (116 834 848 bytes); on `wce_relief` every byte but BOB's uninitialised LF floats of non-origin cells |
| `lf_height_map.dds` | Heights & Normals | L16, no mips, the TIFF's samples through `trunc(f32(f32(a * f32(1 / 65535)) * f32(65535)))` (a sample one step lower now and then) | made byte for byte (`heights_build_check`, `wce_relief` and `main_attila_map`) |
| `lf_sea_height_map.dds` | Heights & Normals | L16 with a full mip chain (to 2 x 2: 11 levels for 4064 x 2874); level 0 as above; each level from the float level above: `f32(((a + b) + c) + d) * 0.25` over the 2 x 2 block, then truncated the same way | made byte for byte, every level (`heights_build_check`, `wce_relief` and `main_attila_map`'s 11 levels of 4064 x 2874: odd sides halve with floor) |
| `lf_height_map.data` | Heights & Normals | CHMF v5 of the LF samples converted as the DDS's (header min = the lowest, max 65535; spacing 1); whole regions of 16 x 16 cells (cells of 8 x 8 samples on a settlement map, 4 x 4 on the campaign one: 11 430 regions; the last row and column reach past the map, rows 1424 .. 1440 of 1437, with its edge samples); each region's tag `[region min, 65535]` | made by `wce_terrain_io::attila::heights` byte for byte (`heights_build_check`: `wce_relief` and `main_attila_map`, 53 MB) |
| `locations.blm` | Tilemap, with `save_meta_data_map` | the `.terry`'s BCAs by type, in entity order; header and the climate table as the game's | counts = the `.terry`'s per type; 5 of 9 types record for record as the game's (its `.terry` was older for the procedural ones). Centre = the BCA's position in cells (`x / 256 + width / 2`, `height / 2 - z / 256`), rectangle from its `Catchment Region` child; the rounding to cells is open |
| `locations.blm.mask0` / `.mask1` | Tilemap, with `save_meta_data_map` | a u16 bitset per cell | open: the low bits of `mask0` look like `1 << climate` (the blm's climate order); other bits follow the placed tiles (their data was not in the scratch kit) and maybe the BCAs. `main_attila_map`: `mask0` agrees with the game's on no cell, `mask1` on 75 % |
| `deployment/deployment_NNNN.xml` | Tilemap, with `save_meta_data_map` | `BATTLE_DEPLOYMENT_AREA_HASH_TABLE`: per alliance a `deployment_area` (category `DAC_*`) of `standard additive` boundaries, tile-local units | one file per two `ECDeploymentZone`s (1 766 -> 883); the game's 2 074 come from another `.terry` |
| `<map>.agf`, `entities.agf` | all | BOB's asset list (`##folder`, `#all`, `#tiles`) | text |

- `Terry file` (7 s on the 32 MB project) writes nothing visible; the blm, masks and deployments come out of `Tilemap` when the map's
  folder has a `rules.bob` with `save_meta_data_map` (docs/re/bob.md).
- No normal chunks were written: the game's 736 come from elsewhere (the kit's 250 MB `normal.dds`; open).
- `Tilemap` logs "Failed to find tile for point 360x977" (and three more points) on `main_attila_map`; the port gives 4 messages too.

## Per-tile LF patches (Ghidra, 2026-11-03)

Project `kit` in `I:\wce_scratch\ghidra_kit` (Attila's `TWeak_TerrainMetadataEditor.AssemblyKit.dll` and `ToolDataBuilderDll.AssemblyKit.dll`,
image base 0x10000000).

- Terry's `Apply Per-Tile LF Patches` (action `on_apply_lf_patches()`, fn 0x100018d0, `ApplyPerTileLFPatches.cpp`): the map's tile
  instances grouped by variation; each variation folder's `lf_mask.tif` read as a scalar field (none: skipped); for each instance
  `TerrainMetadataEditor::apply_lf_patch` (fn 0x10001270): the mask must be the tile's cells times the samples a cell; its rectangle at the
  instance's origin cell times that (turned: w and h swap for 0x20 / 0x80) must lie in the LF, else the instance is skipped; every mask
  sample below 0.9 (`DAT_100c2ec8`) is written into the LF sample under it, the others leave it. Mask sample (u, v) goes to the field at
  (fn 0x10001080, rotation = the instance's flags & 0xf0): 0x20 (h - 1 - v, u), 0x40 (w - 1 - u, h - 1 - v), 0x80 (v, w - 1 - u), else
  (u, v). One undo step, "Apply LF Patches". Tiles with a mask: the custom-mesh cliffs (0 on the low side, 65535 on the plateau) and the
  `sea` set (all 0).
- BOB has the same walk, `apply_per_tile_lf_patches` (fn 0x1002c5c0, `TerrainProcessor.cpp`, the same transform at fn 0x1002c4f0), which
  multiplies the LF by the mask (fn 0x1002c3f0), but nothing calls it (no call, jump or address of it in the DLL): BOB builds the LF as the
  TIFF has it, which agrees with the heights outputs matching the TIFF's byte for byte.
- No Attila kit map has them applied: on all 113 every low sample under a patch differs from the mask (the campaign map: 2.1 million
  samples, mostly the coasts under sea tiles). So the orientation could not be checked on data; both DLLs agree on it.
- The editor: `wce_terrain::lf_patch`, the Terrain tool's "Apply per-tile LF patches" (`terrain_lf_patches`). ASSUMPTION: the mask and
  the LF are read as 0 .. 1 of 65535, so the value written is the mask's raw sample (the masks hold only 0 and 65535).

## Open

- The blm rectangles' rounding, the masks' bits, where the game's 736 normal chunks come from.
- The falloff models' formulas (`Radius Sin`, `Radius Sin Sin`) and `OpSimpleChannel0`'s properties.
- The BCA generator's algorithm (the `Zonifier`): black box first (run it in Thrones' Terry on a scratch map, diff the `.terry`).
