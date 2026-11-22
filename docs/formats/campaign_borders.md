# Campaign borders and trade routes (Attila)

What the campaign map's overlay draws at the start: the region borders coloured by the diplomacy of a chosen faction, and that faction's
trade routes. Read by `wce_terrain_io::attila::{borders, border_fit, trade_routes, diplomacy, overlay}`; checked against a RenderDoc
capture of the game (`main_attila`, the Western Roman Empire, turn 1) and later ones by the `borders_check` example. Addresses are
empire.retail.dll's.

## Frame

The hex grid of `map_data.esf` (`REGIONS_DATA/theatres/CAMPAIGN_THEATRE/HEX_MAP_DATA`: 1016 x 720 on `main_attila_map`) over the
world's **height** (`REGIONS_DATA`'s second `Coord2d`, its y: 556.1439208984375). Flat-top hexes, odd columns half a row up
(fn 0x10950410). In single precision:

- `rowh` = height / (rows + 0.5) (0.77188605), R = `rowh` / √3 (the hexes' radius, 0.44564864), `colw` = 1.5 · R (0.66847295),
  half a row `rowh` / 2. (The width's 678.5 / 1015 = 0.668473 is 1 to 2 ulps off: enough to move a few fitted curves 0.15.)
- a cell's centre: x = column · `colw`, z = row · `rowh` (+ `rowh` / 2 on odd columns); row 0 at z = 0 (the south edge), the grid
  678.5 x 555.8. Column and row clamped to the grid.
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

## Land chains (fns 0x10afda10, 0x10b22e10, 0x10ad5750)

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

## Sea chains (fns 0x10afdb40, 0x10b233a0, 0x10ad5ba0)

After the land, over the startpos's sea regions in order (`WORLD/SEA_REGION_MANAGER/SEA_REGIONS_BLOCK`: per group the sea's key and
its `SEA_REGION`). A sea's factions (its +0x2c count, +0x30 list): its `FACTION_SEA_REGION_MANAGER_BLOCK/FACTION_SEA_REGION_MANAGER`s
in order (`^+1` an id, `^+2` the faction's runtime id, `^+3` its regions). The pbd's sea entries list sea neighbours only.

- every line is kept: **internal** when both seas have exactly one faction and it is the same (fns 0x10aaef10, 0x10aad530), else
  external; the pseudo-seas without a sea record (`att_sea_nilos`, `att_sea_rhenus`, `att_sea_danuvius`) have none, so external;
- lines join chains as on land (fn 0x10b233a0);
- then per sea, until nothing merges (fn 0x10ad5ba0): its chains take the touching chains of the seas related so (one faction, the
  same), external with external, internal with internal (no province test), sharing no region pair; then its own external chains
  take each other.

Drawn as the land's: half-width 0.225, fades 0.75, the outer / inner textures. `main_attila`: 72 chains.

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

For the chosen faction P and a land chain's owner F (its region's owner), the first that holds:

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

A sea chain (fn 0x10b99a80, on its sea's factions; the rebel exception traced 2026-11-21): at war with P (fn 0x10715ff0: F ≠ P and F or P is
the rebels, or P's stance to F is war) counted; the list exactly [P] own; none at war: allied when a faction other than P is allied (fn
0x10715bb0: neither the rebels, stance 2 to 7), else neutral (an empty list neutral); exactly one at war, the rebels (+0x800 null, fn
0x10716690) in the list, and every region the rebels hold abandoned (their +0x54 list, each region's +0x628, fn 0x10951060: the startpos's
`REGION` `REGION_REBEL_MANAGER+1`; none held counts as all): neutral; else war. `main_attila`'s rebels hold its 11 ruins in the east at the
start (Tanais, Samandar, Sarai...), all abandoned: Mare Sarmaticum and Pontus Cimmerius, where the rebels are the only faction at war with
the Western Roman Empire, are neutral.

The faction list: `CAMPAIGN_PREOPEN_MAP_INFO/FACTION_INFOS`, per faction its key, two flags (the second: playable), its list order and
its regions at the start. No default is in the data read; the editor picks the playable faction with the most regions (the Western
Roman Empire on `main_attila`, the captures' player).

The faction colours of standards, figures and settlement flags (models, not the overlay): docs/re/terry.md, "The campaign's start",
"The models".

## `campaign_maps/<map>/trade_routes.ptd` (loader fn 0x1084eb70)

| Field | |
| --- | --- |
| `89 'PTD' 0d 0a 1a 0a`, `u32 1` | magic, version |
| `u32 n`, n x (`u32 len`, ASCII) | region keys (187) |
| `u32 n` segments (2486) | `u8 x 4` flags, `u32 n` curves of 16 bytes: `u16 X0, u16 Z0`, `i16 dx1, dz1`, `i16 dx2, dz2`, `u16 X3, u16 Z3` |
| 2 x route table | `u32 n` x (`u32 region A`, `u32 region B`, `u32 n` + n x `u32` refs = 2 · segment + reversed); table 0 the land legs (1137), 1 the sea legs (1739) |
| `u32` | CRC-32 of every byte before it (not read by the loader) |

A point (fns 0x108445f0, 0x1083a780, 0x1083a740; single precision): x = X · a · 0.25 - a, z = Z · b · 0.25 - b, with a = width /
(1.5 (columns - 1)) (678.5 / 1522.5) and b = height / (2 rows + 1) (the grid's +0x64, +0x68); a handle (dx · a · 0.25, dz · b · 0.25),
P1 = 0.01 · handle 1 + P0, P2 = 0.01 · handle 2 + P3. Bit-exact on the captures.

The drawn routes: the startpos's `CAMPAIGN_MODEL/CAMPAIGN_TRADE_MANAGER/INTERNATIONAL_TRADE_ROUTES`, per group the owner's runtime id and
its `INTERNATIONAL_TRADE_ROUTE`s (`^+1` the partner, `^+2` the path of region runtime ids, `^+3` a sea leg, then its two path indices). The
chosen faction's routes, each leg looked up both ways round in table 1 when it is the sea leg, else table 0; each segment once, in the
file's direction. Only segments whose flag byte 0 is 0 are drawn (fn 0x10aba960 makes no draw object for 1), whichever table named them
(segment 567, the Bosporus crossing, is a sea-leg segment with byte 0 set). A segment is one draw: its curves' lengths as above, uv in world
units scaled so the segment's total is a whole number (uv offset = length before · ⌈L⌉ / L, scale = length · ⌈L⌉ / L); 16 vertices a curve
by arc length as the borders', the last curve 17. Half-width 0.05, colour (240, 200, 20).

## Check (`borders_check`)

Capture A (turn 1, the Western Roman Empire):

- land borders: 950 curves in 23 draws, all exact to the dump's precision (uv 3e-7, vertex t 3.9e-6), colours 23 / 23;
- sea borders: 32 curves in 4 draws, exact, colours 4 / 4;
- trade routes: the 19 draws are segments 1707-1713, 1856-1859, 2100-2107, exact (uv 2.9e-6, t 4.4e-6).

The later captures, all curves: B 555 + 12 sea, rain 67 + 7, w1 58 + 7.

## Known gaps

- The default faction: no data names it (see Colours).
