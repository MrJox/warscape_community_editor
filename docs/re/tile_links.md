# Tile links (findings 2026-10-10; edited since 2026-10-28)

The links of a tile live in its database entry (`TILE_LINK_TARGETS` / `TILE_LINKS`, `docs/formats/tile_database.md`), not in `tile.ted`. Measured on the kit:

- **Cells**: 128 m. A target's or rule's cell (x, y): **x counts along +x from 0**, **y counts from the +z edge**: y = cells_y - 1 - floor(z / 128). Evidence: `dunedain_city_osgiliath/minor` (8 x 8) has river targets at (0, 3) and (7, 4); its river splines start at the west edge at
  z = 576 m (floor(576 / 128) = 4, 8 - 1 - 4 = 3) and leave at the east edge at x = 1025 m, z = 448.5 m (floor = 3, 8 - 1 - 3 = 4). The rules name the neighbour columns: `x = -1` and `x = 8` (the first beyond the east edge), rows 3 and 4.
- A **target** (`tile_set x y`) says this tile offers a join of that kind at its edge cell; a **rule** (`link_set x y base_x base_y is_entry blend_size no_offline_blend test`) says the cell (x, y) must (`TLT_EQUALS`) or must not (`TLT_NOT_EQUALS`) belong to the link set; `base_x / base_y` is the cell of this tile the rule belongs to
  (in osgiliath both rules have base (1, 0), which does not look like the edge cell: unexplained), `blend_size` the pixels the neighbour's terrain is blended in. Semantics beyond that are the Phase 6a RE items.
- **Counts** (battle kit, 2026-10-09): rules `TLT_EQUALS` 1,512, `TLT_NOT_EQUALS` 242; link sets `sea` 908, `river` 209, `roads` 194, `sea_coast` 182, `roads_stone` 104, `roads_mountain` 54, `river_shallow` 51, `cliff_custom` 48. `no_offline_blend` is read as turning off BOB's pre-blend at that edge (from the name, not tested). The height map's one-cell border (`overlap_border_size`) is the overlap where a neighbour's heights and ground blend across the edge.
- The river splines' end points lie on the target cells (osgiliath: both ends), which is the check the validator of 6a will make.

- **`base_x / base_y`** (2026-10-28, a reading of the code, not checked in game): `TILE_LINK::base_point`. The game's fill never reads it
  (`docs/re/battle_maps.md`, "The fit"); the export's link blend does (ToolDataBuilderDll `FUN_10015610`, called from the blend
  `FUN_10015fd0`): for each rotation the link set's linking tile may take (`_settings.xml` `TILE_SET linking_tile`: 1 x 1 tiles,
  `river\1x1_blend`, `roads\1x1_straight`, `sea\1x1`...), it turns the base point in the linking tile's space
  (`rotate_in_tile_space`) and adds the link's point inverted: the cell of this tile, next to the link, where that linking tile's terrain is
  blended in (clamped to the tile, times its cell size). With a 1 x 1 linking tile, (1, 0) is one cell over in the turned linking tile's
  +x, (0, 0) the link's cell itself. Before that, the function notes whether one of the tile's targets lies right next to the link's cell
  (a Manhattan distance of 1). The kit: (1, 0) 1,017 times, (0, 0) 274, (-1, 0) 92.
- **`is_entry`**: the river flow (`calculate_flow`): every river tile reached takes as its flip the `is_entry` of its river link that
  lands on the tile it was reached from (`docs/re/battle_maps.md`, "Flow").

The tool shows them on the ground: the Tile panel's *Tile links* section lists the entry's targets and rules ("cell (-1, 3) must be river; belongs to cell (1, 0), blend 64 px"; a click looks at the cell), and *show on the ground* draws each cell draped over the terrain, coloured by link set (sea blue, river cyan, roads sand, sea_coast yellow, roads_stone grey, roads_mountain brown, river_shallow aqua, cliff_custom orange; others by a hash), a rule that must not hold with a cross,
a target with a pole and a ring at the cell's centre, a rule's base cell joined to its cell by a line. The links are edited there too (2026-10-28, `links_editor.rs`); the validator, the neighbour preview and the link overview are the rest of Phase 6a.
