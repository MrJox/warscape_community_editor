# Tile tool: what is left of the TEd parity plan (temporary)

A working list drawn from `tile_ted_parity.md` (2026-10-30), for the `ingame-checks` branch. Every phase there is done except the items
below. Each item goes back into `tile_ted_parity.md` when it is done; this file goes when the list is empty.

## A. Can be done now

1. **Snapping leftovers** (Phase 5g). TEd's `Ted::GRID_SETTINGS` names four things wce lacks: `snap_mode` `SF_SNAP_TO_EDGES`, the
   `angle_snap_tolerance`, and `horizontal_snap` / `vertical_snap`. Their code was never traced.
   - Trace them in TEd first (Ghidra, black box in TEd if it runs), then add them to the grid popup and the `snap` command.
   - Extend `scripts/tile_snap.txt`.
2. **Normal (Grouped) weight gate** (Phase 5a). TEd's `PM2OpBlend2Base::apply` (fn 0x00482ad0) gates the weight with a per-pixel
   function that its module hands it: where that function is 0, the weight is its neighbourhood's average. wce leaves the gate out.
   - Trace the function.
   - Add it to `PaintMode::Grouped`, with a unit test.
3. **Burning sequence** (Phase 5d follow-up). The editor shows the steady fire only. The engine (fn 0x104bb1e0) has five stages:
   `burning_entity_pre_smoke`, then `burning_entity`, then `burning_entity_collapse` / `entity_collapse` / `burning_entity_aftermath`.
   - Trace the stage timing (`BUILDING_FIRE_STAGE_ONE_DURATION`, `_TWO_`, the `BUILDING_LITE_*` ones).
   - Trace the `_small` choice (entity type 0xb) and the per-triangle mask of the mesh emission (fn 0x1183fa30).
   - Then decide with the user whether the editor plays the sequence: a "play the fire" preview in the Inspector, with the char amount
     ramping to `BURN_SHADER_AMOUNT` 0.3. Or does `burn` stay a static preview of the main stage?
4. **Faded opaque materials** (Phase 5d, `opacity` below 1). TEd files such a building for the blended forward shaders (render flag
   0x2000, `rigidstandard ps30_alpha_blend_40`), which the renderer lacks, so opaque materials don't fade. This is renderer work.
   - Add a blended forward pass for those instances, after the deferred lighting.
   - Check it in `scripts/tile_building_look.txt` (`opacity_05.png`).
5. **`scripts/tile_building_props.txt` is broken** (found 2026-10-29, also without the fire changes). Line 30,
   `tile_edit selected orientation 0,1,0`, fails with "no row 'orientation' to edit". The Inspector's row now seems to be
   "orientation (world)". Fix the script or the row name, then run the script through.
6. **Plan upkeep**: `tile_ted_parity.md` is out of date in places.
   - The status table is stale: 6a says "read-only view", and 6b / 5g list less than is done.
   - The header still names the `workflow-review` branch.
   - The "ground views in a viewport chip" idea is done on master (the Ground chip).

## B. In-game captures (done 2026-10-30, except the coast)

Captures, the cameras read from the dumps, the editor's matching renders and the notes: `%TEMP%\wce_scratch\captures`
(`findings.md`). The game's camera in editor scene units: (x / 2 + half, y / 2 + offset, z / 2 + half); the offset is the tile's
`min_height` for a tile alone, about -30 at Rome's campaign placement (the land under it). Check each dump's `camera_near_far`
(1 / 13000): a dump can hold another pass's camera.

- Done: season mixing (3e): none; the ground shows one season, as the editor does (docs/re/terrain.md).
- Done: the ground climate: the battle map's cell climate; a tile launched alone gets `default` (summer only).
8. Done (2026-10-30): **the coast**. The crash was the missing `<sea_surface_name>` (now written; 07aa3d3). A coast tile alone has no
   sea level in game (the game's own tile too): launch it at a campaign placement (a note says so; 31ddecc). Its captures: two look
   references (the dumps held another pass's camera, as they do with sea in view): the game's surf band is wider and whiter, its sand
   lighter and rippled.
9. **The battle sea** (Phase 4): the radial mesh and the sea bed texture need a frame capture of a coastal battle. PIX captures only
   D3D12 (Attila is D3D11), so RenderDoc, which crashed the game before: one try with the current version.
10. **Launch in game**: drop `<Climate>` / `<Season>` (the game reads neither); say that a tile launched alone has the `default`
    ground. (Coast tiles alone: a note, done.)
11. **Look differences** the captures showed (renderer work, each to look into):
    - roofs: game red terracotta, editor grey-beige (rigid materials; not the faction colours, which are white in game too);
    - trees: game summer trees full and dark, editor thin and pale; game has many small bushes the editor lacks;
    - water: editor brighter, with brownish streaks and sharper glints;
    - settlement buildings: the editor shows sets the game doesn't (a tent cluster, huts): the upgrade level?
    - the editor's `where` "game" readout lacks the centring and the height offset;
    - the strategy view (top-down) is the game's fog of war, not a render to match.

## C. Not planned

- Polygons painted with a brush (exclusion and no-go polygons): a new feature, not TEd's ("Ideas for later").
- Piece-to-piece snapping onto the logic files' platforms and collision: nothing needed it.
- The stacked crates (Phase 2): left unfixed (user, 2026-10-30).

## Suggested order

B's 8-10 with the climates' follow-ups, then 5 and 6, then 3 (tracing only, then ask), 1, 2, 4 and 11.
