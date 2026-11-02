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

## Open

- What `Terry file` writes (the `.terry` -> `locations.blm` / `.mask0` / `.mask1` and the 2 074 deployment files of `terrain.pack`), and
  what `Heights & Normals` writes (`lf_height_map.dds` / `.data`, `lf_sea_height_map.dds`, the 736 normal chunks): BOB on a scratch kit is
  the oracle.
- The falloff models' formulas (`Radius Sin`, `Radius Sin Sin`) and `OpSimpleChannel0`'s properties.
- The BCA generator's algorithm (the `Zonifier`): black box first (run it in Thrones' Terry on a scratch map, diff the `.terry`).
