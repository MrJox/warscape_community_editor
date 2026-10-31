# Experimental: automatic ground coverage

A Tile panel button that finds the tile's steep height differentials (slopes, cliffs) and decorates them: rocks and vegetation along the foot of
each one, rocks only up its face to just under the top edge. Everything goes into the layer `(AUTOGEN) Ground Coverage`, with vegetation
exclusion polygons around the vegetation it adds (BOB drops manual trees that are outside every exclusion polygon, docs/re/vegetation.md).

Status (2026-10-31): the first version (steps 1-4 below) is done and reviewed on `feature/auto_ground_coverage`. It is being redesigned
into a contextual terrain coverage tool: see [Redesign](#redesign-2026-10-31), which comes first; the sections after it describe the first
version (what the redesign builds on).

Status (2026-10-26): steps 1-4 done on `feature/auto_ground_coverage`; step 5 (trying it on kit tiles, tuning the defaults) is the user's.
Building scale (`wce_scale`) was added for the rocks and is **optional** (`scale the rocks`, off by default): TEd has no building scale and
whether Attila honours the building list's scale is not known. Script command for checks: `ground_coverage <preview|generate> <rocks|-> <plants|-> [seed]`.

Seen on mediterranean_mountains/custom_pillar_hercules_a (defaults, seed 7): 301 slopes, 97 covered, 18,685 rocks, 5,339 plants, 635
polygons, about 1 s in release. A whole mountainside is one differential (283,000 m², 15,900 rocks on its face): the face density and a cap
per differential may need tuning.

## Redesign (2026-10-31)

The first version was inspired by JW Evolution 3's one-button cliff decoration. The redesign makes it a contextual terrain coverage tool:
coverage of different kinds of terrain, by kind-specific detection rules, either over the whole tile or with a brush.

### Decisions (user, 2026-10-31)

| Question | Answer |
|---|---|
| Scope | **Fill the whole tile** (as now) **or brush** |
| Brush | **Live**, as the vegetation brush: each dab finds the kind's terrain under the circle and puts rocks and plants there at once; Ctrl + drag takes coverage away. One undo entry per stroke |
| Kinds | **Inland cliffs** (as now, plus: only where the terrain is over the sea), **coastal** (non-walkable coast), **riverbank** (non-walkable river banks) |
| Non-walkable (coast, banks) | **The game's no-go field**: BOB's automatic no-go areas (steeper than 30°, less the go outlines), what the auto no-go view shows |
| Presets | **Named presets per kind**: the kind fixes the detection rule; a preset is a saved file of parameters + rock set + plant set ("Mediterranean coast"). Built-in defaults for each kind |
| Storage | **One layer, a group per kind**: `(AUTOGEN) Ground Coverage` > `Cliffs` / `Coast` / `Riverbanks`. Filling a kind replaces only that kind's filled items; brushed items are kept in their own group |

### What each kind finds

All three work on the same 1 m grid; what changes is which cells are the feature and where its bands are. The context they need (stage B's
final heights, its water map, the no-go field) is worked out once per tile version, as the water depth view does (`water_view::stage_b_of`).

- **Inland cliffs**: the current detection (slope above `slope_deg`, closing, regions, foot and top walks, rise and area filters). New rule:
  a cell whose foot is not above the sea (a tile that meets the sea, `foot height <= sea_margin`) or is in a river is not inland: it is
  left to the coast or the banks. A region whose cells are all left out is dropped (state `ByWater`, shown red in the preview as the
  others left alone).
- **Coastal**: the no-go cells within `water_reach` of the sea. **A tile meets the sea when its entry links to the `sea` tile set**
  (`TILE_LINKS`, `TLT_EQUALS`), as BOB's stage B knows it; a tile's heights alone don't say it (kit river tiles lie all under 0 m). The
  sea is then the ground under 0 m reached from the tile's edge (a pond is not). Regions as for cliffs. Bands by height over the sea
  instead of a walked foot: a **waterline band** from `water_below` under the sea (rocks only, half sunk) up to `foot_up` over it, plants
  on its dry part; a **face band** above it up to `top_margin` under the top, sparser rocks.
- **Riverbank**: the no-go cells within `water_reach` of river water (stage B's water map: a cell whose water surface is over its
  ground), not under it. Bands by height over the local water surface (the nearest water cell's): a **water's edge band** (rocks, and
  plants on its dry part: reeds, bushes) and a **bank band** above it (rocks). A river's bed deeper than `water_below` gets nothing.

Done in step 2: the plan's separate lip band (plants along a coast's top) was left out; the coast's plants go on the dry part of its
waterline band, as the banks'. Stage B's heights, and so its water depth, are in scene units (2 per metre): the depth is divided by the
tile's `unit_scale`.

Coast and banks use the game's no-go field (not the preset's angle), so the coverage marks exactly the ground units can't walk. Buildings
are gone around as now (the same categories, `building_clearance`). Exclusion polygons around the plants' clusters, as now, for every kind.

### Presets

- A file per preset under the projects' `raw_data/art/ground_coverage_presets/<name>.wce` (beside `vegetation_sets`), XML as the sets:
  `<GROUND_COVERAGE_PRESET kind="cliffs|coast|riverbank" rocks="<set>" plants="<set>" params="<Params::to_text>"/>`.
- Built-in defaults per kind (not files; "Save as" makes a file of one). The parameters a kind does not use are not shown for it.
- The window: kind tabs (Cliffs / Coast / Riverbanks), a preset combo per kind with Save, Save as, Delete, then the parameters.
- The last preset of each kind is remembered in the Tile settings (`GroundCoverage` gets one per kind).

### Storage

```
(AUTOGEN) Ground Coverage        LayerInstance
  Cliffs                         group: wce_autogen_preset, _params, _seed, _rocks, _plants of its last fill
    Slope 12                     a filled region: rocks, plants, exclusion polygons
    Brushed                      group
      Stroke 3                   a stroke's rocks, plants and the exclusion polygons around its plants
  Coast
    Shore 4 ...
  Riverbanks
    Bank 2 ...
```

- **Fill** a kind: replaces that kind's region groups, keeps its `Brushed` group; the filled items keep their spacing from the brushed ones
  (a fill after brushing doesn't pile onto the strokes). One undo entry, the question when it would replace items (as now).
- **Old layers** (the first version: `Slope N` groups and the `wce_autogen_*` attributes directly under the layer) read as Cliffs: the first
  fill or stroke moves them into a `Cliffs` group.

### Brush

- A mode of the window: **Fill tile** / **Brush**. Brush takes radius, density (a factor on the preset's densities) and the kind's preset.
- Stroke start: the context and the kind's classification of the whole tile (regions and bands) are taken from the cache, worked out when
  the tile changed (about 1 s on a big tile; a status note while it runs). Dabs only scatter: they throw rocks and plants into the band
  cells under the circle, at the preset's densities, spaced from what is there (the tile's coverage and the stroke's own), clear of the
  buildings. The stroke is previewed as the vegetation brush's; at its end it goes into `Brushed` > `Stroke N` with the exclusion polygons
  of its plants' clusters, one undo entry.
- **Ctrl + drag** takes away the coverage under the circle (of the kind shown, filled or brushed); a group whose plants changed gets its
  exclusion polygons again from what is left; a group left empty goes.
- The overlay while brushing: the kind's bands near the cursor (green foot / waterline, orange face), so it shows where a dab would land.

### Steps (a commit per step, on `feature/auto_ground_coverage`)

1. **Context** (done, e8c3730). `terrain_outlines::no_go_field` (the no-go field less the go outlines, before the blur and the tracing: the field the
   outlines come from) and a `CoverageContext` on the tool (heights, water surface, no-go, sea), resampled to the 1 m grid, cached per
   document version; `water_view::stage_b_of` shared.
2. **Kinds in the algorithm** (done). `wce_tile::ground_coverage` gets `Kind` and the per-kind detection and bands above. The cliffs' sea
   and river rule. The window has a kind combo and the script command an optional kind (`... [seed|-] [cliffs|coast|riverbank]`) until
   step 4. Left for step 5: splitting `generate` into classify (regions and bands, cached) and populate (in a set of cells: the tile or a
   circle). Tests on synthetic
   fields: a cliff at the sea goes to the coast; a cliff over the sea stays a cliff; a steep bank by a river is a bank, a walkable one
   isn't; a pond under 0 m is not the sea.
3. **Storage** (done). Kind groups (`Cliffs`, `Coast`, `Riverbanks`; their regions `Slope N`, `Shore N`, `Bank N`), a run replaces its
   kind's group but `Brushed`, the settings on the kind's group. Old layers: their `Slope N` groups are moved into `Cliffs` by the next
   run of any kind (what was put in the layer by hand stays); their settings stay on the layer (an attribute can't be taken away), read
   for the cliffs until `Cliffs` has its own. A run that finds nothing still makes its group (it keeps the settings). The fill's spacing
   from the brushed items comes with the brush (step 5).
4. **Presets** (done). `wce_tile::ground_coverage_presets` (file, scan, a built-in `Default` per kind); the window's kind tabs, preset
   combo (shows "(changed)" when the values are no longer the preset's), Save, Save as, Delete; the kind's group keeps the preset's name
   (`wce_autogen_preset`); the Tile settings keep the last preset per kind (the old last parameters and sets are dropped). Script:
   `ground_coverage <preview|generate> <cliffs|coast|riverbank> <preset|-> [seed]` (`-`: the tile's last run, else the kind's last
   preset).
5. **Brush** (done). The window's Fill the tile / Brush mode (radius, a density factor). `ground_coverage::classify` (regions and bands
   over the whole tile) + `scatter` (a dab: the band cells under the circle filled up to the preset's densities, counting what is
   there, so dabbing over a covered place adds nothing); `generate` = classify + populate, same results as before. The base a dab
   needs is worked out at a stroke's start when the tile or the values changed; the coverage's own edits (strokes, fills) don't make it
   or the water and no-go field stale. While a stroke lasts its rocks and plants show as crosses (the instances come at its end). Ctrl +
   drag takes the kind's coverage (filled or brushed) away and redoes the polygons of the groups it touched; empty groups go. A fill
   keeps off the kind's brushed items (`Input::keep_off`). Script: `ground_coverage_brush <paint|erase> <radius> <x> <z> [<x> <z>]...`.
   Seen on sea_coast/6x4_bb_45: a 10 m stroke 21 rocks, 14 plants, 2 polygons; the same stroke again 0 rocks, 1 plant.
6. **Check in the app** on kit tiles with a coast and a river (the user), then tune each kind's defaults.

### Open (to settle while building)

- Kit tiles have little of the new kinds: their coasts' no-go ground is under 800 m² a tile, and river/2x2_bl_rt's 9 banks are all under
  the 1.5 m rise (its 3 cliffs are the banks'). The defaults are tuned in step 6.
- sea_coast/6x4_bb_45 opens without its entry (extent 256 m instead of 768 x 512; stage B's heights 0 .. 0.83): a fault of the
  project's reading, not of the coverage; its coast is not found (no sea link known).
- The banks' "local water surface" for a river whose surface falls along the tile (the nearest water cell's height is the guess).
- Whether Ctrl + drag erases every kind or only the window's current one (the plan says the current one).

## Decisions (user, 2026-10-26)

| Question | Answer |
|---|---|
| Rock coverage | Rock `BUILDING_INSTANCE`s (the kit's own way: `gaul_rock_01/02`, `rocks_small_01`, `attila_rock_small_*`, ...) picked from a weighted **rock set**; no terrain texture painting |
| "No collision" | ~~Skip a differential when a building footprint overlaps it~~. Changed 2026-10-26: the coverage **goes around** the buildings (`building_clearance`, 3 m), and only real buildings count: categories generic, fort_wall, fort_tower, unbreachable_fort_wall, gate, bridge (packs' `battlefield_buildings` tables, mods included, else the kit's). Rocks, props (`incidental`), ground pieces (`ground_platform`) and keys of no known category don't. Before this, a TDD settlement's path borders and dummies dropped 274 of 340 slopes. |
| Detection | Slope above an angle (default ~35°) -> connected regions; keep a region only when its rise is above a minimum (default ~3 m) |
| Re-run | **Replace** the layer's contents, one undo entry, a warning that manual edits in the layer are lost |
| Vegetation | A chosen **vegetation set** (existing `.wce` sets); the dialog remembers the last one |
| Bands | **Foot band** (some metres up the slope + some out onto the flat: rocks + vegetation) and **face band** (from there up to a margin under the top: rocks only, sparser) |
| Exclusion | One `VegetationExclusionPolygonInstance` per **cluster** of vegetation (convex hull + margin) |
| UI / scope | Tile panel button -> dialog with parameters, seed, Preview, Generate; **whole tile** |
| Rock storage | **Generalise the sets**: same `.wce` format with a kind (vegetation / building), same folder and editor |
| Rock pose | **Sunk** into the ground (a fraction of the model's height), **bigger at the foot**, smaller up the face. Upright (no surface tilt); rocks may overlap |
| Layer layout | One `UserInstanceGroup` per differential (`Slope 12`): its rocks, vegetation and exclusion polygons |
| Seed | Seed field + reroll; seed and parameters stored on the layer (`wce_*` attributes) so a re-run reproduces it |

## Defaults to confirm

- Hand-placed rocks (`incidental`) don't block (changed 2026-10-26, see the table). The AUTOGEN layer's own rocks never count.
- Parameters (all in the dialog): slope angle 35°, min rise 3 m, min region area 20 m², foot band 3 m up the slope + 4 m out on the flat,
  top margin 1 m, rock density foot 1 / 10 m² and face 1 / 25 m², vegetation density foot 1 / 15 m², vegetation spacing 2 m, rock scale foot
  0.9-1.6 and face 0.5-1.0, sink 25 % of model height, cluster distance 8 m, exclusion margin 2 m.
- Grid: 1 m samples of the terrain (`TileProject::height_at`), the whole tile.
- The layer's export type is Normal. It is created at the end of the root when missing; found by its name otherwise.

## Algorithm (`wce_tile::ground_coverage`, pure, no editor types)

Input: a height sampler (metres), the tile extents, the blocking footprints (centre + radius), the two sets, rock model bounds by key, the
parameters and the seed. Output: a list of differentials, each with its cells (for the preview), its state (kept / too low / too small /
blocked by building N) and its rocks, plants and exclusion polygons.

1. **Slope grid.** Heights on a 1 m grid; slope per cell from central differences; `steep = slope > angle`.
2. **Regions.** A closing (dilate, erode, 1 cell) so a cliff broken by a ledge stays one region; 8-connected components.
3. **Foot and top per cell.** From each steep cell walk down the gradient to the first non-steep cell (local foot height) and up it to the
   first non-steep cell (local top height). Along a long cliff the base and top change, so this is per cell, not per region.
4. **Filter.** Rise = 95th percentile of (top - foot) over the region's cells; drop it below the minimum rise; drop it below the minimum area.
   Drop it when a building footprint (radius + 1 m) touches the region or its foot band.
5. **Bands.** Foot band = steep cells with `h - foot <= foot_up`, plus the non-steep cells within `foot_out` of the region on its low side
   (distance transform from the region; "low side" = the cell's height is nearer the local foot than the local top). Face band = steep cells
   with `foot + foot_up < h <= top - top_margin`.
6. **Scatter** (xorshift seeded from `seed ^ region index`, so a region's result doesn't depend on the others): dart throwing in each band to
   the target density. Rocks: key from the rock set, scale from the band's range, random yaw, y = the lowest terrain under the rock's radius
   minus `sink * model height * scale` (MSM_FREEFORM, the height is explicit). Plants (foot band only): key from the vegetation set, the set's
   scale range else the dialog's, random yaw, kept `spacing` from each other and the rock radii.
7. **Exclusion polygons.** Single-linkage clusters of the plants (cluster distance); convex hull of each cluster's points pushed out by the
   plant radius + margin (each hull vertex offset along its bisector; a lone plant or two get a 12-sided circle / capsule); clipped to the tile.

Tests (synthetic heightfields): a 10 m step cliff -> one region, foot / face bands at the right heights; a 10° ramp -> nothing; a 1.5 m step
-> dropped (too low); a building on the cliff -> dropped (blocked); the same seed -> the same output; every plant is inside a polygon.

## Steps (a commit per step, branch `feature/auto_ground_coverage` from master)

1. **Generalised sets.** `vegetation_sets.rs` gets a kind (`<VEGETATION_SET kind="building">`, missing = vegetation, so the old files read
   unchanged); `veg_sets.rs` shows the kind and filters by it; "new set from selected" takes buildings too. Placing from a building set in
   the Create panel only if it's cheap (it reuses SetInUse); otherwise left for later.
2. **Algorithm** in `wce_tile::ground_coverage` with the tests above.
3. **Document side** (`wce_tile_editor::tile_document::ground_coverage`): footprints of the blocking buildings (`footprints()` minus the
   AUTOGEN layer), rock bounds via `building_model` (loaded when missing; until a model resolves its rock uses a 1 m radius / 1 m height
   and the dialog says so), the layer's children text built from `new_instances` (groups, `BUILDING_INSTANCE`, `VEGETATION_INSTANCE`,
   `VegetationExclusionPolygonInstance`), and one function that creates-or-finds the layer, writes its `wce_autogen_*` attributes and
   replaces its children in **one** undo entry (`begin` / `set_children_text` / `push_splice`, as `set_groups_children` does).
   `project_files::to_ted` already drops `wce_*` attributes; check it does for a layer's too.
4. **Dialog** (`wce_tile_tool`): "Experimental: Auto ground coverage..." in the Tile panel. Parameters, vegetation set and rock set pickers,
   seed + reroll, Preview (overlay of the regions: foot band, face band, dropped regions in red with the reason on hover, and the points
   that would be placed), Generate (warns once when the layer has items). Last values kept in `TileSettings`; when the tile's layer has
   `wce_autogen_*` attributes, they fill the dialog instead.
5. **Check in the app** on a few kit tiles with cliffs, then tune the defaults with the user.

## Risks / notes

- An exclusion polygon also stops BOB's procedural forest inside it: around the foot clusters there is no generated forest (accepted).
- Rock model bounds come from the model files; unresolved keys fall back as above. Sinking uses the lowest terrain under the rock so its
  downhill side doesn't float.
- Very large tiles: ~4 M cells at 1 m; the walks of step 3 are bounded by the region size. Should run well under a second; measure.
- TEd: everything is plain TEd data (buildings, trees, polygons, groups, a layer) except the `wce_autogen_*` attributes, which `to_ted` drops.
