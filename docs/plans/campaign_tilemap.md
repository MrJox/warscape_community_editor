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
4. **The skybox casts its shadow**, as the game's (into every cascade), the Rauros stripes included until the cascades match.

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
- **Environment leftovers:** the skybox casts as the game's (decision 4), but lays stripes over TDD's Rauros where the game shows none
  (the editor's cascades differ from the game's there); the shroud plane reads
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
7. **Skybox shadow stripes over Rauros** (it casts, decision 4): the cascades' constants against a TDD capture at Rauros; the
   stripes go when the editor's cascade fit is the game's.
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

## Progress

**2026-11-19.**
- Phase 1 done: `scripts/re/rd_campaign_frame.py` (the capture's draws, shaders, instances, texture keys, constants by group, camera,
  key events and targets as DDS) and `wce_terrain_io`'s `campaign_frame` example (`shader_names.csv`: the shaders named by the
  game's `fxc/` files' DXBC checksums, 127 of 127 on capture A; `passes.txt`; `mask.png`: 0.10 % of capture A). Capture A read in
  about 5 minutes; its frame written up in docs/re/renderer.md, "The campaign frame".
- Phase 2's first question settled (V, capture A's instances): the tiles, cliff meshes, sea tiles and river meshes carry z x 1.1548
  (applied in the world, after a tile's turn); buildings, props, decals and units are uniform, at their places in the stretched world;
  the tree clumps are (x, y, z, angle), the angle in sixths. So the view stretches the terrain's geometry and moves the models,
  without stretching them.
- Phase 4 items settled for Attila by capture A's pass list: no coast strip (`ps30_main_coast` not drawn: item 3 closed, nothing
  to draw), no grass pass (item 4: off on Attila; TDD's reeds wait for its capture).
- The prerequisite: the settlement buildings are drawn in scripted runs once the startpos is read (a thread): 0 right after
  `terrain_settle`, 304 at Rome 240 frames later. The 2026-11-18 run took its shot too early. Fixed: `terrain_settle` on a
  campaign map waits for the settlements' and the borders' workers and builds the props (304 buildings right after it).
- Phase 2, the lens: a campaign map's lens is the game's campaign camera's: vertical fov 1.4 radians (80.2 degrees; projection [1][1]
  1.18724 in all five captures, every zoom) and near 1 world unit (`Lens::CAMPAIGN_FOV_Y`, `Viewport::set_campaign_lens`, set by
  the map view each frame); battle maps keep the battle camera's.
- No grass on campaign maps (the renderer skips the grass pass and its shadows when `scene.campaign`): capture A's frame has none.
  No visible change at capture A's camera. To check with TDD's capture: the marsh reeds (the 2026-11-16 note kept the grass for
  them; if the game shows reeds, they come from something other than a grass pass).
- **First side by side** (capture A's camera, `cam_look -50489 3562 763 -50489 -959 6763`, `capture 1920x1080` with
  `view_mode game`; `%TEMP%\wce_scratch\campcmp\run\edA*.png` against `campcmp\a6628\screen.png`), the largest differences first:
  1. the distance: the game draws full tiles (forests, the snowy Alps, coasts) to the horizon; the editor's far ring is plain
     olive ground and the view ends short of the horizon, with sea beyond;
  2. orange clumps over the land, none in the game: from `prop_models` (gone with `terrain_props off`; not the grass), most
     likely the forest clumps drawn without `rigidcampaignvegetation` (Phase 4 item 1); the game's forests are dense green clumps;
  3. the geometry: the editor's features sit lower in the frame (the z squash, Phase 2);
  4. the sky: the game has a low cloud band and haze at the horizon; the editor's placed cloud banks show as puffs higher up.
  So Phase 4 item 1 (forests) and the horizon (Phase 2's reach) come right after the stretch.
- **The stretch's design** (an inventory of every place, 2026-11-19): no single place all positions pass through. A per-scene
  stretch next to `game_units` / `terrain_units` (`RenderScene`), applied by the renderer by kind: the terrain (tile patches,
  custom meshes, sea tiles, the overlay's river meshes) and the water meshes get the full S = diag(1, 1, 1.1547) after their turn;
  models, decals, effects and lights only their position's z x 1.1547. Every LF read at world xz (the CPU ground probes, picking,
  `lf_draw`, `g_world_dimensions`, the shroud's and sea's bounds, `g_wp_world_size`, the snow masks) takes z / 1.1547 or an
  anisotropic step; the camera, the Map panel, the overlays and the gizmo convert at the view's edge (`TerryFrame`); the
  borders stop dividing by `CAMPAIGN_Z_SCALE`. Battle maps keep a stretch of 1.
- **Phase 2, the stretch (2026-11-19):** a campaign map is drawn in the game's world. `ContextRequest::z_stretch` /
  `Frame::z_stretch` (2 / sqrt 3 on campaign maps, 1 elsewhere) is part of the frame's `from_map` / `to_map`, so the tiles, cliff
  meshes, sea tiles, rivers and lakes (`water_plane` models) carry S after their turn as the game's; every other model placed through
  the frame keeps its own shape (`Frame::unstretch`: the z row of its 3 x 3 part), at its stretched place. The LF has a z step of its
  own (`LfMap::step_z`, `dz()`; its normals, ranges, lookups, the renderer's LF window and `g_world_dimensions`, the shroud's bounds);
  the LF ground rectangles and blocks keep square grids with the stretch in their world matrix; the blend boxes take it off the turn
  (`RenderScene::z_stretch`). `TerryFrame::z_stretch`: `.terry` points to the view and back (props, effects, overlays, the gizmo's
  moves, the lighting spheres' eye); the borders no longer squash the game's z. Picking, `land_near_centre` and the LF brush use the
  z step. `terrain_cam_game <eye> <forward>` puts the camera where a capture's is (capture A: eye (-50488, 3562, 881.6)).
  Checked at capture A's camera: Rome, the islands, the Tiber's mouth and the trade route within a few pixels of the game's frame
  (30 - 70 pixels low before). Battle maps: a stretch of 1 is the arithmetic as before (an LF test with a z step added).
  Seen, not yet fixed: (1) with the cells' rings now shorter north to south, dark flat shapes beside the islands' cliffs: the far
  ring's plain ground on the land LF (35 m under sea tiles there) poking out of the sea; wider rings (`terrain_view 13000 8 48 96 48`)
  remove them and show the snowy Apennines as the game does: the horizon item below; (2) `gfx view albedo` is black in an
  offscreen campaign capture; (3) a lighter vertical band in the sky at a lower pitch (one windowed shot), not checked against the
  build before the stretch.
- **The horizon (2026-11-19, measured while the user played a GPU-heavy game: relative numbers only).** The game draws its tiles'
  ground to the far plane (500 world units, about 1 500 cells). Merged blocks out to 600 cells (`terrain_view 13000 8 24 1600 600`)
  look like the game's at capture A's camera (the snowy Alps, the haze, the coasts) but cost far more than the default rings. Under
  that load: the cliff meshes' draws were the largest CPU cost (each placement's mesh its own batch, a buffer map each): now one
  upload for all their rows, 12.8 -> 2.0 ms a call; the blend pass's constants are uploaded only when they change; the blend pass's
  draws (628 materials) remain the largest cost, not yet understood (possibly texture residency, possibly the game's load). To do on
  an idle machine: measure the default and far rings, then decide the campaign defaults (or a "game's view distance" switch).
- **The orange clumps (2026-11-19):** the autumn-only `fallen_leaves` decal props, drawn in every season: props now follow their
  seasons (`ECSeasonMask` / the markers' `seasons`), as the effects did; at capture A's camera the land is green as in the game.
- **The forests' facts for item 1 (capture A):** the clump instance is (x, y, z, angle): the list's position plus 0.4 x the model's
  pivot, its byte times 60 degrees; the diffuse the season's `<base>_<season>_diffuse.dds` (spring `_sp`), the dissolve map
  `commontextures/dissolve.dds` (docs/re/terry.md, "Forests"). `campaign_frame --textures <prefix>...` names a capture's textures
  (163 of capture A's 701 under the common, campaign model and map folders). Left: the clumps through `rigidcampaignvegetation`
  (their own mesh kind, the raw Uk12 vertices, the instance's (x, y, z, angle) in `p4`).
- **Phase 4 item 1, the forests (2026-11-19):** the clumps are drawn by the game's `rigidcampaignvegetation` (G-buffer `vs30_main` +
  `ps30_gbuffer` / `ps41_gbuffer_ms`, shadows `vs30_shadow` + `ps30_shadow`): `MeshKind::CampaignVegetation` keeps their Uk12 vertices
  (16 bytes, as capture A binds them), the draw set writes the instance's (x, y, z, angle) into POSITION4's place (the position plus
  0.4 x the pivot, the turn's angle), two-sided, the season's diffuse (`vegetation_diffuse`) and `commontextures/dissolve.dds`; left
  out of the rigid shadows and the pick pass. At capture A's camera the forests stand where and as large as the game's (they were a
  third of its size). Their colour: the Terrain tool never set `scene.vegetation_season`, so the plain diffuse was drawn; now the
  view's season (spring's `_sp`, green with blossom, as capture A's).
- **Phase 4 item 6, the settlements' turns (2026-11-19, capture A):** ports turned by their anchor hex's coast rotation (9 of 9),
  the walls' connecting towers half a turn more (204 of 204), walls and gatehouses already right. Left: the main and secondary
  slots' turn (and `sprawl_edge_0`'s): not in their hexes. Traced: ports by the game's water rule (9 of 9), the land slots a draw of
  the campaign's shared random state (not reproducible): DEVIATION, the game's draw on a seed of each slot's own (docs/re/terry.md).
  `sprawl_edge_0`'s turn traced: a sprawl hex without a road shape draws its turn at the campaign's start (fn 0x10a315f0), the rest
  keep their road rotation; the same DEVIATION (a draw on the hex's own seed). The game's draws are defined by the startpos's
  `RandSeed` but not reproducible from it (docs/re/terry.md); two captures of one campaign disagree on two settlements: open.
- **Phase 3, the comparison (2026-11-19):** the editor's `cb_dump <file>` (every constant buffer as the last frame set it) and
  `scripts/campaign_compare.py` (standard library only): each buffer's game values at the draw of the pass that sets it for the frame
  (its `PASS_OF` table; 4 x 4 matrices either way round, integers read back from their bits) against the editor's; the editor's
  `capture` against the game's screen (mean difference and colour of the ground and sky, the mask left out, a heat map). The depth
  target is now saved as depth (`rd_campaign_frame.py`: it was zeros), so the sky region needs capture A read again.
  **First baseline (capture A's camera):** the camera matches to 0.1 % (the eye's z 0.2 world units off: to look at). Fixed:
  `camera_near_far` and `g_vertical_fov` were never set (every map): now from the projection, (1, 500, 1 / 499) and 1.4 as the game's.
  Left, the campaign's constants the editor sets as the battle's or not at all: the resolve's `shroud_PS` snow and rock heights
  (1.4, 0.4, 0.4, 1.4) and `ps_height_map_cb`'s far blend (`g_far_tile_distance` 0, `g_far_blend_range` (9, 37), `g_blend_contrast` 0,
  `g_lq_infield_blend_range` 100); the sea's `OceanSharedConstants` (detail normal scales / speeds 1 / 1.2, froth 1 / 1, foam anim
  (45, 46, 0.87), lowest trough 1) and `directional_light_PS.g_refraction_depth` 36 (battle 0.75); `shadowmap_PS.g_fFadeRange` 0.29
  (0.2) and `g_vSoftShadowBufferSize` 1; `hdr_to_screen_PS.pos_transform` (1, 1, 0, 0) and `focal_length` 0; the campaign's
  `weather_shared.g_wind_offset` 0. Ground: mean difference 23 of 255, the editor's colour within 1.4 of the game's brightness.
- **Constants matched (2026-11-19, `campaign_compare` at capture A):** the campaign water's `OceanSharedConstants` (froth 1 / 1,
  trough 1, detail repeats 1 / 1, speeds 1.2 / 1.2; the sea's and lakes' alike), `g_refraction_depth` 36 (light and water
  refraction), the tone map's DOF focus and focal length 0 (the files' `variance(0,18.404907)` not carried: the whole frame blurred
  by `blur_kernel_scale`, two captures), `pos_transform`, `g_vSoftShadowBufferSize`. Left: the resolve's snow / rock heights and far
  blend, the wind offset, the fade range (cascades), the foam animation's frames.
- **The horizon's mechanism (2026-11-19, capture A's resolve, event 6762):** the game's campaign resolve blends each pixel from the
  tiles' ground to a map-wide far material between 9 and 37 world units from the camera (`ps_height_map_cb.g_far_tile_distance` 0,
  `g_far_blend_range` (9, 37); the editor's 1e9 switch it off): `s_far_diffuse_map` and `s_far_normal_map`, 8192 x 8192 BC3, one
  mip, not in the packs (made at load), and `s_mask_map` (the 2032 x 1441 snow mask). Decoded (`dds_preview`;
  `%TEMP%\wce_scratch\campcmp\far\far_diffuse.png`): a colour bake of every tile's ground over the whole map (forests, mountains,
  deserts, roads; the sea blank), square over the map's 678.5 x 555.6. So the game does not draw its tiles to the horizon: past
  about 37 units its ground is the LF's geometry with this bake's colour. **Plan for the horizon (replaces "tiles to the far plane"):**
  bake the map-wide far diffuse and normal from the tiles (the blocks' bake, `Template::bake_far`, over the whole map; cached on
  disk), bind them to the resolve with the game's far-blend constants, and keep the far rings as LF ground (cheap). To trace: how
  the game bakes it (its colours against the editor's block bake), its alpha and normal channels, and the snow mask's use.
- **The far material built (2026-11-19, measured on an idle machine):** `Frame::far_map` bakes the map's far colour (each cell's
  tile's far bake, the first tile over it; else the sea bed or the climate's plain colour) and normal (the LF and the tiles' own
  heights, packed as the game's: x in alpha, up in green) at 4096 a side (DEVIATION: the game's 8192), on a thread of the streamer's
  for campaign maps (29 - 67 s on main_attila from the cache); the resolve binds it with the game's far blend (0, 9 - 37 world
  units, `g_world_dimensions` the map's size). The distant land now has its tiles' colours (was the plain olive); the constants
  match the game's; the frame's cost unchanged (5.6 ms, default rings; the merged blocks to 600 cells were 33.5 ms). Script:
  `terrain_far_wait`. Left: the far ring's geometry (its LF ground is flat where the game has mountains; it pokes out of the sea over
  sea tiles beside the islands), an edit's bake again (the bake is made once a map), a disk cache of the bake.
  Idle numbers for the record: default rings 5.8 ms (p95 7.5); merged blocks to 600 cells 33.5 ms (blend 9.7, shadows 9.5).
- **The dark flats beside the islands (2026-11-19):** a chunk's cells owned by tiles the blocks draw (`build_chunk`) were LF ground on
  the land's LF whatever the tile, sea tiles and cliffs included (the land LF 35 m out of the sea there): on a campaign map their
  points over the sea (sea tiles, cliffs, uncovered cells, as the blocks take them) now at the sea bed (`Frame::ground_with`). Left:
  green flats in the far ring there (cell (630, 683) a grass tile: likely the small island's land, drawn flat where the game has its
  trees and cliffs; (603, 686) uncovered: not explained).
- **The distant relief (2026-11-19, idle):** the far bake keeps the tiles' own heights over the LF (`FarMap::relief`, 4096 a side);
  the unpainted blocks past the rings take them (`BlockSource::relief`; `Streamer::rebuild_far` once the bake is made), so the Alps
  and the far Apennines stand at the horizon without their tiles opened. The frame 6.0 ms (5.6 without). Tried and dropped: the far
  level's per-chunk LF ground with the relief too (dark walls along chunk edges where it met tiles drawn otherwise). Left: holes
  beside some mountain tiles seen from high up (their own edges against the flat ring; not traced, may be older), the blue patch
  north-east of Rome at capture A's camera (cell (695, 680), a sea tile).
- **The holes beside the mountains (2026-11-19):** traced with `terrain_far_relief` (relief along a line of cells, and the
  placements over a cell): the bake took each cell's first placement, and a big mountain tile (`large_oval_60_r`, 36 x 23) leaves
  out of its mask the cells where grass tiles lie; its ground there slopes down (the mask's drop), so the bake sank them 256 m:
  holes near, walls far. A cell now takes the highest of the tiles' own heights over it, as `Frame::ground_at` does. The far
  level's chunks take the relief again (the walls were the same sinks). Capture A's ground 22.07 -> 21.87.
- **The blue patch / the coasts' squares (2026-11-19):** traced in capture A (docs/re/renderer.md, "The sea tiles and the coast"):
  the game draws a coast cell's `sea_coast` tile on the land's LF and its `sea` tile in a pass of its own (`ps_sea_geom`: sand, an up
  normal) on the sea's LF; the higher shows, so the shore follows the LF. The editor (1) cut a sea tile's cells out of the land's
  ground past the rings even where a coast tile shares them (`cell_on_sea`: the sea now only with no land tile there; blocks and far
  chunks alike), (2) drew the sea tiles through the terrain's blend (the coast tiles' green on the sea floor): a campaign map's sea
  tiles are `Terrain::sea_floor`, drawn by the game's `ps_sea_geom` after the resolve, the far ones with a sea tile's material
  (`Frame::offer_sea`), and (3) sampled the land LF 3/16 cell off the game's (`lf_map`: the game's texture is the field's 4 x 4
  average). The coasts seen from high up lost their cell squares (beaches along the LF). Left: capture A's nearest ground is drawn at
  the far level (the rings centre on the pivot, 37 cells ahead: the bottom of the frame 31 cells off; wider rings, `terrain_view 13000
  40 48 96 48`, give the coast tiles' sand as the game's); the bay there shows less water than the game's (the sea LF is the game's
  coarser 1016 x 720 read, not ported; the 1/16 cell left); a streaming race once left dark shadows in a capture (not reproduced).
