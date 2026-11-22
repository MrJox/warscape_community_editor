# Tile editor: walls of kit pieces (2026-11-12)

A `WceWallSpline` with *Procedural* off (`wall_procedural`, docs/editor.md) is laid with the game's own fort pieces instead of pieces
made on the fly. *Procedural* on: the wall as it works today (procedural_walls.md), unchanged.

This replaces the first attempt (`feature/combined_walls`, shelved 2026-11-12: walls sank on slopes, steps without ramps, switching kinds
lost the pieces, ends left open). Nothing of it is merged; what it learned is kept here as rules, and the code was written again phase by
phase, each phase reviewed by the user before the next.

**Status (2026-11-22):** phases 1 to 5, the review fixes and follow-ups 6 to 8 built (2026-11-12 .. 13); since then the fit follows
the line along a window rather than by its nearest point anywhere (2026-11-14), and a combined project's variations each lay their kit
walls (2026-11-15). The step-by-step logs and their test runs are in the git history (scratch scripts `~/wce_re/scratch/kitwall/`).

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

## Phase 1: the catalogue

`wce_walls::kit`: `culture_keys`, `catalogue` / `measure` / `classify`, `joints` (`logic::ends`, else `walkway_ends`), `footprint`.
Roles: Straight, Turn, **Jog** (a kink: joints in parallel, shifted across the wall), Curve, Step, Defence, Access, Cap; each piece's
span, turn, rise, shift and walk height between its joints (not its origin, which some pieces are far from).
Gates, gateways, stairs, watchtowers, bridging columns and TDD's helper pieces are left out (listed with why). `examples/kit_pieces.rs`
lists every culture (`KIT_DETAIL=key,...` a piece's joints and platforms, `KIT_LAY=1` lays test lines, `KIT_ENDS=1` dumps every fort
piece's ends).

Measured (`kit_pieces`): each culture's pieces, in Attila, TDD and Rome 2: docs/re/buildings.md, "Each culture's fort pieces". TDD's
gondor has a step down and no step up: a step down gone through backwards is its step up (user, 2026-11-12).

## Phase 2: the flat fit

`kit::lay` -> `KitLayout { placed, stray, short_by, notes }`: each piece laid joint to joint in the ways that keep the outer face outward
(its joints' `side`; a curve that says neither faces out on its convex side); at each joint the way that, with the best two after it,
follows the line best; towers and door sections take a piece's place at `wall_defence_every` / `wall_access_every` (same shape); caps at
both ends. In the editor (`tile_document/kit_walls.rs`) the pieces are drawn whole; `scene::walls` and friends take procedural walls only
(`all_walls` / `is_procedural` for the rest); toggling *Procedural* remakes the wall either way; Check platforms checks each piece as a
building of its own; `wall_info` reports the pieces, the stray and how short of the end the wall stops.

## Phase 3: the ends

- **Attached ends**: a wall's end within `scene::ATTACH` of a fort building's end meets that building's joint (`kit::joints` of its logic
  file). First end: on the joint when the walks match and a piece joins it, else a procedural stretch. Last end: up to 3 pieces landing
  on the joint exactly, else a procedural stretch. Only the last end attached: the wall is laid from it.
- **Stretches** (`kit::Stretch`): a procedural wall of two points, both ends held, made with the procedural walls' pieces on the editor's
  thread (`spawn_wall_job`).
- **Every joint joins**: a piece follows another only where an edge of each has the same corners (`kit::joint_edges`, `kit::joins`, rule
  6 of `docs/re/platforms_and_bridges.md`), so TDD's barbarian `600m` curves and gondor's curves (their ends: docs/re/buildings.md,
  "Each culture's fort pieces") are laid only after each other or a joiner, never after a straight.
- Fixed on the way: `logic::ends` read the Roman external corners' and `tower_b_corner_with_extension`'s joints on their outer face
  (Attila, TDD, Rome 2); `solver::runs` gave a run with both ends attached the far end's level throughout.

## Phase 4: slopes

- **Steps** (`kit::lay_on`, the ground as a function): a step of the culture's takes an in-line piece's place when the ground ahead is
  nearer the level the step goes to (rule 2); since the review a step may also replace a curve or a kink where the ground is a whole step
  or more off (`STEP_DETOUR`). Each piece records its level at its joints (`Placed::levels`).
- **One source of truth** (rule 1): the layout reads the ground below the wall's own height layer (`Tile::ground_below`).
- **The height layer** (`Tile::kit_wall_ground`): owned `wall:<key>#<signature>` (the key its `wall_id`, given when a wall is made and to
  an older one on load); under each piece's base the ground is set to the piece's level, ramped along a step, feathered over 4 m; written
  only when it would change; over the roads' layers; taken out when the wall goes, hides or turns procedural (`sweep_wall_ground`). A
  move of the ground over 5 m is logged (cut and fill). Commands: `wall_levels`, `wall_layers`, `ground_profile`.

## Phase 5: out

- **Exports** (`wce_walls::export::KitWall`, `TileDocument::kit_walls_for_export`): a kit wall is lowered as the editor laid it, found by
  its key (`scene::wall_key`); its game pieces become `BUILDING_INSTANCE`s of their own keys, its stretches procedural pieces of its base
  with their rows (`<prefix>_k<wall>s<stretch>_<piece>`); in its layer (Save for Assembly Kit: *WCE procedural walls*, which File >
  Revert > *To the Assembly Kit's version* drops, putting the walls back). All three exports take them (`save_exported_walls`,
  `save_walls_to_pack`, `walls_for_kit`); the height map they write has the wall's ground.
- **Siege AI** (`siege_input_with`): a run of pieces as a procedural wall's walkway and footprint, its caps, towers and bastions from
  their logic files, its stretches as procedural walls. Generate SiegeAI lays the kit walls not laid yet.

## Review fixes (2026-11-13)

From the user's try on TDD's middle_men_port_a (a 530 m wall over hills down to the sea): saving kept the tile modified (empty layers);
a kit wall now draws its own lines through its pieces' joints; a remake lets the pieces being made go on (`WallPreview::parked`), so
*Procedural* no longer seemed to do nothing on a large tile; steps on curves (above: the deepest cut 11.1 m -> 4.3 m).

## Follow-ups (user, 2026-11-13; done)

- **6. Tooltips** on every row of a `WceWallSpline`'s Inspector (`wce_tile::properties::wall_tip`); only the walls for now.
- **7. Convert to buildings**: a kit wall with no procedural stretch becomes, in one undo entry, a `BUILDING_INSTANCE` per laid piece in
  the wall's layer (`scene::place_pieces`, shared with the exports); the spline moves into a hidden, not exported child layer
  `(converted) <name>`; its height layer becomes the user's (`Kit wall <name> (converted)`). The exports, Siege AI and Check platforms
  skip walls in a layer that is not exported (`scene::exported`).
- **8. Towers** (no setting of their own): the defence slots alternate tower and bastion in both kinds (`kit::defence_of`, the
  procedural walls' `profile.defences`). Each tower gets a live `CaptureLocationInstance` (`capture_point_tower_only`, `CLT_MINOR`,
  `wce_walls::towers`): a rectangle over its walk, named `wce_wall<key>_tower<n>`, linked from the tower's building; drawn, exported,
  given to Generate SiegeAI, made real by Convert to buildings.

## Open

- [ ] The user's review of the whole (2026-11-13).
- [ ] Save for Assembly Kit with kit walls not run (it writes `@wce_proc_data.pack` into the game's `data` folder); it lowers them with
  the same `lower_walls` as the other exports.
- [ ] TDD's gondor has no step up (above).
