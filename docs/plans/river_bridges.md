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

## Steps

1. **The 27 kit crossings into a pack** (done 2026-11-25): `wce_tile_io` example `kit_tiles_pack` exports kit tiles as their folders alone
   (no tile database: the game's lists them) and checks each one's height range against the database the game reads (all 27 the same) and,
   with `--compare`, its files against the packs' (the 2 that ship: the export is today's compile, theirs an older one). Output:
   `~/wce_re/scratch/river_bridges/wce_river_crossings.pack` (27 tiles, 1,084 files, 46 MB). **In game**: a battle on one of them.
2. **The battle map patched**: a tool reading TDD's campaign and battle tile lists (and `tile_map.tiles` / `.index`: which one the game
   reads for a campaign battle is to be checked, so both are written alike), replacing each ford placed under a campaign crossing with the
   battle crossing of the campaign tile's name and rotation, after checking that its river and road links meet the neighbours as the ford's
   did; the other cases reported, not changed. Written into a pack of its own (after TDD's in load order).
3. **TDD-styled bridges**: variations of the crossing tiles (the river and road as the kit's, the bridge one of TDD's, its banks dressed);
   a placement takes a variation by the patch tool (pinned by region, culture or a seed: the user's choice).
4. The same patch for the variation pilot (rivers' new variations, docs: the TDD river survey) once 2 and 3 work.

## Open

- Which file a campaign battle reads its tiles from (`terrain.tile_list` or `tile_map.tiles`): an in-game check after step 2.
- Whether every campaign crossing should become a bridge, or some stay fords (by seed, by region).
- Where the packs go (a pack of the editor's, or into TDD's).
