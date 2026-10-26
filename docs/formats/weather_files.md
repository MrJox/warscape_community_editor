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

`SCENE_ENVIRONMENT` files with the same blocks, one per season:
- `terrain/campaigns/<map>/environment/{spring,summer,autumn,winter}.lighting` (`terrain.pack`);
- `rigidmodels/skybox/default_campaign_*.lighting` (`models.pack`);
- the campaign skybox: `campaign_maps/<map>/display/skybox/` (`campaign_skybox.rigid_model_v2`, `cubemap.dds`);
- porthole / UI lighting: `porthole/**.lighting`, same format.

## No keyframes

Every `.environment` and `.lighting` is a static snapshot. Change over time comes from the engine's cross-fade between environments
(`start_lighting_environment_cross_fade`; linear on every field, docs/re/renderer.md), the wind gust curves of `weather_definitions.xml`
and the per-particle curves of the VFX (`ranged_property`).

## VFX (`vfx/*.xml`)

UTF-16 LE with BOM, CRLF (one shipped file has an odd byte count and is carried as is). `shader_library`
(`fx/particle_vfx_library.hlsl`), `emitters`, `behaviour_chains`, `effect_render_stacks`; `property`, `ranged_property` (curves),
`modifier`, `texture_property`; `vfx_references` to library effects (e.g. `library_vfx_weather_snow`). Weather VFX: `att_bel_weather_*`,
`weather_vfx_*`, `campaign_clouds_*`, `campaign_mountain_cloud`.

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
(Task Manager > Create dump file; RenderDoc crashes the game), read by `scripts/re/dump_cbs.py` and compared with what the editor
computes from the same file (`wce_render` example `env_vs_dump`); render target formats, pass order and sampler states from the same dump.
