# Tile editor: automatic detail (2026-11-18)

Make tiles more detailed with less hand work: the ground textured by rules from the terrain itself, props laid around buildings by what
they are, and whole biomes painted at once. Three features, built in this order on `feature/auto_detail` (2026-11-18 .. 20), now on master:

- **A. Texture rules**: a generated blend layer: rock on steep slopes, mud in hollows and by the water, dry grass on ridges, worn ground by
  roads and buildings. Rebuilt live as the terrain changes; hand paint stays over it.
- **B. Clutter around buildings**: props from weighted sets laid around buildings by their category, as a kind of the ground coverage.
  **Shelved 2026-11-21** with the ground coverage (kept as built on `feature/shelved_ground_coverage`; [ground_coverage.md](ground_coverage.md)).
- **C. Biome areas**: a polygon (drawn, or painted with the area brush) that lays a preset's texture rules, trees, undergrowth and rocks
  at once, with a graded edge.

What the biomes and texture rules shared of the ground coverage (the terrain grid, a placed item, inside a polygon, the real buildings'
categories) stays on master as `wce_tile::placing`. The follow-up ideas are [auto_detail_2.md](auto_detail_2.md).

Status (2026-11-22): A and C built; the user's look on kit tiles is left (A's `Default` values, C's biomes).

## Decisions (user, 2026-11-18)

| Question | Answer |
|---|---|
| Hand paint and the rules | **Hand paint wins**: the rules' layer lies under the user's blend layers, right over the base |
| A tile's own painting (2026-11-18) | **Only over the main ground**: a rule's weight is scaled by how much the base is the texture group most of it is; what the base has painted in other textures (a kit tile's forest, streets, mud) stays |
| When the rules run | **Live**: made again off the editor's thread once an edit settles, as the roads' layers are |
| The ground coverage | Rebased onto `feature/auto_detail` and merged into it; B and C continued it (shelved again 2026-11-21, B with it) |

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
  - **distance to water** (rivers, lakes, the sea: the roads' `WaterMap`, live, not stage B's),
  - **distance to roads** (the road-owned blend layers' pixels) and **to buildings** (the real buildings by the kits' categories, discs of
    their models' radius),
  - **water flow** (where water runs, worked out from the heights; `Default`'s *Silt where water runs*; the erosion's Deposit went with
    the erosion, [auto_detail_2.md](auto_detail_2.md)).
- **Breakup**: noise over the result (scale in metres, amount, the rule's seed), so that edges are not contour lines.
- **Strength**, 0 .. 1.
- Optional **ground type**: where the rule's weight is over a threshold, the ground type map gets that type too (sounds, unit speed). Off
  by default: ground types are gameplay.

### The layer

- One blend layer owned by the rules (owner `texture_rules#<signature>`), **right over the base, under every layer of the user's**
  (`terrain_layers::order_of`: 0 under the user's, 1 theirs, 2 on top; `UNDER_OWNERS`, `Stack::first_on_top`). The roads and kit walls
  stay on top of everything. Merge down stops at the rules' layer.
- A pixel of the layer is the rules' channels with a cover of their summed weight: where no rule holds, the layer is empty and what is
  below shows.
- **Painting with no working layer goes to the base**, which lies under the rules: the paint would not show where a rule covers it. When a
  tile has rules, the editor makes the working layer a user layer (`Painted`, made once if there is none) before the first stroke.
- Made again **live** (`tile_document/texture_rules.rs`): a job off the editor's thread, started when the heights, water, roads or
  buildings changed and nothing has moved for a moment. Only what changed is made again (2026-11-19: `RulesBasis`,
  `Fields::changed_area`, the heights widened by the rules' reach); another rule set or grid, or an undo past the layer, makes the whole
  map. It goes in when the blend stack is as it was taken, with no undo entry (an undo puts back what it is made from); edits of the rules
  are undo entries (`set_texture_rules`). Exports wait for it after the roads (`finish_road_jobs`). Grass follows.
- **Ground types**: a rule may name a ground type and a weight threshold (`ground="rock" ground_threshold="0.5"`), into a ground type
  layer the rules own, under the user's; `Default` lays none (gameplay).
- Combined projects: made per variation, as the roads' layers.

### Rule sets

- A **preset** is a file under the projects' `raw_data/art/texture_rule_sets/<name>.wce` (beside `vegetation_sets`), XML
  (`texture_rules::Preset`, `scan_presets`). Groups by alternatives (`mud|mud_a|mud_b`: the kits name the same ground differently).
  Built in: `Default` (mud by the water and in hollows, silt where water runs, worn ground by roads and buildings, rock on slopes;
  settlement tiles' `settlement_2` as worn ground and `settlement_3` as rock, the packs' textures in every climate).
- **The tile keeps its own copy** of the rules and the preset's name, in `texture_rules.wce` beside its `tile.wce` (as
  `terrain_layers.wcl`; changed while building from an element in tile.wce: a file of its own needs nothing of the TEd conversion, the
  variations' scene or the splices): a tile is shareable without the preset, and editing a preset does not change tiles until they load
  it again. A kit save, an export and a variation's build leave the file out; Save as writes it.

### UI

- The Terrain layers' Blend row over the base (and Paint mode's *Texture rules...*) shows the rules in the Inspector
  (`texture_rules_panel.rs`, styled: Simple and Advanced views, condition chips, range bars): on / off, the preset combo (Save, Save as,
  Delete), the tile's groups and the rules that lay nothing, the list (on / off, add, duplicate, remove, up, down) and the selected rule.
- The selected rule's **outline** in the viewport: where its weight is at least a half (a debug terrain view would have touched the bake,
  the minimap and the view).
- Painting with rules and no working layer makes or picks a `Painted` layer first (the base lies under the rules), for the blend and the
  ground type brushes.
- Script command `texture_rules`.

Checked on kit tiles (2026-11-18, `~/wce_re/scratch/auto_detail/check`): river/2x2_lt_rb (0.4 % of the map once only over the main
ground: its banks are painted mud already), mediterranean_mountains/custom_pillar_hercules_a (8.6 %, 76 % rock as `mountain`),
settlement_nordic_cities/nordic_city_a/medium (2.7 %: worn ground round the huts and by the streets, rock on the cliff).

## B. Clutter around buildings (shelved 2026-11-21)

A ground coverage kind, **Around buildings**: for each real building a clutter rule matches (key patterns with exceptions, categories as
a fallback), incidental buildings from the rule's set in a ring around its footprint rectangle. Built and checked on kit settlement tiles
(2026-11-18), then shelved with the ground coverage (`feature/shelved_ground_coverage`). Decisions (user, 2026-11-18): key patterns
(`*villa*|*farm*`, `except`), all around (the user clears a door), incidental buildings as the kit's props, a tab of the ground coverage
window. Found: the kit's `battlefield_buildings` has 823 `generic` keys (houses, temples, farms alike), 371 `incidental`, 129
`fort_wall`, 76 `ground_platform`, 30 `fort_tower`, 22 unbreachable walls, 10 gates, 10 bridges: the category alone can't tell a farm from
a temple; the kit's `*_villa_*` keys are the cities' houses.

## C. Biome areas

A biome is a preset of a kind of ground: its trees, undergrowth and rocks (sets, or keys inline), their densities, an edge band (smaller,
sparser trees and denser undergrowth towards the edge) and texture rules. It is laid in **areas**: polygons drawn on the tile (and
touched up with a brush), kept live.

### Decisions (user, 2026-11-18)

| Question | Answer |
|---|---|
| How it is put on | **Polygon + brush**: an area for big regions; a brush that adds to or takes from the nearest area |
| Editable | **Live**: an area is an editor element (as a procedural road); move a point or change its biome and its contents are made again |
| BOB's trees | **The area is an exclusion zone**: it exports as a `VegetationExclusionPolygonInstance`, so BOB's procedural trees go there and the biome's own trees are kept |
| Texture | **Texture rules in the area**: the biome's rules (A's) apply inside its areas only, fading out at the edge; hand paint still goes over them |

### Design

- `wce_tile::biomes`: the `Biome` (its props by set name or keys, densities, spacing, scale ranges, edge band, texture rules), its file
  text (`<BIOME>` with the rules' `<rule>` children), the built-in ones from the kit's vegetation (broadleaf wood, pine forest, olive
  grove, reeds and marsh, palm grove), and `generate` (an area's trees, undergrowth and rocks: deterministic by the area's seed, the
  edge band by the distance to the polygon's boundary, spacing between trees, clear of what it is told to keep off).
- A tile keeps the biomes its areas use in `biomes.wce` beside `tile.wce` (as `texture_rules.wce`): self-contained and shareable; the
  presets are files in the projects' `raw_data/art/biomes`.
- An area **is a `VegetationExclusionPolygonInstance`** with the editor's `wce_biome` (the biome's name) and `wce_biome_seed`
  attributes (changed while building from an element of its own, `WceBiomeArea`: it is the exclusion zone the user chose, so TEd and
  BOB get a plain exclusion polygon once `to_ted` drops the attributes, and selection, undo, variations and the editor's preview of
  BOB's trees already know it). Its contents are made in the session, drawn as the tree list's trees and the roads' generated
  buildings are (locked, not tile.ted's), and added in a layer of their own by an export (as `with_road_items`).
- The texture: the rules' job (A) also lays every area's biome rules, each weighted by the area's mask (1 inside past the edge band,
  fading to 0 at the boundary).
- The brush: a stroke's circles rasterised with the area's mask on a grid (added, or taken away with Ctrl), the outline traced again and
  simplified (`biomes::stroke_outline`, `TileDocument::biome_stroke`, one undo entry); a stroke away from every area makes a new one.
  Since 2026-11-21 this is the area brush of every closed polygon type (`area_brush.rs`: the Inspector's *Draw by* Click points or
  Paint).

### As built (2026-11-18 .. 20)

`wce_tile::biomes` (model, text, built-ins, `generate`, tests), `tile_document/biomes.rs` (the area, `biomes.wce`, live contents off the
editor's thread clear of the real buildings and the roads' texture, drawn locked, exported in a layer `WCE biomes` after
`finish_road_jobs`), the texture through A's job (`Rule::area`, an `AreaMask`; the layer is made for areas alone too),
`biomes_panel.rs` (presets, the biome's fields, its texture rules with A's rule editor, key pickers with thumbnails). Commands
`biome_area`, `biome_brush`, `area_brush`, `biomes_wait`, `biomes`. Checked on river/2x2_lt_rb (2026-11-18): a Pine forest area drawn and
a Broadleaf wood brushed, 1135 trees and plants, 34 rocks, the rules' layer 1.9 % of the map.

## Where the tools live (user, 2026-11-19)

Out of the Tile panel's Experimental section and their floating windows, into the Inspector, where the editor sets up what it makes:
biomes are Create panel entries (Vegetation > Biomes) shown in the Inspector with their settings, laying first (Draw an area / Brush);
the texture rules are the Terrain layers > Blend row over the base (and Paint mode's *Texture rules...*). One shows at a time; selecting something else, or
making another entry, gives the Inspector back; a selected biome area shows its biome below its properties (*Edit its biome*, Reroll).
Then (user, same day): the texture rules lose their "experimental" mark; the biomes' Create panel entries get a picture (a 20 m
patch of the biome's inside, kept by its values), and the vegetation is listed by species (the Create panel's folders, the key
picker's headings). The ground coverage went back to the Tile panel's Experimental section, and both left master on 2026-11-21.

## Open

- [ ] The user's look in the app: A's `Default` on kit tiles of different climates; C's biomes.


