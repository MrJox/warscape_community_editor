# Tile editor: automatic detail, second round (plan, 2026-11-20)

The ideas left from the first round's list (docs/plans/auto_detail.md), built in the order the user picked, on `feature/auto_detail_2`
(branched from `feature/auto_detail`, which is unmerged):

- **D. Erosion and fine detail**: hydraulic and thermal erosion plus fine noise over the heights, buildings and roads left flat; where
  water ran and where it dropped its load become texture rule conditions.
- **E. Decals by rules**: puddles in hollows, dirt at building bases, ruts along roads, leaf litter under trees, laid by rules.
- **F. Waterside detail**: reeds and rocks on the banks you can walk on, stones in the riverbed, driftwood on coasts.
- **G. Fields and farmland**: draw an area and get field strips with their own textures, fences round them, orchard rows and haystacks.
- **H. Paths between things**: dirt paths found over the slope between gates, buildings and roads, made as procedural roads.
- **I. District filler**: draw a district and it fills with buildings or prefabs facing the roads, keeping off steep ground.

Status (2026-11-20): D done (steps 1 to 4); E done (steps 1 to 3); F done; G done; H done; I done. The whole plan is built; its look on kit tiles and in game is the user's. Their look on kit tiles is the user's.

Common to all: a seed saved in the tile (the same result each time), a limit on what is laid, the editor's own look (styled.rs) and
the Inspector as their place (see "Where the tools live" in auto_detail.md). The user looks at each in the app when it's done.

## Decisions (mine, 2026-11-20; the user can overturn them)

| Question | Answer |
|---|---|
| Erosion: live or by a button | **Live**, like the texture rules: a derived height layer remade off the editor's thread when the heights under it or its settings change. Settings in `erosion.wce` beside the tile |
| Erosion's place in the stack | Over the user's height layers, under the procedural ones (roads, walls, prefab ground), so it reads what the user sculpted and roads still lay flat over it |
| Decals, fields: derived or saved | **Derived** (live, laid again from their rules or areas, added at export), like the biomes' trees |
| Paths, districts: derived or saved | **Made once** with an undo entry: a path is a procedural road you then edit; a district's buildings are plain buildings in a group you then move; filling again replaces that group |
| Waterside | A fifth ground coverage kind, **Shores** (the walkable banks), in the ground coverage window, as the other kinds are |

## D. Erosion and fine detail

### Design

- `wce_tile::erosion`: `Settings { droplets per km², rain steps, inertia, capacity, erode, deposit, evaporate, thermal talus angle,
  thermal passes, detail amplitude, detail scale, seed, strength }` and `erode(heights, w, h, mpp, mask, &Settings) -> Result {
  delta, flow, deposit }`:
  - hydraulic: droplets (Beyer's particle model) over a bilinear height grid with brush-weighted erosion, so channels come out
    smooth;
  - thermal: material moved down where the slope passes the talus angle;
  - detail: fractal noise scaled by slope (more on steep ground);
  - `mask` (0..1) scales everything: 0 on buildings' footprints and roads (and a fade), so their ground stays as it is;
  - `flow` = how much water ran over each sample (droplets' visits, normalised); `deposit` = what was laid there (positive part of
    the hydraulic delta), both kept for the texture rules.
- Editor: an owned height layer `erosion#<sig>`, ordered between the user's layers and the procedural ones (a new order slot); a job
  like the texture rules' reads the composite under it, writes the delta. Remade when the composite under it, the masks or the settings
  change; never per stroke (waits for the stroke's end, as the rules do). Exports wait for it.
- Texture rules: two new conditions, **Water flow** (0..1) and **Deposit** (metres); they read the erosion result kept with the layer
  (no erosion = flow worked out from the heights alone by D8 accumulation, deposit 0).
- UI: Terrain layers, Height tab: an "Erosion" row (or "+ Erosion") over the user's layers opening its settings in the Inspector:
  presets (Light, Weathered, Rugged), Simple/Advanced, Reroll; viewport outline of the flow when asked.

### Steps (done 2026-11-20)

Done as planned, but: a drop's load goes by the **slope** it runs down, not by the metres it falls a step, and its speed is capped (the
first run dug −11 888 m into a 4 m-a-sample mountain); the stack is made before the first signature (else the first remake ran twice).
The view of where water runs in the viewport is left out (the texture rules' outline of a Water flow rule shows it).

1. `wce_tile::erosion` with tests (a cone gets channels; a flat mask stays untouched; same seed = same result; thermal flattens a
   spike past the talus).
2. The owned layer, the job, `erosion.wce`, the script command `erosion <off|on|preset <name>|wait>`.
3. Texture rule conditions Water flow and Deposit, Default gets "Gravel where water ran" when the tile has the textures.
4. Inspector UI and docs.

## E. Decals by rules

### Design

- `wce_tile::decal_rules`: a **decal rule** = name, decal keys (weighted), the texture rule's conditions (height, slope, hollow, water,
  road, building, flow, deposit) plus **Under trees** (distance to tree trunks), density per 100 m², size range, spacing, alignment
  (Random, Along roads, Facing buildings), breakup. `lay(rules, fields, keep_off, seed, extent) -> Vec<Decal{key,x,z,yaw,sx,sz}>`.
- Editor: live like the biomes' items (a job on a signature, drawn locked, layer "WCE decals" at export), each decal refitted to the
  ground (`decal_refit`). Rules in `decal_rules.wce`; presets in the projects folder.
- The built-in **Default**: puddles in hollows (global_terrain_puddle_*), dirt at building bases (erosion/path decals), ruts along roads,
  leaf litter under trees; only the keys the game has.
- UI: the Create panel's decals get a "Decal rules" entry opening them in the Inspector, the rule list and fields as the texture rules'.

### Steps (done 2026-11-20)

Done as planned, but: the distance to trees is the decal rules' alone (in the texture rules it would loop: the generated forest follows the
blend map they paint); the "Decal rules" entry is a new kind of Create entry (`Make::Rules`) that only opens the Inspector; decals have no
pictures in the key picker (the thumbnails have no ground for a decal to project onto). On the Nordic city: 2757 decals (743 at building
bases, 1928 under trees, 68 puddles).

1. `decal_rules` (conditions shared with texture rules), tests.
2. Editor job, export layer, file, script command `decal_rules <off|load|window|wait>`.
3. Inspector UI (the key picker for decals), Default, docs.

## F. Waterside detail

### Design

A ground coverage kind **Shores**: the walkable ground within reach of water and the shallow water itself, in bands: in the water's
edge (reeds), the bank (rocks, bushes), the riverbed under the surface (stones), and on coasts the tide line (driftwood). Keys picked
with the key picker, presets per climate. It goes where Riverbanks and Coast don't (they take the cliffs).

### Steps (done 2026-11-20, one commit)

Done as planned; with no set chosen the shores lay built-in reeds and stones (the other kinds need a set); the window hides "On the
face" for them and offers thicker reeds. On the river tile (2x2_lt_rb): 7 stretches of bank, 179 stones, 1192 reeds.

1. The kind in `ground_coverage` (bands from the water surface and depth), tests.
2. The window's tab, presets, docs.

## G. Fields and farmland

### Design

- An area like a biome's (an exclusion polygon, so the game's trees keep off) carrying `wce_fields` settings: strip width and angle
  (or along the longest edge), each strip's crop (a texture group and variation, from the tile's own), a share of orchard strips
  (tree rows), fence keys (a building spline round the area or per strip), haystacks/props per hectare, a seed.
- Live: the strips' textures as an owned blend layer (with the texture rules'), trees, fences and props as derived items added at
  export.
- Create panel: "Fields" under Vegetation (draw an area), the Inspector as for biomes.

### Steps (done 2026-11-20)

Done as planned, but: the crops are the kit's own field decals (`global_fields_1..6`, 50 × 25 m in its farm tiles; its `field_1..4` are
5 m squares) laid end to end along a strip rather than textures (the tile has 4 texture groups, none a crop); the ground under the whole
area is one texture rule bound to it; the farmland is made on the editor's thread (no job: it is a few hundred items); the settings are the
area's own attribute (no presets yet); the Inspector shows them under the area's properties. Found on the way: adding anything to an empty
kit tile (`<SCENE_GRAPH .../>`, terrain_plains/1x1/ab) failed; fixed. On that tile an area of about a hectare: 18 to 22 crop decals, about
35 olive trees, 66 fence pieces, 4 haystacks.

1. `wce_tile::fields`: strips from a polygon, the crops, the rows, the fence lines, scatter; tests.
2. Editor: areas, job, blend layer, items, export.
3. Create entry, Inspector UI, docs.

## H. Paths between things

### Design

- `wce_tile::paths`: A* over the height grid, cost by slope (a soft limit and a hard one), water and buildings' footprints barred,
  existing roads cheap; the cell path simplified to a few control points.
- Tool: pick two ends (a gate, a building, a point or a road) and get a `WceRoadSpline` with `road_type` "dirt_track" (or "none" where
  the game has no tracks) and its width; "Connect to the roads" for the selected buildings does each one to the nearest road.
- One undo entry; the path is then an ordinary procedural road.

### Steps (done 2026-11-20)

Done as planned, but: water is not barred, it costs (a ford 6 times a step, a metre deep 25 times: the roads make fords and bridges), so
a path crosses a river where it is narrowest (the river tile's is deep all along: barring it, no path crossed it); the simplify tolerance
is 3 m (1.5 m left a zigzag of 40 points along the 8-way cells). On the river tile: a 386 m path of 16 points across the river; a house
added and connected: a dirt track of the game's pieces.

1. `paths` with tests (around a hill, round a building, not across water, onto a road).
2. The tool (Create panel "Path" under roads, two clicks), Connect to the roads, script command, docs.

## I. District filler

### Design

- Draw a district (a polygon); settings: building patterns (key patterns as the clutter rules, or prefabs), spacing, setback from the
  road, the most slope, a seed.
- Fill: lots along the roads' edges inside the district, each building turned to face its road, its real footprint (logic or model
  bounds) kept off roads, other buildings and steep ground; the rest filled inward by size. Made into a group `District <n>` with one
  undo entry; filling again replaces it.

### Steps (done 2026-11-20)

Done as planned, but prefabs are left out (buildings only, picked as keys); a district is drawn and filled at once, and its group is
found again by its name. Found on the way: the kits' tables name buildings the game has no model for (the first default took barbarian
houses that showed as boxes; the default takes only keys with model pieces), the building splines' fallback size (5 × 0.5 m, a fence's)
made hundreds of slivers (a district guesses 8 × 8 m and says so), and a size not known when first asked was cached for good (looked for in
the scene again now). On the empty plains tile: a path across, a district round it: 51 Nordic houses, two rows facing the path, the inside
filled.

1. `wce_tile::districts`: lots from a polygon and road lines, packing by footprints, tests.
2. Editor: the fill as an edit, the Create entry, the Inspector, script command, docs.
