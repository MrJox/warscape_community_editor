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
- **The ground** is the terrain height at the wall's base, measured along the outer face's base line; each flat run of the walkway is
  held to the ground at its middle.
- **The terrain is not touched**: the wall adapts to it.
- **Gates and bastions are placed first** (as today); a wall runs between them. No generated towers or bastions in this iteration.
- **A free end is closed with the kit's endcap.**
- **Vertices are smooth or sharp**: smooth bends the wall along a curve; sharp is a mitred corner. A vertex whose internal angle is at
  most 90 degrees (a right angle or sharper) is always sharp (user, review of 2026-10-24; first 25).
- **Generated pieces have variable lengths** (vanilla has longer pieces too).
- **Destruction levels are generated; collapse animations are not** (a piece swaps to its next level). Revisit later.
- **The spline is a custom element of the tile model's scene.** TEd will not read it; that is accepted (the model is separate from the TEd
  input for this).
- **The wall is not editable piece by piece.** Its spline and parameters are; the pieces are regenerated from them.
- **Live preview while editing.** Buildings and the pack are generated only when the tile is exported to a pack: one pack per tile for now
  (configurable later). Later: a cache that shares pieces between walls that fit, for pack size.
- **Steps on 15 m walls come from the stepped bastion's ramp** (`_straight_stepped_up/down_10m_square_bastion_a`: 10 m over about 12
  m, inner side, z 5..9), scaled in y to the rise needed.
- **Access on 15 m walls: a door section every 200 m** along the run, placed only where the units' landing at its ground end is walkable:
  not in a no-go region and no hard collision in a 20 x 20 m square around it. Where it is not, that door section is skipped.
- **First iteration: the 8 m and 15 m families.** TDD's Rohan and cliff walls later.
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
- **No pipes, no docking lines, docking points 0** in the logic files of all three (the docking lines are in the database, see "The
  `models_building` row"). Pieces join because their platforms meet at the ends (x = 0 and x = 20): confirmed in game (phase 0), units
  walk from a bent piece onto vanilla neighbours.
- **Firing lines (`ef_lines`)**: per metre a `HighWall` line on the walkway (0.5 m long, facing -z) and two `Overflow` lines (60 in the
  intact straight, 58 damaged, 0 destroyed).
- **Collision outlines**: `hard01` (the palisade's footprint, a quad at z -1.8..-2.8; the damaged level's is ragged), `ground_ad` (a line at
  z = -2.5).
- **`orange_thingies`**: per level a 2D outline in x/z (vertex type 2). In the 8 m wall it equals the palisade's hard footprint; in the
  15 m wall it is the whole footprint (z -2..10.5), so it is a footprint, not a docking strip.
- **Levels:** intact (5 platforms), damaged (17), destroyed (28: the breach's rubble ramp; no collision, no firing lines). The destroyed
  level attaches `barbarian_fort_bridgingcolumn_right` at x = 0 and `_left` at x = 20 (`file_refs`); the intact one a `torch_ground_post`.
- **Zones** (`zone_tech.xml`): one zone region, the footprint rectangle x -0.2..20.2, z -2..2.5.
- **Writers exist**: rpfm_lib encodes `Cs2Parsed`, `RigidModel` and DB tables; the base files of both families write back byte for byte
  (meshes, and logic files through their serde form).

### The meshes' frame (phase 0)

A mesh's vertices are stored around its material's pivot (`v_pivot`, the centre of its box, e.g. (10.0, 3.6, 4.2) for the barbarian
straight): the logic file's frame is vertex + pivot, and the meshes' `min_bb` / `max_bb` are in it. A warp works in that frame and keeps
the pivot. The walls are made of long triangles (20 m along x for both straights, 24 m in the 15 m rubble): they are cut every metre
before a bend, or a bent piece draws its faces as chords.

### The `models_building` row (phase 0)

The game's `models_building` row of a building is more than a name (columns `display_path`, `key`, `logic_file`, `unknown`, `EFLines`,
`DockingLines`; the kit's table has other columns):

- `logic_file`: `buildings\<key>\<key>_tech.cs2.parsed`, the logic file the game reads. A copy must point at its own.
- `EFLines` (bytes): the firing lines again: u32 count, per line a name (u16 length, UTF-16), u32 action, start, end, direction (3 f32 each,
  the direction a unit vector). The barbarian straight: 118 lines (60 intact, 58 damaged).
- `DockingLines` (bytes): **where ladders and towers dock**: u32 count, per line a name, start, end and direction in x / z (2 f32 each,
  facing out). A straight has one line along its outer face (barbarian: (19.93, -2.5) to (0.07, -2.5); 15 m: (10, 0) to (-10, 0)). CA's
  curved pieces have a polyline of short lines (`barbarian_fort_curve_20m_radius` 6 of about 5 m, `_60m_radius` 3 of about 10 m, the kinks
  4); **the step pieces have none**; gateways two tiny ones (0.26 m). A bent piece's line is cut into lines of at most 5 m
  (`wce_walls::lines`).

The rows of `barbarian_fort_straight` and `roman_fort_straight_with_doors_a` are the same in `data.pack` and TDD's main pack; TDD's
`@tdd_dev_buildings` pack overrides `barbarian_fort_straight`'s files (one more platform in the damaged level) but not its row.

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
- **Access in 15 m and Rohan walls is spliced at intervals**: the door section of `_with_doors_a` (or the ladders of `_ladders`), see
  the decisions. The pipes' ground ends follow the terrain, their walkway ends the walkway.
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
access section and its interval: `_with_doors_a`, `_ladders`), its height-change source (`_step_up_a` / `_step_down_a`, the stepped bastions'
ramps, `_step_up`), its endcaps (`_endcap_left` / `_right`) and bridging columns. Derived from the key's family
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
- **Steps:** the step piece's step section (8 m: x 2..5 of `_step_up_a`; 15 m: the stepped bastion's ramp) is spliced in: mesh,
  platforms, firing lines and collision, scaled in y to the rise. Steps down use the `_down` piece's section. For the 15 m ramp, which
  of the bastion's triangles belong to the ramp and the wall around it (and not the bastion) is to be worked out in phase 4.
- **Access (15 m):** the door section of `_with_doors_a` spliced every 200 m where its landing is walkable (decisions). The doors' pipes:
  the ground ends (y 0..0.5) on the terrain, the walkway ends on the walkway. The check reads the tile's no-go map and the hard
  collision outlines of the buildings already placed (generated walls included).
- **Foot:** above a split height the geometry is rigid with the walkway. Below it, the geometry is stretched so the buried bottom
  (y = -2) stays 2 m under the terrain at that x and z. The split comes from the base's ground contact, not a fixed y: the barbarian
  bank's inner edge (y = 4) must still meet the ground where units walk onto it. See "To confirm".
- **Subdivision before warping:** platforms, collision outlines, the outer strip and the zone region span the piece (20 m quads). They are
  cut every metre along x first, so bent pieces keep their shape. Firing lines are short and only moved and turned.
- **Per level** (intact, damaged, destroyed): the same warp on each level's mesh and logic. Collapse animations are dropped. Bridging columns
  are attached only at the piece's two ends (the copies' inner ones are dropped). Torches are kept per copy.
- **Outputs per generated building:** `rigid_model_v2` per level (rpfm's `RigidModel`, materials and textures of the base unchanged), the
  `_tech.cs2.parsed`, the `zone_tech.xml`, and rows in `battlefield_buildings` and `models_building` copied from the base with the new key,
  model and logic file path, and the row's firing and docking lines warped like the logic file (no docking line on a step section, as
  CA's step pieces). Keys: `wce_<tile>_<wall>_<n>`. UVs are kept, so the foot's texture stretches where it is extended; accepted for now.

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
   **Status (2026-10-24): done.** In game (user, 2026-10-24, Edoras and Dale): the copies and both bent runs load, look right and work
   (units on them and across the joints with vanilla pieces, breaching). So pieces join by their platforms meeting, and a generated
   building's levels swap with no collapse. **Not tested: ladders and towers docking** (the bent pieces' split docking lines): it needs the
   tile's siege AI set up, so it is checked with the SiegeAI generator. `cargo run --release -p wce_walls --example wall_spike --
   <game> <rpfm schema_att.ron> <out.pack> --mods <wce.ini's mod_packs>` writes `wce_spike_wall8_copy` / `_bent` and
   `wce_spike_wall15_copy` / `_bent` (bent 10 degrees about their start, no collapses) with their `battlefield_buildings` and
   `models_building` rows. In the editor (overlay of the pack's files, `terrain_rugged_hills_desert/1x1/aa`) a vanilla straight, the bent
   piece and a vanilla straight turned at its end meet with no step, for both families. To place the next piece after a bent one: its start
   is the bent piece's end, at (19.899, 1.741) in the bent piece's frame (barbarian, start at x = 0; the 15 m piece: (9.899, 1.741), start at
   x = -10), turned by the bend; the editor's orientation turns +x towards -z, so that is `orientation -10`.
   To check in game: units onto both bent pieces and across both joints (bank / doors), ladders and a tower on the bent pieces (the
   docking lines), a breach of a bent piece (its levels swap with no collapse), the copies behaving like the vanilla pieces.
   The test maps: `cargo run --release -p wce_walls --example wall_test_maps -- <game> <wce_wall_spike.pack> <out.pack> <workspace>`
   (with `XDG_CONFIG_HOME` at a folder whose `rpfm/schemas/schema_att.ron` rpfm_lib reads) decompiles Edoras (8 m) and Dale (15 m)
   (`tdd_settlement_middle_men_cities/middle_men_city_{edoras,dale}/medium`, settlement level 2 for every TDD culture; TDD's presets
   table replaces the game's, so the vanilla settlements can't be picked with TDD; `--vanilla`: `nordic_city_b/medium` and
   `western_city_b/medium`, Uburzis / Vesontio at level 2 with a barbarian / Roman defender), renames two of the base's instances in the ring to the copy, builds a run (vanilla
   straight, bent piece, vanilla straight turned at its end) on the flat ground nearest the ring that no building or tree (the tile's
   tree lists) is within 10 m of, compiles, and packs the two tiles'
   `building_list` with the spike pack. Only the building list: an unedited decompile and compile gives it back record for record (12 781
   of 12 781), and the edited ones differ from CA's by exactly the two renamed and three added records; the other files would be our
   compile's markers and terrain instead of CA's (the terrain fingerprint covers every building, though walls don't stamp it).
1. **Model and editing:** the `WALL_SPLINE` node, TEd read/write of the custom element, the editing tools, the solver's centre line and
   height profile drawn as lines (no meshes yet).
   **Status (2026-10-24): done.** As built:
   - **Storage:** `WALL_SPLINE` is stored like the polygons (`<points>` of `<point position="x,y,z"/>`, `polygon_closed="0"`) with
     `wall_base`, `wall_tolerance`, `wall_flip` and the points' `sharp`, so the polygon tools (draw, handles, insert, remove, undo) edit
     it and the TEd format reads and writes it with no unknown attribute (`attila::schema`; the round trip gate passes on a tile with a
     wall); the compile ignores it (it picks elements by class).
   - **Solver** (`wce_walls::solver`, pure, unit tested): corners = the ends, the marked points and the points under 25 degrees; between
     them a centripetal Catmull-Rom, sampled every 0.5 scene units; the outer foot on the base's docking line (`wce_walls::profile`);
     runs held to the ground at their middle while the ground stays within the tolerance (at least 4 units long); step sections of the
     base's rise and run, chained `step_gap` apart, centred on the boundary between runs; warnings when they don't fit.
   - **Attached ends:** not stored: an end is attached when it lies on a fort building's end (within 0.5 scene units), and is put there
     when placed or moved within 8 (4 m in the editor). A building end is its model's x extent at z = 0 (the models' bounds stand in for
     the pieces' frames; the logic files' boxes in phase 2). An attached end's run is at the building's level. A wall does not follow a
     moved gate: its end is then free until moved back onto it.
   - **Editor:** `WALL_SPLINE` in the polygon types (with a base wall combo), the layout drawn with the outline, the panel's summary and
     "sharp corner" check box, the Inspector rows (`wce_tile::properties`), redrawn after terrain strokes, building moves and model
     loads. Commands `wall_sharp`, `wall_info`, `assert_wall`; `scripts/tile_walls.txt` in `tile_check.sh`.
2. **Generator, straight runs:** copies, scaling, the foot, all three levels, the logic file, the zone file; the preview through the
   overlay.
   **Status (2026-10-24): done.** As built:
   - **Pieces** (`wce_walls::pieces`): between the ends and corners, a stretch is cut into `round(length / base length)` pieces (at least
     one), one base each, scaled along x to fit (no multi-copy pieces: every piece is about the base's length, like the vanilla walls).
   - **Warp** (`PieceWarp`): the base's x along the centre line by arc length, z across it, y over the layout's level; curves bend the
     piece already (the general path warp; phase 3 adds the mitres). Step sections are, until phase 4, a shear of the mesh over the
     ramp. Below the foot (`Profile::foot`: 8 m y -2..0, 15 m y 0..2) the mesh stretches so its bottom is `bury` (2 / 0.5) under lower
     ground. The output is in the piece's own frame (origin on the centre line at its start, at the level there, x along the wall), so
     the meshes keep the base's size and f16 precision.
   - **Flip** lays the wall from its other end (a rotation of the pieces; a mirror would turn the meshes inside out).
   - **Cuts** (`warp::Cuts`): triangles and platforms are cut along x as often as the tightest curve of the piece needs (a chord within
     2 cm, 1 to 20 units apart) and at the step ramps' ends; the cut vertices are merged. A piece on a radius-40 S curve is 1.9 to 3
     times its base (8 m: 1.6 to 2.6 MB against 0.85; 15 m: 2.3 to 3.7 against 1.3); straight pieces are about the base. Sharing pieces
     between walls is the later cache step.
   - **Full pieces** (`make_piece`): every level's standing meshes, the logic file and the zone file, collapses left out.
     `cargo run --release -p wce_walls --example wall_generate_check -- <game> --mods <...>`: an S curve over a slope and a dip, both
     families, generated twice (the same bytes), every firing line on a platform strip, every file reads back (35-75 ms per piece).
   - **Preview** (`tile_document::walls`): the intact meshes of every piece, made 300 ms after the wall (or its ground, or what it is
     attached to) last changed, kept in `wce_vfs::generated` (files in memory every `Files` reads first) under keys that change with each
     remake, drawn as the wall's entities (a click selects the wall).
3. **Bends and mitres:** the curve warp with subdivision, the corner shear, attachments to gates and bastions.
   **Status (2026-10-24): done** (the curve warp and subdivision came with phase 2). As built:
   - **Mitres** (`PieceWarp::new`): a piece at a section's end (a corner, or an attached end) is sheared along the wall by
     `s' = s + z * shear`, the shear going from the start's value to the end's along the piece, so the end face lies on the corner's
     bisector (normal: the sum of the two sections' directions; both pieces meet on it) or on the face of the building it is attached
     to (normal: the building's x axis). Past its section's ends a piece goes on straight with the section's own end direction (taken
     from its last unit, so the corner's averaging doesn't bend it). The shear is clamped to 4 per unit (a turn of about 150 degrees);
     the solver warns about sharper corners. Unit test: at a right angle both pieces' end faces land on the bisector, point for point.
   - **Exact ends**: a fort building's ends are its logic file's box (`<key>_tech.cs2.parsed`, the frame the vanilla walls join on),
     read once per key; the models' bounds stay the fallback. An attached end whose outer side faces the other way from the building's
     gets a warning (flip the wall).
   - **Steps**: a step chain is kept inside its two runs (centred on their boundary, shifted when that would go past them): next to a
     short attached run it used to start before the wall, which then began at the chain's middle level instead of the building's.
     Still open (phase 4): on ground steeper than the steps can follow, two chains can overlap.
4. **Steps:** splicing the step sections.
   **Status (2026-10-24): done.** As built (the user chose, for 15 m walls, the bastion's stair over a sloped wall):
   - **Sources** (`Profile`): every base names its culture's step pieces (`*_step_up_a` / `_step_down_a`, the stepped bastions), the x
     range of their stair (8 m: 2..5, the first ramp; 15 m: the bastion's ramp, -5.8..6.2 up and -6.2..5.8 down), the walkway's height
     where it starts, and what of it is brought: 8 m, the walkway and the palisade (z -3.5..3, which replace the base's there; the base's
     bank stays and slopes over the step, as the step piece's own does: the step piece's bank has another cross-section, and cutting the
     base's open showed its inside); 15 m, the wooden stair only (z 5.1..8.9, within 0.9 of its slope: without that limit the stair
     well's stone walls came along as wedges), added on the base's walkway, whose wall slopes over the step.
   - **Splice** (`SlabWarp`, `mesh::warp_mesh_with`, `logic::warp_logic_with`): the stair's x range onto the step's distances, its rise
     made the step's (its y shifted so it starts at the level before and ends at the level after), z across as the piece, the foot as
     the piece's; its meshes' blocks (their materials kept), platforms and firing lines added to the piece's first piece at each level
     (the step piece's level of the same number, or its last). A step across two pieces is brought in two halves.
   - **Steep ground:** chains of steps that would overlap are moved on one after the other (and back from the wall's end), with a
     warning when they still overlap.
   - **Doors** (15 m; `pieces::door_pieces`): a door section (`<culture>_fort_straight_with_doors_a`) at 100, then every 200 units, on
     the piece nearest, if it is a plain piece (no step, not at a corner or an end) and the 20 x 20 square inside the wall in front of
     its doors is free: no point of a 5 x 5 grid in a shown no-go polygon, no shown building within (its marker radius); else skipped,
     the panel and `wall_info` say why. The doors' pipes' feet (points under 1 unit) are put on the terrain (`Warp::ground_point`).
     Units: the scene's (the models'), so 200 and 20 are in the units the vanilla walls are 20 long in.
   - **Checks:** `wall_generate_check` has three cases (an S curve with a sharp corner over a slope and a dip, both families; a flat
     15 m wall of 400 with two doors): deterministic, firing lines on platforms, every door's foot on the ground (24 of 24);
     `scripts/tile_walls.txt` adds the door count with and without a no-go over a landing (`assert_wall_doors`). With steps, a piece
     takes 130-200 ms to make (the stair's cuts).
5. **Export:** the pack, the DB rows, the compile reading generated buildings.
   **Status (2026-10-24): done.** As built (`wce_walls::export`, `wce_walls::scene`):
   - **Shared scene code** (`wce_walls::scene`): walls, anchors, layouts, doors and lowering read a `TedScene`, for the editor and the
     exporter alike (the editor's `walls.rs` calls it with its own frame extents and model radii).
   - **Lowering** (`lower_walls`): every wall is laid out from the project's scene and height map; each piece is generated in full
     (`make_piece`: every level, logic and zone files) from its base or the door section, and the wall is replaced, in a copy of the
     scene, by a `BUILDING_INSTANCE` per piece in its layer (`TedScene::add_instance`; a placed fort wall's attributes: breachable,
     dockable, `destruction_level` 0, freeform, the piece's frame as its transform). Keys: `wce_<tile>_<variation>_<wall>_<piece>`.
   - **Rows:** the bases' rows copied under the keys (`db::copy_rows`, now over any `Files`: the first table file by path that has a row
     wins; the game's and TDD's rows of the bases are the same). The `models_building` row's `EFLines` are rebuilt from the piece's
     logic file (the action codes learnt by pairing each source's row and logic file by line name: base, door section, step pieces),
     its `DockingLines` are the base's, warped, cut in lines of at most 5 units, none over the step sections.
   - **Compile:** `workflow::save_exported_prepared` compiles the project with its files changed in memory (the lowered `tile.ted`; the
     project on disk keeps its walls); the new keys take their bases' destruction levels and thresholds.
   - **Pack:** `<out>/wce_<tile folder>.pack` with what the compile wrote for the tile, the tile database in effect with its entry, the
     pieces' folders and the rows (`battlefield_buildings_tables`, `models_building_tables`). The editor's `save_exported` does all of it
     for a tile with walls; it needs rpfm's Attila schema (`rpfm_schema` in the `[tools.tile]` settings or `WCE_RPFM_SCHEMA`, else rpfm's own place).
   - **Check:** `cargo run --release -p wce_walls --example wall_pack_check -- <pack> <schema>`: every generated key has its meshes,
     logic and zone files, both rows (its own logic file path, firing and docking lines) and one instance in the building list. On a
     scratch Edoras with an 8 m wall of 7 pieces and a corner: all passed (118 firing lines, 4 docking lines each); the export took 16 s.
   - **Not checked in game yet.**
6. **Kits:** the kit table for every culture; a check that every base straight generates (a Cargo example as a gate).

Each phase: a Cargo example that generates a fixed wall and compares it with a reference, and an in-game check for those that change what
the game sees (0, 2, 3, 4).

## Review (user, 2026-10-24, before the in-game test of phase 5)

- **Steps: authored rises only.** Stretching geometry vertically (the 15 m wall sheared over each step, the 8 m stair rescaled to the
  rise) gave inclined, misaligned textures, and 15 m walls piled stairs at one end (each ~1 m rise asked for a full 12-unit stair, so
  the chains ran into each other). Now nothing is stretched vertically: runs are level and whole steps apart (a run goes on while the
  ground stays within half a step of it, or the tolerance when larger; the next is the nearest whole number of steps), and a step section
  is its source at its own size, its whole cross-section replacing the base over its length (8 m: one stair of the step piece, x 2..5,
  2.5 up over 3, chained 1 apart; 15 m: the whole stepped bastion, 10 up over 20, like vanilla). The walkway can be half a step off the
  ground between steps (1.25 m / 5 m). An attached far end that is not a whole number of steps from the run before it is noted.
- **Corners: right angles and sharper**, and the joint aligned on both sides. The old mitre sheared the whole end of a piece onto the
  bisector, which on the inner side pulled it backwards (at sharp corners past itself: the back of one piece over the front of the other).
  Now a piece is stretched onto the bisector only on its outer side and cut by it on its inner side (`Warp::clip_planes`: the meshes'
  triangles, the platforms, the collision outlines, the footprint, the zones, the docking lines and the pipes are clipped to the
  section's end faces), so it never folds; the shear limit went from 4 to 10 per unit (an internal angle down to about 12 degrees).
  Unit tests: the right angle's ends meet on the bisector on the outer side, the inner side is cut there, an acute corner doesn't fold.

## Review, second round (user, 2026-10-24)

- **8 m corners with the bank outside the turn** were a big stretched wedge: the last piece was stretched up to 12 units along the wall
  on its outer side to reach the bisector. Now nothing is stretched: the outer side is filled by copies of the base going on straight
  past the piece's end (`PieceWarp::overhangs`, `warp::Shifted`: as many lengths as the face reaches across the wall's depth), cut by
  the bisector like the inner side; meshes, platforms and firing lines of every piece of the base (`logic::Splice::per_piece`), and the
  row's docking lines.
- **Broken bank behind 8 m stairs**: the stair splice took the step piece's whole cross-section over 3 units of its 20, and its bank (shaped
  over its whole length) matched neither side. Now only its walkway and palisade (z -3.5..3) come in; the straight's own bank stays,
  sloped over the stair (grass).
- **Steps: option B** (user's choice; the earlier complaint was local warping at the stairs, not slope). The phase 4 warping came from the
  stair being squashed to the rise (a 2.5 stair into ~1 m), from 15 m chains piling up (each small rise took a full stair), and from the
  stair and the wall around it being sheared differently. Now:
  - a step uses the start of its stair, as much of it as it rises, at its own slope and size: cut, never squashed (`steps_in`: the
    stair's x range shortened to the rise's share of the stair's rise); a rise over one stair chains stairs, the rise shared equally;
  - the wall slopes over a step at most `max_slope` (8 m: the stair's own slope, 2.5 / 3: only its grass bank slopes, the stair replaces
    walkway and palisade; 15 m: 0.4, the stone ramping gently, the bastion's wooden stair laid on it, lengthened along the wall to the
    ramp but never in height): `solver::step_length`;
  - runs between steps are at least `min_run` long (8 m 10, 15 m 20 units): no pile-ups; levels any height again (runs at the ground
    within the tolerance).
  Points only move vertically on a slope, so verticals stay vertical (merlons, posts). Rendered on the desert hills: clean stairs on
  both families; a thin dark sliver at the bottom edge of one 8 m stair remains to look at.

## Review, third round (user, 2026-10-24)

- **Caps** were never made: a free end now gets the base's own end pieces (`Profile::cap_start` / `cap_end`). 8 m: the endcaps
  (`*_endcap_left` / `_right`, 2.6 outside the wall's frame), placed past the ends as they are, vanilla instances (`pieces::cap_instances`;
  previewed from the game's meshes, exported as instances of the game's keys). 15 m: the tower ends (`*_fort_tower_end_left` / `_right`,
  a tower and the start of the walkway in a 20-unit slot) take the first and last slots and are generated like any piece (bent, foot to
  the ground, rows copied).
- **15 m whole steps**: a climb of about 10 (0.7 to 1.3 of `whole_rise`) over boundaries at most 40 apart, on a stretch that turns at
  most 4 degrees over 20, is one stepped bastion (`*_stepped_up/down_10m_square_bastion_a`) as vanilla: exactly 10, its own piece of
  20 (`Piece::base`), the wall's level under it the lower one (the bastion climbs by itself); the runs it spans are merged. On Dale's
  slopes the straight walls take several in a row.
- **Steps at corners and on curves**: a chain is placed inside its runs but not within the wall's depth + 2 of a corner or an attached
  end (their mitres), the straightest place first (a penalty past 6 degrees of turn over its length), then the nearest to the boundary;
  when no place is free it is centred as before and noted. The corner itself is flat, so the mitre and the overhang copies line up.

## Review, fourth round (user, 2026-10-24)

- **Caps are the game's pieces as they are** (not bent, not fitted to the ground), joined to the wall's end: their joining x
  (`Profile::cap_join`: 8 m endcaps 0, 15 m tower ends 10 / -10, the end of their walkway) on the wall's end, along the end's direction,
  at the end's level. The 15 m tower ends no longer take a slot of the wall.
- **Slanted faces beside stepped bastions**: a piece's level was interpolated between samples, so the half unit next to a bastion took
  the bastion's level (a 10 m slope). The warp now reads the level exactly (`solver::wall_level`), from inside the piece (clamped to its
  own distances), and pieces' origins too. Test: the walkway beside a bastion stays level up to it.
- **Troughs between steps** (a step down and a step up meeting): a chain could take any place inside its two runs, so two neighbouring
  chains could squeeze the run between them to nothing. Now a chain stays between the middles of its two runs, a quarter of `min_run`
  from each (half a run of flat walkway between neighbouring steps). Test.

## Spacings (user, 2026-10-24)

Two attributes of the spline (Inspector rows; 0: none):

- **`wall_access_every`** (default 200): the spacing of the pieces that take units up the wall, if the wall has them (15 m: the door
  sections; 8 m walls have none, their bank is the way up). Replaces the fixed 200 (`pieces::door_pieces`).
- **`wall_defence_every`** (default 0): the spacing of the towers and bastions, one group (`Profile::defences`, used in turn): 15 m, the
  tower (`*_fort_tower_c_straight_with_extension`, gondor `gondor_fort_tower_c_straight`) then the bastion
  (`*_fort_straight_with_solid_squared_bastion`, gondor `gondor_fort_bastion`); 8 m, the curved bastion (`*_curved_bastion`; the
  watchtowers stand alone, not in a wall). For gameplay they are the game's pieces as they are (no terrain blending, not bent): a slot of
  exactly the base's length at the wall's level, at `every / 2` then every `every`, on the nearest place within half the spacing that is
  straight (4 degrees over its length), has no step, and keeps the wall's depth + 2 from corners and attached ends
  (`pieces::defence_slots`); else skipped with a note. `Piece::rigid`: placed as instances of the game's keys on export (no generated
  files, no copied rows), drawn from the game's meshes in the preview. Test: alternation, exact slots, origins, coverage, none on curves.

## To confirm

- **How units get onto an 8 m wall** from the ground: the bank's lowest platform is 4 m up. Checked in game on a vanilla wall (phase 0);
  if it is the bank, its lower edge must meet the terrain after the foot is stretched (phase 2).
- **Destruction granularity:** a piece of `k` copies breaches as one building (a longer breach). Accepted by "variable lengths";
  checked in game in phase 2.
