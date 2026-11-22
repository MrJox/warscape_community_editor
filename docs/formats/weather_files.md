# Weather, lighting, VFX and shader files

The files the weather tool reads and writes, as surveyed for the weather plan (2026-10-06; docs/plans/weather.md). The attribute schema of
every element is `docs/re/xml_schema.tsv`; what each attribute drives is `docs/re/environment_to_shader.md`; the VFX runtime is
`docs/re/particles.md`; which file a battle picks is `docs/re/variation_db.md`.

## Battle environments (`weather/**/*.environment`)

Text XML, ASCII, CRLF; 488 files in `data.pack`, more in TDD's `tdd_pack6_weather`. Elements: `LIGHTING`, `POST_PROCESSING`, `FOG`,
`DEPTH_OF_FIELD`, `HDR`, `WEATHER`; `SCENE_NODE_LIST > WEATHER_SCENE_NODE > SKY_DOME > SKYGEN`; `CLOUD_PLANE_MESH > CLOUD >
CLOUD_ELEMENT` (near and far planes, about 60 cloud elements per file); `SUN_MESH`.

Value syntax: plain floats and bools, `rgb(r,g,b)`, `argb(a,r,g,b)`, `variance(a,b)`, `euler(a,b,c)`. The game writes 6-decimal floats and
`'` quotes; an unedited file must save byte for byte.

## Weather definitions (`weather/weather_definitions.xml`)

`WEATHER_DEFINITION` (wind gusts, vegetation wind, `vfx1` / `vfx2` precipitation), `WEATHER_PARTICLE_DEFINITION`,
`WEATHER_DEFINITION_REF` / `_GROUP`. An environment's `WEATHER@definition_name` names an entry here.

## Campaign and porthole lighting (`.lighting`)

The campaign set as surveyed 2026-11-16 (docs/plans/campaign_environment.md; how the game picks and blends the files: docs/re/renderer.md,
"The campaign map", "Lighting and weather"). No database table names a campaign lighting, sky or cloud file; `campaign_vfx_*` are UI /
event effects. No campaign storm system.

- `.lighting` = `SCENE_ENVIRONMENT` with `LIGHTING`, `FOG`, `POST_PROCESSING`, `DEPTH_OF_FIELD`, `HDR`, `WEATHER`: the battle blocks
  less every scene node (no sky dome, clouds, sun mesh). `WEATHER@definition_name` names `weather_definitions.xml`'s `C_*` entries,
  whose `vfx1` are the `*_camp` precipitation VFX. Campaign files use DOF (`use_DOF='true'`).
- `terrain/campaigns/<map>/environment_collection.xml`: `ENVIRONMENT_COLLECTION@global_lighting`, `ENVIRONMENT_SPHERES/SPHERE`
  (`lighting x y z inner_radius outer_radius`, in the `.terry`'s units). main_attila 11 spheres (global `europe_sp`), TDD's
  rom_third_age 9 (global `lighting/europe.lighting`, bare stems), cha 7, Thrones' vik 3. BOB writes it from the `.terry`'s
  `ECEnvironmentVolume`s (docs/re/terry.md, "Campaign maps").
- `terrain/campaigns/<map>/lighting/<climate>_<season>[N].lighting`: main_attila 4 climates (desert, england, europe, med) x 4 seasons
  x {none, 1..4}, plus `global.lighting`; TDD the same set (most byte-identical); Thrones `{bri,ire,scot}_<season>`; cha region stems.
  Also `lighting/environment_map.dds` (512², DXT1).
- Rome 2: no collection; `light_grid.xml` (`ENV_GRID width=2 height=5`, four `SEASON` blocks of 10 files) and
  `lighting/<season>/<season>_{00..04,10..14}.lighting`, looked up bilinearly.
- The skybox: `campaign_maps/<map>/display/skybox/campaign_skybox.rigid_model_v2` (an RMV2 dome, material `default_dry`,
  `campaign_skybox_diffuse.dds`) and `cubemap.dds` (1024², RGBA16F, 11 mips). Attila (main, bel, cha), TDD, Thrones; Rome 2 has no
  cubemap.
- Placed atmosphere: the kit's 310 `ECVFX` with season masks, built into `particle_emitter_marker.markers` (seasonal
  `weather_attila_campaign_clouds_*`, `campaign_mountain_cloud`, rain / snow / storm spots, lake fog, leaves, blossom...).
  `campaign_display_emitter_marker.markers` has no reader in the DLL (a leftover).
- Leftovers: `terrain/campaigns/<map>/environment/{spring,summer,autumn,winter}.lighting` (`terrain.pack`) and
  `rigidmodels/skybox/default_campaign_*.lighting` (`models.pack`) are old euler-format files (`sun_direction`, `light_colour`,
  `hdr_*`...) that nothing in the game loads; every file of the maps' `lighting/` folders has the new set. `default.environment` (a
  full battle-style sky, identical in Attila, TDD, Rome 2): no campaign reader found (a hypothesis, not checked). The weather plan's
  survey (2026-10-06) took the `environment/` files for the campaign's lighting; the campaign survey (2026-11-16) corrected it.
- Porthole / UI lighting: `porthole/**.lighting`, same format.

## No keyframes

Every `.environment` and `.lighting` is a static snapshot. Change over time comes from the engine's cross-fade between environments
(`start_lighting_environment_cross_fade`; linear on every field, docs/re/renderer.md), the wind gust curves of `weather_definitions.xml`
and the per-particle curves of the VFX (`ranged_property`).

## VFX (`vfx/*.xml`)

UTF-16 LE with BOM, CRLF (one shipped file has an odd byte count and is carried as is). `shader_library` (`fx/particle_vfx_library.hlsl`),
`emitters`, `behaviour_chains`, `effect_render_stacks`; `property`, `ranged_property` (curves), `modifier`, `texture_property`;
`vfx_references` to library effects (e.g. `library_vfx_weather_snow`). Weather VFX: `att_bel_weather_*`, `weather_vfx_*`,
`campaign_clouds_*`, `campaign_mountain_cloud`. The VFX tool (`wce_vfx_tool`, docs/plans/vfx_tool.md) edits these files.

## Shaders (`data.pack/fxc/*.fxc`)

5,443 files (DX11 and GLSL). Each is a CA header followed by a standard DXBC blob with an RDEF chunk.
- CA header: magic `0x075BCD15`, the define list (`ENABLE_SHADOWS`, `SM_5_0`, ...), the include list with hashes.
- RDEF gives constant buffer and resource names, offsets and types through `D3DReflect` (dump: `fxc_dump`, `docs/re/shaders/`).
- File names are `<defines>_<source>.hlsl_<stage>_<entry point>`, e.g. `dx11_sm_5_0_rigidstandard.hlsl_ps_ps30_gbuffer`. The include
  lists name the pipeline modules: `gBuffer`, `lighting`, `sky`, `fog`, `weather`, `wind`, `parallel_split_shadowing`, `sm5shadows`,
  `hdr_tone_mapper_shared`, `colorimetry`.

## Database tables

Battle: `battle_weather_types`, `battle_weather_effects`, `battle_sky_types`, `battle_climate_weather_descriptions`,
`battles_to_battle_sky_types_junctions`. Campaign: `seasons`, `campaign_vfx_descriptions`, `campaign_vfx_lookups`. Not edited (scope
decision 2026-10-08: only `warscape_asset_variation_db`, docs/re/variation_db.md).

## Checking the renderer's inputs

No whole-frame comparison with the game (a battle frame has too much other content, and battles cannot be started from the command line).
The game's own shaders run, so what is checked are their inputs: constant buffer values from a full memory dump of the game in a battle
(Task Manager > Create dump file; RenderDoc runs the 32-bit game out of memory unless it loads few packs at low texture settings,
docs/re/water.md), read by `scripts/re/dump_cbs.py` and compared with what the editor
computes from the same file (`wce_render` example `env_vs_dump`); render target formats, pass order and sampler states from the same dump.
