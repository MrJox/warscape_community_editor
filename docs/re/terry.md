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
| `locations.blm` | Tilemap, with `save_meta_data_map` | per battle type the `.terry`'s BCAs in entity order; then the cell count and the climate table | made byte for byte (`wce_terrain::locations`, `built_check`; "locations.blm and its masks" below) |
| `locations.blm.mask0` / `.mask1` | Tilemap, with `save_meta_data_map` | a u16 bitset per cell | made byte for byte (below) |
| `deployment/deployment_NNNN.xml` | Tilemap, with `save_meta_data_map` | `BATTLE_DEPLOYMENT_AREA_HASH_TABLE`: one file per BCA with zones (entity order); per alliance a `deployment_area` per zone (`DZC_*` as `DAC_*`), per region a `standard additive` / `subtractive` boundary relative to the BCA (f32, `%f` rounding half away from zero) | made byte for byte (`wce_terrain::deployment`: 883 of `main_attila_map`, 384 of Thrones' `vik_attila_map`) |
| `<map>.agf`, `entities.agf` | all | BOB's asset list (`##folder`, `#all`, `#tiles`) | text |

- `Terry file` (7 s on the 32 MB project) writes nothing visible; the blm, masks and deployments come out of `Tilemap` when the map's
  folder has a `rules.bob` with `save_meta_data_map` (docs/re/bob.md).
- No normal chunks were written: the game's 736 (`normals/normal_N.dds`, 576 x 576 DXT5 with mips by NVTT: 144-cell squares 64 cells apart, the old tile map's chunk grid) come from elsewhere, likely cut from the kit's 250 MB `normal.dds` (8128 x 5764 ARGB); not made (open).
- `Tilemap` logs "Failed to find tile for point 360x977" (and three more points) on `main_attila_map`; the port gives 4 messages too.

## The scene's structure, from the files and BOB (2026-11-03)

- **Two trees.** An `<entity>`'s `<children>` is the logical tree (a BCA's: its `Catchment Region`, then its deployment zones); its
  `ECTransform`'s own `<children>` is the spatial one, which positions are relative to (a BCA's: the region only). A zone has no
  transform; its regions' positions are on the map (`DZR 0` at -105150 under `BCA 00521` at -104192).
- **Units.** Positions are metres from the map's centre (the tile list's `origin_offset`: `width / 2`, `height / 2` in whole cells), x
  east, z north, 256 a cell: every settlement and port BCA of `main_attila_map` (421) has its centre on a settlement or port tile so
  read, 4 do with z flipped. A polyline's point (x, y) is at (x, z) + its entity's position: BOB's deployment files have the points
  shifted by the BCA's position, y as z.
- **Deployment zones** come in pairs, alliance 0 then 1 (`DZ k/0`, `DZ k/1`; 883 pairs on `main_attila_map`), children of a BCA: BOB's
  `Tilemap` writes one `deployment_NNNN.xml` per BCA with zones (per alliance, its regions as boundaries relative to the BCA's
  position, `DZC_RIVER` as `DAC_RIVER`), none for zones of no BCA. Checked on an edited map: the boundaries are the model's.
- **Versions.** The kit's projects with entities are all `<project version="3"><scene version="10">`; the empty ones are mostly 2 / 3.
  BOB's `Terry file` fails (exit 1, no message) on a version 2 / 3 project with entities in the version 3 / 10 form.
- No entity of the kit is turned or scaled (52 016 transforms of `main_attila_map`: `rotation="0 0 0"`, `scale="1 1 1"`).

## Running Thrones' Terry: the BCA generator's oracle (2026-11-03)

Thrones of Britannia's TWeak / Terry runs on a scratch copy of its kit, driven through its window (it has no command line for a project
or an action: `/standalone <plugin>` and `/autotest` only). What it needs besides the kit: a `data` folder beside the kit holding
`movies.pack` (its test for the game; a junction to the game's `data` does), and `working_data/terrain/tiles/battle` and `campaign`
`tile_database.bin` (from the game's packs; without them loading a project crashes in `Warscape.AssemblyKit.dll`). In Load Options,
"Scene (Entities)" must be on: without the scene, Save writes the LF TIFFs and not the `.terry`.

`vik_attila_map` (Thrones' campaign battle map) through `Tools > Generate Battle Catchment Areas`, then Save As: 636 BCAs (the shipped
project: 630) with their `Catchment Region`s, 780 deployment zones with 1 088 regions; the types: coastal_battle 125 (shipped 118),
land_bridge 236 (235), port_assault 25, settlement_standard 63, settlement_unfortified 158, unfortified_port 29 (31). No
`bca_generation_errors.txt` was written. The fresh output is the oracle; the shipped project came from other inputs or an older
generator. Kept outside the repository (`I:\wce_scratch\thrones_kit\oracle` on this machine).

## The BCA generator (Ghidra, 2026-11-04)

Thrones' `TWeak_TerrainMetadataEditor.AssemblyKit.dll` (project `kit`, `GenerateBattleCatchmentAreas.cpp`, main fn 0x10062bc0); ported as
`wce_terrain::bca` and `bca::zones`, checked by `bca_check` against the oracle above: every one of the 636 BCAs (name, type, cell) and of
their 780 zones, 1 088 regions and 8 317 polyline points (to 5 cm) agrees (gate `bca_<map>`, `WCE_BCA_ORACLE`).

- **The battle field.** The BCAs come from Terry's `BATTLE_TILE_MAP`, in its tile order (the BCA names count up). Terry builds it as the
  game does (`TERRAIN_SETUP::build_terrain` in Thrones' `Warscape.AssemblyKit.dll`: `BATTLE_TILE_MAP::load` of a `terrain.tile_list`, else
  `TILE_MAP::load` of `tile_map.index` / `.tiles` and `TILE_MAP::build_battle_field`, row by row over the origin cells, tile layer
  then geometry layer), through the game's file system: for `vik_attila_map` that is the game packs' tile map (they ship no
  `terrain.tile_list`), not BOB's in working_data. With BOB's battle field the BCAs are the same 636 at the same cells, 514 by the same
  name. A tile_list's tiles missing from the tile database are dropped in order (fn 0x1036da10).
- **BCAs.** Per tile instance, per spec type in the spec's order: a tile in the type's `infield` (its tile set's name, its location under
  `terrain/tiles/battle/`, or that and `/<variation>`) makes a BCA at `(x + width / 2, y + height / 2)` (the unturned size) unless one of
  that type is there or it is within `map_border_for_outfield` of the edge; named `BCA %05d` by the count so far. Another instance of
  `infield_restriction` in the 8 x 8 cells around it (-4 .. 3; a cell's instance is the first whose unmasked subtiles cover it): "Infield
  contains restricted tiles".
- **Deployment zones** (types with a `<deployment category>`), per configuration (one, 0; `dual_config` two):
  - the instances of the 8 x 8 cells around the centre; each one's region in 1/128 cells relative to the centre cell's corner: its
    variation's `deployment_mask.png` for the configuration (mirrored vertically, `width / tile width` pixels a cell, luminance
    `(11 R + 16 G + 5 B) >> 5` above 25 the key 1, above 76 the key -1; the configuration's key, else -1), else none for a tile in the
    deployment's `restriction`, else its unmasked subtiles. Turned with the instance (Qt transforms; the flip is ignored). Thrones' kit
    has no tile folders, so no masks were read; Attila's has 107 (`<tile>/<variation>/deployment_mask.png`, 8-bit RGB, 128 pixels a
    cell mostly; 62 under `main_attila_map`'s tiles). Against Attila's shipped `main_attila_map.terry` (made from other inputs, as
    Thrones' shipped one: no oracle) the zones of 23 551 of 24 337 common BCAs agree; unmirrored masks lose 850 BCAs to errors;
  - their union cut to the battle rectangle: -512 .. 511 inset by `int(1024 * 64 / (unit * 128 * 8) + 1)`, 33 for unit 2 (+-958 m);
  - the Zonifier: the region's y-x banded rectangles' sides as directed unit edges (top left to right, bottom right to left, right side
    down, left up; an edge meeting its reverse cancels), in a multimap by start point (x, then y); a polygon traced from the first edge:
    at each end the first edge from there, the second when the turn is to the left (float cross product > 0) and there is one, a vertex
    where the direction changes; every edge start recorded in a point -> polygon hash. A polygon becomes a hole of the first polygon of
    the other area sign met stepping right (1/128) from its rightmost vertex;
  - each polygon in cells (x 1/128), then Douglas-Peucker from its least point (x, then y) to its greatest and back, squared distances in
    f32 against `deployment_coarseness`^2;
  - fewer than two: "Less than two zones created"; more: "More than two zones created - just kept largest two" (a stable sort by area);
    with `split` (coasts) both outlines cut by two lines along the centroids' axis, `deployment_split_width` / 2 to each side
    (Sutherland-Hodgman in f32): each side a zone of pieces;
  - the entities: `DZ <configuration>/<alliance>` (`ECDeploymentZone`, type `DZT_1V1`, category `DZC_<CATEGORY>`), zone 0 alliance 0 and
    zone 1 alliance 1; with `dual_config` configuration 1's alliances swap when its centroids' axis points against configuration 0's.
    An area below `deployment_min_area`: "DZ c/a is too small". Under it a `DZR <n>` per outline then per hole, depth first
    (`DZRT_ADDITIVE`, a negative area `DZRT_SUBTRACTIVE`), the points `tile_to_world_space(centre + p)`; the region's transform at its
    first point, the points relative (y as z).
  - With `delete_invalid` a BCA with any error goes (the oracle: 57 of 693, its number not reused).
- The y of the BCAs and regions is the terrain's height (the tiles' too: -21.8 on a sea tile whose LF is 0 .. 0.014): not ported.

## locations.blm and its masks (Ghidra, 2026-11-04)

BOB_Terrain's `Tilemap` (BOB_Terrain.AssemblyKit.dll fn 0x10011f60, its masks from ToolDataBuilderDll's `process_terrain_internal`
fn 0x1002e2f0) through the engine's `WARSCAPE::BATTLE_LOCATIONS_MAP` (`add_catchment`, `set_mask`, `save`; Thrones' engine DLL has the
names). Ported as `wce_terrain::locations`; byte for byte (`built_check`) on Attila's `main_attila_map`, `wce_edit`, `wce_relief` and
Thrones' `vik_attila_map`.

- **Records**: the rectangle's edges and the centre to cells as `w / 2 + trunc(x / 256)`, `h / 2 - trunc(z / 256)` (metres, truncated
  toward zero: an exact half rounds the two sides of the map's centre opposite ways); the id `0xffff0000 + n`, `n` the BCA's deployment
  file's number, for a BCA with zones, else -1.
- **Order**: the types and the climate table are `CA_STD::UNORDERED_MAP`s iterated bucket by bucket: 4 buckets, a key appended to
  bucket `hash % n`, grown to `2n + 1` when the keys outnumber the buckets; the hash (fn 0x1003eb50) starts at 0xf73a51c9 and xors each
  signed byte from the last back, shifted 0, 8, 16, 24 in turn. The types are inserted as their first BCA comes, the climates in the tile
  database's order.
- **The climate table** (fn 0x1002d110): per climate of the database, the `environment_map.xml` entries whose `climate` contains its
  name (a substring: `ROME_Destruct` takes `ROME_Destructdes`'s) give `battle_types` land 2, sea 4; `time_of_day` day 0x20000, night
  0x10000; `precipitation` dry 0x400000, rain 0x200000, fog 0x40000, snow 0x100000, storm 0x80000.
- **mask0** per cell of the tile layer (a covered cell: its origin's tile): 1 an `encampable` tile; 0x100 no tile; 8 a tile with a
  barbarian variation `minor`; bits 4 .. 7 the cell's climate; 0x400, 0x800, 0x1000, 0x4000, 0x8000 the `culture_map.png` colour of the
  first five `tile_upgrades.xml` entries with `id` and a colour (when the image covers the map); 0x200 snow; then BOB_Terrain's own
  pass over `nogo.dds` (B, G, R bytes): (185, 139, 0) 4, (0, 174, 0) 2.
- **mask1**: 0x1000 a barbarian variation `small`, 0x2000 `medium`; snow (fn 0x1002dbc0): with `snow_map_sp`, `_su`, `_au`, `_wi.png`
  of one size (BOB accepts them unless both sides differ from the map's: `main_attila_map`'s are 4 rows taller), a white cell sets
  0x100 << season and mask0's 0x200; without them a white `snow_map.png` cell sets 0x200.
- The engine reads them back as `get_available_enviroment_mask` (the climate's flags, 0x200, mask1 << 16), `get_culture_id`, the snow
  attrition queries and `point_contains_mask` (`AM_BARBARIAN_*`).

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

## Campaign maps (Ghidra, 2026-11-04)

BOB's `Terry file` and `Tilemap` on `terrain/campaigns/main_attila_map` (a scratch kit), every one of its 13 files made byte for byte by
`wce_terrain_io::attila::build` (`build_check campaigns/main_attila_map`, gate `build_campaign_<map>`):

- **`prop_marker.markers`** (`PROP_MARKER` v9, class 645566, repository 645563): BOB_Terrain's fn 0x10037b00 per `ECPropMesh`, in
  entity order. The model path (after `working_data/` when it has one) is the group; the matrix is the kit's `ECTransform::world_transform`
  (QtToolUtility; below); the flags are the four visibility getters, the season mask, and 0x10, the fauna bit, when the path holds
  `/fauna/` (case-sensitive: `unknown_74` in our layout, 1694 records). Groups in the engine's marker table order.
- **The transform** (QtToolUtility, all f32 SSE): `on_rotation_changed` (rva 0x190e0) takes the Euler degrees, `d * f32(pi) * f32(1 /
  180) * 0.5` each, `CA::ca_sincos` (x87 `fsin` / `fcos`, rounded to f32), then the quaternion `x = (cz cy) sx - (sz sy) cx`, `y = (sz cy)
  sx + (cz sy) cx`, `z = (sz cy) cx - (cz sy) sx`, `w = (sz sy) sx + (cz cy) cx` (products left to right). `update_transform` (rva
  0x1b0a0): `x2 = 2x` ..., the rotation `[[1 - yy - zz, xy - wz, xz + wy], [xy + wz, 1 - xx - zz, yz - wx], [xz - wy, yz + wx, 1 - xx -
  yy]]` (`xx = x2 * x`, `xy = y * x2`, `wz = w2 * z` ...; `1 - a - b` left to right), times the scale on each column, the translation in the
  last column; full 4 x 4 products, so a zero entry comes out +0. A marker's row `i` is the transform's column `i`.
- **`particle_emitter_marker.markers`** (`PARTICLE_EMITTER_MARKER` v9, class 618864): per `ECVFX` the same matrix, `autoplay`, the
  visibility flags, `scale` and the seasons.
- **`environment_collection.xml`**: the `ECEnvironmentVolume`s, outer radius 0 the global lighting, the others spheres (`%f`); CRLF lines.
- **The LF**: `lf_height_map.dds` the 16256 x 11528 LF's 2 x 2 box average, `lf_height_map_lq.dds` its 4 x 4 one: each output sample
  the block's samples as `a * f32(1 / 65535)` summed row by row from 0 in f32, divided by the block's size, times 65535, truncated (not a
  chain of halvings: that is ±1 off). `lf_height_map.data`: CHMF v5 of a sample a cell, the LF point-sampled at each cell's corner (every
  8th sample; converted as the DDS's), 16 x 16 samples a region, the last row of regions past the map holding the LF's edge samples; the
  header's two counts 1 and 8 (the samples a cell and the LF's samples a cell, ASSUMPTION), which a decoder reading them as samples a
  cell takes for 16 x 128.
- **The fill**: as a battle map's (docs/re/battle_maps.md) with the campaign database's shared geometry and `also_place_tile_set`; the
  battle field has the campaign's header (`is_campaign` 1, LF position 0, 0, origin offset 0, -height).
- `locations.blm` is 28 bytes (`BLMF`, 8, 0, width, height, 0, 0) and both masks empty.

## Open

- The campaign render modes' use in the game: a geometry-layer tile (0x100) under a texture-only one (0x600), the sea added under a
  coast (0x300). The editor leaves the 0x100 ones out and draws the rest whole.
- A campaign prop's y: the kit's follow the tiles' geometry (a line through the LF misses by 0.56 at the median); a new one takes the
  line's.
- Where the game's 736 normal chunks come from (and an encoder for them).
- The falloff models' formulas (`Radius Sin`, `Radius Sin Sin`) and `OpSimpleChannel0`'s properties.
- The BCA generator: the deployment masks (none in Thrones' kit: their file name is a runtime string, `deployment_mask.png` assumed), the
  y of its entities (the terrain's height), its errors file (none written for the oracle).
