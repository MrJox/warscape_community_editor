# Battle maps (2026-10-18, in progress)

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

Exact against BOB on all 68 kit settlement maps (`--example battle_map_check`, oracles from BOB's Terrain processor).

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
  box grown by 2 cells, in map fractions clamped to 0..1, times the field's size, the start rounded half to even.
- Not ported: resource tiles (scan 8 needs a resource map; the campaign map's), custom geometry, `also_place_tile_set`, shared geometry.

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
- `tile_map.index` (fn 0x118ffb60): u32 version (3), u32 width, u32 height, u32 chunk count (736), then per chunk u32 x, y, w, h
  (75 x 75 cells at a 64 cell stride): the normal chunks. `tile_map.tiles`: two arrays of 16 byte cell records (the same fields as a
  placement: name index, x, y, flags, bytes), then a name table; the exact layout of the kit's file is still open.

## Open

- The flags / byte / floats of a placement; the header fields after the origin.
- How the game places and blends the tiles (lf heights, the tiles' own heights, final_alpha, the far / outfield maps).
- main_attila_map's tile_map.tiles layout; the battle locations in its .terry and how a settlement's battle picks its area.
