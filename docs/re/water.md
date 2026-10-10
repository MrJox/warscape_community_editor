# Battle water: rivers and sea (spike, 2026-10-10)

Method as for `terrain.md`: the packs ship a GLSL translation of every shader (`fxc/glsl_sm_3_0_<file>.hlsl_<stage>_<entry>.glsl`) read through `vfs_probe`; the
tile data (`tile.ted`, `river_spline*.xml`, the tile database) measured on the kit. Nothing from the engine's code, nothing shipped in the repo.

## What the game draws

| Shader file | Entries | Role |
|---|---|---|
| `rigidriver.hlsl` | `vs30_main_battle_river` / `ps30_main_battle_river` (also `_river` for the campaign) | river ribbons: a mesh per river spline, drawn after the lighting, writing its own lit colour |
| `waterplane.hlsl` | `vs30_water_battle`, `ps30_battle_{low,medium,high,ultra}_water_quality`, `ps30_battle_envmap` | the sea / lake plane of a battle map (quality levels; `envmap` for the environment cube) |
| `ocean.hlsl` | `ps30_main_*_water_quality` | the campaign sea |

Textures (in the packs): `battleterrain/river/river_normal.dds`, `river_diffuse.dds`, `ice_diffuse_1/2.dds` (winter), `sea/water_detail_normal.dds`, `water_detail_weak_normal.dds`,
`combined_foam.dds`, `campaign_water_detail_normal.dds`.

### The river (`rigidriver`, read from the listing)
- **Vertex shader**: per-vertex `vsIn_P0` (position of the ribbon vertex, `.w` the across coordinate), `P1` (`.xy` texture coordinates, `.z` flow, `.w` fade), `P2..P4` the rows of
  the instance's 3x4 world matrix, `P5.x >= 1` flips the uv. The height is looked up in the map's low-res height map (`s_low_res_height_map`, `vs_height_map_cb`, the same four-texel
  manual bilinear fetch as the terrain's `vs30_geom`): the ribbon follows the terrain's LF height plus its own offset (`river_CB`: `data[0].w` is subtracted from y). So the game's river
  is positioned relative to the terrain, like a landscape-mode spline of the tile.
- **Pixel shader**: three taps of `s_normal` (`river_normal.dds`, two-channel normal in `.wy` like the terrain's) at uv scales (5.9 across, 177 along) x 0.75, x 1 and x 2, scrolled by the
  clock (`camera_VS_PS` row 26: 0.008 / 0.024 / 0.12 per unit) and by the wind block (`wind_PS`); the sum + (0, 1, 0), normalised, is the surface normal. Reflection: `s_environment`
  (cube) along the reflected view, weighted by a Schlick-like term (`0.15 + 0.25 * x^5` of the reflection), skipped when `lighting_VS_PS.data[11].y` is set. Refraction: `s_terrain_refraction`
  (the scene colour) fetched at the screen position displaced by the normal x the water depth (`min(depth * 4, 1)`), tinted by `pow(lighting colour, depth)`, with the depth taken from the G-buffer
  (`gbuffer_channel_4_sampler`) reconstructed through `camera_VS_PS`'s inverse view-projection rows. Sun: a Gaussian-like specular lobe (two terms, 0.85 / 0.15) times `sun_colour`; the whole result is
  multiplied by 0.004 when `g_hdr_on`. Fog (`fog_VS_PS`) is applied inside the shader: the output `color3` is the final lit colour, `color0` the packed normal (alpha 0.12), `color1` = (0, 0, 0, 0.15),
  `color2` = (1, 1, 1, 0.0157).
- The **sea plane** (`waterplane.hlsl`) is not read in this spike: it is the same family (normal-map detail layers, foam atlas `combined_foam.dds`, environment cube, refraction, shore fade from the depth).

## What the tile has

- **Rivers**: `SplineRiverInstance` elements of `tile.ted`, each a chain of `<point>`s: `position` (local to the instance, scene units = 2 per metre horizontally, y in metres), `controlIn` / `controlOut` (Bezier
  handles, local, absolute not relative to the point), `width`, `foam_amount`, `alpha_fade`, `flow_speed`, `terrain_offset`. A segment i -> i+1 is the cubic Bezier (position_i, controlOut_i, controlIn_i+1, position_i+1),
  confirmed against TEd's `river_spline<n>.xml` (the same four points per `<SPLINE>`, world metres, y = the point's world height; `width` there is half the file's: 1 for 2).
  - **Height rule** (measured on `tdd_custom_lothlorien`, `river_ford`, `dunedain_city_osgiliath`): a `MSM_FREEFORM` spline is at the height it stores (instance y + the point's local y); a `MSM_LANDSCAPE` one has local y 0 and lies
    **`terrain_offset` above the terrain under the point** (osgiliath: terrain at the river centre -7.7 m, `terrain_offset` 6 -> water at -1.7 m, banks at +1.1 m; lothlorien's creek 0.1 m above the ground).
  - **Width**: 2 in the small creek, 50 .. 120 on the big rivers. The carved bed is about 40 .. 80 m wide where `width` is 90 .. 120 (a profile across the heights): the tool takes `width` as the full width in metres.
  - The layer of the splines is hidden in some tiles (`flags=" OIF_ACTIVE"` for osgiliath's `rivers`): the water is still drawn, in TEd and here.
- **Sea and lakes**: the tile sets `sea`, `sea_coast`, `sea_coast_landing`, `lakes`, `river_mouth` have heights below 0 (lakes: min_height -17 .. -40 with scale 20 .. 43); the water surface is at y = 0 and the terrain cuts it. River tiles also go below 0 (the ford's heights are all <= 0) and have no
  plane: the rule is the tile set. BOB writes `hf_water_map.data` and the ground type map carries `GT_SHALLOW_WATER` / `GT_DEEP_WATER`; neither is read yet.
- Environment: `lighting` has `deep_water` / `shallow_water` colours and `refraction_scatter` / `sea_bed_scatter`, already mapped into `lighting_VS_PS` (`g_deep_water_colour`, ...).

## What the tool does (`libs/render/water_pass.cpp`, `tools/tile/tile_document.cpp` `rebuild_water`)

**The game's own shaders** (rule of 2026-10-10: everything rendered uses them, no look-alikes): `rigidriver.hlsl` `vs30_main_battle_river` / `ps30_main_battle_river`, `waterplane.hlsl` `vs30_water_battle` /
`ps30_battle_ultra_water_quality` (the "ultra" quality level) and `waterrefraction.hlsl` `ps30_main_3_channel`, the `dx11_sm_3_0_*` variants without the shadow inputs, bound through `ShaderLibrary` / `ConstantBuffers` by their reflected names
like every other pass. Frame order (as far as the shaders show it): the lit scene is copied at half resolution by `waterrefraction` into `s_terrain_refraction` (rgb = light buffer, alpha = `saturate(-ground_y * g_refraction_depth)`, 1 above y = 2, from the depth channel);
the **sea / lake plane is drawn after the light and before the fog** (its shader has no fog, the fog pass covers it: stencil 1); the **river is drawn after the fog** (its shader fogs itself). Both pixel shaders write four targets (G-buffer channels 1 .. 3
and the final lit colour into the HDR target), opaque, depth-tested and writing depth; the water thickness comes from the depth channel read in the shader, the refraction displaces by the normal x the thickness, the colour tints by `g_sea_bed_light_scatter` /
`g_refraction_light_scatter` and fades to `g_deep_water_colour`, reflection from the environment cube, sun glint. All of that is the shader's own; the tool only feeds it:
- **Inputs from the environment** (already mapped for the other passes): `lighting_VS_PS` (`sun_direction`, `sun_colour`, `g_deep_water_colour`, `g_shallow_water_colour`, `g_sea_bed_light_scatter`, `g_refraction_light_scatter`, `g_hdr_on`, `g_ssr_enabled` = 0 so the cube reflection is used), `fog_VS_PS`, `wind_PS`, `camera_VS_PS`.
- **Vertex data**: the river vertex is `P0 = (x, y, z, across)`, `P1 = (u, v, flow, fade)`, the instance stream is the identity 3x4 matrix; the shader's low-res height map contribution is switched off (`g_height_map_lf_scale` 0, a black map) because the mesh carries its own world height (the Bezier chain on the terrain / the spline's stored height, see above) and is rebuilt after a height stroke. The sea plane is a quad over the bordered tile at y = 0 with an identity instance.
- **Textures**: `battleterrain/river/river_normal.dds` (rivers), `sea/water_detail_normal.dds` (sea), the sky cube for `s_sky`, the environment cube for `s_environment`, the refraction copy, the depth channel. Samplers as in `docs/re/samplers.tsv` (normals and cubes linear wrap, refraction and depth point clamp).
- **Assumptions / not verified against the game** (no in-game reference capture yet): `g_refraction_depth` = 0.75 (what both engine constructors store, fn 0x116d45c0 and 0x117037e0, and what the live game's cbuffer dump shows: `directional_light_PS.g_refraction_depth` 0.75; the dump has `g_refraction_light_scatter` 0.6 and `g_sea_bed_light_scatter` (0.97, 1, 1) for that battle); the **uv convention of the engine's river mesh** (the shader multiplies `P1.xy` by 0.01 / 0.001 and flips with `1 - ...`, which looks like normalised coordinates; the tool puts metres across / along and so the ripple density differs from a river of another width); the river width (full `width` in metres); the shadow variants (`sHardShadowBuffer`: water receiving shadows) are not used; the sea's normal texture (`water_detail_normal`) is the one the battle plane is believed to use; the `ultra` quality level is fixed; ice in winter (`ice_diffuse`), foam (`foam_amount` has no input in the battle shaders), the tile's `hf_water_map` and the ground-type water are not read; the river ends have no fade (the shader reads none).
- **A lesson about the listings**: the packs' `glsl_sm_*` files are the GL path and differ from the D3D shaders the game runs on Windows in orientation details (the screen-to-NDC y flip is missing in them: `ndc.y = 1 - 2 v` in the DXBC). Where orientation or a constant matters, read the disassembly of the real `dx11_*.fxc` (`cli/fxc_dump <fxc dir> <out> <asm dir>`), not the GLSL.
