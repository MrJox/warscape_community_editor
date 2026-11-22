# Experimental: automatic ground coverage (shelved)

**Shelved (2026-11-21, the user's call) until a better approach is found.** The whole ground coverage (its kinds cliffs, coast,
riverbanks, shores and around buildings; its window, brush, presets, script commands, settings and tests, and the Tile panel's
Experimental section with it) is out of master and kept as built on `feature/shelved_ground_coverage`. Do not bring it back without the
user. What stays on master: what the biomes and texture rules shared of it, as `wce_tile::placing` (the terrain on a grid, a placed item,
inside a polygon, the real buildings' categories); rock sets (a vegetation set of `kind="building"`, `vegetation_sets::SetKind`); a
building's scale with *Export as prop* (`wce_tile::export_as_prop`, below). Reeds and stones by rivers and lakes came back as the rivers
plan's shores ([rivers.md](rivers.md), step 10).

## What it was

A Tile panel button (2026-10-26) that found the tile's steep height differentials and decorated them: rocks and vegetation along the foot
of each, rocks only up its face to just under the top edge, into the layer `(AUTOGEN) Ground Coverage`, with vegetation exclusion
polygons around the vegetation it added (BOB keeps manual trees only inside exclusion polygons, docs/re/vegetation.md). Redesigned
2026-10-31 into a contextual coverage tool: kinds with their own detection, fill the tile or a live brush, named presets per kind, one
layer with a group per kind; later (2026-11-18 .. 20) the kinds *Around buildings* ([auto_detail.md](auto_detail.md), B) and *Shores*
([auto_detail_2.md](auto_detail_2.md), F). All steps were built; the user's look and tuning (step 6) was never finished.

## Decisions (user)

2026-10-26:
- **Rocks** are `BUILDING_INSTANCE`s of the kit's own rocks (`gaul_rock_01/02`, `rocks_small_01`, `attila_rock_small_*`, ...) from a
  weighted **rock set** (the vegetation sets generalised with a kind); no texture painting. Sunk into the ground, bigger at the foot,
  upright. Scaled rocks optional; since 2026-11-18 a scaled rock takes the building's own `scale` and *Export as prop*
  (`wce_export_as_prop`), since the game ignores a building's scale otherwise (docs/re/buildings.md): plain models, no collision.
- **Buildings**: the coverage goes around them (`building_clearance`, 3 m); only real buildings count (categories generic, fort_wall,
  fort_tower, unbreachable_fort_wall, gate, bridge, from the packs' `battlefield_buildings`, else the kit's). Before, a TDD settlement's
  path borders and dummies dropped 274 of 340 slopes.
- **Detection**: slope above an angle (about 35 degrees), connected regions, kept when their rise is above a minimum (about 3 m).
- **Bands**: a foot band (rocks and vegetation) and a face band (rocks only, sparser); one exclusion polygon per cluster of vegetation.
- **Re-run** replaces the layer's contents, one undo entry; seed and parameters stored on the layer (`wce_*` attributes, which `to_ted`
  drops).

2026-10-31 (the redesign):
- **Fill the whole tile or brush** (live, as the vegetation brush; Ctrl + drag takes coverage away; one undo entry per stroke).
- **Kinds**: inland cliffs (only where the foot is over the sea), coastal and riverbank (the non-walkable coast and banks: **the game's
  no-go field**, BOB's automatic no-go areas, steeper than 30 degrees less the go outlines). **A tile meets the sea when its entry links
  to the `sea` tile set** (`TILE_LINKS`, `TLT_EQUALS`), as BOB's stage B knows it; its heights alone don't say it (kit river tiles lie
  all under 0 m).
- **Named presets per kind** (files under `raw_data/art/ground_coverage_presets`), built-in defaults per kind.
- **One layer, a group per kind** (`Cliffs`, `Coast`, `Riverbanks`), filling a kind replacing only its filled items, brushed items kept.
- **Lower minimum rises by the water**: 0.5 m for riverbanks (was 1.5 m), 1 m for the coast (was 2 m).

## Survey of the kit (2026-10-31)

- **Coasts**: on the first variation of every sea_coast, sea_coast_landing, river_mouth folder and two of each port, the kit's coasts are
  beaches: 37 sea coast and landing tiles found no coast at all (their no-go ground, 0 to 1,700 m², is under the sea or away from it);
  river mouths 0 to 4 slopes, 0.6 to 1.8 m over the sea; ports: nordic_port_a 4 to 5 covered, western_port_a/medium 2, the others none.
- **Riverbanks**: river_shallow and lakes have no no-go ground by their water. River tiles have 8 to 153 banks each, nearly all low:
  0.2 to 1.4 m over the water, half under 0.2 to 0.4 m.
- sea_coast's 4x3 and 6x4 folders have no entries: the kit's tile database lists only its 1x1 and 2x2 sea coasts, and the game's packs
  have only those (the bigger folders are leftovers BOB never builds).
- mediterranean_mountains/custom_pillar_hercules_a (cliffs, defaults, seed 7): 301 slopes, 97 covered, 18,685 rocks, 5,339 plants, 635
  polygons, about 1 s in release; a whole mountainside is one differential (283,000 m², 15,900 rocks on its face).

## Left open when it was shelved

- The user's look and tuning on real coasts and banks; the brush with the mouse (only driven by script).
- The banks' "local water surface" for a river whose surface falls along the tile (the nearest water cell's height was the guess).
- An exclusion polygon also stops BOB's procedural forest inside it (accepted).
