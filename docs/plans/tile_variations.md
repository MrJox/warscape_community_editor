# One project for all of a tile's variations

Today a tile project holds one variation. A settlement has three (`minor`, `small`, `medium`), each a whole project of its own: the
same city three times over, every fix made three times, three times the space. This plan puts all of a tile's variations in **one
project**. Every layer, group and instance says which variations it belongs to (all by default), and so does every terrain layer.
An export makes the variation it is asked for: everything that belongs to it, and nothing else. The idea comes from Warhammer III's
catchment layers.

State: planned and settled 2026-11-15 (the user's answers in "Decisions"). Work on branch `feature/tile_variations`. Done: steps 1
to 9 (see "Steps" and "Results" below): done 2026-11-15. Not written: the Gates' scripted run (`scripts/tile_variations.txt`);
its parts were checked one by one (each step's "Checked"). **Revised 2026-11-15** after the user tried it: no viewed variation, the
user works on the tile itself ("Revision 1: one tile" below, decisions 7 to 10); steps R1 to R4 to do.

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
- **The viewed variation.** The fields (`HeightField`, `BlendField`, `GroundTypeField`) hold the composite of the variation being
  viewed. A stack composes and absorbs with a filter: a layer that does not belong to the viewed variation counts as hidden. Painting
  still goes into the working layer, or the base without one, through the existing `absorb` / `onto` path. Switching the variation
  composes the fields again (`export_into` with the filter). The renderer and everything else that reads the fields stays unchanged.
- **Heights per variation.** The stacks are in metres. An export quantizes each variation with its own row's `min_height` and
  `scale`. It keeps the row's values while that variation's composite is unchanged since the merge or import. Fingerprints are kept
  per variation. Otherwise it computes new values from the composite's range, as the single-variation export does today.
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

- **The Variation chip** in the viewport, next to the view modes (`tile_variation <v>`): the viewed variation. The scene shows
  what belongs to it; the terrain is its composite. The Tile panel's variation chooser switches the view instead of opening another
  project. *Show other variations* (in the first version, the user 2026-11-15) draws what belongs only to other variations as tinted
  ghosts: their instances, and the terrain's outline where theirs differs is left for later. Ghosts cannot be picked. Off by default,
  `tile_variation_ghosts on|off`.
- **The Scene panel's export column** (the export type of the nearest layer, dimmed when inherited) also shows the variations
  (`small, medium`, dimmed when inherited). The Inspector has a checkbox per variation for a layer, a group or a variation folder
  (`tile_variations <list|all>`). An instance has none: it shows its folder's, read-only.
- **An edit changes every variation the element is in** (the user, 2026-11-15). There is no "this variation only" mode: something
  that must differ belongs in a folder for its variations.
- **New elements** go into the active layer and get its variations, as in Warhammer III. If the active layer does not belong to the
  viewed variation, the new element would not show. The chip warns, as it does for a hidden working layer.
- **Making one variation differ.** *Move to a variation folder* (the selection's context menu; `tile_to_variation_folder <list>`)
  moves the selection into a variation folder at its place, made if needed. *Split for this variation* is the usual case in one undo
  entry: the selection is copied into a folder for the viewed variation, the originals go into a folder for the others, and the
  selection moves to the copy.
- **Tidy** (Tile > Variations > Tidy; also run by Combine): variation folders whose contents are identical across their sets join, a
  folder that holds every variation of its parent is taken away. No export changes. It is an explicit command so that Save stays a
  minimal edit.
- **Terrain**: *New layer for this variation* in the Terrain layers section makes a layer that belongs to the viewed variation. The
  panel shows each layer's variations and warns when the working layer does not belong to the viewed one (the paint would not show).
- Problems lists elements tagged with a variation their parent excludes, and variations that no longer exist (after one is removed).

## Exports and loaders

- **File > Export > For Assembly Kit**: writes every variation of the project (a checkbox list; all by default). Each gets its
  folder: `to_variation`, then `to_ted`, the maps from the stacks, the river files from its rivers, its own files. The entry is
  written with every row. TEd and BOB get a normal kit tile.
- **As exported tile / To mod pack**: compiles each chosen variation and puts every variation's row in the tile database.
- **Launch in game**: the viewed variation. The Map context's placement already names a variation, so a combined project serves the
  same.
- **Everything else** (the map context, stage B's linking tiles' heights, the Terrain tool's export, vegetation) finds tiles through
  `sources::find_tile(roots, location, variation)`. For a variation served by a combined project, it returns a **materialized**
  folder: `<projects>/build/variations/<location>/<variation>/`, a single-variation project made by `to_variation` and the maps'
  export. A stamp file holds the project's save fingerprint. A stale folder is made again when it is asked for. It is throw-away
  (`build/`), so loaders don't change, and the space is used only while needed.

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
- The merged base is the reference's base. Variations whose base is the same pixel for pixel form one group. For each group other
  than the reference's, a layer **"Variation base: small, medium"** goes right above the base, tagged with that group. It is a
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

## Steps

1. **`to_variation` and the tags** (`wce_tile::variations`): reading the effective sets, filtering, the root's list. Unit tests on
   small scenes. tile.wce version 2. Version 1 files read as one variation.
2. **The scene merge** and `tile_combine_check -- <kit tile folder>`: merges the kit's variations and checks each one comes back
   byte-identical. Run on every multi-variation tile of the Attila kit (228 entries). Also reports the space saved.
3. **Terrain tags** (done 2026-11-15): `Layer::variations` and `Layer::replace`, `.wcl` version 4, the view filter
   (`Stack::set_view_hidden`, `TerrainLayers::set_view` with the scene layers' sets from `variations::layer_key_variations`), the
   "Variation base" layers, the terrain
   merge. The check above extended to the maps.
4. **The document**: `TileDocument` over a combined project, the viewed variation, the chip, switching. Undo covers tag edits and
   copies. `variations::View` says how a variation sees the scene (what is not there, the folders and groups it goes through, its
   own values); the element index places and shows by it. `TileProject::view_variation` composes the viewed variation's terrain
   from the layers (a combined project has no maps: blank fields in TEd's layout, `HeightField::flat`, `TiffFile::ted_header`), and
   Save writes only `tile.wce` and `terrain_layers.wcl` for it.
   Done (2026-11-15):
   - Opening: a variation whose folder has no tile while its tile folder's `tile.wce` lists it (`variations::combined_for`) opens
     the combined project viewing it: by the entry, by the folder, from the Tile panel's variations and the browser. Another
     variation of the open project is viewed, not opened.
   - The Variation chip (top right of the viewport, left of the Areas chip; only for a combined project) and
     `tile_variation [<v>]`. Switching composes the terrain again, rebuilds the element index and the scene's entities, and
     makes the trees and the grass again. It is not an edit: nothing to undo.
   - The element index shows, picks and places by `View`; the Scene panel lists the viewed variation's tree
     (`Tile::tree_children`: a variation folder's contents in its place, other variations' elements left out).
   - What reads the whole scene rather than the index takes `TileProject::variation_scene()` (`to_variation` of the scene, made
     per call): the tree and grass generators' exclusion polygons, seeds and static trees.
   - Checked on `orc_city_a` combined from the kit: `small` viewed in the combined project renders the same as the kit's `small`
     opened by itself (top-down: identical pixels; low: the same but for a fire's animation).
   - The scene itself views the variation (`TedScene::set_variation`): `get`, `position`, `euler`, `flags` read its own values
     (`variations::own_value`), `world_transform` skips the groups it goes through, `find_all` lists only its elements
     (`in_variation`), so the readers of the whole scene (rivers, roads, walls, siege, the minimap, the Inspector) see what it has.
     An edit of a value that variations have of their own changes every variation: a `position` moves each by the same amount (each
     keeps its height on its own terrain), anything else becomes the same in each. `remove` and the moves take an element's
     `WceVariationValues` lines with it.
   Undo of tag edits and copies comes with step 5's tools.
5. **The editor's tools**: the Scene panel column, the Inspector checkboxes, variation folders, *Move to a variation folder*, *Split
   for this variation*, Tidy, the ghosts, the Terrain layers section, Problems.
   Done (2026-11-15):
   - `variations::edit`: `set_variations`, `wrap_in_folder` / `unwrap_folder`, `split_for` (the copies as the variation has them:
     `to_variation` of their lines), `tidy`; `variations::problems`. Each is one undo entry in the editor (`TileDocument::
     set_variations`, `to_variation_folder`, `split_for_variation`, `tidy_variations`), and a command: `tile_variations`,
     `tile_to_variation_folder`, `tile_split_variation`, `tile_variation_tidy`.
   - The Scene panel: a variations column (the element's, its own bright, inherited dimmed; a click on a layer's, group's or folder's
     ticks them), variation folders as rows of their own ("Only in small"), the context menu's *Move to a variation folder* and
     *Split for <viewed>*. The Inspector: a checkbox per variation for a layer, a group or a folder; an instance's, read-only.
   - Tile > Variations: Tidy, *Show the other variations*. The chip: the same checkbox, and a warning when the working layer is not
     in the viewed variation.
   - Ghosts (`tile_variation_ghosts`): each instance only other variations have, outlined (its model's bounds, its models loaded
     for it) where the first of them places it (`world_transform_as`), in one overlay that is not picked.
   - The Terrain layers section: a layer's variations after its name, dimmed when not in view; checkboxes for the working layer;
     *New layer for <viewed>*; a warning when the working layer is not in view. A change of a layer's variations, or of a scene
     layer's that terrain layers export with, composes the viewed terrain again (`TileProject::refresh_view`), undo too.
   - Problems: tags that name a variation the project does not have or the parent excludes, and a folder of no variation.
   - Checked: the kit's 154 tiles need no Tidy and have no Problems after the merge; on `orc_city_a` in the editor, a building split
     for `small` and moved changes `small` only; split then Tidy gives every variation back as it was; undo of a split leaves nothing
     to save.
   Left: the ghosts are outlines, not tinted models; the variations column is text, not icons.
6. **Exports**: For Assembly Kit, As exported tile, To mod pack, Launch in game. The per-variation height scale.
   Done (2026-11-15):
   - `TileProject::materialize` (`variations::materialize`): a variation written as a project of its own under a `raw_data` of the
     kit's layout: `to_variation` of the scene, its maps composed from the layers that belong to it (`TerrainLayers::for_variation`,
     also written as its `.wcl` so that a layer exporting with a "Don't Export" scene layer is still left out), its rivers' files, the
     files the variations share (the tile folder's) and its own (`variations/<v>/`) over them, the entry. The height map keeps its
     row's range while the composite fits it (`HeightField::holds_range`), else gets the composite's, and the entry written there has
     it: no fingerprints. The editor's root is `<projects>/build/variations/raw_data` (`TileDocument::variations_build_root`); step 7
     takes it over.
   - Every export of a combined project goes through it: For Assembly Kit writes each variation in its folder of the kit
     (`TileDocument::save_variations_for_kit`) and the entry with every row; As exported tile and To mod pack compile each variation
     (`ExportInputs::parts`); Launch in game the viewed one. File > Export ticks the variations (all at first); the commands take
     `--variations a,b` (`save_for_kit`, `save_exported`, `save_to_pack`).
   - Checked on `orc_city_a` combined from the kit (`examples/tile_materialize_check.rs`, and the editor): every variation's tile.ted
     is the kit's as the editor reads and writes it back (TEd's `parallax_scale="-0"` aside, as for any kit tile), its height, blend and
     ground type maps and the entry the kit's byte for byte; `small` compiled from the combined project is the kit's `small`
     compiled, all 57 files byte for byte, once its `protection_map.dds` is in `variations/small/` (where Combine will put it).
   - The procedural roads: in a combined project each variation a road is in has its own layers of it, made over its own ground and
     belonging to it alone (owner `road:<id>@<variation>#<signature>`, layer "Road <id> (<variation>)"); the variation viewed makes
     its own, the others' stay. An export views each variation it writes in turn (`Tile::export_view`: the project's view only, not
     the index or the entities), so that its roads' layers and pieces, and its building splines' items, are made on its ground; the
     variation viewed is viewed again after. The water a road crosses is the viewed variation's rivers (`river_ribbons`).
     Checked: a road drawn across `orc_city_a` where `minor`'s ground differs: after For Assembly Kit, three layers of each type,
     `minor`'s pieces on its ground (0.4 m lower), `small` and `medium` the same where theirs is.
   - The kit walls, the same way: each variation a wall is in has its own height layer of it (`wall:<key>@<variation>#<signature>`,
     "Kit wall <name> (<variation>)"), and an export lays each variation's walls on its ground and lowers that layout
     (`Tile::kit_walls_laid_for_export`, `VariationExport::kit_walls`). Whether a variation's export makes buildings (walls, road
     bridges) is its own (`VariationExport::generated`); `has_walls` of a combined project reads the scene as viewed, not the index.
     The procedural walls' export already read each variation's own project (its heights, its scene).
     Checked: a kit wall drawn across `orc_city_a` where `minor`'s ground differs, As exported tile of `minor` and `small`: two
     height layers, each variation's maps with its own pads (616 pixels differing by more than 5 cm), its layout its own (a step one
     piece later in `minor`), each packed with its walls.
   Combined projects are the editor's only: Import from Assembly Kit does not make one (the user, 2026-11-15).
7. **Loaders**: materialization in `sources::find_tile`, `build/variations/`, the stamp.
   Done (2026-11-15):
   - `sources::find_tile`: a variation folder without a tile whose tile folder's combined project lists it is that project's
     variation written as a project of its own (`variations::materialize::served`) under the root's build folder
     (`materialize::build_root`: `<projects>/build/variations/raw_data` for `<projects>/assembly_kit/raw_data`, the exports' too).
     A stamp (`wce_variation.stamp`: the scene's, the layers', the entry's, the shared and own files' names, sizes and times) says what
     it was written from; it is written again when that changed. An export's write has no stamp (it is made from the editor's scene,
     roads and walls laid), so a loader writes it again. One write at a time (`materialize::writing`): the map context asks from
     several threads, an export from the editor's.
   - `sources::find_tile_folder` (no write: whether a tile is a custom-mesh one) and `sources::serves` (whether a root has it: the
     Terrain tool's check of the tiles a map places).
   - The map context's tiles come through `find_tile`. The linking tiles stage B blends with are read from their own folders' BOB
     files (`final_heights.dds`), which a combined project has none of: a linking tile is not one to combine.
   - Checked (`examples/tile_find_check.rs`): `orc_city_a`'s `medium` written in 1.7 s, taken as it is the next time, written again
     after its layers' file changed; it opens as a project of one variation with its row's numbers and its `protection_map.dds`.
     Not checked in the editor's map context: no kit map places a combined tile.
8. **Combine and Separate**: the dialog, the backup, the commands.
   Done (2026-11-15):
   - `variations::combine::combine`: each ticked variation read from its folder (a project's tile.wce, else tile.ted through `to_wce`;
     its `.wcl`, else stacks over its maps, with its row's range; its other files), the scenes merged and checked (`merge::check`),
     the terrain combined on the reference's base. A file every variation has the same goes in the tile's folder, the others (and each
     one's river files, for their factors) in `variations/<v>/`; BOB's intermediates are left out. The combined project is written to
     `<projects>/build/combine/` with its entry, each variation written back from it (`materialize`) and compared with its source:
     its scene through `to_ted`, its height map's samples and range, its blend and ground type maps and its files byte for byte.
     Only then are the projects that went in moved to `<projects>/backup/combine/<when>/<location>/<v>/` and the combined project put
     in their tile's folder (the entry copied into the projects when they have none).
   - `combine::separate`: the variation written as a project of its own in its folder (`materialize`), the combined project without
     it (`without_variation`: what only it has goes, its name leaves every list, a group of it only gives its place to its contents;
     then Tidy; its layers only it has go, and those exporting with a scene layer that went), checked that every other variation's
     scene and terrain stay as they were before anything is written; the old files to `<projects>/backup/separate/<when>/`. The last
     one separated leaves no combined project.
   - Tile > Variations > Combine... (a tile of several variations, not combined): the dialog lists the entry's variations with where
     each comes from (project, kit, packs: decompiled when read) and the share of its instances the reference has too, read on a thread;
     none ticked; a reference to choose (the one sharing the most at first). Combining runs on a thread, the tile closed (it must be
     saved), and opens it again combined. Tile > Variations > Separate > <variation>: saves, separates, opens again (the variation's
     own project when it was the one viewed). Commands `tile_combine [--ref <v>] <v>...`, `tile_separate <v>`; `examples/tile_combine.rs`.
   - Checked: `orc_city_a` from the kit combined (6.5 s, 31.8 MB of scenes in 14.2 MB), every variation the kit's
     (`tile_materialize_check`); `small` separated is that variation byte for byte, `minor` and `medium` stay the kit's; the other two
     separated (the last leaves the tile's folder with the three projects only), combined again from those projects (moved to the
     backup), still the kit's. In the editor: `tile_combine` on the kit's tile opens the combined project viewing `small`;
     `tile_separate medium` opens it again with `minor, small`. `western_city_b` (18 s, 166 MB in 88 MB) and `plains/1x1`'s random
     alternatives pass the check.
   Left: adding a variation to a combined project (Separate one and Combine again does not: Combine needs a tile not combined); the
   dialog itself was not seen (the script captures show the viewport only).
9. **Docs**: docs/editor.md (Where tiles are saved, Terrain layers, the new section), docs/formats (tile.wce version 2, `.wcl`
   version 4), this plan's state.
   Done (2026-11-15): docs/editor.md's **Tile variations** (Combine, the chip, editing, what is made per variation, exports,
   Separate), notes in Where tiles are saved and Terrain layers, the Tile menu, the export table, the build and backup folders;
   [formats/tile_wce.md](../formats/tile_wce.md) (new): tile.wce, a combined project's marks and folder, terrain_layers.wcl version 4.
   "tile.wce version 2" is the root's `wce_variations`: `wce_version` stays 1.

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
- A scripted run (`scripts/tile_variations.txt`): combine `western_city_b`, view `small`, split a building for it, move it, paint a
  height layer for it, export For Assembly Kit into a scratch kit. Then `minor` and `medium` are byte-identical to the kit's, and
  `small` differs only where the edits went.
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
  with its variations (Combine's "Variation base" layers among them), shown or hidden like any layer. The viewport composes the base
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

Steps:

- **R1. The view.** `TedScene`'s lens becomes the shown set: an element is there when it belongs to a shown variation, its values
  are its display variation's (`variations::own_value`, `world_transform_as` per element). The element index, picking, the Scene
  panel's tree and the readers of the whole scene go by it. The chip and the ghosts go; the header's toggles and
  `tile_variations_shown` come.
  Done (2026-11-15): `variations::display_variation`, `View::of_shown` (each element's display variation, its values, the groups
  it goes through), `TedScene::set_shown` (`get`, `in_variation`, `goes_through`, `set_attr` by the element's display variation),
  `world_transform` by it, `TileProject::show_variations` (a combined project opens with every variation shown; the first shown is
  the tile's: its row's range, its roads and walls), `TerrainLayers::set_view` of the shown set (a layer shows when it belongs to a
  shown variation that has its scene layer). The chip and the ghosts are gone; the Scene panel has "Show:" checkboxes over its header
  (the last one shown can't be unticked), Tile > Variations > Show the same; `tile_variations_shown [<list> | all]`. An export views
  each variation alone in turn and the shown ones again after. Checked: whole-window shots of the port (everything; `minor` alone:
  the escalation layers of `small, medium` gone, `minor`'s buildings and terrain); For Assembly Kit of `orc_city_a` from the editor
  still the kit's (but TEd's `-0`).
- **R2. The terrain.** `TerrainLayers::set_view` with the shown set; the Terrain layers section's checkboxes on every layer, *New
  layer for...*, the warning by the shown set. Combine lists the reference first.
  **Patches build on patches** (the user, 2026-11-15): a level is often the one below it changed (`medium` is `minor`'s terrain
  with `small`'s changes and its own). Combine's terrain merge goes through the variations in the root's order, each against what
  the layers so far give it: a block where it differs goes into a layer of every variation whose map has that block the same (so
  `small`'s changes are one layer of `small, medium`, `medium`'s own one of `medium` over it), instead of one "Variation base" per
  variation. The user does the same by hand: a terrain layer of `small, medium` under one of `medium`.
  Done (2026-11-15): `Stack::combine_by` makes the patches ("Variation patch: <variations>", replacing, per 64 x 64 block), heights
  compared as each variation stores them (`TerrainLayers::combine_in` with the rows' ranges: the same 16-bit values in its range);
  Combine lists the reference first. The Terrain layers section: a variations button on every layer (its checkboxes in a popup),
  *New layer for...* (the variations ticked), a layer in no shown variation dimmed, the warning when the working layer is in none;
  the minimap follows the variations shown (`Tile::view_version`). `gui_close_header` for captures of a panel's lower sections.
  Checked: kit tiles chain (`orc_city_a`: a height patch of `small, medium` and one of each's own; `western_city_b` alike for heights
  and blend); every variation still comes back byte for byte (the port: all files of the three original projects). The port's
  `medium` does not chain its heights: over 60 % of the map its 16-bit values are `small`'s but stored with another range (min
  -20.66 against -20, scale 134.49 against 134.27), so it stands 0.6 m lower there; Combine keeps it as it is (one patch of
  `medium` over most of the map). Shots: the Terrain layers section with everything shown and with `minor` alone.
  **Align heights** (the user, 2026-11-15: `medium` was mostly a copy of `small`): Combine's option (the dialog's checkbox,
  `tile_combine --align`, `combine::combine_with`). A level aligns to the earlier one whose stored samples it shares the most of
  (their ranges differing): per 64 x 64 block it is read the way that agrees with that one for most of the block (its samples in that
  one's range: copied, then saved with another range; or its own heights: saved again, rounded otherwise), a sample either way within
  a centimetre of that one's height takes it exactly, and the level takes that one's range (or what holds its heights). Its instances'
  own heights that only followed the shift (the terrain's move under them, within 5 cm) go. The check: the other levels byte for byte;
  the aligned one's heights as aligned (a step of rounding allowed where it shares a patch, said). Its export changes: the report says
  how many samples took the other's heights and how many own heights went. The port: `medium` aligned to `small` (its range -20 /
  134.27 now), 1,380,620 of its 1,638,400 height samples `small`'s, 14,690 of its 14,758 own heights gone; the patches `small`, `small,
  medium` and `medium` (its own changes: 16 % of its samples, within 0.3 m, its larger town's ground); `minor` and `small` byte for
  byte.
- **R3. The tools.** *Split for a variation* as a submenu; the roads, walls, vegetation and siege of the display variation; Launch
  in game's choice; the Tile panel's chooser.
  Done (2026-11-15): *Split for a variation* is a submenu of the variations (`tile_split_variation <v>`), *Move to a variation folder*
  has *In all but <v>* for each. What the editor makes over the terrain is the tile's variation's (the first shown;
  `Tile::of_tile_variation`): the roads made are its roads, the kit walls' ground layers its walls' (the others' show, laid, without
  one). The prefab references' ground: every reference's, whatever is shown, its layer tagged with its variations (a normalized one
  levelled on the ground as shown: approximate where the variations' ground differs under it). Generate SiegeAI needs one variation
  shown alone: its layout in a layer of it alone, an earlier generated one of it replaced, the layouts in shared layers left (said in
  the log). Tile > Launch in game is a submenu of the variations (`tile_launch_game --variation <v>`): its export and its deployment
  zones (`TileDocument::with_variation_alone`). The Tile panel's variations of the open combined project no longer switch anything.
  Checked in the editor (the port, aligned): a building split for `medium`; SiegeAI refused with all shown, made for `medium` alone; a
  launch dry run of `small` exports `small`.
- **R4. Docs and checks.** docs/editor.md's Tile variations section rewritten; whole-window shots (`shot <file.png>`) of the
  Scene panel, the Inspector, the Terrain layers section and the Combine dialog, looked at before it is called done.

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
