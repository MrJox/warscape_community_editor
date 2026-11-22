# Tile links (findings 2026-10-10; edited since 2026-10-28)

The links of a tile live in its database entry (`TILE_LINK_TARGETS` / `TILE_LINKS`, `docs/formats/tile_database.md`), not in `tile.ted`. Measured on the kit:

- **Cells**: 128 m. A target's or rule's cell (x, y): **x counts along +x from 0**, **y counts from the +z edge**: y = cells_y - 1 - floor(z / 128). Evidence: `dunedain_city_osgiliath/minor` (8 x 8) has river targets at (0, 3) and (7, 4); its river splines start at the west edge at
  z = 576 m (floor(576 / 128) = 4, 8 - 1 - 4 = 3) and leave at the east edge at x = 1025 m, z = 448.5 m (floor = 3, 8 - 1 - 3 = 4). The rules name the neighbour columns: `x = -1` and `x = 8` (the first beyond the east edge), rows 3 and 4.
- A **target** (`tile_set x y`) says this tile offers a join of that kind at its edge cell; a **rule** (`link_set x y base_x base_y is_entry blend_size no_offline_blend test`) says the cell (x, y) must (`TLT_EQUALS`) or must not (`TLT_NOT_EQUALS`) belong to the link set; `base_x / base_y` is the cell of this tile the rule belongs to
  (in osgiliath both rules have base (1, 0), which does not look like the edge cell: unexplained), `blend_size` the pixels the neighbour's terrain is blended in. `base_x / base_y` and `is_entry`: below.
- **Counts** (battle kit, 2026-10-09): rules `TLT_EQUALS` 1,512, `TLT_NOT_EQUALS` 242; link sets `sea` 908, `river` 209, `roads` 194, `sea_coast` 182, `roads_stone` 104, `roads_mountain` 54, `river_shallow` 51, `cliff_custom` 48. `no_offline_blend` is read as turning off BOB's pre-blend at that edge (from the name, not tested). The height map's one-cell border (`overlap_border_size`) is the overlap where a neighbour's heights and ground blend across the edge.
- The river splines' end points lie on the target cells (osgiliath: both ends).

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

The tool shows them on the ground: the Tile panel's *Tile links* section maps the entry's targets and rules, the tile's cells and the ring of neighbours' round them (a click picks a link, or adds one to an empty cell, and looks at its cell unless *Follow* is off), and *Show on the ground* draws each cell draped over the terrain, coloured by link set (sea blue, river cyan, roads sand, sea_coast yellow, roads_stone grey, roads_mountain brown, river_shallow aqua, cliff_custom orange; others by a hash), a rule that must not hold with a cross,
a target with a pole and a ring at the cell's centre. A rule's base point is no cell of this tile (above), so nothing joins it to its cell. The links are edited there too (2026-10-28, `wce_tile_tool::links_editor`); the validator is `wce_tile::link_checks` (its findings
over the kit in the module's header; the `wce_tile` example `link_check` runs it over a kit), and the tiles that can follow a link are shown
through the map context (docs/plans/tile_ted_parity.md, Phase 6a, done).

## Rivers at their links (2026-11-22, docs/plans/rivers.md R1 to R4)

**Units**: every height here is the game's (`TileProject::height_at`, `final_heights.dds`, the river files' y, TEd's scene heights):
twice the editor's metres in a battle tile (`Tile::terrain_height` is `height_at` over the unit, 2), as x and z are. So the deep
connection's bed -11.9 and water -6.2 show as -5.95 and -3.1 in the editor; the widths and distances across are the editor's metres.

Measured with the `wce_tile` example `river_link_profile` over the Attila kit's battle tiles (107 tiles with river files, 206 river
ends within half a cell of an edge; `--grid` prints a tile's heights, used on the linking tiles):

- **Frame**: the files' x / z are the tile's metres with the entry's cell rule above (y = cells_y - 1 - floor(z / 128)): 202 of the 206
  ends sit on a river target that way (63 with z counted the other way). The 4 others are the `*_small_br` river starts and mouths (a
  river that begins or ends inside the tile) and a crossing's side stream (width 20).
- **Two standard connections**, one per link set, the same in every family:
  - `river`: **bed -12.0 m** (flat about ±8 m around the cell's centre), **water about -6.2 m** in the files (`river`, `river_crossing`,
    `river_ford`, `river_start`; median -6.20, -6.22, -6.23) and -7.0 in the four settlement families: a 0.8 m step the kit lives with where
    they meet. **Width 45** (35 in some settlements, 60 in one western city, 37.5 in a ford).
  - `river_shallow` (also `river_start_shallow`, `river_mouth_shallow`): **bed -1.0 m**, **water about +0.42 m**, width 45.
- **Place and direction**: 1.4 m off the cell's centre along the edge (median; 3.3 p90), 5.7 m past the edge (median; 0.85 .. 7.4 p10 ..
  p90), the end's tangent 1.2° from the edge's normal (median; 6.3° p90): the river leaves square to the edge through the cell's middle.
- **The export forces the connection** (`final_heights.dds` against the editing heights): along a river-linked edge the exported heights
  are one cross-section whatever the tile has there (p10 .. p90 spread under 1 m inside ±64 m, against 2 .. 5 m in the editing heights).
  Deep, every 8 m from -64 to 64 across the cell's centre: `0, -0.1, -0.5, -1.2, -2.4, -3.9, -6.8, -11.6, -11.9, -11.6, -6.8, -3.9, -2.4,
  -1.2, -0.5, -0.1, 0`; shallow: `0 .. 0.2` on the banks, `-0.2` at ±16, `-1.0` at ±8 and 0. It is the **linking tile**'s: `river/1x1_blend`
  and `river_shallow/1x1_blend` (1 x 1, the river straight through z = 64) export that same cross-section, the same in every column.
  Away from the cell the edge fades to 0 as a land edge.
- **Inward**: `final ≈ w(d) * editing + (1 - w(d)) * profile`, with w the land edge's fade weight (battle_maps.md): within 0.1 m at the
  edge (median), 0.3 .. 0.7 m 16 .. 48 m in (the fade to 0 is off by 1 .. 2 m). The curve near a link is not exactly the land one (the
  link's `blend_size` 64): ASSUMPTION that a bed which already has the profile over the link's cell comes out unchanged whatever the curve.
- **Water against the bed**: the deep ribbon (45 m) is wider than the wet bed: the water at -6.2 meets the banks about ±17 m from the
  centre, the ribbon's edges go under the ground.
- **Flow (R3, not settled)**: in the deep families the river's first point is mostly at its `is_entry` link (66 against 6 the other way, 24
  in all counting the shallow set); every `river_shallow` tile has it the other way (19 tiles). So the kit's point order is no rule; what
  the player sees (the uv's distance along scrolls with the clock) needs an in-game look before the editor calls a reversed river a problem.
- **Neighbours (R4)**: since the export forces the cross-section, two kit tiles always meet in the bed; the water may step by 0.8 m (river
  family against settlement). A check's tolerance for the water at a link: 1 m.
