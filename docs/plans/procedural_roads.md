# Tile editor: terrain layers and procedural roads (plan, 2026-11-07)

Draw a spline and a road forms over it: the ground bumped and its sides levelled, a texture and a ground type painted, detail laid along it,
bridges over the gaps. All of it lives in layers of its own, so it can be changed or removed without touching the base terrain. The layers
are a feature in their own right, modelled on the newer Terry's: height, blend and ground type layers, which come first. Terry only shows how
the feature works: its projects are not imported (user, 2026-11-07).

Status (2026-11-07): Phases 1 and 2 are built (`wce_tile::terrain_layers`, `tile_document/layers.rs`, `wce_tile_tool/layers_panel.rs`). Roads are not.

## Decisions (user, 2026-11-07)

- **Layers first**, as Terry has them: several per map type, in priority order, set as the working layer like a scene layer. A change goes
  to the working layer, never to the base. They apply in order over the base, so terrain is made by composition.
- **Battle tiles first.** The data model stays generic, so that maps can have them later; the Terrain tool does not get them now.
- **A road's height follows the terrain**: the ground under its centre line, smoothed to a maximum grade, plus its bump. A control point can
  pin a height.
- **Bridges from the kit's pieces first**, chained straight over the gap; generated ones later, on the procedural walls' code.
- **Decals first, models later**: the kit's decals and small props now, a spec for a standard model set to author later.

## What the example shows (Warhammer III, `nuln_map_reduced_wh1`)

A `.terry` project has a `QTU::TerrainMap` per type (`Height`, `Blend8`, `GroundType`), each with an ordered list of
`QTU::TerrainMapLayer`: `id`, `name`, `visible`, `opacity`. Each layer is a file beside the project, `<name>.<type>.<id>.tif`, all 1280 x 1280
for this 16 x 16 tile (LZW). Decoded:

| Type | Pixel | Nothing there | Seen in the example |
|---|---|---|---|
| Height | 32-bit float | 0 | base almost all 0 (1 638 043 of 1 638 400). River -30 .. 32, hills at 35, 50, 60, 70: round offsets |
| Blend8 | 8 weights, one byte each | all 0 | sums of 255 where painted; base almost empty, `Streets` covers 25% of the tile |
| GroundType | one byte | 16 | `Roads` is 8 (road in our `GROUND_TYPES`), `River` 6, 7, 8 |

So: **height layers add, blend layers go over what is below by their weights, a ground type layer replaces where it is not 16**, and
`opacity` scales a layer. A hidden `Guide` layer (`visible="0" opacity="0"`) shows layers are used as drafts too. ASSUMPTION: the composition
is read from the data; Phase 0 checks it against what Terry itself builds. There is no sign of masks or blend modes in the project file.

Attila differs in one place: 16 blend channels (4 groups x 4 variations), not 8. Layers keep the tile's own channels.

## Layers

**Model** (`wce_tile::terrain_layers`). A `Stack` per type beside the tile's field: its base, and an ordered list of `Layer { id, name, visible,
opacity, owner, samples }`. A layer keeps its samples over the rectangle it was painted in: most of it is empty, and a whole blend layer is
50 MB. `owner` is empty, or the procedural feature that writes the layer.

**The fields hold the composite.** `TileProject::height`, `blend` and `ground` keep being the terrain as it shows, so nothing reading them changes
(there are 230 uses of `.height` alone), and a kit save, which writes them, gets the terrain the layers make without knowing of layers.

**Editing goes by difference** (changed from the first plan, which routed the brushes): nothing edits a layer. Brushes, Spline Road, stamps and
imports edit the field as ever, and when the edit closes `Stack::absorb` compares the field with what the stack composes and takes the
difference into the working layer, or the base without one. So every edit works on layers with no code of its own, undo and redo too.
A height layer takes the change itself (scaled by its opacity); a blend layer works out the least cover that gives the new pixel; a ground type
layer takes the new pixel, or stays empty where it equals what is below. The fields tell the stack apart from the display with their own
rectangle (`take_edited`, beside `take_changed`), and the composite written back is not an edit.

**Undo goes by the stack's state.** Each edit's record in the field has a serial; `Stack::sync` keeps the state after an edit under its serial, and
when the field comes back to a serial it knows, that state returns. Layers and the base keep their samples in 64 x 64 blocks shared between
states, so a state costs what its edit touched. A change of the stack itself (add, show, hide, move, merge, remove, rename, opacity) is an undo
entry of its own, `LayerStackCommand`; an opacity drag or a rename is one entry. The working layer is a choice, not an edit, and is not undone.

**A hidden working layer takes the paint** (user, 2026-11-07): it goes where the user said it goes, it just does not show until the layer does, and
the panel warns. A layer's opacity at 0 counts as hidden.

**UI.** A *Terrain layers* section in the Tile panel: a tab per type, the list as a stack (top row applies last, Base at the bottom), an eye per
row, a click makes it the working layer; name, opacity, up, down, Merge down and remove for the working layer. `terrain_layer` is its command.
A road's layers will show its icon and be read-only (not enforced yet: nothing owns a layer so far).

**Files.** `terrain_layers.wcl` beside the tile (changed from a `tile.wce` element with Terry's TIFFs): one compressed file with the bases and
layers. TEd never sees it, a tile with no layers has no such file, and saving after the last layer goes removes it. Terry's TIFFs stay an import.

**Saves.** The tile's maps hold the composite, in the project and in a kit save; the base is only in `terrain_layers.wcl`. It is not copied to the kit.

## Procedural roads

**The element.** `WceRoadSpline`, as `WceWallSpline`: an open polyline of its own kind that TEd never sees, its results regenerated. The tile
has its settings; the Inspector shows them; **Create** panel, new section. Reuses `wce_tile::splines`.

**Settings:** road width, shoulder width, bump height, maximum grade, maximum side slope, texture channel, ground type, edge noise, detail
set, seed. Presets for path (3 m), road (6 m), paved road (9 m).

**Generation** reads the terrain composite *below* its layers, so a road never feeds on itself, and fills three layers it owns, one per type:

1. **Centre line.** Sample the spline every metre or so; the ground under each sample, smoothed along the line; then clamped forward and
   backward to the maximum grade; pinned points held. The bump is added on top.
2. **Height layer.** Across the road, flat at the line's height (tilted a little to a crown if the bump asks). From the road's edge outward the
   ground meets the original over the shoulder; where the slope would pass the maximum side slope, the bank is widened until it holds, up
   to a limit. This is what "level the sides to a maximum degree" means: the bank's width comes from the height it has to cover.
3. **Blend layer.** The chosen channel at full weight on the road, feathered with noise over the edge so that it is not a clean line.
4. **Ground type layer.** Road where the road is, 16 elsewhere. Units and speed in the game follow it.
5. **Detail** (below) and **bridges** (below), as generated instances.

**When it runs.** While dragging, heights and blend on a fast path over the road's own rectangle, with the detail following when the drag
ends, as walls do. A layer under a road changing marks the road stale: the Inspector offers a refresh, and it never rebuilds on its own while
a brush is painting.

**Crossings.** Roads sharing a point weld at it: each ends flat at a common height. Road layers apply in the list's order, so a later road
wins where two overlap. Reaching a tile's edge, the road can end flat at a height the neighbouring tile can match; matching across tiles is
left for later.

**Existing objects** on the terrain follow as they do under a brush (landscape instances). Trees and props on the road are not removed in the
first version, but the road reports them; an optional "clear vegetation" comes later.

## Detail: prefabs along the road

**What the kit has** (`art/prefabs/global/paths`): 8 `global_dirt_road_*` and 6 `global_path_*`, plus `race_track`. Each is 8 to 24 projected
decals with 0 to 10 buildings. Across all of them the buildings are few: `global_ground_pathborder` (29), `lowfence_3` (19), `lowfence_1`
(5), `tree_stump` (2). The decals are `global_terrain_rocks_*`, `global_terrain_path_01`, `border_1`, `path_1`, puddles.

**This is good news.** Decals are projected, so they take the ground's shape whatever it is, and only need a position and a turn. A
prefab mostly needs no mesh bent at all. So "slightly altered to follow the road" means the *layout* is bent, not the models:

- Read a prefab's children as (distance along, distance across) a straight line.
- Lay it down a stretch of road: each child moves to that place on the curve and turns with it. Decals stay decals, and a building stays the
  same building, rigid.

**When a rigid piece is enough.** A straight piece of length L on a curve of radius R is off the curve by L x L / 8R at its middle:

| Piece | R = 30 m | R = 60 m | R = 100 m |
|---|---|---|---|
| 2 m | 2 cm | 1 cm | 1 cm |
| 4 m | 7 cm | 3 cm | 2 cm |
| 10 m | 42 cm | 21 cm | 13 cm |

Pieces up to 4 m never need bending on a road that a unit can follow; a 10 m piece does below about 60 m. So the set is **short pieces, placed
rigid**, which avoids a generated model and a pack per tile. Bending stays possible (`wce_walls::warp`) for anything longer.

**Standard set (decals now, models to author later).** Three styles: dirt, gravel, stone. Per style:

| Piece | Size | Variants | For |
|---|---|---|---|
| Road surface decal | 8 x 4 m | 3 | ruts, wear down the middle |
| Edge decal | 6 x 2 m | 3 | a broken edge on each side |
| Spot decal | 2 to 4 m | 3 | puddles, stones, patches |
| Verge or kerb model | 4 m | 3 | a low solid edge; the only piece needing a mesh |
| End cap | 2 m | 1 | where a road stops |
| Junction patch | 12 x 12 m | 1 | hides a crossing |

So 14 pieces a style, 42 in all, of which 6 decals (2 a style) and 12 models are new work once the kit's decals are reused for the rest; no curved
versions are needed. Three variants is the fewest that hides repetition when they are shuffled with the seed.

## Bridges

**The kit has** `bridge_wood_1`, `bridge_wood_2`, `bridge_wood_20m`, each with an `_end`, `bridge_wood_complete`, `bridge_stone_1`, `pontoon` and
`road_planks`, all in `models_building` with logic files. Vanilla's `river_crossing` tiles use `bridge_wood_complete` 29 times, and the separate
pieces 11 times. `battle_bridge_subculture_jcts` is empty in Attila's kit, so nothing there ties bridges to cultures.

**Finding the gap.** Walk the centre line before the height layer is made: where the road would have to rise or cut past a limit against the
ground, or the ground under it is water, is a gap. Its ends are where the banks can reach. Too short to bother, or wider than the longest
chain: reported on the road, with no bridge.

**Laying it.** The chain is straight, so a gap's line is straightened between its ends. Pieces are fitted to the length with `_end` pieces at
both ends; the road's height ends at the deck's. The height layer leaves the ground under the bridge alone. Buildings go in as generated instances
carrying the kit's own logic, so units cross them as in vanilla.

**Later**, generated bridges: any length and curve, on `wce_walls` (warped meshes, generated platforms and pathing). Not before the rest is proven.

## Save for Assembly Kit and the game

As walls do: detail and bridges go down into a top layer *WCE procedural roads* (Import from Assembly Kit drops it and restores the road), and
the terrain goes as the composite. Nothing new has to go in a pack while pieces are rigid kit assets, which is a large simplification over walls.

## Phases

| Phase | Work | Done when |
|---|---|---|
| 0 | Check the composition rules against Terry's own output | not done: Warhammer III's packs gave no flattened tile quickly. The rules are in one place (`Kind` in `terrain_layers.rs`) |
| 1 | Layer model, composite, file, saves flatten | done |
| 2 | Working layer, brushes and undo on it, the panel | done |
| 4 | `WceRoadSpline`, Create panel, Inspector, height, blend and ground type generation | drawn road on a test tile, and on the game's battle map: units take its ground type |
| 5 | Detail: bent layout of the kit's prefabs, seed, `Clear` | the road reads as a road from close up |
| 6 | Crossings, tile edge ends | two roads weld |
| 7 | Bridges from kit pieces | units cross a generated bridge in a battle |
| 8 | Standard models (art), generated bridges | later, on their own |

Phases 1 and 2 are useful on their own.

## Risks and open questions

- **Composition is inferred** (Phase 0). The riskiest guess is the blend's "over".
- **Bridges and pathing:** a bridge's logic may expect flat ground at its ends, or the game's pathing a certain ground under it. Battles are the only test.
- **The game's own map of the tile:** does a height change need more than the height map (`ai_hints`, baked pathing)? Spline Road works today, so probably not.
- **Rivers** cut by a road, and a road over a river's baked mesh (a decompiled tile's), are untested.
- **Memory:** a float layer at Attila tile sizes is several MB; sparse rectangles keep that down, but a tile with many roads pays for each.
- **Editing after the fact:** a hand edit on a road's layers is lost on regeneration, which is why they are read-only. A hand edit goes on a layer above.
