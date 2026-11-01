# Rome 2's battle renderer: what differs from Attila's

`wce_render`'s renderer draws Rome 2 with its own shaders (`fxc/` of `data.pack` and `data_rome2.pack`; the containers' signature is
0xF8C5DEAD, Attila's 0x075BCD15, otherwise the same v4 layout). The renderer knows the game from its files
(`Files::game` -> `renderer::game::RenderGame`) and switches the passes that differ. Status 2026-10-26: a Rome 2 tile draws with
its terrain, buildings, sky, clouds, the sun and its shadows, the sea, rivers and water planes, trees, grass and decals;
the effects are not drawn.

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

Burning: the packs have `rigidstandardfire` only. Attila's fire variants of the other materials (`rigidtileddirtmapfiremesh`,
`rigidwithdirtburnmesh`, `rigidambientmapfiremesh`) are not there, so the editor draws burning dirt, tiled dirt and ambient map meshes with
their plain shaders (`RenderGame::has_fire_shaders`). What the game draws them with is not known.

## Tone mapping (`post.rs`)

The luminance statistics are in log Y: `downsample_image` (a quarter-size copy), `hdr_rgb_to_hdr_cie_log_y_xy` (into a quarter-size
target: log10 Y, x, y), `hdr_cie_log_y_xy_to_quarter_size_log_y_stats`, `downsample_quarter_size_log_y_stats` to 1x1, then the same
`determine_black_and_white_points` (its input `g_logY_values_texture_sampler`), bloom and `basic_tone_mapper`. `log_Y_stats_buffer`
holds the environment's four points as log10 (the shaders raise 10 to them). The S-curve is built from the environment's single
`low_tones_bias` / `high_tones_bias` (Attila reads the per-channel ones and ignores these).

## Lighting

`directionallight ps30_slm_highdef` (`shadowmap_PS` 688 bytes) differs from Attila's:

- cascades: within `g_vShadowRange.x` of view depth (game units), the cascade is the number of `g_vSplitDepth`'s four depths the
  pixel is past (Attila: `g_split_distances`, with atlas bounds), blended into the next over `g_fFadeRange.x` and faded out over `.y`
  before the last split. The lookup divides only z by w: the cascade matrices must be orthographic (Attila's LiSPSM fit put every
  lookup outside the atlas: no shadows at all);
- the cascades (ShadowMapView, fn 0x114765e0; fit fn 0x11423af0; atlas matrices fn 0x1141f840; constants fn 0x1147aa90): no depth
  analysis, splits from the camera's near plus the battle row of lengths (shadow qualities 4-5: 15, 35, 90, 200; 1-3: two or three
  cascades, 30, 120 / 15, 40, 120), no overlap. Light basis x = (L.z, 0, -L.x), y = L x x (`shadowmap_view_quantize`). The box is a
  square as wide as the slice's longest corner to corner distance (rounded up, plus two texels), its centre snapped to the tile's
  texels; depth the slice's (at least `shadowmap_min_depth` 1024) ending at its far side, the near clip 1 closer. The atlas matrix:
  x * 0.5 tile / W + (x0 + 0.5) / W + that, y flipped likewise. `g_vShadowRange` 360 (`shadow_farz`), `g_fFadeRange` (3, half the
  third split), `g_sample_bias` 0.1 (battle; 0 campaign), the casters' DepthBias 1 and slope 2. The game's atlas at qualities 4-5 is
  4096 (2048 tiles); ours is 2048 (1024 tiles);
- baked shadows (`g_tBakedShadowBuffer`; `g_vBakedShadowmapScale` / `Offset` / `Shear` / `Params`): used beyond
  `g_vBakedShadowmapParams.y` of distance from the camera. Left at zero, every pixel took them (the map unbound: in shadow, the
  sun gone): the distance is set out of reach;
- a cloud shade texture (`cloud_shade_sampler`, `g_cloud_shade_params`; `terrain/textures/common/cloud_shade` in the engine's
  strings): off (`.w` 0);
- no `g_apply_environment_specular`.

To check against a frame of the game: the baked map's use, the cloud shade.
`weather_shared` is 16 bytes (`g_weather_type`, `g_season_type`, `g_snow_mask`).

## Weather

Rome 2 has no `warscape_asset_variation_db`: the battle weather is chosen by folder (fn 0x114ecc90): the game lists
`weather/<climate>/<season>/land/<day|night>/<precipitation>/*.environment` with the battle's season (`default` when it has none), only
when that is empty the `default` season's, last `weather/default/default/land/...`, and picks one at random. The `default/` files are
heavily graded (brightness up to 1.45, blue contrast 0.6) where the season folders are near neutral: a summer battle is lit by `su/`.
The paths without a season level (`weather/<climate>/land/...`) are never built. The tile tool's Weather section builds the
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
3. The composite, full screen per material. The game's DX11 one is `dx11_sm_5_0_rigidterrain` `ps50_terrain_composite` (with
   `vs30_terrain_composite` of the same set); `ps30_terrain_composite` is the low-quality fallback (one texture per group,
   `s_diffuse_r` .. `s_diffuse_a`, its first variation; one repeat `g_blend_pixel_scale`; gloss 0). ps50 reads `t_diffuse_array` /
   `t_normal_array` (the game's hold every group of the climate, four layers each) and the accumulated index map: per channel c, the
   layer is `index * 3 + g_channel_offsets[c]` (the group's first layer), the two nearest layers blended by its fraction, unless
   `g_channel_swizzle[c]` is 2 .. 5 (variation 0 .. 3, fixed); uv = world xz / `g_blend_pixel_scale_array[g_channel_offsets[c] / 4].x`
   (a repeat per group); the gloss target gets `g_light_params_array[layer].xyz` (const reflectivity, lerp reflectivity, const
   smoothness) mixed the same way. `ps_texture_array_cb` is 400 + 400 float4. The renderer gives each material its own 16-layer
   arrays (offsets 0, 4, 8, 12) and swizzle 0. The albedo, normal and gloss are added into the three terrain targets.
4. `ps30_terrain_to_gbuffer_ss`: the terrain targets divided by the accumulated alpha (the depth read from a copy of the depth channel,
   `viewz`, as it writes that channel), the noise map
   (`terrain/textures/common/terrain_noise`, `g_noise_params`), the climate's `medium_detail_map.dds` as a normal detail
   (`g_mid_distance_details`: far distance, repeat, strength), into the G-buffer (albedo as sqrt, specular white, z / w).

Open: the source of `g_channel_swizzle` (0, the index map, here), the far composite (`ps50_far_terrain_composite`, the same with
the far fields' maps) and whether the battle terrain goes through the virtual texture (`terrain_enable_battle_vtex`,
`vs30_terrain_composite_vtex`). The resolve reads the gloss target as reflectivity `x + y * (luminance - x)` and smoothness
`z + w * ((1 - luminance) - z)`.

Shadows: `vs30_shadow` is `vs30_geom` again; `ps30_shadow` is shared.

## Vegetation and decals

- Trees (`rigidbranchcommon_v3` / `rigidleafcommon_v3`, both techniques): the same instance values in other slots: TEXCOORD0..2 the
  position's rows, TEXCOORD3 (.xy: tint and wind seed, 1 - dissolve; no burn), TEXCOORD4..6 the LOD position's rows (Attila: TEXCOORD3..5
  the rows, TEXCOORD6 the values) (`tree_layout`). The leaves' alpha is scaled by saturate(distance^2 * `camera_VS_PS.g_hide_foliage`):
  0 discards every leaf; the engine's value is not known (1 here: faded within a unit of the camera). Four G-buffer targets, no burn or
  fire maps. Billboards: `s_diffuse_map` (Attila's `s_diffuse_map_tree`).
- Grass (`ps30_gbuffer_grass`): another shader than Attila's. It alpha-tests the diffuse (0.35) and the dissolve map, reconstructs the
  ground under the pixel from the depth and fades the blades into it with distance itself, reading the terrain resolve's inputs
  (`s_albedo_map`, `s_terrain_normal_map`, `s_terrain_gloss_map` the composite targets, `s_depth_map` the depth copy,
  `s_mid_distance_detail_map`, `s_noise_map`, `s_shroud`); it writes four targets, opaque (Attila's blends by its alpha).
- Projected decals: the depth `s_depth_buffer` and the normal `s_gbuffer_normal_wrap` (Attila's `gbuffer_channel_4_sampler`,
  `s_gbuffer_normal`); two targets written (normal, albedo), no specular.
- The lists: Rome 2's tiles have the older `<climate>.tree_list` / `default.grass_list` (`wce_formats::old_lists`), not the FASTBIN
  ones; the tile tool reads them (and, without the kit's vegetation parameters, the lists a decompiled project carries).

## Sea (`ocean.rs`)

`dx11_sm_3_0_ocean` `vs30_main` / `ps30_main_ultra_water_quality` is a forward pass: one colour target (the lit sea, opaque), drawn
into the HDR target over the same chunks, surfaces and depth-stencil state as Attila's G-buffer ocean. The same vertex input. Renamed:
`ocean_VS.g_vs_sea_bed_info` / `ocean_PS.g_ps_sea_bed_info` (Attila's `g_vs_sea_info`), the detail normals' scales and speeds in
`ocean_PS` (Attila's `OceanSharedConstants`), the sky cube `s_sky` (Attila's `s_environment`), the foam `combined_foam_texture_sampler`
(`sea/combined_foam.dds`), the sea bed info read by the pixel shader too. With shadows the game's variant is
`dx11_enable_shadows_shadows_hardware_gather_5_0_sm_5_0_ocean` (the plain one's inputs plus `shadowmap_PS` and the shadow maps): the
sea receives the sun's shadows (the lighthouse's on the water in the user's Carthage screenshots, 2026-11-01).

What the pixel shader does (DXBC, 2026-11-01): the sky's reflection comes from `reflection_texture_sampler` alone, sampled at the
pixel's screen position upside down `(x, 1 - y)` plus 0.065 of the normal, times a Fresnel term and 0.65; the sky cube `s_sky` only
fogs. So the planar reflection is the view of a camera at the eye mirrored in the water's plane looking at the mirrored target with +y
up (its picture is the mirror image upside down). Without it (black until 2026-11-01) the sea was dark grey. The colour: the deep water
colour lit (sun times the shadow, ambient), mixed towards the refraction (the scene behind) by `exp(-0.2 d)` of the water's depth d; the
whitecaps: `combined_foam`'s red at the sea uv, faded over 1000 units, sharpened by its green / blue at 15 x, plus the shore foam under
1.6 m of water; all of it times `saturate(-ground * 1.667)` of the sea bed: no foam where the sea bed info reads 0, as everywhere outside
its square (x, z in [-1024, 1024] around the world origin, the battle's centre; the vertex shader's flattening likewise).

**Settled by a RenderDoc capture (2026-11-01)** of a battle at Carthage (`D:\renderdoc\rome2_carthage.rdc`, and a beach view
`rome2_carthage_beach.rdc`; the tile `settlement_greek_ports/greek_carthage/medium` (not large: no round harbour), weather
`rome_med/su/land/day/dry/day_dry_05`, Rome 2 on Windows with RenderDoc 1.46, MSAA off, water quality 5):
- The sea at water quality 5 (Extreme) is `ps30_main_high_water_quality` (the shadowed gather 5_0 variant), not ultra: no whitecap
  sample, so no whitecaps over the open sea (`ocean::ROME2_WATER`).
- The reflection (R11G11B10, half size): a mirrored camera (its eye under the water, its projection oblique at the water) draws a fog
  pass (fog_VS_PS, the sky cube, alpha blended), the sky dome and the cloud planes, before the G-buffer; after the lights, the water's
  refraction copy (half size, RGBA16F) and then `helpers.hlsl` `scree_space_reflections_ps` (screen_space_refl_params: the camera's
  view_projection and its inverse, camera_position_and_reflection_height (eye, 0), min / max water height 0 / 1, tiles_per_dim 256,
  playable_area_size 2048, quality_factor 0.8; textures: scene_tex = the refraction copy, depth_tex = the depth, water_height_tex 256 x
  256 RGBA8) blend the scene's reflection over it (SRC_ALPHA / INV_SRC_ALPHA). Then the sea, before the sky.
- The sea bed info texture (1024 x 1024 B8G8R8A8, `r * 25 - 20`, the same in every capture of the battle) holds the ground as drawn,
  the LF in: at the shore it is within its 8-bit step of the terrain's vertices (traced with RenderDoc's vertex debugger).
- The terrain surface (`rigidterrain` geom, events 1323 .. 1535) is drawn with DepthBias 1, SlopeScaledDepthBias 2 (the shadows'
  state), CULL_FRONT: where the camera grazes the ground its depth lies behind it (1.7 units at 270 units' distance on the beach), so
  ground positions read back from the depth buffer are too low there (0.5 m at that beach), and the sea, depth-tested against it, covers
  the shore's edge further up: the wide foam band of the game's shore. The land's LF is the 256 x 256 window (`g_height_map_dimension`
  256, `g_ws_lf_aabb` (8192, 8192, 16384, 16384), `LF * 3000 - 20`), sampled by a gather whose texel i sits at u = i / 256 (not its
  centre); the sea tiles' the whole sea LF (4064 x 2874, `* 400`). The terrain mesh is decimated (vertices 2 to 16 units apart on the
  2-unit grid).
- The editor's values for that weather equal the game's (sun direction and colour, the deep water colour, the fog, the ambient cube, the
  tone mapper's limits, the S-curve, the black and white points); the light buffer before the tone mapper within a few per cent.
- The editor (`renderer/reflection.rs`, `ocean.rs`): the mirrored sky and clouds (not the fog pass, no oblique plane), then the game's
  SSR over them; its water height map is one texel (water at 0 everywhere: the game's only skips the pixels with no water under them). The
  sea is drawn around the battle's centre (its camera and cascade constants moved to that frame for the draw,
  `Renderer::shift_camera_constants`), so the sea bed square, and the foam, stays on the battle as the game's. Its terrain surface takes
  the game's depth bias (`States::rs_terrain_rome2`).
- Shadows past the cascades: `g_tBakedShadowBuffer` (4096 x 4096 R16, sampled with a linear LESS_EQUAL comparison, wrapping)
  and `g_vBakedShadowmapScale` (1 / 4272, 1 / 4272, -1 / 439.3), `_Offset` (toroidal, around the camera; depth 0.9467), `_Shear` (the
  sun's x / -y and z / -y times the scale), `_Params` (1 / 1922, 238.7, -1 / 47.74, 6): see `renderer/shadow_baked.rs`. The light pass
  resets its samplers before it binds the shadow ones: the baked sampler is bound after (on a plain sampler `sample_c` returns 0: all
  shadowed).
- The terrain resolve's and the grass's `ps_height_map_cb`: `g_mid_distance_details` (20, 400, 3, 0.6) in game units, `g_noise_params`
  (0.7, 0.3, 0.001, 0); grass_PS `g_inv_lod_distance` 0.005, `g_alpha` (1, 0.75), `g_normal_scale_bias` (2, -1), `g_distance_scale` 1.

## Rivers and water planes (`water.rs`)

`rigidriver ps30_main_battle_river` and `waterplane ps30_battle_ultra_water_quality` are forward passes too (one colour target, into the
HDR target), with the planar reflection `s_terrain_reflection` (the mirrored sky, as the sea's) and the sky cube `s_sky` (the water plane; Attila's
`s_environment`). The river's vertex shader reads the LF window from `lf_block_cb` and `s_low_res_height_map` (Attila's
`t_low_res_height_map`, `vs_height_map_cb`); `vs_height_map_cb` keeps the LF / HF scales and offset, so the renderer's LF scale 0 leaves
the ribbon at its mesh height (+ 0.4, the shader's).

## Not drawn yet

- The effects' other views and classes. The billboards are simulated and drawn (docs/re/rome2_vfx.md, `vfx_rome2`) in the DeferredAlpha
  view; the Distortion, SeaColourOverlay and shadow views, ribbons, spawn spacing and mesh spawning, particle events and the rigid
  emitters (the effects' point lights) are not, nor Rome 2's weather particles.
