# Tile editor: procedural walls (plan, 2026-10-24)

A wall drawn as a spline between gates and bastions placed by hand. The editor generates buildings that follow the spline exactly: bent
along its curves, mitred at its sharp corners, stepped on slopes, their foot on the terrain. They are new buildings, written into a pack of
the tile when the tile is exported. Siege battles must work on them: units on the walls, ladders and towers, breaching.

## Decisions (user, 2026-10-24)

- **Follow the line exactly.** No snapping to the kits' fixed lengths and angles (that is the restriction this feature removes).
- **Slopes are climbed by steps**, never by a ramp of the walkway.
- **The walkway keeps the base piece's height over the ground** (siege equipment must dock). Steps come when the drift passes a tolerance
  (default 0.5 m, per wall); a step rises what is needed, up to the culture's step piece's rise; steep ground chains steps. To revisit after
  gameplay tests.
- **The ground** is the terrain height at the wall's base, measured at the middle point of the outer face.
- **The terrain is not touched**: the wall adapts to it.
- **Gates and bastions are placed first** (as today); a wall runs between them. No generated towers or bastions in this iteration.
- **A free end is closed with the kit's endcap.**
- **Vertices are smooth or sharp**: smooth bends the wall along a curve; sharp is a mitred corner. Turns under 25 degrees are sharp
  (see "To confirm").
- **Generated pieces have variable lengths** (vanilla has longer pieces too).
- **Destruction levels are generated; collapse animations are not** (a piece swaps to its next level). Revisit later.
- **The spline is a custom element of the tile model's scene.** TEd will not read it; that is accepted (the model is separate from the TEd
  input for this).
- **The wall is not editable piece by piece.** Its spline and parameters are; the pieces are regenerated from them.
- **Live preview while editing.** Buildings and the pack are generated only when the tile is exported to a pack: one pack per tile for now
  (configurable later). Later: a cache that shares pieces between walls that fit, for pack size.
- **Not in this iteration:** siege AI hints along the wall (a later SiegeAI generator), towers / bastions, terrain shaping, collapse
  animations.

## Findings: what a wall piece is (barbarian kit, 2026-10-24)

From `models2.pack`, `rigidmodels/buildings/barbarian_fort_{straight,step_up_a,step_down_a}/`, the logic files read with rpfm_lib's
`Cs2Parsed` (version 11):

- **Files per piece:** `<key>_piece01_destructNN.rigid_model_v2` (01..03 for the straight, 01..02 for the steps), the collapses
  (`_destructNN_anim` + `.anim` + `.meta`), `<key>_tech.cs2.parsed`, `<key>_zone_tech.xml`. DB rows: `battlefield_buildings` (category
  `fort_wall`, hit points, `destruct_thresholds`, `model`) and `models_building` (`model_file`, `logic_file`, `display_path`).
- **Frame:** the straight runs 0..20 m along x; the outer side is -z (the palisade, hard collision and the outer strip at z -1.8..-2.8);
  the ground is y = 0, the mesh is buried to y = -2.
- **Walkway: platforms** (flat or sloped polygons, flags `false, true, false`). Straight: a flat walkway at y = 8, z -2.4..2.3, and the
  inner earth bank in four sloped strips down to y = 4 at z = 11.5. Step up: flat 0..2 at 8 m, a ramp 2..5 to 10.5 m, flat 5..15, a ramp
  15..18 to 13 m, flat 18..20; the ramps are about 40 degrees (2.5 m over 3 m) and the bank is cut into triangles that follow them. **The
  stairs are only visual**: the pathfinding surface is a slope.
- **No pipes, no docking lines, docking points 0** in all three. **ASSUMPTION**: pieces join because their platforms meet at the ends
  (x = 0 and x = 20). Check in game: a generated run, units walking from one generated piece to the next and onto a vanilla gate.
- **Firing lines (`ef_lines`)**: per metre a `HighWall` line on the walkway (0.5 m long, facing -z) and two `Overflow` lines (60 in the
  intact straight, 58 damaged, 0 destroyed).
- **Collision outlines**: `hard01` (the palisade's footprint, a quad at z -1.8..-2.8; the damaged level's is ragged), `ground_ad` (a line at
  z = -2.5).
- **`orange_thingies`**: per level a 2D outline in x/z (vertex type 2). In the 8 m wall it equals the palisade's hard footprint; in the
  15 m wall it is the whole footprint (z -2..10.5), so it is a footprint, not a docking strip. How ladders and towers find the wall is not
  known (no docking data in any wall); check in game: ladders and a siege tower on a generated, bent piece.
- **Levels:** intact (5 platforms), damaged (17), destroyed (28: the breach's rubble ramp; no collision, no firing lines). The destroyed
  level attaches `barbarian_fort_bridgingcolumn_right` at x = 0 and `_left` at x = 20 (`file_refs`); the intact one a `torch_ground_post`.
- **Zones** (`zone_tech.xml`): one zone region, the footprint rectangle x -0.2..20.2, z -2..2.5.
- **Writers exist**: rpfm_lib encodes `Cs2Parsed`, `RigidModel` and DB tables.

## Findings: the wall families (user, and every straight of the game's and TDD's packs, 2026-10-24)

The user: Attila has two kinds of walls, 8 m and 15 m; the 8 m ones have a ramp behind them, the 15 m ones hidden steps where units climb
at regular intervals; the TDD files may have others. The straights' logic files agree:

| Family | Bases | Frame | Walkway | Way up | Height changes | Levels |
|---|---|---|---|---|---|---|
| 8 m (earth bank + palisade) | `barbarian_fort_straight`, `{eastern,sassanid,western}_fort_small_wall_straight`, TDD `khazad_fort_wall_small_straight`, `mordor_fort_small_wall_straight` | x 0..20, one piece | y 8, z -2.4..2.3 | the bank's sloped platforms behind | `_step_up_a` / `_step_down_a` (5 m, two 2.5 m ramps) | 3 |
| 15 m (stone) | `{roman,eastern,sassanid,greek,egyptian,persian}_fort_straight`, TDD `{easterling,gondorean,gondor}_fort_straight` | x -10..10; piece01 the wall, piece02 / piece03 the end strips of the walkway (x -10..-8 and 8..10, z 0.5..3, 2 levels) | y 15, z 0.5..9 | none in the straight: `_straight_with_doors_a` has 6 `WallDoorTeleport` pipes (inner door at the ground, z 13, to the walkway, z 6..8), the towers 6 more | only `_straight_stepped_up/down_10m_square_bastion_a`: a bastion with a 10 m ramp (15 to 25 m); no step piece | 3 (end strips 2) |
| TDD Rohan (timber) | `rohan_wall_straight` (+ `_ladders`, `_ladders_attacker`, `_ladders_definitive`, `_bastion`, `_step_up`) | x -10..10, 4 m thick | y 10, z -2..2, 16 platforms of 5 x 1 m | `_ladders*`: 4-5 `SiegeLadder2` pipes (ground to walkway) | `_step_up` (10 to 15 m) | 1 (no destruction) |
| TDD cliff | `persian_cliff_fort_straight` | x -10..10 | y 15, 2 platforms | ? | ? | 1, no firing lines |

`gondor_fort_straight`'s frame is x -10.5..10.8 (pieces overlap their neighbours). The 15 m firing lines are of other types (`LowWall`,
`Crenel{Left,Right}{Inner,Outer}`, `Overflow`). The way up is the gameplay part of each family: a generated run must keep it working.

Consequences:

- **The generator works from a description of the base, not from the barbarian layout**: its frame, walkway height and width, outer face,
  pieces and levels, its way up (bank platforms, door pipes, ladder pipes) and its height-change source. A **profile** per family, read
  from the base's files where possible and checked by hand per kit.
- **Access in 15 m and Rohan walls is spliced at intervals**: the door section of `_with_doors_a` (or the ladders of `_ladders`) every N
  metres. The pipes' ground ends follow the terrain, their walkway ends the walkway.
- **The 8 m bank must still reach the ground** after the foot is stretched (the bank's lowest platform is at y = 4, z = 11.5; how units
  get from the ground onto it is not clear from the platforms alone: check in game on a vanilla 8 m wall).

## Design

### The model: a `WALL_SPLINE` node

A node of the tile model's scene (class `WALL_SPLINE`, in a layer like any element), written by the TEd format as a custom element (TEd
ignores it; checked to read back unchanged in `tile.semantic()`):

- `base` (the straight's key; the culture's step piece and endcap come from a kit table, see below), `tolerance` (0.5 m),
- `start` / `end`: a free end, or attached to a building (the gate / bastion element and which of its ends),
- `points`: per vertex its position (x, z) and `sharp` (bool). Heights are not stored: they come from the terrain.

Compiling a tile lowers each `WALL_SPLINE` into `BUILDING_INSTANCE`s of generated keys (stage A sees ordinary buildings), so the rest of the
compile needs no change except where it reads building files (below).

### The kit table: one profile per base

Per base straight (see "The wall families"): its family profile (frame, walkway, outer face, pieces, levels), its way up (the bank, or the
access section and its interval: `_with_doors_a`, `_ladders`), its height-change source (`_step_up_a` / `_step_down_a`, `_step_up`, or
none: see "To confirm"), its endcaps (`_endcap_left` / `_right`) and bridging columns. Derived from the key's family
(`<culture>_fort_straight`, `<culture>_fort_small_wall_straight`, ...), with a hand table for exceptions (TDD's kits).

### The solver: spline to runs and pieces

1. **Centre line.** Smooth vertices: a curve through them (centripetal Catmull-Rom); sharp vertices and attached ends: corners. Sampled by
   arc length.
2. **Height.** The ground along the outer face's base line, which is offset from the centre line by the base's outer face offset (z = -1.8
   for the barbarian straight). The walkway profile: a staircase that stays within `tolerance` of the base's height over that ground. A step
   rises what is needed, up to the step piece's rise; flat runs between steps. At an attached end the height is the gate's end.
3. **Pieces.** Cut at the corners, then each stretch into pieces of variable length. A piece is `k` copies of the base, scaled along x by
   `L / (k * base length)` within a tolerance (start: 0.85..1.15), so the palisade's posts do not stretch visibly. A step goes inside a
   piece (a spliced section, below). Free ends get the endcap.
4. **Fixed ends.** Each piece's end faces must equal its neighbour's: the same height and the same mitre plane.

The solver runs on every edit (the preview); the shapes it makes for the preview are the ones the export writes.

### The generator: a piece from the base

A **warp** maps the base's frame to the world: x is the arc length along the stretch, z is the offset across it, y is the height.

- **Bend:** `p = C(x) + z * N(x) + (y + H(x)) * up`, with C the centre line, N its normal in the plane, H the walkway profile.
- **Mitre:** near a sharp corner the warp shears x so the end face lies on the corner's bisector. The two pieces meet with no gap on the
  inner or outer side, and their platforms share the edge.
- **Steps:** the step piece's step section (x 2..5 of `_step_up_a`) is spliced in: mesh, platforms, firing lines and collision, scaled in y
  to the rise. Steps down use `_step_down_a`'s section.
- **Foot:** above a split height the geometry is rigid with the walkway. Below it, the geometry is stretched so the buried bottom
  (y = -2) stays 2 m under the terrain at that x and z. The split comes from the base's ground contact, not a fixed y: the barbarian
  bank's inner edge (y = 4) must still meet the ground where units walk onto it. See "To confirm".
- **Subdivision before warping:** platforms, collision outlines, the outer strip and the zone region span the piece (20 m quads). They are
  cut every metre along x first, so bent pieces keep their shape. Firing lines are short and only moved and turned.
- **Per level** (intact, damaged, destroyed): the same warp on each level's mesh and logic. Collapse animations are dropped. Bridging columns
  are attached only at the piece's two ends (the copies' inner ones are dropped). Torches are kept per copy.
- **Outputs per generated building:** `rigid_model_v2` per level (rpfm's `RigidModel`, materials and textures of the base unchanged), the
  `_tech.cs2.parsed`, the `zone_tech.xml`, and rows in `battlefield_buildings` and `models_building` copied from the base with the new key
  and model. Keys: `wce_<tile>_<wall>_<n>`. UVs are kept, so the foot's texture stretches where it is extended; accepted for now.

### Preview in the editor

The generator runs in memory. Its meshes feed the model cache the way a pack's files do, through a generated-files overlay over the VFS.
The pieces are drawn but cannot be picked one by one: a click selects the wall. Gizmos edit the vertices (move, add, delete, smooth/sharp)
and the ends (attach to a gate or bastion end, or free). Undo works on the `WALL_SPLINE` node like any edit.

### Export

When the tile is exported to a pack, the wall's generated buildings go into the tile's pack: the files under
`rigidmodels/buildings/<key>/` and the two DB tables. The compile reads them like the game's packs: stage B's building height stamps and
`models_building` lookups, stage A's building tables, all through the same overlay.

## Phases

0. **Spike: what siege needs** (before writing the generator). Copy `barbarian_fort_straight` and `roman_fort_straight_with_doors_a`
   under new keys into a test pack: same files, a translated and slightly bent logic file, DB rows. Place them in a test tile and check in
   game: units walk onto them (the bank, the doors) and on to a vanilla neighbour, ladders and a tower dock, they breach. That settles the
   **ASSUMPTIONS** above before anything depends on them.
1. **Model and editing:** the `WALL_SPLINE` node, TEd read/write of the custom element, the editing tools, the solver's centre line and
   height profile drawn as lines (no meshes yet).
2. **Generator, straight runs:** copies, scaling, the foot, all three levels, the logic file, the zone file; the preview through the
   overlay.
3. **Bends and mitres:** the curve warp with subdivision, the corner shear, attachments to gates and bastions.
4. **Steps:** splicing the step sections.
5. **Export:** the pack, the DB rows, the compile reading generated buildings.
6. **Kits:** the kit table for every culture; a check that every base straight generates (a Cargo example as a gate).

Each phase: a Cargo example that generates a fixed wall and compares it with a reference, and an in-game check for those that change what
the game sees (0, 2, 3, 4).

## To confirm

- **Steps on 15 m walls:** the kit has no step piece; the only height change is the stepped bastion (10 m at once, placed by hand). Options
  in the user's hands: a step made by the generator (a short sheared section of the straight with a sloped walkway platform, about 40
  degrees like the 8 m steps), a step built from the stepped bastion's ramp, or new step art.
- **The access interval** of 15 m and Rohan walls: from the vanilla tiles (to measure), or a parameter of the wall.
- **Which families in the first iteration:** 8 m and 15 m, with Rohan and the cliff wall when the profile allows.

- **The 25 degree rule:** read here as "a vertex whose turn is under 25 degrees is drawn as a sharp corner unless marked smooth". It may
  have been meant the other way (a sharp turn becomes a corner). The user to confirm before phase 3.
- **"The middle point of the outer face"**: read here as the ground measured along the outer face's base line, with each flat run held to
  the ground at its middle. The user to confirm before phase 1's height profile.
- **How units get onto a wall** in each kit: the barbarian wall's inner bank, other kits' stairs and towers. If a kit's way up is inside
  another building, the foot rule is enough; if it is the wall's own bank, the bank's lower edge must meet the terrain (phase 2).
- **Destruction granularity:** a piece of `k` copies breaches as one building (a longer breach). Accepted by "variable lengths";
  checked in game in phase 2.
