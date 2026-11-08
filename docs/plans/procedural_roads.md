# Tile editor: terrain layers and procedural roads (plan, 2026-11-07)

Draw a spline and a road forms over it: the ground bumped and its sides levelled, a texture and a ground type painted, detail laid along it,
bridges over the gaps. All of it lives in layers of its own, so it can be changed or removed without touching the base terrain. The layers
are a feature in their own right, modelled on the newer Terry's: height, blend and ground type layers, which come first. Terry only shows how
the feature works: its projects are not imported (user, 2026-11-07).

Status (2026-11-08): the terrain part (layers, height, texture, ground type: Phases 1, 2, 4 and 6) is built and stays. The first detail and
bridges (old Phases 5 and 7: the kit's path prefabs strung along the road, chains of the kit's bridge buildings) were not up to spec and are
redesigned below (branch `feature/road_detail_redesign`): road pieces from `roads_proc` prefabs fitted by overrides, crossings of water and
drops as fords or bridges, bridges generated on the walls' code. Left for later: pinned heights.

## Decisions (user, 2026-11-07)

- **Layers first**, as Terry has them: several per map type, in priority order, set as the working layer like a scene layer. A change goes
  to the working layer, never to the base. They apply in order over the base, so terrain is made by composition.
- **Battle tiles first.** The data model stays generic, so that maps can have them later; the Terrain tool does not get them now.
- **A road's height follows the terrain**: the ground under its centre line, smoothed to a maximum grade, plus its bump. A control point can
  pin a height.
- ~~Bridges from the kit's pieces first~~ and ~~decals first, models later~~ (2026-11-07): replaced by the decisions of 2026-11-08 below.

### Redesign of the detail and bridges (user, 2026-11-08)

- **Road pieces are prefabs of our own** in `roads_proc` (the projects' prefab folder), built from the kit's decals and small props: one
  basic design per main piece (straight, curve, corner, T junction, X junction, ...), the base of every road type (dirt, mud, ...). The
  spline system lays them along the road and adapts each with **prefab overrides** (a slightly curved stretch takes the straight piece,
  bent about its centre).
- **`roads_proc` lives in the projects folder**, made per game. The base pieces start as **placeholders**: measure splines where the real
  assets go, so whoever draws a piece (the user, or Claude) knows where decals and buildings go for the feature to recognise and adapt them.
- **The first road type is generated** from the kit's decals, to see how it works in the tool; refined by hand afterwards.
- **Standard measures**: a 6 m road, 8 m straights, 12 x 12 m junctions, 4 m ends; the width scales from 0.5 to 2 times.
- **Editing as procedural walls**: a road is not edited piece by piece; its pieces and bridges are made again from its line and settings.
- **Crossings**: water (rivers, lakes, the sea) and big drops. **A crossing less than 0.75 m deep under water is a ford; deeper is a bridge.**
- **Bridges are generated on the walls' logic** (warped base pieces, generated platforms): the kit's bridge buildings are too restrictive.

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

**Export with a scene layer** (user, 2026-11-07). A layer's `depends` names a scene layer by its `wce_layer_id` (an attribute the editor gives it, which TEd never sees). An export leaves the layer out when that scene layer exports as Off, its own `export_type` or, for a normal layer, the one it is in; a scene layer that is gone leaves it in. Escalation layers cannot have terrain (one height, blend and ground map a tile), so they count as exported. Kit saves pass `export_maps` to `save_for_kit_scene`; the game export puts them over the project's files in `save_exported_with`, which every export goes through. The height map keeps the tile's range. Checked: kit and game exports with the scene layer on and off differ in the heights only (the game's mesh, normals and grass follow). This is what would let one project hold several variants of a map, terrain per level, once escalation-like levels can have terrain.

**Undo goes by the stack's state.** Each edit's record in the field has a serial; `Stack::sync` keeps the state after an edit under its serial, and
when the field comes back to a serial it knows, that state returns. Layers and the base keep their samples in 64 x 64 blocks shared between
states, so a state costs what its edit touched. A change of the stack itself (add, show, hide, move, merge, remove, rename, opacity) is an undo
entry of its own, `LayerStackCommand`; an opacity drag or a rename is one entry. The working layer is a choice, not an edit, and is not undone.

**A hidden working layer takes the paint** (user, 2026-11-07): it goes where the user said it goes, it just does not show until the layer does, and
the panel warns. A layer's opacity at 0 counts as hidden.

**UI.** A *Terrain layers* section in the Tile panel: a tab per type, the list as a stack (top row applies last, Base at the bottom), an eye per
row, a click makes it the working layer; name, opacity, up, down, Merge down and remove for the working layer. `terrain_layer` is its command.
A road's layers are not listed at all (user, 2026-11-07), nor can a command reach them, so they cannot be broken by hand. They apply after every layer of the user's: a new layer goes under them, and a stack saved otherwise is put right when the tile opens. They go with their road. An edit under them is taken by what it did to what shows (`Kind::onto`), not by the pixel it left, or a road's texture would stay in the layer below after the road went.

**Files.** `terrain_layers.wcl` beside the tile (changed from a `tile.wce` element with Terry's TIFFs): one compressed file with the bases and
layers. TEd never sees it, a tile with no layers has no such file, and saving after the last layer goes removes it. Terry's TIFFs stay an import.

**Saves.** The tile's maps hold the composite, in the project and in a kit save; the base is only in `terrain_layers.wcl`. It is not copied to the kit.

## Procedural roads

**The element.** `WceRoadSpline`, as `WceWallSpline`: an open polyline of its own kind that TEd never sees, its results regenerated. The tile
has its settings; the Inspector shows them; **Create** panel, new section. Reuses `wce_tile::splines`.

**Settings:** `road_width`, `road_shoulder`, `road_bump`, `road_max_grade`, `road_max_side`, `road_group` and `road_variation` (its texture),
`road_ground`, `road_feather`, `road_seed`, in the Inspector and `road_set`. Not yet: pinned heights, presets, the detail set.

**The line** is a Catmull-Rom curve through the points: no handles, the points are where the road goes. Its outline draws that curve.

**Generation** reads the terrain composite *below* its layers, so a road never feeds on itself, and fills three layers it owns, one per type:

1. **Centre line.** Sample the spline every metre or so; the ground under each sample, smoothed along the line; then clamped forward and
   backward to the maximum grade; pinned points held. The bump is added on top.
2. **Height layer.** Across the road, flat at the line's height (tilted a little to a crown if the bump asks). From the road's edge outward the
   ground meets the original over the shoulder; where the slope would pass the maximum side slope, the bank is widened until it holds, up
   to a limit. This is what "level the sides to a maximum degree" means: the bank's width comes from the height it has to cover.
3. **Blend layer.** The chosen channel at full weight on the road, feathered with noise over the edge so that it is not a clean line.
4. **Ground type layer.** Road where the road is, 16 elsewhere. Units and speed in the game follow it.
5. **Pieces**, **fords** and **bridges** (below), as generated instances.

**When it runs.** The layers are derived data: a road's signature (its line and settings) is kept in its layers' owner, `road:<id>#<signature>`.
A road whose signature differs, or a height edit anywhere, makes the roads again once the line has stopped moving and no stroke is open, so a
drag makes its road at the end, and undoing an edit of a road needs nothing of its own. Layers of a road that went are taken out; opening a tile
makes nothing again.

**Grass** is made again with the road: generated grass takes the new blend map and heights, and packed or generated, no patch stands on a road (`Tile::road_at`, the roads' ground type layers), as a tile's packed list was made before it. So does a stack change, as a layer shown or hidden moves the ground under it. With generated grass that costs about 0.6 s on a large tile each time the roads are made.

**The height at a point** comes from the road's samples near it, by distance, further than 12 past the edge: taken from the nearest alone it jumped where
a point stopped being nearer one stretch than another, in steps across the banks (`the_terrain_a_road_leaves_has_no_steps`).

**Crossings.** A road reads what lies below its layers, so a later road takes an earlier one for ground and joins it at its height. Its bump stays
off where it runs on the other (that one's is already in the ground). The bump also comes in over the road's first and last 8 units, so a road
ending at a tile's edge ends on the ground. Matching a neighbouring tile's road is left for later.

**Existing objects** on the terrain follow as they do under a brush (landscape instances). Trees and props on the road are not removed in the
first version, but the road reports them; an optional "clear vegetation" comes later.

## Road pieces (redesign, 2026-11-08)

### What was there, and why it goes

The first detail (`wce_tile::road_detail`) strung the kit's `global_dirt_road_*` and `global_path_*` prefabs end to end along the line,
each part moved to its distance along and across the road. Those prefabs were made for one straight spot, so laid in a row they read as
a repeated pattern, not as a road; nothing knew a junction, an end or a bend, and the decals kept their size whatever the road's width. It
is replaced, and `road_detail` (the setting and the module) with it.

### The folder

`<projects>/assembly_kit/raw_data/art/prefabs/roads_proc/`, one folder per road type, each with the same pieces:

```
roads_proc/
    base/       straight  curve  corner  t_junction  x_junction  end  ford     (guides only: the placeholders)
    dirt/       the same keys, guides and the kit's decals                       (generated, refined by hand)
    <type>/     ...
```

- A road type is a folder; `road_type` (in place of `road_detail`) lists the folders but `base`, and `none`. A type missing a piece
  takes `base`'s, which lays nothing but its guides' measures (a warning names the missing piece).
- The editor writes `base/` and `dirt/` into a game's projects when the Tile tool mounts them and they are missing, and never writes over
  a file that is there: the user's edits are kept. `dirt/` is written only when the game's kit has its decals; else the log says so.
- They are ordinary `.wce` prefabs: the Prefab tool opens, edits and saves them like any other.

### The measures and the guides

Every piece is drawn at the standard measures, in the prefab's frame: origin at the piece's centre on the road's centre line, the road
along +z, its width across x, the ground at y = 0. Metres below; the file's units are twice as many in a battle tile.

| Piece | Size | Ports (where the next piece joins) | Laid where |
|---|---|---|---|
| `straight` | 8 long, 6 wide | z = -4 and +4 | every stretch, bent up to `STRAIGHT_BEND` |
| `curve` | 45 degrees on a 15 m centre radius (11.8 along the centre) | its two ends | bends tighter than a straight takes |
| `corner` | 12 x 12, two arms at 90 degrees | the two arms' ends | a sharp point |
| `t_junction` | 12 x 12, three arms | the three arms' ends | a road's end on another road |
| `x_junction` | 12 x 12, four arms | the four arms' ends | two roads crossing |
| `end` | 4 long | z = +2 | a free end (none at the tile's edge) |
| `ford` | 8 long, as a straight | z = -4 and +4 | over a ford, the length of the crossing |

The placeholders are **guides**: `WceRoadGuide` elements, open polylines of the editor's own (TEd never sees them, `project_files::to_ted`
drops them as it drops every `Wce*` element), drawn in the Prefab tool and hidden in a tile. A guide's `guide_role` is one of:

- `centre`: the road's centre line, port to port (a junction or corner: one per arm, from the centre out);
- `edge`: the road's edge each side of each arm, 3 m out;
- `verge`: the shoulder's outer limit each side, 3 m past the edge: where the road's buildings stand;
- `port`: a line across the road at a join, its points the two edges.

The ports are what makes a piece a piece: the reader finds the piece's arms and their directions from its ports and centre guides, so a
piece drawn to other measures still reads (its own length and width are what it is fitted by). A piece without them is not used.

### How a part is anchored

A part is a decal, building or simple mesh of the piece (its guides aside). Where it is, read once from the prefab, says how it follows
the road (`road_pieces::read`):

- **its arm**: the arm whose centre guide is nearest; in a straight, curve or ford, the one arm;
- **along**: its distance along that centre guide, as a fraction of the arm;
- **anchor**: `surface` when its origin is between the edges further than 1 m from them; `edge` within 1 m of an edge; `verge` past an
  edge; a building is never `surface` (one on the road is reported, and laid on the verge). A part may say otherwise with
  `road_anchor="surface|edge|verge|fixed"` (an attribute of the editor's, which a kit save drops), `fixed` staying where it is in the
  piece's frame (a junction's middle).

### Fitting a piece to the road

Every piece laid is a **prefab reference** (`wce_prefab="roads_proc/<type>/<piece>"`) with **overrides** (`WceOverrides`) for each part's
`position`, `orientation` and `scale`; no element is added or taken out, which overrides cannot do. For a part at `along` and `across` on
its arm:

- `along` goes to that fraction of the stretch the piece covers: the piece is stretched to fit, `round(stretch / piece length)` pieces a
  stretch as walls are cut, so a straight is 6 to 10 m long at most;
- `across`: a `surface` part's is scaled by the road's width over 6, an `edge` part keeps its distance from the edge (it moves out with
  the width, its size the same), a `verge` part its distance from the verge;
- the part goes to that place on the road's curve and turns with the curve's direction there; a decal's scale is stretched along by the
  piece's stretch and, `surface` only, across by the width;
- heights: decals are projected; buildings stand on the ground (Landscape: Above), as they do anywhere.

Decals cannot bend: a curve is made of its parts moved onto the arc, each rigid. So **a piece's parts are 4 m long at most** (the
off-curve table of the first plan: 7 cm at R = 30 m). A longer decal stays a straight chord of the road.

### Laying a road out

As the walls' solver, on the road's centre line (`road_layout`, a new module of `wce_tile`):

1. **Features first.** A point marked `sharp` (as walls' points are), or a turn of 90 degrees or less, takes a `corner`, its arms turned
   to the real angle (down to 30 degrees, past which the point is reported and laid as a corner at 30). A road's end within 8 m of another
   road snaps onto it and takes a `t_junction` there, on the other road; two roads crossing take an `x_junction`, its arms turned to the
   angles they cross at. A free end takes an `end`; an end on the tile's edge nothing. A ford takes `ford` pieces; a bridge nothing (it is
   its own, below).
2. **Stretches** between features: where the line bends more than `STRAIGHT_BEND` over a straight's length (15 degrees over 8 m, a radius
   of about 30 m), `curve` pieces, bent from their 45 degrees to the bend's own (between 15 and 90 degrees each); elsewhere straights.
3. **Fit** every piece (above); the seed turns pieces end for end and, later, chooses between variants of a piece (`straight_2`, ...).

A road crossing another is still graded over it (the terrain part, as built); the junction pieces only cover the meeting.

### As walls do

The pieces are derived data: made again from the road's line, settings and the pieces' files (their signature), 300 ms after the last
change, kept by that signature, not saved in tile.wce. Not selectable one by one: a click picks the road. Save for Assembly Kit and the
game export lower them into the top layer *WCE procedural roads*, the references baked to groups (as every reference is for TEd); Import
from Assembly Kit drops that layer and restores the road.

## Crossings (redesign, 2026-11-08)

What a road crosses is found once, on what lies below its layers, and both the terrain and the pieces read it (the first version found its
gaps twice, from samplers that did not agree). Along the centre line, a crossing is a stretch where:

- **water** is over the ground: inside a river's width about its curve (`SplineRiverInstance`, its height by the rule in docs/re/water.md:
  freeform at its stored height, landscape `terrain_offset` over the ground), inside a lake's `water_plane` instance (its scaled box, at
  its height), or under y = 0 in a tile of the sea tile sets; or
- **the ground drops away**: as now, the road graded over the ground stands at least 4 m over it for at least 8 m, run out to the top of
  the walls.

Crossings within 8 m of each other are one. A water crossing's **depth** is the deepest the water is over the ground along it:

- **under 0.75 m: a ford.** The road keeps to the bed (no bump, graded on the ground), its texture and ground type as elsewhere, `ford`
  pieces over it. The river is not changed.
- **0.75 m or more, or a drop: a bridge.** Its deck is at the water plus `road_bridge_clearance` (default 2 m) over water, rim to rim over
  a drop; the road climbs to it over its approaches at no more than its maximum grade (the terrain part raises them, and leaves the ground
  under the bridge as it is: `Strip::open_gaps`). `road_bridges` off: every crossing is a ford, whatever its depth, and a drop is graded
  through.

## Bridges (redesign, 2026-11-08)

### What was there, and why it goes

Chains of `bridge_wood_1` or `bridge_wood_2` with their ends, or the whole `bridge_wood_complete`, laid on the straight line between two
feet: straight on a curved road, longer than the gap by up to a piece, 5 or 10 wide whatever the road's width, wood only, and
`bridge_stone_1` unusable (no platforms). `choose_bridge` and `lay_bridge` go.

### Generated on the walls' code

A bridge is made as a wall is (docs/plans/procedural_walls.md, `wce_walls`), with what a bridge needs on top:

- **Bases**: a table, as `profile::BASES`, of the kits' bridge buildings to bend, per game: a deck piece, its end (abutment) if it has
  one, the deck's height and walkway width (from its logic file, as `examples/bridge_dump` reads them). Attila: `bridge_wood_2` with
  `bridge_wood_2_end`, `bridge_stone_1`. `road_bridge` chooses one, as `wall_base` does; the Inspector lists the bases the mounted packs
  have whole.
- **Warp**: the deck pieces along the road's line from rim to rim, `round(length / base length)` pieces, x by arc length; **the deck
  follows the road's height** (a slope, which walls never have: their walkways step), so the warp takes y from the road's profile; scaled
  across from its walkway's width to the road's (walls never scale across). The ends' pieces bent onto each rim, their ramps meeting the
  road's surface.
- **Supports**: the base's feet stretched down to the ground under them (the bed of a river, the floor of a drop), as a wall's foot is
  stretched to stay buried; a support whose ground is further than the base's height times `SUPPORT_STRETCH` is reported.
- **Logic**: the base's platforms warped, or, for a base with none (`bridge_stone_1`), platforms made from the deck's upward faces as a
  stair's treads are (`mesh::treads`); then the walls' fixes: welded edge to edge, merged where they can be, wound as the game's, the
  collision hull bent and closed, every platform's end on the road's surface at both rims. `wall_tech_check`'s checks run on bridges too.
- **Files**: the walls' (`wce_<tile>.pack`, the rows copied from the base, the TEd polyfill in `@wce_proc_data.pack`), keys
  `wce_<tile>_<variation>_road<id>_bridge<n>_<piece>`.
- **AI**: vanilla's `river_crossing` tiles are to be read for what stands beside a bridge (`AIH_BRIDGE_CROSSING` hints, the ground types
  under it); the bridge writes what they have.

## Save for Assembly Kit and the game

As walls do: the pieces go down into a top layer *WCE procedural roads* (Import from Assembly Kit drops it and restores the road), the
bridges' models and rows into the tile's pack and `@wce_proc_data.pack`, and the terrain goes as the composite.

## Phases

| Phase | Work | Done when |
|---|---|---|
| 0 | Check the composition rules against Terry's own output | not done: Warhammer III's packs gave no flattened tile quickly. The rules are in one place (`Kind` in `terrain_layers.rs`) |
| 1 | Layer model, composite, file, saves flatten | done |
| 2 | Working layer, brushes and undo on it, the panel | done |
| 4 | `WceRoadSpline`, Create panel, Inspector, height, blend and ground type generation | built; to check in a battle that units take its ground type |
| 6 | Crossings of roads, tile edge ends | built |
| R1 | `WceRoadGuide`, `road_pieces::read` (arms, anchors), `roads_proc/base` written on mount, guides drawn in the Prefab tool and hidden in tiles | the base pieces open in the Prefab tool and read back as their measures |
| R2 | `roads_proc/dirt` generated from the kit's decals | every piece reads, parts at most 4 m, no building on the road |
| R3 | `road_layout`: features, stretches, fitting by overrides; the pieces as references, preview, lowering; `road_type`; old `road_detail` out | a curved road with a sharp point and a free end lays dirt pieces that join at their ports |
| R4 | Junctions: T on a snapped end, X on a crossing | two crossing roads and a road ending on one |
| R5 | Crossings: water and drops found once, depth, fords | a shallow river forded, a deep one and a canyon left to bridges |
| R6 | Bridges on `wce_walls`: bases, warp along the road's line and height, width, supports, logic, packs | `wall_tech_check`'s checks pass on a curved, sloped bridge |
| R7 | In game (user): units on a ford and over a bridge, the AI's crossing | |

The terrain part's phases keep their numbers; the old Phases 5 (detail), 7 (bridges from kit pieces) and 8 (standard models, generated
bridges) are replaced by R1 to R7.

A road's outline is draped on the terrain, as every outline is, so over a gap it runs down the canyon's sides and not along the bridge.

## Risks and open questions

- **Composition is inferred** (Phase 0). The riskiest guess is the blend's "over".
- **Decals as a curve**: whether short rigid decals read as one road through a bend, and how much they overlap at a piece's joints, is
  only known once dirt is generated (R2, R3).
- **Junction pieces over graded crossings**: the second road is graded over the first; a 12 m junction piece may not cover where their
  banks meet.
- **A ford's ground type**: the road's, or the river's (vanilla's `river_ford` tiles to read). Units' speed in it is a battle test.
- **Bridges and pathing**: the game's pathing under and beside a generated bridge, and whether it needs AI hints, are battle tests.
- **Bridge bases in other games**: the table is Attila's first; Rome II's and Thrones' bridge buildings to measure.
- **The game's own map of the tile:** does a height change need more than the height map (`ai_hints`, baked pathing)? Spline Road works
  today, so probably not.
- **Memory:** a float layer at Attila tile sizes is several MB; sparse rectangles keep that down, but a tile with many roads pays for each.
- **Touching a road up by hand** is not possible: nothing goes over its layers. Its settings are the only way to change it.
