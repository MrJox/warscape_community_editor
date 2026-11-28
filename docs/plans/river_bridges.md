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
   **Built 2026-11-25** (cliff and woods; islands not yet): every other seed has a **cliff** on the outer side of the river's sharpest
   bend (its middle half; 18 samples of the smoothed line, tapered at the ends): its foot 3 m past the river's widest water (measured
   from its width handles, after its widths are varied; the relief is imported again with it), climbing 9 m over 9 m; kit rocks
   (`gaul_rock_01/02`) every 9 m along its face where it stands half its height. **Woods**: blob areas of a built-in biome (broadleaf,
   pine on a third of the seeds) where a forest noise is high, 15 m past the widest water, off each other and the edges. The river is drawn
   through the smoothed line (2 points a stretch: its spline turned sharply at the line's few points), its widths -20 .. +30 %. Found on
   the way: an undo of an attribute's addition (`commands::ABSENT`) failed once a later structural edit had baked it into the text, and
   the splices undone before it then cut the wrong bytes (a crash); `XmlDoc::remove_attr` takes it out of the text.
   Islands need the river parted round raised ground (its bed layer cuts any mound under its ribbon): left for later.
5. **The road router**: a grid search over the tile from road link to road link, its cost the grade (steep ground avoided), the turns,
   the water (crossed once, square to the river), a seeded noise for a semi-random way; the path simplified into a `WceRoadSpline` (dirt),
   its ends snapped to the links; bridges on (the river is deep enough for a bridge where it crosses). One road link: from it over the
   river to a random end on the far side.
   **Built 2026-11-26** (`wce_tile::road_route`; script `river_tile_gen <seed> [clear] [road=all|none|<link>]`): an A* search over a 4 m
   grid, its state the cell and the way the road came in (turns of 90 degrees at most): a metre costs 1, plus the grade (squared, as a
   share of 0.22; 60 more over it), a seeded noise (up to 1.2) and near the edges (up to 2 within 48 m, not at its ends: the relief fades flat
   there and the way ran along them); turns 6 m per 45 degrees squared; the water 6 a metre, 400 to go in and to turn in it (crossed once,
   straight, square). Simplified within 2.5 m, each stretch in and out of the water on its own. The road (dirt, its bridge by the
   procedural roads where the river is deep) goes from road link to road link (`road=all`), or from one link (`road=<n>`, or the tile's only
   one) over the river to a place by the seed on the far side, 30 m past the water, half the tile from the link, 40 m in from the edges.
   The woods keep 14 m off it. Checked on the kit crossing (links on the north and south edges) with seeds 22 .. 28.
   Changed on the way: the hills stay 2 m over the valley's floor (dips under the river's water became ponds), the river runs straight
   from its links for 50 m (it bent sharply inside its own width), and the clear takes the kit tile's exclusion areas too.
   Open: on some seeds the river's line still bends hard near a link (seed 23).
6. **Ford tiles**: the river and dressing as above, a ford across the middle.
   **Built 2026-11-26**: a tile with no road (no road link, or `road=none`) gets a ford (`add_river_ford`: the bed held at wading depth,
   its edges lined with stones and splash, its crossing and go hints on export) where the river's line runs straight nearest the tile's
   middle (`wce_tile::river_tile_gen::ford_place`: the distance from the middle, plus 80 m a radian of turn over a few samples, plus up to
   24 m by the seed), 6 samples clear of the cliff's stretch (the cliff left out if it has the only place); 24 .. 40 m long by the seed;
   the woods keep half its length and 14 m off the line across the river there (70 m past the water to either side). Checked on the six
   ford kinds the spots use (`2x2_lt_rt`, `lt_rb`, `lb_rt`, `bl_rt`, `br_rt`, `lt_bl`; seeds 22, 23): the bed 0.27 m under the water at
   the ford (the kit's 0.25), 3 m away from it.
6.5. **Cliff banks** (user, 2026-11-28: cliffs of cliff rock buildings, set per river point by a checkbox): a river point's `wce_cliff`
   (`left` / `right`, as the river runs) makes the bank on that side a cliff from halfway to the point before to halfway to the one after
   (neighbouring points on one side, one run). The river's bed raises the ground behind the run to the cliff's top (a plateau, tapered at
   its ends, easing back to the land); the shore lays the kit's cliff pieces along the bank (`attila_cliff_01..03`, the kit's settlements
   chain them 11.3 m apart (18 m long, 6 m tall), their rock face on their -z side, their top grass blending into the ground; the game's list in
   `shores.toml`, `cliffs`), and no reeds or stones on that bank. A checkbox in the river point's rows (on: the outer bank of its bend),
   a script command; the generator ticks its outer bend's points instead of its own raised relief and loose rocks.
   **Built 2026-11-28** (`river_cliffs.rs`, `river_bed::CliffBank`, `shores::cliff_pieces`; script `river_cliff <point>|all
   left|right|outer|off | list`): the pieces measured in the editor's metres are 18 m long, 2 m under their middle to 4.15 m over it
   (the model bounds are in the scene's units, twice the editor's metres on a battle tile), their rock face on -z sloping from their foot
   3.5 m in front of their middle; they stand a metre inside the water's drawn edge (the bank meets the water some metres inside it: at
   the edge, a strip of shallow bank with stones lay in front of them), 1 m over the water, one every 11.5 m at most along the bank's own
   line (along the river's middle line they parted round the outside of a bend). The ground behind climbs inside them (from 2 m inside
   the edge over 2 m: climbing from the water, it buried their faces) to 5 m over the water, held 8 m, easing down over 25 m, tapering over
   12 m along; behind them the river's bank cut is kept off the land. The generator marks the river's points along its bend's outer side
   but those within 6 samples of where its road crosses (two at least, or no cliff); its raised relief and loose rocks are gone.
   **Fixed 2026-11-28** (user: raised ground with no cliff in front at the ends, gaps, an abrupt border at a bend, rocks wanted at the
   ends): the ground tapered 12 m past the stretch's ends, a ramp of bare grass with no face; now the face falls inside the stretch over
   24 m with its pieces sunk and tilted along it (`CliffBank::face`, `shores::CliffPiece::up`), nothing raised past the ends at the water,
   a shoulder behind. By the inside of a bend the ground's cells take the cliff of their nearest place only, so a cliff ending there stood
   as a wall on the bisector: now blended over 20 m from the other leg, behind its own bank (`Channel::blend_cliffs`). The bank's offset
   line folded by the inside of a tight bend and piled pieces there, turned every way: its folded samples are left out. Two rocks at each
   end where the face is half its height (`cliff_ends`, `attila_rock_01/03/04`).
   **Reworked 2026-11-28** (user): a one-point cliff (27 m) had 9 m ramps tilting its pieces 30-40° and a shoulder 28 m past its ends:
   ramps are 24 m always, the shoulder 0.3 m a metre back. Shores by point (user's choices): the river's *cliffs* in its Shores (none,
   outer bends, left, right, both) with each point's own over it (as the river, none, left, right, both); each point's own reeds, stones
   and decals too (`ShoreStretch`, blended over 10 m); a seeded *Shore variation* (cliff runs of 2-3 points on the outer bank, amounts a
   step up or down; hand-set points kept, `wce_shore_random`); the river panel's long notes moved into tooltips.
7. **Batch**: every crossing and ford tile the spots use, N variations each (by seed), exported into one pack with the battle tile
   database (TDD's, the variations added).
8. **The map patch**: TDD's battle map placements at the spots replaced: a crossing (its variation by seed) where roads arrive at one end
   or both, a ford variation where none does; the other kinds matched by their river and road edges. Both `terrain.tile_list` and
   `tile_map.tiles` written alike; in-game check.
9. Then the plain river tiles (the generator without the road), and TDD's bridges.

## Open

- Which file a campaign battle reads its tiles from (`terrain.tile_list` or `tile_map.tiles`): an in-game check after step 2.
- Where the packs go (a pack of the editor's, or into TDD's).
