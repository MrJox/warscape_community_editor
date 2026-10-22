# Decompiling an exported tile into a TEd project

`wce_tile_io::attila::decompile` turns the model of an exported tile (`attila.tile_binary`, docs/formats/tile_binary.md) into the
model `attila.ted_project` writes (docs/formats/tile_model.md). Plan and status: docs/plans/tile_roundtrip.md, Phase 2 step 3. Check:
`cargo run --release -p wce_tile_io --example tile_decompile_check -- <game folder>` (every tile folder of the packs, against the kit's
raw tile where there is one).

What the binary does not carry is reported per tile (`Losses`): the camera, the vegetation seeds, the layers' names and structure, the
nogo / LF influence maps, the height map's border ring, TEd-only properties, the zones listed below.

## The kit is evidence, not a gate

The kit's raw tiles are not always what the packs were exported from, nor what BOB exported:

- `height_map_0.png` is BOB's input. BOB writes `final_heights.dds` from it (already different: 9 cm of relief exported as 0 on
  terrain_rolling_hills_med/1x1/aa), then the CHMF is `final_heights` plus raised building stamps (fresh BOB, farm_western/1x1/aa: 4 % of
  the samples, up to +1 m, all positive).
- 167 pack tiles ship a flat CHMF (range ~1e-13) and a flat terrain mesh where the kit's tile has relief; rolling_valley tiles ship ~0.18
  times the kit's relief.
- farm_*, town_greek_plains, roads_* and sea_coast differ in decal rotations and heights, some building groups in their simple meshes'
  yaw (a constant per group).

## Frames

L = the tile's extent in z, `height` cells x 128 m. The binary's x, z are metres with z mirrored; TEd's scene is in half metres
horizontally.

| What | Binary | Scene |
| --- | --- | --- |
| buildings, VFX, point lights | (x, y, z) | (2 x, y, 2 (L - z)) |
| decals, simple meshes | matrix translation (x, y, z) | (2 x, 2 y, 2 (L - z)) |
| outlines, polygons, capture locations | (x, z) | (2 x, 2 (L - z)) |

The decompiled numbers are quantised through TEd's formatter (`Num::Ted`, `Fixed6` for the entry), so the project reads back as written.

## Rotations

- **Buildings**: the quaternion as TEd's euler, R = Rz(c) Ry(a) Rx(-b) (`wce_tile::world_transform`).
- **Props** (`prop_marker.markers`, BMD `prop_list`): the matrix rows divided by |scale_k| are A; the rotation is F A^T F with
  F = diag(1, 1, -1). A simple mesh's y and z rows are negated (1,147 paired: 208 agree, the rest differ by a constant yaw per kit building
  group). A decal's scale is twice the record's `scale_x/y/z`, a simple mesh's twice its rows' lengths.
- **Particle emitters**: the matrix transposed, not mirrored; the scale doubled.
- Decals in a group with a pitch: the exporter seems to apply the decal's own small yaw before the group's pitch (up to 0.003 off TEd's
  composition).

## Buildings

- `destruction_level` = round(starting_damage x (levels - 1)); the levels of a building are the fewest destructs of its pieces in
  `rigidmodels/buildings/<key>/<key>_tech.cs2.parsed` (`destruction_levels`; roman_fort_straight: 3, 2, 2 -> 2). The caller gives the
  lookup (`Context::building_levels`); without it a damaged building gets 1.
- `weak_wall`, `ai_breachable`, `dockable` are written per `battlefield_buildings` category (`Context::building_category`).
- Lite building zones: a building of the main `building_list` is `lite` exactly when its position is inside a LiteBuildingZone (every
  paired building of eastern_city_b/small and nordic_city_b/medium). The zones are rebuilt from the flags: a quadtree over the lite
  buildings (squares without other buildings covered whole, mixed ones split down to 1/4 unit), touching rectangles merged; every
  building is checked to keep its flag (4,722 zones on 102 tiles where the kit has 140).

## Vegetation

The trees of one tree list (`Context::tree_list`; default `outfield_tents.tree_list.bin`, then `default.tree_list.bin`) become explicit
`VEGETATION_INSTANCE`s in a "Decompiled vegetation" layer, in the buildings' frame: the seeds are not in the binary, so regenerating would
move them. The tree list's `id`s are rpfm's (random), not data. Grass lists are carried along as files.

## Logic

- Outlines (go / no-go), zones, deployment areas (clipped to the playable area by the export), AI hints (the u32 is an index into
  TEd's 41-entry `AI_HINT_TYPE` table; a closed hint repeats its first point), separators, the siege graph (node keys 21 / 22 / 26 / 30:
  area / intersection / wall area / entry; edge keys 23 / 27 / else: street / area connection / wall), civilian deployment zones.
- A polygon's position is the centre of its points' bounding box, its `area` the shoelace area (every class but the AI hints, open
  ones too).
- **Capture locations**: the escalation layers' from their BMDs, the normal layers' from `capture_location_list.xml` (`position_x/y` the
  flag, `capture_point_list` the points `x,y;x,y...`, `flag_facing` a vector). A circle (a radius, one point): the instance at the
  centre, the flag at `flag_position`, the boundary TEd's 20-point circle of twice the radius. Otherwise a polygon through the points,
  the instance at the flag. `flag_facing` = atan2(-y, x) mod 2 pi.

## Not in the binary

- Vegetation exclusion polygons: applied to the tree lists by BOB. Four tiles ship a `vegetation_exclusion.xml` bounding box that matches
  no kit polygon.
- River splines: the river tiles ship the water surface (`hf_water_map.data`) only.
- Civilian shelters are left out (BOB generates them for tiles whose tile.ted has none).

## The entry

The tile database row of the location (a mod's database before the game's), as BOB writes the entry, with the variation's `min_height`
/ `scale` set to the CHMF's min / range. A location no database has (mods' `assembly_kit` battles, 16 tiles of the game's packs) gets
an entry made from the binary: the size from the CHMF (samples x spacing / 128), one variation, the other fields their defaults.
