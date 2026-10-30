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
5. Done (2026-10-30): `scripts/tile_building_props.txt` runs through again (the orientation row is `euler` since the Global / Local
   coordinates).
6. **Plan upkeep**: `tile_ted_parity.md` is out of date in places.
   - The status table is stale: 6a says "read-only view", and 6b / 5g list less than is done.
   - The header still names the `workflow-review` branch.
   - The "ground views in a viewport chip" idea is done on master (the Ground chip).

## B. In-game captures (done 2026-10-30)

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
9. Done (2026-10-30): **the battle sea**. RenderDoc works once the 32-bit game uses less memory (only the launch pack, low settings):
   the radial mesh and the sea bed texture are in docs/re/water.md; the sea bed texture is now 1024 x 1024 (c9d149a).
10. Done (2026-10-30): **Launch in game** writes no `<Climate>` / `<Season>` (8e71d1a) and notes a tile launched alone that will have
    the `default` ground (2fe4d7c).
11. **Look differences** the captures showed (renderer work):
    - Done: roofs grey-blue: `g_apply_environment_specular` was 1, the game's is 0 (b5f2cac, docs/re/renderer.md).
    - Done: a tree on a steep bank lay along it (the trees' second, untilted matrix; 20c8eea).
    - **The environment specular pass**: the game adds the sky's reflection in a pass of its own (`environment_specular_light.hlsl`,
      lower resolution, blurred in x and y); the editor has none since b5f2cac. Small on the roofs (+0.1).
    - **Water**: the editor's river is bluer and brighter than the game's dark slate, with brownish streaks and sharper glints; the
      coast's surf band is narrower and less white than the game's.
    - **Trees**: the game's summer trees are fuller and darker, and it has many small bushes the editor lacks; at Rome the editor draws
      another species than the game at the same spot (which tree list the game picks: the climate's?).
    - **Settlement buildings**: the editor shows sets the game doesn't (a tent cluster, huts): the upgrade level?
    - **Exposure**: re-measure the white point (game 22.0, editor 16.4 at the Rome view) now that the lighting matches.
    - The editor's `where` "game" readout lacks the centring and the height offset.
    - The image gates' references change with b5f2cac and 20c8eea: to run and record.

## C. Needs review

- Piece-to-piece snapping onto the logic files' platforms (the wall-walks) and collision shapes: the end-to-end join uses the end
  anchors only. Using them would place things on a wall-walk at its height and join corners, towers and gatehouses by their surfaces
  (user, 2026-10-30: needs review; first read what the logic files hold for a few wall and gate pieces).

## D. Not planned

- Polygons painted with a brush (exclusion and no-go polygons): a new feature, not TEd's ("Ideas for later").
- The stacked crates (Phase 2): left unfixed (user, 2026-10-30).

## Suggested order

5 and 6, the image gates, then 11, then 3 (tracing only, then ask), 1, 2 and 4.
