# Battle terrain: how the game draws it (spike, 2026-10-09)

Method as for `renderer.md`: the game's compiled shaders (reflection through `cli/fxc_dump`, cbuffer names in `docs/re/shaders/cbuffers.txt`) plus a
readable source of what they do. The packs ship a GLSL translation of every shader next to the `.fxc` files (`fxc/glsl_sm_4_0_<file>.hlsl_<stage>_<entry>.glsl`,
SM4 instruction listings with the original register names), so the terrain shaders can be read without a disassembler. Constants are cross-checked
against the live cbuffer dump (`cbs.txt` from `dump_cbs.py`, a battle on a real map). Nothing here is shipped in the repo; the files are read from the
user's installation.

Status: **architecture and inputs verified from the shaders; draw order, patch mesh layout and the data feeding the textures are open** (list at the end).
`Terrain` entities of the RenderScene are drawn today with the interim material (one tiled diffuse through the rigid G-buffer shader), so the tile tool
works while the pixel-close pass is built.

## Shaders (`rigidterrain.hlsl`, `rigidterrainblend.hlsl`)

| Entry | Stage | Role |
|---|---|---|
| `vs30_geom_new` (`vs30_geom`, `vs41_geom_new`) | VS | the base terrain surface: grid vertices displaced by the low-res height map |
| `ps_depth` | PS | the base surface pass writes only view-space depth: `color0.x = vsOut_T1.x` |
| `vs30_blend` | VS | draws one box per terrain tile (instanced), flat per-tile data to the pixel shader |
| `ps30_blend` | PS | the material pass: projects the tile's maps onto the surface using the depth buffer, writes G-buffer channels 1-3 |
| `vs30_far_blend` / `ps30_far_blend` | VS/PS | the same for distant terrain (`g_far_*`), from a coarse world-sized map |
| `ps_blend_common_ts` / `vs_blend_common_ts` | VS/PS | the shared tile-space blend used by the editor and campaign variants |
| `vs30_custom_geom_mat`, `ps30_custom_geom_gbuffer` | VS/PS | custom geometry (cliffs, roads) with a material instead of tile blending |
| `ps_sea_geom` | PS | sea surface geometry |

`terrain.hlsl` and `terrainblend.hlsl` hold the campaign-map variants (`ps30_main_coast`, `ps30_main_trade_route`) and the editor visualisations.

## Pass structure (verified from the shader code)

1. **Surface.** The patch mesh (`vsIn_P0`: grid vertex, `vsIn_P1..P3`: rows of the instance's 3x4 world matrix, like the rigid shaders) is displaced in the
   vertex shader. World x, z = `dot(P0, P1)`, `dot(P0, P3)`; the high-frequency height comes from the vertex itself (`dot(P0, P2) * g_height_map_hf_scale`).
   The low-frequency height is read from `t_low_res_height_map` (the map's LF height texture, `lf_height_map.dds` in the BOB outputs) with a manual bilinear
   fetch of four texels:
   `uv = world_xz / g_world_dimensions + g_lf_uv_offset`, clamped to one texel inside the map (`1 - g_height_map_dimension.zw`), texel = `uv * g_height_map_dimension.xy`,
   `h = bilinear(fetch) * g_height_map_lf_scale + hf - g_height_map_vertical_offset`. Then a small distance bias: the sign of `h` (above or below sea level 0)
   times `saturate((|xz - camera.xz| - 20) * 0.1) * 0.09` is added to y, which moves distant land up and distant sea bed down by 9 cm so that the
   material and water passes win the depth test. Output: `gl_Position = view_projection * (x, y, z, 1)`, `T0 = world position`, `T1.x = view-space z`
   (`dot(pos, camera_VS_PS.view row 2)`). The pixel shader writes that depth into the depth channel; no colour.
2. **Material pass per tile.** `vs30_blend` scales a unit box from `T1` (min corner, world) to `T2` (max corner) so that it covers the tile's footprint; the
   flat outputs carry the tile rectangle (`T1.xy` = min xz, `T1.zw` = max xz), the tile's rotation index (`T2.x`, 0..3) and the layer/mapping data (`T2.y`, `T3` =
   four indices into `ps_mapping_array`). `ps30_blend` then, for every pixel the box covers:
   - reconstructs the world position from `s_depth_map` and `inv_view_projection` (rows 21..24 of `camera_VS_PS`), and discards outside the tile rectangle;
   - derives the tile-local uv from the rectangle and the rotation index (`c[4]` table of 90 degree rotations);
   - `s_alpha_map`: tile alpha, `alpha * 1.1 - 0.1 < 0` discards (feathered tile edge);
   - `s_blend_map` (RGBA, normalised to unit length): weights of four terrain layers; `s_index_map` (RGBA): per-layer selector in [0, 1] times 3, whose integer part
     picks a slice of the layer's four variations in the texture array and whose fraction blends with the next slice;
   - for each of the four layers: `slot = mapping[T3.i] * 4 + selector`, uv = `world_xz / g_blend_pixel_scale_array[slot].x` times the layer weight, samples
     `s_diffuse_array` and `s_normal_array` (2D texture arrays, mip bias -0.66) at slice `slot` and `slot + 1` and blends;
   - normals are two-channel (`.wy`, +0.00196 bias, `*2-1`, z rebuilt from `sqrt(1 - x² - y²)`), blended per layer, combined with the tile's own `s_normal_map` and the
     map-wide `s_lf_normal_map` (weights `g_normal_scales`, `g_mid_distance_normal_strength`, distance falloff `g_mid_distance_*`);
   - outputs: `color0 = (diffuse, alpha)`, `color1 = (normal.xyz, alpha)`, `color2 = (light params x, y: from `s_light_params` rows chosen by the same layer indices,
     z: the layer-exempt-group mask result, w: the weighted per-layer value `g_blend_pixel_scale_array[...].z`)`. The alpha channel blends the pass into the G-buffer.
3. **Far blend** replaces pass 2 beyond `g_far_tile_distance` with the coarse map-wide material (`g_far_world_*_uv_offset`, `g_far_blend_range`).

## Resources

| Name | Kind | Source (to be confirmed per item) |
|---|---|---|
| `t_low_res_height_map` | VS texture | LF height map of the map (`lf_height_map.dds`; units and special values in the attila-battle-terrain skill) |
| `s_depth_map` | PS texture | the depth channel written by pass 1 |
| `s_alpha_map`, `s_blend_map`, `s_index_map`, `s_normal_map` | PS textures | per tile, from the tile's `blend_map` / `final_*` / `ground_type_map` family (`tile_database.bin` selects) |
| `s_lf_normal_map` | PS texture | map-wide normal map from the LF height map |
| `s_light_params` | PS texture | one row per terrain texture: light parameters, width `g_light_params_texture_width` |
| `s_diffuse_array`, `s_normal_array` | PS texture arrays | every terrain texture of the climate (`terrain/textures/battle/...`), sections sized by `g_array_section_sizes` (dump: 8, 5, 13) |
| `vs_height_map_cb` (96 bytes) | cbuffer | `g_height_map_lf_scale`, `_hf_scale`, `_vertical_offset`, `g_blend_lf_min_max`, `g_height_map_dimension`, `g_world_dimensions`, `g_infield_dimensions`, `g_lf_uv_offset`, `g_layer1/2_texture_index`, `g_far_terrain_scale`, `g_far_render_bounds` |
| `ps_height_map_cb` (128 bytes) | cbuffer | live values in the dump: `g_mid_distance_details` (20, 550, 3, 0.9), `g_near_distance_details` (200, 4, 0.6), `g_mid_distance_slope` (0.96, 3), `g_normal_scales` (1, 1, 1), `g_blend_gain` 1, `g_blend_contrast` 100, `g_far_tile_distance` 500..3000 |
| `ps_texture_array_cb` (6480 bytes) | cbuffer | `g_blend_pixel_scale_array[400]` (dump: 9, 9, 0.3), `g_remapped_season`, `g_climate_change_index`, `g_layer_exempt_groups` (1e6 = none), `g_sea_texture_index` |
| `ps_mapping_array` (32768 bytes) | cbuffer | `g_array_mapping[2048]`: tile layer index to texture array slot |

## What the interim terrain pass does and does not

`ModelCache::terrain` builds the height grid into 64 x 64 cell patches in the rigid vertex format (half-float positions local to the patch, per-patch offset in the
instance matrix, normals from the heights) and draws them through `rigidstandard` with one tiled diffuse texture and flat normals/specular. Missing against the
game: LF + HF height composition and the sea-bias, per-tile material layers with their alpha/blend/index maps, normal blending and detail by distance, the far blend, the
sea. It is enough for editing: the surface shape, picking and lighting are right.

## Open items before a pixel-close pass

- Draw order and states of passes 1 and 2 in the engine (blend state of the material pass into G-buffer channels 1-3, stencil), from a dump or from the call
  order in `empire.retail.dll` (the terrain node's render function).
- Depth convention: `ps_depth` writes the view-space z (`T1.x`), while pass 2 feeds `(ndc.xy, depth channel, 1)` through the matrix at `camera_VS_PS + 336`
  (named `inv_view_projection`) and divides by w. `renderer.md` records the rigid G-buffer depth as z/w ("assumption"); check which one the depth channel holds
  before building pass 2 (the answer decides whether the matrix is the plain inverse view-projection).
- The patch mesh: grid resolution and instance layout of `vsIn_P0..P3` for `vs30_geom_new` (the shader only fixes that P0 is a vertex of a regular grid and P1..P3 the world
  rows); LOD scheme between patches.
- How `g_height_map_lf_scale/hf_scale` relate to the LF height units (raw 478/440 per the skill) and to the tile `height_map_0.png`.
- The per-tile map set and which file feeds which sampler (`alpha`, `blend`, `index`, `normal`), their formats after BOB (`tile_map.index`, `tile_map.tiles`), and how
  `ps_mapping_array` is filled from `terrain.tile_list` / `tile_database.bin`.
- `s_light_params` content and the season/climate handling (`g_remapped_season`, `g_climate_change_index`, `g_layer_exempt_groups`).
- The campaign-map terrain (`terrain.hlsl`) is a separate set and not needed for the battle tools.

## The tool's ground colour (Phase 3 of `docs/plans/tile_ted_parity.md`, 2026-10-10)

What was measured about the inputs (black box, from the kit and the packs; nothing from the engine's code):
- **`blend_map.tif` has 16 weights per pixel** (16 bit each, `blend_map.tif` of the TEd layout: 242-byte header, then `width * height * 16` little-endian samples), the same grid as the height map (`(cells + 2) * ppc` square, one cell of border: 384 x 384 for a 1 x 1 tile). The 16 weights of a pixel sum to 65535 (65519 .. 65535 over a whole tile). Channel `4 g + v` is the weight of **texture group g** (0..3 = the entry's `TEXTURE_GROUP` red, green, blue, alpha) **variation v** (0..3, the `0_`..`3_` textures of the group): in `tdd_custom_lothlorien/1x1/aa` (grass / mud / forest / rock) channel 0 carries 57 % of all weight, channel 4 25 %, channel 8 16 %, channel 12 3 %, and each group uses one or two variations. BOB reduces this to the RGBA `blend.dds` (the four group weights) and `index.dds` (which variation), the two maps `ps30_blend` reads (`s_blend_map` / `s_index_map` above).
- **Texture files**: `terrain/textures/battle/<climate lower case>/<group>/<variation>_<su|au|wi>_<diffuse|normal>.dds`, DXT5, 1024 x 1024, 11 mips, in the TDD weather pack and the vanilla terrain packs. The climates: `default` (all 18 groups, summer only), `rome_continental`, `rome_med`, `rome_atlantic`, `rome_desert`, `rome_easterndesert`, `rome_destruct`, `rome_destructdes`, `rome_grey`; a climate that lacks a group (rome_desert has no `moorland`) uses `default`; seasons: `su` always, `au` / `wi` in the main climates. The group names are those of the tile entries and of `_settings.xml`'s `<TEXTURE name=...>` lists.
- **Scale**: `_settings.xml` `<CLIMATE><TEXTURES><TEXTURE name blend_pixel_scale ...>`: metres per repeat of the texture inside the tile (rock 40.1, mud 9, mountain 1200.1, ...), what `g_blend_pixel_scale_array[slot].x` holds in the shader (uv = world xz / scale).
- `ground_type_map.png`: RGBA 8 bit over the bordered tile (all zero in the TEd-made tile checked, river and sea values in the river tiles); `protection_map.dds`: R32F 0 / 1 over the tile without border (128 x 128 for a 1 x 1 tile).

What the tool does with them (`tools/tile/terrain_bake.*`, `libs/formats/dds_decode.*`, `tile_climate.*`, `libs/render` terrain skin): a **CPU stand-in for pass 2** that evaluates the same inputs per texel instead of per pixel and **bakes** the result once per tile / climate / season: for every texel of an image over the bordered tile (up to 4 texels per metre, 2048 on a side at most) the 16 weights are sampled bilinearly from the blend map, each layer with weight is sampled from its texture (bilinear, wrapped, at `world / blend_pixel_scale`, from the mip whose texel density matches the image's) and the weighted sum is the colour (blended in sRGB space; weights of layers whose texture is missing are dropped and the rest renormalised). The renderer cuts the image into the terrain patches (one texture of its own per 64 x 64 cell patch, a texel of border, mips by box filter, uv 0..1 over the patch so half-float uvs stay exact) and draws it through the rigid G-buffer shader as the diffuse. A height stroke does not touch the image (the colour does not depend on the heights).
- **Not done** (what the game's pass has and this does not): the per-pixel blend at screen resolution (the bake is at most 4 texels per metre: close up it is softer than the game), the normal maps of the layers and of the tile (`s_normal_map`, `_normal.dds`: the terrain is lit with the heights' normals only), the mid- and near-distance detail layers, the index map's variation mixing inside a layer (the 16 weights already select variations), `s_light_params` (specular / gloss of the layers), the far blend and outfield scale, the edge blend with neighbour tiles, season mixing (`g_remapped_season`). Colour is blended in sRGB space like a plain image average; whether the game blends in linear space is not checked. The climate is a tool setting (the tile does not store it).
- Debug views (TEd has them): *blend groups* (the four group weights as flat red / green / blue / yellow), *ground types* (the PNG as stored), *protection* (red = 1).
