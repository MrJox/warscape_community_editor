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
   - derives the tile-local uv from the rectangle and the rotation index (`icb[4]` table of 90 degree rotations, read from the DXBC 2026-10-21): with
     (u, v) the pixel's place in the rectangle, centred (-0.5 .. 0.5, v along +z), index 0 keeps (u, v), 1 gives (-v, u), 2 (-u, -v), 3 (v, -u), plus 0.5.
     The same table turns the tile's own normal (`s_normal_map` .w = x, .y = z) back into the world: a tile placed turned by quarter turns draws its own,
     unturned maps (the map context relies on it);
   - `s_alpha_map`: tile alpha, `alpha * 1.1 - 0.1 < 0` discards (feathered tile edge);
   - `s_blend_map` (RGBA, normalised to unit length): weights of four terrain layers; `s_index_map` (RGBA): per-layer selector in [0, 1] times 3, whose integer part
     picks a slice of the layer's four variations in the texture array and whose fraction blends with the next slice;
   - for each of the four layers: `slot = mapping[T3.i] * 4 + selector`, uv = `world_xz / g_blend_pixel_scale_array[slot].x` times the layer weight, samples
     `s_diffuse_array` and `s_normal_array` (2D texture arrays, mip bias -0.66) at slice `slot` and `slot + 1` and blends;
   - normals are two-channel (`.wy`, +0.00196 bias, `*2-1`, z rebuilt from `sqrt(1 - x² - y²)`), blended per layer, combined with the tile's own `s_normal_map` and the
     map-wide `s_lf_normal_map` (weights `g_normal_scales`, `g_mid_distance_normal_strength`, distance falloff `g_mid_distance_*`). `s_lf_normal_map` is read at
     `uv = (x, -z) / g_world_dimensions` (vs_height_map_cb, no offset; the sampler wraps), .w = x and .y = -z of the normal (scaled by `g_normal_scales.x`): the base the
     tile's normal is added to;
   - outputs: `color0 = (diffuse, alpha)`, `color1 = (normal.xyz, alpha)`, `color2 = (light params x, y: from `s_light_params` rows chosen by the same layer indices,
     z: the layer-exempt-group mask result, w: the weighted per-layer value `g_blend_pixel_scale_array[...].z`)`. The alpha channel blends the pass into the G-buffer.
3. **Far blend** replaces pass 2 beyond `g_far_tile_distance` with the coarse map-wide material (`g_far_world_*_uv_offset`, `g_far_blend_range`).

## TerrainBlend meshes (`rigidterrainblend.hlsl`, 2026-10-18, from the shader code)

Buildings carry ground skirts (ramps, banks, earthworks) as rigid-model meshes of material type TerrainBlend (86): the default static vertex
layout (rpfm_lib read them as two float4s until 2026-10-18), the same vertex inputs as rigidcommon, often no textures of their own. They take
the ground's colour in one of two ways:

- **Terrain pass.** `vs30_geom` + `ps30_geom` write the mesh's view-space depth (`dp4(world, view row 2)`), the value `ps_depth` writes for the
  terrain surface; `ps30_blend` reconstructs positions from the z / w target and the resolve reads the view-depth one, so a mesh written into both
  (with stencil 2) is painted by the blend and resolve passes like the ground at its x, z. The z / w value has no rigidterrainblend shader:
  `vs30_main` (world position in `TEXCOORD0`) + rigidterrain's `ps30_geom` produce it. The tool does this (`draw_blend_meshes`); vertical faces
  get the top-down projection's streaks.
- **Virtual texture.** `ps30_null_gbuffer`, the same pixel shader body as the terrain surface's `rigidterrain.hlsl` `ps30_geom_vtex`: both read
  the terrain's virtual texture (next section) and write the terrain pass's intermediate targets, which `ps30_terrain_to_gbuffer_ts` resolves.

Without a terrain material (the weather tool's flat ground) the tool draws them with `ps30_gbuffer` and their own textures (white when they
have none).

## The terrain virtual texture (`virtualtexture.hlsl`, 2026-10-18, from the shader code)

The game has two ways to colour the terrain. The **screen-space** one is the tool's (`ps30_blend` boxes every frame, `ps30_terrain_to_gbuffer_ss`).
The **virtual texture** one bakes the same blend into a page cache once and lets the surface sample it. Not implemented by the tool. Shaders:

| Entry | Role |
|---|---|
| `vs30_quad_main` | fullscreen / viewport quad: `pos = v0.xz * 2 - 1`, z 0, w 1 (no constants) |
| `ps30_page_touch` | feedback: which pages, at which mip, the frame needs |
| `ps30_copy_blend_to_cache` (`_with_coastline_`) | bakes a blended page into the cache atlas |
| `ps30_page_table_replace` | points the page table at a newly resident page |
| `rigidterrain ps30_geom_vtex`, `rigidterrainblend ps30_null_gbuffer` | the surface and TerrainBlend meshes reading the cache (same body) |
| `rigidterrain ps30_terrain_to_gbuffer_ts` | resolve of the virtual-texture targets into the G-buffer |

**`cbTerrainVirtualTexture`** (c0..c2 = `g_terrain_vtex_constants_0..2`; the meaning is inferred from the arithmetic):

- `c0.xy`: world x, z of the terrain's uv origin; `c0.zw`: page-table size in texels at mip 0;
- `c1.x`: one page's extent in uv at mip 0 (1 / pages across); `c1.y`: the virtual texture's size in texels at mip 0 (the mip selection's
  scale); `c1.w`: 1 / world size;
- `c2.y`: a page's stride in the cache atlas (uv), `c2.z`: a page's interior in the atlas (uv), `c2.w`: the interior's offset (the border, uv);
  `c1.z`, `c2.x` are not read by these shaders.

**Reading** (`ps30_geom_vtex` / `ps30_null_gbuffer`; `v1.xyz` = world position, from `vs30_geom_new` or `rigidterrainblend vs30_main`):

1. `uv = ((x - c0.x) * c1.w, 1 - (z - c0.y) * c1.w)`; `t = uv * c1.y`; `lod = max(0, 0.5 * log2(max(|ddx t|^2, |ddy t|^2)))`.
2. Two mips, `A = floor(lod)` and `B = max(floor(lod) - 1, 0)`, blended by `frac(lod)` (B at 0, A at 1): one level finer than plain trilinear.
3. Per mip m: page-table texel `floor(uv * c0.zw / 2^m)`, read with `sample_l` at that texel's corner and level m (a point-sampled, mipped
   page table, `t0` / `vtex_page_table_sampler`); `round_ni(rgb * 255 + 0.5)` = (page x, page y, page mip p). The page holding `uv` at mip p
   spans `2^p * c1.x` in uv; `in_page = frac(uv / (2^p * c1.x))`; atlas uv = `(page x, page y) * c2.y + in_page * c2.z + c2.w`.
4. `t1` (`vtex_diffuse_cache_sampler`) and `t2` (`vtex_normal_cache_sampler`) at that atlas uv (plain `sample`).
5. Outputs: `o0 = (diffuse.rgb, 1)`, `o1 = normal cache (rgba)`, `o2 = 0`, `o3.x = z / w` (`view_projection` rows 2 / 3). The four targets are
   the terrain pass's albedo, normal, gloss and z / w targets (the tool's `acc[0..2]` and `zw`); the view-depth target comes from `ps_depth`
   as before.

**Resolving** (`ps30_terrain_to_gbuffer_ts`, the `_ss` resolve's twin): the albedo is read as is (`_ss` divides by `alpha + 0.001`: the blend's
premultiplied sums), the gloss target swizzled `.wxyz`; it writes the G-buffer (`o0 = normal * 0.5 + 0.5`, `o1 = sqrt(albedo)`,
`o2 = (1, 1, 1, 1 / 255)`, `o3.x` depth, `o4 = 0`). The virtual texture has no light parameters (`o2 = 0` above).

**Filling a page** (`ps30_copy_blend_to_cache`, `cbCopyBlendToCache`): the page's area is first blended into `land_albedo` / `land_normal`
(the `ps30_blend` outputs: premultiplied colour and normal); the shader is drawn over the page's slot in the atlas (viewport) and writes
`o0 = (albedo.rgb / albedo.a, albedo.a)` to the diffuse cache and `o1 = (normal.xyz / albedo.a * 0.5 + 0.5, normal.w)` to the normal cache.
Source uv: `((pos.x - h - vp.x) * vp.z, 1 - (pos.y - h - vp.y) * vp.w)`, `vp` = `g_viewport_pos_inv_size` (slot x, y, 1 / w, 1 / h),
`h` = `g_to_shfs_lhfs_hfo.x` (the half-pixel offset, as `camera_VS_PS` +448). `g_world_scale_offset`, `g_height_texture_sizes` and
`g_virtual_height` are declared but not read here (presumably for the page's blend draw). The coastline variant takes `sea_albedo` /
`sea_normal` where the sea's alpha is non-zero (sea over land), and divides the normal by its own source's alpha.

**Page table update** (`ps30_page_table_replace`, `cbPageTableReplace`): drawn over a region of one page-table mip (uv = `pos * g_MulAdd.xy
+ g_MulAdd.zw`); every texel whose entry (read at level `g_OldPage_SampleMip.z`) equals `g_OldPage_SampleMip.xy` (within 0.001) becomes
`g_Replacement`, others are kept. So each page-table texel names the best resident page covering it, coarser pages standing in until a finer
one is baked.

**Feedback** (`ps30_page_touch`, `cbPageTouch.xyz` = (1 / feedback width, 1 / feedback height, mip bias)): per feedback pixel, the depth
buffer (`t0`, its `.x`: the shader reads `t0.yzxw` and uses `.z`) is unprojected with `camera_VS_PS` rows 21, 23, 24 (`inv_view_projection` x,
z, w) to world x, z; sky (depth 1) is discarded. Mip = `max(floor(lod) + bias, 0)` as above; page = `floor(uv / (2^mip * c1.x))`; written as
`(page.x mod 256, page.y mod 256, page.x / 256 + 16 * (page.y / 256), mip + 1) / 255` (alpha 0 = nothing): the CPU reads it back and
schedules the pages.

**For the tool** (the plan, not done): a single resident "page" per terrain is enough to use the game's readers. Bake the tile's blend once
top-down (an orthographic view: `zw` from the surface, the blend boxes into the three accumulators), copy it into a one-page cache with
`ps30_copy_blend_to_cache`, and use a 1 x 1 page table holding (0, 0, 0) (`c0.zw = (1, 1)`, `c1.x = 1`, `c2 = (0, 1, 1 - 2b, b)` for a border
b); `ps30_geom_vtex` / `ps30_null_gbuffer` then read it and `_ts` resolves. Resolution is the limit (2048^2 over a 1024 m tile is 0.5 m per
texel against the blend pass's per-pixel detail), which real paging (feedback, page bake, table update) lifts. The weather tool's flat
ground could use the same path with a cache baked from its tiled diffuse and a flat normal.

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
1. **Surface**: `vs30_geom_new` (`POSITION0` = (x, height, z) of a patch's 65 x 65 grid, one vertex buffer per 64 x 64 cell patch of a heights buffer, shared by the terrains that
   hold it; `POSITION1..3` = the rows of the terrain's 3x4 world matrix moved to the patch, which may turn it about y; since 2026-10-21 the height comes from the vertex,
   `dot(P0, P2) * g_height_map_hf_scale`, as the game's tile meshes carry theirs) with `ps30_geom` (clip z/w into
   `terrain_zw_`, also the hardware depth, stencil 2 where the surface is nearest) and again with `ps_depth` (view-space z into `terrain_viewz_`, depth test EQUAL).
   *Addressing of the height texture* (the shader's own, read from the DXBC): `uv = world xz / g_world_dimensions + g_lf_uv_offset` (one scalar offset for both axes), the second axis flipped
   (`row = size - k`), texel = `uv * g_height_map_dimension`, four `ld` loads and a manual bilinear, no clamp below sample 1 and `1 - 1/size` above. It holds the map context's LF
   (R16 unorm, a square window around the camera, rows stored top = highest z; `g_height_map_lf_scale` = the LF's height, `g_height_map_vertical_offset` = 20 m plus the mean LF under the
   open tile), or a zero texture for a tile alone. One offset for both axes: the terrain of an LF window is drawn in a frame shifted so that its column 0 and its rows start at the same
   x and z (the camera constants shifted with it, put back after), the offset sliding that frame's origin to the camera along the window's diagonal (a shift to the map's corner lost the
   depth's precision: 80 km of coordinates). See docs/re/battle_maps.md, "The map context in the editor".
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
- **Samplers (verified 2026-10-18)**: the engine's sampler objects hold a D3D11_SAMPLER_DESC at +8 (fn 0x116d3f70 defaults: point, wrap, anisotropy 1,
  MaxLOD max; creation, fn 0x11767530, makes 16 copies with MinLOD 0..15 for streaming). The texture-quality class (vtable 0x11c8bab8) takes its filter
  from the texture filtering setting (fn 0x1177f410: anisotropic 2 / 4 / 8 / 16, else trilinear). rigidterrain (fn 0x117000c0): `s_albedo_map`,
  `s_terrain_normal_map`, `s_terrain_gloss_map`, `s_depth_map`, `s_low_res_height_map` point / clamp; the mid / near detail maps, the far and infield
  maps, `s_mask_map` and `s_diffuse_array` quality / wrap; `s_normal_array` trilinear / wrap. The blend shader (fn 0x116fd320): `s_alpha_map`
  quality / clamp; `s_index_map`, `s_blend_map`, `s_normal_map`, `s_lf_normal_map`, `s_diffuse_array` quality / wrap; `s_depth_map`, `s_light_params`
  point / clamp; `s_normal_array` trilinear / wrap. `s_snow_normals` (fn 0x116f7b40) trilinear / wrap. The editor uses anisotropic 8 for the quality class.
- **`g_overlay_view_projection` (verified)**: identity is the declared default (directional_light_PS fn 0x116d45c0, waterplane_VS fn 0x116d8fb0); a tile has
  no overlay, so it stays. `s_overlay` (trilinear / clamp) must be (0, 0, 0, 1) for "no overlay" (see the resolve note above).
- **Assumptions / open** (no in-game capture to compare against yet): the far-blend
  and map-wide normal layers, season mixing (see "Seasons and snow" for the snow normals), the second and third
  layer indices of the resolve (`g_layer1/2_texture_index`, the mask is black so they stay off; fn 0x118839d0 sets them from ints at the
  drawable's +0x270 + 0x80 / + 0x84 and binds the mask from its +0x34 object, vfunc +0xac, beside the far maps: the map-level path, not followed further), neighbour tiles (their alpha feather and the overlap between tiles), one detail texture pair per climate.
- **The CPU bake** (`tools/tile/terrain_bake`) remains only for TEd's debug views of the tile's maps (blend groups, ground types, protection): editor visualisations, not the game's look.

### What was measured about the inputs (Phase 3, black box)
- **`blend_map.tif` has 16 weights per pixel** (16 bit each, `blend_map.tif` of the TEd layout: 242-byte header, then `width * height * 16` little-endian samples), the same grid as the height map (`(cells + 2) * ppc` square, one cell of border: 384 x 384 for a 1 x 1 tile). The 16 weights of a pixel sum to 65535 (65519 .. 65535 over a whole tile). Channel `4 g + v` is the weight of **texture group g** (0..3 = the entry's `TEXTURE_GROUP` red, green, blue, alpha) **variation v** (0..3, the `0_`..`3_` textures of the group): in `tdd_custom_lothlorien/1x1/aa` (grass / mud / forest / rock) channel 0 carries 57 % of all weight, channel 4 25 %, channel 8 16 %, channel 12 3 %, and each group uses one or two variations. BOB reduces this to the RGBA `blend.dds` (the four group weights) and `index.dds` (which variation), the two maps `ps30_blend` reads (`s_blend_map` / `s_index_map` above).
- **Texture files**: `terrain/textures/battle/<climate lower case>/<group>/<variation>_<su|au|wi>_<diffuse|normal>.dds`, DXT5, 1024 x 1024, 11 mips, in the TDD weather pack and the vanilla terrain packs. The climates: `default` (all 18 groups, summer only), `rome_continental`, `rome_med`, `rome_atlantic`, `rome_desert`, `rome_easterndesert`, `rome_destruct`, `rome_destructdes`, `rome_grey`; a climate that lacks a group (rome_desert has no `moorland`) uses `default`; seasons: `su` always, `au` / `wi` in the main climates. The group names are those of the tile entries and of `_settings.xml`'s `<TEXTURE name=...>` lists.
- **Scale**: `_settings.xml` `<CLIMATE><TEXTURES><TEXTURE name blend_pixel_scale ...>`: metres per repeat of the texture inside the tile (rock 40.1, mud 9, mountain 1200.1, ...), what `g_blend_pixel_scale_array[slot].x` holds in the shader (uv = world xz / scale).
- **`ground_types.dds`** (L8, the tile without border, battle tiles only) is written by `TOOLDATABUILDER::convert_blend_map`
  (`ToolDataBuilderDll`, `TerrainBlendMap.cpp`). A pixel painted in `ground_type_map.png` (any colour but black) is that colour's ground
  type, `EMPIREUTILITY::ground_type_to_colour` (ARGB): forest 0 (64, 225, 231), grass 1 (0, 255, 0), mud 2 (150, 75, 0), sand 3
  (244, 164, 96), scrub 4 (172, 225, 175), rock 5 (170, 170, 170), deep water 6 (0, 0, 255), shallow water 7 (93, 255, 255), road 8
  (255, 96, 0), wooden floor 9 (255, 100, 100), snow 10 (255, 255, 255), sharp stones 11 (255, 255, 0), burnt 12 (64, 0, 0), none 13 black
  (every painted pixel of the kit's 33 painted tiles matches; the river tiles paint their water, the crossings their roads). Any other
  pixel the strongest of the four group weights (ties: red, green, blue, alpha in that order) through
  `EMPIREUTILITY::ground_type_from_texture_group` of the entry's `TEXTURE_GROUP` name: forest 0; grass, agri, settlement_1 1; mud,
  settlement_3 2; sand 3; rock 5; urban, settlement_2 8 (`Settlement_N` capitalised too); any other name 1 with an assert ("There is no
  ground type for texture group %s. See BenM."). The values are the `ground_types` table's `enum_value`. Checked on western_city_b/small
  from its DXT5 `blend.dds`: 99.2 % (the rest is the DXT error). The DDS files are written by `TOOLDATABUILDER::write_dds_ATI_compressor`
  (ATI's compressor: what raw -> binary has to reproduce).
- `ground_type_map.png`: RGBA 8 bit over the bordered tile, black but where a ground type is painted (the colours above); `protection_map.dds`: R32F 0 / 1 over the tile without border (128 x 128 for a 1 x 1 tile).


- Debug views (TEd has them): *blend groups* (the four group weights as flat red / green / blue / yellow), *ground types* (the PNG as stored), *protection* (red = 1).

## Seasons and snow (2026-10-18)

- **Seasons:** the game has four battle seasons (SEASON_SPRING, SUMMER, AUTUMN, WINTER; texture codes sp / su / au / wi). The texture
  array (fn 0x11821af0, filled by fn 0x1180cbf0 from `%s/%s/%s/%d_%s` and, for climate-change states, `%d_%s%c`) holds every season and
  climate-change state: the blend shader's slot is mapping + z * (x * (y * g_remapped_season + g_climate_change_index) + T2.y) with
  `g_array_section_sizes` (x, y, z). A missing texture falls back (fn 0x1180cb20) to lower climate-change states, then the climate's
  summer, then the `default` climate by the same rules: what the editor's `climate_texture_path` does (no vanilla texture has a
  climate-change suffix). The editor loads one season and climate (sections (1, 1, 0)); the tile look now offers spring too (trees ship
  spring textures; the ground has a few).
- **Detail maps:** the game looks for `medium_detail_map_<season>_*` and `medium_detail_map_snow_*` before the plain ones (fn 0x118209e0);
  no vanilla climate ships them, so one pair per climate is right.
- **Snow:** the weather node loads `commontextures/snow_normal.dds` as the global `s_snow_normals` (fn 0x118ce490); the editor now binds
  it in the rigid, decal, terrain-blend and terrain resolve passes (it bound black or the flat normal). The terrain resolve snows when
  `g_snowing_intensity` > 0 outside the campaign; leaves, branches and billboards likewise; rigid meshes also need the instance's
  POSITION5.w > 0 and decals POSITION5.z: the editor sets both to 1 (ASSUMPTION: the engine's per-instance value was not traced).
  `g_snowing_intensity` is the environment's WEATHER snowing_intensity (most files have 0; e.g. charlemagne/med_wi_land_day_snow_2 has 1).
  The campaign's `snow_mask_<season>.dds` (main_attila_map, cha_attila_map) are map-level.
- **Snow on the ground (2026-10-22, from the DXBC of `ps30_terrain_to_gbuffer_ss`):** the resolve reads `g_snowing_intensity` and `g_wind`
  of `weather_shared` (`g_season_type` and `g_snow_mask` are unused there). With intensity i > 0 it perturbs the normal with
  `s_snow_normals` (uv = xz * 0.0005 and * 0.07), and where the normal faces away from the wind (direction (wind.x, -1, wind.y)) beyond a
  threshold 1 - 0.7 * (min(1.25 i^2, 1) * 0.2 + 4 * sat(i^2 - 0.8)), sharpened by * 200 and a smoothstep, it lerps the colour to 0.4 grey
  and moves the other channels toward snow. So snow cover is the weather's, one value for the whole battle: none without falling snow.
- **The campaign's season maps** (kit `main_attila_map/snow_map_<sp|su|au|wi>.png`, 2032 x 1441 cells): black no snow, white snow, blue sea;
  only winter has snow (418 610 cells). They say where a battle can be snowy in a season (its weather), not a texture layer. The
  climates' `<v>_wi_*` textures are winter ground, not white (rome_med grass 0: summer (119, 117, 67), winter (117, 102, 77)).
- **Map-level, not in the tile's scope:** `definition.xml` gives the battle map's base terrain (2560 x 2560 for western_city_b/small); the
  ground beyond the tile, the tile's `final_alpha` feather into it, the far maps and the outfield come from the map (the later tilemaps
  scope). The tile's `outfield_mesh.rigid_model_v2` is a 49 x 49 low-resolution copy of the tile's own 1024 m.

## Shadow casting (2026-10-18)

There are no shadow-receiving terrain variants: the terrain receives in the deferred directional light like everything else. It casts
with its own technique, shadow_30_new = `vs30_shadow_new` + `ps30_shadow` (docs/re/renderer.md, "Shadow casting"): the same grid and
height map as geom_new, CULL_BACK, the shadow rasterizers' bias (2500, slope 1.8). The editor draws it into every cascade.

## TEd's blend painting and its Blendmap Filters (2026-10-26, static RE of `TEd.AssemblyKit.exe`, TEd not run)

- **The Blend Map paint module** (constructor `FUN_004867d0`, name `L"Blend Map"`) registers the `QTU::GridPainter` operations
  **Normal**, **Normal (Grouped)**, one more without a name string (`FUN_004833e0`), **Auto-Flange**, **Rock-painting** and **Spline
  splat** (below). The **Influence Map** module (`FUN_004891e0`,
  `L"Influence Map"`, `lf_influence_map.png`) has the operation **Subtract** among others. The operation without a name string is a
  blur: `QTU::GPOWeightedAverage` (each channel moves towards its neighbourhood's weighted average by the stroke's weight).
- **How an operation is applied** (`QtToolUtility`: `QTU::GridPainter`, `GPOBase`): an operation is a `GPOBase` with
  `apply(GridPaintSource const &, float weight, GridPaintTarget &)` (the pixel's 16 channels in, 16 out) and `brush_accumulator()`. The
  blend operations' accumulator (`FUN_00481560`) is `acc + (1 - acc) * falloff * strength`: during a stroke each pixel's weight builds up
  towards 1 with every dab, and `apply` gets that weight. Dabs are dropped every `spacing` (a GridPainter property) along the mouse path
  (`GridPainter::drop`). The Blendmap Filters mask the result (below).
- **Auto-Flange** (`Ted::PM2::OpBlend2AutoFlange`, `PaintMode2Operations.h`; `apply` = `FUN_00482f40`). Properties: `channel_group` (a
  texture group g) and `threshold` T (0 when made); a third value, the variations per group (4, inferred: the formula spans exactly the
  group's four channels). With the stroke weight w:
  - w > T: f = (w - T) / (1 - T); every channel is set to 0, then at p = 4g + 3f (between the group's variation 0 and 3) the two
    channels around p get 1 - frac(p) and frac(p). The pixel becomes all group g, its variation sliding from 0 (the brush's edge, the
    stroke's start) to 3 (its middle, after more dabs).
  - w <= T: when the group's variation 0 is under 0.99, every channel is scaled by (1 - w / T) / (1 - c0) and variation 0 set to w / T:
    variation 0 fades in.
  So a stroke paints a group with a graded rim: variation 0 outside, variation 3 inside ("flange"). It sets values from the stroke's
  weight; it does not add to them.
- **Rock-painting** (`Ted::RockPaintOperation`; `apply` = `FUN_004836b0`). One property, `channel` (any of the 16). It ignores the
  stroke's weight (the argument is overwritten unread): wherever the brush reaches, the channel is set to t(slope) and the other channels
  are scaled to make room (`(1 - t) / (1 - c)`; nothing is done where the channel is already 1). With s = 1 - n.y (n the terrain normal
  at the pixel, `clamp`ed), A = smoothstep(0.1, 0.7, s), B = smoothstep(0.3, 0.9, s) (smoothstep = (3 - 2x) x^2 of the clamped
  ratio), t = A (1 - B) + B: 0 below about 26 degrees, 0.01 at 30, 0.24 at 45, 0.81 at 60, 1 from about 72.5. The channel is set even
  where t is lower than what is there (it can take rock away on gentle slopes).
- **Spline splat** (`Ted::SplineBlendPaintOperation` on a spline operation base, `PaintMode2Operations.h`). Properties `spline` (one of
  the scene's splines), `apply` (an action) and `channel`. Its `apply` per pixel (`FUN_00485060`) is a plain paint: every channel times
  (1 - w), then the channel + w. Setting `apply` (`FUN_00484890`) walks the chosen spline's segments, builds a quad per segment from its
  two points and their widths (`FUN_00483950`), paints each through the painter, and makes one undo record: the channel laid along the
  whole spline at once, as wide as the spline.
- **Blendmap Filters** (`Ted::BlendFilterDialog`, `BlendFilterDialog.cpp`, constructor `FUN_005c3c00`, made by the Blend Map module): two
  checkable groups, both off when the dialog is made. **Height Filter**: Min Height / Max Height, -1000 .. 1000 (defaults -1000 and
  1000). **Slope Filter**: Min Slope / Max Slope in degrees, 0 .. 90, kept as `cos(min)` / `cos(max)`. Each min / max pair is held
  ordered (`FUN_005c4a90`..`FUN_005c4ba0`: moving one past the other moves the other). **Use No-Go Range** (`FUN_005c4a40`, also run
  when the dialog is made) sets the slope range to `EMPIREUTILITY::maximum_passable_slope_angle_degrees()` .. 90, i.e. only the slopes
  units cannot walk.
- **The filter is a 0 / 1 mask** (`FUN_004866f0`, called by slot 26 of the Blend Map and Influence Map modules' vtables): 0 where the
  height filter is on and the terrain's height at the point is outside [min, max], or the slope filter is on and the terrain normal's y
  is outside [cos(max), cos(min)]; 1 otherwise. No soft edge. It masks every operation of the two paint modules (it is the module's
  virtual, not an operation's). TEd's height is the terrain point's y (the scene's units).
- `MAXIMUM_PASSABLE_SLOPE_ANGLE_DEGREES` (EmpireUtility, `GroundTypeField.cpp`, "Sets the slope which defines no go regions"): a config
  variable, default **30** degrees.
