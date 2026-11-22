# One project for all of a tile's variations

Before this plan a tile project held one variation. A settlement has three (`minor`, `small`, `medium`), each a whole project of its own: the
same city three times over, every fix made three times, three times the space. This plan puts all of a tile's variations in **one
project**. Every layer, group and instance says which variations it belongs to (all by default), and so does every terrain layer.
An export makes the variation it is asked for: everything that belongs to it, and nothing else. The idea comes from Warhammer III's
catchment layers.

State (2026-11-22): planned and settled 2026-11-15 (the user's answers in "Decisions"), built the same day on
`feature/tile_variations` (steps 1 to 9), then **revised** after the user tried it ("Revision 1: one tile": no viewed variation, the
user works on the tile itself; R1 to R4 done 2026-11-15); in master. How it works now: docs/editor.md, "Tile variations"; the format:
[formats/tile_wce.md](../formats/tile_wce.md). The open items are under "Open" at the end of the as-built summary.

## What the games and kits do

**Attila** (and Rome 2, ToB: the same layout). A tile database entry (`_tile_database/TILES/<name>.xml`) holds what is shared: the size,
the tile set, the texture group, the links. Each `<VARIATION>` row holds what is not: `location` (the folder), `min_height`, `scale`,
`normal_strength`, `raw_data_tri_density`, a colour, `barbarian`. Each variation is a whole folder: `tile.ted`, `height_map_0.png`,
`blend_map.tif`, `ground_type_map.png`, sometimes `nogo.png`, `lf_influence_map.png`, `protection_map.dds`, plus BOB's intermediates
(`final_heights.dds`, `final_alpha.dds`, `tile_normal.tga`). The game picks a settlement's variation by its level. Nothing in the engine
knows that two variations are related.

Attila also has a second axis that lives **inside** one variation: escalation layers (`LayerExportType_Escalation1..3`, exported as
`bmd_data_escalation<n>`, one standing at a time). It stays as it is. A layer can be both "Escalation 2" and "small, medium only".

Measured on the Attila kit (2026-11-15, instances matched by type, key, position and orientation):

| Tile | minor | small | medium | in all three | shared by small and medium |
|---|---|---|---|---|---|
| `settlement_western_cities/western_city_b` | 130,674 | 141,519 | 146,017 | 92,353 | 101,172 |
| `settlement_western_cities/western_city_a` | 125,189 | 137,823 | 139,022 | 46,558 (46,785 with positions to 1 m) | 69,237 |

- In `western_city_b`, two thirds of each variation are exact copies. One project would hold about 220k elements instead of 418k.
- `western_city_a`'s variations really are different, not slightly moved: rounding positions to the metre finds almost nothing more.
  Merging has to match elements exactly. A loose match would change what one variation exports.
- The height maps differ between variations, and so do their `scale` values (minor 20.80, small and medium 23.0). The ground type maps are
  identical. The blend maps differ.
- 228 of the kit's 717 entries have more than one variation. Most are **random alternatives** (`plains/1x1/aa`, `ab`, ...: unrelated
  terrain that the game picks at random), not levels of one place. Merging those saves nothing. So merging is chosen per tile, never
  forced.

**Warhammer III** (`fro_settlements/aquitanie_unwalled`, read for the idea, not supported): one `.terry` project, one terrain, layers in
`.layer` files. A layer entity carries `<ECLayer export="true" bmd_export_type="catchment_01"/>`. Layers with an empty type go into
every export. A typed layer goes into that catchment's BMD only: there, deployment zones, capture points, AI hints and a tower for
"F2A" (land battle) against "Unwalled" (settlement battle). Catchments vary only the logic, inside one exported tile. Walled and
unwalled are still two separate tiles (`aquitanie_walled`, `aquitanie_unwalled`). What we take from it: **one source, layers tagged
with the outputs they belong to, the export filters.** We go further than Warhammer III: our tags select **whole exported tiles**,
terrain included.

## The model

### Variations

A tile's variations are its entry's `<VARIATION>` rows, named by their `location` (`minor`, `small`, `medium`; `aa`, `ab`). A
**combined project** holds some or all of them. The others stay single-variation projects, as today, or come from the kit or the
packs. Adding a variation to a combined project adds a row to the entry. The per-variation values (`min_height`, `scale`, colour,
`barbarian`, `normal_strength`, density) stay in the entry's rows, edited in the Tile panel as today.

### The scene (`tile.wce`, version 2)

- **Folders carry the variations, instances don't.** A layer or a group (`LayerInstance`, `UserInstanceGroup`, `INSTANCE_GROUP`,
  `LAYER_INSTANCE_GROUP`) can carry **`wce_variations="small,medium"`**. Without the attribute, it belongs to every variation. This
  follows tile.wce's convention: an attribute TEd lacks is named `wce_...`. An instance belongs to the variations of the folder it is in:
  what only one variation has goes in a folder for that variation, never in a shared one (the user, 2026-11-15).
- **It is inherited:** an element belongs to the variations that it and every ancestor list. A folder's attribute can only narrow
  its parent's set. A folder naming a variation that its parent excludes is flagged in Problems.
- **Variation folders** (`WceVariationFolder`, an element of the editor's own): a folder that exists only to hold what some variations
  have, at its place in a shared layer, so that the layer itself need not be split. The merge makes them (below) and the user can make
  them too (*New variation folder*). `to_variation` takes the folder away and puts its children in its place, so the variation's file
  has no trace of it. (`to_ted` drops a `Wce...` element with everything inside, so the unwrapping must come first.) A
  `LayerInstance` tagged by the user stays a layer in the export.
- The root lists the project's variations: `wce_variations="minor,small,medium"` on `<project>`. A single-variation project is the
  same format with one variation and no tags. **There is one format and one code path, whatever the number of variations.**
- An instance belongs to a variation whole, or not at all: what is inside it (a polygon's `point`s, a river's points, a spline's
  handles) never varies by itself.
- **Groups of some variations only** (`wce_group_variations="minor"` on a `UserInstanceGroup` or `INSTANCE_GROUP`, never a layer):
  the group is there in those variations. In the others it is not, and what its `children` holds takes its place, one level up
  (`to_variation` takes its lines away and dedents the contents). This keeps once what each level of a settlement holds in other
  groups (a level regrouped). In TEd a nested group's `position` is relative to its parent, so the contents keep their relative
  places and move with whichever group holds them in each variation.
- **Values of an instance per variation** (`<WceVariationValues wce_variations="small" position="..."/>`, right before the instance):
  TEd puts a Landscape element on the terrain when it saves, so the same building in two levels whose terrain differs differs in
  its `position`'s height. The merge matches elements that differ only there, and each variation whose value differs keeps it in
  such a line, which `to_variation` puts into the element. A container keeps its own the same way, in its `WceVariationStart`.
- The root's other parts (`modules`, `camera`) can differ between variations (their vegetation seeds, their camera): each variation's
  copy is kept in a `WceVariationFolder` there too.
- **View state** (`WceVariationStart`): TEd's `flags` on a layer or group (whether it showed, was active) often differ between
  variations of the same layer. A shared layer keeps the first variation's in its start tag; each variation whose value differs gets a
  `<WceVariationStart wce_variations="small" flags="..."/>` as the layer's first child, put back into the start tag by `to_variation`.
  The root does the same for any attribute (an older file's `version`: `plains/8x8` has 16 and 15).
- The elements' order is kept so that each variation's elements, read in file order, are exactly that variation's file. The merge
  below builds the file that way.
- `to_variation(text, v)` keeps what belongs to `v`, unwraps the variation folders, applies the view states and drops the attributes. The result is a
  single-variation tile.wce, which `to_ted` turns into TEd's file as today. Every export goes through it.

### The terrain (`terrain_layers.wcl`, version 4)

The stacks (`wce_tile::terrain_layers`) already have most of what is needed: a base, ordered layers, heights in metres, and a layer
that exports only with a scene layer (`depends`).

- A terrain layer gets its own **variations** (empty: all). Its effective set is its own set, narrowed by its `depends` layer's set
  when it has one.
- A combined project always has a `.wcl` file. It holds the shared base and every layer. The kit maps (`height_map_0.png`,
  `blend_map.tif`, `ground_type_map.png`) are made only by an export.
- **What shows.** The fields (`HeightField`, `BlendField`, `GroundTypeField`) hold the composite of the variations shown (Revision 1;
  first planned as one viewed variation). A stack composes and absorbs with a filter: a layer that belongs to no shown variation counts
  as hidden (`TerrainLayers::set_view`). Painting goes into the working layer, or the base without one. The renderer and everything
  else that reads the fields stays unchanged.
- **Heights per variation.** The stacks are in metres. An export quantizes each variation with its own row's `min_height` and
  `scale` while its composite fits that range (`HeightField::holds_range`), else with the composite's range, as the single-variation
  export does; no fingerprints.
- Painted maps that are not layered (`nogo.png`, `lf_influence_map.png`, TEd's painted `protection_map.dds`) are kept **per
  variation**, in `variations/<v>/`. Making them layer kinds could come later. A file that is identical in every variation is
  kept once, in the tile's folder. They stay per variation (the user, 2026-11-15).

### Files of the project

```
<projects>/assembly_kit/raw_data/terrain/tiles/battle/<set>/<tile>/
    tile.wce                 the scene, wce_variations on the root
    terrain_layers.wcl       base + layers, all kinds
    nogo.png ...             a painted map that is identical in every variation
    variations/<v>/...       what one variation has of its own: painted maps, files the project does not model
    <aa>/tile.wce ...        a variation that is not in the combined project: a single-variation project, as today
```

The entry stays at `_tile_database/TILES/<name>.xml`. If a variation is listed on the root of `<tile>/tile.wce`, the combined project
serves it. Otherwise its own folder does, or the kit, or the packs (`sources`' order).

BOB's intermediates (`final_heights.dds`, `final_alpha.dds`, `tile_normal.tga`) are left out of the combined project: they are not
inputs (BOB makes them again from TEd's files, docs/formats/tile_compile.md), and a copy from before an edit is stale anyway. The
migration's backup keeps the originals. The check below does not compare them.

## Working in the editor

As built after Revision 1 (below); the full description is docs/editor.md, "Tile variations".

- **The view**: every variation shown at once, each element once; the Scene panel's "Show:" checkboxes (Tile > Variations > Show;
  `tile_variations_shown <list|all>`) filter it. An element whose variations differ in a value is drawn as its display variation (the
  first shown one it belongs to) has it. The first version's Variation chip and ghosts are gone.
- **An edit changes every variation the element is in** (the user, 2026-11-15). There is no "this variation only" mode: something
  that must differ belongs in a folder for its variations.
- **Tags**: the Scene panel's Variations column and the Inspector's checkboxes for a layer, a group or a variation folder
  (`tile_variations <list|all>`); an instance shows its folder's, read-only. New elements go into the working layer and get its
  variations.
- **Making variations differ**: *Move to a variation folder* (*Only in <v>*, *In all but <v>*; `tile_to_variation_folder <list>`) and
  *Split for a variation* (a submenu; `tile_split_variation <v>`), one undo entry each. **Tidy** (Tile > Variations > Tidy,
  `tile_variation_tidy`; also run by Combine): variation folders whose contents are identical across their sets join, a folder that holds
  every variation of its parent is taken away; no export changes. An explicit command, so that Save stays a minimal edit.
- **Terrain**: a variations button on every terrain layer and *New layer for...*; a warning when the working layer is in no shown
  variation.
- **Problems** lists elements tagged with a variation their parent excludes or the project does not have, and folders of no variation.

## Exports and loaders

- **File > Export > For Assembly Kit**: writes every variation of the project (a checkbox list; all by default). Each gets its
  folder: `to_variation`, then `to_ted`, the maps from the stacks, the river files from its rivers, its own files. The entry is
  written with every row. TEd and BOB get a normal kit tile.
- **As exported tile / To mod pack**: compiles each chosen variation and puts every variation's row in the tile database.
- **Launch in game**: a submenu of the variations (`tile_launch_game --variation <v>`). The Map context's placement already names a
  variation, so a combined project serves the same.
- **Everything else** (the map context, the Terrain tool's export, vegetation) finds tiles through
  `sources::find_tile(roots, location, variation)`. For a variation served by a combined project, it returns a **materialized**
  folder under `<projects>/build/variations/raw_data` (in the kit's layout), a single-variation project made by `to_variation` and the
  maps' export. A stamp file (`wce_variation.stamp`: the scene's, the layers', the entry's and the files' names, sizes and times) says
  what it was written from. A stale folder is made again when it is asked for. It is throw-away (`build/`), so loaders don't change,
  and the space is used only while needed. Stage B's linking tiles are read from their own folders' BOB files: not combined tiles.

## Migration: Combine variations

One operation for every source: a variation can be the user's single-variation project, the kit's tile, or one extracted from the
packs (decompiled).

**When**: only from the menu, *Tile > Variations > Combine...* (`tile_combine [--ref <v>] [v...]`). Opening a tile never offers it
(the user, 2026-11-15). Any variations can be combined, random alternatives (`aa`, `ab`) too, but only when the user ticks them and
presses Combine: nothing combines by itself.

**The dialog**: the variations, each with its source (project, kit, packs) and the share of its elements that the reference variation
also has. A tick says whether it goes in; none is ticked at first, the shares are there to help choose. The **reference** variation
is the one with the most shared elements by default. It gives the terrain base.

**The scene merge** (`wce_tile::variations::merge`):

1. Each variation is read as tile.wce text (a project's as it is, the kit's through `to_wce`, the packs' through the decompile).
   Prefab references stay references: `strip_prefab_contents`, as Save does.
2. Merged from the root down. A container's children are a sequence of keys: an instance's key is its whole subtree's text, a
   container's is its start tag. The sequences are merged pairwise with a diff (Myers or patience, on hashed keys; the vegetation
   layers have 20k+ children, so not a quadratic LCS). The result contains every variation's sequence, in that variation's order.
   Matched containers are merged recursively. A container whose set is narrower than its parent's gets `wce_variations`. Instances
   whose set is narrower than their container's go into variation folders: a run of neighbours with the same set shares one folder,
   at the run's place, so the order of every variation is kept.
3. Two layers with the same start tag but different contents merge into one layer with tagged children. Two layers with the same
   `wce_layer_id` but different start tags (renamed in one variation) stay two layers. One gets a new id, and the terrain layers that
   depend on it in that variation are changed to the new id.
4. A container's start tag is matched without its view state (`flags`); the root's without any attribute value. What differs goes in
   `WceVariationStart` lines (above).
5. What the exact keys leave, in this order (found on the kit, 2026-11-15):
   - **Near matches**: keys without the height of `position` (and without the indent, so that the same element one level deeper
     matches). Most of `western_city_a` is this: its levels' terrains differ, and so does every Landscape element's y.
   - **Groups by their contents**: unmatched groups with the same tag and attributes (values aside) and at least half of the
     smaller one's children the same (near keys): a district whose pivot moved. Their values differ per variation (START).
   - **Regrouped**: a group of either side whose contents are among the other side's items there holds them for both
     (`wce_group_variations`), recursively.
   - The anchors where the order differs are chosen by **size**, not count: in `western_city_c` the `fields` layer (most of the
     tile) comes last in one level, and keeping more small layers in order had dropped it.

**The terrain merge**:

- Each variation's stacks are read (its `.wcl`, or a base made from its maps, in metres, with its row's scale).
- The merged base is the reference's base. As first built, each group of variations with the same base got a layer **"Variation base:
  small, medium"** right above it; since R2 the patches build on each other ("Variation patch: <variations>", below). Either is a
  **replacing** layer (`Layer::replace`): it holds that group's base whole in each 64 x 64 block where it differs from the reference's,
  and where it holds a block that block is what shows. A difference to add did not do (found by the check, 2026-11-15): heights in f32
  metres lose the steps of a flat tile (`dense_forest_plains/1x1/aa`: a range of 0.09 m, a step of 1.4 micrometres) against another of
  20 m, and TEd's blend pixels sum to 65529..65535, not the 65535 a covering layer pixel needs. Replacing is exact for every type.
  Painting into such a layer first makes the block it lacks from what shows below, then takes the edit.
- Each variation's own layers follow, merged in each variation's order by id, settings and samples: a layer that several variations
  have the same is kept once, with their set; an id two different layers had is made new.

**The check, before anything is written**: every ticked variation is exported back from the merged project in memory and compared
with
its source:
- the scene: byte-identical to the source's tile.wce (and through `to_ted`, to the kit's tile.ted for a kit source);
- the maps: byte-identical, through the kit's encoder, with the row's `min_height` / `scale`;
- the painted maps and other files: byte-identical.

If anything differs, nothing is written and the report shows the first difference per file. **Combining is lossless by construction,
and every run checks it.**

**Writing**: the combined project is written. The single-variation projects that were merged are moved, not deleted, to
`<projects>/backup/combine/<date>/<location>/<variation>/`, and the log says where. The kit and the packs are never touched.

**Back out**: *Tile > Variations > Separate...* (`tile_separate <v>`) writes one variation as its own single-variation project
(`to_variation` and the maps) and removes it from the combined project. Its elements are dropped, and so are the layers no other
variation has.

## Other games

- Rome 2 has no kit tiles. Combining from the packs (decompiled variations) works the same.
- Warhammer III, if it comes: its catchments (`bmd_export_type`) are an axis inside one tile, like Attila's escalation layers. They
  would sit beside variations, not replace them.
- The tags are names of the entry's variations. Nothing in the model is specific to Attila.

## As built

Steps 1 to 9 (2026-11-15), each checked on `orc_city_a` combined from the kit and by the examples below; the detail is in the git history.

- `wce_tile::variations`: `to_variation`, the tags and `View` (step 1); `merge` and `merge::check` (step 2); `edit` (`set_variations`,
  `wrap_in_folder` / `unwrap_folder`, `split_for`, `tidy`) and `problems` (step 5); `materialize` (`served`, `build_root`, `writing`:
  one write at a time) (steps 6, 7); `combine::combine` / `combine_with` and `combine::separate` (`without_variation`) (step 8).
- Terrain (step 3): `Layer::variations` and `Layer::replace`, `.wcl` version 4, `TerrainLayers::set_view`, `for_variation`,
  `combine_in`, `Stack::combine_by`. "tile.wce version 2" is the root's `wce_variations`: `wce_version` stays 1.
- The editor (steps 4, 5): `TedScene::set_shown` (`get`, `position`, `euler`, `flags` read the display variation's own values,
  `world_transform` skips the groups it goes through, `find_all` lists only shown elements), `TileProject::show_variations`,
  `refresh_view`, `materialize`; the element index and the Scene panel's tree (`Tile::tree_children`) go by the view; readers of the whole
  scene take `TileProject::variation_scene()`.
- Exports (step 6): every export of a combined project goes through `materialize`: For Assembly Kit (`save_variations_for_kit`), As
  exported tile and To mod pack (`ExportInputs::parts`), Launch in game (`TileDocument::with_variation_alone`); `--variations a,b` on
  `save_for_kit`, `save_exported`, `save_to_pack`. The procedural roads (owner `road:<id>@<variation>#<signature>`) and kit walls
  (`wall:<key>@<variation>#<signature>`, `Tile::kit_walls_laid_for_export`) have layers per variation; an export views each variation
  alone in turn (`Tile::export_view`). Combined projects are the editor's only: Import from Assembly Kit (now File > Revert > To the
  Assembly Kit's version) does not make one (the user, 2026-11-15).
- Loaders (step 7): `sources::find_tile`, `find_tile_folder`, `serves`. The linking tiles stage B blends with are read from their own
  folders' BOB files, which a combined project has none of: a linking tile is not one to combine.
- Combine and Separate (step 8): the dialog (`combine_ui.rs`, `tile_combine_dialog [tick <list>]`), the commands `tile_combine [--ref
  <v>] [--align] <v>...` and `tile_separate <v>`; the projects that went in are moved to `<projects>/backup/combine/<when>/...`, the
  separated one's old files to `<projects>/backup/separate/<when>/`. **Align heights** (the user, 2026-11-15): a level that is mostly a
  copy of an earlier one takes that one's range and its samples within a centimetre; its export changes and the report says how much.
- After the plan: a variation keeps its carried terrain and vegetation (67d5d98b: the other variations' rows tried as the game's tile
  database has them), the combined tiles marked in the Tiles panel, the radar exported per variation.
- Examples: `tile_combine_check` (the gate below), `tile_materialize_check`, `tile_find_check`, `tile_combine`, `tile_variation_of`.

### Open

- The Gates' scripted run (combine `western_city_b`, split, paint, export, compare) is not written; `scripts/tile_variations.txt` is
  an older script (a project's other variations opening from the kit until saved).
- Adding a variation to a combined project: Combine needs a tile not combined, so Separate one and Combine again does not do it.
- Not seen in a shot: the context menu's submenus and Launch in game's (a script does not open menus). Not checked in the map context:
  no kit map places a combined tile.

## Results

`tile_combine_check` on the Attila kit (2026-11-15, `assembly_kit/raw_data/terrain/tiles/battle`): **154 tiles combined, every
variation byte-identical** as tile.wce and, through `to_ted`, as the kit's tile.ted. 74 entries list variations the kit has no
tile.ted for. About 6 s for a big settlement (three 50 MB files).

- With exact matching only, most vanilla settlements shared little: `western_city_b` 49 to 56%, most others under 5%. What looked
  like regrouping was mostly the terrain: every Landscape element's height differs between levels whose terrains differ, and so do
  the groups' pivots. With near matches, groups by contents, regrouping and size-weighted anchors (2026-11-15), shares of each
  variation's instances that another has too: `western_city_a` 36 to 59%, `western_city_b` 69 to 83%, `western_city_c` 65 to 73%,
  `sassanid_city_a` 52 to 64%, `sassanid_city_c` 35 to 67%, `western_port_b` 1% to 41 to 58%. Over the kit's 154 tiles, 5.77M
  instances become 4.21M (92% before) and 3,405 MB of scene files 2,643 MB (3,195 MB before); no tile shares less than before.
  `western_city_a` is 115 MB instead of 159 MB.
- Settlements made by copying one level into the next share a lot: TDD's `dunedain_city_cair_andros` 97 to 99%, `middle_men_city_edoras`
  95%, `orc_city_a` small/medium 88 to 94%, `middle_men_city_a` minor/small 70%. 31.8 MB of `orc_city_a` become 17.7 MB.
- Random alternatives (`plains`, `town_greek_plains`) share nothing; their combined file is about the sum of theirs.
- **Maps** (the same run, 2026-11-15): every variation's height map (its 16-bit values in its own range), blend map and ground type
  map come back from the combined stacks byte for byte, for all 154 tiles. The 13.3 GB of maps become 3.3 GB of
  `terrain_layers.wcl` (deflated; the blend maps are 52 MB of raw TIFF each): `orc_city_a` 161.9 MB in 9.9 MB, `western_city_b`
  158.2 MB in 17.3 MB.
- Found on the way, not part of this plan: `to_ted(to_wce(tile.ted))` is not the kit's file for `orc_city_a`'s three variations:
  TEd wrote `parallax_scale="-0"` and `ted_number_text` shortens it to `0`.

## Gates

- `tile_combine_check` on the Attila kit's multi-variation tiles: every variation byte-identical after the merge, for scene and maps.
  The count of tiles where the merge shares nothing (random alternatives) is reported, not a failure.
- A scripted run (not written yet; `scripts/tile_variations.txt` is another script): combine `western_city_b`, split a building for
  `small`, move it, paint a height layer for it, export For Assembly Kit into a scratch kit. Then `minor` and `medium` are
  byte-identical to the kit's, and `small` differs only where the edits went.
- Round trip of version 1 projects: open, save, unchanged bytes.

## Revision 1: one tile (the user, 2026-11-15)

The first version had the user pick a variation to work in (the Variation chip) and see the tile as that variation has it. That is not
what the user wants: **the user works on the tile itself**, and marks layers, groups, folders and terrain patches as belonging to one or
more variations. There is no variation to be in. What stays: the file formats, Combine and Separate, the exports, the loaders, every
mark and folder, the per-variation roads and walls of the exports.

- **The view: everything, with filters** (decision 7). The viewport, the Scene panel and picking show every variation's elements
  at once, each element once. The Scene panel's header gets a toggle per variation (and `tile_variations_shown <list|all>`): what
  belongs to no shown variation is hidden, like a hidden layer. All shown at first; a view setting, not saved in the tile and not
  undone. Showing one variation alone shows exactly its tile. No chip, no ghosts (the filters replace them).
- **Where an element is drawn.** An element whose variations differ in a value (a `WceVariationValues` line, a container's
  `WceVariationStart`, a group that is a group in some variations only) is drawn as its **display variation** has it: the first shown
  variation (in the root's order) that it belongs to. Combine lists the reference variation first, so with everything shown that is
  the reference. Edits keep their rule (decision 1, and 9): a move moves each variation's copy by the same amount.
- **The terrain: one terrain with patches** (decision 8). The base is the reference's; every difference is a terrain layer tagged
  with its variations (Combine's "Variation patch" layers among them), shown or hidden like any layer. The viewport composes the base
  with every visible layer belonging to a shown variation (everything shown: the patches of every variation together; one shown:
  exactly its terrain). Painting goes into the working layer, whose variations say which variations get it; the panel warns when the
  working layer belongs to no shown variation (the paint would not show).
- **Per-level heights stay** (decision 9): Combine keeps each level's own height of a shared building (`WceVariationValues`), the
  exports are byte-identical to the originals; the editor draws the display variation's.
- **The tools that stay** (decision 10): the layer, group and folder checkboxes (Inspector, the Scene panel's Variations column),
  *Move to a variation folder*, *Split for a variation* (a submenu of the variations: the selection copied into a folder for the one
  chosen, the originals into one for the others), the terrain layers' variations (checkboxes on every layer, *New layer for...* a
  choice of variations). Tidy and Problems stay.
- **What the editor makes over the terrain** (the procedural roads' layers and pieces, the kit walls, the vegetation generator, the
  siege AI, the minimap) is made for the display variation of the whole tile: the first shown variation. The exports make each
  variation's own, as now.
- **Launch in game** asks which variation (the first shown at first). The Tile panel's variation chooser shows the combined
  project's variations and their rows (height range, colour...), and no longer switches anything.

Steps R1 to R4 were done 2026-11-15 (`variations::display_variation`, `View::of_shown`, `TedScene::set_shown`, the Scene panel's Show
checkboxes; `Stack::combine_by` and the patches; the tools by the display variation, `Tile::of_tile_variation`; docs/editor.md rewritten
and whole-window shots looked at). **Patches build on patches** (the user, 2026-11-15): Combine's terrain merge goes through the
variations in the root's order, each against what the layers so far give it, so `small`'s changes are one layer of `small, medium` and
`medium`'s own one of `medium` over it. Found on the port: its `medium` does not chain its heights (over 60 % of the map its 16-bit
values are `small`'s stored with another range, 0.6 m lower), hence Align heights. Generate SiegeAI needs one variation shown alone.

## Decisions (the user, 2026-11-15)

1. **An edit changes every variation the element is in.** Something meant for one variation does not sit in a shared folder: it
   goes in a folder for that variation. So only layers, groups and variation folders carry variations, never an instance.
2. **Combine is menu-only.** Opening a tile never offers it.
3. **BOB's intermediates are left out** of the combined project and made again by every export; the backup keeps the originals
   (proposed after the user was unsure, accepted when the plan was settled).
4. **nogo, LF influence and protection maps stay per variation** (`variations/<v>/`).
5. **Random alternatives can be combined**, but only when the user ticks them and presses Combine; nothing is ticked at first.
6. **Ghosts of the other variations are in the first version** (step 5), off by default. Replaced by decision 7.
7. **No viewed variation: everything shown, with a filter per variation** (Revision 1). No chip.
8. **One terrain with patches**: the reference's base, every difference a layer tagged with its variations.
9. **Per-level heights stay**: exports byte-identical; the editor draws one (the display variation's).
10. **The tools that stay**: layer, group and folder checkboxes, Move to a variation folder, Split for a variation (chosen in a
    submenu), the terrain layers' variations.
