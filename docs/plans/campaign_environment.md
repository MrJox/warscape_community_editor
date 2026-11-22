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

The files: docs/formats/weather_files.md, "Campaign and porthole lighting". The engine (the collection's loader, season names, the
spheres' blend, the skybox, fog and shroud plane, the overlay camera, shadows, campaign-only constants, placed effects, tweakables):
docs/re/renderer.md, "The campaign map".

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
8. **Docs**: docs/formats/weather_files.md (the `lighting/` set, the collection, `light_grid.xml`, `environment_map.dds`) and
   docs/re/renderer.md, "The campaign map", done 2026-11-22. Left: docs/re/environment_to_shader.md for the campaign fields.

## Progress

As built (2026-11-16 to 2026-11-18; the detail is in the git history):

- **Units and the campaign mode:** one camera for every draw in the campaign world's units (the `.terry`'s: the map 678.5 wide);
  `RenderScene::game_units` the frame's unit (0.0026087), `RenderScene::terrain_units` 0.2 only for the tile database's repeats
  (`Renderer::tile_scale`, `Renderer::in_tile_units`); `RenderScene::campaign` (the decals' `g_is_campaign`, skipping the weather's
  snow). The older `.lighting` attributes are not read, on purpose (only the leftover files have them).
- **The cascades:** the campaign rows and pull-back 14 in the shaders' units (docs/re/renderer.md, "The campaign map", "Shadows").
  Their fit against the game's (the depth analysis) was finished under campaign_tilemap.md (2026-11-21).
- **Lighting spheres** (`wce_scene::campaign_lighting`, `wce_terrain_tool/src/lighting.rs`): the collection read, names resolved per
  season and stage as fn 0x118f6360, blended at the eye every frame the weights change (`blend_weighted`, the spheres in their order,
  the global last: Thrones' V2 decided the order, 2026-11-21, campaign_tilemap.md). The Terrain panel's weather: automatic (the
  spheres) or one file pinned (`terrain_weather`), the files and their share at the camera listed. A map without a collection takes
  `environment/<season>.lighting`, else `default.environment` (ASSUMPTION).

### Sky

`wce_terrain_tool/src/skybox.rs`: the skybox at the eye x 37.5 (plus its pivot), drawn unlit (`model_cache`'s `unlit`, the
`display/skybox/` models), fogged; `s_sky` `commontextures/default_white_cubemap.dds`, `s_environment` the map's `cubemap.dds`; no sky
or environment cube of the editor's, no engine ambient read-back (the files have their own: docs/re/renderer.md, "The campaign map").
It casts no shadow (decision of 2026-11-20). Checked against the screenshots:
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
- Phase 8's docs: docs/re/environment_to_shader.md's campaign fields.
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
