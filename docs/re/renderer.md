# Battle renderer: passes, shaders and findings

Status legend: **verified** (disassembly of the shader and/or empire.retail.dll), **assumption** (to check against a memory dump with `scripts/re/dump_cbs.py`), **todo**.

## Frame (as implemented in `src/render/renderer.cpp`)

| # | Pass | Game shader(s) | Notes |
| --- | --- | --- | --- |
| 1 | Sky cubemap | `skydome.hlsl` vs30_main + ps30_skygen | verified: engine renders the sky into a cubemap ("EnvMapGen", fn 0x18e00e0). 256² RGBA16F here (size: assumption) |
| 2 | G-buffer | `rigidcommon.hlsl` vs30_main + `rigidstandard.hlsl` ps30_gbuffer / _alpha_test | 5 targets, layout below |
| 3 | Sky (forward) | `skydome.hlsl` ps30_main | samples the sky cube; below-horizon directions clamp to the horizon |
| 4 | Directional light | `directionallight.hlsl` ps30_slm_highdef (shadows_hardware_gather_5_0, sm_5_0) | full-screen, stencil = geometry. Ambient cube + env. specular + sun |
| 4b | Fog | `battlefog.hlsl` ps30_main | over geometry (stencil), alpha-blended (assumption); sky cube + sun-lit fog colour |
| 5 | Luma stats | `hdr_tone_mapper.hlsl` downsample_image_store_luma, hdr_luma_to_quarter_size_luma_stats, downsample_quarter_size_luma_stats | the "quarter" image, a fixed 256 x 256 (rgb + luma), then a min/max/avg pyramid halving to 1x1 |
| 6 | Exposure | determine_black_and_white_points | 1x1 ping-pong; `prev + (target-prev)*(1-speed)^(30*dt)`; read back (4-frame ring) into `constant_tone_mapping_buffer` |
| 7 | Bloom | high_pass_luminance_cutoff, blur_x, blur_y | 256 x 256, 11-tap |
| 8 | Tone map | constant_tone_mapper_with_bloom | linear output; S-curve LUT (see below) |
| 9 | To screen | `hdr_to_screen.hlsl` ps30_main_high | fog of war (off), `g_brightness`, gamma, then `colour_matrix` in gamma space |

Not yet: shadow cascades (split count 0 = unshadowed), SSAO (white), clouds (`cloud_plane.hlsl`), sun (`sun.hlsl`), precipitation/VFX, DOF, terrain/ground.

## G-buffer (from rigidstandard ps30_gbuffer and directionallight)

| RT | Content | Format |
| --- | --- | --- |
| 0 `gbuffer_channel_1` | normal / max(abs) * 0.5 + 0.5, .w gloss | RGBA8 (assumption) |
| 1 `gbuffer_channel_2` | sqrt(albedo), .w mask | RGBA8 (assumption) |
| 2 `gbuffer_channel_3` | specular colour, .w material id / 255 | RGBA8 |
| 3 `gbuffer_channel_4` | z / w | R32F (assumption) |
| 4 | per-instance value (rigidcommon VS t0 lookup) | R32F (assumption) |

Render target formats, verified later (2026-10-08, the dumps): G-buffer A8R8G8B8 and R32F, HDR A16B16G16R16F, SSAO R16F.

Colour textures (diffuse, specular) are sampled through sRGB views (assumption: the sqrt/square packing only makes sense for linear albedo).

## Vertex/instance input (rigidcommon vs30_main), verified

Vertex format 0 (32 bytes): POSITION0 half4, TEXCOORD0 half4 (uv0, uv1), NORMAL0/1/2 = normal/tangent/bitangent as BGRA8 (`*2-1` in the shader; the spec's "X/Z swapped" is the BGRA order), COLOR0 BGRA8.
Per instance POSITION1..7 (float4 each): rows of the 3x4 world matrix, then PS inputs: POSITION5.w snow layer, POSITION6.y damage brighten, .z desaturate, POSITION7 faction colour override (.w index).
Matrices: clip.i = dot(pos, row i) — row-major upload, column-vector maths.

## Engine-side transforms

| Value | Status | Source |
| --- | --- | --- |
| `skydome_PS` colours | **verified** (3 dumps): fn 0x18e3700 computes pow 2.2 * scale * min(L, 5) * 5000 / 1200 into the sky node, fn 0x1923b50 copies it. L = LIGHTING `cubemap_sun_colour_scale`, or `sun_colour_scale` when that is 0 (fn 0x17594b0 on the lighting block at scene+0x1d64: +0x34 sun_colour_scale, +0xc0 cubemap_sun_colour_scale). 5000 = engine float var (dll+0x2a37730)+0x48 = 25000 * 0.2 when dll global +0x71658 > 0 (HDR), else 1 |
| `lighting_VS_PS.sun_direction/sun_colour` | verified: `set_sun(dir, colour)` normalises dir, copies colour (fn 0x178a230); callers not yet traced |
| sun azimuth/elevation convention | **assumption** (azimuth from +Z towards +X) |
| ambient cube (custom) | **verified** (rain + snow dumps): pow 2.2 * ambient_cube_hdr_scaler, no ambient_scale; lr = (left, right), tb = (top, bottom), fb = (front, back) |
| `luma_stats_buffer` | verified by names: absolute/max black, min/absolute white 1:1 from HDR |
| `colour_matrix` | **verified** fn 0x172c100: Overlay * Hue * Saturation(Haeberli 0.3086/0.6094/0.0820) * Contrast(pivot 0.5) * Brightness(x engine factor [this+0xe0]); hue rotation sign layout assumption |
| S-curve LUT | **verified** 256x1 RGBA16F: per channel the cubic Bezier (0,0), (1/3, low_tones_bias_c), (2/3, high_tones_bias_c), (1,1), 5120 samples binned to the nearest texel (fn 0x174a780, upload fn 0x17940b0, biases from the HDR block in fn 0x17284a0). lock_rgb_relative_tones only links channels in the engine debug panel |
| colorimetry gamma | assumption 2.2 (graphics option) |
| **HDR light scale** | **key open item**: with `g_hdr_on` the lighting and fog shaders multiply light colours by 0.004 (1/250); ambient by 360*0.004. So the engine's `sun_colour` constant is ~250x the plain linear(colour)*scale. Exact factor: memory dump |

## Building mesh materials (2026-10-18)

LOD 0 meshes of the 2,279 models under `rigidmodels/buildings/` by material: DefaultMaterial 17,349, TiledDirtmap 2,842, TerrainBlend 699,
ShipAmbientmap 530 (all drawn before), and:
- **Dirtmap** (8 models: `norse_statue_medium`, placed 223 times, and the `rock_desert_*` rocks): `rigidwithdirtmesh` G-buffer shaders
  (`ps30_gbuffer`, `_alpha_test`) on rigidcommon's vertex shader. rigidstandard plus `s_decal_dirt_map` at uv * `g_uv2_tile_interval`
  (`rigid_config_PS` +32, the material's floats 0 and 1, as for TiledDirtmap) + the instance's POSITION5.yz (0: not set by the tool),
  faded by `s_decal_dirt_mask` at the uv: the dirt map's xy bends the normal and its w times the mask's darkens the colour towards
  (0.03, 0.025, 0.02) and dulls the specular. Textures: `DecalDirtmap`, `DecalDirtmask`. Drawn (MeshKind::Dirtmap).
- **ProjectedDecalV2 / V3** (176 meshes in 135 models, all in damaged `destruct02+` pieces: rubble): an 8 corner box, the material's name
  the texture base (`rigidmodels/buildings/textures/rubble_1` -> `_diffuse.dds`, `_normal.dds`), `decal_center` / `decal_size`; V3 boxes
  are turned about y by the angle in the material's `uk_4` (1.2217 rad = 70 degrees for eastern_agora_2storey_building_01). Corners: 0..3
  the top face (0 -> 3 along x, 0 -> 1 along z), 4..7 below them (all 176). Drawn by the decal pass (rigidprojecteddecal `ps30_diffuse`) at
  the building's world matrix times the box, opaque, not tiled. The game registers `main` (pass 2) and `main_bugged` (pass 8, `ps30_diffuse_bugged`:
  the box axes not normalised) techniques; which one a model decal takes was not traced. The decal also covers what is next to the ruin
  inside its box (a neighbour's roof), as the shader only fades by how much a surface faces the box's y axis.
- **Uk54 / Uk56 / Uk57** (7 models: `carthage_cothon_ring_v6`, `greek_agora_*`, `greek_statue_*`): the same geometry four times, no
  textures, a `texture_directory` that is not in the packs, and `filters` `Dry=On` / `Snow=On` (54), `Burning=On` (56), `Rain=On` (57)
  (the engine's STANDARD_RIGID_NODE_FILTERS). Not drawn; only `greek_statue_hera` is placed (once).
- **Weighted** (the battering rams' `_weighted_anim` models, placed by no tile): skinned. Not drawn.
- **Water** (Uk8 vertices: `garden_fountain`, `garden_pool_1`, `tannery`, `water_plane`): with the water planes.

Note: the editor draws a building at its `destruct01` pieces unless its destruction level is edited in the Property Editor; the
`destruction_level` saved in tile.ted (321 damaged decal buildings on eastern_city_a/medium) is not applied when the tile opens.

- Placeholder textures (verified 2026-10-18, the material resolver fn 0x1186c880, every rigid material): a texture path containing
  `/test_gray` loads `CommonTextures/default_grey`, `/test_black`, `/test_mask` and `/test_gloss` `default_black`, `/test_white`
  `default_white` (`_srgb` in colour slots), `/flatnormal` `default_normal`. The placeholders' own files differ: flatnormal.dds is an
  RGB (128, 128, 255) DXT1 normal, which the shaders' `.wy` decode turns sideways. That made the terrain-blend decks of bridges
  (vertex alpha 0, but `ps_terrain_material` adds its unit normal to the terrain's accumulation unweighted, as the game's ONE / ONE
  blend does, fn 0x116caef0) show bands of tilted normals.

## Point lights (2026-10-18)

- **Data.** `POINT_LIGHT_INSTANCE` (5.4k in the corpus; 1,141 on western_city_b/small, mostly `window_orange_01` groups): radius,
  `colour="argb(r,g,b,a)"` (r, g, b, a order despite the name: 1.0, 0.596, 0.459 is warm), colour_scale (`[0, 5]`), colour_min,
  falloff_type `WPLFT_SMOOTH` / `WPLFT_LINEAR`, animation_type `LAT_NONE` / `LAT_RADIUS_SIN` / `LAT_RADIUS_SIN_SIN`, random_offset, animation_speed_scale.
  BOB packs them into `point_light_marker.markers`: UTF-16 headers `BASE_MARKER_REPOSITORY` / `POINT_LIGHT_MARKER`, u32 3 at 0x88, the count at
  0x8e, records from 0x92: f32 x, y, z, radius, r, g, b, colour_scale; a byte (1); u16 + `LAT_*`; 4 f32 (animation_speed_scale x, y, colour_min,
  random_offset); u16 + `WPLFT_*`. Radius, colour and scale are the tile's values unchanged. The positions are in another frame than the
  editor's world: the first light at editor (434.588, 11.107, 623.643) is (434.59, 22.21, 400.36) in the markers (x equal, y doubled,
  z mirrored; the game puts the y on the ground height, see Map lights). Not resolved further, as the editor draws from tile.ted.
- **Shaders** (`PointLightSmoothFalloff.hlsl` / `PointLightLinearFalloff.hlsl`, registered by fn 0x116f1a30 through fn 0x116e3bc0 with
  the `_Shadow` variants and the `slm` / `skinlm`, high / low def, `_ms` techniques): vs30_main draws POSITION0 * POSITION2.w +
  POSITION2.xyz (POSITION2 = centre, radius) and passes POSITION1 (the colour) on. ps30_slm_highdef rebuilds the world position from the
  depth channel (`point_light_PS.g_viewport_offset_scale`, as the decals), then attenuation = saturate(1 - d / r), squared when smooth; the
  directional light's diffuse and specular from the G-buffer, times SSAO. With `g_hdr_on` the diffuse is multiplied by 0.004 (the sun's
  1 / 5000 scale); without it the specular by 273.
- **Pipeline** (fn 0x116e3bc0's state objects): blend ONE / ONE add, write mask rgb (the alpha keeps the directional light's refraction
  weight); rasterizer cull mode 2 (front faces culled: the inside of the volume is drawn).
- **Map lights** (verified in the code). The battle loader (fn 0x10dcc6f0) reads the tile's marker files into a marker container (fn
  0x10ab5e90), transforms them by the tile placement (fn 0x10de4470: positions by its matrix, the point lights' radius by its scale), and
  fn 0x10407e90 turns each point light record into a runtime light. The record (0x40 bytes, read by fn 0x10ac38b0): +4 position, +0x10
  radius, +0x14 rgb, +0x20 colour_scale, +0x24 the LAT enum (`LAT_NONE` 0, `LAT_RADIUS_SIN` 1, `LAT_RADIUS_SIN_SIN` 2), +0x28 / +0x2c
  animation_speed_scale, +0x30 colour_min, +0x34 random_offset, +0x38 the WPLFT enum (`WPLFT_LINEAR` 0, `WPLFT_SMOOTH` 1), +0x3c the
  file's byte (1 everywhere; it picks which ground height the y is put on, fn 0x10db8260). In the file the four floats after the LAT
  string are (speed x, speed y, colour_min, random_offset): the two `colour_min="0.2"` lights of western_city_b/small are (1, 2, 0.2, 2).
  BOB writes the lights in tile.ted document order (all 1,141 records match by index).
  The runtime light (0x3c bytes): position, rgb +0xc, radius +0x18, animation +0x1c (the LAT enum), speeds +0x20 / +0x24, colour_scale
  +0x28, colour_min +0x2c, phase +0x30, smooth +0x34, cast shadow +0x38. fn 0x10407e90 sets the phase from the game's generator (seed
  0x12344321, state = state * 0x343fd + 0x269ec3 per light, phase = (state >> 16) * (1 / 65535)), smooth = 1 and cast shadow = 0: a map's
  lights are all drawn smooth (falloff_type is not read; 112 of western_city_b/small's are WPLFT_LINEAR) and random_offset is not used.
  The lighting node (fn 0x118ed4a0, "Lighting Node draw_update_node", lights at +0x1c0 / +0x1c4) submits each one every frame.
- **Colour** (verified in the code; no gamma conversion anywhere on the way). The submission (fn 0x118d9680): colour * (colour_scale +
  colour_min) for LAT_NONE; colour * (colour_scale * |sin(t * speed_x + phase)| + colour_min) for 1; the same with |sin(t * speed_x +
  phase) * sin(t * speed_y + phase)| for 2 (the sine's argument reduced by 2 pi; the "radius" animations modulate the colour, not the
  radius). The add-light method (fn 0x118dc6c0, vtable +0x58) multiplies the colour by fn 0x11753690: the `MAX_HDR_SUN_COLOUR_SCALE` tweak
  (Scene.cpp, default 25000, object 0x12a37730 +0x48) * 0.2 = 5000 when HDR is on, else 1 (the factor the shader's 0.004 undoes), and
  files the light by falloff and shadow; the draw (fn 0x118de4a0 pass 0x1b -> fn 0x118da9a0) copies it into POSITION1 = (colour, 1),
  POSITION2 = (centre, radius). The light definitions' XML (fn 0x11900a40: `LIGHTS` / `POINT_LIGHT`: name, red, green, blue, radius,
  falloff, animation, animation_x, animation_y, colour_min, colour_scale, shadow `LOW` / `HIGH`, for lights attached by name) goes the same
  way with its own falloff and shadow.
- **Editor.** `Payload::PointLight` per shown instance (colour, colour_scale, colour_min, radius, the animation, its speeds and the game's
  phase from the light's index among the tile's lights, hidden ones included (visibility is the editor's only); always smooth); the light pass (`renderer/lights.rs`) after the directional
  light: a cube around each light's sphere, back faces, no depth test, stencil = geometry, instanced per falloff, the colour animated with
  the frame clock. The window lights give small warm pools at night and are faint by day. Selecting a light draws its reach (a circle of its radius in each axis plane); in the tile tool a handle on it drags the radius.
  Not drawn: shadow-casting lights (only the XML definitions can ask for them).

## Test map

A battle building is a folder of pieces; `<name>_pieceNN_destruct01.rigid_model_v2` is each piece's intact state. Piece placement nodes in `<name>_tech.cs2.parsed` were identity in every file checked, so pieces are drawn at the building origin. Each mesh's vertices are stored around its material's `v_pivot`, which places them in the building's frame: drawn at vertex + pivot (2026-10-18; ignoring it sank most buildings by about half their height and put docks 9 m up; tile instances also scale the geometry, see docs/formats/tile_ted.md). Evidence: the meshes' own `min_bb` / `max_bb` are vertex bounds + pivot; the tech file's bounding box fits vertex + pivot better than the bare vertices in 862 of 946 building models with a non-zero pivot (`30_30_10`, pivot (15, 5, -15): exactly); the amphitheater's centred vertices (y +-6.59, pivot y 6.47) stand on the ground only with it. 11 models have different pivots per mesh.

## DLL addresses (empire.retail.dll 1.6.0)

- shader manager: `*( *(dll+0x2a21d18) + 0x71558 )` (fn 0x178c460); shader registry init fn 0x16f1a30
- manager + 0x634 `lighting_VS_PS`; + 0xd31ec SKYDOME_SHADER (cb `skydome_PS` at +0x298); + 0xb804 tone mapper (`g_scurve_texture` var at +0x838); + 0x30380 hdr_to_screen
- property name tables: SKYGEN 0x1da74a4, HDR 0x1da5f80 (accessor vfuncs 0x19081f0, 0x176af70)

## Verified against a memory dump (2026-10-06, med_sp_land_day_dry.environment, tdd_pack6)

`cli/env_vs_dump` compares the editor's mapping with `scripts/re/dump_cbs.py` output. All file-driven values match:
sky (pow 2.2 * scale * 7.5; horizon decay *-0.5, circumsolar *-10, backscatter as is), sun colour (pow 2.2 * scale * 5000),
sun direction (cos el sin az, sin el, cos el cos az), fog (argb = r,g,b,a; start - precipitation modifier; top >= 0.001),
colour_matrix, water colours, colorimetry (= gfx_gamma_setting / gfx_brightness_setting from preferences.script.txt).
The game's live tone mapper is basic_tone_mapper (constant_tone_mapping_buffer is all zero).

Open:
- Ambient cube when use_custom_ambient_cube_colours = false (276 files): not the XML colours, and not a plain integral
  of the sky (per-channel ratios differ). set_ambient is fn 0x178ab70 (6 colours via device vfunc +0x260), reached
  through vtable 0x1c99868. Values are multiples of 0.025.
  Dry dump (hdr_scaler 3.45): lr (3.45 3.75 3.7)(2.3 2.575 2.65), tb (3.175 3.7 3.9)(0.8953 0.8701 0.8323),
  fb (1.925 2.25 2.45)(2.15 2.45 2.55). lr[0].x equals ambient_cube_hdr_scaler, so this is probably another colour
  source times the same scaler; ambient_scale (unused by custom cubes) may belong to this path.
- S-curve LUT fill code; shadow cascades (dump: 4 splits, 2048 atlas, fade 0.1/0.296), clouds, sun, SSAO.

## Second and third dumps (2026-10-07, rain and snow battles)

`env_vs_dump --all` ranks every .environment file by matching constants: rain = weather/charlemagne/
cont_sp_land_day_rain_2 (35 matches, next best 31), snow = weather/default/default/land/day/snow/snow_3_heavy (35, next 23).
Every file-driven lighting_VS_PS / skydome_PS / fog_VS_PS / tone-mapper value now matches in all three dumps.

## Clouds (2026-10-07)

Rebuilt from the DLL plus the live objects in the memory dump (`src/render/clouds.cpp`):
- init fn 0x18e5b10: every cell of a (grid_size+2)^2 grid spawns a cloud when `rand <= cloud_density`.
- spawn fn 0x18f0bf0: jitter, cloud rotation (+-cloud_rotation*pi), template, per-element sprites (4 rands each),
  element offset * cell size, angular size min(scale, 4) * cell angle, texture_row/column/index 0 = random.
- template fn 0x1905f60: first CLOUD with this_cloud_only, else weighted by probability.
- sprite fn 0x18d82a0 stores the instance data cloud_plane.hlsl reads (TEXCOORD1..5), checked on 741 live sprites.
- Dome constants per plane type (dump): near radius 608000 / centre -592800 / jitter 0.5 / atlas 2x2;
  far 972800 / -912000 / 0 / 1x2. Cell angle = 2*acos(-centre/radius)/grid_size (both planes).
- PS constants (dump): transmitted/reflected raw, shadow colour pow 2.2, horizon_alpha_start 0.7 -> 0.97 (assumed 1-(1-x)/10).
Open: drift with cloud_velocity (update body not decoded), g_wind_alignment, blend state (assumed alpha).

## Open-item investigation (2026-10-07, second pass)

S-curve (tone mapper LUT), not yet located:
- Two LUT textures: `HDR_CONSTANT_TONE_MAP_VIEW_COMMON SCurve Texture` (fn 0x178c710, view +0x80, dirty byte +0x84) and
  `HDR_DYNAMIC_TONE_MAP_VIEW SCurve Texture` (fn 0x17659b0, view +0x88); both 256x1, engine format 0x1e.
  ps_basic_tone_mapper (the variant the game uses) samples it per channel at (luma-normalised colour, 0.5).
- HDR block (one per environment node; rain dump: scene+0x1688, a blended copy at scene+0x20b8): vtable 0x1c9aea8;
  +0x2c absolute_black, +0x30 max_black, +0x34 min_white, +0x38 absolute_white, +0x3c auto_exposure_speed,
  +0x40/+0x44 low/high red, +0x48/+0x4c green, +0x50/+0x54 blue, +0x58 lock_rgb_relative_tones (byte).
  Debug sliders (fn 0x1755760) bind these, bias range -5..5.
- Defaults (fn 0x177cd80, "Set RGB Tone Values to Defaults"): black 0.01, max black 2, min white 6, absolute white 40,
  speed 0.02, low 0.33, high 0.66 on every channel, lock true.
- Ruled out: Perlin bias (no ln 0.5 constants), GPU generation (no generator shader), the float3->texture path
  (fn 0x177cc70/0x18d7cb0, used only by the CPU environment cube). The LUT bytes are not in either dump.

Ambient (use_custom_ambient_cube_colours = false), not yet located:
- set_ambient fn 0x178ab70 <- fn 0x177cfe0 (copies 6 float3 from a struct at +0x18..+0x5c) <- fn 0x177d090 =
  vtable 0x1c997d0 slot +0x98 (35 call sites in 0x1700000-0x1960000; none tests the custom flag directly).
- fn 0x18f0a70 averages the six faces of the CPU-generated environment cube (fn 0x18d7d20 per face: sky through
  fn 0x18e3700-style factors + sun from LIGHTING +0x38) into one colour (* constant at 0x1b2b7b4). Candidate input.

Disp8 note: fields below 0x80 are encoded as one byte, so `multi_disp.py`/`disp_scan.py` (4-byte patterns) miss them;
`scripts/re/bias_scan.py` disassembles functions and handles them.

## Non-custom ambient cube: solved (2026-10-08, Ghidra)

- EnvMapGen (fn 0x118e00e0) renders the full scene into the environment cube from (0, ground + 50, 0), 90 degree faces,
  far 972800, then reads back every face's lowest mip (its average) as float3 (R11G11B10-quantised values in the dump).
- fn 0x118f0980 replaces the bottom face: 0.35 * luma(top) * (0.71, 0.69, 0.66), luma = Rec.709; 0.35 = engine variable
  (dll+0x2a773d8 +0x48, same in both dumps). fn 0x177da80 stores the six colours in the environment interpolator (+0xfa4),
  fn 0x1177dab0 copies them to every LIGHTING block (+0xd0.., flag +0xcc).
- Ambient apply fn 0x178a300: if use_custom_ambient_cube_colours or no computed cube: linear(XML) * hdr_scaler; else
  computed * ambient_scale with lr = (+x, -x), tb = (+y, bottom), fb = (+z, -z). Dry dump: computed * 0.8 matches every
  cbuffer value exactly. Blending two lighting blocks (fn 0x175f4e0) mixes custom and computed cubes the same way.
- Editor: implemented with the sky cube's face averages. The game's cube also contains clouds, sun and terrain, so the
  editor's values differ (dry file: game top (3.97, 4.63, 4.88) vs editor (1.55, 2.99, 4.75)); next step is to render
  clouds and ground into the editor's environment cube.

## S-curve and HDR properties (2026-10-08, solved)

- The tone-map views fill their "SCurve Texture" in vtable slot 27 (fn 0x1727fc0 constant view, fn 0x17284a0 dynamic
  view) through fn 0x17940b0: three channels from fn 0x174a780, written as half floats (alpha 0). fn 0x174a780 evaluates
  the Bezier with the runtime basis matrix at dll+0x1d26b18 (-1 3 -3 1 / 3 -6 3 0 / -3 3 0 0 / 1 0 0 0, read from the dump).
  The search earlier missed it because the texture is RGBA16F and the fill sits behind a vtable.
- Battle HDR property table (dll+0x1da5ba0): absolute_black_point, max_black_point, min_white_point,
  absolute_white_point, auto_exposure_speed, low/high_tones_bias_red/green/blue, lock_rgb_relative_tones. Defaults
  (fn 0x177cd80): 0.01, 2, 6, 40, 0.02, 0.33, 0.66, true. The single low/high_tones_bias, black/white_point and
  high_pass_threshold of older files are in another table (dll+0x1da6034) and do not affect battles.

## Lens flare and vignette (2026-10-08)

- LENSE_FLARE_SHADER (fn 0x116de600, manager +0x34f28): passes sun_flare1..4 (lenseflare.hlsl flare_0..3) and vignette,
  drawn after hdr_to_screen on the final image (fn 0x1173ec10). Flares: blend ONE/ONE add; vignette: SRC_ALPHA /
  INV_SRC_ALPHA, black, alpha = saturate(2r - 0.9)^1.7.
- Textures (fn 0x116dd4e0): lense_flare_1 = commontextures/green_dot_flare.dds, 2 = streaks_flare.dds, 3 = rays_flare.dds.
- Constants (fn 0x1172b1b0): sun_position = camera + to_sun * 7600 (checked in two dumps), sun_flare = SUN_MESH
  sun_flare (loader fn 0x118dd040 clamps 0..1), time = battle clock, half_texel_offset = 0.5 / size, vignette_strength 1.
- sun_occlusion: read from the sun node (vfunc +0x80, not decoded). Editor: visible fraction of the sun disc from two
  occlusion queries (ASSUMPTION; matches 0 with the sun off screen in the dry dump).

## Depth of field (2026-10-08)

- Battles composite DOF inside hdr_to_screen compiled with ENABLE_DEPTH_OF_FIELD (25 taps of sample_data_5x5 scaled by
  coc = 1 - exp(-(z - min(focus_distance, 500))^2 / focal_length^2); result = lerp(frame, blur, blur_kernel_scale)).
  The separate DOF_COC / blur shaders (depth_of_field.hlsl) are another path and are not used by the editor.
- Constants (fn 0x172aae0): use_DOF -> focus_distance = DOF_focus first value, focal_length = second value,
  blur_kernel_scale = DOF_blur_size (DOF block at scene +0x20a0: +0 use_DOF, +4/+8 DOF_focus, +0x14 DOF_blur_size);
  otherwise the battle camera's values (dumps: 0, 1000, 1) with the plain hdr_to_screen.
- sample_data_3x3/5x5/9x9 (dump): offsets x / width, y / height, Gaussian weights exp(-(x^2 + y^2) / 8) normalised per
  kernel, w = 1. Only use_DOF='true' files: campaign/porthole lightings and weather/charlemagne/day/1.environment.

## Environment cube (2026-10-08)

The editor now renders EnvMapGen itself (Renderer::render_env_cube): six 256^2 faces from (0, 50, 0) through the
G-buffer, sky, cloud, directional-light and fog passes (no shadows, XML ambient while rendering, sky cube as
s_environment). The result is s_environment for reflections and the source of the non-custom ambient cube.
Dry dump comparison (game computed cube = cbuffer / ambient_scale 0.8):

| face | game | editor, geometry + sky | + clouds | + sun disc |
| --- | --- | --- | --- | --- |
| +x | 4.31 4.69 4.63 | 4.05 4.97 4.82 | 7.35 7.55 6.09 | 46.2 46.5 42.4 |
| -x | 2.88 3.22 3.31 | 2.13 2.88 2.94 | 6.12 6.22 4.75 | |
| +y | 3.97 4.63 4.88 | 1.54 2.98 4.74 | 7.08 7.09 6.20 | |
| +z | 2.41 2.81 3.06 | 2.43 3.23 3.30 | 4.88 5.09 3.99 | |
| -z | 2.69 3.06 3.19 | 2.68 3.51 3.53 | 4.74 4.97 3.87 | |

So the game skips the sun disc. Clouds are needed for the top face but the editor's clouds add about twice the game's
amount (the dump's cloud layout is random and unknown; horizon_alpha_start's remap is still an assumption). Open.

Follow-up (2026-10-08): the dry dump's own 743 cloud sprites (extracted from memory, loaded with AWE_CLOUD_SPRITES) give
the same editor cube (+y 7.19 7.13 6.14) as the editor's random field, so the gap is not the cloud layout. Lighting the
clouds with the previously computed ambient instead of the XML ambient makes it brighter (8.7), not dimmer. The cloud
PS brightness depends on ambient_cube_tb[0], sun_colour and camera_position (cloud_plane.hlsl ps30_main); what the
game feeds it during EnvMapGen is still unknown. Needs a GPU capture of the EnvMapGen pass.

### Env-cube clouds: EnvMapGen scene render (in progress)
- fn 0x1192b640 (EnvMapGen per-face scene) calls fn 0x1192a900 twice (cloud planes, object +0x620; guarded by +0x598/+0x59c), last argument 0.
- fn 0x1192a900: if byte[1]==0 init (fn 0x118e5b10); if dirty byte[0] set: timer +0xc += dt; when timer > interval +0x8 and
  (+0x48)->+0xec * k > threshold, for each of +0x44 columns LCG (0x343fd/0x269ec3) roll <= probability +0x4 spawns (fn 0x118f0bf0), timer reset;
  then submits the draw (fn 0x118b40e0) with the args {param_2, this+0x50, param_4, param_5} and clears the dirty byte.
- fn 0x118b40e0 -> 0x118b72f0 -> 0x118b8500 is a parallel-for over the plane's clouds calling fn 0x1192aa40 per cloud.
- fn 0x1192aa40 (per cloud): angle +0x9c += speed +0x88 * dt; past the end angle the cloud dies (+0x84 = 1, removed by
  fn 0x11911630). With the force flag (EnvMapGen passes 0) both visibility bytes +4/+5 = 1; otherwise the cloud's
  bounds are frustum-tested and +4 / +5 = visible in the two frusta. No draw and no colour/alpha change happens here.
- Conclusion: the EnvMapGen calls only advance and cull the cloud planes; they do not explain the ~2x env-cube top face.
  The remaining suspects are the cloud PS inputs during the env pass (ambient_cube_tb[0], sun_colour, camera_position).

### Env-cube clouds: cloud PS inputs ruled out (2026-10-08)
- cloud_plane ps30_main (disassembled): colour = (ambient_cube_tb[0] + sun_colour * 0.0001) * 0.5 * 0.65 * (g_hdr_on ? 360 * 0.004 : 1)
  plus sun-dependent terms scaled by sun_colour * 0.004 * 0.35 (g_hdr_on) - lighting_VS_PS cb1[1], cb1[4], cb1[11].x.
  The VS only moves the dome with the view matrix; the 50 m env camera offset is irrelevant at the dome radius.
- Editor experiments (dry file + dumped sprites, temporary env-var overrides, since removed): the far plane contributes nothing
  to the cube, the whole cloud excess is the near plane. Cube faces without clouds: +x 4.11 5.07 4.95, -x 2.13 2.92 3.01,
  +y 1.55 2.99 4.75, +z 2.46 3.29 3.39, -z 2.72 3.58 3.64. Game minus no-cloud: sides ~0 (-0.5..+0.7), top (2.4, 1.6, 0.1);
  editor minus no-cloud: sides +2..+3, top (5.7, 4.2, 1.4).
- Cloud lighting is bilinear in (sun scale, ambient scale), so four runs fit it exactly. A least-squares fit of the game faces
  to (cloud opacity, ambient scale, sun scale) leaves a large residual: no uniform scale of sun, ambient or alpha explains the
  game. g_hdr_on = 0 gives faces of 600-1400 (so the game runs the cloud PS in the HDR branch). The game's clouds are
  close to invisible on the side faces and warm on the top, so what differs is structural (which sprites reach the cube, or a
  different cloud-PS input such as sun_colour / g_sun_direction), not a brightness factor.
- Next: a Task Manager full dump taken while the game regenerates the cube is not possible (EnvMapGen runs in a few ms);
  instead decode the callers of fn 0x118b40e0 for the EnvMapGen call and compare the draw's sprite count / list with the main
  view's, and check whether the env pass binds a different cloud_plane_PS block (fn 0x1191e430 fills it per plane).

## Shadow casting (2026-10-18, Ghidra)

- **Depth only.** In the DX11 path the shadow map (fn 0x117665e0, ShadowMapView) is the depth buffer `SHADOW_MAP_VIEW DB` in
  TTF_R32_TYPELESS (engine format 0x25; the other shadow methods use TTF_DF24 0x22 / TTF_D24X8 0x23); the colour target
  `SHADOW_MAP_VIEW RT` is only made when its format is not TTF_NUM_FORMATS (0x3f), which it is here. The light pass (fn 0x1178ae60)
  binds the depth (view +0xa4) to all three shadow-buffer slots when the hardware flag (+0x2cd, set by every method) is on; the colour
  target (+0xa0) is the software path's. So the `ps30_shadow` z / w colour output goes nowhere and the comparison sampler reads the
  rasterised depth. The editor: an R32_TYPELESS texture, D32_FLOAT depth view and R32_FLOAT shader view, no colour target.
- **Depth bias.** Every shadow rasterizer (vtable 0x11c8bb30, a subclass of the base description fn 0x116e4af0: solid, CULL_BACK,
  depth clip on) gets its bias when it is created (fn 0x11767500 -> 0x11784690 -> 0x117846d0, once the shadow map object exists):
  DepthBias = int(`g_new_depth_bias`) = 2500 (desc +0x14), SlopeScaledDepthBias = `g_new_slope_scale_depth_bias` = 1.8 (+0x1c), no
  clamp. The presets some shaders write first (rigidterrain +0x9754: 1000 / -2, +0x9788: 0 / -2) are overwritten; the
  `g_battle_*` / `g_campaign_*` bias tweakables have no reader. `g_new_shadow_map_sample_bias` (0) fills `g_sample_bias`, as the
  dump shows. The rigid casters keep CULL_NONE (+0x1a60, cull field 1); the terrain's shadow_30_new technique keeps CULL_BACK.
- **Terrain.** shadow_30_new (rigidterrain, technique +0x5bbc, rasterizer +0x9720) = `vs30_shadow_new` + `ps30_shadow`: the
  height-map grid placed like geom_new (same vs_height_map_cb, the same 9 cm lift past 20 m from the camera). The editor draws the
  material terrain into each cascade with it (the patches culled by the cascade's matrix); the legacy flat patches were receivers
  only and still are. The camera position the lift reads in the shadow pass is the main camera's here (the shadow camera's in the
  game is not checked; at most 9 cm).
- Light depth range (fn 0x117329a0): the fitted extent plus a pull-back tweakable, `g_battle_terrain_on_shadow_cam_depth_offset_battle`
  5000 m with battle terrain on, `shadow_cam_depth_offset_battle` 150 m without, `shadow_cam_depth_offset_campaign` 14 m on the
  campaign map. The editor keeps its 4000 m range (part of the light-fit item below); at the depths the casters sit the bias unit is
  2^-24 of the range either way.

## Remaining engine items, status (2026-10-08)
- Lighting cross-fade: linear (b - a) * t + a on every field (fn 0x175f4e0). Colour conversion: pow 2.2 (verified).
- Cloud columns: death limit (cells / 2) * cell angle matches the dumped sprite angles; the spawn timer restarts at 0 after
  a spawn (fn 0x1192a900). Drift (decoded 2026-10-28): a cloud's speed is (0.975..1.025) * cloud_velocity * 0.1 radians of the dome per
  second (fn 0x18f0bf0, constant 0x1afbd60 = 0.1; the editor had left the 0.1 out, so clouds drifted 10x too fast), and the column interval
  (object +0x8, set by fn 0x190bbf0 together with the cell angle +0x30 and the end angle +0x10 = cell angle + cap) is cell_angle /
  (cloud_velocity * 0.1), only when that product is over 0.001. No dump shows a moving field (the dry file has cloud_velocity 0).
- Env-pass shadows: binding the main-camera shadow map changes the cube by < 1%; left off.
- Shadow light fit (fn 0x117329a0, solved 2026-10-18, `renderer/shadow_fit.rs`): light-space perspective shadow maps (LiSPSM,
  `warp_shadows` default on). The basis (fn 0x11758dd0): x = normalize(view x light), y = normalize(light x x) (the view direction made
  perpendicular to the light), z = light; with |view . light| > 0.999 the camera's side vector stands in for the view. The slice's eight
  corners (fn 0x11698df0, NDC (+-1, +-1) at the slice's near and far distances) give a tight light-space box; the eye sits at its x / y
  centre, `g_battle_terrain_on_shadow_cam_depth_offset_battle` (5000 m) in front of its near z; depth = 5000 + the z extent. Without
  the warp (view along the light) the matrix is that box's ortho (x, y to -1..1, z to 0..1). With it: sin(gamma) =
  sqrt(1 - (view . z)^2), n = (z_n + sqrt(z_n (z_n + depth sin(gamma)))) / sin(gamma) (z_n = the slice's near distance, `warp_distance`
  0), the projection centre C = eye + z depth / 2 - y (h / 2 + n), the half angle theta = the largest |acos| (fn 0x119efa10 is acosf) of
  the corners' directions from C around y, f = n + h; the perspective rows r0 = x / sin(theta), r1 = -z 2n / depth, r2 = y f / (f - n)
  (+ fn / (n - f)), r3 = y (all relative to C), and the game's remap (F = R W (P V)^-1 with R = [1 0 0 0; 0 0 2 -1; 0 -0.5 0 0.5;
  0 0 0 1]) gives the view-projection rows (r0, 2 r2 - r3, 0.5 r3 - 0.5 r1, r3). The dry dump's g_amHardSplit cascade 0 matches this
  structure exactly (w row a unit vector perpendicular to the light, y row parallel to it with n / f = 0.896, the depth row minus half
  the w row along the light with n / depth = 0.753, x row perpendicular to both with 1 / sin(theta) = 2.89). Not snapped to texels
  (the game does not either). The corners at the largest angle land at x = 1 / cos(theta) (the 1 / sin scale), as in the game.
- Shadow instability (the game's too: flicker and "retreat", worse at low fps), causes from the code, not isolated in captures:
  stale split distances (the cascades are scaled to the depth buffer analysis, min / max visible depth, read back two frames late as
  ported, the game's own latency not verified, and converted with the current camera: zooming in at low fps, the first cascade starts
  past the nearest geometry and the near shadows come back over the next frames); and the per-frame fit (warp and tight box refitted
  every frame, not snapped to texels: edges crawl). A shader patch can't fix either (matrices and splits come from the CPU), only widen
  the cascade blend / end fade. In the game: the `warp_shadows` / `split_d0..3` tweakables (if a release build can set them), or a DLL
  patch of the split setup (fn 0x11784c00).
- SSAO jitter texture: generator not found; the pattern is removed by the blur, so the HBAO sample content stays. Leads (2026-10-18):
  the SSAO view (vtable 0x11c9d758, resources fn 0x11766de0) makes it at view +0x7c, 256 x 256, flags 0xa0 (0x20 = CPU-written, as
  against 0x84 for render targets), the first supported of two formats (fn 0x11759bb0), no initial data; its draw (fn 0x11729660) binds
  it, and none of the view's methods write it. `CommonTextures/jitter3.dds` is the environment-specular view's (fn 0x11903ec0), and the
  Gaussian noise fill fn 0x1174a080 (Box-Muller from the game's LCG, seed 0) is for another object's +0xe4 texture.

### Env-cube clouds: top face only (2026-10-08)
- Per-face experiment (dry file): without clouds the side faces are +x 4.11 5.07 4.95, -x 2.13 2.92 3.01, +z 2.46 3.29 3.39,
  -z 2.72 3.58 3.64 against the game's 4.31 4.69 4.63, 2.88 3.22 3.31, 2.41 2.81 3.06, 2.69 3.06 3.19 (within 10-15%), while
  clouds on every face put them 100% off. The editor now draws the env-pass clouds on the +y face only.
- Top face is still 7.21 7.16 6.16 against 3.97 4.63 4.88 (cloud-free 1.55 2.99 4.75). A fit of the cloud lighting needs
  sun scale 0.46/0.58/0.75 per channel (ambient on), i.e. not a uniform factor, so the top-face cloud colour or the game's
  cloud-free sky differs. Open.

## Scene units and game units (2026-10-20)

- The game's battle world is in TEd units: a settlement tile is 2048 of them (the battle locations' `ECRectangle` 2048 x 2048), and the
  data BOB writes in the half-size frame (bmd building lists with scale 1, tree and grass lists, the VFX markers' scale / unit_scale, the
  point light markers) is scaled back by the tile placement (`RENDER_PARAMS` `unit_scale` 2). The game's shaders and their constants
  (terrain `blend_pixel_scale` repeats, `g_mid_distance_details`, the grass' 100-unit sink, fog, SSAO, decal tiling, ocean depths) are in
  those units. `g_blend_pixel_scale_array` is filled by fn 0x11821af0 as (blend_pixel_scale, outfield_blend_pixel_scale) times a float
  of the map (fn 0x107c0b20, +0x7c; 1 in the dumped battle: the array held 9, 9, 0.3) and mid_distance_strength.
- The editor's tile scene stays in scene units (half metres horizontally, models at 1 / unit_scale, docs/formats/tile_ted.md). The
  renderer culls, picks and draws overlays in scene units and hands the game's shaders game units (`CameraState::units` =
  `RenderScene::game_units`, 2 for a tile, 1 for the weather sample map): the camera constants (`game_view`, `game_proj`: near and far
  times the factor, the same NDC and depth), the instance matrices (`DrawSet::gather`), the terrain patches, heights and blend boxes, the
  decals, point lights, grass, rivers, the ocean grid and sea bed, the placed effects (simulated in game units), the shadow matrices
  (`game_clip`) and split distances.
- Why the half frame exists: the tile data (terrain grid of 128-unit cells, blend / height maps at about one texel per unit, BOB's
  lists and markers) is authored in one tile frame, and `RENDER_PARAMS` `unit_scale` maps it into each world: 2 for battles, 0.2 for the
  campaign map (`terrain/tiles/campaign/_tile_database/_settings.xml`). Game units are metres (a soldier's head is at 1.75,
  `att_nordic_04_skin01_cut_1`), so a battle tile cell is 256 m; buildings are authored large for that (a nordic longhouse is 17.7 tall).
  tile.ted positions are already in battle units (twice the tile frame); TEd, BOB and the editor draw the tile frame.
- Before this the ground's textures repeated half as often as in the game (twice the texel size: blurry rock and sand up close), the
  grass reached twice as far, and every distance constant was off by two.


## Environment specular and the grey roofs (2026-10-30, RenderDoc)

Two RenderDoc captures of Rome (western_city_rome at its campaign placement, `D:\renderdoc\rome.rdc` with the vanilla weather,
`rome_tdd.rdc` with TDD's `cc4_day_dry_1.environment`), against a RenderDoc capture of the editor at the same camera, roof pixel by roof
pixel:

- The G-buffer agrees: the roof's albedo (0.40, 0.32, 0.26) in the game, (0.42, 0.33, 0.27) in the editor; specular colour white in both
  (the buildings' specular atlases are near white). Every constant the editor derives from the environment matches
  (`env_vs_dump` with `WCE_MOD_PACKS`).
- The directional light's output did not: the game's roof (2.0, 1.41, 0.97), warm like its albedo; the editor's (2.53, 2.88, 3.05),
  blue-white. The game's `lighting_VS_PS.g_apply_environment_specular` is 0 in both captures (the editor had 1): the game draws the
  environment's specular in a pass of its own after the lights (`environment_specular_light.hlsl`, DEFERRED_ENVIRONMENT_SPECULAR_SHADER,
  at a lower resolution, then blurred in x and y; events 9460 / 9477 / 9488 of rome_tdd), which on that roof adds only (0.09, 0.12,
  0.16). With the flag at 1 the directional light reflected the sky at full strength on the white specular: grey-blue roofs and walls.
- The editor now sets 0 (`environment_apply.rs`) and, since, draws the separate pass too (below). ASSUMPTION: the
  flag is 0 at every graphics setting (both captures were at low settings, and the separate pass ran there); its writer was not traced
  (the variable is declared by fn 0x116f6870).
- The exposure (2026-10-30, the same capture): the game's tone mapper downsamples the 1920 x 1080 image into a fixed 256 x 256 (the
  "quarter" image; the bloom works on it too), then halves it to 1 x 1 in the luma statistics (128, 64, ... 1; events 9780-9863). The
  statistics shader takes four point taps at +-1/4 of an output texel, a 2 x 2 block. The editor had a quarter of the view's size and
  divided by 4 per level, so each level read a quarter of its input and the maximum missed the highlights: its white point never left
  the environment's minimum (17). Now as the game (`TONEMAP_SIZE`): the average luma at that view is 3.382 against the game's 3.373.
  The white point then differed (game 22.1 from a maximum of 20.6, editor 17.0 from 13.9): the maximum is a few sunlit pixels of the
  plaza's white marble columns, which are TDD's (`ruin_single_column_*` in `tdd_pack5_buildings`): the capture's launch pack carried
  the tile's buildings from the editor's mod packs, and the editor's scratch settings had none. With the TDD packs mounted the editor's
  white point is 22.3 against the game's 22.1 at the same 1920 x 1080 view, and the frame's regions agree within a few percent.
  `luma` logs the points.

## EnvMapGen in a RenderDoc capture (2026-10-30)

A capture of the battle's first frame (frame 129 of a launch through RenderDoc, Rome at its campaign placement, TDD's weather, every
graphics setting at its minimum; `TargetControl.QueueCapture`) holds the environment cube's build (fn 0x118e00e0): per face (256 x 256,
R11G11B10, slice by slice) a clear, the sky dome (s_sky) and the two cloud planes (122 and 128 sprites, `cloud_plane_PS` with the
remapped horizon alpha: 0.97 and 0.99 for the file's 0.7 and 0.9), and nothing else: no G-buffer, no lights, no fog. The faces average
(3.3, 4.5, 5.3) on the sides, (4.3, 6.3, 7.7) on top, 0 below. The camera is at (0, 108.15, 0): 50 m over the ground at the world origin
(`fStack_31c = 50` plus the terrain height, Rome's land ~58 m there). The editor renders the cube through its scene passes (G-buffer,
sky, clouds on top, light, fog) from 50 m over the scene's (0, 0): the lit ground fills the lower halves, which the minimum-settings
game leaves black (the editor's cube averages about 2.3 x the game's; the river reflects it: brighter, bluer water).

- Done: the cube's camera is 50 game metres above the ground at the battle's centre (`RenderScene::battle_centre`, the tile's centre).
- Open: whether the scene is in the cube at other settings. The October dump (normal settings, vanilla dry weather) matched the editor's
  scene cube within 10-25% (above), which a sky-only cube with black lower halves would not. Per face fn 0x1192b640 fills a render
  list (vfunc +0xe8 of the scene) and fn 0x11923ad0(1) marks the pass; what joins the list (and whether a setting decides) is not
  traced. Settle it with a memory dump at normal settings and vanilla weather (engine ambient: the cube's face averages are in
  `lighting_VS_PS.ambient_cube_*`), against the editor's two cubes.

## The environment specular pass (2026-10-30, RenderDoc)

`environment_specular_light.hlsl` (DEFERRED_ENVIRONMENT_SPECULAR_SHADER; the editor runs the single-sampled full-resolution variant of
the game's `..._full_res_ms`, and `reflections_blur.hlsl` `blur_x` / `blur_y_fullres`), after the water and before the fog
(`Renderer::pass_environment_specular`):

- Per pixel where the stencil's low four bits are 1 (all the game's geometry; the sky is 0): the reflected view ray; a screen-space march
  of up to `g_ray_max_samples` (20) steps through the depth, a hit taking the light buffer there; otherwise the environment cube, and
  below the horizon the battle map's far diffuse (`s_terrain`, 8192 x 8192, the map's ground; squared, times the sun colour and the
  sun's height). Outputs: the reflection (R11G11B10) and (blur radius, weight) (R8G8); the weight is a fresnel term
  (`pow(dot, 10)`) over the G-buffer's channel 2 .w (the gloss map's y), times the SSAO. The blurs (x, then y) multiply by the specular
  colour (channel 3) and the weight and add the result to the light buffer (ONE / ONE). On Rome's roof: reflection (4.4, 6.4, 7.6),
  weight 0.0235, added (0.10, 0.14, 0.16).
- `parameters` (the capture): g_flags 0, g_ray_length_scale 1, g_ray_max_samples 20, g_acceptance_depth_range (0.4, 1),
  g_border_falloff_range (0.9, 1), g_depth_bias_intensity 0.1, g_depth_falloff_range (0.996, 0.99), g_blur_radius_multiplier (2, 5),
  g_steep_reflection_adjustment_intensity 0.06, g_terrain_offset_scale (0.5, 0.5, 28672, -28672), g_buffers_scale 1; the blurs'
  `parameters` (another layout under the same name, written with `ConstantBuffers::set_at`): g_flags 0, g_min_normal_dot 0.98,
  g_min_depth 0.5, g_buffers_scale 1.
- **The gloss maps are sampled sRGB** (found on the way): the game's G-buffer holds the rigid materials' gloss map linearised (roof
  channel 1 .w 0.0078 and channel 2 .w 0.0235 where the editor had 0.094 and 0.157, their sRGB decodes 0.009 and 0.021). The editor
  read them linear: glossiness and the reflection weight 5 to 10 x too high (with this pass, white roofs). Fixed (`model_cache.rs`).
- ASSUMPTIONS: the march reads the light buffer (the low-settings capture bound a placeholder there: screen-space reflections off);
  `s_terrain` is a constant ground colour (0.49, 0.43, 0.30), the average of the map's far diffuse at Rome (the editor builds no far
  diffuse); the stencil is a texture of 1s (the editor's depth has no stencil; the sky's specular colour is 0, so it adds nothing there);
  the jitter is the SSAO's (256 x 256, the pass reads its first 64 x 64).

### The cube at max settings (2026-10-30, memory dump)

A memory dump at the user's max settings (Rome at its campaign placement, vanilla `cc4_day_dry_1`: engine ambient, so
`lighting_VS_PS.ambient_cube_*` are the cube's face averages; `g_ssr_enabled` 1 there) against the editor (`cb lighting_VS_PS` after
120 frames at the same placement and weather):

| face | game | editor, scene cube | editor, sky + clouds on every face | editor, sky, clouds on top only |
| --- | --- | --- | --- | --- |
| +x | 0.52 0.55 0.56 | 0.98 0.84 0.69 | 1.63 1.62 1.60 | 0.86 0.99 1.10 |
| -x | 1.11 1.05 0.89 | 1.62 1.47 1.18 | 2.18 2.09 1.88 | 2.01 2.05 1.81 |
| +y | 1.52 1.34 1.22 | 1.93 1.73 1.63 | 1.93 1.73 1.63 | 1.93 1.73 1.63 |
| +z | 0.65 0.64 0.63 | 1.02 0.88 0.76 | 1.60 1.59 1.58 | 0.89 1.02 1.11 |
| -z | 1.02 0.97 0.83 | 1.52 1.37 1.13 | 2.04 1.96 1.78 | 1.96 2.00 1.77 |

(the bottom is the engine's 0.35 * luma(top) stand-in in all.) No editor variant fits: the game's side faces are neutral, between the
scene cube (warm: the lit ground) and the sky-only ones (blue), and darker than both; the top face, sky and clouds in every variant, is
1.27 x the game's. So at max settings the game's cube is not the minimum-settings sky-only one (its lower halves are not black), but
not the editor's lit scene either.

**Settled (2026-10-30): the cube is the sky and the clouds above the horizon, black below, at every setting; the editor's clouds were
four times too big.** Comparing the cloud draws of the game's cube build (`envgen_frame129.rdc`) with the editor's: the game's sprite
is a 7 x 7 grid (216 indices) in [-0.5, 0.5] with uv = xy + 0.5; the editor drew a single quad in [-1, 1] (v flipped), four times the
area through the same shader. With the game's grid, the top face (sky and clouds) matches the minimum-settings capture within 10%
(editor 4.7 6.6 7.9, game 4.3 6.3 7.7), and the editor's sky-only side faces are twice the game's: the game's lower halves are black
(its dome is a half sphere). Against the max-settings dump, half the editor's sky-only side face is the game's within 5-15%: the
cube has no scene at max settings either (the October match of the scene cube came from the clouds' excess). The editor's cube is now
the sky and both cloud planes on every face, the bottom face and the side faces' lower halves black (`render_env_cube`); its ambient
at Rome with the vanilla dry weather: +x 0.61 0.62 0.64 (game 0.52 0.55 0.56), -x 0.98 0.95 0.87 (1.11 1.05 0.89), top 1.50 1.41 1.37
(1.52 1.34 1.22), +z 0.64 0.65 0.66 (0.65 0.64 0.63), -z 0.98 0.92 0.85 (1.02 0.97 0.83); the rest is the clouds' random layout and
the wind. The river, which reflects the cube, is the game's dark slate now. The cloud grid is the main view's too.
