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

## Blend map

`blend_map.tif` (16 weights per pixel, channel 4 g + v = texture group g, variation v) comes back from BOB's reduction of it
(docs/re/terrain.md): `blend.dds` holds each group's weight, the sum of its four variations (not normalised to unit length; the kit's
western_city_b/small agrees to 0.68 / 255, the DXT5 error), and `index.dds` the variation position t = (w1 + 2 w2 + 3 w3) / sum / 3. A
group's weight goes back to variations floor(3 t) and the next, split by the fraction; BOB's reduction of the result gives the same two
maps. What is lost: the variations a group mixed beyond two (the dominant variation agrees with the kit's on 71.5 % of western_city_b's
pixels, the group weights to 0.59 / 255).

## Ground types

BOB computes `ground_types.dds` from the blend map, the entry's texture group names and the colours painted in `ground_type_map.png`
(docs/re/terrain.md). The decompiled PNG paints, in TEd's colours, every pixel whose binary ground type is not the strongest group's of
the decoded `blend.dds`: the painted water and roads, and the near ties the DXT5 error turned. BOB's rule on the rebuilt blend map and
PNG gives the binary's ground types back on 99.99 % of the pixels (`tile_decompile_check`). An entry made up for the tile gets its
texture group names from the ground types (per group, the type it carries most, as forest / grass / mud / sand / rock / urban).

## Rotations

- **Buildings**: the quaternion as TEd's euler, R = Rz(c) Ry(a) Rx(-b) (`wce_tile::world_transform`).
- **Props** (`prop_marker.markers`, BMD `prop_list`): row k of the matrix is s_k (F R^T F)_k with F = diag(1, 1, -1) and s the half
  scale: for a decal its magnitude (its signed scale is the record's `scale_x/y/z`, TEd's z usually negative; in a version 5 file the
  fields are magnitudes and the rows carry the sign: a z that makes the rotation a mirror is negative); for a simple mesh f_k s_k with f = F,
  s its rows' lengths, positive unless that makes the rotation a mirror, then the axis that leaves the model upright is negative
  (osgiliath). A model whose record has `unknown_69` is named `water_plane` (TEd's flag is the name), else `Object`.
- **Euler angles** from a rotation: a by atan2 (asin loses digits near 90 degrees), of the two angle sets the one with less pitch and roll
  (TEd's (a, 0, 0) for a turn past 90 degrees), and near gimbal lock (cos a < 0.01) refined against all nine elements (Gauss-Newton).
- **Particle emitters**: the matrix transposed, not mirrored; the scale doubled.
- Decals in a group with a pitch: the exporter seems to apply the decal's own small yaw before the group's pitch (up to 0.003 off TEd's
  composition).

## Buildings

- `destruction_level` from `starting_damage`, the inverse of TEd's export (`Ted::BMDExporter::export_`, {level, min 0, max levels - 1}):
  the levels of a building are the most destructs of its pieces in `rigidmodels/buildings/<key>/<key>_tech.cs2.parsed`
  (`destruction_levels`; roman_fort_straight: 3, 2, 2 -> 3). Without the building's `destruct_thresholds` (`battlefield_buildings`)
  damage = level / max, past 1 for a level past the model's (eastern_small_domestic_1, 2 levels, has buildings at level 2: 2.0); with
  them level 0 is 0, level max 1 and any other its threshold (index level - 1; roman_fort_straight [0.8]: level 1 is 0.8). The caller
  gives the lookups (`Context::building_levels`, `building_thresholds`); without them a damaged building gets 1.
- `weak_wall`, `ai_breachable`, `dockable` are written per `battlefield_buildings` category (`Context::building_category`).
- Lite building zones: a building of the main `building_list` is `lite` exactly when its position is inside a LiteBuildingZone (every
  paired building of eastern_city_b/small and nordic_city_b/medium). The zones are rebuilt from the flags: a quadtree over the lite
  buildings (squares without other buildings covered whole, mixed ones split down to 1/4 unit), touching rectangles merged; every
  building is checked to keep its flag (4,722 zones on 102 tiles where the kit has 140).

## Vegetation

The trees of one tree list (`Context::tree_list`; default `outfield_tents.tree_list.bin`, then `default.tree_list.bin`) become explicit
`VEGETATION_INSTANCE`s in a "Decompiled vegetation" layer, in the buildings' frame: the seeds are not in the binary, so regenerating would
move them. A tree whose list flag is 0 gets `surface_mode` MSM_LANDSCAPE (BOB's flag is MSM_FREEFORM). The layer also holds a
`VegetationExclusionPolygonInstance` over the whole tile (a metre past each edge): BOB keeps only the static trees inside an exclusion
polygon and generates nothing there, so an edited project exports exactly its instances. The tree list's `id`s are rpfm's (random), not
data.

Every vegetation file (tree, grass and procedural BMD files, the older lists, the per-climate emitter markers) is carried in
`decompiled_vegetation/`, and BOB's terrain files in `decompiled_terrain/`, with the fingerprints of what they come from: the compile
writes them back while the project is unchanged (docs/formats/tile_compile.md, "Carried outputs").

## Logic

- Outlines (go / no-go), zones, deployment areas (clipped to the playable area by the export), AI hints (the u32 is an index into
  TEd's 41-entry `AI_HINT_TYPE` table; a closed hint repeats its first point), separators, the siege graph (node keys 21 / 22 / 26 / 30:
  area / intersection / wall area / entry; edge keys 23 / 27 / else: street / area connection / wall), civilian deployment zones.
- A polygon's position is the centre of its points' bounding box, its `area` the shoelace area (every class but the AI hints, open
  ones too).
- **Capture locations**: the escalation layers' from their BMDs, the normal layers' from `capture_location_list.xml` (`position_x/y` the
  flag, `capture_point_list` the points `x,y;x,y...`, `flag_facing` a vector). A circle (a radius, one point): the instance at the
  centre, the flag at `flag_position`, the boundary TEd's 20-point circle of twice the radius. Otherwise a polygon through the points,
  the instance at the flag. `flag_facing` = atan2(-y, x) mod 2 pi. Each is named `capture_point_<k>` (its place in its list), and the
  buildings it links (`building_list`, a BMD's `building_links`) get that name as their `capture_location`.
- **Escalation layers** (`bmd_data_escalation<n>.bin`, a layer `Escalation <n>` of `LayerExportType_Escalation<n>`, made even when the
  file is empty: TEd writes one per layer): buildings, props (a model with `allow_in_outfield` is a `water_plane`), particle emitters,
  capture locations, no-go regions, zones, civilian deployment zones. The props and emitters come grouped by key in `MARKER_TABLE`
  order: the groups are placed in an insertion order that gives that order back (docs/formats/tile_compile.md, "Insertion order"), the
  records' order kept within a group.
- An empty `civilian_deployment.bin` is carried along (the compiler writes the file when there are zones), and so are
  `vegetation_exclusion.xml` (docs/formats/tile_compile.md), `sound_emitter_list.xml`, `tree_random_seed.xml` and
  `loading_screen_radar.jpg` (a few tiles; nothing in TEd's project or BOB makes them).
- A deployment area's `facing_direction` is the file's `radians` - 1.570796, plus 6.283185 below 0 (the inverse of the export).

## Not in the binary

- Vegetation exclusion polygons: applied to the tree lists by BOB. Four tiles ship a `vegetation_exclusion.xml` bounding box that matches
  no kit polygon.
- River splines: the river tiles ship the water surface (`hf_water_map.data`) only.
- Civilian shelters: a `CivilianShelter` per record (target and position at y 0, the kit's colours), its capacity as
  `max_shelter_capacity` unless u32::MAX (an attribute the kit's TEd does not know; the compiler reads it). An empty file is carried.

## The entry

The tile database row of the location (a mod's database before the game's), as BOB writes the entry, with the variation's `min_height`
/ `scale` set to the CHMF's min / range (saving the project as an exported tile puts the database's values back while the terrain is
unchanged, docs/formats/tile_workflow.md). A location no database has (mods' `assembly_kit` battles, 16 tiles of the game's packs) gets
an entry made from the binary: the size from the CHMF (samples x spacing / 128), one variation, the other fields their defaults.
