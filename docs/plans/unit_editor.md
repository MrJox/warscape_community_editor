# The Unit tool

A tool for editing a unit's looks: its **variant mesh definitions** (VMDs), shown on the unit's own skeleton, animated, in the renderer.
Then the DB rows that decide which VMD a unit wears. It replaced the `unit` placeholder in `wce_app::tools`. It builds on the unit
rendering in [units.md](units.md); that plan's decision "Unit tool viewer: dev-only" is replaced by the decisions below.

State (2026-11-22): planned 2026-11-14, decisions taken with the user (the list at the end); steps 1 to 7 built 2026-11-14 .. 15, in
master (feature `unit`, in the default features; kept out of the `release` set 2026-11-21, "not ready for production"). What is left
is under "Left". How it works for a user: [editor.md, Unit tool](../editor.md#unit-tool-wce_unit_tool).

## What it builds on

- `wce_formats::variant_mesh` (`VariantMeshFile`: a tree over a lossless `XmlDoc`; attributes outside the tree and unknown children stay
  in the doc; TDD's 11 malformed files parse through `repair()`).
- `wce_render::unit_assembly` (`assemble`, and `assemble_with` from step 2), `unit_catalog::UnitCatalog`, `unit_animation::UnitModel`.
- The tile's references (`wce_tile_tool::ref_units`) as the working example; the Weather tool's `XmlFileDocument` as the document model
  (`CommandStack<XmlDoc>`, saved to `<projects>/loose/<vfs path>`, mounted above the packs).
- `wce_walls::pack::write_pack_as` and `wce_walls::db` for packs and DB rows; `wce_launch::battle_xml` for a later Launch in game.

## Design

### The document

- **One VMD file** (`.variantmeshdefinition`), opened from the packs, a mod or the project (`opens()`). Saved to
  `<projects>/loose/variantmeshes/variantmeshdefinitions/<name>` like a weather file (`project_folder`), so every loader (the tile's
  references too) sees the edit at once.
- **`VmdDocument`** (new crate `wce_unit_tool`, Cargo feature `unit`): `CommandStack<XmlDoc>` plus the tree rebuilt after each change.
  Attribute edits are span edits; structural ones re-parse. Both are byte-minimal by `XmlDoc`'s rules. Round trip and minimal edits are
  gated (below).
- **Repaired files:** opening one says so in the panel. Saving writes the repaired text, which is a change to bytes the user didn't edit;
  the save message says so. Nothing is written until the user saves.
- **Preview as:** the unit context comes from a "Preview as" unit + faction (the catalog's units that use this VMD first, then any unit):
  it gives the skeleton, the animation table, the mount, the scale and the faction colours. It is a tool setting remembered per VMD, not
  part of the file. Without one, the game's default unit's skeleton (`rome_man_game` for Attila) and `STAND`.
- **Opening by unit:** the Units panel lists `unit_variants` rows (unit, faction, VMD). Opening one opens its VMD and sets Preview as.

### What the preview shows: seed and pins

- **Seed**, as the game does (the editor's generator, not the game's: already an assumption in units.md), with Next / Previous.
- **Pins:** per slot, "random" (default), "off", or "alternative N". Per mesh with several tints, which tint; per `.texture_array`, which
  decal. Pins are view state, not saved in the file. Selecting a node in the Structure panel pins its path for the moment, so the piece
  being edited always shows (a toggle turns that off).
- **Grid:** N soldiers side by side (seeds `seed..seed+N`), to judge the variety as a unit shows it.
- Needs the assembler to take choices: `assemble_with(read, &VariantMeshFile, seed, &Pins) -> AssembledUnit`. The `Pins` are keyed by the
  tree's path (`NodePath`, so they survive the re-parses that shift element indices). The same RNG draws are consumed whether
  a choice is pinned or not, so pinning one slot doesn't reshuffle the others. `assemble` stays as the call with no pins.
- The document's own tree goes to the assembler, not the file in the VFS, so unsaved edits show. References load from the VFS (an open
  referenced VMD is not live; see Later).

### Panels

- **Units:** search over `unit_variants` (unit, faction, variant), open a unit's VMD. Mods included (as the catalog reads them).
- **Files** (built as Unit files): the VMDs in the VFS, by folder, with their owner (pack, mod, project). The model and texture fields
  pick through `wce_editor::pickers` (`pick_button`, `search_combo`).
- **Structure:** the tree. Meshes show their model's file name, slots their name, attach point and probability, references their target
  (expandable, read-only, with Open). Context menu and buttons: add slot, add mesh, add reference, duplicate, delete, move up / down. Drag to
  reorder later if wanted.
- **Inspector**, for the selected node:
  - Mesh: `model`, `decal_diffuse`, `decal_normal`, `imposter_model` (VFS picker, red if missing), `mask0..2` (combo 0, 1, 2), tints
    (add / remove; `color_edit3` and `random_range`), `META_DATA` lines.
  - Slot: `name`, `attach_point` (combo: `weapon_01..05` and the parent model's attachment points, free text allowed), `probability`
    (slider 0..1, "unset" check).
  - Reference: `definition` (VFS picker over VMDs).
  - Attributes the tree doesn't know show as raw text rows, editable, so nothing in the file is out of reach.
- **Preview:** Preview as (unit, faction), seed, pins summary with "clear pins", grid size, mount on / off, the slot to play (the Plays
  combo of `ref_units`), play / pause / scrub, faction colour overrides (as `ref_human`), an "isolate selection" toggle that draws only the
  selected node's parts.
- **Problems:** missing models, textures and references, unknown attach points, masks out of range, probability out of 0..1, reference
  cycles, a model whose skeleton doesn't match the unit's. Each line selects its node.

### The stage

- No tile: a plain ground with the climate's and season's grass texture, lit by the stage's own weather chips (`weather.rs`, as the
  Tile tool's), the orbit camera focused on the figure.
- The figure at the origin, a mount under its rider. In the grid, soldiers in a row at the unit's spacing.
- **Selection in the viewport:** skinned meshes are not in the id pass. Built (2026-11-15): clicks are found on the CPU (a part of a
  referenced file selects the reference) and the selected node's parts are outlined.

### DB: the tables that decide looks (phase 2)

- **Tables:** `unit_variants` (faction, unit, variant, height_scale, height_variation, unit_card), `variants` (variant_name, variant_filename,
  low_poly_filename, scale, scale_variation, mount_scale), `mounts` (variant), `unit_variants_colours` (per unit and faction colours).
  Stats (`land_units`, `main_units`) are out of scope until asked for.
- **Checked** (step 6, `wce_render` example `unit_tables_check`, 2026-11-14): in Attila, vanilla and with TDD, `variants` has only
  `variant_name` (the schema's `variant_filename`, `scale`, `mount_scale` columns are other games' versions), so a unit's `variant` is its
  VMD's file name, as the catalog had it: 1440 of 1445 vanilla rows find their file (the 5 others are transport crews with none), 2087 of
  2112 with TDD (25 of TDD's Arnor and Gondor units name files it doesn't ship). What the catalog missed is `unit_variants_colours`: a
  unit's own colours by faction and soldier type (soldier, officer, standard bearer, musician), 2 rows in vanilla, 57 (28 units) in TDD.
  It reads them now, the soldiers' row over the faction's colours (**ASSUMPTION**: not compared with the game). Rome 2's packs aren't on
  this machine: not checked there.
- **Keys:** a `unit_variants` row is told apart by its unit and faction (the schema's key columns), not its `name`, which repeats per
  faction. Every table's rows are keyed by the schema's key columns.
- **Storage:** the edited rows as rpfm DB files in the project, `<projects>/loose/db/<table>_tables/!wce_unit_tool`, encoded by rpfm. The
  loose folder is mounted first, so the catalog and the preview see them, and they ship as they are. The `!` sorts the file before the
  game's: the catalog keeps a key's first row, and so does the game by modders' practice (**ASSUMPTION**, not checked in the game yet).
- **Editing:** the Preview panel's "The unit's looks (DB)" section: the previewed unit's row for its faction (the VMD it wears, Wear the
  open file, Its own copy..., height scale and variation), its own colours, Revert. Built differently from the first draft: the edits are
  saved at once and are not in the undo history (Revert takes the project's rows of the unit away), and a faction without a row of its
  own edits the row it falls back to (no new rows per faction yet).
- **Export > To mod pack:** the project's VMDs and textures the tool wrote, and the DB files, through `write_pack_as` (merged into an
  existing pack as the UI tool does). A Launch in game for a unit (a custom battle with this unit on one side) is a later step:
  `battle_xml` already takes units.

### Other games

Rome 2 has the same VMD, anim and table formats (units.md): the tool works there with the same code. Later games (`wsmodel`, `.anim` v6+,
binary fragments) need their own readers behind the game profile; until then the tool is listed in their `unsupported_tools`.

## Plan (as built)

1. **Tree from a doc** (2026-11-14, `wce_formats`): `VariantMeshFile::from_doc`, `NodePath`, the structural helpers in tree terms; the
   `.texture_array` parser moved into `wce_formats`. `unit_formats_check` rebuilds every VMD's tree from its doc.
2. **Assembly with pins** (2026-11-14, `wce_render`): `assemble_with`; no pins equals `assemble`, pinning one slot leaves the others.
3. **The tool skeleton** (2026-11-14, `wce_unit_tool`, feature `unit`): `VmdDocument`, the stage, the Unit files and Structure panels.
4. **Inspector and Preview** (2026-11-15): every field with undo; Preview as, seed, pins, grid, slot to play, colours, isolate.
5. **Problems and the Units panel**, opening by unit (2026-11-14); clicking parts and their outline (2026-11-15).
6. **DB check** (2026-11-14, example `unit_tables_check`; findings above).
7. **DB editing** (2026-11-15; gate `unit_db_check`): the rows in the project, the "Unit's looks" section, a copy of the VMD for this
   unit, Export > To mod pack.

Built on top (2026-11-15): pictures for the file fields, the slots' alternatives, Unit files and Units (the editor's thumbnail
renderer); slot names with the ones the engine looks up (example `slot_kinds`, docs/re/variant_mesh_slots.md), warned in Problems; a
ship's definition drawn as the ship with its oars, weapons, sails and ropes (`ship.rs`, script command `unit_ships`); the stage's
weather chips.

## What the build found

- **A part's file** (a helmet's, a shield's: most of the 8,507 definitions under `variantmeshes/`, where references point; only 4,894 are
  under `variantmeshdefinitions/`) worn alone floats in the air. `Pins::focus` names the file the paths are in, wherever it is referenced,
  and `show_focus` has every slot on the way to it pick it: the unit is dressed through its own definition (or its mount's), the edited text
  served for the open file. Alone only when the unit's definitions never reference it. Opening such a file, a thread reads every
  definition's references once and finds a unit that wears it.
- **Generators per node:** the first assembler drew its numbers in tree order, so pinning one slot reshuffled the rest. Each node now
  draws from a generator seeded by the seed and its place: a seed gives another soldier than before 2026-11-14 (the editor's picks, never
  the game's).
- **The shared scene:** every tool's documents publish into one scene, so a tile would stand behind the soldier. The tool hides the other
  tools' layers while active; `Tool::deactivated` (new) shows them again.
- `XmlDoc::set` on an element without attributes used `'`; it takes the document's quote now.

## Left

- In game: a pack from Export > To mod pack, to see that `!wce_unit_tool` rows win and the unit wears the edited file.
- New `unit_variants` rows for a faction that has none; undo for the DB edits; officers', standard bearers' and musicians' colours.
- Referenced definitions edited live in the same preview (only the open file is live); Launch in game with the unit; Rome 2 (packs not on
  this machine); stats tables, Share project, animation browsing beyond the slot combo (all "when asked").

## Decisions (user, 2026-11-14)

- **Document:** a VMD file, with a "Preview as" unit + faction for the context; opening a unit opens its VMD.
- **Preview:** seed as the game, plus per-slot pins and a grid of seeds.
- **DB:** visual links first (`unit_variants`, `variants`, `mounts`, colours), as phase 2. Stats later, if ever.
- **Release:** a release tool behind its own Cargo feature `unit`, in the default features once usable, replacing the placeholder.
  Kept out of the `release` set 2026-11-21 (not ready for production).

## Open questions

- Whether pins should optionally be saved in the project (a per-VMD sidecar) to keep a "showcase" soldier across sessions.
