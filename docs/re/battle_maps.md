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

## The map context in the editor (Tile panel > Map context; command `map_context`)

- Maps: the game's campaign battle map (terrain.pack `terrain/battles/main_attila_map`: `tile_map.*`, `lf_height_map.dds`, `locations.blm`)
  and the kit's battle maps (filled by the port, `lf_heights.tif`). `wce_tile::battle_map::context`: the placements, where a tile is
  placed, the neighbours within a radius, each placement's frame transform (unit tests against `rotate_in_tile_space`); the cells no
  neighbour covers get LF ground (`Context::uncovered`).
- `wce_tile_editor::tile_document::context`: each distinct neighbour tile is opened (4 at a time) and published once into a scratch
  scene, then copied per placement through its transform; terrain is resampled into the open frame (the renderer's height lookup is by
  world position, so a terrain entity can't be rotated) with one material per tile and rotation (blend / index resampled over the tile
  without its border, normal map from the tile's own heights).
- While the context is shown the open tile is drawn over its own cells only (`Tile::set_clip_to_tile`: terrain and material maps without
  the height map's border); its border lay over the neighbours (it hid the river tiles around Rome and z-fought with them).
- Terrain: the game's terrain vertex shader (`vs30_geom_new`) samples its height texture at `world.xz / g_world_dimensions +
  g_lf_uv_offset` with ONE offset for both axes, so a height texture must start at the same world x and z. A lone tile does; neighbours
  around it don't, and the renderer then dropped the game's terrain pass for the whole scene (plain ground everywhere, the open tile too).
  As the game does with its map-wide LF texture, the neighbours read one shared height map (`SharedHeights`, square, on the diagonal, 2 m,
  over all of them) and the ground beyond another (LF pixels); each terrain keeps its own grid for the patches. Every context terrain has a
  material (the cells no tile covers: a neighbour's material averaged to one colour). The renderer also kept at most 4 terrain materials
  (cleared when more were built in one frame, so most drew without textures): it now keeps the ones the frame uses and shares the climates'
  texture arrays between materials.
- Water: the renderer draws water vertices as they are (no entity world matrix), so the neighbours' river ribbons are moved into the frame
  and lifted per vertex when the context is built. The tiles' sea planes are dropped for one sea plane at 0 m over the whole context
  (ASSUMPTION: the battle sea is at 0 m; the LF's lowest value is -vertical_offset, -20 m, under the open sea); with it the coast around
  Rome falls where the coast tiles are.
- The kit's `plains/3x1/3x1_aa/height_map_0.png` is broken (its PNG signature went through a text-mode newline conversion: `0D 0A 1A 0D 0A`):
  that tile can't be opened, its cells show the LF ground.
- ASSUMPTIONS: the open tile is shown at its own heights, so the context is relative to it: a neighbour point at its height + (LF - the
  mean LF under the open tile) / unit_scale, objects lifted by that at their position; seams can be off by up to half the LF range under
  the open tile (the report gives it). The LF slope is not in a neighbour's shading normals. The neighbours' borders (overlap cells) and
  final_alpha feathering are not drawn (hard edges between tiles). Where the near (2 m) and far (LF pixel) maps meet, a thin crack can show.
- Checked on western_city_b small: in its kit map (18 neighbours) and at the campaign's (480, 461), rotated 270 degrees (130 neighbours from
  45 tiles, 1.2 s, about 1 GB peak): roads and fields run on across the seams, rotated tiles included. western_city_rome small at the
  campaign's (619, 676): the Tiber goes on through the river tiles north and south of the city (its links: river at cells (3, -1) and
  (3, 8), the map's river tiles at x 622) down to the coast.
- Earlier version that grew the window until no neighbour stuck out: on the campaign it ran through the 12 x 12 sea and mountain tiles to
  200 x 170 cells (5,350 neighbours, 6 GB) and the app was killed (OOM).

## Open

- The flags / byte / floats of a placement; the header fields after the origin.
- How the game places and blends the tiles (lf heights, the tiles' own heights, final_alpha, the far / outfield maps).
- How a campaign battle picks its location and area (fn 0x10b93720); `locations.blm.mask0`.
