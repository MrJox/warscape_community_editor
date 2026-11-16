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
