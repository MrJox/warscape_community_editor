# Tile editor: walls of kit pieces (plan, 2026-11-12)

A `WceWallSpline` with *Procedural* off (`wall_procedural`, docs/editor.md) is laid with the game's own fort pieces instead of pieces
made on the fly. *Procedural* on: the wall as it works today (procedural_walls.md), unchanged.

This replaces the first attempt (`feature/combined_walls`, shelved 2026-11-12: walls sank on slopes, steps without ramps, switching kinds
lost the pieces, ends left open). Nothing of it is merged; what it learned is kept here as rules, and the code is written again phase by
phase, each phase reviewed by the user before the next. Ported already: `logic::ends` (the joints of TDD's gondor pieces, c8d3ea8a).

## Decisions (user, 2026-11-12)

- **The path, approximately**: the kit pieces follow the wall's line (the same centre line as the procedural wall: its handles, smooth and
  sharp points) as closely as their fixed sizes and turns allow. **No platform is ever broken**: pieces are whole, joint to joint.
- **Pieces**: straights, turns (corners, kinks, 45 degree joiners), **curves at any angle** (the 8 m cultures' 60 m curves, TDD's
  `barbarian_fort_600m_*`, the gondor 5 degree ones), **towers and bastions** at `wall_defence_every`, **door sections** at
  `wall_access_every`, **end caps** on free ends.
- **Snapping**: an end on a gate, tower or wall piece (a fort building's end, as the procedural wall's ends attach) starts or finishes on
  that building's joint. When the kit pieces cannot meet it cleanly (height, rotation, position off), **the last 1 or 2 pieces before it
  are procedural**: a procedural stretch from the last kit joint to the building's joint. Nowhere else does a kit wall go procedural.
- **Slopes: steps and the terrain**: the culture's step pieces where the ground changes level, and the ground reshaped under the wall
  (a height layer of the wall's own, as the roads have) so the pieces stand on it.
- **Live from the line**: what is saved is the line and its settings; the pieces are made again when they, or the ground under the wall,
  change, drawn as previews, and written as the kit's own buildings only when the tile is exported or saved for the Assembly Kit.

## Rules (from the first attempt's failures)

1. **One source of truth for the level**: a piece's base level is decided once, by the layout; the terrain layer sets the ground to that
   level under the piece, never the other way round. The layout reads the ground **without** the wall's own layer (no feedback).
2. **Steps only where they help** (a step that leaves the wall farther off the ground than before is wrong), and the ground ramps under
   a step from one level to the other along the piece.
3. **Every joint exact**: neighbours meet corner for corner (`docs/re/platforms_and_bridges.md`, rule 6: 0.0141 across, 0.1 in height);
   checked by Check platforms' wall walks on every laid wall (a gate in the tests).
4. **Switching kinds is a full remake**: on toggling *Procedural*, the old kind's previews, layer and pieces go and the new kind's are made
   at once; nothing of one kind is read by the other's paths (`scene::walls` and friends take procedural walls only).
5. **Ends are closed**: an end is attached (on a building's joint, kit or procedural), or capped.

## Design

- **`wce_walls::kit`** (pure, no editor):
  - `catalogue(culture, files)`: the culture's fort keys (the `wall_base` prefix) with their logic files read: per piece its two joints
    (`logic::ends`, else the walk's farthest ends for cap-less curves), length, turn (signed degrees), rise (step), role (straight, turn,
    curve, step, defence, access, cap), footprint. Pieces whose joints don't read are left out and listed.
  - `lay(line, ground, ends, settings, catalogue) -> KitLayout`: the pieces (key, transform, joints, base level), the procedural end
    stretches, and a report (the most the wall strays from the line, what was left out and why).
- **Editor** (`tile_document/kit_walls.rs`): made on a thread (as the procedural previews), previews of the real models, the wall's height
  layer (after the roads'), the tech overlay, Check platforms, Siege AI and the exports reading the laid pieces.

## Phases (each reviewed by the user before the next)

1. **Catalogue**: the pieces of each culture sorted and measured; an example listing them; unit tests on the game's and TDD's pieces.
2. **Flat fit**: straights, turns and curves along the line on flat ground, caps on free ends, towers and door sections at their spacings;
   previews in the editor; the *Procedural* switch remakes the wall either way.
3. **Ends**: snapping to gates, towers and wall pieces; the procedural last 1 or 2 pieces when the kit cannot meet the joint cleanly.
4. **Slopes**: step pieces and the wall's height layer.
5. **Out**: export and Save for Assembly Kit, Siege AI, Check platforms.

## Phase 1: the catalogue (done 2026-11-12)

`wce_walls::kit`: `culture_keys` (the keys with the base's prefix), `catalogue` / `measure` / `classify`, `joints` (`logic::ends`, else
`walkway_ends`), `footprint`. Roles: Straight, Turn, **Jog** (a kink: joints in parallel, shifted across the wall: the 8 m `kink_left` /
`kink_right`, 20 along by 10 across), Curve, Step, Defence, Access, Cap; each piece's span, turn, rise, shift and walk height between its
joints (not its origin: TDD's gondor curves sit hundreds of units from theirs). Gates, gateways, stairs, watchtowers, bridging columns
and TDD's helper pieces are left out (listed with why). Unit tests on the roles; `examples/kit_pieces.rs` lists every culture
(`KIT_DETAIL=key,...` prints a piece's joints and platforms).

Measured (`kit_pieces`):
- Attila: barbarian, eastern / sassanid / western small walls (8 m: straight, two kinks, the 20 m and 60 m curves, steps, curved bastion,
  caps), roman (15 m: straight, angled joiner, 6 turns, steps, 3 defences, a door section, tower caps), eastern and sassanid (both sets).
  Greek, egyptian and persian have a few pieces only in Attila's packs (no straight): Rome 2's cultures.
- With TDD's packs: all 15 bases complete; khazad and mordor as the 8 m set; easterling and gondorean as the roman set; TDD's barbarian has
  the five `600m` curves (17.8 degrees); gondor has 3 straights, 6 turns, 5 curves (5 degrees each, 41.5 chord, whatever their names
  say), 9 defences, 3 access pieces, caps.
- Rome 2: barbarian, roman, greek, egyptian, persian complete.

For phase 2: **TDD's `gondor_fort_straight_stepped_up_bastion_a` and `gondor_fort_curve_600m` have no platforms** (left out), so gondor has
a step down and no step up; and some pieces' joints don't say their outer side (`side` zero: the walk-end joints, gondor's steps), which
the orientation has to infer.

## Phase 2: the flat fit (done 2026-11-12)

`wce_walls::kit::lay(line, catalogue, LayOptions)` -> `KitLayout { placed, stray, short_by, notes }`:
- **Ways** (`ways`): each piece is laid joint to joint, entry to exit, in the ways that keep the wall's outer face outward: by its
  joints' `side`; a curve that says neither faces out on its convex side; anything else either way. TDD's gondor step down, gone through
  backwards, is its step up (user 2026-11-12).
- **The fit**: from the line's first point along its first segment, at each joint the way that, with the best two after it, follows the
  line best (distance off the line, heading, a cost per piece, worth for length), until the end is nearer than half a straight. Each
  joint is the last one's exit, so no platform is broken by construction.
- **Towers and door sections**: every `wall_defence_every` / `wall_access_every`, a defence / access piece of the same shape (span, turn,
  shift) takes the place of the piece laid.
- **Caps**: both ends, the cap whose side fits.

Editor (`tile_document/kit_walls.rs`):
- A wall with *Procedural* off is laid at the ground under its first point and drawn as the game's pieces, whole.
- `scene::walls` and friends take procedural walls only; `all_walls` / `is_procedural` are there for the rest.
- Toggling *Procedural* remakes the wall either way, with its own preview signature.
- Check platforms checks a kit wall's pieces each as a building of its own, so the joints between them count.
- `wall_info` reports the pieces, the stray and how short of the end the wall stops.

Measured:
- `kit_pieces` with `KIT_LAY=1`, on a straight, an L each way, a quarter arc of 200 and an S, every culture: strays 10.7 at most and
  stops short by 10 at most.
- In the editor, farm_western/1x1/ab, a barbarian L of 90 + 80 m: 18 pieces (caps, straights, the 20 m internal curve at the corner),
  strays 0.35; Check platforms: 85 joints, 0 breaks.

Left for later phases: the ends stop where the pieces do (up to half a straight short), ground is flat (phase 4), no snapping (phase 3).

## Phase 3: the ends (done 2026-11-12)

- **Attached ends**: a wall's end within `scene::ATTACH` of a fort building's end (as a procedural wall attaches) meets that building's
  joint (`kit::joints` of its logic file, through its transform: the 8 m cultures' gates have no end caps, their walk's ends are the joints),
  on it whatever its point was left within reach. The line's end is made straight along the building's way for a straight's length.
- **First end**: on the joint, the wall at the building's walk, when the walks match (within 0.5) and a piece joins it; else a procedural
  stretch a straight long, the pieces from its end. **Last end**: up to 3 pieces landing on the joint exactly (0.01 across, 0.1 degrees,
  joining it, at its walk); else the pieces stop 0.5 to 1.5 straights short and a procedural stretch meets it. **Only the last end
  attached**: the wall is laid from it (the pieces gone through the other way), so that the pieces meet it.
- **Stretches** (`kit::Stretch`): a procedural wall of two points, the Bezier leaving and arriving along the ways, both ends held (the
  kit joint's level, the building's walk less the culture's: a stretch to another culture's gate climbs to its walk), made with the
  procedural walls' pieces on the editor's thread (`spawn_wall_job`, shared with them), the kit pieces drawn whole beside them.
- **Every joint joins**: each piece's joints carry their platforms' edges (`kit::joint_edges`), and a piece follows another only where
  an edge of each has the same corners (`kit::joins`, rule 6 of `docs/re/platforms_and_bridges.md`); caps, the buildings' joints and the
  exact finishes too. So TDD's barbarian `600m` curves (4.70 across where its straights are 4.10) and gondor's curves (one edge where its
  straights have two) are laid only after each other or a joiner, never after a straight. The catalogue leaves out a piece whose joint
  joins none of its culture's.
- **Pieces at their walk**: each piece stands where its entry joint's walk meets the last one's (a piece whose walk is off its culture's
  stands that much higher or lower).

Fixed on the way:
- `logic::ends` read the Roman external corners' and `tower_b_corner_with_extension`'s joints on their outer face (every culture of the
  Roman set, Attila, TDD and Rome 2): of the pairs of cap sides nearly as far apart as the farthest, the one whose sides the walk's end
  continues is taken (`examples/kit_pieces` `KIT_ENDS=1` dumps every fort piece's ends: only those pieces changed).
- `solver::runs`: a wall with both ends attached at levels within the tolerance and one run between them took the far end's level for
  the whole run, its first end that much off its building (0.22 at the test's gate); the run is split, each end at its own level.

Tested (`scratch kitwall/k3.txt`, farm_western/1x1/ab with a barbarian and a Roman gate): a barbarian wall from the barbarian gate meets it
(5 joints); one from it to the Roman gate meets its first gate and climbs to the Roman one's walk by a stretch (a break there: the Roman
gate's joint is another culture's, the report says so); a Roman wall attached by its last end only is laid from it and meets it. Check
platforms: no break inside any wall.

## Phase 4: slopes (done 2026-11-12)

- **Steps** (`kit::lay_on`, the ground as a function): where the layout lays an in-line piece, a step of the culture's that joins there
  (and that a piece goes on from) takes its place when the ground ahead (over the next piece and a half) is nearer the level the step
  goes to than the level the wall is at: never where it would leave the wall farther off (rule 2). A step down gone through backwards is a
  step up (TDD's gondor). Each piece records its level at its joints (`Placed::levels`: equal but at a step).
- **One source of truth** (rule 1): the layout reads the ground **below the wall's own height layer** (`Tile::ground_below`); the
  layer follows the layout, never the other way round. The procedural end stretches stand on the same ground.
- **The height layer** (`Tile::kit_wall_ground`): a height layer of the wall's own, owned `wall:<key>#<signature>` (the key its `wall_id`,
  given when a wall is made and by `migrate_walls` to an older one; a copied wall's n-th repeat `<id>.<n>`). Under each piece's base
  (`KitPiece::area`: the hull of its level 0 platforms and footprint; the 8 m sets' `orange_thingies` are only a strip along the outer
  face) the ground is set to the piece's level, ramped from one level to the other along a step, feathered over 4 m outside (smoothstep);
  where two pieces' bases overlap, the one whose joints a point lies nearest the line between. The signature is what it writes (the
  ground below to the millimetre): a layer is written only when it would change, and when it does the other walls are looked at again.
  The walls' layers go over the roads' (the roads' job keeps them there), in the document's order. A wall gone, hidden or made procedural
  takes its layer out (`sweep_wall_ground`: on the tile's structure changing and on a procedural remake). A move of the ground over 5 m
  is logged (cut and fill).
- Commands: `wall_levels`, `wall_layers`, `ground_profile`.

Tested (scratch `kitwall/k4..k8.txt`, terrain_rugged_hills_desert/1x1/aa): a 12 m hill raised with the brush, a barbarian wall over it
climbs five 2.5 m steps and comes down five, each piece's middle within 0.75 m of the ground below and the ground under it at its level; a Roman wall
where the hill is lower takes one 5 m bastion step each way; Check platforms 0 breaks. A 37 m rock spire under a line: the barbarian wall
climbs 10 m and the layer cuts the rest (logged). *Procedural* on takes the layer out, off brings the same back; deleting the wall takes it
out, undo brings it back; saved and opened again, the layer is as it was (not made again).

Left: phase 5 (the exports, Save for Assembly Kit, Siege AI). The height layer is in the tile's layers, so a save writes the ground it
makes into the height map already.

## Phase 5: out (done 2026-11-13)

- **Exports** (`wce_walls::export::KitWall`, `lower_walls`'s `kit`; `TileDocument::kit_walls_for_export`): an export lowers a kit wall as
  the editor laid it (on the ground below its own height layer: the export's height map has the layer in it, and laid on it the wall
  could step elsewhere, rule 1), found by its key (`scene::wall_key`, its `wall_id`). Its game pieces become `BUILDING_INSTANCE`s of their
  own keys (the game's buildings: no files nor rows), its stretches procedural pieces of its base, generated, with their rows, keyed
  `<prefix>_k<wall>s<stretch>_<piece>`; a wall's pieces go in its layer (Save for Assembly Kit: *WCE procedural walls*, which Import from
  Assembly Kit drops, putting the walls back). A kit wall the editor could not lay is left out, noted. All three exports take them: Export
  (`save_exported_walls`), save to a pack (`save_walls_to_pack`), Save for Assembly Kit (`walls_for_kit`). The ground the wall's layer makes
  is in the height map they write.
- **Siege AI** (`siege_input_with`): a kit wall's run of pieces is given as a procedural wall's (its walkway through the pieces' joints,
  every half unit, the footprint across its base's depth: a barbarian piece's own footprint is a strip along its outer face, which closes no
  outline), its caps, towers and bastions from their logic files, its stretches as procedural walls. Generate SiegeAI lays the kit walls
  whose layout is not kept yet (a republish clears them until their previews are made again: it had found none).
- **Check platforms**: phase 2 (each piece a building of its own).

Tested (scratch `kitwall/k9.txt`, `k10.txt`, farm_western/1x1/ab):
- Export and save to a pack of the gate test (three kit walls): the building list has all 40 kit pieces (26 barbarian, 14 Roman), both
  gates and the one stretch piece (`wce_farm_western_1x1_ab_k1s0_00`); the stretch's piece and rows in the pack.
- A closed fort: a barbarian ring from one end of a barbarian gate to the other (flipped: both ends said their outer side was the other
  way), the first end on the gate, the last by a 15 m stretch, 0 breaks; a capture point inside. Generate SiegeAI: 1 area, 2 wall
  edges, as the same ring made procedural (1 area, 3 wall edges; both warn that the outline is closed over gaps with the buildings).
- Save for Assembly Kit was not run: it writes `@wce_proc_data.pack` into the game's own `data` folder; it lowers the walls with the same
  `lower_walls` as the others.

Tools: `examples/building_list_dump` (wce_tile_io) prints a binary building list's buildings.

## Review fixes (2026-11-13)

From the user's first try on TDD's middle_men_port_a (a 530 m wall over hills down to the sea):
- **Saving kept the tile modified**: a kit wall's layer made and taken out again (*Procedural* back on) left the tile's layers empty
  but marked changed, and with no layers file to remove the save never cleared the mark (`TileProject::save`: empty layers are saved
  by doing nothing).
- **The wall's line vanished**: a kit wall drew no lines of its own (a procedural wall's walkway, foot, steps and ends,
  `wall_overlay`), so only its centre line on the ground showed, under its pieces and its raised ground. It draws the same lines through
  its pieces' joints as laid (foot at the level its layer sets, walkway, red uprights at the steps, green crosses at attached ends,
  its stretches straight), redrawn when it is laid again; the polygons are draped again when a layer changes the ground.
- ***Procedural* seemed to do nothing but the ground**: every polygon rebuild (models still loading, a layer changing the ground) marks
  every wall changed, and each remake threw away the wall's pieces being made and started again: on a large tile the procedural
  pieces were never finished, so the kit pieces stayed. A remake that wants what is being made lets it go on (`WallPreview::parked`).
- **Barbarian walls under the ground**: steps went only in place of straight pieces, so along a curving line up a hill the wall kept
  its level and the layer cut it up to 11 m into the slope. A step now also takes the place of a curve or a kink where the ground is a
  whole step or more off and the step follows the line nearly as well (`STEP_DETOUR`: up to 0.3 of a straight worse): on the port the
  deepest cut went from 11.1 m to 4.3 m; the fill (10.4 m) is where the line runs down a cliff to the sea faster than steps go.
  Check platforms: 0 breaks in the wall walks.

Tested (scratch `kitwall/p1..p7.txt` on a copy of middle_men_port_a with its database entry; k1, k3, k6 again: as before).

## Follow-ups (user, 2026-11-13)

Three more before the walls are done, each reviewed before the next.

### 6. Tooltips on the wall's Inspector rows

A tooltip on every row of a `WceWallSpline`'s Inspector (`wce_tile::properties::wall_tip`, shown with `Inspector::hint`): what the row
does, in which kind (procedural or kit) and in what units. Only the walls for now; the rest of the app gets them later, on another
branch.

### 7. Convert to buildings

A button under a kit wall's rows. It is enabled only when the wall is laid and **has no procedural stretch** (its pieces are all the
game's). In one undo entry it:
- adds each laid piece as a `BUILDING_INSTANCE` of its own key in the wall's layer, placed and turned as laid (`scene::place_pieces`,
  shared with the exports' `scene::lower`);
- moves the spline into a new child layer of that layer, `(converted) <name>`, hidden (no `OIF_VISIBLE`) and *Don't Export*
  (`LayerExportType_Off`);
- keeps the wall's ground: its height layer is no longer the wall's (owner cleared) but a layer of the user's, `Kit wall <name>
  (converted)`, so hiding the spline does not take it out from under the buildings.

The exports, Siege AI and Check platforms skip walls in a layer that is not exported (`scene::exported`).

### 8. Towers

- **Wall towers** at their own spacing, `wall_tower_every` (default 0 = none), beside `wall_defence_every`, which from then on places
  only the bastions. A slot both want gets the tower. Only cultures with tower wall pieces get them (the Roman-style sets'
  `*_tower_c_straight_with_extension` and `*_tower_b_corner_with_extension`, Gondor's curved towers). For the 8 m cultures the setting
  does nothing, and the Inspector says so. Procedural walls take the same split: towers at `wall_tower_every`, bastions at
  `wall_defence_every`, instead of alternating at one spacing.
- **Tower capture points**, generated live like the pieces. Each tower gets a `CaptureLocationInstance` of `capture_point_tower_only`,
  `CLT_MINOR`, with its boundary a rectangle over the top of the wall behind the tower (the tower piece's walkway, its length and
  width). They are drawn in the editor, written by the exports, read by Siege AI, and become real elements on Convert to buildings.

Follow-ups 6 and 7 done (2026-11-13). The button's tooltip says why it is greyed out (`Inspector::button` shows its hint, disabled
too). The undo entry joins the scene's splice and the height stack's change (`CommandStack::join_last`). Tested (scratch
`kitwall/c1.txt`, `c2.txt`):
- A barbarian wall over a 12 m hill: 14 buildings; its ground layer is the user's now (`terrain_layer height list`); Check platforms
  0 breaks over the buildings. Undo brings the wall, its pieces and its owned layer back; redo converts again.
- An export of farm_western/1x1/ab after converting a 10-piece wall: the building list has the 10 pieces once (the hidden wall adds
  nothing).
