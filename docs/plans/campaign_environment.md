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
  DEVIATION, for review (the user, 2026-11-17: not settled): the skybox casts no shadow (the game's casts into every cascade;
  outside their slices). To review: draw it into the cascades as the game does and compare (stray shadows if the editor's fit differs).
  Compared (2026-11-18, the view's captures `%TEMP%\wce_scratch\ce\sky_shadow.png` / `sky_noshadow.png` over Rome, `skyt_b.png` /
  `skyt_a.png` at TDD's Rauros): casting as the game does changes nothing over Rome (mean pixel difference 0.22) but lays horizontal
  stripes of shadow over all of Rauros' ground (the editor's cascades are not the game's): kept off, for the user's decision. Checked against the screenshots:
  the cloud band and pale sky over Italy and over TDD's Argonath; Attila's sea blue.
- **Phase 4, the shroud plane (2026-11-17):** `wce_render` `renderer/shroud.rs`, after the fog: the game's grid (256 x 256 over +-500
  world units, the camera quantised by 1000 / 255), draped 1.1 units over a low-res map of the land LF (every 8th sample on Attila:
  2032 x 1441, as the game's), in the map's corner frame, drawn twice as the game draws it; `s_shroud` white (revealed), so it shows
  only over the outer 2 % of the map (the edge fade with the scrolling `shroud_cloud` layers); the land is unchanged. Not checked
  against the game (a screenshot of a map edge wanted). DEVIATION, for review: depth read only (the game's writes it). Left: the
  fog-of-war switch (a faction's `s_shroud` at the start: the game's is 1485 x 1240 A8, padded and blurred; its making to trace).
- **Phase 4, the overlay (2026-11-17; RenderDoc camp_attila, events 6768 - 15879; scripts in `%TEMP%\wce_scratch\camprd\ov`):**
  `wce_render` `renderer/campaign_overlay.rs`: the tiles' river meshes drawn flat into an RGBA16F target (1.5 x the screen, at most
  2048: ASSUMPTION from 2048 x 1620 at 1080p) cleared to (0, 0, 0, 1), colour SrcAlpha / InvSrcAlpha, alpha Zero / InvSrcAlpha, with
  `rigidriver.hlsl` `main_river` (their vertices repacked: (x, y, z, flow sign), (u, v, end fade, across 0 .. 1 from the metres off
  the centre line: ASSUMPTION, the half width the mesh's widest), `s_lf_colour` the sea colour map, `s_normal` the battle river
  normal (the capture's is its mip 2); the overlay camera the view's rotation, fov and aspect, moved back horizontally by d (near
  + d, far - d; d ASSUMED: the eye's height times the cotangent of the view's lowest ray; the game's 1.01 / 1.69 not derived). Read
  by the directional light (the G-buffer's terrain bit) and the campaign sea through `g_overlay_view_projection`; the rivers are not
  drawn as meshes on a campaign map. Checked: the Tiber at Rome; TDD's rivers are water again (lighter than the user's
  screenshot). Left: the campaign borders (`campaignborder.hlsl`, the same pass), the overlay camera's d from the game (Ghidra or a
  third capture at another zoom).
- **The lakes (2026-11-17):** a campaign map's tiles' water planes drawn with `waterplane.hlsl` `vs30_water_campaign_lake` /
  `ps30_campaign_lake` (the battle water plane's inputs, the campaign sea's pixel shader less the overlay), the sea's campaign inputs,
  transparency 30; the render origin pinned to the map's south-west corner on a campaign map (the whole map is under 700 of its units:
  no precision lost), so the lakes, the sea and the overlay read the colour map at world xz over the map's size, as the game's frame
  does. TDD's Rauros lake and marsh pools are dark water, as in the user's screenshot. Seen: a few dark triangles of water plane
  beside the pools (not checked against the game).
- Seen on master 81952bcf (not this branch's): in scripted runs Rome's settlement buildings are not drawn (their models never
  requested); this branch shows the same since its rebase onto it.
- **Borders and trade routes drawn (2026-11-17):** the data side `wce_terrain_io::attila::overlay` (docs/formats/campaign_borders.md;
  `borders_check`: 947 / 950 land curves of the capture within 4.3e-4, the trade routes within 3e-5); in the view `wce_terrain_tool`
  `lines.rs` (read on a worker for the faction picked in the Terrain panel, `borders for`; default the playable faction holding the
  most regions: ASSUMPTION, nothing marks the game's) and `wce_render` `campaign_overlay.rs` (`campaignborder.hlsl` border, a strip
  of 16 columns a curve into the overlay after the rivers; the game's z over `CAMPAIGN_Z_SCALE` into the editor's square cells).
  Checked: WRE's trade route dashed yellow in the sea west of Rome, its own border orange over the Alps. Left: the sea-region
  borders and three land curves (docs/formats/campaign_borders.md, known gaps); the trade routes' land legs (only the sea legs drawn,
  as the capture shows; the rule not found); a darker block of sea at the overlay's edge near the horizon (the overlay camera's d).
- **Phase 5, the weather VFX (2026-11-18; investigation and scripts in `%TEMP%\wce_scratch\campvfx`):** the precipitation is the
  lighting's: each campaign `.lighting` names a `WEATHER definition_name` (`C_rain gusty`, `C_snow stormy`, `dry desert`...; no db
  table), and the sphere blend (fn 0x11721160) takes the definition of the heaviest input (strict >, the first of a tie; the global's
  top-up competes), blending only its wind degrees and snowing intensity: as `blend_weighted` already does, so the view's weather
  follows the spheres (checked: rain off Britain, `england_sp` = `C_rain gusty`). ASSUMPTION: around the camera at scale 1 in the
  campaign world's units, as the battle's weather node (the particle system already runs in the shaders' units); a rainy capture
  would settle it. The placed effects: `terrain/campaigns/<map>/particle_emitter_marker.markers` (310 on main_attila: lake fog,
  waterfalls, mountain clouds, flies, blossom (spring), autumn leaves, the season's cloud banks, rain / storm / snow spots), or the
  `.terry`'s `ECVFX` through `campaign::particle_markers` (the same bytes as BOB's); drawn by `wce_terrain_tool` `prop_models.rs` as
  the props (their frame, the marker's scale: RenderDoc, the instance's scale the marker's), of the view's season, within 80 units
  of the camera's target (ASSUMPTION: the game's activation rule not traced). `campaign_display_emitter_marker.markers` is unused
  (no reader in the DLL; a leftover). DEVIATION: the snow / destruction zone flags not read. Left: the cloud banks and the rain /
  storm / snow spots are `burst_rigid` carriers spawning library effects (`property_event OnRigidSpawn`), which the particle runtime
  does not do yet, so they show nothing; their lightning (`library_campaign_stormclouds`).
- **Phase 5, the started effects (2026-11-18; Ghidra fns 0x1187aed0, 0x11863820, 0x1183fa30, 0x11895130):** `property_event`
  parsed (`VfxProps::events`); a `burst_rigid` carrier with `OnRigidSpawn` starts its `vfx_name` once per rigid born, at the
  rigid's spawn point (the ring and box offset and SpawnPosition, by the parent's scale and rotation), at scale 1 and unturned, not
  tied to the rigid (action 0; 1 attaches it, taken as 0); its emitters end at their LifeTime and their particles finish (RenderDoc:
  the children alive 33 s on, their emitters long stopped); it stays while its parent does (ASSUMPTION: the game's removal not
  traced). Checked: the spring cloud banks over the Alps north of Rome. Left: the storm and rain clouds' drift (their library
  `affected_by_wind`: the campaign's VFX wind (2, 0, 0) units / s in both captures, its direction not traced); the storm clouds'
  `cloud_heavy` (`scene_view_type` 2, SeaColourOverlay: drawn into the overlay, not supported); OnParticleSpawn (the waterfall's
  `vfx_name="3"` names no file: nothing).
- **Phase 5, settled by the user's captures (2026-11-18; `D:\renderdoc\camp_attila_rain_frame64369.rdc` turn 1 over Londinium,
  `_frame92544` winter over Britain, `_frame94246` winter over the Alps; dumps and scripts in `%TEMP%\wce_scratch\campvfx\rd2`):**
  - The rain is `C_rain gusty`'s `rain_heavy_BTL` at the eye, scale 1, in the campaign world's units (drops 0.75 x 1.4, falling
    20 - 30 a second, the 30-unit recycle box): as the editor draws it. Drops show only 5 - 20 units from the eye.
  - main_attila_map's lighting is spring all year, as decision 1 has it: the winter captures fit `england_sp` / `europe_sp` /
    `med_sp`, and it rains over snowy Britain in winter; the season changes the ground and snow masks only.
  - The spheres' weights as the editor's (3D distance, the sphere's raw y, z x 1.1547): computed 0.959 / 0.953 / 0.448 against
    the fitted 0.958 / 0.953 / 0.448. The sun colour blends linear (to the power 2.2): now `blend_weighted`'s.
  - The campaign's weather constants are 0 (`g_snowing_intensity`, `g_weather_type`, `g_season_type`, `g_wind`, `wind_VS`) and
    its VFX wind a fixed (2, 0, 0) units / s in all five captures (every main_attila file's wind_dir_degrees 14.4 would give
    (1.94, 0, 0.5)): now the renderer's on a campaign map. `affected_by_wind` effects drift with it (the storm and rain clouds).
  - Started effects outlive their parent (children started over Britain alive 34 s after the camera left): they now end when
    their emitters and particles have (ASSUMPTION: the game's removal not traced).
  - The `scene_view_type` 2 `cloud_heavy` is simulated but never drawn (each frame: the main and second particle views only):
    the editor not drawing it is the game's. The dark patch under the Alps' clouds is the fog of war (`s_shroud`'s middle level,
    in the G-buffer before lighting), not a cloud shadow (the clouds cast none).
  - `g_fog_distance_start` is the file's less `fog_precipitation_distance_modifier` rain or not (as the editor's).
  - Left: the placed effects' activation rule (the snow spots 65 units from the eye did not run); the snow / destruction zones.
    One scripted run (900 frames) ended without its shot at the time limit; three runs after it (300, 600, 900 frames) took
    37 - 42 s: not reproduced.
- **Phase 5, the placed effects' rules (2026-11-18; Ghidra fns 0x10af7dc0, 0x10b053d0, 0x10acca10; the five captures):**
  - Activation: an effect plays while in view (within 1.15 of the screen's half size, in front) and within 80 units of the eye,
    whatever its CullDistances (every playing one in all five captures; those in view from 83 out, and all behind, stopped);
    stopped, it stops emitting and its particles live on; started children keep playing (`wce_render::particles`
    `CAMPAIGN_EFFECT_REACH`). ASSUMPTION: per effect (the game's per-frame test, likely per node of its spatial tree, not found).
    `autoplay` and `use_in_outfield` are never read on the campaign.
  - Snow zone: the season's snow mask at the marker (L8, row 0 the north edge); inside at >= 128 (ASSUMPTION: no capture has a
    marker between 0 and 255); no mask: outside. Checked: the rain spot west of Britain shows in spring, not in winter
    (`snow_mask_wi` 255 there, the game's W1 capture). The destruction zone: every vanilla marker shows inside and outside it.
  - **The snow masks' names corrected:** stage 0 is `snow_mask_<season>.dds`, stage n `snow_mask_<season><n>.dds` (the weather
    node, fn 0x118f6360; RenderDoc: the game's `s_mask_map` (R8G8 2032 x 1441) equals `snow_mask_wi` in the winter capture at
    stage 0, and is empty on turn 1 in spring: Attila ships no `snow_mask_sp`). The editor used stage + 1 (`snow_mask_sp1` on
    turn 1): the spring Alps' mask snow is gone, their snowy peaks are the terrain's own. The lighting names now follow the same rule (stage 0 the season's own file, stage n `<season><n>`): TDD ships
    `europe_sp` and `europe_sp1` to `sp4` (five stages per season, the plain file unreachable with stage + 1), and both its
    startpos and Attila's start at stage 0 (`WORLD/CLIMATE`, now in the open log): TDD's turn 1 lights with `europe_sp`,
    `med_sp`, `desert_sp` (was `_sp1`); Attila unchanged.
- **The overlay camera's back-off, from the game (2026-11-18; fn 0x11729930, `overlay_camera_push` 0.7; scripts
  `%TEMP%\wce_scratch\ovre`, `camprd\ovcam.py`):** the view's rotation and fov moved back horizontally by d, near + d, far - d;
  d = 0.7 x 8 x |(ray_xz . forward_xz) / ray_y|, the ray from the eye to the near plane's bottom middle: the distance along the
  view between where that ray meets two heights 8 apart (their fields' writer not traced: the 8 fitted, 7.97 - 8.05 in the
  five captures, d 0.35 - 1.69 at pitches 31 - 72 degrees). Now `campaign_overlay.rs`'s (was a guess). The darker rectangular
  block of sea south-east of Rome is unchanged by it: another cause, open. Found (2026-11-18): it is there since before the lakes'
  shaders (11-17 captures); the game draws no lake quad there (its 55 in capture A: a lake north of Rome at (177 - 185, 334 -
  340) and a few at (210 - 213, 283 - 286); `camprd\lakes.py`) and shows open sea; under it (`terrain_pick`: cell (628, 700))
  the land LF is 32 m above the sea under sea tiles. Suspects, not checked: the sea bed texture (`ocean_sea_bed`, built from the
  scene's terrains) or the far ground's chunks (off the land LF) where the game's sea pass has its sea LF.
  **Settled (2026-11-18):** neither: it was the custom cliff tile `cliff_custom/6x4_inner_right_t1_r3` (cells (624 - 630, 699 -
  703), turned 180 degrees), an inner cliff whose mesh covers 47 % of its cells; the rest showed the cliff floor, at the battle's
  fixed depth under the sea, through the flat sea plane, where the game has land. A campaign map's cliff floor now
  (`template.rs` `custom_floor`, `mod.rs` `custom_floor_heights`): on the land LF, its points the mesh leaves bare whose nearest
  tile edge has a land tile beyond it 2 m under the LF, the rest twice the battle's depth under it. Checked against the user's
  screenshot (the cliff line with land behind it) and the west coast's cliffs (unchanged). Tried and dropped: every campaign sea
  tile and uncovered cell back on the land LF (the sand dome south of Rome rose again); the floor on the LF everywhere, at the
  mesh's nearest vertex, or by connected bare regions (land shelves in front of the west coast's cliffs).
- **Campaign maps' loading errors (2026-11-18, user):** none left on TDD and Attila: blank models (no LODs: `ap_ruin_blank_part`,
  Rome's `roman_wall_12`...) are empty, not failed; placed effects naming no effect (TDD's
  `gaul_campaign_vfx_blossom_vfx_autumn_leaves`) left out and listed once (info); the view-type-2 emitters not reported on a
  campaign map (the game draws none there). Left as warnings: the game files' textures that are not powers of two.
- Open: the fog-of-war switch (decision 2); the sea-region borders and three land curves;
  the trade routes' land legs; the skybox's shadow (for review). Done since: TDD's lakes and rivers (the lakes' shaders, the
  overlay). Not checked: the z stretch of the hex world against the game's (the editor's frame has square cells: the game's map is
  15 % longer north to south); the view's lens (the game's 80 degrees); the view streams short of the horizon.

## Decisions (user, 2026-11-16)

1. **Names resolved as the engine does**: if the captures confirm it, main_attila_map's lighting stays spring all year (its season
   picker then changes the ground and snow only). No "intended season" fallback.
2. **The shroud plane drawn, fog of war optional**: by default fully revealed (the fog and the scrolling clouds as over explored land);
   a switch shows a faction's shroud at the start (the shroud texture's source to be traced from the startpos).
   The texture's data side: [campaign_shroud.md](../formats/campaign_shroud.md) (`wce_terrain_io::attila::shroud`).
3. **Build first, captures later**: Phases 1 and 3 (no captures needed) start; the sky and fog (Phases 2 and 4) are built on the
   Ghidra reading and marked ASSUMPTION until the user's Phase 0 captures settle them.
4. **First pass**: the view on Attila and TDD (Phases 1 to 4), the weather VFX (Phase 5), Thrones and Rome 2 (Phase 7). Editing
   (Phase 6) is later.

5. **Borders and trade routes (user, 2026-11-17):** the borders coloured by diplomacy for a faction picked in the Terrain panel
   (default the campaign's first playable; relations from the startpos); their curves exactly the game's fit from `borders.pbd`'s
   hex-cell chains (traced, no approximation); the trade routes (`trade_routes.ptd`, the yellow dashed lines) drawn in the same
   overlay pass. Found so far (2026-11-17): `borders.pbd` = u16 1, u16 0x12a, the region keys, per region its neighbour segments as
   lines of (u16 col, u16 row) hex cells on the 1016 x 720 grid, a CRC32; colours by fn 0x10946810: own (180, 60, 0), at war red,
   allied (0, 90, 190), neutral 178 grey, owner 0 black; the outer texture between factions, the inner between provinces of one owner;
   half-width 0.225, start / end fades 0.75, the distance fade 95 units; the dashed lines half-width 0.05, colour (240, 200, 20).
   Data side done (2026-11-18: the height-based grid, the sea chains, the trade routes by flag; no gap left in capture A): [campaign_borders.md](../formats/campaign_borders.md).

Order: 1, 3, 2, 4, 5, 7, 6; Phase 8's docs with each phase.
