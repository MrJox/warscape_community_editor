# Startpos editing: armies, agents and settlements on the campaign map

Goal: the campaign map in the Terrain tool shows a campaign's start position as the game draws it (armies, agents, navies and
the settlements) and edits it by clicking on them: move, owner, an army's units, a settlement's buildings. Attila and TDD first
(Thrones' start is drawn too); Rome 2 later (its startpos layout differs).

**Scope now (user, 2026-11-21, later): read and draw only, no edits.** The start's armies, agents, navies, settlements and
animated props are read and drawn as the game shows them; editing (phases 3 to 6 and the open questions below) is decided later.
The start is read from the campaign's compiled startpos, as the settlements already are (what the game loads).

User decisions (2026-11-21, before the scope above): edits go to the Assembly Kit's `start_pos_*` tables in the project, with the
compiled `startpos.esf` as the fallback for maps without tables (TDD); the game's own models from the start; first edits: move,
owner, army units, settlement buildings.

## What is known (research of 2026-11-21)

What the game reads and draws (the startpos's records, the kit's `start_pos_*` tables, the models, banners and colours):
docs/re/terry.md, "The campaign's start: the startpos and its figures".

### The kit tables and RPFM

- RPFM's generator (`rpfm_extensions::startpos`, the user's `../rpfm`) builds the startpos from the kit's `start_pos_*` tables
  (Attila about 0.90 of the fields equal to the game's); its extractor (`extract::extract_tables`) turns a startpos back into the
  tables (TDD's included).
- Patching a startpos directly: a move is three leaves (plus `MILITARY_FORCE`[7] / `GARRISON_RESIDENCE`[1] in and out of a
  settlement, and the AI's `CAI_SITUATED` cell); an owner change moves records between factions' arrays and touches a dozen linked
  records (provinces, ownership lists, buildings, garrisons, AI mirrors); new units need fresh global ids and AI entries; a building in
  an empty slot inserts a record. Line of sight is recomputed by the game; whether it rebuilds the AI's records is not known.
- rpfm_lib's ESF encoder keeps the content but not the bytes (other size padding, a weaker LZMA: 14 MB for 7 MB); whether the game
  loads it is untested.

### The models

- The chain from a character to its variant mesh and idles: docs/re/terry.md, "The campaign's start", "The models".
- The editor already assembles and draws skinned variant meshes for battle units (`unit_assembly.rs`, `renderer/skinned.rs`,
  `unit_animation.rs`, `Payload::SkinnedInstance`); missing: the campaign animation tables and a campaign loader.
- Scale: check against the user's screenshots before guessing (the per-entity factor is not traced).

### The Terrain tool

- Picking: an ID pass (meshes, terrain) and overlays; skinned meshes are not in the ID pass (the Tile tool picks its reference
  soldiers with spheres). Settlement models are added locked with no item (`prop_models.rs`), so not pickable.
- Editing: `Document::inspect` / `ImGuiInspector`, `Command`s on `MapDocument`'s stack (`TerryAttrCommand` is the model), the
  ImGuizmo gizmo through `item_transform` / `transform_items`.
- Saving: nothing writes kit db XML or a startpos yet.

## Drawn (2026-11-21, read only)

As built, the start is read from the compiled startpos only (`wce_terrain_io::attila::characters::read`; Attila's, TDD's and
Thrones' layouts), not from the kit tables; phase 1's shared model below is not built. Drawn by the Terrain tool
(`wce_terrain_tool::start_figures`, locked, not pickable):

- The start's characters the map shows (Attila 105, TDD 46), and Thrones' starting rebels (`characters::scripted_rebels`).
- Their figures (`wce_render::campaign_character`) and an army's banner (`ARMY_FLAG_SCALE`, Attila's offset), Thrones' in the
  faction's uniform and banner colours. Which characters, rows, banners and colours the game takes: docs/re/terry.md, "The campaign's
  start".
- The fauna: a model with its own `.anim` beside it, posed skinned (`wce_render::animated_prop`).
- Left: a navy's banner, if the game gives one (not in the captures); the banner's wind (the flag's `TextureComboVertexWind` draws
  static); the figures' idles only (no walking: the start stands still). The open items of the campaign captures (Thrones' banner
  offset, the figures' colour permutation, the rebels' art sets) are in docs/plans/campaign_tilemap.md, "Open".

## Phases (3 to 6 deferred: scope above)

1. **The start as a model** (`wce_terrain_io::attila::startpos`, new): `Start { factions, characters (id, type, faction, cell,
   art set, force), forces (id, commander, navy, units), settlements (region, owner, slots and buildings) }`, read from the kit
   tables (the project's, then the kit's: the data source order) when the campaign has them, else from the startpos (packs, then the
   game's `data`). One model for both, so everything after it does not care where it came from. Checked: Attila's tables and its
   startpos give the same characters, cells and units (less the generated garrisons).
2. **Drawn with the game's models**: a campaign character loader (art set -> arts row -> uniform -> variant mesh; the campaign
   animation tables for the idle; ships for navies and armies at sea), placed at the cell's world point on the ground, facing pi.
   Garrisons and characters inside settlements not drawn (the game doesn't). Scale and the banner checked against the user's
   screenshots (Documents, `325610_*.png`); the banner after the figures.
3. **Clicking**: characters picked by a sphere or box around each figure (as the Tile tool's soldiers), settlements by their
   models (pickable, an item id per settlement); a selection item that says which (character id / region key).
4. **Editing** (undoable commands on `MapDocument`'s stack, the view rebuilt from the model):
   - move: the gizmo or a drag, snapped to hex cells, onto land for armies and agents (sea for navies); into a settlement when
     dropped on one;
   - owner: a faction list (a character takes its force along; a settlement its region);
   - an army's units: list, add (the units the faction can recruit, by `units_to_groupings_military_permissions` or all), remove,
     strength;
   - a settlement's buildings: per slot, the buildings its slot type allows; the drawn buildings and roads follow.
5. **Saving**: the kit tables written to the project (`raw_data/db/start_pos_*.xml`, the kit's own XML layout, only the tables
   that changed, the rest untouched). The startpos fallback (TDD): see the open question below.
6. **Export** (later): a startpos for the game (RPFM's generator, or the patched startpos) into the mod pack.

## Open questions for the user

1. **TDD's fallback.** Two ways: (a) patch TDD's startpos directly: moves are easy, unit and building changes mean new ids and
   inserted records, owner changes touch a dozen linked records and the AI's mirrors (unknown whether the game rebuilds them); or
   (b) extract TDD's startpos into kit tables in the project once (RPFM's extractor), edit the tables like Attila's, and generate
   the startpos on export (RPFM's generator: about 0.90 of the fields equal for Attila, so the rest of TDD's startpos would come out
   as the generator builds it, not as TDD shipped it). Recommendation: (a) for moves, units and buildings, owner changes later;
   (b) when the generator's gaps close.
2. **rpfm_extensions as a dependency** (for the extractor / generator and `MapScale`): `rpfm_extensions::startpos` is on rpfm's
   `develop` now (the `startpos-thrones-rome2` work merged there); the editor depends on `rpfm_lib` alone. Add it, or reimplement what
   phase 1 needs here?
3. **Export's startpos**: generate it from the tables with RPFM, or leave it to the user's RPFM?

## RPFM's startpos generator (state of 2026-11-05)

The generator (`rpfm_extensions::startpos` in the user's rpfm, now on `develop`) builds a campaign's `startpos.esf` from the Assembly
Kit's tables; the export of phase 6 and the fallback of question 1 depend on it. Its work log, evidence and DLL addresses were in
`I:\wce_scratch\rpfm_startpos\NOTES.md` (the Windows scratch, with the test data and the user's load tests). rpfm's own rules apply to
work there (`AGENTS.md` of rpfm: `Startpos:` commit prefix, tests in `*_test.rs`, no `cargo fmt`).

**User policy:** a game goes into `SUPPORTED_GAMES` (Attila alone today) only when every known gap is fixed; don't add Thrones or
Rome 2 before that, and don't offer to.

Fields equal to the game's startpos on 2026-11-05: Thrones `vik_attila` 0.8492, Attila `main_attila` 0.9029, `pro_attila` 0.9105; TDD's
124 victory objectives all equal. CAI_HISTORY is left out on purpose (only CA's internal builds write it). Not gaps: CEB+2 (a draw at
load), the art set hash and region line of sight (recomputed at load).

Thrones' gaps then (later rpfm commits may have closed some: check rpfm's log): the garrison unit order (the builder not found; if it
orders by object pointer hash it can't be reproduced); the trade route path tie-break (111 Thrones and 2 Attila paths;
`FUN_107f7420`); route resource amounts (406 routes, written as zeros); Welsh garrisons (`wel_swordsmen` from the
`wel_armoured_swordsmen` group); smaller fields (NOTES.md rounds 7 to 9); names and the main RNG (seeded 0xCACA at a startpos build:
the name picking and every main-RNG draw in order; the largest item, last); the gravitas base and the AI analyses (CAI beliefs /
desires), both waiting for the user's in-game load tests.

Rome 2 (on hold until Thrones' gaps close, user's decision): the generator stops at CHARACTER_DETAILS; characters, diplomacy, region
slots and buildings, religion, fortifications and the CAI are laid out otherwise (706 differing paths), a record-by-record port; five
objective types of main_3c / main_invasion have ids but no known record layout.
