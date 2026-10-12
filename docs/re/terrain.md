# Battle terrain: how the game draws it (spike, 2026-10-09)

Method as for `renderer.md`: the game's compiled shaders (reflection through `cli/fxc_dump`, cbuffer names in `docs/re/shaders/cbuffers.txt`) plus a
readable source of what they do. The packs ship a GLSL translation of every shader next to the `.fxc` files (`fxc/glsl_sm_4_0_<file>.hlsl_<stage>_<entry>.glsl`,
SM4 instruction listings with the original register names), so the terrain shaders can be read without a disassembler. Constants are cross-checked
against the live cbuffer dump (`cbs.txt` from `dump_cbs.py`, a battle on a real map). Nothing here is shipped in the repo; the files are read from the
user's installation.

Status: **architecture and inputs verified from the shaders; draw order, patch mesh layout and the data feeding the textures are open** (list at the end).
`Terrain` entities of the RenderScene with a `TerrainMaterial` are drawn by the game's passes (the last section); without one (the weather editor's flat ground) they are drawn with the interim material
(one tiled diffuse through the rigid G-buffer shader). **Fixed 2026-10-10**: that interim path (and every mesh without a normal map) used `ModelCache::flat_normal_` = `0xff80ff80`, which the rigid shader decodes
(`.wy`, x = A, y = G) as the tangent-space normal (1, 1, 0): the G-buffer normal lay in the surface plane, not up. The constant is now `0x7fff7f7f` (x = y = 0.5, z rebuilt = up). The weather editor's ground and
every normal-less mesh are lit differently since (max 29 levels on the dry capture); the capture references of `build/gates_base_perf` were re-baselined (`docs/baseline.md`).

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

## The tool's terrain pass (2026-10-10: the game's own shaders, `libs/render/terrain_pass.cpp`)

Rule of the project: everything drawn to show the game's look runs the game's compiled shaders. The first Phase 3 version baked the colour on the CPU; it is replaced
by the game's passes, fed the tile's own maps. **Read the DXBC disassembly of the `dx11_*.fxc` (`cli/fxc_dump <dir> <out> <asm dir>`), not the pack's GLSL listings**: the
GLSL is the GL path and differs in orientation (the screen-to-NDC y flip is missing in it) and in the texture names (`t_diffuse_array`, not `s_diffuse_array`).

### Frame order
After the objects' G-buffer pass (`pass_gbuffer`, which skips the legacy ground when every visible terrain has a material), `pass_terrain_material`:
1. **Surface**: `vs30_geom_new` (grid vertices, `POSITION1..3` = the rows of a 3x4 world matrix per 64 x 64 cell patch; the height comes from the texture) with `ps30_geom` (clip z/w into
   `terrain_zw_`, also the hardware depth, stencil 2 where the surface is nearest) and again with `ps_depth` (view-space z into `terrain_viewz_`, depth test EQUAL).
   *Addressing of the height texture* (the shader's own, read from the DXBC): `uv = world xz / g_world_dimensions + g_lf_uv_offset` (one scalar offset for both axes), the second axis flipped
   (`row = size - k`), texel = `uv * g_height_map_dimension`, four `ld` loads and a manual bilinear, no clamp below sample 1 and `1 - 1/size` above. So the tool's texture is square, one sample
   padded on the low side (`origin - step` is the world position of sample 0) and rows are stored top = highest z; `g_height_map_lf_scale` 1, `hf_scale` 1 (the instance row P2 carries y), offset 0.
   The vertex shader also moves distant land up and sea bed down by up to 9 cm so the material pass wins the depth test (`sign(h) * saturate((dist - 20) * 0.1) * 0.09`).
2. **Blend**: `vs30_blend` draws one unit box per tile (instance data: `TEXCOORD0` the four layer mapping indices, `1` the box minimum, `2` the maximum, `3` the rotation index and layer base),
   back faces only, stencil-tested to the terrain pixels, `ps30_blend` into three RGBA16F targets with ONE / ONE blending (premultiplied by the tile's alpha: colour, normal, light parameters;
   the alpha channel accumulates). It reads `s_depth_map` = the clip z/w of step 1 and rebuilds the world position with `inv_view_projection`.
3. **Resolve**: `vs30_terrain_to_gbuffer` (a fullscreen quad) with `ps30_terrain_to_gbuffer_ss`: divides the accumulated targets by their alpha, applies the mid-distance detail (colour and
   normal, the climate's `medium_detail_map_colour/_normal.dds`), the far maps and the snow / shroud branches, and writes **the five G-buffer targets** (channel 1 normal + gloss, 2 sqrt albedo +
   reflectivity, 3 specular + material id 1, 4 clip z/w computed from the view-z map, 5 instance id 0); stencil EQUAL 2 -> DECR, so the lights see geometry (stencil 1).
   *The directional light treats material id 1 pixels as overlay receivers*: the result is `overlay.rgb + lit * overlay.a` (`s_overlay`, `g_overlay_view_projection`), so `s_overlay` must be
   alpha 1 (colour 0): the renderer binds `overlay_none_srv_`. A black texture there (alpha 0) made the whole ground black.

### The inputs
- **Blend / index maps** (`s_blend_map`, `s_index_map`, RGBA8 over the bordered tile, 1 texel per metre): BOB's reduction of `blend_map.tif`'s 16 weights, checked against BOB's `blend.dds` /
  `index.dds` of the osgiliath test map (1280 x 1280, same orientation, correlation 0.998, the rest is DXT5): `blend.rgba` = the sums of the four variation weights of group red / green / blue /
  alpha, `index.rgba` = `(1 w1 + 2 w2 + 3 w3) / (w0 + w1 + w2 + w3) / 3` per group (the shader multiplies it by 3 and blends the two neighbouring variations).
- **Normal map** (`s_normal_map`): `.a` = x, `.g` = z of the unit normal * 0.5 + 0.5. TEd's `tile_normal.tga` (BGRA, bottom-up) is BOB's `normal.dds` channel for channel (`TGA.R -> A`, `TGA.G -> G`,
  rows reversed to top-down) and equals the height map's gradient (`R ~ -dh/dx`, `G ~ -dh/dz`, correlation 0.97 / 0.98); the tool computes it from the live heights (central differences, one
  texel per height sample) so a brush stroke changes the shading. The shader adds the unit tile normal's xz to the (flat) map-wide normal and normalises. The sign was checked physically: with the
  default sun (from +z) the +z side of a raised spike is the bright one.
- **Texture arrays** (`t_diffuse_array` sRGB, `t_normal_array`): slice `4 * group + variation` of the climate's `terrain/textures/battle/<climate>/<group>/<variation>_<su|au|wi>_<diffuse|normal>.dds`
  (1024 x 1024 DXT5, 11 mips, whole files, no streaming); a slice that does not exist or does not match the first one's size / format is a neutral one. The layer mapping `g_array_mapping[i] = i`,
  `g_array_section_sizes = (1, 1, 0, 0)` (so season / climate indices do not move the slot), `g_blend_pixel_scale_array[layer] = (blend_pixel_scale, outfield scale, mid_distance_strength)` (the shader's `cb3[mapping + 1]` is that element: row 0 of the cbuffer is `g_light_params_texture_width`; the first version stored it one element too high, so layer 0 had scale 0 and drew one flat texel - fixed 2026-10-10) from
  `_settings.xml`, `s_light_params` = (`lerp_smoothness`, `lerp_reflectivity`) per slot (16 x 1, `g_light_params_texture_width` 16).
- **Alpha map**: 1 everywhere (the editor shows one tile). The kit's `final_alpha.dds` (R32F, 1 sample per metre over the tile without its border, 0 at the edge ramping to 1 in about 25 m) is the
  feather for neighbours and is not used yet.
- **Constants** the live game shows (a battle dump, `cbs.txt`): `g_mid_distance_details` (20, 550, 3, 0.9), `g_near_distance_details` (200, 4, 0.6, 0), `g_mid_distance_normal_strength` 2,
  `g_mid_distance_slope` (0.96, 3), `g_normal_scales` (1, 1, 1), `g_blend_gain` 1; in the live game `g_array_section_sizes` = (8, 5, 13), `g_light_params_texture_width` 32, `g_layer_exempt_groups` 1e6 (none),
  `g_refraction_depth` 0.75. `g_far_tile_distance` and `g_far_blend_range` are set very large here: the map-wide normal map, the far diffuse / normal maps and the mask map (`s_mask_map` = black) are a
  map-level feature (BOB's outfield data), not the tile's, and are switched off.
- **Assumptions / open** (no in-game capture to compare against yet): the sampler states of the maps (clamp linear here), `g_overlay_view_projection` (identity, a constant overlay), the far-blend
  and map-wide normal layers, the shadow-receiving variants of the shaders, `s_snow_normals` (a flat map: the snow branch needs `g_snowing_intensity` > 0), season mixing, the second and third
  layer indices of the resolve (`g_layer1/2_texture_index`, the mask is black so they stay off), neighbour tiles (their alpha feather and the overlap between tiles), one detail texture pair per climate.
- **The CPU bake** (`tools/tile/terrain_bake`) remains only for TEd's debug views of the tile's maps (blend groups, ground types, protection): editor visualisations, not the game's look.

### What was measured about the inputs (Phase 3, black box)
- **`blend_map.tif` has 16 weights per pixel** (16 bit each, `blend_map.tif` of the TEd layout: 242-byte header, then `width * height * 16` little-endian samples), the same grid as the height map (`(cells + 2) * ppc` square, one cell of border: 384 x 384 for a 1 x 1 tile). The 16 weights of a pixel sum to 65535 (65519 .. 65535 over a whole tile). Channel `4 g + v` is the weight of **texture group g** (0..3 = the entry's `TEXTURE_GROUP` red, green, blue, alpha) **variation v** (0..3, the `0_`..`3_` textures of the group): in `tdd_custom_lothlorien/1x1/aa` (grass / mud / forest / rock) channel 0 carries 57 % of all weight, channel 4 25 %, channel 8 16 %, channel 12 3 %, and each group uses one or two variations. BOB reduces this to the RGBA `blend.dds` (the four group weights) and `index.dds` (which variation), the two maps `ps30_blend` reads (`s_blend_map` / `s_index_map` above).
- **Texture files**: `terrain/textures/battle/<climate lower case>/<group>/<variation>_<su|au|wi>_<diffuse|normal>.dds`, DXT5, 1024 x 1024, 11 mips, in the TDD weather pack and the vanilla terrain packs. The climates: `default` (all 18 groups, summer only), `rome_continental`, `rome_med`, `rome_atlantic`, `rome_desert`, `rome_easterndesert`, `rome_destruct`, `rome_destructdes`, `rome_grey`; a climate that lacks a group (rome_desert has no `moorland`) uses `default`; seasons: `su` always, `au` / `wi` in the main climates. The group names are those of the tile entries and of `_settings.xml`'s `<TEXTURE name=...>` lists.
- **Scale**: `_settings.xml` `<CLIMATE><TEXTURES><TEXTURE name blend_pixel_scale ...>`: metres per repeat of the texture inside the tile (rock 40.1, mud 9, mountain 1200.1, ...), what `g_blend_pixel_scale_array[slot].x` holds in the shader (uv = world xz / scale).
- `ground_type_map.png`: RGBA 8 bit over the bordered tile (all zero in the TEd-made tile checked, river and sea values in the river tiles); `protection_map.dds`: R32F 0 / 1 over the tile without border (128 x 128 for a 1 x 1 tile).


- Debug views (TEd has them): *blend groups* (the four group weights as flat red / green / blue / yellow), *ground types* (the PNG as stored), *protection* (red = 1).
