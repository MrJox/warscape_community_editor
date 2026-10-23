# Compiling a TEd project into an exported tile

`wce_tile_io::attila::compile` turns the model of a TEd project (`attila.ted_project`) into the documents of an exported tile
(`attila.tile_binary`), which that format's writer turns into bytes. Plan and status: docs/plans/tile_roundtrip.md, Phase 2 step 4. It
follows TEd's own export (Ghidra, `TEd.AssemblyKit.exe`): stage A, the logic TEd writes, comes first; BOB's stages follow.

Check: `tile_decompile_check` (gates `tile_decompile`, `tile_decompile_all`) decompiles every tile folder of the packs, writes and reads
the project back, compiles it, and compares each document with the binary's, byte for byte and field by field. With `WCE_COMPILE_KIT=1`
it compiles the kit's raw tile instead (TEd's own project): the direct test of the export, where the kit is the shipped version (its
nested groups add their text's rounding, up to 6.5e-5 on a building's position).

## Exactness

The shipped binaries were exported from TEd's floats in memory, finer than tile.ted's `%.6f` text: some of their values come from no
text (an exact 180 degree turn has no euler whose float sine is 0). The check accepts what the text cannot carry (user, 2026-10-22): a
position axis within 4 ulps or 1e-6, a quaternion component within 1e-5; everything else must be identical. Known exceptions, each counted
by name: buildings whose binary quaternion is not unit (TEd's matrix was not a pure rotation; 6 in 3 TDD tiles), starting damages no level
gives with the current building data (an older model's levels; 49), and legacy lists the project carries as files.

## Layers

An instance exports with its layer's `export_type`; a `LayerExportType_Normal` layer inside another layer exports with that one (the
kit's escalation layers hold their content in normal sub-layers: "Gates and Towers", "Capture_Points").

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
  `boundary` per polygon under it (type, points) and the `orientation`: the facing direction + 1.570796, less 6.283185 above pi (f32,
  TEd's literals: the kit's 0, 1.570796, 4.712389 are the packs' 1.570796, 3.141592, 0). The boundaries are clipped to the playable area,
  the tile less 32.5 m on every side (all 777 files of the packs keep inside it), by Sutherland-Hodgman against y >= 32.5, x <= width -
  32.5, y <= height - 32.5, x >= 32.5 in that order, an edge from the previous point to the current: the kit's tiles give the packs'
  files, 6 of 9 byte for byte (the others 2 ulps off on a cut point).
- `capture_location_list.xml`: per `CaptureLocationInstance` of the normal layers (the escalation layers' are in their BMDs): radius
  (half TEd's circle, 0 for a polygon), the flag's position, players, type (`CAPTURE_LOCATION_MAJOR` / `MINOR`), the point list (a
  circle's centre or the polygon's points), the key, the facing as (cos f, -sin f), and `building_list`: the indices (in the main list,
  `;` separated) of the buildings whose `capture_location` property is the capture location's `name` (TEd's towers and gates name the
  capture location they belong to). The kit's tiles give the binaries' links this way, the escalation BMDs' included. A circle's point is
  its `CaptureLocationPolygonInstance`'s position (the kit's sit off their capture location: (-7, 0, -3) and the like); every number of
  the point list keeps a decimal (`492.5,576.0`).
- `vegetation_exclusion.xml` (4 encampments; TEd writes it when the location has `/encampments/`, `FUN_005002d0`): the box of the
  buildings' bounds as the editor has them loaded (each building's cached world box, `FUN_00413dc0`, to tile space and padded,
  `FUN_005029d0`, then joined, `FUN_00502a50`). The collision and render models' boxes do not give the shipped ones (the editor's
  render state and the pad are not known), so the decompiler carries the file.

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
- Capture locations of the layer (one set), linking (by `capture_location`) into the layer's own buildings; no-go regions (`non_terrain_outlines`); zones
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

## Stage B: BOB's Tile processor (`attila::bob_tile`, `attila::ati_dxt`)

BOB's own intermediates (`final_heights.dds`, `final_alpha.dds`, `tile_normal.tga`) are not inputs: BOB rebuilds them from TEd's raw files
(deleted, a fresh run writes them again, every output the same). Check: `bob_tile_check <raw tile> <BOB's working tile>` against a fresh
run (`~/wce_re/scripts/bob_tile.sh Tile <tile>`).

- **Blend and index maps** (`TOOLDATABUILDER::convert_blend_map`, `FUN_10027430`): the 16 weights as `IMAGE::convert` makes them
  (x the f32 of 1 / 65535: 514 / 65535 x 255 truncates to 1); per group the sum of its variations x 255 truncated (blend), and
  (0 w0 + 0.33333 w1 + 0.66666 w2 + w3) / sum x 255 truncated (index); group red, green, blue, alpha to R, G, B, A.
- **Ground types** (the same function): per pixel of the tile without its 128-pixel border the strongest group of the blend map (ties to
  red, green, blue, alpha) as its `ground_type_from_texture_group`, a colour of `ground_type_map.png`'s palette over it. Written by
  `IMAGE::save_raw_to_file`: L8, no mips.
- **Normal map** (`FUN_10012a00` -> `FUN_10021300`, `calculate_normal`; written as `tile_normal.tga`): the heights in metres (the
  16-bit samples x the f32 of 1 / 65535, x scale + min height), per pixel the Sobel gradients (x: 1 0 -1 / 2 0 -2 / 1 0 -1, y its
  transpose, samples clamped at the edges) / 9, (gx, gy, 1 / normal strength) normalised, each component (n + 1) x 0.5 x 255 truncated
  (R x, G y, B z, A 255). All in f32: the x87 runs at 24-bit precision here (a Direct3D device's FPU setting, most likely), unlike the
  blend compression (one pixel of terrain_rolling_hills_med/2x2/ae tells them apart).
- **`normal.dds`** (`FUN_10087420`): each pixel to (B 0, G y, R 255, A x), DXT5 like the others; none for a flat map (every pixel 127,
  127, 255: the variation's `normal_common` becomes `terrain/textures/common/flat_normal.dds`).
- **Final heights** (`final_heights.dds`, R32F, `cells x 128 + 1` square): the height map without its one-cell border, in metres as
  above, times the tile's influence (`final_alpha.dds`), TEd's `WS_SCENE_NODE_TERRAIN_EDITOR_V2::build_hf_influence_map`
  (Warscape.AssemblyKit.dll). Per cell, each side is 1 when a cell of the tile lies past it, else 0; a corner is 1 when its cell exists
  or its two sides differ. Per sample, `lerp(i, a, b)`: in the first half of the cell (`i < (k + 1) / 2`) from side a to 1,
  `t = clamp(i - 2, 0, 5000) / ((k + 0.5) / 2)`, `(1 - a) t + a`; in the second half the same from side b with `k - i`. The value is
  `v = lerp_y x lerp_x x lerp_y(lerp_x(corners above), lerp_x(corners below)) x 2.1`, the influence `(1 - 1 / powf(e, v^2)) / 0.9878448`
  (e as the f32 2.7182817). Byte for byte on terrain_rolling_hills_med/2x2/ae (tile links, which open a side to a neighbour, not
  ported).
- **`hf_height_map.data`** (`FUN_10089b80`): not the final heights but the terrain mesh (below) rasterised ("Rasterize final
  heightmap", `FUN_1001bf20`), the buildings' height meshes raised into it (`FUN_1008a700` -> `WARSCAPE::rasterise_max_heights`;
  "Height map mesh file ... is missing" when a `.cs2` lacks), written by `COMPRESSED_HEIGHT_MAP::convert` (Warscape.AssemblyKit.dll
  0x104ac240): CHMF 5, the field's min and max in metres, each sample `(h - min) / (max - min) x 65535` truncated (SSE), one region,
  the encoder of `wce_formats::chmf`. The rasteriser (`terrain_mesh::rasterise`): a field of zeros the final heights' size; per
  triangle (a, b, c) in order, the samples of its box in whole metres (truncated, widened by one below) whose barycentric weights are
  all within [-0.0001, 1.0001] get `(c.h w + b.h v) + a.h u` (`v = (d20 d11 - d21 d01) / det`, `w = (d21 d00 - d20 d01) / det`,
  `u = (1 - v) - w`, all f32 as BOB's SSE orders them) at row `size - 1 - z`. Byte for byte on terrain_rolling_hills_med/2x2/ae
  (no building stamps; BOB's min of -7e-14 included) and farm_western/1x1/aa (31 stamps).
- **Building stamps** (BOB_Tile `FUN_1000bb10` builds the list, ToolDataBuilderDll `FUN_1008a700` stamps it): every
  `BUILDING_INSTANCE` of tile.ted (document order, any layer) whose `battlefield_buildings` row has `height_map_mesh` 1 (23 keys:
  the global grounds, ruins, display walls, a bridge, a swamp), its mesh from `models_building.cs2_file` under the kit's `raw_data`
  (any case). The matrix is TEd's world matrix (`QTU::TedEntity::world_transform`, f32) with its x and z rows halved. The mesh
  (`FUN_1008a2f0`): the CS2's meshes with a string attribute starting `height_map_mesh` (`class_rigidINFO height_map_mesh01`), moved
  by their scene node (identity in the vanilla grounds; the ruins' nodes are translated), renumbered into one list
  (`wce_formats::cas2`, `terrain_mesh::HeightMesh`). `WARSCAPE::rasterise_max_heights` (`terrain_mesh::rasterise_max_heights`, SSE
  f32): per triangle moved by the matrix, the samples of its truncated box widened by one (clamped to the field less its last row
  and column; field row z, not flipped) inside it by the crossing rule over (a c), (b a), (c b) take the height interpolated by the
  sub-triangle areas (`|cross| / 2 / area`) when it is not below theirs.
- **Terrain mesh** (`TILE_RAW_DATA::build_base_mesh`, `FUN_10019750`; `attila::terrain_mesh`): the surface of `mesh.rigid_model_v2`
  and the CHMF's source.
  - A vertex per final-heights sample: x, z in metres from the near corner, the height of the field's row `size - 1 - z`, the normal
    `calculate_normal` there (the normal map's Sobel, f32). BOB's vertex is also written with the final alpha (the RMV2's w).
  - A flag per vertex: 0 (locked) near the edge, where the cell of x - 1, x + 1, z - 1 or z + 1 (rounded down) is off the tile: the
    border, and the column and row next to the far edges (BOB's asymmetry); and every 16th sample both ways; and where
    `protection_map.dds` (TEd's painted R32F, `cells x 128` square) is above 0.5, vertex (x, z) reading its row z - 1. 2 (free)
    elsewhere. (3: the fringe's top row, which only moves along the edge; 1 and 4 are campaign flags.)
  - The grid: per square, row by row, (x, z) (x, z + 1) (x + 1, z) and (x, z + 1) (x + 1, z + 1) (x + 1, z).
  - `TRIANGLE_MERGER` (the tweaker `terrain_enable_triangle_merge`, default on; `FUN_10003140`): passes 1 ..= 50 (max(trunc(120 /
    6), 50)) of a longest edge of pass x 2.4 m; after 4 passes in a row without a change it skips 10. Each pass rebuilds per vertex its
    triangles, then tries every vertex in index order: a free vertex may go when each neighbour's normal is within the angle
    (|n . n'| >= cos of `_settings.xml`'s `triangle_decimation_angle_factors0`, 20 degrees: degrees x 3.1415925 x f32(1 / 180),
    `fcos`) and its height within 3 m; its targets are the neighbours (per triangle, in corner order, repeats kept) for which every
    remapped triangle keeps its locked corners (a triangle collapsing onto a free target must have no corner below 2), is not thin
    (the sine of its angle at the first corner, in the ground plane, >= 0.1), has no edge over the pass's length and is not turned
    over (`(c.z - a.z)(b.x - a.x) - (b.z - a.z)(c.x - a.x) <= 0`); the first target not free wins, else the nearest (MSVC's
    insertion sort, stable; an introsort past 32 candidates, not ported). The collapse appends the vertex's triangles to the target's,
    renames it, zeroes the degenerate ones.
  - The fringe (`FUN_10017120`, `terrain_mesh::add_fringe`; before the merger): per edge (z = 0, z = max, x = 0, x = max) the edge's
    vertices again 10 m lower (BOB's vertex +0x78 = 1), then per square two upright triangles into the grid list (top t, bottom b:
    z = 0 (t i, t i+1, b i) (t i+1, b i+1, b i); z = max (t i+1, t i, b i) (t i+1, b i, b i+1); x = 0 (t j, b j, t j+1)
    (t j+1, b j, b j+1); x = max (t j, t j+1, b j) (t j+1, b j+1, b j)). The tops are the surface's vertices (locked); a bottom's
    flag is 1 at the corners and where x and z are multiples of 16, else 4 (moves only into a 4 or a 1): the bottom row decimates
    along the edge. The thin test measures upright triangles in their own plane, the height step does not apply to flags 1 and 4.
  - `mesh.rigid_model_v2` (`terrain_mesh::rmv2`): the merged list (surface then fringe, in order) cut into blocks by the
    `MESH_SPLITTER` (`FUN_10004970`): a block takes triangles until it holds 65,000 vertices or more, and the triangle that filled
    it starts the next block again (its loop only advances while there is room); a block's vertices numbered by first use (the
    vertex list cleaner), each f16 x, height, z, alpha (rounded to nearest; the alpha of the influence's row z, unflipped, where
    the height is row `size - 1 - z`: BOB's), u16 indices. Header: `RMV2` 6, 1 LOD, zero skeleton name; the LOD's block count,
    vertex and index bytes, offset 0xa0, distance 70; per block its type 0x61, size, header 0xa8, vertex count, index offset and
    count, the whole mesh's f32 bounds, `rigid_default` (BOB leaves the rest of its 32 bytes uninitialised), `TerrainBase<k>`, 32
    zeros, the tile's metres x and z, samples per row, 1, 16, the block's count of surface indices; then its vertices, indices.
  - `outfield_mesh.rigid_model_v2`: the same from the same final heights with a grid of 16 (near-edge band 16, the "every n-th
    sample" lock and the fringe's kept bottoms at multiples of 2048), built first (`process_tile_internal`).
  - Exact against fresh BOB, the uninitialised name tails masked: terrain_rolling_hills_med/1x1/aa and /2x2/ae, farm_western/1x1/aa
    (all three terrain files) and western_city_b/small (both meshes, 8 blocks, 9.4 MB; BOB writes no CHMF there: two display walls'
    meshes are missing from the kit). Entry point `terrain_mesh::terrain_files` (with `building_stamps` and `decimation_angle` from
    the kit); check `terrain_mesh_check <raw tile> <BOB's working tile> <kit raw_data>`.
- **`tile.agf`** (BOB's "Create tile agf"; `bob_tile::tile_agf`): `##<folder>`; `#all` (entities and files, sorted together);
  `#entities` only when the tile has buildings (`battle/buildings/<key>` per building key of tile.ted, sorted); `#files` (the nine
  outputs the Tile processor declares, BOB_Tile `FUN_100095b0`, written or not: blend, ground_types, hf_height_map, hf_water_map,
  index, mesh, normal, outfield_mesh, terrain_outlines; and `mesh<x>x<y>.rigid_model_v2` per cell but 0x0, sorted); each section
  closed by a blank line, `#files` by two; then per file `##<file>:<file>` and two blank lines. The folder is the entry's location
  and the variation, with `/`. Byte for byte on 4 tiles.
- **`terrain_outlines.xml`** (`FUN_10088790` -> `FUN_100877d0` -> EmpireUtility `no_go_outlines_from_heights`;
  `attila::terrain_outlines`): not `nogo.png` (its writer, `FUN_1002a1a0`, is another action), the steep areas of the final heights:
  - No-go samples (`HEIGHTFIELD_OUTLINE_DATA_PROVIDER`, `FUN_10037df0` / `FUN_10a82a10`): the normal by central differences (the
    sample itself past the edges) at 2 units per sample (the tile database's `unit_scale`, whatever the sample density), no-go where
    its upright part is below sin(90 - 30) degrees (`MAXIMUM_PASSABLE_SLOPE_ANGLE_DEGREES`, a tweaker of default 30; `fsin`), f32.
  - Blurred (`BLUR_OUTLINE_DATA_PROVIDER::blur(3, 0.1)`): a separable triangle kernel (3 - |i|) / 3 x 1 / 3 across then down,
    clamped, set above 0.1.
  - Traced (`OUTLINE_CALCULATOR::compute_outlines`): row by row, an unmarked set cell starting an outline (2 x 2 case 1, or 9 with
    its upper left cell traced) not inside an outline found before is walked by the marching squares table (EmpireUtility
    0x10df6980: move, three edge points, the cells marked with the outline's number; saddles 6 and 9 by the previous step), the
    points merged when in line (`add_point`), until it closes or w h / 4 steps; kept when wider or taller than 6 cells.
  - Simplified (`OUTLINE_SIMPLIFIER::simplify`, in units of 2 per cell): 3 passes at error 0.99 lookahead 10; 1 at 3.0 growing the
    area only; 1 at 1.0. Per outline (not left unchanged by a pass of the same call), from each kept point the target among the next
    `lookahead` (from two ahead, the last point never) whose skipped points lie within the error of the shortcut (point to segment)
    and on an allowed side, with the least error (ties to the later), taken when `is_valid_outline`: the shortcut crosses no kept
    segment, no edge of the outline from the target on, no edge of an outline whose box it touches (Cyrus-Beck), crossings measured
    along the edge (`LINE_2::intersects` with the edge as the line: an end of the edge is no crossing).
  - Written (`EMPIRE_OUTLINE_LIST::save_to_xml`): `<OUTLINE_LIST>\n`, per outline `<OUTLINE>\n`, `<position x='%f' y='%f'/>` per point
    (x, rows - y, in samples), `</OUTLINE>\n`; `</OUTLINE_LIST>\n`.
  - Byte for byte on dense_forest_rugged/8x8/ac (28 outlines) and the gentle tiles (empty). Not handled yet: the tile's go outlines
    (rasterised out of the no-go field first, `FUN_100c0eb0`) and masked tiles (an outline must lie in valid subtiles; the
    atlantic_mountains/small_round/aa oracle keeps the one of its two outlines that does).
- `ground_type_map.PNG` (upper case) is BOB's too (Windows); the project carries such a file as is, stage B reads it.
- **DDS** (`write_dds_ATI_compressor`): levels down to log2 of the larger side truncated (8 for 384), each the 2 x 2 box of the one above
  ((sum + 2) >> 2, the last row and column clamped); header flags 0xa1007, the first level's size, caps 0x401008.
- **ATI's DXT5 compressor** (`ATI_TC_ConvertTexture`, statically linked; `ati_dxt`): the 2008 ATI_Compress, ported from AMD
  Compressonator's legacy codec and corrected where the 2008 binary differs:
  - the colour part runs on the x87 under `/fp:precise` (each expression in double, rounded to float where stored);
  - its ramp searches are SSE2 (`FUN_101e5940`, `FUN_101e54a0`): the nearest ramp point by `cvtps2dq` (ties to even) clamped to the
    ends, the errors in four lanes summed (l0 + l2) + (l1 + l3), no early exit; the colour index vector the same way;
  - the alpha part is single float operations (the x87's stored results are the f32 ones);
  - a float endpoint becomes a byte through int (`(BYTE)256.0f` = 0: an all-255 alpha block is written 0, 255).
- Measured: blend, index, normal and ground types byte for byte on farm_western/1x1/aa, terrain_rolling_hills_med/1x1/aa and /2x2/ae
  (40 m of relief), blend, index and ground types on western_city_b/small (BOB stops before its normal map: the kit lacks its buildings'
  height-map meshes), but 2 colour blocks of western_city_b/small's index (of 133,000): the 2008 refinement reads, for the padding of
  a 4-colour group, error and colour slots it never writes (its stack), so a stale NaN can stop a refinement pass in rare blocks.
