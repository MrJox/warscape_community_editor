# Tile editor: automatic detail, second round (2026-11-20)

The ideas left from the first round's list ([auto_detail.md](auto_detail.md)), built in the order the user picked on `feature/auto_detail_2`
(2026-11-20), all six:

- **D. Erosion and fine detail**: hydraulic and thermal erosion plus fine noise over the heights, buildings and roads left flat; where
  water ran and where it dropped its load become texture rule conditions.
- **E. Decals by rules**: puddles in hollows, dirt at building bases, ruts along roads, leaf litter under trees, laid by rules.
- **F. Waterside detail**: reeds and rocks on the banks you can walk on, stones in the riverbed, driftwood on coasts.
- **G. Fields and farmland**: draw an area and get field strips with their own textures, fences round them, orchard rows and haystacks.
- **H. Paths between things**: dirt paths found over the slope between gates, buildings and roads, made as procedural roads.
- **I. District filler**: draw a district and it fills with buildings or prefabs facing the roads, keeping off steep ground.

## Status (2026-11-22): all six shelved

**Shelved 2026-11-21, the user's call after a look**; each stays as built on its branch and is not to be brought back without the user:

| Tool | Branch | Why |
|---|---|---|
| D erosion, I districts | `feature/shelved_erosion_districts` | the user's look |
| H paths | `feature/shelved_paths` (with its zigzag fix) | less control than drawing a procedural road |
| E decal rules, G fields | `feature/shelved_fields_decal_rules` | the user's look |
| F shores (a ground coverage kind) | `feature/shelved_ground_coverage` | went with the ground coverage ([ground_coverage.md](ground_coverage.md)) |

What stays on master from this round: new decals at their own size (`wce_tile::decal_sizes`) and decal pictures in the key picker; the
procedural roads' snapping to the tile's road links ([procedural_roads.md](procedural_roads.md)); the area brush for every closed
polygon type (`area_brush.rs`, also the biomes'); and of D, the texture rules' *Water flow* condition worked out from the heights
(`texture_rules::flow_of`; *Deposit* went with the erosion). A tile saved with an erosion layer drops it when opened
(`terrain_layers::RETIRED_OWNERS`). Reeds and stones by rivers and lakes came back another way: the rivers plan's shores
([rivers.md](rivers.md), step 10, `wce_tile::shores`).

## Decisions (mine, 2026-11-20; the user could overturn them)

| Question | Answer |
|---|---|
| Erosion: live or by a button | **Live**, like the texture rules: a derived height layer remade off the editor's thread. Settings in `erosion.wce` beside the tile |
| Erosion's place in the stack | Over the user's height layers, under the procedural ones (roads, walls, prefab ground) |
| Decals, fields: derived or saved | **Derived** (live, laid again from their rules or areas, added at export), like the biomes' trees |
| Paths, districts: derived or saved | **Made once** with an undo entry: a path is a procedural road you then edit; a district's buildings are plain buildings in a group |
| Waterside | A fifth ground coverage kind, **Shores** |

## What was built, and found (for a return)

- **D** (`wce_tile::erosion`): droplets (Beyer's particle model) with brush-weighted erosion, thermal talus, slope-scaled fractal detail,
  a mask keeping buildings and roads; an owned height layer `erosion#<sig>`; Water flow and Deposit conditions. A drop's load goes by the
  slope it runs down and its speed is capped (the first run dug -11 888 m into a 4 m-a-sample mountain).
- **E** (`wce_tile::decal_rules`): the texture rules' conditions plus the distance to trees (the decal rules' alone: in the texture rules
  it would loop, the generated forest following the blend map they paint), density, size, spacing, alignment; live, exported in a
  `WCE decals` layer. On the Nordic city: 2757 decals (743 at building bases, 1928 under trees, 68 puddles). Decal thumbnails have no
  ground to project onto (pictures came later and stayed).
- **F**: bands from the water's surface and depth (reeds at the edge, stones in the bed); built-in reeds and stones when no set is chosen.
- **G** (`wce_tile::fields`): the crops are the kit's field decals (`global_fields_1..6`, 50 x 25 m in its farm tiles; its `field_1..4`
  are 5 m squares) laid along strips, the ground one texture rule bound to the area, fences a closed building spline's; later farms cut by
  lanes, each fenced, and kinds of farmland. Found: adding anything to an empty kit tile's root failed (fixed on master).
- **H** (`wce_tile::paths`): A* over the heights by slope; water costs rather than bars (a path crosses where a river is narrowest; the
  roads make the ford or bridge); simplified at 3 m, then pulled taut (a grid's way zigzags on a slant).
- **I** (`wce_tile::districts`): lots along the roads facing them, footprints kept off roads, buildings and steep ground. Found: the kits'
  tables name buildings the game has no model for (the default takes only keys with model pieces); the building splines' fallback size
  (5 x 0.5 m, a fence's) made slivers (a district guesses 8 x 8 m); a size not known when first asked was cached for good (a district
  looks again).
