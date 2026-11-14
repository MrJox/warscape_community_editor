# One project for all of a tile's variations

Today a tile project holds one variation. A settlement has three (`minor`, `small`, `medium`), each a whole project of its own: the
same city three times over, every fix made three times, three times the space. This plan puts all of a tile's variations in **one
project**. Every layer, group and instance says which variations it belongs to (all by default), and so does every terrain layer.
An export makes the variation it is asked for: everything that belongs to it, and nothing else. The idea comes from Warhammer III's
catchment layers.

State: planned and settled 2026-11-15 (the user's answers in "Decisions"). Work on branch `feature/tile_variations`. Done: steps 1
and 2 for the scene (`wce_tile::variations`, `to_variation`, `merge`, `check`, `examples/tile_combine_check.rs`); see "Results" below.

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

**The terrain merge**:

- Each variation's stacks are read (its `.wcl`, or a base made from its maps, in metres, with its row's scale).
- The merged base is the reference's base. Variations whose base is the same pixel for pixel form one group. For each group other
  than the reference's, a layer **"Variation base: small, medium"** goes right above the base, tagged with that group. For heights it
  holds the difference in metres (0 where equal). For blend and ground type it holds that group's base pixels where they differ and
  nothing elsewhere. A blend pixel of the base sums to 65535, so a full-cover layer pixel gives it back exactly. A ground type pixel
  with alpha 255 replaces.
- Each variation's own layers follow, merged by id and samples: a layer with the same id and samples in several variations is
  kept once, with their set.

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
3. **Terrain tags**: `Layer::variations`, `.wcl` version 4, the compose and absorb filter, the "Variation base" layers, the terrain
   merge. The check above extended to the maps.
4. **The document**: `TileDocument` over a combined project, the viewed variation, the chip, switching. Undo covers tag edits and
   copies.
5. **The editor's tools**: the Scene panel column, the Inspector checkboxes, variation folders, *Move to a variation folder*, *Split
   for this variation*, Tidy, the ghosts, the Terrain layers section, Problems.
6. **Exports**: For Assembly Kit, As exported tile, To mod pack, Launch in game. The per-variation height scale.
7. **Loaders**: materialization in `sources::find_tile`, `build/variations/`, the stamp.
8. **Combine and Separate**: the dialog, the backup, the commands.
9. **Docs**: docs/editor.md (Where tiles are saved, Terrain layers, the new section), docs/formats (tile.wce version 2, `.wcl`
   version 4), this plan's state.

## Results

`tile_combine_check` on the Attila kit (2026-11-15, `assembly_kit/raw_data/terrain/tiles/battle`): **154 tiles combined, every
variation byte-identical** as tile.wce and, through `to_ted`, as the kit's tile.ted. 74 entries list variations the kit has no
tile.ted for. About 6 s for a big settlement (three 50 MB files).

- Most vanilla settlements share little as files. CA rebuilt or regrouped each level: in `western_city_a` minor's "Buildings" holds
  338 groups at other places than small's 3,428, so the same buildings sit in different groups. Shares of each variation's instances
  that another has too: `western_city_b` 49 to 56%, `sassanid_city_a` minor/small 50%, `western_port_a` minor/small 31%,
  `western_city_rome` small/medium 45% / 28%, most others under 5%.
- Settlements made by copying one level into the next share a lot: TDD's `dunedain_city_cair_andros` 97 to 99%, `middle_men_city_edoras`
  95%, `orc_city_a` small/medium 88 to 94%, `middle_men_city_a` minor/small 70%. 31.8 MB of `orc_city_a` become 17.7 MB.
- Random alternatives (`plains`, `town_greek_plains`) share nothing; their combined file is about the sum of theirs.
- Found on the way, not part of this plan: `to_ted(to_wce(tile.ted))` is not the kit's file for `orc_city_a`'s three variations:
  TEd wrote `parallax_scale="-0"` and `ted_number_text` shortens it to `0`.

## Gates

- `tile_combine_check` on the Attila kit's multi-variation tiles: every variation byte-identical after the merge, for scene and maps.
  The count of tiles where the merge shares nothing (random alternatives) is reported, not a failure.
- A scripted run (`scripts/tile_variations.txt`): combine `western_city_b`, view `small`, split a building for it, move it, paint a
  height layer for it, export For Assembly Kit into a scratch kit. Then `minor` and `medium` are byte-identical to the kit's, and
  `small` differs only where the edits went.
- Round trip of version 1 projects: open, save, unchanged bytes.

## Decisions (the user, 2026-11-15)

1. **An edit changes every variation the element is in.** Something meant for one variation does not sit in a shared folder: it
   goes in a folder for that variation. So only layers, groups and variation folders carry variations, never an instance.
2. **Combine is menu-only.** Opening a tile never offers it.
3. **BOB's intermediates are left out** of the combined project and made again by every export; the backup keeps the originals
   (proposed after the user was unsure, accepted when the plan was settled).
4. **nogo, LF influence and protection maps stay per variation** (`variations/<v>/`).
5. **Random alternatives can be combined**, but only when the user ticks them and presses Combine; nothing is ticked at first.
6. **Ghosts of the other variations are in the first version** (step 5), off by default.
