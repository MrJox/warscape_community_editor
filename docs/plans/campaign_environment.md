# Campaign environment: plan

The campaign map view (the Terrain tool on `terrain/campaigns/<map>`) lit, fogged and skied as the game does, the counterpart of the battle
view's weathers. Started 2026-11-16 (investigation only; nothing built). Supersedes docs/plans/weather.md "Campaign mode" (Phase 3),
which points here.

Marks: **V** verified (data, disassembly, code), **H** hypothesis, to be checked (Phase 0). Addresses are RVAs of Attila's
`empire.retail.dll` (Ghidra project `attila`); scratch decompiles of this survey are in `%TEMP%\wce_scratch\campre`, data samples in
`%TEMP%\wce_scratch\campenv` (not in the repo).

## Where the editor is today (V)

- `TerrainTool::light` (`wce_terrain_tool/src/tool.rs` ~999) picks one file (the weather picker, else `environment_collection.xml`'s
  `global_lighting` with `_sp.` turned into the view's season, else `environment/<season>.lighting`, else `default.environment`, else a
  battle `weather/<climate>/day/dry` file) and hands it to `Editor::set_weather_preview`, the battle path.
- `wce_scene::environment::read_environment` reads `LIGHTING`, `FOG`, `HDR`, `POST_PROCESSING`, `DEPTH_OF_FIELD`, `WEATHER` from a
  `.lighting`; it has no `SKYGEN` / `CLOUD_PLANE_MESH` / `SUN_MESH` (no campaign file has them), so `has_sky` is false and the sky cube,
  the env cube, reflections and the fog's sky blend are black. No sun disc, flare or clouds. That is the "no sky" the view shows.
- Dropped attributes of the older `.lighting` files: `sun_direction` (euler), `light_direction`, `light_colour[_scale]`,
  `ambient_cube_scale`, `specular_scale`, `hdr_exposure`, `hdr_bloom`, `hdr_cutoff`, `volume_fog_density`, `lock_rgb_relative_tones`
  (those files fall back to sun 0 / 45).
- `scene.game_units` is never set by the Terrain tool (stays 1, or whatever the Tile tool left): the campaign is 0.2 game units a scene
  unit, so fog, DOF, shadows, the env cube's height and particles are off by 5x or more.
- Battle constants used as they are: `battlefog`, shadow pull-back 150 (campaign 14), battle cascade rows, `CLOUD_SHADE_BATTLE_*`,
  `GAME_VIEW_DISTANCE`, `SUN_DISTANCE`, the env cube at 50 over `battle_centre` (0, 0 in the Terrain tool), the battle `Lens`.
- `WEATHER@definition_name` (`C_rain gusty`, ...) spawns precipitation around the camera at battle scale.
- Already campaign-aware: the terrain resolve's `g_is_campaign` (`terrain.rs` ~1944), the campaign sea (`scene.campaign_sea`,
  `waterplane.hlsl` campaign entries, `campaign_CB`, `campaign_ocean_cb`), the snow masks (`scene.campaign_snow`), campaign trees.
  Hard-coded 0: `projected_decal_PS.g_is_campaign`, `shroud_PS.g_shroud_enabled` (`decals.rs` ~460).
- Environment volumes: parsed and edited as `.terry` entities, written byte for byte; never evaluated at render time.
- Cross-fade: `blend_environments` exists (the Weather tool's Mix); `update_lighting` clears the blend every frame.

## How the game does it

### Data (V)

- `terrain/campaigns/<map>/environment_collection.xml`: `ENVIRONMENT_COLLECTION@global_lighting`, `ENVIRONMENT_SPHERES/SPHERE`
  (`lighting x y z inner_radius outer_radius`). main_attila 11 spheres (global `europe_sp`), TDD's rom_third_age 9 (global
  `lighting/europe.lighting`, bare stems), cha 7, Thrones' vik 3.
- `terrain/campaigns/<map>/lighting/<climate>_<season>[N].lighting`: main_attila 4 climates (desert, england, europe, med) x 4 seasons
  x {none, 1..4}, plus `global.lighting`; TDD the same set (most byte-identical); Thrones `{bri,ire,scot}_<season>`; cha region stems.
  Also `lighting/environment_map.dds` (512², DXT1).
- `.lighting` = `SCENE_ENVIRONMENT` with `LIGHTING`, `FOG`, `POST_PROCESSING`, `DEPTH_OF_FIELD`, `HDR`, `WEATHER`: the battle blocks
  less every scene node (no sky dome, clouds, sun mesh). `WEATHER@definition_name` names `weather_definitions.xml`'s `C_*` entries,
  whose `vfx1` are the `*_camp` precipitation VFX. Campaign files use DOF (`use_DOF='true'`).
- Skybox: `campaign_maps/<map>/display/skybox/campaign_skybox.rigid_model_v2` (an RMV2 dome, material `default_dry`,
  `campaign_skybox_diffuse.dds`) and `cubemap.dds` (1024², RGBA16F, 11 mips). Attila (main, bel, cha), TDD, Thrones; Rome 2 has no
  cubemap.
- Placed atmosphere: the kit's 310 `ECVFX` with season masks, built into `particle_emitter_marker.markers` (seasonal
  `weather_attila_campaign_clouds_*`, `campaign_mountain_cloud`, rain / snow / storm spots, lake fog, leaves, blossom...). The
  `.markers` format is not decoded.
- `environment/{season}.lighting` and `rigidmodels/skybox/default_campaign_*.lighting`: old euler-format files; no loader found (H:
  leftovers). `default.environment` (a full battle-style sky, identical in Attila, TDD, Rome 2): no campaign reader found (H).
- Rome 2: no collection; `light_grid.xml` (`ENV_GRID width=2 height=5`, four `SEASON` blocks of 10 files) and
  `lighting/<season>/<season>_{00..04,10..14}.lighting`.
- No database table names a campaign lighting, sky or cloud file; `campaign_vfx_*` are UI / event effects. No campaign storm system.

### Engine

- Collection parser 16c5a50 / 16c5530 / 16bd080; loader 1760cf0 multiplies each sphere's z by `CAMPAIGN_Z_SCALE` = 1.1547 (2/√3) (V).
  Without a collection, `light_grid.xml` (1760e00), bilinear lookup 1791ea0 (V).
- Season names (fn 18f6360; V for the path, H for the order): per season 0..3 (`sp su au wi`) try `<stem>_<season><N>.lighting`, then
  `<stem>_<season>.lighting`, then the name as written. N = campaign+0x818, the same N as `snow_mask_<season><N>.dds` (H: the climate
  stage). Consequence (H, to confirm): main_attila's names already end in `_sp`, so `europe_sp_sp*` is never found and its lighting stays
  spring all year; TDD's and cha's bare stems do change.
- Blend (173d760, 17609c0; V): a sphere's weight w = 1 - clamp((d² - inner²) / (outer² - inner²), 0, 1); sum w·env; if Σw > 1 divide by
  Σw, if Σw < 1 add (1 - Σw)·global. Every frame, at a scene position fetched as the skybox's is (H: the camera's eye). No fade on a
  season change found (H).
- Skybox (afd250, drawn b29f60; V): the dome at the camera, scaled by `campaign_sky_scale` = 37.5; `cubemap.dds` handed to the renderer
  (vfunc 0x100, slot -1; what it binds to is H: `s_sky` / `s_environment`).
- No SKYGEN, cloud planes or sun mesh on the campaign (V: the files have none). EnvMapGen (18e00e0) has no campaign branch (whether it
  runs there: H). The sun is static per file; no time of day.
- Shroud plane (`shroud_plane.hlsl`, set up 190dd5c, extent ±500, H grid 256): a grid draped over the low-res height map following the
  quantised camera; its pixel shader is the battle fog formula (every `fog_VS_PS` field) mixed with `s_sky`, the shroud's density
  (1 - a)^6.4, and four layers of `s_shroud_cloud` scrolled by time (scales 0.04 / 0.026 / 0.01333 / 0.008667) lit by the sun. H: on the
  campaign this plane is the fog; whether `battlefog` runs too is a capture question.
- `directionallight.hlsl` has a campaign branch: `s_overlay` through `g_overlay_view_projection`, and the shroud as a light factor
  (0.05..1) when `g_shroud_enabled`.
- Campaign-only cbuffer fields: `shroud_PS` (enabled, `g_campaign_world_bounds`, `g_uv_offset`), `shroud_plane_buffer`,
  `vs_height_map_cb`, `g_overlay_view_projection`, `campaign_CB`, `campaign_ocean_cb`, `campaign_tree_ps`, `trade_route_PS`,
  `campaign_border_PS`, `g_is_campaign` = 1. Read by no shader: `g_season_type`, `g_campaign_autumn_timer`, `g_snow_height`.
- Tweakables: `shadow_cam_depth_offset_campaign` 14, `campaign_shadow_split_scale` 0.1, `shadow_split_campaign_min_distance_delta`
  0.01, `g_campaign_terrain_slope_scale_depth_bias` -2, `g_campaign_depth_bias` 0, `terrain_campaign_waterplane_scale_*` 15/15/2/2,
  `CAMPAIGN_WATER_SEA_INTENSITY_SHROUD` 0.9, `shroud_padding` 20 / `shroud_edge_padding` 128 / `shroud_half_edge_padding` 64,
  `shroud_blur_kernel_size_v2` 10, `terrain_override_season` -1. The campaign camera's near / far: not found.

## Phases

0. **Black box** (the user's captures; the questions the code does not settle):
   - A memory dump on the campaign map (main_attila, then TDD), one per season if possible: the resolved names at scene+0x22b4 (the
     spring-only quirk, N), `camera_VS_PS.camera_near_far`, `fog_VS_PS`, `lighting_VS_PS`, `shroud_PS`, `campaign_CB`,
     `campaign_ocean_cb`; read by `scripts/re/dump_cbs.py`, compared by an `env_vs_dump`-style check.
   - A RenderDoc frame of the campaign map (low settings, few packs): the pass order (battlefog or the shroud plane alone), what
     `s_sky` / `s_environment` hold (`cubemap.dds` or an EnvMapGen cube), the skybox draw's shader and blend, the shroud plane's grid
     and blend, `s_overlay`.
   - The camera at two zooms near a sphere's edge (eye or look-at); a season change at the end of a turn (a fade or not).
   **The checklist (asked of the user 2026-11-16):** RenderDoc 1.46 as for `coast.rdc` (low textures, "unlimited video memory" off,
   few packs: the 32-bit game runs out of address space). Capture A, vanilla Attila (`D:\renderdoc\camp_attila.rdc`): the main
   campaign on turn 1, the camera over a coast with the sea, a river and a settlement in view, at a middle zoom, tilted so the
   horizon and the sky show. Capture B, if TDD fits in memory (`camp_tdd.rdc`): the Rauros lake and the marshes south of it, as in
   the 2026-11-16 screenshot. Optional: Capture C, A's place at the closest zoom. A screenshot of each view at the user's normal
   settings beside it. Read from them: `camera_VS_PS` (eye, view, projection: near / far, the units) in the terrain's draw, a
   prop's, a tree's, the sea's, a lake model's and a river's; the skybox draw (shader, its world matrix, depth and blend states, its
   place in the pass order); what `t_sky` / `t_environment` hold; whether `battlefog` runs, the shroud plane's draw; `fog_VS_PS`,
   `shadowmap_PS`.
1. **The campaign frame** (no new visuals, the base everything else stands on): `scene.game_units` 0.2 for campaign maps; a campaign
   mode in the scene (`g_is_campaign` everywhere it is read, shadow pull-back 14 and the campaign split scale, the campaign tweakables
   above); the env cube's position at the camera's ground; the older `.lighting` attributes read (sun euler, light colour, the HDR and
   scale fields) as the game's reader does (that reader to be found); the battle-weather fallback removed for campaign maps.
2. **Sky**: the skybox model at the camera x 37.5 with its own material, `cubemap.dds` where Phase 0 says it binds; no SKYGEN.
   Reflections and the fog's sky blend come from it.
3. **Environment volumes**: `environment_collection.xml` read into the scene's environment (a shared reader with
   `wce_terrain_io::attila::campaign`), the names resolved per season and stage as 18f6360, the N-way blend at the camera every frame
   (`blend_environments` generalised), `CAMPAIGN_Z_SCALE` on z. Terrain panel: automatic (the game's blend) or one file pinned; the
   season and climate stage as inputs; the spheres drawn as overlays with their weight at the camera. Live: an edited volume or an
   edited `.lighting` (the Weather tool's open document) relights the view.
4. **Fog and clouds**: the campaign fog as Phase 0 shows it (the shroud plane with the shroud off, or battlefog with campaign values),
   the shroud clouds scrolled by time, `s_overlay` if it is more than the UI's.
5. **Weather VFX**: `C_*` precipitation at campaign scale around the camera; the placed VFX of the map (`.terry` `ECVFX` for kit maps,
   `particle_emitter_marker.markers` decoded for pack maps) by season, with the existing VFX runtime.
6. **Editing**: a campaign `.lighting` opened in the Weather tool previews on the open campaign map (the Inspector's sections already
   cover its blocks); the volumes' lighting picked from the map's files; File > Export writes the edited `.lighting` and the collection.
7. **Other games**: Thrones (same layout; check its DLL's blend and names), Rome 2 (`light_grid.xml` bilinear, no cubemap: what its sky
   is, from its DLL), TDD (bare stems, its own stage thresholds `rom_climate_change_phases`).
8. **Docs**: docs/formats/weather_files.md (the `lighting/` set, the collection, `light_grid.xml`, `environment_map.dds`), a "Campaign"
   section in docs/re/renderer.md with the constants above, docs/re/environment_to_shader.md for the campaign fields.

## Progress

**2026-11-16 (branch `campaign-environment`).**
- Phase 1, the campaign frame: `RenderScene::game_units` 0.2 on a campaign map, 2 on a battle map (the Terrain tool never set it: 1, or
  the Tile tool's 2); `RenderScene::campaign` (the decals' `g_is_campaign`: they skip the weather's snow, as the terrain does; the
  shadows below). Not done, on purpose: the older `.lighting` attributes (`sun_direction` euler, `light_colour`, `hdr_*`...): only
  the leftover files have them (`environment/<season>.lighting`, `rigidmodels/skybox/default_campaign_*`), which nothing in the game
  loads; every file of the maps' `lighting/` folders has the new set.
- The campaign shadows (fns 0x117665e0, 0x11784c00, 0x117329a0, 0x118debf0; V): the battle's cascade code with the table's campaign
  rows (mode * 4 + quality at dll+0x1c88aa0: (10, 30, 30, 30), (5, 15, 35, 35), (5, 10, 20, 40), (5, 10, 20, 40)), the depth
  analysis as in battle, pull-back 14; the terrain casts only at the top quality (`terrain_enable_shadows`, set only for the campaign
  at quality index 2). `campaign_shadow_split_scale`, `battle_shadow_split_scale`, `g_campaign_depth_bias` and
  `g_campaign_terrain_slope_scale_depth_bias` have no reader; `shadow_split_campaign_min_distance_delta` only gates the depth
  analysis's refresh (0.01, as the battle's). ASSUMPTION: the rows in the shaders' units (0.2 a scene unit). At 0.2 with the battle
  rows the marsh reeds threw long, blocky shadows over Rauros; with the campaign's there are none at the view's height, as in the
  user's screenshot.
- Phase 3, the lighting spheres (`wce_scene::campaign_lighting`, `wce_terrain_tool` `lighting.rs`): the collection read, every name
  resolved per season and stage (n = the climate stage + 1, as the snow masks), the spheres weighted at the camera's eye in `.terry`
  units (z by `CAMPAIGN_Z_SCALE`; y by the frame's unit: ASSUMPTION) and blended over the global lighting every frame the weights
  change; the Terrain panel lists the files and their share at the camera. TDD's `rom_third_age_map` now lights with
  `europe_sp1` / `desert_sp1` / `med_sp1` (its collection's `europe.lighting` does not exist: the view fell back to the leftover
  `environment/spring.lighting`); `main_attila_map` with `europe_sp`, `med_sp`, `desert_sp`, `england_sp` as written (spring all year,
  decision 1).
- **Units, settled by RenderDoc (2026-11-16, `D:\renderdoc\camp_attila_frame6628.rdc`, `_frame7363.rdc`, vanilla, Roma; the user's
  screenshots 325610_20261116231426 / 231459; scripts and dumps in `%TEMP%\wce_scratch\camprd`):** one camera for every draw, in the
  campaign world's units (the `.terry`'s: the map 678.5 wide; eye (207.5, 9.3, 280.1), about 37 degrees down, vertical fov 80 degrees,
  **near 1, far 500**). The tiles' scale is in their instance matrices (a terrain patch x 0.0026087 = 0.33391 / 128, y 0.013043 =
  that / 0.2, z x 1.1547: the game's world is the hex grid's, z longer than the editor's square-celled frame), and the terrain's
  repeats are passed converted (`g_blend_pixel_scale_array` 620 x 0.013043 = 8.087); its details raw (mid (1000, 1000, 3, 0.6), near
  (200, 20, 0.6, 0)). So `game_units` = the frame's unit (0.0026087) for everything and `RenderScene::terrain_units` 0.2 only converts
  the tile database's repeats (`Renderer::tile_scale`); the tiles' grass still draws in 0.2 (`Renderer::in_tile_units`; the game's
  frame has no grass pass: ASSUMPTION kept for TDD's marsh reeds, not checked). Shadows: split distances 1, 6.25, 16.8, 37.8, 79.8 =
  the campaign rows x 1.05 in those units, 4 cascades, terrain, trees, buildings and the skybox casting. Fog: `battlefog` fullscreen with
  the file's values, then the shroud plane (twice; `g_shroud_enabled` 1). Water: the sea `campaign_CB` (15, 15, 2, 2), refraction
  -0.2 / 4.1, uv scales 0.1, transparency 35 (the lake quads 30), `g_ps_sea_info` (0, 0, -3.482, 6.983); the rivers are drawn into a
  2048 x 1620 overlay that the lighting and the sea read (`g_overlay_view_projection`), not as meshes.
- **Phase 2, the sky (2026-11-16):** the skybox (`wce_terrain_tool` `skybox.rs`): `campaign_skybox.rigid_model_v2` at the eye x 37.5 (its
  pivot added: the capture's translation is the eye + (−0.91, 111.4, 0.55)), drawn in the G-buffer with `g_unlit` 1 (`GpuMesh::unlit`,
  the `display/skybox/` models), lit by nothing, fogged as the rest. `s_sky` the engine's white cube
  (`commontextures/default_white_cubemap.dds`; the capture's is 1 x 1), `s_environment` the map's `cubemap.dds` (the capture's 512,
  10 mips: the top mip dropped by the texture setting); no sky cube, no environment cube, no engine ambient read-back (161 of 163
  campaign files have their own ambient). The projection's far plane 500 world units on a campaign map (the streaming keeps its own).
  DEVIATION: the skybox casts no shadow (the game's casts into every cascade; outside their slices). Checked against the screenshots:
  the cloud band and pale sky over Italy and over TDD's Argonath; Attila's sea blue.
- Open: TDD's Rauros lake and marsh pools show their beds and its rivers are bed-coloured (they were water at 0.2); the campaign
  constants above changed nothing there, so their water takes another path (the tiles' water objects); the TDD capture crashed under
  RenderDoc. The rivers' overlay technique. The z stretch of the hex world (the editor's frame has square cells: the game's map is
  15 % longer north to south). The view's lens (the game's 80 degrees). The view streams short of the horizon.

## Decisions (user, 2026-11-16)

1. **Names resolved as the engine does**: if the captures confirm it, main_attila_map's lighting stays spring all year (its season
   picker then changes the ground and snow only). No "intended season" fallback.
2. **The shroud plane drawn, fog of war optional**: by default fully revealed (the fog and the scrolling clouds as over explored land);
   a switch shows a faction's shroud at the start (the shroud texture's source to be traced from the startpos).
3. **Build first, captures later**: Phases 1 and 3 (no captures needed) start; the sky and fog (Phases 2 and 4) are built on the
   Ghidra reading and marked ASSUMPTION until the user's Phase 0 captures settle them.
4. **First pass**: the view on Attila and TDD (Phases 1 to 4), the weather VFX (Phase 5), Thrones and Rome 2 (Phase 7). Editing
   (Phase 6) is later.

Order: 1, 3, 2, 4, 5, 7, 6; Phase 8's docs with each phase.
