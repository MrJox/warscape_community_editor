# The Unit tool

A tool for editing a unit's looks: its **variant mesh definitions** (VMDs), shown on the unit's own skeleton, animated, in the renderer.
Later, the DB rows that decide which VMD a unit wears. It replaces the `unit` placeholder in `wce_app::tools`. It builds on the unit
rendering in [units.md](units.md); that plan's decision "Unit tool viewer: dev-only" is replaced by the decisions below.

State: planned 2026-11-14, decisions taken with the user (the list at the end). Nothing built yet.

## What the code has to build on

- **VMD format** (`wce_formats::variant_mesh`): `VariantMeshFile { doc: XmlDoc, root, repaired }`. It is a tree (`VariantMesh`, `Slot`,
  `Alternative::{Mesh, Reference}`) over a lossless `XmlDoc`. Each node keeps its `element` index. Attribute edits (`XmlDoc::set`) change only
  their span; structural edits (`with_child`, `removed_element`, `duplicated_element`, `moved_element`, the in-place journal) re-parse, so
  element indices shift. The tree is a snapshot: today the only way to rebuild it is `parse(doc.save())`, because `mesh()` and `slot()` are
  private. Attributes outside the tree (`use_different_attach_point_parts`, `mask3`) and unknown children stay in the doc, untouched.
  TDD's 11 malformed files parse through `repair()` (`repaired = true`), and their `doc` holds the repaired text.
- **Assembly** (`wce_render::unit_assembly::assemble(read, definition, seed)`): a seed decides everything (slot probability, the alternative,
  the tint, the decal of a `.texture_array`), through private code. There is no way to ask for a given alternative. Attachment points come
  from the parent model's RMV2. `.texture_array` is read only inside the assembler.
- **Units** (`unit_catalog::UnitCatalog`, `unit_animation::UnitModel`): `UnitModel::load(read, catalog, unit, faction, seed)` resolves
  `unit_variants` to the VMD (`variant_path`: the `variant` column used as the file name), the land unit's animation table, the mount, the
  scale and the faction colours. `play(slot)` sets a looping schedule, and `instances(t)` is pure in `t`. `man.parts` is public, so the parts
  can be replaced without touching the animation. The catalog keeps string projections only and reads mods (first file wins).
- **In a tile** (`wce_tile_tool::ref_units`): a working example of all of this. The catalog and models load on threads; the entities start
  as placeholders and are posed each frame with `scene.update_pose`; the Inspector has a filterable combo (private `RefUnits::combo`), a seed
  with Next, colour overrides and a Plays combo.
- **Tools and documents** (`wce_editor::Tool`, `wce_editor_core::Document`): the Weather tool's `XmlFileDocument` is the model to copy.
  It holds a `CommandStack<XmlDoc>`, takes undo with mergeable commands broken on `gesture_ended`, and saves to `<projects>/loose/<vfs path>`,
  which is mounted above the packs. The Weather tool also shows how to have a scene without a tile (`SampleMapDocument`, a `stand_in`) and
  how to aim the orbit camera.
- **Packs and DB** (`wce_walls::pack::write_pack_as` writes files and rpfm `DB` tables; `wce_walls::db` has row copy and cell edits;
  `wce_vfs::mod_pack` merges files into a pack). Launch in game writes a battle XML with `<unit_type>` per side (`wce_launch::battle_xml`).
- **What is missing:** skinned meshes are not in the id pass (no click picking, no outline); there is no shared VFS file picker, searchable
  combo or DB row picker.

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
  tree's path (slot names and indices, so they survive the re-parses that shift element indices). The same RNG draws are consumed whether
  a choice is pinned or not, so pinning one slot doesn't reshuffle the others. `assemble` stays as the call with no pins.
- The document's own tree goes to the assembler, not the file in the VFS, so unsaved edits show. References load from the VFS (an open
  referenced VMD is not live; see Later).

### Panels

- **Units:** search over `unit_variants` (unit, faction, variant), open a unit's VMD. Mods included (as the catalog reads them).
- **Files:** the VMDs in the VFS, by folder, with their owner (pack, mod, project). A shared `VfsPicker` widget (`wce_editor`), which the
  model and texture fields reuse.
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

- No tile: a flat ground stand-in (a disc of a game ground texture, or the Weather tool's sample map), lit by the weather preview
  (`lights_scene` false, as the Tile tool), the orbit camera focused on the figure. Attila's default environment until the user picks one.
- The figure at the origin, a mount under its rider. In the grid, soldiers in a row at the unit's spacing.
- **Selection in the viewport:** skinned meshes are not in the id pass. First version: the selected node's parts outlined by drawing them a
  second time (or with "isolate"). Clicking a part to select its node needs skinned meshes in the id pass (a step of its own, also wanted by
  `ref_human`'s selection outline).

### DB: the tables that decide looks (phase 2)

- **Tables:** `unit_variants` (faction, unit, variant, height_scale, height_variation, unit_card), `variants` (variant_name, variant_filename,
  low_poly_filename, scale, scale_variation, mount_scale), `mounts` (variant), `unit_variants_colours` (per unit and faction colours).
  Stats (`land_units`, `main_units`) are out of scope until asked for.
- **Check first:** the catalog takes `unit_variants.variant` as the VMD's file name and doesn't read `variants`. If `variants` maps
  names to different `variant_filename`s, or `scale` / `unit_variants_colours` change what a unit looks like, the catalog is wrong today,
  and the tile's references with it. Step 6 checks this over Attila, Rome 2 and TDD before any DB edit.
- **Storage:** the edited rows as rpfm DB files in the project, `<projects>/loose/db/<table>_tables/wce_unit_tool`, encoded by rpfm. The
  loose folder is mounted first, so the catalog and the preview see them, and they ship as they are.
- **Editing:** the Preview panel's unit gets a "Unit's looks" section: which VMD each faction's row uses (new rows for a faction that has
  none), height scale and variation, the colours. "Give this unit a copy of the VMD" duplicates the file under a new name and points the row
  at it (the common modding step), in one undoable command.
- **Export > To mod pack:** the project's VMDs and textures the tool wrote, and the DB files, through `write_pack_as` (merged into an
  existing pack as the UI tool does). A Launch in game for a unit (a custom battle with this unit on one side) is a later step:
  `battle_xml` already takes units.

### Other games

Rome 2 has the same VMD, anim and table formats (units.md): the tool works there with the same code. Later games (`wsmodel`, `.anim` v6+,
binary fragments) need their own readers behind the game profile; until then the tool is listed in their `unsupported_tools`.

## Plan

Each step ends with something visible and a test or a gate.

1. **Tree from a doc** (`wce_formats`): `VariantMeshFile::from_doc(XmlDoc)` (the tree without a save and re-parse), `TreePath` for a node
   (survives structural edits), the structural helpers in tree terms: add slot / mesh / reference / tint / meta, remove, duplicate, move.
   `.texture_array` parser moved into `wce_formats`. Unit tests: every operation changes only its span (minimal edit), and save after no
   edit is the input. Gate: `unit_formats_check` gains a pass that rebuilds every VMD's tree from its doc and compares it with the parsed one.
2. **Assembly with pins** (`wce_render`): `assemble_with(read, &VariantMeshFile, seed, &Pins)`, each part keeping the tree path it came
   from (for isolate, the outline and Problems). Tests: no pins equals `assemble` for every seed tried; pinning one slot leaves the other
   slots' picks unchanged.
3. **The tool skeleton** (`wce_unit_tool`, feature `unit`, in the default features; the placeholder goes): `VmdDocument` (open, undo, save to
   loose, revert), the stage, the Files and Structure panels, the figure on the default unit, playing `STAND`. Shot: a legionary on the
   stage, script-driven (`--script`, non-activating).
4. **Inspector and Preview:** every field above with undo; Preview as, seed, pins, grid, slot to play, colours, isolate. A shared
   `SearchCombo` and `VfsPicker` in `wce_editor` (`ref_units` switches to them). Shots: the same soldier with a slot pinned to each
   alternative; an edited tint.
5. **Problems and the Units panel**, opening by unit; the selection outline for skinned parts.
6. **DB check:** `variants`, `unit_variants_colours` and `scale` against what the catalog does, over Attila, Rome 2 and TDD (an example
   that lists the differences). Fix the catalog if it is wrong (the tile's references benefit too).
7. **DB editing:** the rows above in the project, the "Unit's looks" section, copy-VMD-for-this-unit, Export > To mod pack. Gate: the DB
   files written decode with rpfm and encode back to their bytes; an untouched row round trips.
8. **Later, when asked:** clicking skinned parts (id pass), referenced VMDs edited live in the same preview, Launch in game with the unit,
   stats tables, Share project, animation browsing beyond the slot combo (a clip list, the fragment's lines).

## Decisions (user, 2026-11-14)

- **Document:** a VMD file, with a "Preview as" unit + faction for the context; opening a unit opens its VMD.
- **Preview:** seed as the game, plus per-slot pins and a grid of seeds.
- **DB:** visual links first (`unit_variants`, `variants`, `mounts`, colours), as phase 2. Stats later, if ever.
- **Release:** a release tool behind its own Cargo feature `unit`, in the default features once usable, replacing the placeholder.

## Open questions

- Whether `variants.variant_filename` and `unit_variants_colours` matter (step 6 answers it from the data).
- The stage's ground: a plain textured disc or the Weather tool's sample map.
- Whether pins should optionally be saved in the project (a per-VMD sidecar) to keep a "showcase" soldier across sessions.
