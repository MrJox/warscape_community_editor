# River bridges in TDD's campaign battles (plan, 2026-11-25)

Bridges where TDD's campaign roads cross its rivers, in place of the fords its battles have there now; then TDD-styled bridges as their
variations. On `feature/river_crossings`. RE behind it: [docs/re/river_hints.md](../re/river_hints.md).

## What there is (measured 2026-11-25)

- Attila's kit has 29 `river_crossing` battle tiles (a 2 x 2 river with a road over it, every way the river and the road can cross it),
  complete raw projects, all with the one `bridge_wood_complete`. The packs ship 2 of them; both tile databases (vanilla's in
  `tiles.pack`, TDD's own) list all 29.
- Neither vanilla's campaign battle map (`main_attila_map/tile_map.tiles`) nor TDD's (`rom_third_age_map`) places any: the battle map's
  fill never picks them (why: not traced; their links, the fill's order).
- TDD's campaign map (`terrain/campaigns/rom_third_age_map/terrain.tile_list`) and its battle map (`terrain/battles/rom_third_age_map/
  terrain.tile_list`) share one grid (2032 x 1441 cells). The campaign map places 116 `river_crossing` tiles (17 kinds) where its roads
  cross its rivers; at 106 of them the battle map has a **`river_ford`** tile at the same origin (those are the fords "at regular
  intervals"), at 4 nothing starts there, at 5 a road's end piece, at 1 a plain river.
- TDD's `battlefield_buildings` has bridges of its own: `bridge_stone_1`, `bridge_stgon_1`, `gondor_bridge_small`,
  `gondor_bridge_large_*`, `moria_bridge_rough`, `pontoon`, `road_planks`, `bridge_wood_20m`.

## Decisions (user, 2026-11-25)

| Question | Answer |
|---|---|
| What first | The 2 x 2 tiles at TDD's campaign crossings: crossings (a road and a bridge) where roads arrive, fords where none do; then the plain river tiles with the same generator |
| How they reach the game | **New variations** (`bb`, `cc`, ...) of the existing tiles, the same names and links; the map patch picks one per placement by a seed; nothing vanilla replaced. The battle tile database in the pack is the one the game reads (TDD's) with the new variations added |
| Ground | **Fresh relief by seed** (rolling hills and valleys by noise), faded to the edges' heights where neighbours meet |
| Bridge | The editor's **generated wooden bridge** (procedural roads); TDD's bridges later |
| Roads | A dirt road from road link to road link, routed round steep ground (a semi-random way, curving round the terrain); over the river a bridge; with one road link: from it, over the river, ending somewhere on the far side |
| No road link | A ford |
| Terrain | The river from link to link along a natural, random path (curves), and the tile dressed: islands, a cliff bank, forest |

## Steps

1. **The 27 kit crossings into a pack** (done 2026-11-25, `kit_tiles_pack`; see above). Superseded by the generated variations for
   TDD, kept as a fallback.
2. **Where the spots are** (done 2026-11-25, `wce_tile` example `crossing_spots`): of the 116 campaign crossings, 80 have a battle
   crossing of their name (66 with road tiles at both road ends, 64 of them river at both river ends: all on fords now; 8 one road end;
   6 none); 36 are other campaign kinds (`2x2_lbl_rb_crossing_*`, `2x2_lb_rt_crossing_*`), 35 on fords.
3. **The generator, ground and river** (`wce_tile::river_tile_gen`, pure; an editor command over the open tile): fresh relief by seed
   (noise, faded to 0 over the land edges' 64 m and held under the links' cells); the river's line from its entry link to its exit link
   through random control points (meanders, no loops, square to the edges at the links), its widths randomized, its bed carved, its shore
   on; checked by script on a kit crossing and a ford tile.
   **Built 2026-11-25** (`generate_river_tile`, script `river_tile_gen <seed> [clear]`): `clear` takes the tile's rivers, roads,
   buildings, plants, hints, decals and water planes away and resets its paint (the grass group, the ground types cleared); the relief is
   fBm noise (14 m), faded over the edges' 64 m, with the river's **valley** in it (the ground within 70 m of its smoothed line going to a
   floor 1 m over the links' water); the river follows `river_line` (square to the edges for 30 m, meanders up to 22 % of its length to the
   side, never back on itself), is snapped to its links, its water set from one link's height to the other's along it (drawn on the
   ground, it climbed over hills and flooded the dips), its widths varied -30 .. +40 %. One undo entry; its undo gives back the scene text,
   the heights and the ground types exactly. Found on the way: `TedPointAttrsCommand` put back an attribute a point had not had as an empty
   one, the text longer than before, and the undo entries before it misplaced theirs (a crash); it now takes it away (`commands::ABSENT`).
4. **Dressing**: forest and undergrowth as biome areas away from the water and the road; an island where the river is wide enough (the
   river parted round raised ground); a cliff bank on one side of a stretch (a steeper bank and rocks).
5. **The road router**: a grid search over the tile from road link to road link, its cost the grade (steep ground avoided), the turns,
   the water (crossed once, square to the river), a seeded noise for a semi-random way; the path simplified into a `WceRoadSpline` (dirt),
   its ends snapped to the links; bridges on (the river is deep enough for a bridge where it crosses). One road link: from it over the
   river to a random end on the far side.
6. **Ford tiles**: the river and dressing as above, a ford across the middle.
7. **Batch**: every crossing and ford tile the spots use, N variations each (by seed), exported into one pack with the battle tile
   database (TDD's, the variations added).
8. **The map patch**: TDD's battle map placements at the spots replaced: a crossing (its variation by seed) where roads arrive at one end
   or both, a ford variation where none does; the other kinds matched by their river and road edges. Both `terrain.tile_list` and
   `tile_map.tiles` written alike; in-game check.
9. Then the plain river tiles (the generator without the road), and TDD's bridges.

## Open

- Which file a campaign battle reads its tiles from (`terrain.tile_list` or `tile_map.tiles`): an in-game check after step 2.
- Where the packs go (a pack of the editor's, or into TDD's).
