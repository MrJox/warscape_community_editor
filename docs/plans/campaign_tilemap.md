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
4. **The skybox casts no shadow**, as the game's in effect (revised 2026-11-20 on the user's report: the game draws it into every
   cascade, but not a texel of the shadow atlas changes at its four draws in capture A; it lands outside each cascade).

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
- **Normals:** settled 2026-11-20 (Progress, "The LF normals").
- **Settlements:** buildings unturned (the turn is not in the startpos, not found in the DLL); sprawl levels, walls around zones
  without a building, the farmland rule and the road cap are ASSUMPTIONS. Seen 2026-11-18: in scripted runs Rome's settlement buildings
  were not drawn (their models never requested).
- **Environment leftovers:** the skybox casts as the game's (decision 4), but lays stripes over TDD's Rauros where the game shows none
  (the editor's cascades differ from the game's there); the shroud plane reads
  depth but does not write it (settled 2026-11-20: the game's writes depth, but into a copy of the scene's depth, 6106, that nothing
  reads after it in capture A's frame: the particles test 6111, the UI 6150; no effect, the editor's equivalent); the overlay's size (1.5 x the screen) and the rivers' half-width are ASSUMPTIONS; the sea border's rebel
  exception, the placed effects' activation granularity, the storm clouds' drift direction and the started effects' removal are not
  traced; the fog of war and the shroud at the map's edge are not compared with the game.
- **Map:** far ground with no tile data olive in some runs and pink in others (parked 2026-11-07). TDD's Rauros water edge, notches
  and triangles: gone at T1's and T2's cameras (2026-11-21, the lake tile's heights from its mesh, Progress).

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
7. **The cascades' fit against the game's** (the skybox's stripes over Rauros showed it differs; the skybox no longer casts, decision
   4): the cascades' constants against a TDD capture at Rauros.
8. **Shroud plane depth write** (settled: no effect in the game's frame), the overlay's size (a capture at another resolution) and the rivers' half-width (their pixels in the
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

Taken 2026-11-20 (one campaign, the Western Roman Empire, turn 1; the settings above: every quality 3, textures "small"; each with a
Steam screenshot in Documents, `325610_20261120<time>_1.png`), files `D:\renderdoc\camp_attila_G_frame<N>.rdc`:

| Name | Frame | Size | View |
|---|---|---|---|
| G | 11596 | 1920 x 1080 | Germania north of the Alps, the fog of war over the north |
| E, E2 | 12470, 13058 | 1920 x 1080 | the Atlantic edge: shroud and sea |
| A_res | 4153 | 1280 x 720 | Rome's coast at capture A's middle zoom |
| C | 8325 | 1280 x 720 | Rome close (the province panel open) |
| F1, F | 8961, 9207 | 1280 x 720 | the far zoom over Italy, looking down; tilted to the horizon |
| T0 (TDD) | 9836 | 1280 x 720 | Mordor's mountains and the plains of Gorgoroth, as Mordor (`camp_tdd_mtn_frame9836.rdc`) |
| T1 (TDD) | 4837 | 1280 x 720 | Rauros, the falls and their water, as Rohan (`camp_tdd_rauros_frame4837.rdc`) |
| T2 (TDD) | 4964 | 1280 x 720 | the marshes below Rauros, as Rohan (`camp_tdd_marsh_frame4964.rdc`) |

At 1280 x 720 the game draws flat light-blue polygons over parts of the sea (the user: a rendering error at that size; north of
Rome, about Corsica, a slab in F): left out of the comparisons there. No TDD captures yet.

- **Capture G, the fog of war (2026-11-20, in progress):** the camera matches (every camera constant). The editor's fog of war
  texture is the game's (97.5 % of its texels equal, mean difference 0.5 of 255), and every constant the shroud plane reads
  (`lighting_VS_PS`, `fog_VS_PS`, `shroud_PS`, `shroud_plane_buffer`) matches at its draw (12129). Its pixel shader (disassembled):
  alpha = 2 (1 - shroud)^6.4 x min(the depth point's distance to the plane, 1)^1.35, drawn twice; the colour the clouds' luminance x
  the sun x 0.004 (HDR) blended with the fog terms; the depth `t_readable_depth_texture` the terrain passes' (6155, no model): the
  editor binds the G-buffer's (with the trees). Tried: a ground-only depth pass, and a depth read as infinitely far: neither changes
  the image (reverted). The game's fogged pixel (960, 300): its two draws (12.07, 10.80, 9.76) at alpha 0.315 each, the screen
  (171, 157, 140); the editor's (93, 89, 80), (63, 63, 39) without the fog. The tone map's constants match too; the whole frame is
  darker (11 of 255) and a forest pixel 32 against the game's 51 without fog. **The cause:** the editor read `shroud_cloud.dds` as
  sRGB, the game as `BC3_UNORM` (a RenderDoc capture of the editor, `scripts/re/rd_capture_editor.py`: the same constants, the
  same cloud texels, the alpha 0.3144 against 0.3146, the colour a third, as 0.43 decoded from sRGB is). Read linear, the fogged
  pixel's HDR is 5.7 against the game's 6.6 (2.2 before) and the auto exposure's white point 18.2 against 18.1 (`hdr_dump`, the
  editor's HDR target, against the capture's). The explored ground is not darker: its G-buffer albedo and normals match the game's
  over patches (within 2 %), and the editor's HDR and final image are 5 to 25 % brighter there (one pixel, (960, 700), lands on
  different tree instances in the two frames). What's left in G is the distance item (Phase 4's, the rings decision): the dark and
  light squares on the left, about 120 cells out, are the merged blocks' per-cell colours with no trees (props stop at 96 cells),
  where the game draws its tiles, forests and towns; they stay at rings of 600 cells. And past the far ring the editor drew
  nothing (black corners at the top), where the game has ground under its fog: fixed with capture E, below.
- **Capture E, the map edge (2026-11-20):** the shroud at the map's edge matches (HDR within a few % over the frame's thirds). The
  sea and coasts past about 190 cells were missing (black, a wall-like edge): the renderer draws a campaign map to the game's far
  plane (500 world units, `CAMPAIGN_FAR`) but the far ring followed the lens's, the battle camera's 13 000 scene units (34 world
  units). The campaign lens now has the game's far plane (`Lens::CAMPAIGN_FAR`, `Viewport::set_campaign_lens`), and the far LF
  ring streams to it: E's right third 2.54 / 3.33 / 4.90 against the game's 2.47 / 3.25 / 4.72 (1.90 / 2.43 / 3.82 before), G's
  corners filled. Its cost, measured idle (2026-11-20, `render.total`, 200 frames): capture A's camera 7.4 ms before, 50 to 105 ms
  with the far ring as blocks (731 of them, every pass and cascade), G's 8.8 to 14.2. Now the blocks past the rings go by a quadtree
  (`Lod::Distant(n)`: squares of 2^n blocks, n up to 3, once further than their side; as coarse as they are large; no shadow), and
  only the 2 x 2 squares keep their custom-mesh tiles (`DISTANT_CUSTOM`: the whole map's cliffs were most of the cost, 13 ms): A
  10.1 ms, G 11.0 ms (28 blocks and 55 squares at A). The rest is mostly the shadows (2.8 to 4.0 ms at A: the cascades reach as far
  as the depth does). Seen in G, for later: a dark sawtooth over a hill at the bottom left (also before).
- **Captures Ares, C, F1, F (2026-11-20, all 1280 x 720):** the HDR within 5 to 15 % over the frames' thirds where comparable (F1
  and F: the game's flat light-blue polygons over the sea, its 1280 x 720 error, excluded); F's horizon (mountains, haze, sky)
  matches since the far ring above. The trade routes were a pale line with faint dashes where the game's are clean dashes: the
  overlay's lines were read trilinear, the game's 16 x anisotropic (capture F, `s_border_diffuse`); from the overlay's grazing
  camera the dashes blurred into their gaps (alpha 0.09 there, against nothing drawn in the game's, and the lines' colour is lit,
  31 in HDR). Now `s_wrap_aniso` (the texture filtering setting). C's road out of Rome was `roads_stone` (grey gravel) where the
  game's is `roads`' pale track: at the start every road is level 1 (docs/re/terry.md, "Roads"; `roads::START_LEVEL`). Left, seen:
  the road a little wider than the game's; F's cliffs (the smeared cliff item); the distance item (F1's mid-distance land
  blocky).
- **The shallow coastal band (capture Ares, 2026-11-20):** the game's sea is turquoise over the sand by the coasts, ours was the
  deep sea's blue. Not the water's depth (the same 0.944 at the band; drawing the sea floor into it changed nothing) but the floor
  the water refracts: the game's sea pass writes the sea tiles' sand (0.85 / 0.73 / 0.56), ours a fifth of it, darkened by the fog
  of war. `ps_sea_geom` reads the fog at its world position over `g_campaign_world_bounds`, and the terrain patches are drawn in
  a frame moved by their LF window's shift: the fog was read some way off, where the map was unexplored. The bounds now move
  with the frame (`ShroudGpu::world`, `draw_terrain_patches`). The band shows at both coasts; the shore pixel 7.5 / 9.0 / 9.5
  against the game's 7.8 / 9.3 / 9.0; ours a little brighter further out (840, 650: 3.7 / 6.8 / 9.0 against 1.8 / 3.9 / 6.4).
- **TDD's Mordor (capture T0, 2026-11-21):** the mountains and plains drew light tan where the game's are dark ash: their placements'
  climate is index 8, TDD's `wasteland` (texture set `mordor_a`, in its campaign `tile_database.bin`), and the map's climate names came
  from the kits' databases alone (eight). Fixed: the names extend from the packs' database. The far ring's strips (24 to 40 cells)
  then still showed Mordor green: they use the plain material, and no climate's plain was ever retextured in the Terrain tool (the
  plain base's texture groups came only from the tile editor's open tile; `Frame::offer_plain` now takes the offering grass tile's).
  That tile names `grass_a` on every channel, not `climate`, so a climate whose texture set is none of its groups (`wasteland`'s
  `mordor_a`, `northern_europe`'s `grass_b`) has its set on all four. The game draws the tiles themselves out to its far blend (9 to
  37 world units). `terrain_far_relief` prints the cell's plain textures and the tile's textures, their means and its far colour.
  Seen in G meanwhile, older (cmpG7 of 2026-11-20 has it): the far blocks west of the river are a light / dark patchwork by tile
  (`grass_05` bakes 73 74 31, `grass_06` 45 47 30), where the game's forest covers it evenly. Minas Tirith not captured: a custom
  model, no sprawl.
- **TDD's marshes (capture T2, 2026-11-21):** the rivers drew pale and washed out, 3.5 x the game's in the overlay (mean of its river
  pixels (4.9, 9.5, 12.5) against (1.0, 2.6, 4.0)). Shaders, constants, textures (colour map, environment, sky, normals) and the
  camera all the same; the samplers not: `main_river` reads `s_lf_colour` at (x / width, -z / height), and the game's sampler wraps
  (16 x anisotropic) where the editor's clamped, so every river took the colour map's top row. Fixed (the normals wrap anisotropic
  and the environment wraps too, as the game's); Attila's are right now as well (capture A's river (55, 110, 138), the game's
  (53, 107, 140), before (49, 62, 93)). The game's river mesh is 5 vertices across (P1.x 0 .. width, P1.w 0 .. 1), P0.w a value of
  the river's (0.1 .. 0.8, the shader takes only its sign), P1.z the end fade; the editor's 2 across and P0.w the flow's sign give
  the same pixels. Left: the rivers end about 40 cells off and the right third is plain ground with the far relief (no pools, no
  reeds), where the game draws its tiles out to its far blend: the rings decision (the game draws 9 to 37 world units, 27 to 110
  cells; the editor's tile rings end near 40).
- **TDD's Rauros (capture T1, 2026-11-21):** straight water strips west of the lake. The lake is one tile
  (`lakes\22x32_rauros`, 22 x 31 cells) whose origin chunk lies 39 cells from the camera: drawn at that chunk's level (far, step 64)
  right under the camera, its banks sank and its water planes showed through. Fixed: a chunk's level (and a block's, a distant
  square's) goes by the cells its tiles draw (`Frame::chunk_extent`). Left: `enviro_vfx_lake_fog` draws a cloud over the lake
  where the game's frame has none (a visibility rule of the game's not traced: height, distance or season).
- **G's sawtooth (2026-11-20):** dark teeth across mountain tiles past the tile rings (`atlantic_mountains\large_point_60_r`, and a
  smaller one on the right). The far chunks draw a tile's cells as plain ground raised by the far relief (40 to 90 m there), and
  the plain material's blend box stands 30 units over the LF: what rose out of it resolved black. A ground with heights of its
  own now has a box over them, and no level of detail (two at different strides parted along their edge, as the blocks'
  comment has it). Also: a far chunk drew its tiles' cells inside a block's square, which the block draws (drawn twice).
- **F's cliffs, settled (2026-11-21, RenderDoc: captures A, F, T0 and editor captures of F's view):** three causes.
  (1) The texture: every cliff draw of the game's campaign frames (A 60, F 61, TDD's T0 8) binds
  `terrain/textures/campaign/default/rock_b/3_su_diffuse.dds` (its top mip's SHA1 the pack file's), whatever the climate or season;
  the editor drew the battle cliff texture (`0_su_default`), another rock twice as bright. Where the engine names it is not traced
  (fn 0x117ca6f0 builds it from a base passed down from 0x1185b0c0's callers). (2) The layers' weight: a campaign cliff tile gave its
  material's blend box twice (the cliff and its floor), so the ground's layers weighed 2 a box against the cliff's 10 (the game: one
  box a tile; at the same world point its layers add 2.9, the editor's 5.4); the floor now has none (`Terrain::no_blend_box`).
  (3) The beige panels: the editor's cliff floor (its invention: the game draws nothing but the mesh) stood at the land's height in
  front of the faces (the game sees the face 0.3 units further and lower there). A bare floor point is now land only when the
  covered point nearest it has only the top over it (the mesh's lowest surface there high), it is not within three points of a face,
  and its land region has at least 16 points (the slits between a face's thin bands were spikes); margins never land. The inner
  cliff south-east of Rome keeps its land; A's west coast unchanged.
- **F's smeared cliffs (2026-11-20, superseded by the above):** beige streaks down the cliff faces south-east of Rome where the game's are grey rock. In
  the game the pixel's terrain albedo is two layer draws (weight 1 each) and the cliff's own `custom_geom_mat` draw (weight 10,
  LessEqual); in the editor's the layers alone: our custom-material pass never lands there. Without its depth test it does
  (the streaks turn to rock, as the game's), so the cliff mesh covers them but something nearer holds the depth. Not the two
  passes' vertex shaders (the same instance rows and level; LessEqual instead of Equal changed nothing). Suspect: a land tile's
  ground skirt (a ring a grid step out, 20 units down) hanging over the cliff face, textured by the layers' top-down projection;
  skirts off changed nothing, but the tiles come from the disk cache, which may hold them skirted. Next: find the draw that
  writes the depth there (pixel history on the one-sample depth returned nothing; try its R32 copy).

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
- **The green flats by the islands (2026-11-19):** not the cells first suspected ((603, 686) has no tile and draws as sea): the flat by
  the island west of capture A's coast lay over `sea/8x8` tiles on land LF up to 52 m. `terrain_far_relief` now lists the cell's owner
  and placements (`Frame::cell_note`) and the scene's terrains over the point with the height each draws: a block drew those cells at
  the land's LF. The blocks' grid rows were a frame unit apart where the world matrix stretches them by the hex z (1.1547): each row
  read the map further south than it was drawn (six cells at a block's north edge, every block 15 % taller than its square). Rows now
  z_stretch apart; the blocks take a coast cell (a land tile under a sea tile) as land, as the chunks do; the far bake paints sea-tile
  cells as sea bed (it baked the climate's green where the LF under them is land).
- **The far bake after edits, and on disk (2026-11-19):** the bake runs on a window of cells too (`Frame::far_map_over`); the
  streamer's bake thread, after the whole map, waits for edited cells (`Streamer::refresh_far`: an LF stroke's `rebuild`, a refill's
  `swap_frame` over its changed chunks and the reach of their tiles), lets the edits pause half a second, bakes their window again
  (a 96 x 96-cell stroke: 1.1 s) and swaps the new material in; the Terrain tool uploads each new one and rebuilds the blocks over
  its cells. The whole map's bake is cached (`cache::write_far`, `<key>.far`, 36 MB for main_attila, the newest four kept): the key
  is every placement's tile (its folder's cache key for its look) and place, the climates, the LF's samples and the frame's numbers;
  main_attila's bake 28.7 s, read back 1.1 s.
- **The LFs as the game reads them (2026-11-20):** capture A's textures dumped (`rd_tex.py`): the land's (4064 x 2882) is exactly
  the field's 4 x 4 box averages from sample 0 (the field eight a cell); the sea's (2032 x 1441, 11 mips) its 2 x 2, read by the sea
  pass on its second mip's grid while gathering from the first (`wce_scene::lf_derive`). Read so, the game's terrain depth at
  twelve bay points is reproduced to 0.1 m and six sea-pass depths to 0.4 m. A campaign frame now draws views derived so
  (`Frame::lf`, `sea_lf`) and keeps the fields on their own lattices for picking and painting (`lf_edit`, `sea_lf_edit`); the LF
  mirrors make the views again over each stroke (`LfMirror::with_derived`). The editor's ground at the bay points: 6.8 m rms -> 0.36.
  The campaign sea reads its depth from the terrain's z / w taken before the sea pass, as the game's (its depth input is the terrain
  passes' target). Capture A's ground 21.96 -> 20.42 (wide rings).
  Left: at the bottom of capture A's frame the editor draws something at +2.8 m on the game's ray over the bay where no terrain is
  (not a tile: a model, likely; the close view of the bay matches the LF, its shore where the heights put it); faint cell lines in
  the shallow water of the close view; a cliff's texture smeared beside it.
- **The rings at the game's camera (2026-11-20, measured, not decided):** at capture A's camera the default rings (8 / 24 / 24 / 64
  around the pivot, 37 cells ahead) leave the frame's bottom (31 cells off) at the far level; `terrain_view 13000 40 48 96 48` gives
  it the coast tiles' sand as the game's, with 100 near chunks instead of 8: the terrain's surface pass 1.2 -> 6.1 ms, the frame's
  p95 9.8 -> 11.9 ms (the machine loaded: the sky / light / fog pass alone went 2.9 .. 7.9 ms between runs; relative numbers).
  Scripts measuring a campaign frame wait for the far bake first (`terrain_far_wait`): its first bake after a key change ran inside
  the window (p95 75 ms). Not checked: a battle map after the far level's sea cut (the port map tried draws under a weather layer).
- **Clouds falling like the rain (2026-11-20, the user's report with the TDD packs):** a particle keeps the emitter instance slot it
  was born with, and the slot names its constants row and position; the placed effects' slots followed the weather's emitters and
  the order of the live instances, so a weather change (a rain sphere at the camera: 0, 1 or 2 emitters) or an effect coming or
  going moved them under the living particles: clouds took the rain's row (its 30-unit camera box) or another instance's place,
  fell through the ground and came back on top, or vanished early. The weather now has a fixed block (16), every placed instance
  keeps its slot, and a gone instance's slot keeps its data until its particles have died (`Particles::retired`). Checked: the
  clouds west of Rome live their 35 - 50 s through effects coming and going (one vanished at 48 s before). Not reproduced: the
  user's exact view (their weather and season not in the settings).
- **The cascades' splits (2026-11-20):** the camera's near was taken in scene units as the rows' game units: on a campaign map (383
  scene units a game unit) every split sat 383 units off the eye (the editor's 383, 388, 398, 418, 458 against capture A's 1, 6.25,
  16.76, 37.77, 79.80), and nothing near the camera had a shadow. Now 1, 6, 16, 36, 76 in a scripted run (no depth analysis there:
  the game's range is the row's end times 1.05). NOTE: capture A was taken at minimal graphics (the user, 2026-11-20): its rows,
  atlas size, texture mips, far-blend distances and LOD may be the lowest level's; the campaign rows by quality are to be checked in
  the game's table before matching more of it. The game's split start (the near plane, with the nearest ground 7 units off) not
  ported: the editor's analysis starts at the nearest depth.
- **The LF normals (2026-11-20):** capture A's terrain blend draws all bind one `s_lf_normal_map`, named by its bytes
  `terrain/campaigns/main_attila_map/lf_normal.dds` (8128 x 5764 DXT5, 13 mips; the capture's 1016 x 720 its fourth mip: the user's
  texture quality "small" drops three); no chunk of `normals/` is bound. It is BOB's DXT5 of the kit's `normal.dds` (BGRA8, R and G
  the normal's x and z, B up), stored R 255, G z, B 0, A x: the editor's own LF normal layout. The editor drew normals made from the
  LF's slopes; a campaign frame now reads the map's (`game_lf_normals`: the kit's `normal.dds`, else the packs' `lf_normal.dds`, at
  mip 1, two texels a cell). Their components follow the LF's slopes by 0.64 / 0.65 (the rest the authored detail). Capture A's
  ground 19.62 -> 18.68. DEVIATION: an LF stroke's texels are made from the heights again. The game's preferences file (written
  2026-11-19) has every quality at 3 but textures at 0: the captures' settings (the user, 2026-11-20): only their texture mips are
  reduced; the other quality-scaled constants in them are the max level's.
- **Constants, arrays included (2026-11-20):** `campaign_compare.py` skipped array variables (the cascades' splits and matrices among
  them, which is how the 383-unit splits went unseen); it now compares them element by element (the editor's dump puts each on a
  16-byte boundary). Capture A against the editor: the cascades' matrices within 1 - 4 % (the scripted run lacks the depth
  analysis's 1.05 stretch). The other differences are read by no shader that draws them (disassembled at their events): the
  resolve and the blend pass read none of `g_blend_contrast`, `g_lq_infield_blend_range`, `g_snow_height`, `g_rock_*`; the campaign
  water no `g_foam_anim_data`; the buildings `g_wind` (0 on a campaign map, as the game's), not `g_wind_offset`; the light pass no
  `g_amSoftSplit`. Left as they are.
- **Rauros' west shore and the marshes' squares (2026-11-21, captures T1 and T2):** (1) a 36 m trench along the lake tile's west
  edge (`lakes/22x32_rauros`): the editor's ground from the game's camera, ray by ray, matched capture T1's world points (75.2 m a
  game unit) everywhere but there, where the game's is the LF's (226.7 m, the LF 230). The tile's height map lacks 10864 of its 10912
  blocks and its mesh leaves the strip out; `decompile::mesh_filled` gave those samples the missing blocks' reading, the map's
  minimum (the lake's bed). They are now at the LF (0 m over it); the neighbour trees' folder `_6`. (2) Square shadows over the
  marshes: the editor cast every rigid mesh opaque; the game casts an alpha-tested material's (`swamp_plant`, a flat quad of lily
  pads, `swamp_flower`, `swamp_reed`) with `rigidcommon`'s `ps30_shadow_alpha_test` and its diffuse (discard under 0.5 alpha), as
  the editor now does.
- **Rauros' lake from its mesh (2026-11-21, capture T1):** the trench fix put the samples the mesh "left out" at the LF, and a stair of
  cells stood at the LF south of the lake (the game's ground there 200 m, the lake's shallow bed). The mesh leaves nothing out: 943
  long triangles fanning from a few vertices were skipped as slivers; over the lake they lie at the land's height, alone they cover
  the west strip and the stair. `terrain_heights::mesh_heights` now takes every triangle and the lowest where they overlap: the
  game's ground within 1 - 2 m at the stair and the west strip (8 m on the steep shore). Winding does not separate them (both
  windings cover the tile: the mesh has an underside). ASSUMPTION: why the lowest (the game's height texture made with a less-than
  depth test) is not traced. Neighbour trees `_7`.
