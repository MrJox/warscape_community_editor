# Tile editor: building splines (2026-11-13)

Draw a line, pick one or more buildings, and they are laid along it end to end: fences and palisades in a line, aligned and turned with it.
A Create panel entry, *Building spline (fences, palisades)*, in the Buildings category.

## Decisions (user, 2026-11-13)

- **A live spline**, as the procedural roads and walls are: a `WceBuildingSpline` (not TEd's: kept in tile.wce only). Its buildings are
  derived data, laid again whenever the line, its settings, the ground or the buildings' models change; never saved. A click on one of
  them picks the spline. An export (Save for Assembly Kit, the game's) writes them as plain `BUILDING_INSTANCE`s in the top layer
  *WCE building splines* (not for a spline in a layer that is not exported); Import from Assembly Kit drops that layer and puts the splines
  back, as it does for the walls.
- **Each building as long as its model, plus a gap**: its footprint (x and z) from its logic file's box (`<key>_tech.cs2.parsed`), else
  the scene's model bounds, else its piece files' vertices (LOD 0). It is laid along the longer of the two axes: a fence modelled along
  its z (`egyptian_highfence`) is turned a quarter (user, 2026-11-13: laid along x, it went across the line, packed by its thickness). The gap (metres) may be below 0 to overlap. A building more than half past the
  line's end is not laid.
- **Laid as the roads' fences are** (`road_layout`): its long axis along the line, on the chord of its stretch (from where it starts to where it
  ends), so a building whose pivot is at one end (`lowfence_3`) does not stick out of a bend.
- **Align to ground**: tilted to the slope under it, read end to end (`road_layout::ground_tilt`, shared with the roads' fences), its
  middle on the ground. Off: upright, its pivot on the ground. **Tilt across slope** (off by default; user, 2026-11-13: fences leaned
  sideways on hillsides) also leans it with the slope across it, read 0.5 m each way.
- **Random**: each building picked from the list by the seed, else the list repeats in order.
- **Extras**: closed loop, damage chance (a random damaged `destruction_level`, as `road_fence_damage`), flip chance (end for end), turn
  and side jitter, side offset (to the right as the line runs from its first point; below 0 to its left).
- **Joiners** (user, 2026-11-13): an optional second list of buildings (posts, pillars) laid on each joint between two of the others, to
  hide the gap and make the line less regular. One between where the two buildings it joins end and start as they stand (after their turn and
  side jitter: user, 2026-11-13, a post on the drawn line stood off a turned fence), its long axis along the mean of their ways, tilted
  as they are, not turned by a jitter of its own; picked as the buildings are (in order, or by the seed), with the flip and damage chances. They do not change the spacing. A closed line has one on the joint that closes it; *Joiners at the ends* puts one at
  each end of an open line too.

## Where it is

- `wce_tile::building_spline`: the settings, `lay` (the layout, unit-tested), the instances' text.
- `wce_tile_editor::tile_document::building_splines`: settings from the attributes, the curve (handles as the roads'), extents, laying per
  frame when the document, the models or the ground changed, drawing, the export layer. `bspline_info` (script) logs what is laid.
- `wce_tile_tool::building_spline_ui`: the Inspector below the grid: the list of buildings (remove, move), a search to add one, and the keys
  of the buildings selected in the scene added in one click; the same for the joiners.

## Open

- A building wider across than along its run (a gate seen end on) is turned the wrong way: no setting to force the axis yet.
- Not in the technology overlay or the radar.
