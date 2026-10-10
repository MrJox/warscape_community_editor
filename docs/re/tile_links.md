# Tile links (read-only view; findings 2026-10-10)

The links of a tile live in its database entry (`TILE_LINK_TARGETS` / `TILE_LINKS`, `docs/formats/tile_database.md`), not in `tile.ted`. Measured on the kit:

- **Cells**: 128 m. A target's or rule's cell (x, y): **x counts along +x from 0**, **y counts from the +z edge**: y = cells_y - 1 - floor(z / 128). Evidence: `dunedain_city_osgiliath/minor` (8 x 8) has river targets at (0, 3) and (7, 4); its river splines start at the west edge at
  z = 576 m (floor(576 / 128) = 4, 8 - 1 - 4 = 3) and leave at the east edge at x = 1025 m, z = 448.5 m (floor = 3, 8 - 1 - 3 = 4). The rules name the neighbour columns: `x = -1` and `x = 8` (the first beyond the east edge), rows 3 and 4.
- A **target** (`tile_set x y`) says this tile offers a join of that kind at its edge cell; a **rule** (`link_set x y base_x base_y is_entry blend_size no_offline_blend test`) says the cell (x, y) must (`TLT_EQUALS`) or must not (`TLT_NOT_EQUALS`) belong to the link set; `base_x / base_y` is the cell of this tile the rule belongs to
  (in osgiliath both rules have base (1, 0), which does not look like the edge cell: unexplained), `blend_size` the pixels the neighbour's terrain is blended in. Semantics beyond that are the Phase 6a RE items.
- The river splines' end points lie on the target cells (osgiliath: both ends), which is the check the validator of 6a will make.

The tool shows them on the ground: the Tile panel's *Tile links* section lists the entry's targets and rules ("cell (-1, 3) must be river; belongs to cell (1, 0), blend 64 px"; a click looks at the cell), and *show on the ground* draws each cell draped over the terrain, coloured by link set (sea blue, river cyan, roads sand, sea_coast yellow, roads_stone grey, roads_mountain brown, river_shallow aqua, cliff_custom orange; others by a hash), a rule that must not hold with a cross,
a target with a pole and a ring at the cell's centre, a rule's base cell joined to its cell by a line. Read-only: editing the links, the validator, the neighbour preview and the link overview are the rest of Phase 6a.
