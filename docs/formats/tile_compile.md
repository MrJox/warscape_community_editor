# Compiling a TEd project into an exported tile

`wce_tile_io::attila::compile` turns the model of a TEd project (`attila.ted_project`) into the documents of an exported tile
(`attila.tile_binary`), which that format's writer turns into bytes. Plan and status: docs/plans/tile_roundtrip.md, Phase 2 step 4. It
follows TEd's own export (Ghidra, `TEd.AssemblyKit.exe`): stage A, the logic TEd writes, comes first; BOB's stages follow.

Check: `tile_decompile_check` (gates `tile_decompile`, `tile_decompile_all`) decompiles every tile folder of the packs, writes and reads
the project back, compiles it, and compares each document with the binary's, byte for byte and field by field.

## Exactness

The shipped binaries were exported from TEd's floats in memory, finer than tile.ted's `%.6f` text: some of their values come from no
text (an exact 180 degree turn has no euler whose float sine is 0). The check accepts what the text cannot carry (user, 2026-10-22): a
position axis within 4 ulps or 1e-6, a quaternion component within 1e-5; everything else must be identical. Known exceptions, each counted
by name: buildings whose binary quaternion is not unit (TEd's matrix was not a pure rotation; 6 in 3 TDD tiles), starting damages no level
gives with the current building data (an older model's levels; 49), and legacy lists the project carries as files.

## Arithmetic

- A value is read from its text as `sscanf` does (f32). World matrices are `QTU::TedEntity::world_transform`'s
  (`wce_tile::world_transform::ted_world_matrix_f32`: the euler to rotation `FUN_100af1e0`, the parent's times the local, in f32).
- The x87 runs at 53-bit precision: TEd's intermediate results are doubles, rounded to f32 where the code stores a float.

## Building lists (`Ted::BMDExporter::export_`)

- `building_list` from the buildings of the normal layers, `building_list_<layer name in lower case>` from each separate layer
  (`LayerExportType_Separate`; a list even when the layer is empty); off and escalation layers are not in the lists. The buildings in the
  scene's order; record version 12.
- Position (`Ted::transform_to_game_tile_space`): x = world x * 0.5, y = world y, z = extent - world z * 0.5 (extent = the entry's height
  cells * 128).
- Orientation: the world matrix's quaternion, `FUN_00474030` (Shoemake's method on TEd's `MATRIX_44`, whose element (r, c) is at r + 4 c:
  the transpose of the row-major matrix of `ted_world_matrix_f32`).
- Starting damage from `destruction_level` ({level, min 0, max levels - 1}; levels = the most destructs over the building's pieces in its
  `.cs2.parsed`): without the building's `destruct_thresholds` (`battlefield_buildings`) level / max; with them 0, 1 at max, else
  threshold[level - 1].
- `on_fire` = `burn`; `weak_point`, `ai_breachable`, `dockable`, `indestructible`, `toggleable` from the instance (defaults false, true,
  true, false, false); `start_disabled` false, `position_type` 2 (`lf_relative`), scale 1.
- `lite`: the building's position inside a `LiteBuildingZone` of a normal layer (the main list only).

## Markers

The instances of the normal layers in the scene's order, one record each, grouped by key in `MARKER_TABLE` order (docs/re/ted_export.md:
a string key hashed, a number key its own hash; a hash table from 13 slots growing to 2 n + 1). TEd writes no file without records (the
empty ones of the legacy campaign river tiles are carried by the project). The version to write is an option (the check passes the
binary's; TEd's current ones: props 9, emitters 9, lights 3, the rest 1).

| File | From | Record |
| --- | --- | --- |
| `prop_marker.markers` | `ProjectedDecalInstance`, `SIMPLE_MESH_INSTANCE`; key the model | rows: the world rotation in the binary's frame (F R^T F, F = diag(1, 1, -1)) times half the scale (a decal: its magnitude, version 5 signed; a model: F times it signed); translation (x / 2, y / 2, extent - z / 2); `is_decal`; `unknown_69` = a model named `water_plane`; seasons (decals 15), the visibility flags (decals all on); a decal's parallax, tiled and half scale (version 5: magnitudes) |
| `particle_emitter_marker.markers` | `VFXInstance`; key `vfx` | the rotation transposed, translation (x / 2, y, extent - z / 2); 6 zero bytes; autoplay, use_in_outfield, visibility, half the scale, seasons |
| `point_light_marker.markers` | `POINT_LIGHT_INSTANCE`; key the name | (x / 2, y, extent - z / 2), radius, colour, colour scale, `unknown_32` 1, animation, colour min, random offset, falloff |
| `ai_hint_polyline_marker.markers` | `AI_HINT_POLYGON_INSTANCE`; key the hint type's number | the polygon's world points (x / 2, extent - z / 2); a closed one repeats its first point |
| `ai_separator_marker.markers` | `AISeparatorInstance`; key the type (`PALT_NONE` 0, `PALT_GENERIC` 1, `PALT_FORT` 2, by name) | the points |
| `ai_hint_polyline_list_marker.markers` | `SiegeAINodeInstance` / `SiegeAIEdgeInstance` (area 21, intersection 22, wall area 26, entry 30, street 23, area connection 27, wall 28, palisade 40) | a polyline per `SIEGE_AI_POLYGON_INSTANCE` under it |

## definition.xml

`BATTLE_MAP_DEFINITION`: `bmd_type` 1, `terrain_type` 3, `base_terrain_width` / `height` = (cells + 2) * 256 (25.6 for a campaign tile,
f32, `%.6f`), `disable_outlines` false, and `climate`: TEd's preview climate when it exported, not in the project; the decompiler carries
the shipped file and the compiler takes the climate from it, else `default`. TEd's BMD XML layout: single quotes, two spaces between
attributes, a space before `/>`, CRLF.

## The logic XML files

TEd's layouts per file (CRLF, single quotes; the outline files and zones without indentation, deployment with tabs); numbers `%.6f` of the
f32, the capture point lists trimmed (`512.19342`), the facing with one decimal at least (`1.0,-0.0`).

- `go_outlines.xml`, `non_terrain_outlines.xml`: an `OUTLINE` per `GO_REGION_POLYGON_INSTANCE` / `NOGO_REGION_POLYGON_INSTANCE`, its
  world points (x / 2, extent - z / 2) on one line. Written only when there are some (no tile ships an empty one).
- `zones.xml`: per `ZONE_POLYGON_INSTANCE` a `TRANSFORMED_BATTLEFIELD_ZONE_TEMPLATE`: the template (`zone_name` ZONE_POLYGON_INSTANCE,
  the points, an empty `ENTITY_FORMATION_TEMPLATE`) and an identity `Transformation` (all 1,909 of the packs are so).
- `deployment_areas.xml`: per alliance (by id) its `deployment_area`s (the `DeploymentZonePolygonInstanceGroup`s: id, category), a
  `boundary` per polygon under it (type, points) and the `orientation` (the facing direction). TEd clips the boundaries to the playable
  area; a decompiled project holds clipped ones, the compiler does not clip yet.
- `capture_location_list.xml`: per `CaptureLocationInstance` of the normal layers (the escalation layers' are in their BMDs): radius
  (half TEd's circle, 0 for a polygon), the flag's position, players, type (`CAPTURE_LOCATION_MAJOR` / `MINOR`), the point list (a
  circle's centre or the polygon's points), the key, the facing as (cos f, -sin f), and `building_list`: the main list's `fort_tower` and
  `gate` buildings within 20 m of the boundary, `;` separated. **Open**: TEd's linking rule. Its export (`FUN_00500f60`, the instance
  copied by `FUN_00437190`) sets no links, and neither the instance nor `add_capture_location_set` holds any: they come from code not
  found yet (likely the buildings' footprints at their destruction level: escalation 3 of western_city_b/small links a gate without its
  two destroyed towers but keeps two other destroyed ones). The 20 m rule is an approximation; the check compares the links apart.

## Escalation BMDs (`bmd_data_escalation<n>.bin` and `.xml`)

A file per `LayerExportType_Escalation<n>` layer (n 1 to 3), written even when the layer is empty: rpfm_lib's `Bmd` (version 6 unless
the binary's is passed: 4 and 5 drop the civilian lists), built through serde_json and checked by deserialising it.

- Buildings: the building list's records as `Building` version 3 (`BBPT_LF_RELATIVE`, the transform zero, `lite` false, the other
  properties false).
- Props (`PropMarker`, version 2, or 1 for older files; the version is an option like the markers'), particle emitters
  (`ParticleEmitterMarker`): the markers' records of version 9 (the same frames). Grouped by key in `MARKER_TABLE` order, like a markers
  file, but a BMD keeps no record order: the decompiler places the groups in an insertion order that gives the file's order back (below).
  A prop's `allow_in_outfield` is the water-plane flag (the markers' `unknown_69`: TEd's flags word, bit 3); an emitter's snow and
  destruction flags are the instance's `visible_*` ones.
- Capture locations of the layer (one set), linking into the layer's own buildings; no-go regions (`non_terrain_outlines`); zones
  (`zones_template_list`: `ZONE_POLYGON_INSTANCE`, the points, an empty formation template, an identity transform); civilian deployment
  zones.
- TEd's garbage: the PROP serialiser (`FUN_004d38e0` binary, `FUN_004d4a10` XML) reads `snow` and `destruction` into locals it never
  sets, and a model's decal fields (`FUN_00502c60`) are members its export never sets (`17479188480.0` and the like in the XML). They are
  written as 0; the check counts the bytes that differ by name.
- The XML twin (`BATTLE_MAP_DEFINITION_DATA`): the same data in TEd's XML names (`PROP` > `marker` with `decal_params_*`,
  `PARTICLE_EMITTER` > `marker` version 4, `BUILDING` with the properties as attributes, `CAPTURE_LOCATION` with `building_indices`
  repeating `index`), `%f` floats, tabs, single quotes, CRLF. Every vanilla escalation BMD has one; TDD's packs drop all BMD twins.

**Insertion order** (`marker_insertion_order`). Linear probing fills the same slots whatever the order, so the file's group order gives
each key its final slot; a key must be inserted after the keys between its home slot and its own. The table's growth re-inserts the
keys it held in their slot order, so that order is the full smaller table's, which must be reachable in turn: the search draws orders
level by level (favouring keys near their home in the smaller table), with a simulated annealing over insertion orders where that finds
none. All 759 prop and emitter lists of the packs' escalation BMDs (the Python prototype, whose dump truncates 2 lists' long keys) and
the kit's own order for western_city_b and eastern_city_b reproduce.

## civilian_deployment.bin

rpfm_lib's `CivilianDeploymentFile` (version 1): the normal layers' `CivilianDeploymentZone`s (kind = `civilian_type`, size =
`civilian_amount`, the polygon's points). Written when there are zones; 154 tiles ship it empty and 1,245 not at all, so the decompiler
carries an empty one. `civilian_shelter.bin` is not TEd's: its capacities are not in tile.ted (`CivilianShelter`: a position and a
target), so it belongs to BOB's stage.

Measured over the packs (1,654 tile folders, 4,063,006 buildings): keys and every flag identical; positions identical or within the
text's precision; quaternions within 1e-5 but the 6 not unit; every `building_list_<layer>` byte-identical.
