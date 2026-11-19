# Campaign tilemap rendering: plan

The campaign map in the Terrain tool (`terrain/campaigns/<map>`) drawn as the game draws it, checked the way the tiles and battle maps are:
the game's frame and the editor's at the same camera, compared pass by pass. Written 2026-11-19; nothing built yet. It builds on
docs/plans/campaign_environment.md (lighting, sky, fog, overlay, weather: mostly done) and docs/plans/terrain_editor.md Phase 8 (the
campaign map's contents), and takes over their open rendering items.

Marks: **V** verified (data, disassembly, code), **H** hypothesis, to be checked. Captures are the user's (`D:\renderdoc\camp_*.rdc`);
RenderDoc scripts run through the hidden wrapper (`wce_scratch/py/rd_hidden.sh`), never `qrenderdoc --python` directly.

## Scope

In: everything the game draws of the map itself: the ground (tiles, cliffs, roads, farmland, the far ground), the sea, lakes and rivers,
the forests, props, settlements (buildings, sprawl, walls, ports), borders and trade routes, the sky, fog, shroud and fog of war, the
placed and weather VFX. Out: armies, agents, fleets, settlement banners, selection rings, the UI (masked out of the comparison, below),
editing the campaign `.lighting` (campaign_environment.md Phase 6).

Games: Attila's `main_attila_map` and TDD's `rom_third_age_map` first, Thrones' `vik_attila_map` after with its own captures. Rome 2 (no
cubemap, `light_grid.xml`, sprawl models missing from the packs, no aqueducts) is a later plan.

## Decisions (user, 2026-11-19)

1. **The game's world, not the editor's square frame.** The game's campaign world is the hex grid's: z is 1.1547 (2/√3) times the
   editor's (V: the terrain patches' instance matrices in capture A, z x 1.1547; `map_data.esf`'s height 556.144 = 1441 rows x 0.33391
   x 1.1547). The view draws in that world: the tiles with the game's instance matrices, every model, line and texture at its
   game-world place. The documents keep their own coordinates (`.terry`, cells); picking, the gizmo, the Map panel and the brushes
   convert at the view's edge.
2. **Accurate = constants and images.** At a capture's camera: every pass's shader constants equal the game's (within float noise),
   and the G-buffer and final (pre-UI) images match per region (ground, sea, forests, buildings, sky) within thresholds set from the
   first baseline and reviewed with the user; then the user's side-by-side look.
3. **Attila and TDD, then Thrones.** Rome 2 later.

## Where it stands (2026-11-19)

Settled against captures (campaign_environment.md, "Progress"): the units (one camera in world units, far 500), the shadow cascades'
rows, the lighting spheres and stage names, the skybox and `cubemap.dds`, `battlefog` then the shroud plane, the sea and lake
constants, the river overlay and its camera, borders and trade routes (exact vs capture A), the rain, the campaign's zero weather
constants and fixed VFX wind, the placed effects' rules, the fog-of-war texture (97.5 % of texels exact). The ground goes through the
battle `rigidterrain` passes, as the game's (V, docs/re/terrain.md).

Never done: a whole frame compared at the same camera. Known gaps (V, from the code unless marked):
- **Frame:** the editor draws z 15 % short (decision 1). The campaign view keeps the battle lens (vertical fov 65°, near 1 scene unit;
  `camera_controller.rs`): the game's is 80°, near 1 world unit. The view streams short of the horizon.
- **Forests:** the clumps are drawn as static meshes (`model_cache.rs` ~1128), not with `rigidcampaignvegetation.hlsl`: no turn, no
  lean towards the camera, `campaign_tree_ps` unset. Their size and lean are ASSUMPTIONS.
- **Coast strip:** `campaignterrain.hlsl` `ps30_main_coast` is drawn nowhere in the editor (H: whether the game draws it on Attila).
- **Grass:** drawn on campaign maps; capture A's frame has no grass pass (kept unchecked for TDD's marsh reeds).
- **Normals:** which LF normal chunks the game's campaign terrain reads (`terrain/campaigns/<map>/normals` or the battle set) is not
  checked against the editor's.
- **Settlements:** buildings unturned (the turn is not in the startpos, not found in the DLL); sprawl levels, walls around zones
  without a building, the farmland rule and the road cap are ASSUMPTIONS. Seen 2026-11-18: in scripted runs Rome's settlement buildings
  were not drawn (their models never requested).
- **Environment leftovers:** the skybox casts no shadow (the game's does; the editor's cascades stripe Rauros); the shroud plane reads
  depth but does not write it; the overlay's size (1.5 x the screen) and the rivers' half-width are ASSUMPTIONS; the sea border's rebel
  exception, the placed effects' activation granularity, the storm clouds' drift direction and the started effects' removal are not
  traced; the fog of war and the shroud at the map's edge are not compared with the game.
- **Map:** far ground with no tile data olive in some runs and pink in others (parked 2026-11-07); TDD's Rauros: a straight east water
  edge, two black notches, dark triangles of water plane beside the marsh pools.

## Phases

### 1. The capture reader (tooling, no rendering change)

A RenderDoc script, `scripts/re/rd_campaign_frame.py` (the style of `rd_terrain_dump.py`), writing one folder per capture:
- `camera.json`: `camera_VS_PS` from the terrain's first draw (eye, view, projection, near / far, viewport size) and the frame's time
  constant (the shroud clouds', water's and particles' clock).
- `constants/<pass>.json`: every cbuffer of one representative draw per pass (terrain resolve, sea, lake, overlay rivers and borders,
  light, fog, shroud plane, skybox, a prop, a tree clump, a settlement building, grass if any), named as `dump_cbs.py` names them.
- `targets/`: the G-buffer (albedo, normal, the material bits), depth, the lit HDR before tone mapping and the tone-mapped image
  before the UI, as PNG / EXR with the event they come from.
- `mask.png`: the pixels of what is out of scope: the G-buffer before and after the armies', agents', fleets' and banners' draws
  (picked by shader and model path), differing pixels masked; the UI is not in the pre-UI target.
- `draws.csv`: the draws with their shader, model or texture names and instance matrices (the settlements' turns, the trees' size and
  lean, the coast strip's existence, the normal chunk bound to the terrain).

Check: capture A (`camp_attila_frame6628.rdc`) gives the camera already noted (eye (207.5, 9.3, 280.1), fov 80°, near 1, far 500) and
the constants the plan quotes. Docs: a "Campaign frame" section in docs/re/renderer.md for the pass list and what each target holds.

### 2. The game's world and lens

- **Find first (capture A's `draws.csv`, H):** whether props, tree clumps and settlement models carry the z x 1.1547 in their instance
  matrices as the terrain does, or are placed at stretched positions unstretched themselves; the same for the rivers' overlay meshes,
  the lakes' quads and the sea.
- One campaign world transform in `wce_scene` (the frame's cell to world: x by the cell size, z by the cell size x 1.1547, the render
  origin at the map's south-west corner as now) used by every campaign drawable as the game does it (decided by the step above); the
  places that divide by `CAMPAIGN_Z_SCALE` today (the borders, the lighting spheres' z) keep the game's z instead. The LF-sampled
  textures (`s_lf_colour`, the snow masks, `s_shroud`, the overlay) read at world xz over the world's size, as the game's.
- The ray picking, the gizmo, the LF brush, the Map panel's camera box and the entity overlays convert world to frame and back at one
  place. Battle maps unchanged.
- The campaign lens on a campaign map: vertical fov 80°, near 1 world unit, far 500 (kept); the streaming reach covering the far plane
  (the horizon), within the performance budget (docs/performance.md).
- A command `terrain_cam_game <camera.json>`: the view at the capture's eye, view and projection, the capture's resolution through
  `--capture <WxH>`, the clock at the capture's time, the season, stage and fog-of-war faction from the capture's notes.

Check: the editor's `camera_VS_PS` (the `cb` command) equals `camera.json`; the borders (already exact in game-world units) land on the
capture's border pixels; 20 landmarks (settlement centres, the Tiber's mouth, Alpine passes) at the same pixels within 1 px.
Prerequisite: settlement buildings drawn in scripted runs (the 2026-11-18 observation fixed or found gone).

### 3. The comparison

`scripts/campaign_compare.sh <capture folder>`: an unattended editor run (`--capture`, hidden desktop, ends with `quit`) at the
capture's camera, the editor's own constants and targets, and `scripts/campaign_compare.py` (numpy / PIL, as `imgcmp.py`):
- constants: every field of every pass, game against editor, the differing ones listed by size;
- images: per region (from the game's G-buffer bits, depth and the pass masks: ground, sea and lakes, forests, buildings, sky; the
  out-of-scope mask left out) the mean and 95th percentile difference, the luminance and hue shift, a heat map PNG and a side-by-side;
- a report under `%TEMP%\wce_scratch\campcmp\<capture>` and a summary in docs/baseline/campaign_compare.txt.

The editor's targets: a RenderDoc capture of the editor's scripted run (H: not set up for the Terrain tool yet; the `cb` command gives
the constants without it). Time-driven parts (water, shroud clouds, particles) compare at the capture's clock; where that cannot be
matched they compare by statistics, not per pixel. Gate `campaign_compare_<capture>` in `gates.sh` when `WCE_CAMPAIGN_CAPTURES` names
the captures' folder (as `WCE_BCA_ORACLE`); captures and extracted targets never committed.

The first baseline over captures A (6628, 7363) and the rain ones sets the thresholds, reviewed with the user.

### 4. Closing the gaps (each checked by Phase 3, the largest region error first)

1. **Forests**: `rigidcampaignvegetation.hlsl` for the clumps (the 0.4 in the shader, not the model cache), the instance's turn and
   lean, `campaign_tree_ps`; size and lean from the capture's instances.
2. **The frame's leftovers** found by the first baseline (constants that differ: shadows, fog, light, sea).
3. **Coast strip**: if capture A draws `ps30_main_coast`, its geometry's source (H: a built file or the sea LF's edge) and the draw;
   if not, noted as not drawn by the game.
4. **Grass**: drawn on campaign maps only if the game draws it (A says no; TDD's reeds by the TDD capture).
5. **Normals**: the chunk set the game binds; the editor's the same (and the suspected 2.5 x scale of the edited chunks for `main_*`
   maps checked, memory note of 2026-11-06).
6. **Settlements**: the buildings' turns from the capture's instances (the rule, or the display slot's field once found); sprawl,
   walls, ports, farmland and road levels against capture C's close view; each ASSUMPTION replaced or kept with its evidence.
7. **Skybox shadow** (the user's decision, pending): once the shadow constants match the game's, cast it as the game does and compare
   at Rauros; the stripes then point at a remaining cascade difference.
8. **Shroud plane depth write**, the overlay's size (a capture at another resolution) and the rivers' half-width (their pixels in the
   overlay target): the game's.
9. **Map**: the far ground's colour rule (one tile per climate, fixed); the horizon (Phase 2's reach).
10. **Environment leftovers** with their captures: the fog of war over Germania, the shroud at a map edge, the placed effects'
    activation, the storm clouds' drift, the rebel sea-border colour.

### 5. TDD

The same with TDD's captures: the Rauros water edge, notches and triangles; the marsh reeds (grass or not); its lighting stems and
stages (already resolved, compared now); its custom tiles and cliffs.

### 6. Thrones

`vik_attila_map` with its captures: the frame (its DLL's world transform, lens and constants checked as Attila's: H same), its
settlements and sprawl, `{bri,ire,scot}_<season>` lighting, its skybox and cubemap.

### 7. Docs

docs/re/renderer.md "Campaign frame" (Phase 1), docs/re/terry.md for the world transform and the settlements' turns, docs/formats where
a file is decoded, the ASSUMPTION / DEVIATION notes in the code updated or removed with their evidence; campaign_environment.md's and
terrain_editor.md's open lists pointed here.

## Captures needed from the user

Each one turn 1 of a new campaign unless noted, the graphics settings written down (the editor runs with the same `[graphics]`), the
player faction named, a screenshot at the user's normal settings beside it. RenderDoc 1.46, low textures, few packs (as before).

| Capture | Where | Settles |
|---|---|---|
| A (have: frames 6628, 7363) | Attila, Rome's coast, middle zoom | Phases 1 to 3, the baseline |
| C | Attila, the same place at the closest zoom | forests, settlements, sprawl, walls, coast strip, grass |
| F | Attila, the farthest zoom, tilted to the horizon | the horizon, the far ground, the lens |
| G | Attila, Germania north of the Alps | the fog of war, the shroud |
| E | Attila, a map edge | the shroud plane at the edge |
| T1, T2 | TDD, Rauros and the marshes; Minas Tirith close | Phase 5 |
| V1, V2 | Thrones, a coast middle zoom; a settlement close | Phase 6 |

The rain and winter captures (64369, 92544, 94246) are kept as extra baselines.

## Order

1, 2, 3 (the baseline), then 4 in its order (re-ranked by the baseline's errors), 5, 6; Phase 7's docs with each phase. Captures C and
F are wanted when Phase 3's baseline is ready; G, E, T and V when their items come up.
