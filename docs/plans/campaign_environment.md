# Campaign environment: plan

The campaign map view (the Terrain tool on `terrain/campaigns/<map>`) lit, fogged and skied as the game does, the counterpart of the battle
view's weathers. Started 2026-11-16; Phases 1 to 5 built 2026-11-16 to 2026-11-18 and checked against the user's RenderDoc captures.
Supersedes docs/plans/weather.md "Campaign mode" (Phase 3), which points here. The whole-frame comparison against the game and the
rendering gaps left (the cascades' fit, the far ground, settlements) moved to docs/plans/campaign_tilemap.md on 2026-11-19.

Marks: **V** verified (data, disassembly, code), **H** hypothesis, to be checked. Addresses are RVAs of Attila's `empire.retail.dll`
(Ghidra project `attila`); scratch decompiles and data samples of this work are in `%TEMP%\wce_scratch\{campre,campenv,camprd,campvfx}`
(not in the repo). Captures: `D:\renderdoc\camp_attila_frame6628.rdc` / `_frame7363.rdc` (capture A, Rome's coast, turn 1),
`camp_attila_rain_frame64369.rdc` (Londinium, rain), `_frame92544` / `_frame94246` (winter over Britain and the Alps).

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
  `weather_attila_campaign_clouds_*`, `campaign_mountain_cloud`, rain / snow / storm spots, lake fog, leaves, blossom...).
  `campaign_display_emitter_marker.markers` has no reader in the DLL (a leftover).
- `environment/{season}.lighting` and `rigidmodels/skybox/default_campaign_*.lighting`: old euler-format files (`sun_direction`,
  `light_colour`, `hdr_*`...) that nothing in the game loads; every file of the maps' `lighting/` folders has the new set.
  `default.environment` (a full battle-style sky, identical in Attila, TDD, Rome 2): no campaign reader found (H).
- Rome 2: no collection; `light_grid.xml` (`ENV_GRID width=2 height=5`, four `SEASON` blocks of 10 files) and
  `lighting/<season>/<season>_{00..04,10..14}.lighting`.
- No database table names a campaign lighting, sky or cloud file; `campaign_vfx_*` are UI / event effects. No campaign storm system.

### Engine

- Collection parser 16c5a50 / 16c5530 / 16bd080; loader 1760cf0 multiplies each sphere's z by `CAMPAIGN_Z_SCALE` = 1.1547 (2/√3) (V).
  Without a collection, `light_grid.xml` (1760e00), bilinear lookup 1791ea0 (V).
- Season names (fn 0x118f6360, V): per season (`sp su au wi`) stage 0 is `<stem>_<season>.lighting`, stage n
  `<stem>_<season><n>.lighting`, else the name as written; the snow masks the same (`snow_mask_<season>[n].dds`). main_attila's names
  already end in `_sp`, so its lighting stays spring all year (V, the winter captures; decision 1); TDD's and cha's bare stems change.
  Both startposes start at stage 0 (`WORLD/CLIMATE`).
- Blend (173d760, 17609c0; V): a sphere's weight w = 1 - clamp((d² - inner²) / (outer² - inner²), 0, 1) at the eye (3D distance, the
  sphere's raw y, z x 1.1547; the captures' weights reproduced to 0.001); sum w·env; if Σw > 1 divide by Σw, if Σw < 1 add
  (1 - Σw)·global. The sun colour blends linear (to the power 2.2). The weather is the heaviest input's definition (fn 0x11721160:
  strict >, the global's top-up competes), only its wind degrees and snowing intensity blended. No fade on a season change found (H).
- Skybox (afd250, drawn b29f60; V): the dome at the camera, scaled by `campaign_sky_scale` = 37.5, drawn in the G-buffer unlit
  (`rigid_config_PS.g_unlit` 1, the only such draw), fogged; `s_sky` the engine's white cube, `s_environment` the map's `cubemap.dds`.
  No SKYGEN, cloud planes or sun mesh; the sun is static per file, no time of day.
- Fog: `battlefog` fullscreen with the file's values, then the shroud plane (`shroud_plane.hlsl`, set up 190dd5c): a 256 x 256 grid
  over ±500 world units following the camera quantised by 1000 / 255, draped over the low-res land LF, drawn twice; the battle fog
  formula mixed with `s_sky`, the shroud's density (1 - a)^6.4 and four layers of `s_shroud_cloud` scrolled by time (scales 0.04 /
  0.026 / 0.01333 / 0.008667) lit by the sun.
- `directionallight.hlsl` has a campaign branch: `s_overlay` through `g_overlay_view_projection`, and the shroud as a light factor
  (0.05..1) when `g_shroud_enabled`.
- The overlay camera (fn 0x11729930, `overlay_camera_push` 0.7): the view's rotation and fov moved back horizontally by
  d = 0.7 x 8 x |(ray_xz . forward_xz) / ray_y|, the ray from the eye to the near plane's bottom middle; near + d, far - d (the 8 fitted
  from five captures, its writer not traced).
- Shadows (fns 0x117665e0, 0x11784c00, 0x117329a0, 0x118debf0; V): the battle's cascade code with the table's campaign rows (mode * 4 +
  quality at dll+0x1c88aa0: (10, 30, 30, 30), (5, 15, 35, 35), (5, 10, 20, 40), (5, 10, 20, 40)), the depth analysis as in battle,
  pull-back `shadow_cam_depth_offset_campaign` 14; the terrain casts only at the top quality. `campaign_shadow_split_scale`,
  `battle_shadow_split_scale`, `g_campaign_depth_bias` and `g_campaign_terrain_slope_scale_depth_bias` have no reader;
  `shadow_split_campaign_min_distance_delta` only gates the depth analysis's refresh.
- Campaign-only cbuffer fields: `shroud_PS` (enabled, `g_campaign_world_bounds`, `g_uv_offset`), `shroud_plane_buffer`,
  `vs_height_map_cb`, `g_overlay_view_projection`, `campaign_CB`, `campaign_ocean_cb`, `campaign_tree_ps`, `trade_route_PS`,
  `campaign_border_PS`, `g_is_campaign` = 1. Read by no shader: `g_season_type`, `g_campaign_autumn_timer`, `g_snow_height`. The
  campaign's weather constants are 0 (`g_snowing_intensity`, `g_weather_type`, `g_season_type`, `g_wind`, `wind_VS`) and its VFX wind
  a fixed (2, 0, 0) units / s in all five captures (its source not traced).
- Placed effects (fns 0x10af7dc0, 0x10b053d0, 0x10acca10): an effect plays while in view (within 1.15 of the screen's half size, in
  front) and within 80 units of the eye, whatever its CullDistances; stopped, it stops emitting and its particles live on. `autoplay`
  and `use_in_outfield` are never read on the campaign. Snow zone: the season's snow mask at the marker. `burst_rigid` carriers start
  their `property_event OnRigidSpawn` effects once per rigid born (fns 0x1187aed0, 0x11863820, 0x1183fa30, 0x11895130). The
  `scene_view_type` 2 `cloud_heavy` is simulated but never drawn.
- Tweakables: `terrain_campaign_waterplane_scale_*` 15/15/2/2, `CAMPAIGN_WATER_SEA_INTENSITY_SHROUD` 0.9, `shroud_padding` 20 /
  `shroud_edge_padding` 128 / `shroud_half_edge_padding` 64, `shroud_blur_kernel_size_v2` 10, `terrain_override_season` -1.

## Phases

0. **Black box**: the user's captures (listed above). Done 2026-11-16 to 2026-11-18.
1. **The campaign frame**: units, the campaign mode, shadows. Built.
2. **Sky**: the skybox and `cubemap.dds`. Built.
3. **Environment volumes**: the lighting spheres blended at the camera. Built; the spheres as overlays and live relighting from an
   edited volume or `.lighting` are not done.
4. **Fog and clouds**: the shroud plane, the overlay (rivers, borders, trade routes), the fog of war. Built.
5. **Weather VFX**: the precipitation and the placed effects. Built.
6. **Editing**: a campaign `.lighting` opened in the Weather tool previews on the open campaign map; the volumes' lighting picked from
   the map's files; File > Export writes the edited `.lighting` and the collection. Not started (decision 4: later).
7. **Other games**: Thrones (its lighting and weather fold checked against captures V1 / V2 on 2026-11-21, campaign_tilemap.md),
   Rome 2 (`light_grid.xml` bilinear, no cubemap: not read yet; a map without a collection takes `environment/<season>.lighting`), TDD
   (bare stems, its own stage thresholds `rom_climate_change_phases`).
8. **Docs**: docs/formats/weather_files.md (the `lighting/` set, the collection, `light_grid.xml`, `environment_map.dds`), a "Campaign"
   section in docs/re/renderer.md with the constants above, docs/re/environment_to_shader.md for the campaign fields. Not done
   (docs/re/renderer.md has "The campaign frame", the pass list, from campaign_tilemap.md's Phase 1).

## Progress

As built (2026-11-16 to 2026-11-18; the detail is in the git history):

- **Units and the campaign mode:** one camera for every draw in the campaign world's units (the `.terry`'s: the map 678.5 wide);
  `RenderScene::game_units` the frame's unit (0.0026087), `RenderScene::terrain_units` 0.2 only for the tile database's repeats
  (`Renderer::tile_scale`, `Renderer::in_tile_units`); `RenderScene::campaign` (the decals' `g_is_campaign`, skipping the weather's
  snow). The older `.lighting` attributes are not read, on purpose (only the leftover files have them).
- **The cascades:** the campaign rows and pull-back 14 in the shaders' units; capture A's splits 1, 6.25, 16.8, 37.8, 79.8 are the rows
  x 1.05. Their fit against the game's (the depth analysis) was finished under campaign_tilemap.md (2026-11-21).
- **Lighting spheres** (`wce_scene::campaign_lighting`, `wce_terrain_tool/src/lighting.rs`): the collection read, names resolved per
  season and stage as fn 0x118f6360, blended at the eye every frame the weights change (`blend_weighted`, the spheres in their order,
  the global last: Thrones' V2 decided the order, 2026-11-21, campaign_tilemap.md). The Terrain panel's weather: automatic (the
  spheres) or one file pinned (`terrain_weather`), the files and their share at the camera listed. A map without a collection takes
  `environment/<season>.lighting`, else `default.environment` (ASSUMPTION).

### Sky

`wce_terrain_tool/src/skybox.rs`: the skybox at the eye x 37.5 (plus its pivot), drawn unlit (`model_cache`'s `unlit`, the
`display/skybox/` models), fogged; `s_sky` `commontextures/default_white_cubemap.dds`, `s_environment` the map's `cubemap.dds`; no sky
or environment cube of the editor's, no engine ambient read-back (161 of 163 campaign files have their own ambient). It casts no shadow
(decision of 2026-11-20: the game's four cascade draws change no texel of capture A's shadow atlas). Checked against the screenshots:
the cloud band and pale sky over Italy and TDD's Argonath.

### Fog, overlay and fog of war

- **Shroud plane** (`wce_render/src/renderer/shroud.rs`): the game's grid, over a low-res map of the land LF (every 8th sample), drawn
  twice; it reads depth and writes none (the game's write goes to a depth copy nothing reads after it: no effect, settled 2026-11-20).
  The shroud at a map's edge matches capture E (campaign_tilemap.md).
- **Overlay** (`wce_render/src/renderer/campaign_overlay.rs`): an RGBA16F target (1.5 x the screen, at most 2048; settled 2026-11-21)
  with the tiles' own river meshes through `rigidriver.hlsl` `main_river`, then the borders and trade routes (`campaignborder.hlsl`);
  read by the directional light and the campaign sea; the overlay camera as the game's (above). The rivers are not drawn as meshes on a
  campaign map.
- **Lakes:** the tiles' water planes with `waterplane.hlsl` `vs30_water_campaign_lake` / `ps30_campaign_lake`, transparency 30; the
  render origin pinned to the map's south-west corner, so the lakes, sea and overlay read the colour map at world xz.
- **Borders and trade routes** (decision 5): `wce_terrain_io::attila::{overlay, borders, border_fit, trade_routes, diplomacy}`
  (docs/formats/campaign_borders.md; `borders_check`: capture A 950 / 950 land, 32 / 32 sea, 19 / 19 trade curves, 27 / 27 colours);
  the view's `wce_terrain_tool/src/lines.rs`, for the faction picked in the Terrain panel (`borders for`; default the playable
  faction holding the most regions: ASSUMPTION).
- **Fog of war** (decision 2): `wce_terrain_io::attila::shroud` (docs/formats/campaign_shroud.md; `shroud_check`: 97.5 % of capture A's
  texels exact), the Terrain panel's "fog of war at the start" (`terrain_fog on|off`, off by default), bound to every reader of
  `s_shroud`. Matched against capture G on 2026-11-20 (campaign_tilemap.md; `shroud_cloud.dds` is read linear, as the game's BC3_UNORM).
- A campaign cliff tile's floor (`custom_floor`, `custom_floor_heights` in `wce_tile_editor::tile_document::context`) is placed on the
  land LF where the mesh leaves land bare (the dark block of sea south-east of Rome, 2026-11-18; refined 2026-11-21 in
  campaign_tilemap.md).

### Weather VFX

- The precipitation is the lighting's `WEATHER definition_name`, drawn around the eye at scale 1 in the campaign world's units (rain
  capture: `rain_heavy_BTL`, the 30-unit recycle box, as the editor's); the campaign's VFX wind (2, 0, 0) on a campaign map.
- The placed effects: `particle_emitter_marker.markers`, or a kit map's `.terry` `ECVFX` through
  `wce_terrain_io::attila::campaign::particle_markers`; drawn by `wce_terrain_tool/src/prop_models.rs` with the props, of the view's
  season, under the game's activation rule (`wce_render::particles` `CAMPAIGN_EFFECT_REACH`, 80 units) and snow zone (inside at >= 128:
  ASSUMPTION). Started effects (`VfxProps::events`, `OnRigidSpawn`) run at the rigid's spawn point and end when their emitters and
  particles have (ASSUMPTION: the game's removal not traced). A fixed block of 16 emitter slots for the weather and kept slots for the
  placed instances (`Particles::retired`, 2026-11-20) keep particles on their own constants row.
- Loading: blank models (no LODs) are empty, placed effects naming no effect are left out and listed once, the view-type-2 emitters are
  not reported on a campaign map.

## Open

- Phase 3's leftovers: the spheres drawn as overlays with their weight; live relighting from an edited volume or `.lighting`.
- Phase 5: the placed effects' activation granularity (per effect: ASSUMPTION) and the started effects' removal; the destruction zone
  flags (every vanilla marker shows inside and outside); the source of the fixed VFX wind; OnParticleSpawn (the waterfall's
  `vfx_name="3"` names no file).
- Phase 6 (editing) and Phase 7's Rome 2 (`light_grid.xml`, its sky).
- Phase 8's docs.
- The other rendering items (cascades, far ground, settlements, `enviro_vfx_lake_fog` over Rauros): docs/plans/campaign_tilemap.md.

## Decisions (user, 2026-11-16)

1. **Names resolved as the engine does**: main_attila_map's lighting stays spring all year (its season picker changes the ground and
   snow only; confirmed by the winter captures, 2026-11-18). No "intended season" fallback.
2. **The shroud plane drawn, fog of war optional**: by default fully revealed (the fog and the scrolling clouds as over explored land);
   a switch shows a faction's shroud at the start. The texture's data side: [campaign_shroud.md](../formats/campaign_shroud.md)
   (`wce_terrain_io::attila::shroud`).
3. **Build first, captures later**: Phases 1 and 3 first; the sky and fog built on the Ghidra reading and marked ASSUMPTION until the
   captures settled them.
4. **First pass**: the view on Attila and TDD (Phases 1 to 4), the weather VFX (Phase 5), Thrones and Rome 2 (Phase 7). Editing
   (Phase 6) is later.
5. **Borders and trade routes (user, 2026-11-17):** the borders coloured by diplomacy for a faction picked in the Terrain panel
   (relations from the startpos); their curves exactly the game's fit from `borders.pbd`'s hex-cell chains (traced, no approximation);
   the trade routes (`trade_routes.ptd`, the yellow dashed lines) drawn in the same overlay pass. Format and colours:
   [campaign_borders.md](../formats/campaign_borders.md).
6. **The skybox casts no shadow** (2026-11-20, revising 2026-11-19's "it casts"): the game's cascade draws land outside every cascade.
