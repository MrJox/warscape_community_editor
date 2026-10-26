# Experimental: automatic ground coverage

A Tile panel button that finds the tile's steep height differentials (slopes, cliffs) and decorates them: rocks and vegetation along the foot of
each one, rocks only up its face to just under the top edge. Everything goes into the layer `(AUTOGEN) Ground Coverage`, with vegetation
exclusion polygons around the vegetation it adds (BOB drops manual trees that are outside every exclusion polygon, docs/re/vegetation.md).

Status (2026-10-26): steps 1-4 done on `feature/auto_ground_coverage`; step 5 (trying it on kit tiles, tuning the defaults) is the user's.
Building scale (`wce_scale`) was added for the rocks and is **optional** (`scale the rocks`, off by default): TEd has no building scale and
whether Attila honours the building list's scale is not known. Script command for checks: `ground_coverage <preview|generate> <rocks|-> <plants|-> [seed]`.

Seen on mediterranean_mountains/custom_pillar_hercules_a (defaults, seed 7): 301 slopes, 97 covered, 18,685 rocks, 5,339 plants, 635
polygons, about 1 s in release. A whole mountainside is one differential (283,000 m², 15,900 rocks on its face): the face density and a cap
per differential may need tuning.

## Decisions (user, 2026-10-26)

| Question | Answer |
|---|---|
| Rock coverage | Rock `BUILDING_INSTANCE`s (the kit's own way: `gaul_rock_01/02`, `rocks_small_01`, `attila_rock_small_*`, ...) picked from a weighted **rock set**; no terrain texture painting |
| "No collision" | Skip a differential when a **building** footprint overlaps it (whole differential dropped, no avoid-around) |
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

- Hand-placed rocks (any `BUILDING_INSTANCE` outside the AUTOGEN layer) count as buildings and block a differential. The AUTOGEN layer's own
  rocks never block (they are replaced).
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
