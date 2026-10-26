# Tile editor: procedural walls (plan, 2026-10-24)

A wall drawn as a spline between gates and bastions placed by hand. The editor generates buildings that follow the spline exactly: bent
along its curves, mitred at its sharp corners, stepped on slopes, their foot on the terrain. Siege battles must work on them: units on the
walls, ladders and towers, breaching. What a wall piece is (frame, platforms, levels, the `models_building` row's `EFLines` and
`DockingLines`, the families, the docking rule): [docs/re/buildings.md](../re/buildings.md), "Wall pieces". The editor side (panel,
Inspector rows, commands, Save for Assembly Kit): [docs/editor.md](../editor.md), procedural walls row.

## Decisions (user, 2026-10-24 unless dated)

- **Follow the line exactly.** No snapping to the kits' fixed lengths and angles.
- **The terrain is not touched**: the wall adapts to it. The ground is the terrain at the outer face's base line.
- **Slopes are climbed by steps**, never by a ramp of the walkway. **Steps use authored rises only** (review rounds 1-2, option B):
  nothing is stretched vertically; a step uses as much of the start of its culture's stair as it rises, at its own slope; a larger rise
  chains stairs. 15 m climbs of about 10 are a whole stepped bastion, as vanilla. Runs between steps are at least `min_run` long.
- **The walkway keeps the base's height over the ground** within a tolerance (default 0.5 m, per wall). To revisit after gameplay tests.
- **Gates and bastions are placed by hand**; a wall runs between them. **Towers and bastions along a wall** come from `wall_defence_every`
  (default 0 = none): the game's pieces as they are, on straight stretches with no step, away from corners (`pieces::defence_slots`).
- **Access on 15 m walls:** a door section every `wall_access_every` (default 200 units), only where the 20 x 20 landing inside is free of
  no-go and buildings; else skipped with a note. 8 m walls need none (the bank).
- **A free end gets the culture's cap** (8 m endcaps, 15 m tower ends), the game's piece as it is, joined at the wall's end.
- **Vertices are smooth or sharp**; an internal angle of 90 degrees or less is always sharp.
- **Pieces have variable lengths** (about the base's, one base each).
- **Destruction levels are generated; collapse animations are not** (a piece swaps to its next level). Revisit later.
- **The spline is a custom element (`WceWallSpline`) of the tile model**, not editable piece by piece; its pieces are regenerated.
- **TEd sees the pieces** (2026-10-25, option A of three): Save for Assembly Kit lowers the walls into a top layer *WCE procedural walls*,
  their files and rows into the movie pack `@wce_proc_data.pack`; Import from Assembly Kit drops that layer and restores the walls.
- **Live preview while editing; generated files only on export**, one pack per tile (configurable later).
- **Families:** 8 m and 15 m now; TDD's Rohan and cliff walls later.
- **Not in this iteration:** terrain shaping, collapse animations.

## Design (as built)

- **Model:** `WceWallSpline`, stored like a polygon (`<points>`, `polygon_closed="0"`) with `wall_base`, `wall_tolerance`, `wall_flip`,
  `wall_access_every`, `wall_defence_every` and per point `sharp`. Ends are not stored as attached: an end on a fort building's end (within
  0.5 units; snapped within 8) is attached, at that building's level and along its axis.
- **Kit table** (`wce_walls::profile::BASES`): per base straight its family profile, step pieces and stair range, door section, caps and
  defences.
- **Solver** (`wce_walls::solver`): corners at the ends, marked points and turns under 25 degrees; centripetal Catmull-Rom between them;
  the centre line straightened under defences and at attached ends so every joint is exact (`straighten`); runs held to the ground,
  steps placed inside their runs away from corners and attached ends, straightest place first.
- **Pieces and warp** (`wce_walls::pieces`, `warp`, `mesh`, `logic`): each stretch cut into `round(length / base length)` pieces, the
  base warped along the centre line (x by arc length, z across, y over the level), triangles and platforms cut as the curve needs; the foot
  stretched to stay buried; mitres stretch the outer side onto the bisector and clip the inner side (shear limit 10 per unit, internal
  angles down to about 12 degrees), with overhang copies filling 8 m outer corners; stair sections spliced only into the levels their
  step piece has. Every level's meshes, the logic and zone files (`make_piece`).
- **Shared scene code** (`wce_walls::scene`): walls, anchors, layouts, doors and lowering over a `TedScene`, for the editor and exporters.
- **Preview** (`wce_tile_editor` `tile_document/walls.rs`): intact meshes made 300 ms after the last change, served from `wce_vfs::generated`.
- **Lowering happens at export, not in the compile:** `workflow::save_exported_prepared` compiles with the walls lowered in memory
  (`lower_walls`) into `BUILDING_INSTANCE`s keyed `wce_<tile>_<variation>_<wall>_<piece>`; rows copied from the bases (`db::copy_rows`),
  `EFLines` rebuilt from the piece, `DockingLines` warped and cut to 5 units, none over steps. `wce_walls::pack` writes
  `wce_<tile folder>.pack`; `export::walls_for_kit` and `pack::replace_in_proc_pack` the TEd polyfill (a tile replaces only its own keys
  and table files in `@wce_proc_data.pack`).
- **Siege AI:** Generate SiegeAI reads the walls' layouts (`tile_document/siege.rs` via `wce_walls::scene::walls`), so the AI layout
  follows any wall when regenerated (docs/plans/siege_ai.md).
- **Mod packs and other games** (2026-10-26): Save to mod pack of a tile with walls (`export::save_walls_to_pack`) lowers the walls,
  compiles the project as the mod pack save does (`workflow::exported_pack_files`) and puts the tile's files, its tile database files, the
  pieces and their rows (`db/<table>/<key prefix>`) into the user's pack in place of what it had of the tile (`pack::replace_in_pack`).
  The packs are written for the game of the files (its rpfm key). The Inspector lists the bases the mounted packs have whole
  (`profile::available_bases`): Rome 2 has five (barbarian, roman, greek, egyptian and persian forts); a Rome 2 tile with a roman and a
  greek wall passes `wall_pack_check <pack> schema_rom2.ron rome_2`.

## Status (2026-10-26)

Phases 0-5 done and four review rounds (commits 5082a93..a93070a, e77b7d1): the in-game spike (bent copies load, units cross joints,
breach), model and editing, generator, bends and mitres, steps, export, caps, spacings, the docking fix, destroyed stairs, the TEd
polyfill. Checks:

- `cargo run --release -p wce_walls --example wall_generate_check -- <game> --mods <...>`: S curves with a corner over slope and dip, a
  flat 15 m wall with doors; deterministic, firing lines on platforms, door feet on the ground, stairless levels unchanged.
- `wall_pack_check <pack> <schema>`: every generated key has its files, rows and one instance. `wall_joint_check`: every piece end
  against its neighbour. `wall_spike`, `wall_test_maps`, `wall_edoras_test` (one Edoras wall rebuilt; `--variant terrain-only|wall-only`;
  its own moving of CA's AI polygons is superseded by Generate SiegeAI).
- Unit tests in `wce_walls` (mitres, joints exact, levels beside bastions, troughs, defence slots); `scripts/tile_walls.txt` in
  `tile_check.sh`.

## Open

- [ ] **Phase 6, kits:** the table covers every 8 m and 15 m culture (incl. TDD khazad, mordor, easterling, gondorean, gondor), but
  `wall_generate_check` only generates `barbarian_fort_straight` and `roman_fort_straight`: make it a gate that every base in
  `profile::BASES` generates (pieces, steps, doors, caps, defences).
- [ ] **TDD Rohan and cliff families** in `profile.rs` (ladder pipes as access, `_step_up`; the cliff wall's way up unknown).
- [ ] **In game (user):** record the docking fix (`solver::straighten`, eb1fc15): units cross from docked ladders and towers onto
  generated walls; one Edoras wall pack checked in game end to end.
- [ ] **TEd (user):** TEd lists and draws the pieces from `@wce_proc_data.pack`, and its export of the tile works with them.
- [ ] **In game (user), Rome 2:** the pieces' frames and walkways are Attila's measures of the same models; a Rome 2 wall checked in game.
- [ ] A thin dark sliver at the bottom edge of one 8 m stair (desert hills render).
- [ ] A cache sharing pieces between walls that fit, for pack size (a bent piece is 1.6-3.7 times its base).
- Deferred by decision: collapse animations, terrain shaping.
