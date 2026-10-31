# Tile tool: what is left of the TEd parity plan (temporary)

A working list drawn from `tile_ted_parity.md` (2026-10-30), for the `ingame-checks` branch. Every phase there is done except the items
below. Each item goes back into `tile_ted_parity.md` when it is done; this file goes when the list is empty.

## A. Can be done now

1. Done (2026-10-31): **Snapping leftovers**: traced statically (tile_ted_parity.md "TEd's snapping"); then wce's own (user: not TEd's):
   snap to edges, footprint corners, heights, the angle tolerance (`scripts/tile_snap.txt`, unit test).
2. Done (2026-10-30): **Normal (Grouped) weight gate**: the Blendmap Filters' test; a pixel they leave out takes the share of its 5 x 5
   box that passes (`blend_field::grouped_gate`, unit test; docs/re/terrain.md).
3. Done (2026-10-31): **Burning sequence**: traced (docs/re/buildings.md) and played by the fire preview (user: play it; `fire.rs`,
   the Inspector's *Play the fire*, `tile_fire`, `scripts/tile_building_look.txt`).
4. Done (2026-10-30): **Faded opaque materials**: the blended forward pass (`pass_faded`, the materials' `ps30_alpha_blend_40`).
5. Done (2026-10-30): `scripts/tile_building_props.txt` runs through again (the orientation row is `euler` since the Global / Local
   coordinates).
6. Done (2026-10-30): plan upkeep (`tile_ted_parity.md`'s header and status table).

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
    - Done: the environment specular pass (`environment_specular_light.hlsl`, blurred in x and y; f7247be).
    - Done: the river: `g_ssr_enabled` 1 and the rivers before the environment specular pass, as the game (docs/re/water.md).
    - **Coast**: the surf band looked narrower and less white than the game's; no capture of the fixed coast launch has a usable camera
      (the dumps held other passes' cameras), so it needs one more (screenshot + dump, near / far 1 / 13000).
    - Done: trees fuller and darker: the tree list follows the ground's climate (08cc811); the leaves and branches are drawn
      two-sided and tinted by their materials' colours (docs/re/vegetation.md). Left: the game's MSAA alpha-to-coverage edges.
    - Done: settlement buildings the game doesn't show (Rome's camp of round huts): its *Escalation 2* layer, which the game loads
      for the `escalation2` upgrade only; the Game view now leaves out the layers the battle would not load.
    - Done: exposure: the tone mapper's statistics now run on the game's fixed 256 x 256 image halved to 1 x 1; at the Rome view the
      white point is 22.3 against the game's 22.1 with the same packs (docs/re/renderer.md).
    - Done: `where` reads the launched battle's game coordinates (centred, the height offset; docs/editor.md).
    - Done: the image gates' references re-recorded (`build/gates_base_perf`, docs/baseline.md); the rain and snow shots' run-to-run
      difference was the dock layout an earlier run saved (`shots.sh` gives each capture fresh settings now).

## C. Needs review

- Done (2026-10-31): piece-to-piece snapping reviewed against the logic files and rebuilt on their end caps (tile_ted_parity.md, Phase
  5g): corners, angled joiners and stepped bastions join, outer faces together, at the wall-walks' heights.

## D. Not planned

- Polygons painted with a brush (exclusion and no-go polygons): a new feature, not TEd's ("Ideas for later").
- The stacked crates (Phase 2): left unfixed (user, 2026-10-30).

## Suggested order

5 and 6, the image gates, then 11, then 3 (tracing only, then ask), 1, 2 and 4.
