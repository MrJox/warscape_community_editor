# Tile editor: automatic detail (plan, 2026-11-18)

Make tiles more detailed with less hand work: the ground textured by rules from the terrain itself, props laid around buildings by what
they are, and whole biomes painted at once. Three features, in this order, on `feature/auto_detail`:

- **A. Texture rules**: a generated blend layer: rock on steep slopes, mud in hollows and by the water, dry grass on ridges, worn ground by
  roads and buildings. Rebuilt live as the terrain changes; hand paint stays over it.
- **B. Clutter around buildings**: props from weighted sets laid around buildings by their category, as a kind of the ground coverage.
- **C. Biome brush**: a brush or a polygon that lays a preset's texture rules, trees, undergrowth and rocks at once, with a graded edge.

B and C build on the ground coverage (docs/plans/ground_coverage.md: kinds, presets, fill or brush, the `(AUTOGEN) Ground Coverage`
layer, exclusion polygons around what it plants), rebased onto this branch and merged into it (user, 2026-11-18).

Status (2026-11-18): A's steps 1 to 3 done (the rules, their layer under the user's, made live; no UI yet but the document's API). The
ground coverage rebased onto master (19 commits and a formatting one, `feature/auto_ground_coverage`) and merged into this branch.

## Decisions (user, 2026-11-18)

| Question | Answer |
|---|---|
| Hand paint and the rules | **Hand paint wins**: the rules' layer lies under the user's blend layers, right over the base |
| When the rules run | **Live**: made again off the editor's thread once an edit settles, as the roads' layers are |
| The ground coverage | Rebased onto `feature/auto_detail` and merged into it; B and C continue it here |

## A. Texture rules

### What a rule is

A rule says where a texture goes, by the terrain under it; the rules of a tile are a **rule set**, applied in order (a later rule over an
earlier one, as blend layers go over each other).

- **Texture**: a texture group by name (`rock`, `mud`, `grass`, `forest`, ...; the names of the climates' `texture_set`s, as the Paint
  brush lists them) and a variation, 0 .. 3, continuous as Normal (Grouped)'s (1.3: 70 % of variation 1, 30 % of 2). A group the tile's
  `TEXTURE_GROUP` does not have is skipped, and the panel says so.
- **Conditions**, each a range with soft edges (a fade in metres or degrees on each side), multiplied together:
  - **height** (metres; absolute, or over the sea level of a tile that meets the sea),
  - **slope** (degrees),
  - **curvature** (hollows to ridges, from the height's Laplacian over a radius in metres: where water collects, where wind dries),
  - **distance to water** (rivers, lakes, the sea: stage B's water map, as the ground coverage's context has it),
  - **distance to roads** (the procedural roads' strips) and **to buildings** (the real buildings' footprints, the coverage's categories).
- **Breakup**: noise over the result (scale in metres, amount, the rule's seed), so that edges are not contour lines.
- **Strength**, 0 .. 1.
- Optional **ground type**: where the rule's weight is over a threshold, the ground type map gets that type too (sounds, unit speed). Off
  by default: ground types are gameplay.

### The layer

- One blend layer owned by the rules (owner `texture_rules`), **right over the base, under every layer of the user's**: the first owned
  layer that is not on top. `Stack::add`, `put_owned_on_top` and the roads' sort (`roads.rs`, `road_terrain`) learn an owned layer that
  goes under (the rule today: what procedural features own applies last). The roads and kit walls stay on top of everything.
- A pixel of the layer is the rules' channels with a cover of their summed weight: where no rule holds, the layer is empty and what is
  below shows.
- **Painting with no working layer goes to the base**, which lies under the rules: the paint would not show where a rule covers it. When a
  tile has rules, the editor makes the working layer a user layer (`Painted`, made once if there is none) before the first stroke.
- Made again **live**: a job off the editor's thread (`tile_document::jobs`, as `road_terrain`), started when the heights, water, roads or
  buildings changed and nothing has moved for a moment; over the edited rectangle widened by the rules' largest reach (curvature radius,
  distances), the whole map when the rules changed. The result goes in as the stack's own change, so undo takes the edit and its texture
  back together.
- Combined projects: made per variation, as the roads' layers.

### Rule sets

- A **preset** is a file under the projects' `raw_data/art/texture_rule_sets/<name>.wce` (beside `vegetation_sets` and the coverage's
  presets), XML as they are. Built in: `Default` (rock on slopes, mud in hollows and by water, worn ground by roads and buildings) and a few
  by climate if the group names allow.
- **The tile keeps its own copy** of the rules and the preset's name, in `texture_rules.wce` beside its `tile.wce` (as
  `terrain_layers.wcl`; changed while building from an element in tile.wce: a file of its own needs nothing of the TEd conversion, the
  variations' scene or the splices): a tile is shareable without the preset, and editing a preset does not change tiles until they load
  it again. A kit save, an export and a variation's build leave the file out; Save as writes it.

### UI

- Tile panel, Terrain layers section: the rules' layer shows as owned (not editable by hand), with **Rules...** opening the rules window.
- The rules window: preset combo (Save, Save as, Delete, "(changed)"), the list of rules (on / off, reorder, add, remove, duplicate), the
  selected rule's texture and conditions, each condition with a small graph of its range and fades.
- **Preview** of a rule: the ground views get a "rule weight" view (white where the rule lays its texture) for the selected rule.

### Steps (a commit per step)

1. **The rules in `wce_tile`** (done: `texture_rules.rs`): rules, conditions, fades, breakup; a rule set from and to text; the analysis fields
   (slope, curvature) from a height field; `generate` over a rectangle into a blend layer's pixels. Unit tests on synthetic fields: a
   ramp gets rock from its threshold, a bowl mud at its bottom, a later rule over an earlier one, an unknown group skipped, nothing where no
   rule holds. Done as planned, plus a built-in `Default` (mud in hollows, rock on slopes) and the rules' reach (how far past an edit the
   result can change). The breakup moves each condition's value by up to its fade times the amount, so the inside of a range never changes
   and no spots appear far from an edge.
2. **The owned layer under the user's** (done): the stack's ordering (an owned layer that goes under), the roads' sort, painting with no working
   layer into `Painted`. Tests: hand paint over the rules shows; roads stay on top; undo of a stroke and of the rules' change.
   Done: `terrain_layers::order_of` (0 under the user's, 1 theirs, 2 on top), `UNDER_OWNERS`, `Stack::first_on_top`; the roads' and the
   prefabs' ground follow it. Merge down stops at the rules' layer (it would go when the layer is made again). Painting with texture rules
   and no working layer makes or picks `Painted` first (its own undo entry), for strokes, strokes along a line and Paint the whole tile.
3. **Live regeneration in the editor** (done, but the command and the reach-widened rectangle): the job, the triggers, the reach-widened rectangle, combined projects. Script command
   `texture_rules <on|off|regenerate|load <preset>>`; a check script on a kit tile (a few tiles with `--filter`, no corpus run).
   Done: `tile_document/texture_rules.rs`. The layer's owner is `texture_rules#<signature>`, the signature a hash of the rules, the texture
   group slots and the heights' content (hashed again only when the field's version moves): a tile opened again does not remake it. A
   change waits a look (a drag, a stroke), then a job remakes the **whole map** (the rectangle widened by the reach is left for when it is
   measured to be needed); it goes in when the blend stack is as it was taken, no undo entry (an undo puts back what it is made from).
   Edits of the rules are undo entries (`set_texture_rules`); exports wait for it after the roads (`finish_road_jobs`). Grass follows.
   Left for step 5: the script command and the check on kit tiles.
4. **Distances**: water (stage B's water map), roads, buildings.
5. **Presets and the window**, the rule weight view.
6. **Ground types per rule**.
7. **Check in the app** (the user): defaults of `Default` on a few kit tiles of different climates.

## B. Clutter around buildings (after A; questions to settle when it starts)

A ground coverage kind, **Around buildings**: for each real building of a category (or a key pattern), props from a weighted set (a
building set, `kind="building"`) in a ring around its footprint, at a density, clear of other footprints, roads and the building's
entrances. Fill and brush as the other kinds, a group per kind in the coverage's layer.

To settle with the user: which categories get what (farms: hay, carts; storehouses: crates, barrels; walls: debris), whether a building's
entrances can be known (the kit's building data, or a side from the footprint's shape) or are left to the user, whether rules are per
culture, how the props stand (Follow surface), the cap per building.

## C. Biome brush (after B; questions to settle when it starts)

A preset of a biome: texture rules (A), a tree set and an undergrowth set (vegetation sets), a rock set, densities and an edge band. Painted
with a brush or filled into a polygon: the texture into the rules' layer masked by the biome's area, the trees and rocks as coverage
(exclusion polygons around the trees, as the coverage makes them); towards the edge, smaller trees and denser undergrowth.

To settle with the user: brush, polygon or both; whether a biome area stays editable (a polygon that is made again when moved) or is laid
once; how biomes meet BOB's procedural trees (`tree_list`) and grass (which follow the blend map, so the texture rules already steer the
grass).
