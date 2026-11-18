# Campaign shroud (Attila)

The campaign map's fog of war: the A8 texture `s_shroud` the game's campaign shaders sample, as a faction sees it on turn 1. Built by
`wce_terrain_io::attila::shroud` from the startpos and `map_data.esf`; checked against RenderDoc captures of the game (`main_attila`,
the Western Roman Empire, turn 1, frame 6628) by the `shroud_check` example: 97.5 % of the texels exact, 98.2 % within 2, 99.05 % within
16, mean difference 0.50 (the Python reference the module was ported from gives the same bytes).

## The texture

- A8, 1485 x 1240 on `main_attila_map`: the content, 2 texels a world unit, 1357 x 1112 = ceil(2 x 678.5) x ceil(2 x 555.625) (the
  startpos's `QUAD_TREE_BIT_ARRAY` size), plus 64 texels of padding on every side.
- Row 0 is the map's south edge (z = 0), x runs east. Content texel (x, y) covers world x in [x / 2, (x + 1) / 2), z in [y / 2, (y + 1) / 2).
- Fill: 64 (hidden) everywhere, the padding included; over the content 128 where explored, then 255 where visible, then 0 where the
  faction's third shroud bitmap is set. With the shroud disabled the texture is 255 everywhere.
- Blur: separable, horizontal then vertical, each pass summed in single precision and truncated to a byte. 20 taps at offsets -10 to +9,
  weight e^(-x² / (4 · 10)) · 0.19947, normalised to sum 1 (the padding keeps the taps off the texture's edges).

## The startpos's records

- `QUAD_TREE_BIT_ARRAY`: `u32` width (1357), `u32` height (1112), `u32` size (2048, the root square), `u8` blob. The blob is a
  depth-first quadtree over the root square, row 0 south: a node byte holds four 2-bit child codes from the low bits up, the children
  NW, NE, SW, SE ((0, half), (half, half), (0, 0), (half, 0)): `0b10` empty, `0b11` full, `0b01` subdivided (its node follows). A node
  byte 0 makes the node a leaf instead: 8 bytes (two little-endian `u32`, 64 bits), bit i the cell (i % 8, i / 8) of an 8 x 8 grid over
  the node's square, cell row 0 its south edge. Bits beyond width x height are dropped.
- `FACTION/CAMPAIGN_SHROUD`: three `QUAD_TREE_BIT_ARRAY`s (explored, visible, a third whose set bits are written 0), a `bool` (true on
  all 29 shrouds of `main_attila`), `CAMPAIGN_SHROUD_FILLERS`. Only some factions have one.
- `LINE_OF_SIGHT`: `bool` (on), the bounding box (two `Coord2d`, world units), a `QUAD_TREE_BIT_ARRAY` over the whole map. Under
  `CHARACTER` (generals, garrison colonels; the innermost `FACTION` the owner), `REGION` (`REGION`'s second leaf its key),
  `GARRISON_RESIDENCE` (the settlement; its `REGION` around it), `FACTION_SEA_REGION_MANAGER` (its first `u32` the sea region, the
  second the faction's id). Those under `FILLERS_QUADTREE_DATA_LOS_BLOCK` are not used.
- Region owners: `CAMPAIGN_PREOPEN_MAP_INFO/REGION_OWNERSHIPS_BY_THEATRE/REGION_OWNERSHIPS` (region key, faction key). Stances:
  `OLD_DIPLOMACY_RELATIONSHIP` (docs/formats/campaign_borders.md, `diplomacy`).
- `map_data.esf`: `REGIONS_BLOCK/REGION_DATA` (the regions in index order), their `CONNECTIVITY_DIRECT_*` records (traversable or not,
  same or other: each group's first `u16` a neighbour's index), `HEX_MAP_DATA` (byte 0 of a cell its region).

## The turn-1 model (fitted)

The game's own rule is not decompiled; this one is fitted to the captures. For the faction F, its group G = F and its `military_allies`,
and V = the factions to which a member of G has the stance `master` (their vassals):

- visible: the lines of sight of F's own characters, of the regions and settlements of the regions G owns, and of F's sea regions;
- explored: the first shroud bitmap of every member of G; and for the regions G and V own plus their direct neighbours (leaving out
  `att_reg_terra_incognita` and `att_sea_lake`): their hex areas (each texel takes the region of the hex cell whose centre is nearest
  its centre, (x + 0.5) / 2, (y + 0.5) / 2; cells as docs/formats/campaign_borders.md, "Frame") and their regions' lines of sight;
- 0: F's third bitmap.

Known misses against frame 6628: `att_reg_caucasia_gabala`'s area; a few visible spots; an explored disc at the map's (0, 0) corner
that the game does not show.

## The shaders

- `g_uv_offset` = (64 / 1485, 64 / 1240), `g_campaign_world_bounds` = (0, 0, 678.5, 555.625).
- uv = `g_uv_offset` + (pos.xz - bounds.xy) / (bounds.zw - bounds.xy) · (1 - 2 · `g_uv_offset`), faded to the edge over 0.01 by a smoothstep.
- G-buffer albedo darkening: e = 0.2 · smoothstep(sat((s - 0.1) · 2.5)), v = smoothstep(sat(2 (s - 0.5))),
  albedo' = lerp(sat(albedo · e), albedo, v).
- Borders: alpha · (b + 0.25 (1 - b)), b = 2 · sat(s - 0.5).
- The directional light: `s_overlay`'s colour · (sat(s - 0.5) · 1.9 + 0.05).
- The shroud plane (the fog and clouds over the map) reads it too.
