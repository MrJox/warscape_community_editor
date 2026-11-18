# Tile editor: automatic detail (plan, 2026-11-18)

Make tiles more detailed with less hand work: the ground textured by rules from the terrain itself, props laid around buildings by what
they are, and whole biomes painted at once. Three features, in this order, on `feature/auto_detail`:

- **A. Texture rules**: a generated blend layer: rock on steep slopes, mud in hollows and by the water, dry grass on ridges, worn ground by
  roads and buildings. Rebuilt live as the terrain changes; hand paint stays over it.
- **B. Clutter around buildings**: props from weighted sets laid around buildings by their category, as a kind of the ground coverage.
- **C. Biome brush**: a brush or a polygon that lays a preset's texture rules, trees, undergrowth and rocks at once, with a graded edge.

B and C build on the ground coverage (docs/plans/ground_coverage.md: kinds, presets, fill or brush, the `(AUTOGEN) Ground Coverage`
layer, exclusion polygons around what it plants), rebased onto this branch and merged into it (user, 2026-11-18).

Status (2026-11-18): A's steps 1 to 6 done; step 7 (the look on kit tiles, `Default`'s values) is the user's. B's step 1 done (the rules, their layer under the user's, made live; no UI yet but the document's API). The
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
4. **Distances** (done): water, roads, buildings. Done as: `texture_rules::Distances` (exact Euclidean distance transform on the blend
   map's grid), the rules' `water`, `road` and `building` ranges (endless where the tile has none). Water is the roads' `WaterMap`
   (rivers' ribbons and lake planes over the ground, the sea under 0 in the sea's sets), not stage B's (live, without a compile); roads
   are the road-owned blend layers' pixels; buildings are the real ones by the kits' categories (the coverage's), discs of their models'
   radius. Only what an enabled rule uses is worked out; the scene's part is cached by the scene's and the models' versions.
5. **Presets and the window** (done), the rule weight view. Done as: `texture_rules::Preset` / `scan_presets` (the built-in `Default` and
   the files), groups by alternatives (`mud|mud_a|mud_b`: the kit names the same ground differently), a fuller `Default` (mud by the
   water and in hollows, worn ground by roads and by buildings, rock on slopes); the window (`texture_rules_panel.rs`, Tile panel >
   Texture rules...): on / off, preset combo with Save, Save as, Delete, the tile's groups and the rules that lay nothing, the list
   (on / off, add, duplicate, remove, up, down) and the selected rule's fields, an edit given to the tile as one undo entry once no field
   is active. The rule weight view became an **outline in the viewport** of where the selected rule's weight is at least a half (on a
   grid of a 256th of the tile, 1 m at least): a debug terrain view would have touched the bake, the minimap and the view. Script
   command `texture_rules`. Checked on kit tiles (2026-11-18, `~/wce_re/scratch/auto_detail/check`): river/2x2_lt_rb (mud by the water
   19 % of the tile, rock skipped: the tile has none), mediterranean_mountains/custom_pillar_hercules_a (8.6 % of the map, 76 % rock as
   `mountain`, 24 % mud; the rock band widens along the face), settlement_nordic_cities/nordic_city_a/medium (nothing: its groups are
   `settlement_1..3` and `forest`; settlements need their own preset).
6. **Ground types per rule** (done). A rule may name a ground type and a weight threshold (`ground="rock" ground_threshold="0.5"`);
   where its weight passes it, the type goes into a ground type layer the rules own, under the user's as their blend layer; the latest
   rule laying one at a place wins. `Default` lays none (gameplay). A tile without a ground type map still gets its textures. The ground
   brush with rules that lay ground types and no working layer goes into a `Painted` ground layer, as the paint brush does.
7. **Check in the app** (the user): defaults of `Default` on a few kit tiles of different climates.

## B. Clutter around buildings

A ground coverage kind, **Around buildings** (`Kind::Buildings`): for each real building a clutter rule matches, props from the rule's
set in a ring around its footprint. Fill and brush as the other kinds, its own group in the coverage's layer.

### Decisions (user, 2026-11-18)

| Question | Answer |
|---|---|
| Which buildings get what | **Key patterns** (`*villa*\|*farm*`), the category as a fallback; the first rule matching a building is its |
| Doors and streets | **All around**: the user clears a door by hand (or Ctrl + drag with the brush) |
| The props | **Incidental buildings**, the kit's way (barrels, crates, carts are `incidental` BUILDING_INSTANCEs) |
| Where | **A fourth tab of the ground coverage window**: presets, Fill the tile / Brush, its group in `(AUTOGEN) Ground Coverage` |

Found: the kit's `battlefield_buildings` has 823 `generic` keys (houses, temples, farms alike), 371 `incidental` (tents, washing lines,
market stands, barricades, carts, hides, dock goods), 129 `fort_wall`, 76 `ground_platform`, 30 `fort_tower`, 22 unbreachable walls, 10
gates, 10 bridges: the category alone can't tell a farm from a temple.

### Design

- A building is a `Target`: its key and category, where it stands, its turn and its footprint as a **rectangle** in its own frame (the
  model's bounds, scaled): a circle's corners would leave the long sides of a house bare. Its ring is the ground `ring_gap` ..
  `ring_gap + ring_width` out of the rectangle; ground within `ring_gap` of any building (a neighbour) is left out.
- A `ClutterRule`: name, key patterns (`*`, `|`), categories (`|`), and its props: a building set by name, or keys inline (the built-in
  rules', so that they work without set files).
- Props stand on the ground (seat at the centre, not sunk, scale 1), at the density, apart from each other, no more than
  `max_per_building` a building.

### Steps (a commit per step)

1. **The kind in `wce_tile::ground_coverage`** (done): `Kind::Buildings`, `Target`, `ClutterRule`, `glob`, the ring classification,
   the props per region's rule in the fill and the brush (`Classified::owner`), the three parameters. Tests: patterns, a turned
   footprint, a ring round a matched house clear of its unmatched neighbour, the cap.
2. **Rules in the presets** and the built-in ones (markets, docks, townhouses, barbarian houses, farms and villas, from the kit's own
   incidental keys).
3. **The tile's side and the window's tab**: the targets from the scene (the real buildings by the kits' categories, their models'
   bounds, their world turn), the rules' editor, the sets resolved; Fill and Brush.
4. **Check on kit settlement tiles** (a few, `--filter`-sized), then the user's look.

## C. Biome brush (after B; questions to settle when it starts)

A preset of a biome: texture rules (A), a tree set and an undergrowth set (vegetation sets), a rock set, densities and an edge band. Painted
with a brush or filled into a polygon: the texture into the rules' layer masked by the biome's area, the trees and rocks as coverage
(exclusion polygons around the trees, as the coverage makes them); towards the edge, smaller trees and denser undergrowth.

To settle with the user: brush, polygon or both; whether a biome area stays editable (a polygon that is made again when moved) or is laid
once; how biomes meet BOB's procedural trees (`tree_list`) and grass (which follow the blend map, so the texture rules already steer the
grass).
