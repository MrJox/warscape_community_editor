# Battle maps (2026-10-18 / 19)

What surrounds a tile in a battle: the map it is placed in. Goal: show a tile in the context of a kit battle map (the user picks the
map, and where a tile appears more than once, which placement).

## Kit sources (`assembly_kit/raw_data/terrain/battles/<map>/`)

- 69 settlement maps (`settlement_<culture>_<city|port>_<variant>_<size>`) and `main_attila_map` (the campaign's battle map).
- A settlement map: `tile_map.png` (64 x 64 cells, RGB), `climate_map.png` (64 x 64), `lf_heights.tif` (512 x 512, 16 bit: 8 px per
  cell), `lf_sea_heights.tif` (128 x 128), `normal.dds`, and a near empty `<map>.terry`.
- `main_attila_map`: `tile_map.png` 2032 x 1437 cells, `climate_map` / `culture_map` / `snow_map` (same size), `lf_heights.tif` 8128 x
  5748 (4 px per cell), `explicit_tiles.txt` (`x,y,tile path,rotation degrees`), `config.xml` (default texture groups), and BOB's
  final tile map already built: `tile_map.tiles` + `tile_map.index`. Its 32 MB `.terry` holds the battle locations (entities such as
  "Thermopylae" with an `ECRectangle` 2048 x 2048 and an `ECTransform` position).
- `tile_map.png` colours: `_tile_database/_settings.xml` gives each TILE_SET a colour (plains (90, 118, 71), settlement_western_cities
  (150, 75, 50) ...) and each climate one (`climate_map`: ROME_Med (255, 120, 0) ...). A tile's VARIATION can have its own colour
  (`TILES/settlement_western_cities_western_city_b.xml`: small = (150, 75, 64)): a map can pin a variation by colour; a tile set colour
  lets BOB choose.
- The cell is 1/8 of an 8 x 8 tile: a settlement tile (2048 local units, 1024 m in the editor) spans 8 x 8 cells.

## BOB's fill (`Warscape.AssemblyKit.dll` WARSCAPE::EDITOR_TILE_MAP; ported: `wce_tile::battle_map::fill`)

Exact against BOB on all 68 kit settlement maps and the campaign map `main_attila_map` (268 944 placements) (`--example
battle_map_check`, oracles from BOB's Terrain processor; the campaign map takes BOB 5 minutes).

- **Database** (`post_load_fixup`): tile set `i` = `0x80000000 | (i + 1)` (`_settings.xml` order, then "user" and "assembly_kit" when
  absent); tiles sorted by area (width x height) descending, then link targets descending, then location ascending (a total order),
  tile `i` = `i`; variation `v` of tile `t` = `0x40000000 | v << 16 | t`.
- **Cells** (`load_map`, `get_tile_id`): a tile set's colour wins, then a tile's, then a variation's (black: 0); the climate is the
  index of the climate with the pixel's colour (0 when none). `scan_tile_areas`: a padded box per set; a set cell pads by 128, a tile /
  variation cell by its tile's size, and a 0 cell counts as tile 0 (the largest).
- **Order** (`build_tile_map`): explicit tiles (`x,y,location,rotation`; a tile's location draws a random variation), then the scans 1 2
  9 8 5 7 6 3 4 0, then `calculate_flow`. Candidates per scan, in database order: masked tiles in the masked scans (4, 6), unmasked
  ones elsewhere; large (width or height >= `terrain_large_tile` 8) in 1 3 6, the others in the rest (both >= 8 excluded); 8: unmasked 2 x 2
  without links; 7: more than two link targets or one in another set, sorted by link target count (MSVC std::sort); 5: link targets in
  more than one set; 1 2: the set's `shared_geometry` is its name (no vanilla set). Each free cell, row by row, tries the candidates whose
  set's box holds it; the first that fits is placed (the masked scans keep trying).
- **The fit** (`test_final_tile_position`): per rotation, every (mask-valid) cell inside the map, free, non-zero and of the tile (a set
  cell: the tile's set; a tile / variation cell: the tile, and a variation cell pins it); then the tile's links: the linked cell's set
  (by `link_as_set`) against the link's, `TLT_EQUALS` passing when they match and the cell has no links yet or the link map has the
  set, `TLT_NOT_EQUALS` failing when they match. Then, for a random-rotatable tile, a variation is drawn (always, even when nothing fits,
  unless a cell pinned one; the exact scan's match is drawn over) and, unless the map is 64 cells wide, a starting rotation; the first
  fitting rotation from there wins. Masked tiles take the climate of their first cell.
- **Random generator** (fn 0x102806a0, state at +0x24 seeded 0x12344332): `x = x * 0x343fd + 0x269ec3`, `v = x >> 16`, rerolled while
  `v <= 0xffff % n`, result `v % n`.
- **Placing** (`place_tile`): the first cell gets the placed index (a 1-based list of the variations placed), the others `-(first + 1)`;
  rotation 0x10 (0 deg) .. 0x80, layer bits 0x700 (0x600 over a geometry tile). `add_tile_to_link_map`: a tile without link targets marks
  its cells with its set's link set; one with targets marks its cells' last slot and each target's set at the target cell.
- **Flow** (`calculate_flow`): from each river mouth, a breadth-first walk over the links into `river` / `river_crossing`; every tile reached
  takes, as its flip (flag 4), the `is_entry` of its `river` / `river_transition_end` link that lands on the tile it was reached from.
- **Heights** (`calculate_lf_min_maxs`): per placed tile, the min / max of the lf height field (`lf_heights.tif` / 65535) over the tile's
  box grown by 2 cells, in map fractions clamped to 0..1, times the field's size, all in single precision, the start truncated (on the campaign map, 1437 rows at 4 pixels per
  cell, a start of 92 comes out as 91.99999 and takes one more row).
- Not ported: resource tiles (scan 8 reads `EmpireDesignData/campaign_maps/<campaign>/dynamic_resources.png`, which the kit lacks; BOB
  then skips the scan), custom geometry, `also_place_tile_set`, shared geometry.

## BOB's output: `terrain.tile_list` (game reader fn 0x118ff250)

Running BOB's Terrain processor on a kit map (the scratch kit, consumers = every file of the map folder, see the memory note on BOB
configurations) writes `working_data/terrain/battles/<map>/terrain.tile_list`, `lf_height_map.data` / `.dds` and `lf_sea_height_map.dds`
(the historical battles in terrain2.pack ship the same files). Layout, version 11 (little endian):

- u32 version (11), u32 placement count;
- f32 x 6 (0.5, 0.5, 1, 1, 500, 1.333);
- u32 0, u32 map width, u32 map height (cells: 64, 64), u32 x2 (half size: 32, 32), f32 x2 (origin: -32, -32), i32 x2 (-8, -8 for the
  settlement map; 18, 18 for catalaunian_plains), u32 x2 (64, 64 / 101, 101), u8 (0), then more header bytes up to the name table;
- u32 name count, then each name as u16 length + chars (`terrain\tiles\battle\<set>\<size>\<variation>`);
- placements, 19 bytes each: u32 name index, u16 cell x, u16 cell y, u16 flags (0x0710 / 0x0720 / 0x0740 / 0x0780: the low byte one
  bit per rotation, to confirm), u8 (2 or 3), f32, f32 (0.00999 / about 0.12 - 0.14; meaning open).

settlement_western_city_b_small: 73 placements; the settlement tile `western_city_b\small` at (28, 28), 4 x 4 plains around it, 8 x 8
plains (aa .. ad) elsewhere.

## Game side

- Land battles: `terrain.pack` `terrain/battles/main_attila_map/` (lf height map, 736 normal chunks, tile_map.tiles / .index / .png,
  locations.blm + masks, 2074 deployment files). Historical battles: `terrain2.pack` `terrain/battles/<name>/` (lf heights, normals,
  terrain.tile_list). The settlement maps are not packed on their own.
- `tile_map.index` + `tile_map.tiles` (BOB's `TILE_MAP::save` 0x102e3770, game reader fn 0x118ffb60; `wce_tile::battle_map::tile_map`):
  version 9 in terrain.pack. The index is u32 version, width, height (2032 x 1437). The tiles file is the tile layer then the geometry
  layer, one 20-byte cell each (i32 index: > 0 the 1-based name placed with its origin here, < 0 -(origin cell + 1), 0 empty; u16 rotation
  0x10 .. 0x80 | layer bits 0x700, an empty cell 0x10; u16 x 2 origin; u8 climate; u8 flipped (the river flow); f32 x 2 lf min / max),
  then u32 name count and the names (u16 length + bytes) in placement order. The game also reads versions 6 / 7 (16-byte cells, a chunk
  list of 75 x 75 cell rects in the index): the kit's `main_attila_map/tile_map.tiles` / `.index` are such an old file (7 names, it
  matches nothing) and are not the reference.
- terrain.pack's tiles are not BOB's fill of the kit's map: the same 665 names, statistically the same placements, but only about 72 %
  of the origins hold the same tile (other sources or database at CA). Their tile sets follow the kit's `tile_map.png` (terrain.pack's
  `tile_map.png` differs from it in 75 743 cells). For the campaign in game, terrain.pack's file is the one to show.
- `locations.blm` (game reader fn 0x118573a0; `wce_tile::battle_map::locations`): "BLMF", u32 version 8, u32 type count 9, u32 width,
  height; per battle type a name (u16 length) and u32 count, then 9 x i32 per location: the cell rect (x0, y0, x1, y1 inclusive), centre,
  an id (-1, or 0xffff0000 + n for coastal / bridge battles), 0, 0. Then u32 cell count and a climate table. Counts: land_normal 2,
  settlement_unfortified 123, land_ambush 23 560, naval_normal 23, port_assault 67, coastal_battle 224, settlement_standard 186,
  unfortified_port 44, land_bridge 665. A settlement_standard rect starts at its settlement tile's origin (186 settlement tiles placed).
  `locations.blm.mask0`: one u16 per cell (bit flags, meaning open). The battle setup that picks one is fn 0x10b93720.

## Heights of a placed tile (game)

- The battle ground is `LF(x, z) * lf_height - vertical_offset + the tile's own height` (rigidterrain `vs30_geom_new`: `y = t_low_res_height_map *
  g_height_map_lf_scale + hf * g_height_map_hf_scale - g_height_map_vertical_offset`, the HF coming with the tile mesh's vertices;
  RENDER_PARAMS `lf_height` 3000, `hf_height` 1, `vertical_offset` 20).
- Nothing flattens the LF under a tile: BOB's `lf_height_map.dds` is the map's `lf_heights.tif` (237 of 46.7 million pixels differ by 1 on
  the campaign map), and the shipped tiles' `lf_influence_map.png` are 1 everywhere. The campaign LF is mostly flat under settlements
  (range over the tile: median 2.7 m, 10th percentile 0, 90th 35 m). A placement's LF min / max (2 cells around the tile) is the vertical
  extent of its blend box (`g_blend_lf_min_max`).
- Not traced: whether the game lifts a tile's buildings by the LF too (the building list's y is used as it is; on the kit maps the LF
  under the settlements is flat, so it doesn't show there).

## The map context in the editor (Tile panel > Map context; commands `map_context`, `map_context_settle`, `map_context_cache`)

- Maps: the game's campaign battle map (terrain.pack `terrain/battles/main_attila_map`: `tile_map.*`, `lf_height_map.dds`, `locations.blm`)
  and the kit's battle maps (filled by the port, `lf_heights.tif`). `wce_tile::battle_map::context`: the placements, where a tile is
  placed, each placement's frame transform (unit tests against `rotate_in_tile_space`).
- **Streamed** (2026-10-21; `wce_tile_editor::tile_document::context`): the map is cut into chunks of 8 x 8 cells, each drawn at a level by
  its distance from the focus (the orbit camera's target, the fly camera's eye, or the open tile with "follow the camera" off): **near**
  (everything of its tiles), **mid** (the tiles' coarse heights, quarter-resolution material, buildings and rivers: no vegetation, decals,
  lights, effects), **far** (the LF ground alone). The rings are the panel's sliders (defaults 8 / 24 / 64 cells); the far ring is at least what
  the camera sees (its far plane from its eye, whatever the focus, in whole chunks), so that no chunk comes and goes at the horizon; a
  chunk keeps its level within 2 cells past its ring's edge. Four worker threads build the chunks nearest first (a queued chunk no longer wanted is dropped); each
  frame the panel puts what they finished on one scene layer (a chunk's entities are a range of ids, removed in one pass).
- **Ownership**: a chunk owns the placements whose origin cell it holds; every cell has one owner (the open tile, the first placement over
  it, or nobody), so the levels never overlap: a far chunk draws LF ground over the cells it owns (its own placements' too, which reach out
  of it by up to the largest tile), merged into a few rectangles. The placements over the open tile's cells are not drawn.
- **Templates** (`context/template.rs`): a distinct tile is opened once (a whole `TileDocument`, published into a scratch scene) for all its
  placements: its own heights every 2 units over the tile and 4 grid steps of its border (neighbours overlap instead of meeting at an edge
  whose two sides differ), a skirt ring 20 units down, a coarse grid of one 64-cell patch (the mid ring), its material with its border and an
  alpha map (1 over the tile, 0 over the border, 4-unit texels whose edges fall on the tile's: bilinear, it fades out over a texel past the
  edge as the neighbour's fades in; the resolve divides by the weights' sum), the same material at a quarter, and its other entities.
  Placements share the heights buffers and materials (`Arc`): the renderer keeps one copy of their geometry and maps on the GPU.
- **Disk cache** (`context/cache.rs`, `<settings>/map_context/<key>.bin`, lz4): the template's heights, material and entities; the key hashes
  the tile folder's files (path, size, time), the terrain look, the trees switch and the format version. Rome small at the campaign's
  (619, 676) with the default rings: 89 tiles, 76 MB; 23 s to open them (four at a time), 0.5 s from the cache. The game files the tiles
  name are not in the key: `map_context_cache clear` empties it.
- **Heights as the game draws them**: the map's LF laid into the open frame (`LfMap`: the frame's axes are the map's turned by quarter
  turns, so its samples are the map's pixels, 8128 x 5748 on the campaign map) under every terrain, through the game's vertex shader
  (`t_low_res_height_map`, `g_height_map_lf_scale`, `g_height_map_vertical_offset`: `LF * 3000 - 20 - mean LF under the open tile`, scene
  units), each tile's own heights in the vertices (`dot(P0, P2) * g_height_map_hf_scale`). The open tile's ground is drawn on it too (its
  objects and picking keep its own heights: where the LF varies under it, they are off by that much; the report gives the range). The
  neighbours' objects are lifted by the LF at their position.
- **Placements as world matrices**: a terrain entity may be turned about y (the renderer's patch rows take the rotation; the LF is looked up
  by world position, so it stays right), its blend box is the material's rectangle turned with it, and the rotation index the blend shader
  takes (`T2.x`, its `icb` table) turns the maps back: no resampling per placement or rotation (the first version resampled every
  neighbour's heights and maps).
- **The LF texture's addressing** (`vs30_geom_new`): `uv = world xz / g_world_dimensions + g_lf_uv_offset`, one offset for both axes, so a
  texture's column 0 and its rows must start at the same x and z. The renderer draws each LF group's terrain in a frame moved by a shift
  (the camera constants moved with it, put back after) where they do; moving it to the map's corner (80 km away on the campaign map) lost the
  depth's precision (the sea, 30 units below, showed through the ground in stripes). The GPU gets a square window of the LF (2048 samples a
  side at most, the whole map when it is smaller) around the camera, made again when the camera leaves its middle, and the uv offset slides
  the shifted origin along the window's diagonal to the camera: the coordinates stay small where the terrain is seen.
- **LF normals**: `s_lf_normal_map` (ps30_blend) is read at `uv = (x, -z) / g_world_dimensions`, wrapping, .w = x and .y = -z of the normal;
  the context gives the map's LF normals laid out on that torus (4096 texels a side at most), so the far ground and the tiles get the LF's
  slope (it was flat).
- **Plain ground**: the far ring, the cells nobody covers and the tiles that could not be opened: zero heights on the LF's lattice (one
  buffer per rectangle size), a neighbour's material averaged to one colour (one id and one normal id for all of them), its blend box a
  little past its rectangle (2 units; 16 in the far ring) so that two of them side by side leave no pixel to neither (the resolve draws such
  a pixel black). The sea: one plane at 0 m over the cells within the far ring of the focus and 4 chunks more, moved with the focus.
- **Climates** (2026-10-22): the map gives every cell a climate (`climate_map.png`; BOB keeps it per placement: the tile map record's
  climate byte, the tile list's too), an index into the database's climates (`_settings.xml` order). Each placement's tile is opened in
  its climate (the template key and the cache key have it; around Rome the campaign map says ROME_Med, 3966 placements within 100 cells,
  and ROME_Atlantic, 36), and the plain ground is cut by the cells' climates, each with the base material given that climate's textures
  (`terrain_material::retexture`: the open tile's blend averaged, the climate's slots; one id per climate). The open tile keeps the climate
  of its own panel; the Map context section shows the map's at its placement and can use it. Not done: the per-season `snow_map_*.png`.
- **The renderer**: the scene logs its structural changes (`StructureChange`: removed id ranges, payloads replaced, anything else), so the
  draw set and the decals follow a chunk coming or going instead of rebuilding 150 000 items (45 ms a change, 170 ms at worst); the
  entities whose models are still loading are added when they come; the context's terrains skip the legacy patches (`Terrain::legacy`);
  the sea bed follows only the terrains over it; terrain materials stay on the GPU for 32 updates after their last use.
- Checked (2026-10-21, RX 7900 XTX): a lone tile against master: the same pixels (PSNR 65 - 75 dB, a few pixels of float rounding; 128-cell
  patches changed the distant level of detail and were dropped); a brush stroke and its undo the same. Rome small with master's rings
  (8 near, ground to 56): 10.3 / 13.4 ms a frame against master's 9.9 / 13.3 ms; the first ring 0.7 s against 2.8 s. Walking the focus east
  12 chunks while 9 new tiles open on the workers: 15.6 ms a frame on average, p95 40 ms, the worst 140 ms (new tiles' maps uploaded on the
  render thread).
- The kit's `plains/3x1/3x1_aa/height_map_0.png` is broken (its PNG signature went through a text-mode newline conversion: `0D 0A 1A 0D 0A`),
  and the `cliff_custom` tiles have no `tile.ted` in the kit: their cells show the LF ground.
- ASSUMPTIONS / known gaps: the mean-LF reference for the open tile (above); the LF bilinear in the CPU lift and the shader on the same
  lattice (4.6 cm steps: the 16-bit LF); the alpha feather is synthetic (the tiles' final_alpha is not read); at the far ring's outer edge
  one can see under the ground's last edge in thin lines (master too); new tiles' material maps (mip chains) are built on the render thread
  (several arriving in one frame hitch it) - fixed, below.
- **Uploads off the render thread** (2026-10-21): a streamed chunk's terrains get their GPU side (the patches' vertex buffers, the
  material's maps with their mip chains, the climate's texture arrays) made on the loading threads (`Renderer::terrain_ready`; the D3D11
  device is free-threaded); the panel puts a chunk on the scene once all its terrains are ready (the version it replaces stays drawn
  meanwhile: no holes, no frame without textures), at most 3 ms of them a frame. The draw set builds the legacy terrain set again only
  when a legacy terrain comes or goes. The walk above: 12.9 ms a frame on average (was 15.6), p95 36 ms, the worst 54 ms (was 140);
  over tiles already in memory 10.5 ms, p95 28, worst 38. What is left comes with the chunks' level changes in one frame (the draw set
  update, the shadow pass's first draws with the new buffers, the sea) and the tile opening on the workers competing for the cores.
- **Removals cost the chunk, not the set; loading below normal priority** (2026-10-21): the draw set kills a removed entity's item
  through the entity -> item table (culling skips dead items, new ones take their slots, the arrays are packed again once a quarter of
  them is dead): its update went from 3.1 ms to 0.6 ms a change. The streamer's workers and the thread pool their tile loading runs on
  (its own: rayon's global one is the renderer's culling) are below normal priority (`jobs::lower_thread_priority`); a worker waits for
  another's tile outside that pool (waiting inside it deadlocked: a pool thread in a parallel loop took up the waiting job). The walks:
  12.4 ms a frame, p95 32, worst 46 while tiles open; 10.4 ms, p95 27, worst 35 over tiles in memory. Opening tiles still slows every
  pass by about 2 ms (memory and caches, which priority does not share out).
- The campaign map's heights are the game's own `terrain/battles/main_attila_map/lf_height_map.dds` (8128 x 5748, `l16_dds_heights`),
  laid into the open frame (`LfMap`); its 736 normal chunks are not read (the LF normals are made from the heights).
- **Render origin** (2026-10-21): far from the open tile (1,170 cells away on the campaign map, 150 000 scene units) float32 positions lost
  the depth's and the ground textures' precision: the blend shader rebuilds positions from depth and samples the layers at `world /
  repeat`, so the ground swam in moire rings as the camera moved (a 0.1-unit move changed 35 % of the pixels, now 0.3 %, as next to the
  tile). The renderer gives the shaders positions relative to a render origin (the camera, instances, terrain rows and blend boxes, the LF
  normal map rolled by it, decals, lights, grass, shadows, the sea, rivers, effects); the scene, culling and picking stay absolute. The
  origin stays within 1216 scene units of the camera and moves on that grid (2026-10-21; it was 16 384: 4 km from the origin, at the
  harbour of western_port_a, the blend pass's positions from depth were off enough that the ground slid a few pixels against the quay's
  stone as the camera moved; within about 1.2 km, as a game battle is of its own origin, it does not). DEVIATION: the terrain layers'
  tilings are rounded to whole divisions of the grid (`tiling`: 9, 40.1, 620, 1200.1 m become 9.007, 40.53, 608, 1216, at most 1.9 %
  off), so the ground's patterns do not move when the origin does; the grid is a multiple of the LF samples (32), its normal texels (64)
  and the alpha texels (4). A move rolls the LF normal map on the GPU (one level, from an unrolled copy: up to four region copies),
  shifts the decals' instances, makes the sea bed again (5 to 8 ms) and the particles alive keep their old places. The shadow cascades are fitted around the camera relative to it too (fitted absolute and
  moved to the origin after, their matrices lost the precision: the shadows, the trees' most, flickered as the camera moved); culling keeps
  an absolute copy. Its height is the sea's (`RenderScene::sea_level`, the map context's `-mean LF / unit`): the shaders get heights
  relative to the sea, as the game's are to its sea at 0, so the terrain vertex shader's 9 cm lift of land and drop of sea bed (by the
  height's sign) separates the shore from the water as the game means it (relative to the open tile, it pushed all ground near a coast
  down). DEVIATION (2026-10-21): the height is measured from 1.5 steps of the 16-bit LF (6.9 cm) above the sea (`Frame::bias_y`). The
  map's coasts sit a step above 0 m (the LF holds 0 m to 4.6 cm: 437 / 65535 * 3000 - 20 = +0.5 cm, the coast at `western_port_a`
  is 438, +5.0 cm), and the tiles, built against their own sea at 0, lay their quays on ground just below it: measured from the sea, that
  ground was land, rose 18 cm against the quay's deck and fought with it as the camera moved (the tile alone draws it sunk). Ground
  less than 6.9 cm above the sea counts as sea bed now.
- Earlier versions: a radius around the open tile built at once, every neighbour resampled into the open frame and one shared 2 m height map
  over all of them (130 neighbours from 45 tiles, 1.2 s, about 1 GB); growing the window until no neighbour stuck out ran through the
  12 x 12 sea and mountain tiles to 200 x 170 cells (5,350 neighbours, 6 GB) and the app was killed (OOM).

## Open

- The flags / byte / floats of a placement; the header fields after the origin.
- How the game places and blends the tiles (lf heights, the tiles' own heights, final_alpha, the far / outfield maps).
- How a campaign battle picks its location and area (fn 0x10b93720); `locations.blm.mask0`.
