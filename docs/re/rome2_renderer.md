# Rome 2's battle renderer: what differs from Attila's

`wce_render`'s renderer draws Rome 2 with its own shaders (`fxc/` of `data.pack` and `data_rome2.pack`; the containers' signature is
0xF8C5DEAD, Attila's 0x075BCD15, otherwise the same v4 layout). The renderer knows the game from its files
(`Files::game` -> `renderer::game::RenderGame`) and switches the passes that differ. Status 2026-10-26: a Rome 2 tile draws with
its terrain, buildings, sky, clouds, lights and shadows; the sea and the effects are not drawn.

Tools: `shader_info` (wce_d3d11 example: a game's shaders' reflection, `WCE_GAME_KEY=rome_2`), and the scratch scripts
`fxc2dxbc.py` / `disasm.sh` (vkd3d-compiler's disassembly) under `~/wce_re/scratch/rome2/scripts`.

## Shaders the editor uses

Of the 104 shader names the renderer and the effects use, 24 are missing in Rome 2: `dx11_sm_4_0_rigidterrain.*`, `vs30_geom_new`,
`vs30_shadow_new`, `ps_depth`, `rigidterrainblend` `ps30_geom` / `vs30_geom` / `ps_terrain_material`, `dx11_sm_4_0_ocean.*`, every
`particle_vfx_*`, and three of the tone mapper. The others reflect differently but the renderer binds by name: the constant buffers'
offsets change (`camera_VS_PS` is 560 bytes, Attila's 576), not their meaning, except where this file says.

## G-buffer

`rigidstandard ps30_gbuffer` writes the same four channels as Attila's (normal and gloss, sqrt albedo and mask, specular, z / w) and
not the fifth (the per-instance value); material id 0. `rigidcommon vs30_main` has no `POSITION7` / `SV_InstanceID` input and no
pixel selection texture.

## Tone mapping (`post.rs`)

The luminance statistics are in log Y: `downsample_image` (a quarter-size copy), `hdr_rgb_to_hdr_cie_log_y_xy` (into a quarter-size
target: log10 Y, x, y), `hdr_cie_log_y_xy_to_quarter_size_log_y_stats`, `downsample_quarter_size_log_y_stats` to 1x1, then the same
`determine_black_and_white_points` (its input `g_logY_values_texture_sampler`), bloom and `basic_tone_mapper`. `log_Y_stats_buffer`
holds the environment's four points as log10 (the shaders raise 10 to them). The S-curve is built from the environment's single
`low_tones_bias` / `high_tones_bias` (Attila reads the per-channel ones and ignores these).

## Lighting

`directionallight ps30_slm_highdef` adds a cloud shade texture (`cloud_shade_sampler`, `g_cloud_shade_params`), baked shadows
(`g_tBakedShadowBuffer`, `g_vBakedShadowmap*`) and selects cascades by `g_vSplitDepth` / `g_vShadowRange` instead of Attila's
`g_split_distances`; it has no `g_apply_environment_specular`. Not set by the renderer yet (zeros): to check against a dump.
`weather_shared` is 16 bytes (`g_weather_type`, `g_season_type`, `g_snow_mask`).

## Weather

Rome 2 has no `warscape_asset_variation_db`: the battle weather is chosen by folder,
`weather/<climate>/[<season>/]<land|sea>/<day|night>/<dry|fog|rain|...>/<file>.environment`. The tile tool's Weather section builds the
database from the folders. The environment schema is a subset of Attila's (no `terrain_relative_height_fog`, per-channel tones
biases, `cubemap_sun_colour_scale`, `snowing_intensity`, `wind_dir_degrees`).

## Terrain (`terrain_rome2.rs`)

No texture arrays: the tiles' maps are accumulated, then textured in screen space per texture group.

1. Surface: `vs30_geom` + `ps30_geom` (z / w into the G-buffer's depth channel, stencil 2). The vertex shader is Attila's
   `vs30_geom_new` with the LF height window placed by `lf_block_cb` instead of `g_world_dimensions` + `g_lf_uv_offset`: uv =
   (x, z) / `g_ws_lf_aabb`.zw (after + .xy), v = 1 - v, then `* g_lf_aabb.zw + g_lf_aabb.xy`, then the same with `g_normal_lf_aabb`;
   `g_height_map_dimension` moved to `lf_block_cb`. Its input signature also declares `NORMAL0` and `POSITION4`, never read.
2. Per material (a tile's maps): `vs30_blend` + `ps30_blend` over its boxes. Instance: P1 the tile's rectangle in the box's frame,
   P2 the weights of `g_blend_lf_min_max` added to y, P3 .. P5 the box's world rows, P6 the maps' rotation index. The pixel shader
   reconstructs the world position from the depth, discards outside the rectangle, and writes `blend * alpha`, `index * alpha`,
   `(normal, 1) * alpha` (the tile's normal on the LF normal, both through `lf_block_cb`) into three accumulation targets.
3. `ps30_terrain_composite`, full screen per material: uv = world xz / `g_blend_pixel_scale` (one repeat for the four groups); the
   albedo is the accumulated weights times `s_diffuse_r` .. `s_diffuse_a`, the normal the groups' normal maps (`s_normal_r` ..) on the
   accumulated one, faded to up with distance (`g_far_tile_distance`); added into the three terrain targets (albedo + alpha, normal,
   gloss 0).
4. `ps30_terrain_to_gbuffer_ss`: the terrain targets divided by the accumulated alpha, the noise map
   (`terrain/textures/common/terrain_noise`, `g_noise_params`), the climate's `medium_detail_map.dds` as a normal detail
   (`g_mid_distance_details`: far distance, repeat, strength), into the G-buffer (albedo as sqrt, specular white, z / w).

Open: no shader reads the index map (`ps30_blend` writes it to the second target, which `ps30_terrain_composite` does not sample; the
tile-space variants `ps_blend_common_ts` / `ps30_terrain_composite_ts` the same), so which of a group's four variation textures the
game binds is not known: the first. `g_blend_pixel_scale` is the material's layers' mean repeat (the game's value is not known; the
database's `RENDER_PARAMS` says 9). The game may draw the battle terrain through its virtual texture instead
(`terrain_enable_battle_vtex`, `ps30_geom_vtex`). To settle: a frame capture of the game.

Shadows: `vs30_shadow` is `vs30_geom` again; `ps30_shadow` is shared.

## Not drawn yet

- The sea: Rome 2's `dx11_sm_3_0_ocean` is a forward pass (one colour target; `reflection_texture_sampler`,
  `refraction_texture_sampler`, `combined_foam_texture_sampler`, the sky cube, `fog_VS_PS`, `ocean_PS`), not Attila's G-buffer ocean.
- The effects: Rome 2 draws them with shaders generated per effect (`vfx_<effect>_generated.hlsl`: `ps30_main`, `_high_quality`,
  `distortion`, `overlay`, `shadowmap`), a different particle system from Attila's GPU one. The particle system is made disabled.
