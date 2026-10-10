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

## What the tool does (Phase 4b, `libs/render/water_pass.cpp`, `tools/tile/tile_document.cpp` `rebuild_water`)

An own small pass, **not the game's shaders**: after the fog, before the post passes, alpha-blended into the HDR target with the scene's depth test. The same ingredients: the river's normal map in three scales scrolled along the flow, a Schlick fresnel
between the sky cube and a body colour (the environment's shallow -> deep colours by the water's thickness over the ground, lit by the sky's average and the sun), the sun's glint, opacity from the thickness (read from the G-buffer depth) with a soft bank,
foam at the banks from `foam_amount`. Rivers: the Bezier chains sampled to ribbons (one mesh per spline, `uv` = metres across / along); the sea plane: a quad over the bordered tile at y = 0. Landscape rivers are rebuilt on the terrain after a height stroke.

**Not done**: refraction of the scene colour, the engine's exact fresnel / specular / fog, the sea's detail layers and foam atlas, wind-driven scroll, ice in winter, water in the environment cube, the tile's `hf_water_map` and ground-type water, shadows on water, river ends blending into the neighbour tile's river, the spline editor (4c).
