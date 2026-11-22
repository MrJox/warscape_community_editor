# Campaign tilemap rendering: plan

The campaign map in the Terrain tool (`terrain/campaigns/<map>`) drawn as the game draws it, checked the way the tiles and battle maps are:
the game's frame and the editor's at the same camera, compared pass by pass. Written 2026-11-19; built and checked against captures
2026-11-19 to 2026-11-21 (Attila, TDD, Thrones); **paused 2026-11-21** with the open list below. It builds on
docs/plans/campaign_environment.md (lighting, sky, fog, overlay, weather) and docs/plans/terrain_editor.md Phase 8 (the campaign map's
contents), and takes over their open rendering items.

Marks: **V** verified (data, disassembly, code), **H** hypothesis, to be checked. Captures are the user's (`D:\renderdoc\camp_*.rdc`);
RenderDoc scripts run through the hidden wrapper (`wce_scratch/py/rd_hidden.sh`), never `qrenderdoc --python` directly. Extracts and
scratch scripts: `%TEMP%\wce_scratch\campcmp` (not in the repo).

## Scope

In: everything the game draws of the map itself: the ground (tiles, cliffs, roads, farmland, the far ground), the sea, lakes and rivers,
the forests, props, settlements (buildings, sprawl, walls, ports), borders and trade routes, the sky, fog, shroud and fog of war, the
placed and weather VFX. Out: the UI (masked out of the comparison), editing the campaign `.lighting` (campaign_environment.md Phase 6).
Armies and characters at the start were added on the way (Thrones' V2: banners, figures, the starting rebels).

Games: Attila's `main_attila_map` and TDD's `rom_third_age_map` first, Thrones' `vik_attila_map` after with its own captures. Rome 2 (no
cubemap, `light_grid.xml`, sprawl models missing from the packs, no aqueducts) is a later plan.

## Decisions (user, 2026-11-19)

1. **The game's world, not the editor's square frame.** The game's campaign world is the hex grid's: z is 1.1547 (2/√3) times the
   editor's (V: the terrain patches' instance matrices in capture A, z x 1.1547; `map_data.esf`'s height 556.144 = 1441 rows x 0.33391
   x 1.1547). The view draws in that world; the documents keep their own coordinates (`.terry`, cells); picking, the gizmo, the Map
   panel and the brushes convert at the view's edge.
2. **Accurate = constants and images.** At a capture's camera: every pass's shader constants equal the game's (within float noise),
   and the frame matches per region (ground, sea, forests, buildings, sky) within thresholds reviewed with the user; then the user's
   side-by-side look.
3. **Attila and TDD, then Thrones.** Rome 2 later.
4. **The skybox casts no shadow**, as the game's in effect (revised 2026-11-20 on the user's report: the game draws it into every
   cascade, but not a texel of the shadow atlas changes at its four draws in capture A; it lands outside each cascade).

## Phases

### 1. The capture reader

Built 2026-11-19: `scripts/re/rd_campaign_frame.py` (the capture's draws, shaders, instances, texture keys, constants by group,
camera, key events and targets as DDS) and `wce_terrain_io`'s `campaign_frame` example (`shader_names.csv`: shaders named by the
game's `fxc/` files' DXBC checksums, 127 of 127 on capture A; `passes.txt`; `mask.png`; `--textures <prefix>...` names a capture's
textures). Capture A's frame: docs/re/renderer.md, "The campaign frame". `scripts/re/rd_capture_editor.py` captures the editor's own
frame for the same reading.

### 2. The game's world and lens

Built 2026-11-19. Capture A's instances (V): the tiles, cliff meshes, sea tiles and river meshes carry z x 1.1547 after a tile's turn;
buildings, props, decals and units are unstretched at their places in the stretched world; tree clumps are (x, y, z, angle), the angle
in sixths. `ContextRequest::z_stretch` / `Frame::z_stretch` (2 / √3 on campaign maps, 1 elsewhere) in the frame's `from_map` /
`to_map`, `Frame::unstretch` for models, `LfMap::step_z`, `RenderScene::z_stretch`, `TerryFrame::z_stretch` at the view's edge. The
lens: `Lens::CAMPAIGN_FOV_Y` (1.4 rad), near 1, far `Lens::CAMPAIGN_FAR` 500 world units (`Viewport::set_campaign_lens`; capture T1's
projection and `camera_near_far` equal); the camera limits from `campaign_variables` (Thrones 7 - 23 units and 0.75 - 1.15 rad,
Attila 9 - 45 and 0.35 - 1.15). `terrain_cam_game <eye> <forward>` puts the camera at a capture's. Battle maps unchanged.

### 3. The comparison

Built 2026-11-19 in part: the editor's `cb_dump <file>` (every constant buffer as the last frame set it) and `scripts/campaign_compare.py`
(standard library only; game values at the draw of the pass that sets them, arrays element by element, against the editor's; the
editor's `capture` against the game's screen per region with a heat map). Not built: a wrapper script for an unattended run, a gate
(`campaign_compare_<capture>` with `WCE_CAMPAIGN_CAPTURES`) and a committed baseline summary; the comparisons so far were run by hand.

### 4. Closing the gaps

Done (details under Progress): the forests, the frame's constants, the coast strip (not drawn by the game), grass (none on a campaign
map), the LF normals, the settlements' turns (ports and towers; land slots a DEVIATION), the cascades' fit, the shroud plane's depth,
the overlay's size, the rivers, the far ground and the horizon. Left: the open list below.

### 5. TDD

Captures T0, T1, T2 read 2026-11-21: Mordor's climate, Rauros' lake and shores, the marshes' rivers and shadows fixed. Minas Tirith
not captured.

### 6. Thrones

Captures V1, V2 read 2026-11-21: the frame lines up as Attila's; factions, borders, rings, camera limits, palisades, mountains,
weather fold, banners, flags and the starting rebels fixed. Left in the open list.

### 7. Docs

docs/re/renderer.md "The campaign frame" written (Phase 1); docs/re/terry.md has the settlements, forests and walls. Left: the findings
under Progress that have no docs/re or docs/formats home yet (the far material, the LFs as the game reads them, the hex stretch, the
cascades' analysis mask, Thrones' `FACTION` record, the faction colours of standards and flags).

## Captures

Taken by the user; RenderDoc 1.46, low textures, few packs; each with a Steam screenshot. Files in `D:\renderdoc\`.

| Name | File / frame | Size | View |
|---|---|---|---|
| A | `camp_attila_frame6628.rdc`, `_frame7363.rdc` | 1920 x 1080 | Attila, Rome's coast, middle zoom (minimal graphics; only texture mips reduced) |
| rain, winter | frames 64369, 92544, 94246 | | Londinium rain; winter over Britain and the Alps (extra baselines) |
| G | `camp_attila_G_frame11596.rdc` | 1920 x 1080 | Germania north of the Alps, the fog of war |
| E, E2 | frames 12470, 13058 | 1920 x 1080 | the Atlantic edge: shroud and sea |
| A_res (Ares) | frame 4153 | 1280 x 720 | Rome's coast at A's middle zoom |
| C | frame 8325 | 1280 x 720 | Rome close |
| F1, F | frames 8961, 9207 | 1280 x 720 | far zoom over Italy, looking down; tilted to the horizon |
| T0 | `camp_tdd_mtn_frame9836.rdc` | 1280 x 720 | TDD, Mordor's mountains and Gorgoroth |
| T1 | `camp_tdd_rauros_frame4837.rdc` | 1280 x 720 | TDD, Rauros |
| T2 | `camp_tdd_marsh_frame4964.rdc` | 1280 x 720 | TDD, the marshes below Rauros |
| V1 | `camp_thrones_V1_frame15571.rdc` | | Thrones, the Llyn peninsula and Anglesey, middle zoom (player Gwynedd) |
| V2 | `camp_thrones_V1_frame16053.rdc` | | Thrones, Aberffro (walled settlement on Anglesey) close |

At 1280 x 720 the game draws flat light-blue polygons over parts of the sea (a rendering error of the game at that size): left out of
the comparisons there. Extracts: `campcmp\cap_V2`, `cap_Ares`; `py\wallmatch.py` matches the editor's wall pieces (`cities_check`) to a
capture's draws, `py\fcmatch.py` a capture's `rigid_faction_colours` to the tables, `py\rd_fcol.py` reads them at given events.

## Open (paused 2026-11-21)

Settlements and walls:
- **Wall shapes:** on three of Aberffro's cells (V2) the game has a straight `wall_14` where the editor has a `wall_12` or `wall_13`
  (the walk's mask there; one cell also gets two pieces from two walks), and Rome's walls (capture A) differ more: many of the
  editor's 13s are the game's 14s and several of its pieces have no game piece near.
- **Aberffro's north-east wall (V2):** the game shows three small flags where the editor's poles are bare, and the editor two flags
  the game has not; which pieces stand there is not traced (follows from the shapes above, or from the turn's direction).
- **The `_inverse` flip** rests on V2's Aberffro and one piece of Rome; check it on another walled city.
- **Settlement path cells:** the cells the game's path search between the slots adds (fn 0x10a4cfe0) are not decoded or drawn.
- **Flag colours on Attila:** the rule (the owner's `faction_banners` primary, secondary, tertiary, black) is V2's; Thrones'
  tertiaries are all black, so whether the third is the tertiary or black is not known: check on an Attila capture's flags.
- **The land slots' and `sprawl_edge_0`'s turns:** a draw of the campaign's shared random state, not reproducible from the startpos's
  `RandSeed` (DEVIATION: a draw on each slot's own seed; docs/re/terry.md). The farmland rule and the road cap are ASSUMPTIONS.

Characters and armies:
- **Thrones' banner offset:** the game's standard stands at (265.18, 256.38) for Anaraud at (265.20, 256.47); the editor uses
  Attila's offset (-0.40, -0.11).
- **Figures' colours:** the game permutes the three uniform colours on some meshes (V2's skinned draws); the editor gives every
  mesh the same order.
- **The starting rebels:** the army's general takes an art set by the rebels' subculture (Thrones has no rebel art sets) and no
  units; its faction is the one whose key ends `rebels_<culture>` (ASSUMPTION, by V2's colours); the force's size by difficulty
  is not shown. Any army without a legacy emblem carries `nomadic_emblem_07` (ASSUMPTION from the DLLs' string).
- **The battle unit catalog** still colours units from `factions`; the campaign's rule (`faction_uniform_colours`) is not checked
  for battles.

Environment and performance:
- **The weather fold:** the sphere-order rule rests on V2 alone (Attila's Londinium agrees either way).
- **`g_rain_specular_*` / `g_rain_terrain_*`:** 1 in V2's constants, 0 in the editor's (the campaign's rain constants).
- **The forests ahead to 240 world units:** their CPU and GPU cost not measured (wants an idle window: `perf` at captures A and F
  against `TREE_REACH` 0).
- **The far ground's colour** varying between runs (olive or pink where no tile data; parked 2026-11-07).
- **`enviro_vfx_lake_fog`** draws a cloud over Rauros where the game's T1 frame has none (a visibility rule not traced).
- **Not traced:** where the engine names the campaign cliff texture (fn 0x117ca6f0); the storm clouds' VFX wind source; why the lowest
  of overlapping lake-mesh triangles is the ground (ASSUMPTION); the cascades' near end at T1 (3.64 against 3.13).
- **Seen, not fixed:** the road a little wider than the game's (C); faint cell lines in the shallow water up close (A); a model at
  +2.8 m over A's bay; `gfx view albedo` black in an offscreen campaign capture; the far bake made at 4096 a side (DEVIATION: the
  game's 8192); edited LF normals made from the heights (DEVIATION); 94 of 135 of V1's shaders named (the Thrones fxc set).

## Progress

As built 2026-11-19 to 2026-11-21 (the step-by-step detail is in the git history, `git log -- docs/plans/campaign_tilemap.md`):

- **Frame and constants:** `camera_near_far` and `g_vertical_fov` set from the projection on every map; the campaign water's
  `OceanSharedConstants`, `g_refraction_depth` 36, the tone map's DOF focus and focal length 0, `pos_transform`,
  `g_vSoftShadowBufferSize` as the game's. The remaining differences (`g_blend_contrast`, `g_lq_infield_blend_range`, `g_snow_height`,
  `g_rock_*`, `g_foam_anim_data`, `g_wind_offset`, `g_amSoftSplit`) are read by no shader that draws them (disassembled).
- **Rings:** a campaign map's own (`Rings::campaign` in `wce_map_view`: full detail 9 world units, the tiles to 37 as the game's far
  blend, the LF ground to 128; props 64). Past the rings the blocks go by a quadtree (`Lod::Distant(n)`, only the 2 x 2 squares keep
  their custom-mesh tiles, `DISTANT_CUSTOM`); the far LF ring streams to the far plane. A chunk's level goes by the cells its tiles draw
  (`Frame::chunk_extent`).
- **The horizon and the far material (V, capture A event 6762):** the game's resolve blends from the tiles' ground to a map-wide far
  material between 9 and 37 world units (`ps_height_map_cb.g_far_tile_distance` 0, `g_far_blend_range` (9, 37)): `s_far_diffuse_map`
  and `s_far_normal_map` (8192², BC3, made at load) and `s_mask_map` (the snow mask). The editor bakes its own (`Frame::far_map`,
  `FarMap` with the tiles' relief, `Frame::far_map_over` for edited windows, `Streamer::refresh_far`; cached as `<key>.far` by
  `cache::write_far`); `terrain_far_wait`, `terrain_far_dump`, `terrain_far_relief` for scripts. Against the game's over six land boxes:
  within 2 of 255 a channel. A cell takes the highest of the tiles' own heights over it; sea-tile cells bake as sea bed.
- **The LFs as the game reads them (V):** the land texture is the field's 4 x 4 box averages from sample 0, the sea's the 2 x 2 read on
  its second mip's grid (`wce_scene::lf_derive`, `Frame::lf` / `sea_lf`, `LfMirror::with_derived`): the ground at twelve bay points
  6.8 m rms -> 0.36. The campaign sea reads its depth from the terrain's z / w before the sea pass.
- **The LF normals (V):** the terrain binds one `s_lf_normal_map`, the map's `lf_normal.dds` (BOB's DXT5 of the kit's `normal.dds`); no
  `normals/` chunk is bound. The editor reads the kit's `normal.dds`, else the packs' `lf_normal.dds` (`game_lf_normals`, mip 1).
- **Coasts and sea tiles** (docs/re/renderer.md, "The sea tiles and the coast"): a campaign map's sea tiles are `Terrain::sea_floor`
  drawn by `ps_sea_geom` after the resolve (`Frame::offer_sea` past the rings); a coast cell with a land tile is land; the points over
  the sea at the sea bed (`Frame::ground_with`); the shroud's bounds move with the patches' frame (`ShroudGpu::world`). No coast strip:
  `ps30_main_coast` is drawn in none of the captures.
- **Cliffs (V, captures A, F, T0):** every campaign cliff draw binds `terrain/textures/campaign/default/rock_b/3_su_diffuse.dds`
  whatever the climate or season; one blend box a tile (`Terrain::no_blend_box` on the floor); a bare floor point is land only away
  from the faces in a land region of 16 points or more. Cliff meshes upload all their rows at once.
- **Forests (V):** `MeshKind::CampaignVegetation` through `rigidcampaignvegetation.hlsl` (Uk12 vertices, instance (x, y, z, angle) =
  the list's position plus 0.4 x the pivot, sixths), the view's season's diffuse and `commontextures/dissolve.dds`; drawn 240 world
  units ahead of the view (`prop_models::TREE_REACH`; the game's reach 186 to 236 units, about 21 000 trees). Props follow their seasons
  (`ECSeasonMask` / the markers' `seasons`). No grass pass on a campaign map; TDD's reeds are the tiles' models.
- **Shadows:** the cascades' near was taken in scene units (every split 383 off): fixed; the game's depth analysis ported
  (`shadow_map_depth_buffer_analysis`: `ps30_downsize_store_data` to 256² R32G32, then `ps30_downsize_data` to one texel, read two
  frames later). Its `gbuffer_channel_3_sampler` holds `commontextures/no_texture.dds` (the snow normals' slot left bound) through a
  top-mip-only sampler (`States::s_clamp_point_top`), whose alpha bit 2 leaves only a few edge columns: F's splits within 0.2 %.
  Alpha-tested materials cast through `rigidcommon`'s `ps30_shadow_alpha_test`.
- **Samplers:** the overlay's lines and the rivers' `s_lf_colour` wrap 16 x anisotropic as the game's (`s_wrap_aniso`); `shroud_cloud.dds`
  read linear (the game's BC3_UNORM; capture G).
- **Lakes and custom tiles:** `terrain_heights::mesh_heights` (`wce_tile_io`) takes every mesh triangle and the lowest where they
  overlap (Rauros within 1 - 2 m); `decompile::mesh_filled` no longer sinks a height map's missing blocks to the map's minimum; Snowdon's
  plateau the highest of the mesh's long fans.
- **Climates:** the map's climate names extend from the packs' database (TDD's `wasteland`, `mordor_a`); `Frame::offer_plain` takes
  the offering grass tile's texture groups for the plain material.
- **Roads:** at the start every road is level 1 (`roads::START_LEVEL`; docs/re/terry.md, "Roads").
- **Settlements:** ports turned by their anchor hex's coast rotation (9 of 9), connecting towers half a turn more (204 of 204); the
  walls' `_inverse` side by "the edge before the turn leads into the zones"; the flags are the wall pieces' `Cloth` blocks
  (`flag1_lod01`), drawn rigid (`GpuMesh::flag`) in the owner's `faction_banners` colours (`FactionColours::flags_only`,
  `Cities::owners`); Thrones' palisades (great halls the major settlements, the abandoned rows out, no wall on water).
  `terrain_settle` waits for the settlements' and borders' workers.
- **Particles:** a fixed block of 16 emitter slots for the weather, placed instances keep their slots, a gone instance's slot keeps its
  data until its particles die (`Particles::retired`): clouds no longer fall with the rain.
- **Thrones:** its `FACTION` record has a `CAMPAIGN_EFFECT_BUNDLES` record between the id and the key (the readers took every faction
  for the rebels); the weather folds the spheres in their order (`blend_weighted`; ASSUMPTION from V2); standards and figures take
  `rigid_faction_colours` = the faction's `faction_uniform_colours` and its `faction_banners` primary (`CampaignCatalog`,
  `MeshInstance::faction_colours`); the starting rebels from the campaign scripts' `CreateStartingRebels` calls
  (`characters::scripted_rebels`).
- **Performance (relative numbers, loaded machine unless noted):** at capture A the frame 7.4 ms before the far ring, 10.1 ms with the
  quadtree; the far bake 29 s on main_attila (1.1 s from the cache); idle default rings 5.8 ms (p95 7.5).
