# Tile editor: terrain layers and procedural roads (plan, 2026-11-07)

Draw a spline and a road forms over it: the ground bumped and its sides levelled, a texture and a ground type painted, detail laid along it,
bridges over the gaps. All of it lives in layers of its own, so it can be changed or removed without touching the base terrain. The layers
are a feature in their own right, copied from the newer Terry: height, blend and ground type layers, which come first.

Nothing here is built yet. Status: plan, for review.

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

**Model** (`wce_tile`). `TerrainLayers<T>`: an ordered list of `TerrainLayer { id, name, visible, opacity, owner, pixels }` over the map of
its type. `owner` is empty, or the procedural feature that writes the layer. The existing `HeightField`, `BlendField` and `GroundTypeField`
stay what they are and become the base. Everything that reads the terrain (renderer, heights under objects, siege, exports) reads a
**composite**: the base with the layers applied. With no layers it is the base itself, at no cost.

**Composite.** Worked out on change, only over the dirty rectangle. Height adds, in floats. A layer is sparse in practice (most of a
tile is untouched), so each keeps its bounding rectangle and is skipped elsewhere.

**Editing.** The working layer is a new state beside the scene's. Brushes, Spline Road, Level, stamp and import write to it instead of the
base. Height brushes on an additive layer need care: *flatten to 10 m* has to write `10 - (what the layers below give)`, so a brush works on
the composite and stores the difference. Undo keeps the layer's changed rectangle, as a stroke does now. A brush with no working layer of its
type still edits the base.

**UI.** A *Terrain layers* section in the Tile panel: a tab per type, a list with visibility, opacity and name, new, duplicate, delete,
reorder by dragging, merge down, and which one is the working layer. Layers a road owns show its icon and are read-only, with a button to
select the road. Styled as the links and tile database sections are now.

**Files.** `tile.wce` keeps them: a `WceTerrainLayers` element (the file's convention: TEd never sees it) listing them, the pixels in files
beside it with Terry's naming, `<tile>.<type>.<id>.tif`. Importing a Terry project's layers is then close to free.

**Saves.**
- A tile with no layers must save byte for byte as it does now: nothing new is written.
- *Save for Assembly Kit* writes the composite as the height, blend and ground maps: TEd has no layers, so it gets the terrain they make
  (our polyfill rule). The layers stay in `tile.wce`.
- The game's export uses the composite too.

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
| 0 | Check the composition rules against Terry's own output; read the layers' opacity in use | a Warhammer III project's layers compose to what Terry builds |
| 1 | Layer model, composite, `tile.wce` files, saves flatten | layers made by command show in the viewport, a kit save holds the composite, no-layer tiles round-trip byte for byte |
| 2 | Working layer, brushes and undo on it, the panel | painting goes to a layer; hide it and the change goes |
| 3 | Terry import | `nuln_city`'s layers open in the editor (Warhammer's textures aside) |
| 4 | `WceRoadSpline`, Create panel, Inspector, height, blend and ground type generation | drawn road on a test tile, and on the game's battle map: units take its ground type |
| 5 | Detail: bent layout of the kit's prefabs, seed, `Clear` | the road reads as a road from close up |
| 6 | Crossings, tile edge ends | two roads weld |
| 7 | Bridges from kit pieces | units cross a generated bridge in a battle |
| 8 | Standard models (art), generated bridges | later, on their own |

Phases 1 and 2 are useful on their own and can ship before any road exists.

## Risks and open questions

- **Composition is inferred** (Phase 0). The riskiest guess is the blend's "over".
- **Bridges and pathing:** a bridge's logic may expect flat ground at its ends, or the game's pathing a certain ground under it. Battles are the only test.
- **The game's own map of the tile:** does a height change need more than the height map (`ai_hints`, baked pathing)? Spline Road works today, so probably not.
- **Rivers** cut by a road, and a road over a river's baked mesh (a decompiled tile's), are untested.
- **Memory:** a float layer at Attila tile sizes is several MB; sparse rectangles keep that down, but a tile with many roads pays for each.
- **Editing after the fact:** a hand edit on a road's layers is lost on regeneration, which is why they are read-only. A hand edit goes on a layer above.
