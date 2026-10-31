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
    - Coast (2026-10-31, a screenshot and two dumps at max settings, sea_coast/2x2_bb_straight at its campaign placement, the camera
      matched): the surf band now matches the game's closely (foam width, whitecaps); found and fixed on the way: the battle's frame is
      turned 180 degrees at that placement, and the editor lit the scene with the sun from the opposite side (5deff33). Left:
      - Done: **sand**. The beach's look is the tile's projected decals (`global_terrain_beach_01`, `beach01blend`, `beach01end`:
        ripples by the water, a seaweed and pebble band, pitted dry sand), not the ground textures. TEd's decal scales have a
        negative z, and the editor drew decals with the plain `ps30_diffuse`, whose v runs the other way (the wet edge inland) and which
        shears turned non-square boxes; they are drawn with `ps30_diffuse_bugged` now, exact for any box (docs/re/renderer.md
        "Projected decals: which pixel shader"; also gen_dun_medium_final_2's smeared roads).
        Also found: the game's ground there is ROME_Atlantic (its texture paths in the dump: agri, forest, grass, mud, rock, sand,
        settlement_1..3, the nine slots of `g_blend_pixel_scale_array`), the battle map cell's climate.
      - **Shallows** (open): the game's water by the shore is clear over the sand under the foam; the editor's foam band is more
        opaque. Ruled out (2026-10-31): the water constants (`g_sea_bed_light_scatter`, `g_refraction_light_scatter`,
        `g_deep_water_colour`, `g_transparency_exponential_scale` equal the game's dump), the ground under the shore (the editor's
        heights along the camera's line equal the game's sea bed texture from coast.rdc within 0.1 - 0.2 m, its 0 m line at the same
        place) and the waves' phase (captures 5 s apart: the waterline does not move). Left: the shading's inputs (the refraction
        copy, the shadowed shader variant); needs the game's frame at that view (RenderDoc runs out of memory at max settings).
      - Done: **horizon**. The dark slivers were sea pixels with no ground behind them: the fog reads `gbuffer_channel_4`, which the
        game's is its depth buffer cleared to 1 (the far plane) and the editor's an R32F target cleared to 0 (the near plane: no fog).
        It is cleared to 1 now (docs/re/renderer.md "G-buffer channel 4"). Also: the sea reaches 28.8 km as the game's mesh (the lone
        tile's was the tile and its border; the map context's stopped at the map's edges). Left: the game has ground under its sea
        out to 8 km (the battle's 16 384 m LF, `vs_height_map_cb.g_world_dimensions`), so its far sea fogs smoothly with distance;
        the editor's sea beyond the tile's ground (or past the map's edge) has nothing under it and fogs fully, a brighter band
        towards the horizon. A sea bed under the sea there (at the LF's -20 m) would close it.
    - Done: trees fuller and darker: the tree list follows the ground's climate (08cc811); the leaves and branches are drawn
      two-sided and tinted by their materials' colours (docs/re/vegetation.md). Left: the game's MSAA alpha-to-coverage edges: no MSAA
      support for now (user, 2026-10-31; docs/re/vegetation.md).
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
