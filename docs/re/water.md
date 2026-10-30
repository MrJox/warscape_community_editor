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
  confirmed against TEd's `river_spline<n>.xml` (the same four points per `<SPLINE>`). The files' exact rule (x, z scene / 2, x 5 in a campaign tile; y scene / 1.25; width x 0.5; which point's values each POINT carries; old files left stale) is in `wce_tile::river_files`, checked by `examples/river_files_check.rs` (2026-10-25: all 148 rivers of today's TEd as TEd wrote them, 98 of them up to a last digit; the 99 old ones differ). The editor writes a tile's river files on save when a river changed.
  - **Height rule** (measured on `tdd_custom_lothlorien`, `river_ford`, `dunedain_city_osgiliath`): a `MSM_FREEFORM` spline is at the height it stores (instance y + the point's local y); a `MSM_LANDSCAPE` one has local y 0 and lies
    **`terrain_offset` above the terrain under the point** (osgiliath: terrain at the river centre -7.7 m, `terrain_offset` 6 -> water at -1.7 m, banks at +1.1 m; lothlorien's creek 0.1 m above the ground).
  - **Width**: 2 in the small creek, 50 .. 120 on the big rivers. The carved bed is about 40 .. 80 m wide where `width` is 90 .. 120 (a profile across the heights): the tool takes `width` as the full width in metres.
  - The layer of the splines is hidden in some tiles (`flags=" OIF_ACTIVE"` for osgiliath's `rivers`): the water is still drawn, in TEd and here.
- **Lakes and water planes (2026-10-18)**: lakes are not a plane at y = 0. Every `lakes` tile (22) has its water as
  `SIMPLE_MESH_INSTANCE`s of `RigidModels/Buildings/water_plane/water_plane.rigid_model_v2` (1,145 such instances in the kit, also in
  settlements, river starts, swamps and dunes), scaled over the lake (`scale="vector(1150,1,800)"`) and placed freeform at the lake's height
  (-6.9 m on lakes/10x10/aa). The model is one mesh of material `Water` (83) in vertex format Uk8 (12 bytes: f16 position, f16 uv); fountains,
  garden pools and the tannery carry `Water` meshes too. `waterplane.hlsl` `vs30_water_battle` reads only the position and the instance's world
  rows (P1..P3; the ripples' uv are world x / z * 0.01 scrolled by the clock), so the tool draws these meshes with the sea plane's shaders in
  the sea pass, from the G-buffer pass's visible instances. The D3D shader declares TEXCOORD0 and POSITION4..6 as well (the input layout
  must provide them).
- **Sea**: the tile sets `sea`, `sea_coast`, `sea_coast_landing`, `river_mouth` have heights below 0 (lakes: min_height -17 .. -40 with scale 20 .. 43); the water surface is at y = 0 and the terrain cuts it. River tiles also go below 0 (the ford's heights are all <= 0) and have no
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
- **Settled 2026-10-27** (disassembly of the D3D shaders with vkd3d-compiler; the game's BOB port in `wce_tile_io::attila::river`):
  - **The river's uv**: `vs30_main_battle_river` writes `uv = P1.xy * (0.01, 0.001)` (flipped as `1 - uv` when the instance's
    `v5.x >= 1`), and `ps30_main_battle_river` reads only the position (`v1`) and that uv (`v4.xy`, times (5.9, 177) for the normal
    map). BOB's river vertex (the meshes' type-90 block) is `x, y, z, flow, width across (m), distance along (m), alpha, across (0..1)`:
    as two float4s, `P1.xy` = metres across and along, which is what the tool feeds. The flow, alpha and across fraction reach the
    interpolators (`o7`, `o4.zw`) but the battle pixel shader reads none of them: **no fade at the river's ends and no foam** in a
    battle (the campaign variant `ps30_main_river` is another shader).
  - **Shadows on water**: the packs ship `enable_shadows_*` variants of `ps30_main_battle_river` and `ps30_battle_ultra_water_quality`
    with the same inputs plus `shadowmap_PS`, `g_tShadowBuffer_SM50` and a comparison sampler; with shadows on, the tool uses the
    `hardware_gather_5_0_sm_5_0` ones (the variant of the game's lighting, `DIR_LIGHT_PS`): the water takes the shadows of what stands
    over it (a bridge's on its river).
  - `g_refraction_depth` 0.75: the engine's constructors and the live game's cbuffer dump agree.
- **Settled in game (2026-10-29)**: **winter rivers do not freeze in a battle**. A screenshot of a snowing winter battle on a river map
  shows the river as open water: dark, rippled, with sun glints, and the snowy bed showing through where it is shallow along the
  banks. There is no ice sheet and no ice texture, so battles do not use the ice mode below (mode 1). The editor's river, the same in
  every season, is right. Nor can a mod turn it on (below).
- **Ice in winter (the engine)**: No battle water shader (river, water plane) has an ice input. The engine loads
  `BattleTerrain/river/ice_diffuse_1` and `_2` in the constructor fn 0x116c98a0 (empire.retail.dll 1.6.0; Ghidra project
  `~/wce_re/game_proj`), into +0xd4 / +0xd8 of an object whose base (fn 0x116d9190, material type 0x58) is made with "river" and
  `BattleTerrain/river` and a rectangle. That object is the second of three modes of a water display of the **Terrain Node** (class made by
  fn 0x118c9e10, type id 0xaba7777 in the component factory fn 0x117378d0): fn 0x11924ac0 keeps at +0x7d0 (+2000) nothing (mode 0), the
  ice object over the node's bounds (+0x794; mode 1), or a `campaign_water_lf` object (`terrain/textures/common`, mode 2).
  **Settled (static analysis, 2026-10-29): the mode is never set, and mode 1 is not ice.** The setter is slot +0x58 (entry 0x11ccdf04)
  of the node's primary vtable 0x11ccdeac (the base 0x11ccdcf4 has purecall there; 0x11ccdcd0, named here before, is the vtable of the
  +0x50 "Node Entries" sub-object). The display at +0x7d0 is zeroed in the ctor, written only by the setter, and drawn by slot +0x24
  (fn 0x1190c610, render pass 1). Nothing calls the setter: not the users of the node's getters (fn 0x10391b80, 191 call sites;
  fn 0x116c0660, 37; fn 0x1174e810), not the node's own code, and none of the DLL's 730 `call [reg+0x58]` sites with a 0 / 1 / 2
  argument; the campaign's `campaign_water_lf` plane comes from other components (0xaba7778 / 0xaba7779, through fn 0x117bfd10). The
  only "ice" strings are the two texture paths (no script or console binding). The ice object's draw (fn 0x1177f610) feeds
  BATTLE_WATER_PLANE_SHADER (shader manager +0x1131c, fn 0x116c99d0: `waterplane.hlsl`, `ps30_battle_*_water_quality`), so mode 1
  would be a flat sea-shader quad over the node's bounds; `ice_diffuse_1 / _2` have no sampler in that shader. Each mode also writes a
  float to `FUN_117b6660(scene)+0x1b4` (1.333 for mode 1, 1/36 for mode 2; meaning unknown). Most likely dead code. **Not moddable**: no
  db table, tweaker, script, battle setting or pack file reaches it; frozen battle rivers would need a new shader and engine code.
  Battles do not use mode 1 (checked in game, above). The `ultra` quality level is fixed; the sea's normal texture
  (`water_detail_normal`) is the one the battle plane is believed to use.
- **A lesson about the listings**: the packs' `glsl_sm_*` files are the GL path and differ from the D3D shaders the game runs on Windows in orientation details (the screen-to-NDC y flip is missing in them: `ndc.y = 1 - 2 v` in the DXBC). Where orientation or a constant matters, read the disassembly of the real `dx11_*.fxc` (`cli/fxc_dump <fxc dir> <out> <asm dir>`), not the GLSL.

## The battle sea of a coastal battle (2026-10-19: decompiled empire.retail.dll + a memory dump)

The editor's sea plane (waterplane `ps30_battle_*_water_quality`) has no foam input, but the game draws a band of breaking-wave foam
along the shore (in-game capture of a coastal battle, 2026-10-19). What the game actually draws there:

- **The Sea Display** (class name `SEA_DISPLAY`, ctor fn 0x116f0c20, vtable rva 0x1ca6404; created for battles by fn 0x11770ca0 with
  its fourth argument 1, for the campaign map by fn 0x1190cd40). It loads `sea/combined_foam.dds` (slot 0x18) and
  `sea/water_detail_normal.dds` (slot 0x19; `sea/campaign_water_detail_normal.dds` on the campaign map), creates a 512 x 512 texture of its
  own (slot 0x17: formats 0x2b / 0x35, fn 0x11700e90; believed to be the **sea bed info** the vertex shader samples) and three float grids
  (slots 0x1f-0x2f: 256 x 256, 128 x 128, 256 x 256, sized from its settings at +0x70 / +0x74). In battle it also builds a
  **`RADIAL_MESH`** (fn 0x116e47e0, vtable rva 0x1ca62c4): 80 segments around, rings from its settings, the sea's surface mesh.
- **The shader is `ocean.hlsl`** (sm_4_0; registered next to the ocean2 materials, strings at rva 0x16e0a77 .. 0x16e0b5a; its samplers in fn 0x116e1f20: `vs30_main` + `ps30_main_{ultra,high,medium,low}_water_quality`,
  `main_prefill_40`), not waterplane. Proof from the dump: the only outside reference to the Sea Display's combined_foam texture is a
  material texture table whose slots are `g_foam_anim_texture` (Sea Display slot 0x14), `g_sea_surface` (0x15), `g_swell_surface` (0x16),
  `g_static_foam_texture` (combined_foam), `g_reflection_texture`, `g_refraction_texture`: ocean.hlsl's inputs. The vertex shader also reads
  `sea_bed_info_texture` and `ocean_VS`; the pixel shader `OceanSharedConstants`, the detail normal, the colour overlays.
- **No SeaV2 in a coastal battle**: the `ocean2.hlsl` coast ocean (component `SeaV2`, id 0x0bba1356, ctor fn 0x116f17d0, vtable rva
  0x1ca2d38: `terrain/ocean/coast_foam_mips`, `land_1024_coast`, `coast_amplitude_mask_1`) has no instance in the dump; one Sea Display and
  one radial mesh do.
- **The surfaces are generated at runtime**: no sea surface file or table ships (`SeaSurfaces.pack`, `sea_surfaces_table` are named in
  the DLL but not in the packs); the engine's strings name a Phillips-spectrum wave simulation (`swell_phillips_constant`,
  `swell_num_divisions`, `swell_horz_scale`, `swell_wind_speed`, `swell_repeat_time`, `swell_fps`). Not traced yet.
- **Live constants of the ocean material** (the dump, a coastal battle, ultra quality): `ocean_VS`: `g_sea_decay` 0.0006, `g_sea_uv_scale`
  0.0142857 (1/70), `g_swell_uv_scale` 0.00125 (1/800), `g_vs_sea_info` (-20, 25, -0.117621, 0.239849). `OceanSharedConstants`:
  `g_froth_distortion_speed` 0.35, `g_froth_distortion_amount` 0.05, `g_lowest_wave_trough_y_pos` 0, `g_ps_sea_info` (-20, 25, -0.117621,
  0.239849), `g_detail_normal_uv_scale1/2` 15.8 / 5, `g_detail_normal_uv_speed1/2` 7.8 / 6.8, `g_transparency_exponential_scale` 4,
  `g_foam_anim_data` (33, 34, 0.650017, 0) (two animation frames and the blend between them, it seems). The waterplane materials have their
  own copies of `OceanSharedConstants` (other values: 1 / 1 / 0, all zero).
- **Still to trace** (the surfaces, foam animation and constants were traced 2026-10-20, below): how the sea bed info texture is filled
  and the radial mesh's vertex layout and placement.
- How this was found (reusable): string VAs scanned in .text, Ghidra headless decompiles (`C:\Users\frodo\ghidra_work`), and a minidump
  read with a small reader (module list + Memory64 list): count heap objects by vtable, follow pointers from known objects, and read
  variable objects (+0x08 name, +0x14 slot in the cbuffer's CPU copy).
- **What the ocean shaders do** (DXBC disassembly of `dx11_sm_4_0_ocean.hlsl_vs_vs30_main` and `_ps_ps30_main_ultra_water_quality`):
  - Vertex input: `POSITION0` the displaced surface point (y = the wave height), `POSITION1` the rest point (its xz give the uv:
    `* g_sea_uv_scale` and `* g_swell_uv_scale`; projected at the water level `g_vs_sea_info.z` for the reflection / refraction
    coordinates), `TEXCOORD0.x` passed on (the pixel shader doesn't read it). No world matrix: the vertices are in world space.
  - `sea_bed_info_texture` covers x, z in [-1024, 1024] (`(x + 1024) / 2048`; outside it the point keeps its own height); red is the
    ground height, `r * g_vs_sea_info.y + g_vs_sea_info.x` (-20 .. 5 m). Over shallow ground the waves blend into a small ripple
    (`-(sin(0.02 x + t) + sin(0.02 z + t) + sin(0.04 x + 2 t) + sin(0.04 z + 2 t) + 4) / 8`, by `saturate((ripple - ground) * 0.05)`), and
    where the ground is above 0 the point is pushed down to -4 m (`saturate(2 * ground)`), under the terrain.
  - The normal: two detail normal samples (`g_detail_normal_uv_scale1/2`, scrolled by time * `g_detail_normal_uv_speed1/2` and fixed
    factors) combined with the sea and swell surfaces, which hold **angles** ((x - 0.5) * 2 pi, sin / cos): 0.5 is a flat surface.
  - **Foam**: the water depth comes from the depth channel (the reconstructed ground point under the pixel). Whitecaps: the animated
    foam array (slices `g_foam_anim_data.x / .y` blended by `.z`, at the sea uv), faded over 1000 m, sharpened with the static foam at
    uv * 15. **Shore foam**: the static foam texture at uv * 7.5 scrolled by +- time * 0.05, both layers multiplied, where the depth is
    under 1.6 m (`1 - saturate(depth - 0.6)`); the whole foam fades out in the last 0.6 m (`min(depth * 1.667, 1)`). The foam colour is
    (0.4, 0.5, 0.6) lit by the sun and the ambient, mixed with the colour overlay.
  - Outputs as waterplane: the G-buffer channels 1 .. 3 (normal, w 0.12; (0, 0, 0, 0.02); (1, 1, 1, 0.0157)) and the lit colour, opaque.
- **The sea's surfaces and whitecaps (2026-10-20)**: no wave simulation at run time. The battle's wind (a key of `wind_levels_tables`:
  calm / light / moderate / strong / storm_westerly, read from `BATTLE_ENV + 0x6410c` by fn 0x102d0d40) names a row of
  `sea_surfaces_tables` (`wind_level_0` .. `_4`), and fn 0x106520a0 loads that row and two prebaked files from data.pack's root,
  `<row>_sea` and `<row>_swell` (fn 0x116889f0 / 0x1167fcd0; fn 0x11688b00 can regenerate them from Phillips-spectrum settings, a
  developer path). The file: magic 0x12345678, version 1, n, the patch size, the loop length, the frame count, two flag bytes, the height
  range (min, max), the u16 range of the two angle channels (min x, min z, max x, max z), the range of two slope channels, then per frame
  a height image (1 byte a texel), an angle image (2 bytes), a slope image (2 bytes; empty for the swell), each `w, h` first, and two record
  lists: 16 bytes `(x, y: i32, foam now, foam at the next frame: f32)` (the whitecaps; none on the calm levels) and 8 bytes (always empty).
  The sea is 256 x 256 over 70 m, 100 frames in 10 s; the swell 128 x 128 over 800 m, 60 frames in 30 s (so `g_sea_uv_scale` 1/70,
  `g_swell_uv_scale` 1/800, and `g_vs_sea_info.zw` = the sea's height min and max - min).
  - Per frame (fn 0x117693a0 and the jobs fn 0x11794880 sets up): the frame pair at the time (fn 0x1169a360: `t mod loop * frames /
    loop`, its integer part, the next frame modulo the count, the fraction), and each surface's texture (R16G16) is the angle bytes of both
    frames decoded `min + byte * (max - min) / 255` and blended. Checked against the dump: the sea grid is frame 33 -> 34 at 0.65, equal to
    one u16 on every row the dump did not catch mid-update; the swell fits the same clock.
  - `g_foam_anim_data` = the sea's frame, next frame and blend (the dumps: (33, 34, 0.65) and (89, 90, 0.59)): the whitecap array has a
    slice per sea frame. It is baked when the sea is created (fn 0x11769000): 300 steps of loop / frames (three loops), each stamping the
    frame's records `foam = max(foam, a + (b - a) * w)` into a float grid (fn 0x11723000, with spray flags at 0.75 / 0.2 for the VFX), then
    every cell above 0.001 is multiplied by the row's `foam_decay_rate` and written as `round(foam * 255 + 0.5)` (fn 0x11719330); red is the
    blended height bytes. Slice = the sea frame, so the third loop's (warmed up) foam remains. Only the levels with `foam_enabled` have
    records (strong and storm).
  - The rest of `OceanSharedConstants` / `ocean_VS` from the row: `froth_distortion_*`, `sea_decay`, `wave_trough_y_value` (=
    `g_lowest_wave_trough_y_pos`), and the four detail normal columns in another order than RPFM's names (RPFM scale1, speed1, scale2,
    speed2 = 7.8, 6.8, 15.8, 5 give scale1, scale2, speed1, speed2 = 15.8, 5, 7.8, 6.8 in the dump). `g_transparency_exponential_scale`
    (4) is not in the row.
  - The sea bed info texture the ocean material binds (`g_sea_bed_info_texture`, sampler `sea_bed_info_texture_sampler` linear clamp)
    is not the Sea Display's own 512 x 512 texture (slot 0x17: nothing binds it) but the handle in its slot 0x1a: a 1024 x 1024 texture
    (2 m a texel over the shader's 2048 m) loaded from a memory-mapped file (its handle's release fn 0x116502a0 unmaps a view). Which file
    is not settled: the likely one is a 2048 m cut of the battle map's `terrain/battles/<map>/lf_sea_height_map.dds` (16-bit, one
    channel; 31 MB for main_attila_map), but its content is GPU-only, so neither the code nor a CPU memory dump shows it.
  - Not found in the code: what fills the radial mesh (80 segments of a sin / cos table, 164 rings; `[0x24]`, `[0x25]` = 43.2, 2.72 at
    the dumped camera). The capture below shows the mesh it makes and the sea bed texture.
- **The editor (2026-10-19; 2026-10-20 the surfaces, whitecaps and wind)**: `crates/wce_render/src/renderer/ocean.rs` (the files:
  `crates/wce_render/src/sea_surface.rs`) draws the sea planes with these shaders, the wind's table row and prebaked surfaces, the
  whitecap array, `combined_foam.dds` and `water_detail_normal.dds`. Stand-ins: a quadtree grid instead of the radial mesh, displaced by
  the blended sea and swell heights; the sea bed texture built from the scene's terrain heights (512 x 512, encoded for the shader's
  `r * 25 - 20`; it only changes how the waves flatten near the shore).
- **Settled by a RenderDoc capture (2026-10-30)** of a coastal battle (sea_coast/2x2_bb_straight at its campaign placement, cell
  (244, 101), sea surface wind_level_0, camera 63.9 m up; `D:\renderdoc\coast.rdc`, RenderDoc 1.46). RenderDoc runs out of the 32-bit
  game's address space (E_OUTOFMEMORY while loading) unless the game uses less: only the launch pack as mod, textures low, "unlimited
  video memory" off; the capture was taken with every graphics setting at its minimum, so the mesh's size and the sea bed
  texture's resolution may be the low setting's (not checked at ultra). The ocean is two draws of the same mesh (a depth pass, event 897, then the colour pass, 914), 78 000 indices:
  - **The radial mesh**: 13 120 vertices = 80 spokes x 164 rings, spoke-major (index = spoke * 164 + ring), 28-byte vertices
    (POSITION0 float3, POSITION1 float3, TEXCOORD0 float). The spokes are every 4.5 degrees all round, and they start at one centre that
    is not under the camera: on the sea 122.6 m ahead of it horizontally (1.92 x the camera's height; one capture, so the rule is
    not settled). Every spoke has the same ring radii (m): 0, then steps of 0.5 m to 32, 1 m to 64.5, 2 m to 96.5, 4 m to 128.5,
    8 m to 160.5, 16 m to 224.5, 32 m to 352.5, 64 m to 608.5, 128 m to 1120.5, 256 m to 2144.5, 512 m to 4192.5, 1024 m to 8288.5,
    2048 m to 28 768.5. POSITION0 and POSITION1 differ by at most 0.04 m and share their y (-0.28 .. 0.23 m): the CPU gives the
    vertices only a small height, the waves are the vertex shader's (from the sea and swell surfaces). TEXCOORD0 is 1 out to
    192.5 m, then 0.968, 0.934, 0.859, 0.774, 0.678, 0.573 at 208.5 .. 352.5 m, and 0 from 416.5 m. The vertex shader only passes it on (COLOR0.z), and the ultra-quality pixel shader
    nor do any of the 24 ocean pixel shaders (every water quality, with and without shadows) read COLOR0: nothing to port (the
    editor writes 0).
  - **The sea bed info texture** (VS t0): 1024 x 1024 B8G8R8A8, grey (r = g = b), alpha 255: the ground's height around the camera,
    `r * 25 - 20` (black -20 m: open sea; white +5 m: land), the shore and the map's tiles visible as blocks. So 2 m texels over the
    shader's 2048 m square. The editor builds its own at that size since (it was 512 x 512).
  - **Constants**: `ocean_VS` g_sea_decay 0.0006, g_sea_uv_scale 0.014286 (1 / 70), g_swell_uv_scale 0.00125 (1 / 800), g_vs_sea_info
    (-20, 25, -0.11762, 0.23985); `OceanSharedConstants` froth distortion speed 0.35 / amount 0.05, lowest wave trough y 0, g_ps_sea_info
    as the VS's, detail normal uv scales 15.8 / 5, speeds 7.8 / 6.8, g_transparency_exponential_scale 4, then 19, 20, 0.37.
  Read with `scripts/re/rd_ocean_dump.py` (run by `qrenderdoc --python`).
