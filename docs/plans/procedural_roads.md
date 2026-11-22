# Tile editor: terrain layers and procedural roads (2026-11-07)

Draw a spline and a road forms over it: the ground bumped and its sides levelled, a texture and a ground type painted, detail laid along it,
bridges over the gaps. All of it lives in layers of its own, so it can be changed or removed without touching the base terrain. The layers
are a feature in their own right, modelled on the newer Terry's: height, blend and ground type layers, which come first. Terry only shows how
the feature works: its projects are not imported (user, 2026-11-07).

Status (2026-11-22): built on master. The terrain part (layers, height, texture, ground type: Phases 1, 2, 4 and 6, 2026-11-07); the
redesigned detail and bridges (R1 to R7, 2026-11-08): road pieces from `roads_proc` prefabs fitted by overrides, crossings of water and
drops as fords or bridges, bridges generated on the walls' code, the AI's crossing hints; variety (V1 to V14, 2026-11-15 .. 16); road
points snap to the tile's road links (2026-11-21, the snap chip's *snap roads to road links*). The first detail and bridges (the kit's path
prefabs strung along the road, chains of the kit's bridge buildings) are gone. Left: the checks in game, pinned heights (Open).

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

**Off the editor's thread** (user, 2026-11-08; `tile_document/jobs.rs`). The layers are made on a thread of their own (`road_terrain`), in
copies of the stacks (their samples are shared blocks: cheap), and take the stacks' place when they are done, the maps composed again only
over where the roads' layers were and are (the whole of each map before: `Layer::extent`); written to meanwhile (a brush, an undo, a
prefab's ground: `Stack::unchanged_since`), the result is let go and the roads are made again. The instances (pieces, hints, bridges) are
made the same way (`road_items`) once the terrain is, their bridges' files put among the generated ones when they are done. An export waits
for both (`finish_road_jobs`, from `text_for_export`); scripts with `tile_wait_jobs`. A road with a bridge on the kit's river tile: its worst
frame 176 ms before, 29 ms after (the work, 145 ms, on the threads).

**Grass** is made again with the road: generated grass takes the new blend map and heights, and packed or generated, no patch stands on a road (`Tile::road_at`, the roads' ground type layers), as a tile's packed list was made before it. So does a stack change, as a layer shown or hidden moves the ground under it. With generated grass that costs about 0.6 s on a large tile each time the roads are made. **Trees** too (user 2026-11-08: the grass followed the roads, the trees and bushes not): the tree list is made again with the roads (a generated one from the new blend map and heights), and no list tree stands on a road, packed or generated (`add_list_trees`). The VEGETATION_INSTANCEs placed by hand stay where they are.

**The height at a point** comes from the road's samples near it, by distance, further than 12 past the edge: taken from the nearest alone it jumped where
a point stopped being nearer one stretch than another, in steps across the banks (`the_terrain_a_road_leaves_has_no_steps`).

**Crossings.** A road reads what lies below its layers, so a later road takes an earlier one for ground and joins it at its height: where it
runs on the other it is pinned to that one's surface (bump and all), is graded on the ground under that bump, and keeps its own bump up to it.
The bump also comes in over the road's first and last 8 units, so a road ending at a tile's edge ends on the ground; not from an end on another
road. The roads are made in their elements' order, except that a road ending on another comes after it (`made_order`), its layers ordered to
match: a T's through road first, the branch coming to it.

(User, 2026-11-08: before, a T's later road was graded on the other's raised surface and sank a hollow in it.) A road point drawn or
dragged within 12 m of a tile edge a road leaves by (the entry's road links) goes onto it, so the road meets the next tile's (2026-11-21).

**Existing objects** on the terrain follow as they do under a brush (landscape instances). Trees and props on the road are not removed in the
first version, but the road reports them; an optional "clear vegetation" comes later.

## Road pieces (redesign, 2026-11-08)

The first detail (`road_detail`: the kit's `global_dirt_road_*` and `global_path_*` prefabs strung end to end) read as a repeated pattern and
knew no junction, end or bend; it was replaced.

### The folder

`<projects>/assembly_kit/raw_data/art/prefabs/roads_proc/<type>/road_<type>_<piece>` (`road_pieces::key`: a prefab's key is its file's
name, so the type is in the key). `base/` holds placeholders (guides only); `dirt/` and `dirt_track/` (one lane, 3 m) are generated from
the kit's decals (`wce_tile::road_dirt`); the dirt types are `tables/<game>/road_types.toml` (name, width, lanes), the mods' and the
user's `road_types_*.toml` over it. The editor writes them into a game's projects when the prefab library is first scanned, each piece
only where its file is missing, and only where the game's packs have the decals (Rome 2 has no dirt type). **Generated pieces carry a
mark** (`wce_generated`, a hash of their contents): while a piece still hashes to it a newer generation replaces it; edited, it is the
user's and stays (`road_pieces::UNMARKED_GENERATIONS` knows the pieces written before the mark). They are ordinary `.wce` prefabs. A type
missing a piece takes `base`'s, with a warning. The Inspector's `road_type` and `road_fence_type` are selectors (`Tile::game_choices`):
the tables' types, then the library's other `roads_proc` types; choosing a type puts the road at the table's width.

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
| `fence` | 5 long, 1 wide | as a straight | along the road, 1 m past its edge |

The placeholders are **guides**: `WceRoadGuide` elements (TEd never sees them), drawn on the Prefab tool's stage only. A guide's
`guide_role`: `centre` (port to port; a star piece one per arm), `edge` (3 m out), `verge` (3 m past the edge: where buildings stand),
`port` (across the road at a join). The reader finds a piece's arms from its ports and centre guides, so a piece drawn to other measures
still reads; a piece without them is not used.

### How a part is anchored

A part (a decal, building or simple mesh of the piece) is read once (`road_pieces::read`): its arm (nearest centre guide), its `along`
(fraction of the arm), its anchor: `surface` between the edges further than 1 m from them, `edge` within 1 m, `verge` past an edge, a
building never `surface`; `road_anchor="surface|edge|verge|fixed"` overrides it (`fixed` stays where it is in the piece's frame). Since V1
the piece's top-level `road_role` groups decide instead (below).

### Fitting a piece to the road

Every piece laid is a **prefab reference** (`wce_prefab="roads_proc/<type>/<piece>"`) with **overrides** of each part's `position`,
`orientation`, `scale`, `surface_mode` (freeform) and `lf_offset`; nothing added or taken out. `along` goes to that fraction of the
stretch the piece covers (`round(stretch / piece length)` pieces a stretch); `across` is scaled by the road's width over 6 for a `surface`
part, kept from the edge or the verge for the others; the part turns with the curve there; decals are projected, buildings stand on the
ground, tilted to it. Decals cannot bend: a part is a chord of the road, **at most 0.4 m off it where its piece is bent the most**
(`road_dirt::max_part_length`); **ruts and borders run 0.75 m past a piece's ports** (`road_dirt::OVERLAP`: decals fade at their ends).

### Laying a road out

On the road's centre line (`wce_tile::road_layout`), every road's line known before any is laid:

1. **Features.** A `sharp` point (`road_sharp`) or a turn of 90 degrees or more takes a `corner` (the line comes into it straight from
   each side); a road's end within 8 m (`SNAP`) of another road's line is taken onto it (`snap_ends`) and within 1 m makes a
   `t_junction`; two roads crossing make an `x_junction`, laid by the first in the document; a road over itself makes an X and its second
   pass is held at the first's height (`roads::over_itself`); a free end takes an `end`, an end on the tile's edge nothing; fords take
   `ford` pieces, bridges are their own. A feature with no room is reported and the road bends there. A star piece's arms are matched to
   the ways by the order of nearest angles, turning the same way; each arm then turns on its own.
2. **Stretches** between features: `curve` pieces where the line bends more than `STRAIGHT_BEND` (15 degrees over 8 m), else straights.
3. **Fit** every piece (above); the seed picks variants and turns. The overrides are applied to the prefab's contents (`prefab_refs::apply`)
   and the guides taken out: that group is what is shown and what an export lowers.

The pieces are derived data, as a wall's: made again 300 ms after the last change, kept by their signature, not saved; a click picks the
road.

## Fences and rounder corners (user, 2026-11-08)

- **Fences** per road (`road_fences`: none, left, right, both), on a line 1 m past the road's edge (`road_layout::fence_runs`), at the
  fence piece's own length, covering each free stretch end to end and stopping 0.5 m short of corners, junctions, bridges, fords and end
  pieces; round the outside of corners and along a junction's through side on arcs (`star_fences`); left out where they would stand on a
  road. A fence is laid on the chord of its stretch (`lowfence_3`'s pivot is at its end) and stands upright, pitched along its length only
  (2026-11-16, as the placement's *Follow slope*). `road_fence_damage` is the chance each one is destroyed (a random damaged
  `destruction_level` by the seed; the editor draws it).
- **Rounded corners and junction turns**: ruts and edges on quarter circles about the inside corners, in chords of `path_1` (the kit's
  bent `path_2` turns only about 25 degrees).
- **Banked straights**: dirt's straight variants carry the kit's road dressing (worn patch, puddle, erosion, rocks) and its
  `global_ground_pathborder` banks, laid 2.5 m out on the verge and tilted to the ground (7 m along, 5 m across); only between straights
  and curves.

## Crossings (redesign, 2026-11-08)

What a road crosses is found once, on what lies below its layers, and both the terrain and the pieces read it (`roads::plan`,
`wet_stretches`, `graded_pinned`). **Water** (`Tile::water_map`): the rivers' ribbons as drawn, the lakes' `water_plane` instances, the
sea at 0 m on the sea tile sets; water is water from 5 cm deep; wet stretches less than 8 m apart are one. **Drops**: the graded road at
least 4 m over the ground for at least 8 m.

- **Under 0.75 m deep: a ford.** The road is pinned to the bed (no bump), leaves the terrain's heights alone there, lays its texture and
  ground type and `ford` pieces from bank to bank.
- **0.75 m or more, or a drop: a bridge**, its deck at least `road_bridge_clearance` (default 2 m) over the highest water; the road climbs
  to it at its grade. `road_bridges` off: every crossing is a ford.
- A road is made again when the water changes (its signature has the rivers' and water planes' elements).

## Bridges (redesign, 2026-11-08)

Generated on the walls' code (`wce_walls::bridge`), replacing chains of the kit's bridge buildings:

- **Bases**: `tables/<game>/bridges.toml` and the mods' and the user's `bridges_*.toml` over it (since 2026-11-16): a deck piece and its
  end piece. Only the wooden bridge (`bridge_wood_2` with `bridge_wood_2_end`) is offered: `bridge_stone_1` is a humped bridge of its own,
  which cut into pieces repeated its hump (user, 2026-11-08); other bases wait for a standard a base is made to. `road_bridge` picks one.
- **A base is turned, then bent**: `bridge::prepare` turns the kit's z-running bridge a quarter and measures it (its length where its
  platforms end, its width, its walked-on height). `BridgeWarp` lays it along the deck (the road's line and height: a deck slopes, a
  wall's walkway does not), widened to the road's width, its posts stretched down to the ground. The deck is cut into `round(length /
  base length)` pieces (`bridge::make_piece`, the walls' logic fixes through `logic::warp_logic`).
- **The ends, as the kit's split bridge** (user, 2026-11-08, after in-game tests): an end piece past each end of the deck, nothing added;
  the deck 1 unit over the road (`DECK_LIFT`), the road held level with each rim for `LEVEL_PAST_RIM` and the bank held back under the
  ends (`Strip::under_deck`). **Why units did not cross** (docs/re/platforms_and_bridges.md): the game links a platform edge to the ground
  only where a ground edge has the same two corners, the ground's edges along a building being its collision outline's, and links one edge
  per platform and ground region; so an end piece's outlines and `orange_thingies` are made from its platform's corners
  (`footprint_from_platform`) and the pieces' platforms drop in-line corners (`without_inline_corners`): one full-width foot edge and
  one full-width joint at each end.
- **A bridge over water** gets a no-go region each side (`road_layout::nogo_region`, export only).
- **In the editor** a road's bridges are made with its pieces (`Tile::bridge_items`), drawn, and shown by the technology overlay and
  `wall_tech_check`. **In an export** a bridge is a `WceBridgeSpan` (`bridge::span_text`), which `export::lower_walls` makes into pieces
  as it makes walls (keys `<tile prefix>_r<n>_<k>`), files and rows into the tile's pack or `@wce_proc_data.pack`.
- **Check**: `cargo run --release -p wce_walls --example bridge_check -- <game folder> [radius rise] [schema_att.ron]` bends the bases
  along a rising quarter circle and checks the deck walkable end to end, the joints, the end pieces' feet and joins, the files reading
  back, and with the schema a scene lowered as an export lowers it.
- `ROAD_MAKING` (now 10) is in a road's signature: bump it when roads are made differently, so saved tiles make their roads again (the
  launch after a bridge fix once compiled stale heights; the maps a road changes are marked to be saved, and a launch and Save to pack
  finish the road jobs first).

## The AI at a crossing, and the checks in game (R7, 2026-11-08)

Every `river_crossing` tile of Attila's kit has one closed `AIH_BRIDGE_CROSSING` hint polygon over its bridge (about 35 scene units across
by 115 to 120 along, foot to foot); its `river_ford` tiles have one over the ford (65 to 80 by 100 to 115), not an `AIH_FORDING_POINT`. A
road writes that hint over each of its bridges and fords in an export (`road_layout::crossing_hint`, its outline following the road; over
a ford at least 32 m across), in the layer *WCE procedural roads*; not drawn in the editor. `tile_export_text <file>` writes the scene as
an export reads it.

**Checks in game (user)**, with Launch in game on a tile with a road over a shallow and a deep river (or the kit's `river` and
`river_shallow` 2x2_bl_rt in the projects):

1. A unit marched over the ford crosses on the road, at the road's speed or the ford's (the ground type under a ford is the road's: is
   that what the game wants, or the river's?).
2. A unit marched over a generated wood bridge walks on its deck from bank to bank, through its joints, and on a curved bridge too.
3. The AI attacking or defending across the river goes over the bridge and the ford (the hints), not around or into the water.
4. Nothing of a bridge floats or sinks: its posts in the bed, its deck on the road at both banks, and its pieces in the pack.
5. Save for Assembly Kit, then TEd: the bridge's pieces show (from `@wce_proc_data.pack`), the hints are there, and File > Revert >
   *To the Assembly Kit's version* gives back the road.

Units crossed a generated bridge in game after the end fixes (2026-11-08, by one half of each end until the in-line corners went).

## Variety (user, 2026-11-15)

The roads were too static and alike. Decided (grilled 2026-11-15) and built (2026-11-15 .. 16):

- **V1 Role layers**: a piece's top-level groups carry a `road_role` (`surface`, `edge`, `verge`, `fence`, `detail`, `fixed`, and
  `joiner` since V13; the Prefab tool's Inspector row; `road_pieces::PartRole`); a part outside every role group is anchored by its place.
- **V2 Variants of every piece, weighted**: `<piece>_2` to `_9` (`MAX_VARIANT`), `road_weight` on a piece's group (default 1), picked by
  weight and the road's seed; dirt's banked straights weigh 3. Bridges have none (one base).
- **V3 The detail layer**: `detail` parts shown by `road_detail_density` (0 to 1, default 0.5), stable by the seed, the piece and the part
  (`detail_kept`); one left out is not drawn nor exported.
- **V4 Per-point fence and damage**: a point's `road_fence` (unset, `none`, or a fence piece) and `road_fence_damage` cover the fences
  nearest it; the Polygon panel's rows, `road_point_fence`.
- **V5 Irregularity**: `road_irregularity` 0 to 1 (new roads 0.3): the line wanders off the spline (at most 1.5 m, `roads::wandered`),
  the width wobbles up to 25 %, the texture's edge is ragged, decals are jittered; buildings and fences are not.
- **V6 Road types blend at junctions**: each type lays its own junction piece with its arms' parts, the middle shared by the seed; the
  later road's texture cross-fades over the earlier one's.
- **V7 Decals across bridges**: the road's straights' decals laid along the deck in boxes `DECK_DECAL_BOX` (0.7 scene units: under 0.75,
  never refitted by the game, docs/re/renderer.md) tilted with the deck. Seen in game (2026-11-16); the editor refits decal boxes and
  draws decals on buildings as the game does since 2026-11-16.
- **V8 Collapses**: every generated wall and bridge piece carries its base's collapse animations, warped as its meshes are
  (`mesh::warp_collapse`; `wce_formats::anim::RawAnim` round trips the game's `.anim` files; example `collapse_check`); a wall's or
  road's *Destruction (preview)* row plays them in the editor.
- **V9 Detail turns**, dirt's pieces with about twice the detail (near the kit's `global_dirt_road_*` and `global_path_*`).
- **V10 Per-point width**: a point's `road_width` ramps to the next (`roads::sample_widths`); terrain, pieces, hints, decks and fences
  follow; `road_point_width`. An end piece's back border is not scaled across.
- **V11 A one-lane type**: `dirt_track`.
- **V12 Fence types**: 13 named fences of the kit's about 5 m fences per dirt type; `road_fence_type`, a point's own winning.
- **V13 Fence joiners and the fences file**: `tables/<game>/road_fences.toml` (and `road_fences_*.toml` over it; `wce_tile::road_fences`)
  holds each fence's building, its axis and its joiners; joiners are laid on the joints between fences as a building spline's
  (`building_spline::joiner`); `road_fence_joiners`, `road_fence_joiner_ends`.
- **V14 Selectors**: road types and fence types from the tables.

## Save for Assembly Kit and the game

As walls do: the pieces (the references baked to groups) and the crossing hints go down into a top layer *WCE procedural roads* (File >
Revert > *To the Assembly Kit's version* drops it and restores the road), the bridges, made again from their spans there (`lower_walls`),
with their models and rows into the tile's pack or `@wce_proc_data.pack`, and the terrain goes as the composite.

## Open

- [ ] **Phase 0**: check the composition rules against Terry's own output (Warhammer III's packs gave no flattened tile quickly; the
  rules are in one place, `Kind` in `terrain_layers.rs`). The riskiest guess is the blend's "over".
- [ ] **In game (user)**: the R7 checks above; that units take a road's ground type; a ford's ground type (the road's, or the river's:
  vanilla's `river_ford` tiles to read).
- [ ] Pinned heights; presets.
- [ ] Bridge bases: only the wooden one; Rome II's and Thrones' bridge buildings to measure; a standard for a base (a level deck, a span
  that repeats, what is stretched to the ground).
- [ ] Junction pieces over graded crossings: a 12 m junction piece may not cover where two roads' banks meet.
- [ ] Trees and props on a road are not removed (no "clear vegetation" yet); list trees and grass are kept off roads.
- [ ] A road's outline is draped on the terrain, so over a gap it runs down the canyon's sides, not along the bridge.
- [ ] **Touching a road up by hand** is not possible: nothing goes over its layers. Its settings are the only way to change it.
- [ ] Memory: a float layer at Attila tile sizes is several MB; sparse rectangles keep that down, but a tile with many roads pays for
  each.
