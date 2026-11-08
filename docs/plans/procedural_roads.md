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

Until 2026-11-08 (user: a T's bump spoilt the other road) the later road's bump went off over 8 m next to the other and it was graded on that
one's raised surface, not pinned to it: a branch made after the through road ended a little under it, and its flat and banks sank a hollow in
the through road; made before it, the through road's own bump went off round the branch's end. Matching a neighbouring tile's road is left
for later.

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
    base/       road_base_straight  road_base_curve  road_base_corner  road_base_t_junction  road_base_x_junction  road_base_end  road_base_ford
    dirt/       road_dirt_straight  ...      (guides and the kit's decals: generated, refined by hand)
    <type>/     road_<type>_<piece>
```

A prefab's key is its file's name and the library has one list of keys across every folder, so the type is in the key: `road_<type>_<piece>`
(`road_pieces::key`); the folder is where it is kept.

- A road type is a folder; `road_type` (in place of `road_detail`; a text field for now, default `dirt`) names one, or `none`. A type
  missing a piece takes `base`'s, which lays nothing but its guides' measures (a warning names the missing piece).
- **Generated pieces carry a mark** (`wce_generated`, a hash of their contents when written, `road_pieces::write_generated`): while a
  piece still hashes to it, nobody edited it, and a newer generation replaces it; edited, or without the mark (written before 2026-11-08's
  R3), it is the user's and stays.
- The editor writes `base/` and `dirt/` into a game's projects when the prefab library is first scanned for that game (the Tile and Prefab
  tools share it; `road_pieces::write_base`), each piece only where its file is missing: the user's edits are kept. `dirt/` is written only when the game's kit has its decals; else the log says so.
- They are ordinary `.wce` prefabs: the Prefab tool opens, edits and saves them like any other.
- **Dirt** (`wce_tile::road_dirt`, R2): the kit's road decals as its `global_dirt_road_*` prefabs use them, a decal's x along the road.
  Two strips of `path_1` ruts down the road (grass between them, as on the kit's roads), `border_1` / `border_2` on the edges with their
  dark lip facing out, rocks, a puddle and erosion as spots; the ford wetter and stonier. The curve is five 4.6 m chords of ruts and
  borders a side; corners and junctions run their arms' ruts across the middle with one more laid diagonally over it (`road_anchor=
  "fixed"`) so that the meeting is worn all over. The kit's `path_2` is a bent rut, not a patch, and is not used. Its ruts are cut down
  by at most 2.2 times (the curve's 4.6 m from the kit's 10 m).

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
drops them as it drops every `Wce*` element), drawn on the Prefab tool's stage and hidden anywhere else (the index takes the visible bit off a
guide outside the stage's group). All guides draw in one colour (the overlay's colour table has no free slot); their names say their role
(`edge left`, `port front`, ...). A guide's `guide_role` is one of:

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

Decals cannot bend: a curve is made of its parts moved onto the arc, each rigid, so a part is a chord of the road, off it by `L x L / 8R`
at its middle. **A part may be off the road by 0.4 m at most where its piece is bent the most** (0.3 at first; raised with the overlaps below) (changed 2026-11-08 from a flat 4 m, which
cut the kit's 10 m ruts to 4 m on straights too; `road_dirt::max_part_length`): a straight, bent up to 15 degrees over 8 m (R of about
30 m), takes parts up to 9.9 m; a curve, bent up to 90 degrees (R down to 7.5 m), up to 4.9 m. **Ruts and borders run 0.75 m past a piece's ports** (`road_dirt::OVERLAP`; user 2026-11-08: a decal fades at its ends, and pieces end to end left bands of grass between them, along the ruts and the edges): a straight's are 9.5 m, a curve has five 4.6 m chords a side, and junctions', corners' and ends' run past their ports too; borders are 2 m wide, over the ruts' edge. Pieces written before the generation mark (R1's, R2's) are known by their contents (`road_pieces::UNMARKED_GENERATIONS`) and replaced too when unedited. Corners, junctions and ends are not
bent (their arms turn whole) and have no limit.

### Laying a road out

As the walls' solver, on the road's centre line (`road_layout`, a new module of `wce_tile`):

1. **Features first.** A point marked `sharp` (as walls' points are: `road_sharp`, or the Polygon panel's checkbox), or one the road turns
   at by 90 degrees or more (`roads::sharp_points`), takes a `corner`. The road's line comes into such a point straight from each side
   (`roads::centre_line_sharp`: its curve is cut there, as at an end), for the terrain too. The corner's ports are where the line is its arm's
   length from the point in a straight line (`road_layout::chord`), so the next pieces meet them exactly; each arm turns on its own to its
   way, and a left turn takes the right-turning piece's arms the other way round, so its outer edge stays outside. A corner with no room
   (too near an end, a bridge or another corner) is reported and the road bends there instead. A road's end within 8 m of another
   road snaps onto it and takes a `t_junction` there, on the other road; two roads crossing take an `x_junction`, its arms turned to the
   angles they cross at. A free end takes an `end`; an end on the tile's edge nothing. A ford takes `ford` pieces; a bridge nothing (it is
   its own, below).
2. **Stretches** between features: where the line bends more than `STRAIGHT_BEND` over a straight's length (15 degrees over 8 m, a radius
   of about 30 m), `curve` pieces, bent from their 45 degrees to the bend's own (between 15 and 90 degrees each); elsewhere straights.
3. **Fit** every piece (above, `road_layout::fit`); the seed turns straights and curves end for end and, later, chooses between variants
   of a piece (`straight_2`, ...). Each part's overrides are its `position`, `orientation`, `scale` (decals), `surface_mode` (freeform)
   and `lf_offset` (its height, the ground's there, as the kit's tiles keep a freeform instance); they are applied to the prefab's
   contents (`prefab_refs::apply`) and the guides taken out, which gives the piece as a group: what is shown, and what an export lowers.

Built as said (2026-11-08) but for fords (R5). An end within 1 m of the tile's edge gets no end piece. The bridges are
still the kit's pieces until R6. The same piece repeats every 8 m (its rocks and puddle show the period, `the road on a kit tile` check):
variants are the remedy, when a type has them.

A road crossing another is still graded over it (the terrain part, as built); the junction pieces only cover the meeting.

**Junctions** (R4, 2026-11-08; `road_layout::snap_ends`, `junctions`). Every road's line is known before any is laid:

- **An end on a road.** A road's end within 8 m (`SNAP`) of another road's line, and more than 8 m from that road's own ends, is taken
  onto it, for its terrain and its pieces alike (`roads()` in the document), so the road reaches it and its signature follows the other
  road. A road's end within 1 m of another's line makes a `t_junction` on that road, its through arms along it and its branch along the
  ending road; the ending road leaves the junction its last stretch and has no end piece there (`End::Joined`). Two ends meeting end to
  end are not joined: two roads, each with its end.
- **Two roads crossing**, more than 1 m from either's ends, make an `x_junction`, laid by the first of them in the document; the other
  leaves it its stretch.
- A junction's ports are where each road's line is its arms' length from the meeting in a straight line, as a corner's are, and it is the
  size of the road that lays it. One with no room (too near an end) is reported and not made.
- **Arms to ways**: a star piece's arms are matched to the roads' ways by the order whose angles between ways are nearest the piece's,
  turning the same way (`arranged`, every order of up to 5 arms). So a T drawn with its branch on the right, laid with the branch on the
  left, takes its through arms swapped and keeps its unbroken edge opposite the branch, and a corner the same (this replaced R3's swap).
  Each arm then turns on its own to its way, so roads may meet at any angle.

### As walls do

The pieces are derived data: made again from the road's line, settings and the pieces' files (their signature), 300 ms after the last
change, kept by that signature, not saved in tile.wce. Not selectable one by one: a click picks the road. Save for Assembly Kit and the
game export lower them into the top layer *WCE procedural roads*, the references baked to groups (as every reference is for TEd); Import
from Assembly Kit drops that layer and restores the road.

## Fences and rounder corners (user, 2026-11-08)

- **Fences, optional per road** (`road_fences`: none, left, right, both, as the road runs). A road type has a `fence` piece
  (`road_<type>_fence`: a fence's line, 5 m long and 1 m wide, its guides as a straight's; dirt's is one `lowfence_3`, its x along the line,
  as the kit's path prefabs place theirs about every 5 m; the base's has nothing). Fences are not laid on the road but along a line 1 m past
  its edge (`road_layout::fence_runs`), end to end at the piece's own length (never stretched), each run centred in a stretch the road holds
  nothing on: they stop 0.5 m short of its corners, junctions, bridges, fords and end pieces (`Layout::held`). `lowfence_3` is a low
  dry-stone wall; another fence is the piece's to change.
- **The corner piece is a rounded turn** (the road's line and terrain stay as they are): dirt's ruts and edges follow a quarter circle from
  port to port about the inside of the turn (radius 6 at the centre, the outer edge 9, the inner 3), in chords, laid as road (`surface`)
  so that a wider road widens it, each chord with the arm it is nearer. It replaced the L of two arms' ruts and edges with a square patch at
  the outer corner, which was too abrupt.
- **Fences round corners and junctions** (user, 2026-11-08). Between each two arms next to each other of a corner or a junction the road
  lays, a fence runs on the arc that meets both arms' fence lines square at their ports (`road_layout::star_fences`): round the outside of
  a corner (radius 10 for a square one), straight along a junction's through side; an arc shorter than three quarters of a fence, or whose
  ends are that close (a square inside corner, 2 m about its middle), is left open: a fence across it stood diagonally into the road.
  Which sides: those of the road laying the piece, as it runs through it. Along the road the fences now cover each free stretch end to end
  (`fences_between`: the first from its start, the last to its end, those between spread, overlapping a little), so they meet the arcs.
- **Fences follow the ground**: a road piece's buildings are tilted to the ground under them (`fit_dressed`, `Dress::tilt`: the slope
  along them from end to end, 2 m each way, and across, 0.5 m), their up onto its normal, their heading kept.
- **Destroyed fences**: `road_fence_damage` (0 to 1) is the chance each fence is destroyed: a destroyed one takes a random damaged
  `destruction_level` (1 to its building's last; `lowfence_3` has one), from the road's seed, so it stays as it was. Checked: 48 of 121 at 0.4.
  The editor draws it so: a generated building's preview takes its `destruction_level`'s piece files, as the scene's buildings do (until
  2026-11-08 it drew every one intact, and the slider seemed to do nothing).
- **A fence on a bend** (user, 2026-11-08). `lowfence_3`'s pivot is at its end, the model running 5.25 m along its x from it: the fence
  piece has it at its back port, and a rigid (`fixed`) part along a stretch is laid on the chord of the stretch, from the point it starts at
  to the one it ends at, and tilted by the ground about the chord's middle. With the pivot at the middle and turned by the line there,
  each fence was 2.5 m ahead of its place, which on a bend stuck out of it outside and into the road inside.
- **Junctions' curves**: the T's branch turns into the through road either way and the X has a turn in each quarter: ruts and the inside
  edges on quarter circles about the inside corners (`road_dirt::quarter`, as the corner's), in place of two pairs of straight ruts crossing
  and L-shaped inside edges. The kit's bent rut (`path_2`) was measured for it (a capture traced from above): it turns about 25 degrees
  over its length after a straight start, too little for a junction's turn alone, so the turns are chords of `path_1`.
- **Variants** (user, 2026-11-08: the roads were monotonous, the same straight every 8 m). A road type may have variants of a piece
  (`road_pieces::VARIANTS`: `straight_2` to `straight_5`), read as the piece they vary; each straight laid is it or one of them, by the
  road's seed and its place (`pick`). The dirt ones carry what else the kit's `global_dirt_road_*` prefabs have along their roads
  (measured in the road's frame: their angles are radians): its worn patch (`global_terrain_path_04`, 7.5 x 3.5 m), a puddle, erosion
  beside the road (`erossion_03` and `_12`), rocks; and its banks of ground, `global_ground_pathborder`: a ridge of grassy ground 18.6 m
  long and about 10 m across, its crest over its origin, on one side, the other or both. The kit lays a bank at its road's edge, rolled
  10 degrees and 0.4 m down, on sloping ground, which shows it; on a generated road's levelled ground that sank it out of sight, so it lies
  2.5 m out on the verge, 0.1 m down. Being long, a variant with a bank is laid only between straights and curves, not by a junction, a
  corner, an end, a bridge or a ford. A part's own height in its piece is now kept when it is fitted (`up`).
  The banks are tilted to the ground as the fences are (user, 2026-11-08: upright, they did not follow the slope): the ground read 7 m
  along and 5 m across them each way (`PIECE_TILT`; a fence's `FENCE_TILT` is 2 and 0.5), their own roll kept on top. Rigid and 18.6 m
  long, a bank on the downhill side of a road along a hillside shows its outer edge where the slope steepens past the road's shoulder.
  A straight with a bank is picked three times as often as one without (user, 2026-11-08: more of them).
- **Sharp corners and a road over itself** (user, 2026-11-08). The outside of a corner's fences hugs the road: along each arm's fence
  line to the corner, round its middle at the fence line's distance (the road's ground is its line's, rounded there by its half width;
  the arc square to both fence lines at the ports cut into it, and at a turn sharper than a square went through the road, its fences
  crossing in the middle). A fence that would stand on a road (its own past a sharp turn, where it crosses itself, or another) is left
  out. A road crossing itself lays an X there (`junctions`), and its second pass is held at the first's height (`roads::over_itself`):
  graded apart, the two passes crossed at two heights, one cut into the other.

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

**Built** (R5, 2026-11-08; `roads::plan`, `WaterMap`, `wet_stretches`, `graded_pinned`):

- **The water** (`Tile::water_map`): the rivers' ribbons as the water is drawn (`Tile::river_ribbons`, factored out of `rebuild_water`, so
  a road meets the water where it shows: a sample's surface height and half its width), the lakes' `water_plane` instances (their model's
  bounds through their transform, at their height), and the sea at 0 m when the tile's set is `sea`, `sea_coast`, `sea_coast_landing` or
  `river_mouth` (the sea the editor draws over every tile, as TEd does, is context only). Water is water to a road where it stands at
  least 5 cm over the ground; wet stretches less than 8 m apart are one.
- **A ford** pins the road to the bed (`graded_pinned`: the grading holds the pinned samples and brings the road around to them at its
  grade), its bump off; the road's height layer leaves the terrain's heights alone there (`Strip::keep_heights`), and lays its texture and
  ground type. `ford` pieces, as long as straights, from bank to bank.
- **A bridge over water** spans the wet stretch and a sample of bank each side; its deck stands at least `road_bridge_clearance` over the
  highest water, and the road is raised to it on each side at its grade. Drops are found as before, on the road graded with the water's
  bridges and fords taken out. The bridge laid is still the kit's (R6 generates it).
- **A road is made again when the water changes**: its signature has the rivers' and the water planes' elements.
- Checked on the kit's `river/2x2_bl_rt` and `river_shallow/2x2_bl_rt` (scratch copies with their database entries): the same road is a
  bridge over the deep river and 7 ford pieces over the shallow one (`road_items` now counts the pieces by kind).

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

### Built (R6, 2026-11-08; `wce_walls::bridge`)

- **A base is turned, then bent.** The kit's bridges run along their z, the walls' cuts and warps along x: `bridge::prepare` turns a base's
  files a quarter (`QuarterTurn`, a turn, not a mirror: the walls' own warp with a yaw of -90 degrees and no cuts), then measures it: its
  length where its platforms end (`logic::joint_extent_x`, as a wall piece's joint: bridge_wood_2's meshes reach to -10.02..10.01 for its
  platform's -10..10, and two pieces' platforms missed each other by that), its width and lowest point from its intact meshes, and the
  height walked on: its platforms', else the widest upward face along its middle (`mesh::treads`), where a platform is made across 0.8 of
  its width (`bridge_stone_1` has none).
- **`BridgeWarp`**: the base's x onto its piece's stretch of the deck, its z across, widened by the road's width over the base's, its
  walked-on height onto the deck's surface (the road's height there: a deck slopes as the road does); what lies more than a metre under
  that (its posts) is stretched down to the ground under it, half a metre into it. Its yaw is the deck's way, so the meshes' normals and
  the logic's directions turn with it. Points come out relative to the piece's place, a building turned by nothing.
- **Pieces** (`piece_stretches`): the deck cut into `round(length / base length)`, each a building made from the base under a key of its
  own (`bridge::make_piece`: every level's meshes, the logic file through `logic::warp_logic`, which brings the walls' cutting, welding,
  merging, winding and hull bending, and the zones). Two pieces meet on one cross-section: their ends are the same points.
- **In the editor** the bridges of a road are made with its pieces (`Tile::bridge_items`), their files among the generated ones
  (`wce_vfs::generated`) under keys new at each making, drawn as they are; the bases are read once per tile. `road_bridge` chooses the base
  (`wood`, `stone`), `road_bridge_clearance` the deck's height over water. The old chains of kit buildings are gone.
- **In an export** a bridge is its element, `WceBridgeSpan` (its deck's points, base and width, `bridge::span_text`), which
  `export::lower_walls` makes into pieces as it makes walls: keys `<tile prefix>_r<n>_<k>`, the base's rows copied under each, files and
  rows into the tile's pack (game export, launch, mod pack) or `@wce_proc_data.pack` (Save for Assembly Kit). The exports go that way when
  the tile has walls or bridges (`Tile::has_generated_buildings`).
- **Checked**: `cargo run --release -p wce_walls --example bridge_check -- <game folder> [radius rise] [schema_att.ron]` bends both bases
  along a quarter circle rising over it and checks what `wall_tech_check` checks: the deck walkable from end to end, no platforms over one
  another, every platform corner at a joint shared by the next piece, every corner on the deck's surface, the files reading back; with the
  schema, a scene of two bridges lowered as an export lowers it (every piece with its files and its row). Both bases pass on radii of 30
  and 60 with rises of 10 and 6 (scene units). Drawn on the kit's deep river, straight and curved.
- **Fixed after a look in the editor** (user, 2026-11-08): a bridge's ends stood off the ground. Its deck was laid on a plan made another
  way than the terrain's (the ground sampled at the nearest pixel, not between pixels; no earlier road taken for ground), and the terrain
  was left alone over the whole gap, its rims too. Now one plan for both (`bilinear`, the earlier roads' strips), the terrain made up to
  the rims and left alone only inside (`road_strip`), and the deck run 2 m past each rim onto the road (`BRIDGE_ON_ROAD`): its ends lie on
  the road. The technology overlay did not draw the bridges' pieces: a road's are drawn as a wall's are (`generated_buildings`: its
  bridges' pieces' logic files at their frames), and `wall_tech_check` takes a selected road's too.
- **Not yet**: units crossing in a battle (R7); Rome II's and Thrones' bases; the AI's hints.
- **Under the deck, and beside it** (user, 2026-11-08: units did not path onto a bridge; ground showed over its end). The bridge is as
  wide as the road, and on a 64-density tile the height map's pixels are 2 m: the first one past the road's edge, up its bank, stood over
  the deck's side. From end to end of a deck the terrain is kept under it, 0.3 m at its rims to 0.15 m at its ends, and its bank starts
  2.5 m past the road's edge (`Strip::under_deck`, `road_strip`). A bridge over water also gets a no-go region on each side, from 1 m past
  the deck's edge to 10 m along the water (`road_layout::nogo_region`, `side_outline`; export only, as the crossing's hint). Vanilla's
  river crossings have none there (5 of 6 checked; they have the nogo map). The platform is the kit's own (`bridge_wood_2`: one at deck
  height, 10 x 20 m, its flags all false, as every wall's), bent along the deck.
- **Ground platforms at the ends** (user, 2026-11-08: units still red-crossed the far end, and went round the bridge's sides). The kit's
  `bridge_wood_complete` (tech_dump) is its deck (1.2 up, flags false), a 2-unit ramp at each end down to 0.2 (flags false), and past each
  ramp a platform down to the terrain, 6 units long, `flag_2` set: the editor draws it as ground. A generated bridge's first and last
  pieces now get one past each end of the deck (`bridge::with_ground_platforms`, `GROUND_PLATFORM`: 3 m): from the deck platform's end
  edge (its two corners shared, as the game's pieces join platforms) on along the road down to the ground, `flag_2` set, wound as the
  game's. The deck's own 2 m on the road stands for the kit's ramp. `bridge_check` checks each end has one, joined to the deck.
- **The wooden bridge alone** (user, 2026-11-08). `bridge_stone_1` is a humped stone bridge of its own: cut into pieces, each repeated its
  hump and its arch, its deck in steps from piece to piece and its arches pulled down to the ground unevenly. It is no longer offered;
  a road set to it gets the wooden one. Other bases wait for a standard a base is made to (its deck level along it, a span that repeats,
  what is stretched to the ground).

## The AI at a crossing, and the checks in game (R7, 2026-11-08)

**What vanilla has.** Every `river_crossing` tile of Attila's kit has one closed `AIH_BRIDGE_CROSSING` hint polygon over its bridge
(`bridge_wood_complete`): about 35 scene units across (the bridge's 20 and 7 more each side) by 115 to 120 along, from foot to foot. Its
`river_ford` tiles have one too, over the ford, not an `AIH_FORDING_POINT`: 65 to 80 across by 100 to 115 along. Their `AIH_RIVER_EDGE`
lines and `AIH_COVER_LINE`s are the river's and the map's, not the crossing's.

**What a road writes.** Each of its bridges and fords gets that hint in an export (`road_layout::crossing_hint`): closed, on the ground,
its outline following the road (`strip_outline`, a side forward and the other back every 4 m, so a curved crossing is covered as it
bends): over a bridge the road's width and 3.5 m more each side, from 2 m before its gap to 2 m past; over a ford at least 32 m across, as
vanilla's. In the layer *WCE procedural roads* with the pieces; not drawn in the editor. `tile_export_text <file>` writes the scene as an
export reads it (the pieces, the bridges' spans, the hints): on the kit's `river` tile a road has one span and one hint, on
`river_shallow` seven ford pieces and one hint.

**Checks in game (user)**, with Launch in game on a tile with a road over a shallow and a deep river (or the kit's `river` and
`river_shallow` 2x2_bl_rt in the projects):

1. A unit marched over the ford crosses on the road, at the road's speed or the ford's (the ground type under a ford is the road's: is
   that what the game wants, or the river's?).
2. A unit marched over a generated wood bridge walks on its deck from bank to bank, through its joints (no stop or detour at a piece's
   end), and on a curved bridge too.
3. The AI attacking or defending across the river goes over the bridge and the ford (the hints), not around or into the water.
4. Nothing of a bridge floats or sinks: its posts in the bed, its deck on the road at both banks, and its pieces in the pack (a mod pack
   save, or the launch's).
5. Save for Assembly Kit, then TEd: the bridge's pieces show (from `@wce_proc_data.pack`), the hints are there, and Import from Assembly
   Kit gives back the road.

What fails comes back as a change to the plan.

## Save for Assembly Kit and the game

As walls do: the pieces go down into a top layer *WCE procedural roads* (Import from Assembly Kit drops it and restores the road), the
bridges, made again from their spans there (`lower_walls`), with their models and rows into the tile's pack or `@wce_proc_data.pack`, and
the terrain goes as the composite.

## Phases

| Phase | Work | Done when |
|---|---|---|
| 0 | Check the composition rules against Terry's own output | not done: Warhammer III's packs gave no flattened tile quickly. The rules are in one place (`Kind` in `terrain_layers.rs`) |
| 1 | Layer model, composite, file, saves flatten | done |
| 2 | Working layer, brushes and undo on it, the panel | done |
| 4 | `WceRoadSpline`, Create panel, Inspector, height, blend and ground type generation | built; to check in a battle that units take its ground type |
| 6 | Crossings of roads, tile edge ends | built |
| R1 | `WceRoadGuide`, `road_pieces::read` (arms, anchors), `roads_proc/base` written on mount, guides drawn in the Prefab tool and hidden in tiles | done (2026-11-08): the base pieces open in the Prefab tool and read back as their measures (`road_pieces` tests, `a_road_pieces_guides_show_on_the_prefab_stage_only`) |
| R2 | `roads_proc/dirt` generated from the kit's decals | done (2026-11-08): every piece reads, its parts within the bend limit, no building (`road_dirt` tests); seen in the Prefab tool |
| R3 | `road_layout`: features, stretches, fitting by overrides; the pieces as references, preview, lowering; `road_type`; old `road_detail` out | done (2026-11-08): a curved road with a sharp point and free ends lays dirt pieces that meet port to port (`road_layout` tests); drawn on a kit tile |
| R4 | Junctions: T on a snapped end, X on a crossing | done (2026-11-08): two crossing roads and a road ending 4 m short of one, every port meeting another (`road_layout` tests); drawn on a kit tile |
| R5 | Crossings: water and drops found once, depth, fords | done (2026-11-08): a shallow river forded, a deep one and a canyon left to bridges (`roads` tests); both rivers on kit tiles |
| R6 | Bridges on `wce_walls`: bases, warp along the road's line and height, width, supports, logic, packs | done (2026-11-08): `bridge_check` passes on curved, sloped bridges of both bases, and lowers them as an export does |
| R7 | In game (user): units on a ford and over a bridge, the AI's crossing | crossing hints written (2026-11-08) as vanilla's; the checks in game are the user's ("The AI at a crossing") |

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
