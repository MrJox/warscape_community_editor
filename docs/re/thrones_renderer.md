# Thrones of Britannia's battle renderer: what differs from Attila's

Total War Saga: Thrones of Britannia (`GameProfile::THRONES_OF_BRITANNIA`, key `thrones_of_britannia`) runs Attila's engine, a later
build (`empire.retail.dll` 33.5 MB, Attila's 32.2 MB; the kit's TEd, BOB and BOB processors are other builds too, with the same
feature strings: civilians, escalation, lite building zones, the alliances' indices, tile upgrades, `warscape_asset_variation_db`).
The editor draws it as Attila (`RenderGame::of` gives `Attila` for any game but Rome 2). Status 2026-10-31: battle tiles (a
settlement, a coast, a river, a custom tile) and the map context of `vik_attila_map` draw with their terrain, buildings, trees, grass,
sky, clouds, the sun and its shadows, the sea, rivers and the weather's rain and snow; nothing to check against a frame of the game
yet.

Tools: `shader_diff` (wce_d3d11 example: two games' `fxc/` side by side, by bytecode and by reflection), `shader_info`
(`WCE_GAME_KEY=thrones_of_britannia`), and `D3DDisassemble` (d3dcompiler_47) on the shaders' DXBC for the instructions. The
engine's addresses here are of this machine's `empire.retail.dll` (image base 0x10000000).

## Packs and files

PFH4 packs, as Attila's; `manifest.txt` lists every pack but `blood.pack`; no patch packs. The weather files are Attila's schema (the
`vikings/` folder is the game's own); `warscape_asset_variation_db` is there; the campaign's battle map is
`terrain/battles/vik_attila_map`. The Assembly Kit has TEd, BOB and the tile database (`_tile_database/TILES`, 314 entries) but no
tile sources: the vanilla tiles are opened from the packs (extracted into the projects, as Attila's mod tiles).

## Shaders

`shader_diff` against Attila (5093 shaders in Attila, 5121 here): 3756 the same bytecode, 804 other bytecode with the same
reflection, 505 another reflection, none missing, 28 new. Of the shaders the editor uses, all are there; six reflect otherwise:

- `hdr_to_screen` (`ps30_main_high` and its depth of field / distortion variants): `god_rays_sampler` (added: colour += 0.1 x its
  red) and `grain_tex_offset` (unused). The editor binds black to the god rays (`pass_to_screen`), as the game with them off. The
  alpha is unchanged.
- `directionallight ps30_slm_highdef` (gather 5.0): `directional_light_PS` 96 bytes (80): `g_cloud_shade_params` and
  `cloud_shade_sampler`. The shade multiplies the sun where the G-buffer's z / w is below `g_cloud_shade_params.w`, sampled at
  (the pixel projected along `sun_direction` onto y = 0) x `.z` + `.xy`: left at zero, the comparison
  never holds (z / w >= 0) and the sun is as Attila's.
- `ocean ps30_main_ultra_water_quality` (sm_4_0): the pixel shader reads the sea bed itself (`sea_bed_info_texture_sampler` at
  (xz + 3 x normal.xz + 1024) / 2048, height = red x `g_ps_sea_info.y` + `.x`, 0 outside) instead of
  `g_transparency_exponential_scale`, `g_sea_bed_light_scatter`, `g_refraction_light_scatter` (unused now). The editor already binds
  the texture and sets `g_ps_sea_info` as the vertex shader's `g_vs_sea_info`.

The rest of the 505 are shaders the editor does not use: the units' blood materials (`rigid*blood*`: `weather_shared` grows to 64
bytes with `g_rain_specular_strength` / `_glosiness`, read, and `g_rain_terrain_specular_*` / `g_rain_terrain_reflectivity`, never
read; `s_fire_map` gone), the campaign's water, the other shader models' and shadow kinds' variants of the three above. The terrain,
trees, grass, sky, clouds, decals, rivers, water planes, tone mapper and particles are Attila's shaders.

New shaders: `hdr_god_rays.hlsl` (`vs30_main`, `ps30_god_rays`), `gaussianblur ps30_gaussian_*_blur_r_channel_wide` and the
`enable_grain` variants of `hdr_to_screen`.

## Graphics options

Three options Attila lacks (`preferences.script.txt`): `gfx_grain` (film grain: `grain_texture`, `grain_tex_offset`),
`gfx_cloud_shadows` (`terrain/textures/common/cloud_shade`; `cloud_shade_battle_scale` / `_speed`, `cloud_shade_campaign_*`) and
`gfx_god_rays`. The editor draws the grain and the cloud shadows as the options ask (docs/re/graphics_options.md); the god rays
stay off (their pass's constants, `hdr_god_rays_PS`, are not mapped).

## Environment

The environment files have Attila's attributes and two of their own, read by the engine, by no shader:
`FOG.fog_distance_scale_far` and `FOG.fog_height_strength_far` (-1 in most of `weather/vikings/`, 0 in 17 files). They blend in by a factor
of the renderer (fn 0x117fc980, which writes `fog_VS_PS` each frame from the FOG node; the node's fields by index: fn 0x117c62e0, names at
0x11ee58b4):

- `g_fog_distance_scale` = `fog_distance_scale_far` >= 0 ? lerp(`fog_distance_scale`, `fog_distance_scale_far`, t) :
  `fog_distance_scale`; `g_fog_height_strength` likewise with `fog_height_strength_far`;
- t (renderer + 0x2388) moves each frame toward a target (+ 0x238c) at a rate (+ 0x37c, at most 1): t = (1 - r) t + r target.

What sets the target is not found (no instruction writes + 0x238c by that displacement; only the constructor zeroes it). The editor
draws t = 0, the near values: the game's own result for the files at -1, and for the others while the target is 0. To settle: what
moves the target (the camera's height or distance is the likely input), for instance by watching `fog_VS_PS` in the game while the
camera moves out over a tile lit by `weather/vikings/at_sp_land_day_rain.environment` (`fog_distance_scale_far` 0).
