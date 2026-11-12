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
