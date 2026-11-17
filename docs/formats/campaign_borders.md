# Campaign borders and trade routes (Attila)

What the campaign map's overlay draws at the start: the region borders coloured by the diplomacy of a chosen faction, and that faction's
trade routes. Read by `wce_terrain_io::attila::{borders, border_fit, trade_routes, diplomacy, overlay}`; checked against a RenderDoc
capture of the game (`main_attila`, the Western Roman Empire, turn 1) by the `borders_check` example. Addresses are empire.retail.dll's.

## Frame

The hex grid of `map_data.esf` (`REGIONS_DATA/theatres/CAMPAIGN_THEATRE/HEX_MAP_DATA`: 1016 x 720 on `main_attila_map`) over the
world's width (`REGIONS_DATA`'s second `Coord2d`: 678.5). Flat-top hexes, odd columns half a row up (fn 0x10950410):

- `colw` = width / (columns - 1) (0.668473), R = `colw` / 1.5 (the hexes' radius, 0.445649), `rowh` = √3 · R (0.771887);
- a cell's centre: x = column · `colw`, z = row · `rowh` (+ `rowh` / 2 on odd columns); row 0 at z = 0 (the south edge), the grid
  678.5 x 555.6. All in single precision; column and row clamped to the grid.
- Neighbours (fn 0x10b760a0), index 0 the next row then clockwise, q = row + (column & 1): (c, r+1), (c+1, q), (c+1, q-1), (c, r-1),
  (c-1, q-1), (c-1, q). Hex distance (fn 0x10b9d4c0): dx = a.c - b.c, dy = ((a.c+1) >> 1) - ((b.c+1) >> 1) - b.r + a.r,
  max(|dx|, |dy|, |dy - dx|).

## `campaign_maps/<map>/display/borders/borders.pbd`

Little-endian.

| Field | |
| --- | --- |
| `u16 1`, `u16 0x12a` | version |
| `u32 n`, n x (`u32 len`, ASCII) | region keys (224 on `main_attila_map`, the sea regions and `terra_incognita` included) |
| `u32 n` entries | per region (203): `u32 region` (index into the keys), `u32 n` segments |
| segment | `u32 neighbour` (index into the keys), `u32 n` lines, each `u32 n` + n x (`u16 column`, `u16 row`) |
| `u32` | CRC-32 (zlib's) of every byte before it |

A segment's line is the region's edge cells towards that neighbour; the pairs are symmetric (each side has its own line). 839 segments,
808 of one line, 31 of two.

## Chains (fns 0x10afda10, 0x10b22e10, 0x10ad5750)

Over the startpos's regions in order (`WORLD/REGION_MANAGER/REGIONS_ARRAY`; the sea regions are not there), each region's lines in the
file's order:

- owner and province: the startpos's owner (`SETTLEMENT+9`, a faction's runtime id; the rebels count as one owner), the db's
  `region_to_province_junctions`;
- a line towards a neighbour with no owner (the sea, off the map) or another owner is **external**; towards the same owner's other
  province **internal**; the same province: dropped;
- a line joins the region's first chain of its kind (internal / external) one of whose ends is within one cell (hex distance <= 1) of
  one of the line's ends (fn 0x10b2b8d0), else starts a chain; a chain remembers the region pairs it borders;
- the join (fns 0x10acad50 / 0x10acac50): the four end distances (chain start / line start, start / line end, end / line start,
  end / line end), first one of 0 (the shared cell dropped), then one of 1: prepend the line reversed, prepend it, append it, append
  it reversed;
- then per region, repeated until nothing merges (fn 0x10ad5750): its chains take the touching chains of the owner's other regions
  (external with external, internal with internal when the two regions share a province) that share no region pair with them (fn
  0x10af92b0), and its own external chains take each other the same way.

`main_attila`: 145 chains.

## The fit (fn 0x10aae0b0)

A chain's cells to cubic Béziers, in single precision:

1. Funnel (fns 0x10c01890, 0x10c355d0, 0x10c4cf50): from the first cell's big-hex vertex (k + 4) % 6 (k the direction to the second
   cell; vertex angles 0.5235984, 1.570795, 2.6179917, 3.6651886, 4.712385, 5.759582, x = sin · r, z = cos · r), walk the steps
   cell -> next cell; each step gives portal points (fn 0x10ab3e40: the vertices, radius 0.15, of the cells around the step on side A
   or B, 6, 3, 9, 12 or 15 points by the turn between the step's direction and the previous one); a point crossing the funnel's other
   side makes that side's point the next apex, the walk rewinds to that point's step. Ends at the last step.
2. Post-processing (fn 0x10c37260): the end point the last cell's big-hex vertex (j + 5) % 6 (j the last step's direction);
   consecutive segments whose directions, quantised to 16 bits (atan2(x, z) · 10430.378, rounded half to even), differ by less
   than 0.0628318 radians merge; zero-length ones go; tangents: the first segment's direction, at joints the bisector, the last end
   minus its direction.
3. Short segments: a run of segments with squared length <= (1.05 · 0.15)², not first or last, becomes one point: a run of one its
   midpoint pushed 0.2 · 0.15 along its normal (side A +, B -), two the shared point, three the second's midpoint pushed so.
4. Handles (fns 0x10c361c0, 0x10bead10, 0x10c89f20): each segment's handles along its tangents, length 0.4 · its length when no
   portal point between its apexes constrains it, else the nearest point's distance from the chord over the tangent's sine (sine
   > 0.01), at most 0.4 · the length; at least 0.15. The last segment keeps its unit tangents.

As drawn (fns 0x10ab2ed0, 0x10ac9bb0): a curve with P0 = P3 is dropped; a handle on its end point is replaced by the midpoint of the
other handle and the far end (both: the chord's midpoint), such a curve's length its chord; else its length is the left Riemann sum
of |B'(t)| over 1000 steps (fn 0x1082a900). uv offset = the length before the curve / the chain's, uv scale = its length / the
chain's. Each curve draws 16 vertices at equal steps of its arc length: a 100-entry cumulative speed table (fn 0x1082e8f0) inverted
at i / 16 (fn 0x10848d00); a chain's last curve adds a 17th at t = 1. Half-width 0.225; the outer texture for external chains, the
inner for internal ones.

## Colours (fn 0x10946810)

For the chosen faction P and a chain's owner F (its region's owner), the first that holds:

| Class | When | RGB |
| --- | --- | --- |
| rebel | F has no `factions` record (the startpos's `WORLD/REBEL_FACTION`, empty key) | 0, 0, 0 |
| war | F ≠ P and (P is the rebels or P's stance to F is war; fn 0x10715ff0) | 255, 0, 0 |
| own | F = P | 180, 60, 0 |
| allied | P's stance to F is 2 to 7 (fn 0x10829140) | 0, 90, 190 |
| neutral | otherwise | 178, 178, 178 |

Stances: `FACTION/OLD_DIPLOMACY_MANAGER/DIPLOMACY_RELATIONSHIPS_ARRAY/OLD_DIPLOMACY_RELATIONSHIP`, `^+1` the other faction's runtime
id, `^+4` the stance: `war` 0, `neutral` 1, `defensive_allies` 2, `military_allies` 3, `protector` 4, `client_state` 5, `master` 6,
`vassal` 7; no record is neutral. Values / 255 for the shader.

The faction list: `CAMPAIGN_PREOPEN_MAP_INFO/FACTION_INFOS`, per faction its key, two flags (the second: playable), its list order and
its regions at the start. No default is in the data read; the editor picks the playable faction with the most regions (the Western
Roman Empire on `main_attila`, the captures' player).

## `campaign_maps/<map>/trade_routes.ptd` (loader fn 0x1084eb70)

| Field | |
| --- | --- |
| `89 'PTD' 0d 0a 1a 0a`, `u32 1` | magic, version |
| `u32 n`, n x (`u32 len`, ASCII) | region keys (187) |
| `u32 n` segments (2486) | `u8 x 4` flags, `u32 n` curves of 16 bytes: `u16 X0, u16 Z0`, `i16 dx1, dz1`, `i16 dx2, dz2`, `u16 X3, u16 Z3` |
| 2 x route table | `u32 n` x (`u32 region A`, `u32 region B`, `u32 n` + n x `u32` refs = 2 · segment + reversed); table 0 the land legs (1137), 1 the sea legs (1739) |
| `u32` | CRC-32 of every byte before it (not read by the loader) |

A point: x = (X / 4 - 1) · R, z = (Z / 4 - 1) · R · √3 / 2; P1 = P0 + 0.01 · (dx1 / 4 · R, dz1 / 4 · R · √3 / 2), P2 = P3 + the same
of (dx2, dz2).

The drawn routes: the startpos's `CAMPAIGN_MODEL/CAMPAIGN_TRADE_MANAGER/INTERNATIONAL_TRADE_ROUTES`, per group the owner's runtime id
and its `INTERNATIONAL_TRADE_ROUTE`s (`^+1` the partner, `^+2` the path of region runtime ids, `^+3` a sea leg, then its two path
indices). The chosen faction's routes, each leg looked up both ways round in table 1 when it is the sea leg, else table 0; each
segment once, in the file's direction. A segment is one draw: its curves' lengths as above, uv in world units scaled so the
segment's total is a whole number (uv offset = length before · ⌈L⌉ / L, scale = length · ⌈L⌉ / L); 16 vertices a curve by arc
length as the borders', the last curve 17. Half-width 0.05, colour (240, 200, 20).

## Check (`borders_check`, capture A)

- Borders: 982 captured curves in 27 draws; 947 within 1e-3 (max control-point error 4.3e-4, uv 4.7e-6, vertex t 7.2e-5).
- Trade routes: the 19 draws are segments 1707-1713, 1856-1859, 2100-2107 (max error 3.1e-5, uv 3.3e-5, t 5.7e-6).

## Known gaps

- Three land curves (draws 15805, 15825, 15839, one curve each, 0.13 to 0.15 off): not explained. Untraced too: a leading or
  trailing run of short segments, and closed chains' wrap-around, are kept as they are.
- The sea borders (draws 15858, 15865, 15872, 15879, 32 curves): not built; the chains here are the land regions' only.
- Trade routes: capture A drew only the sea legs' segments of the player's routes; land legs' segments in view (615, 696-699,
  2096-2099) were not drawn, the rule is not known. The reader returns both, flagged (`sea_leg`).
- The default faction: no data names it (see Colours).
