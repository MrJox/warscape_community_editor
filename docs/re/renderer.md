# Battle renderer: passes, shaders and findings

Status legend: **verified** (disassembly of the shader and/or empire.retail.dll), **assumption** (to check against a memory dump with `scripts/re/dump_cbs.py`), **todo**.

## Frame (the first port's order; the passes are now in `wce_render`'s `renderer/` modules)

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

Added since (sections below): shadow cascades, SSAO, clouds (`cloud_plane.hlsl`), sun (`sun.hlsl`), lens flare, VFX, DOF, the environment
specular pass, terrain and grass.

## G-buffer (from rigidstandard ps30_gbuffer and directionallight)

| RT | Content | Format |
| --- | --- | --- |
| 0 `gbuffer_channel_1` | normal / max(abs) * 0.5 + 0.5, .w gloss | RGBA8 (assumption) |
| 1 `gbuffer_channel_2` | sqrt(albedo), .w mask | RGBA8 (assumption) |
| 2 `gbuffer_channel_3` | specular colour, .w material id / 255 | RGBA8 |
| 3 `gbuffer_channel_4` | z / w, cleared to 1 (the game reads its depth buffer here: "G-buffer channel 4" below) | R32F (assumption) |
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
| **HDR light scale** | **verified** (dumps, "Point lights" below): with `g_hdr_on` the lighting and fog shaders multiply light colours by 0.004 (1/250); ambient by 360*0.004. The engine's `sun_colour` is linear(colour) * scale * 5000 (`MAX_HDR_SUN_COLOUR_SCALE` 25000 * 0.2) |

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

Note: the editor draws a building at the pieces of the `destruction_level` saved in tile.ted (321 damaged decal buildings on
eastern_city_a/medium), or of the level edited in the Inspector.

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

`wce_render`'s `env_vs_dump` example compares the editor's mapping with `scripts/re/dump_cbs.py` output. All file-driven values match:
sky (pow 2.2 * scale * 7.5; horizon decay *-0.5, circumsolar *-10, backscatter as is), sun colour (pow 2.2 * scale * 5000),
sun direction (cos el sin az, sin el, cos el cos az), fog (argb = r,g,b,a; start - precipitation modifier; top >= 0.001),
colour_matrix, water colours, colorimetry (= gfx_gamma_setting / gfx_brightness_setting from preferences.script.txt).
The game's live tone mapper is basic_tone_mapper (constant_tone_mapping_buffer is all zero).

Open:
- Ambient cube when use_custom_ambient_cube_colours = false (276 files; solved 2026-10-08, "Non-custom ambient cube" below): not the XML
  colours, and not a plain integral of the sky (per-channel ratios differ). set_ambient is fn 0x178ab70 (6 colours via device vfunc
  +0x260), reached through vtable 0x1c99868. Values are multiples of 0.025. Dry dump (hdr_scaler 3.45): lr (3.45 3.75 3.7)(2.3 2.575
  2.65), tb (3.175 3.7 3.9)(0.8953 0.8701 0.8323), fb (1.925 2.25 2.45)(2.15 2.45 2.55). lr[0].x equals ambient_cube_hdr_scaler, so this
  is probably another colour source times the same scaler; ambient_scale (unused by custom cubes) may belong to this path.
- S-curve LUT fill code; shadow cascades (dump: 4 splits, 2048 atlas, fade 0.1/0.296), clouds, sun, SSAO (all solved or drawn since:
  sections below).

## Second and third dumps (2026-10-07, rain and snow battles)

`env_vs_dump --all` ranks every .environment file by matching constants: rain = weather/charlemagne/
cont_sp_land_day_rain_2 (35 matches, next best 31), snow = weather/default/default/land/day/snow/snow_3_heavy (35, next 23).
Every file-driven lighting_VS_PS / skydome_PS / fog_VS_PS / tone-mapper value now matches in all three dumps.

## Clouds (2026-10-07)

Rebuilt from the DLL plus the live objects in the memory dump (`wce_scene` `clouds.rs`):
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

S-curve (tone mapper LUT), not located in this pass (solved the next day, below):
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

Ambient (use_custom_ambient_cube_colours = false), not located in this pass (solved the next day, below):
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
- Editor: first implemented with the sky cube's face averages (dry file: game top (3.97, 4.63, 4.88) vs editor (1.55, 2.99, 4.75)).
  What the game's cube holds was settled on 2026-10-30 ("The cube at max settings" below): the sky and the clouds, no scene.

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
- blur_kernel_scale (DOF_blur_size) is the lerp's factor, so above 1 the composite extrapolates past the blur: halos on edges out of
  focus (2026-11-13, seen in the editor with at_au_land_day_rain's 6.3, a file that ships with use_DOF off). The shipped file with it on,
  weather/charlemagne/day/1.environment, has DOF_blur_size 0.25 and DOF_focus variance(0,0).

## Environment cube (2026-10-08)

Superseded on 2026-10-30 ("The cube at max settings" below: the cube is the sky and the clouds only). At the time the editor
rendered EnvMapGen (Renderer::render_env_cube) as six 256^2 faces from (0, 50, 0) through the G-buffer, sky, cloud, directional-light
and fog passes (no shadows, XML ambient while rendering, sky cube as
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

Follow-up (2026-10-08): the dry dump's own 743 cloud sprites (extracted from memory, loaded with an environment variable of the C++
editor, since removed) give the same editor cube (+y 7.19 7.13 6.14) as the editor's random field, so the gap is not the cloud layout.
Lighting the clouds with the previously computed ambient instead of the XML ambient makes it brighter (8.7), not dimmer. The cloud PS
brightness depends on ambient_cube_tb[0], sun_colour and camera_position (cloud_plane.hlsl ps30_main); what the game feeds it during
EnvMapGen is still unknown. Needs a GPU capture of the EnvMapGen pass.

### Env-cube clouds: EnvMapGen scene render (2026-10-08; settled 2026-10-30, below)
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
  campaign map. Corrected 2026-10-31 (below, "Shadow cascades: units and pull-back"): the coast dump's boxes fit 150, not 5000.

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
  clouds on every face put them 100% off. The editor then drew the env-pass clouds on the +y face only (superseded on 2026-10-30, below:
  the editor's cloud sprites were four times too big).
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
- The editor now sets 0 (`environment_apply.rs`) and, since, draws the separate pass too (below). The flag is 0
  whenever screen space reflections are on (both captures; the separate pass is that setting's) and 1 at 0, where the directional light
  adds the specular itself (2026-11-01, fn 0x11904130: docs/re/graphics_options.md, "Screen space reflections"; the renderer's
  `reflections_off`).
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

## Projected decals: which pixel shader (2026-10-31)

- The engine's decal material (fn 0x116d3fe0) registers the techniques `main` (`ps30_diffuse`), `main_bugged` (`ps30_diffuse_bugged`),
  their `_z` and multisample variants and `main_far`, all on `vs30_main` and with stencil 0x81 (see "Which pixels take projected decals").
  Both pixel shaders take the box as the vertex shader does (POSITION1..3: the rows of the world matrix, POSITION4: the size), but:
  - `ps30_diffuse` normalises the rows and maps a point by sum_i d_i * row_i / |row_i| * size_i / size^2, uv = (x + 0.5, 0.5 - z): the
    box's inverse only when the turn does not mix axes of different sizes (square boxes, or turns by quarter turns). A turned 20 x 5
    road decal (`path_1`) came out sheared and partly cut away (gen_dun_medium_final_2's roads: flat smeared blocks where the game draws
    ruts).
  - `ps30_diffuse_bugged` maps by the rows transposed over size^2, the exact inverse for any turn and size, with uv = (x + 0.5, z + 0.5).
    With TEd's negative z scale (every kit decal) that v is the texture the right way up (the beach decals' wet edge by the sea).
- The editor draws with the `_bugged` pair (`ps30_parallax_bugged` for the parallax decals) and TEd's signed scale. The first coast fix
  took the scale's magnitudes with the plain shader, which put the beach the right way up but left turned non-square decals sheared;
  it was undone. Decals inside models (ruins' rubble, ProjectedDecalV2) have their box's z turned over so they keep the orientation they
  had. ASSUMPTION: which technique the engine picks per decal was not traced; the `_bugged` one is the one that maps the boxes the game's
  files give exactly.

## G-buffer channel 4 (2026-10-31, RenderDoc)

- The game's `gbuffer_channel_4` (the fog's, the ocean's, the decals' and the soft particles' depth) is its scene depth buffer (D24S8,
  coast.rdc: resource 2822, read by the ocean at event 914 and the fog at 945), so where nothing was drawn it holds 1, the far plane.
  The ocean is drawn into another depth buffer (a depth-only pass, event 897, then the colour pass), so the fog of a sea pixel is that
  of the ground under it. The editor's channel 4 is an R32F target written with z / w; it was cleared to 0 (the near plane), so a sea
  pixel with no ground behind it got no fog at all: dark slivers along the horizon where the map context's far ground had gaps. It is
  cleared to 1 now. The fog shader (`battlefog.hlsl` ps30_main) has no case for the sky: a depth of 1 fogs as far as the far plane.

## Which pixels take projected decals (2026-10-31, RenderDoc)

- The game's G-buffer stencil has two values for geometry: 129 (bit 7 | 1) for the ground (coast.rdc: event 307, the terrain's depth
  pass; 808, the terrain) and the TerrainBlend meshes (692 .. 711: its rocks, `attila_rock_*`, whose one mesh is a TerrainBlend), and 1
  for every other model, tree and billboard (552 .. 776). The lights compare with mask 3 (event 863: equal 1), so bit 7 is a mark of
  its own: the pixels decals may paint. (The captures were at the lowest settings, which draw no decals, so the decal pass's own state
  was not seen; the rule is read from the marks.)
- The editor painted every pixel inside a decal's box. The kit's road and camp decals have tall boxes (`path_1`: 20 x 30 x 5 TEd units),
  so on gen_dun_medium_final_2 they smeared their textures over the tents, tables, crates and walls standing in them. Now the terrain
  surface and the TerrainBlend meshes write stencil `TERRAIN_STENCIL` (2 | `DECAL_RECEIVER`), the decal pass tests `DECAL_RECEIVER`, and
  the other tests (the terrain's resolve, the lights, the fog) compare `STENCIL_LOW` only. Models drawn over the ground replace the
  stencil with 1, so they clear the mark.
- Checked on the same tile against the game's compiled `gen_dun_medium` (`prop_marker.markers`, with `prop_markers_dump` and
  `decal_list`): outside the escalation layers, 15 313 of its decals pair with the game's records by position and all but one have the
  same axes, so the boxes themselves were right (the tilted ones, rock clumps and some `path_1`, are tilted in the game too).
- Not marked: the legacy terrain patches (a scene whose ground has no material) draw as models, so decals skip them.
- **From the code, and contradicted (2026-11-15).** `RigidProjectedDecal`'s techniques (empire.retail.dll fn 0x116d3fe0: `main`, `main_z`,
  `main_bugged(_z)`, `main_ms(_z)`, `main_bugged_ms(_z)`, `main_far`) are registered with stencil reference 0x81, and their depth-stencil
  states' defaults (base constructor fn 0x116e7430, as a D3D11_DEPTH_STENCIL_DESC) have the stencil test on, masks 0xFF, KEEP / KEEP and
  EQUAL. Of every technique registration (89 shader classes, through fn 0x118c8440) only `rigidterrain`, `rigidterrainblend` and
  `resolve_depth_buffer` pass 0x81 too; the buildings' rigid shaders (fn 0x116e80f0) register `gbuffer` and `gbuffer_alpha_test` with 1.
  TEd's engine (`Warscape.AssemblyKit.dll`, fn 0x101820f0) registers the decal techniques the same way. **Yet TEd paints ordinary projected
  decals (the kit's `rocky_04` prefab: `global_terrain_rocks_04`, `dirt_1`...) over an ordinary house's roof** (the user's screenshot,
  2026-11-15). So a building's pixels do reach 0x81 at run time by a way not found yet (a guess: the reference or the state changes with the
  decal setting; the capture above that gave models 1 was at the lowest settings, which draw no decals). The editor's rule (only the ground
  and TerrainBlend meshes take decals) is therefore not the game's: to be settled by a capture at high settings or a test in game.
- **In game (2026-11-15, test tile `wce_decal_test`, TDD mods, launched from the editor):** `path_1` ruts laid along a `bridge_wood_2`
  (their boxes 40 units tall about the ground) paint its planks: two darker, scuffed lanes along the deck. A `global_terrain_puddle_02` and
  a `path_1` over a `western_villa_house01` (a thatched house with the mods) left its pitched roof clean; the ground about it showed the
  puddle only faintly. So buildings take decals in the game (a near-level deck at least); a steep roof did not (the facing fade, or its
  material: not told apart). How a building's pixels reach the decal pass's stencil 0x81 is still not found.
- **Corrected by RenderDoc (2026-11-16): the deck is not painted.** The "two darker, scuffed lanes" above are the bridge's own texture
  (the user, looking again). Captures `decals_frame5533.rdc` (the bridge) and `decals_frame7803.rdc` (the house), Attila at terrain
  quality 2, textures 0, 1280 x 720:
  - With decals drawn, buildings do write stencil 129 (`ALWAYS / REPLACE`: the bridge, event 408; the house, 412 and 415), so the decals'
    `EQUAL 129` passes on them: at the decal draw the stencil is 0x81 everywhere but the grass (0x41). (coast.rdc's models at 1 were at
    the lowest settings, which draw no decals.) The stencil is not what keeps buildings clean.
  - What does: the decal's box. Its vertex shader draws a unit cube (-0.5 .. 0.5) by the instance's rows (POSITION1..3), and the
    instance's y row is (0, 0.75, 0) for every decal on the tile (`path_1` of scale (60, 40, -5) and the puddle of (32, 40, -32) alike),
    its origin at y 30.0884, the ground's height there: a box 0.75 m tall about the ground. The file's `scale.y` (40) is not used for it.
    The deck, 0.89 m over the ground, is above the box's top: every deck pixel along the ruts' lanes fails the box's depth test (pixel
    history, events 448), and the pixel shader's own test (the pixel's world position from the depth copy taken after the buildings,
    event 425, against the half sizes) limits x and z only: y's bound is scale.y squared / 2 against 0.75 y, never reached.
  - So decals paint what lies within about 0.375 m of the ground under them, buildings included (a doorstep, a low deck, a plinth), and
    nothing higher. Not traced: where 0.75 and the origin's height come from (a constant, or fitted to the terrain under the footprint;
    this tile is flat), and whether TEd's house roof (painted in TEd) means TEd builds its boxes from `scale.y` while the game does not.
  - The editor keeps its rule (decals on the ground and TerrainBlend meshes only, boxes from `scale.y`) until that is traced: marking
    buildings with boxes from `scale.y` smears road and camp decals over tents and walls (2026-10-31), which the game does not.
- **Traced (2026-11-16, empire.retail.dll): the game refits battle prop decals to the terrain.** Three tweakables (static initialisers at
  RVA 0x51b00 .. 0x51bc0): `rescale_battle_prop_decals` (bool, default on; global 0x122b41f8), `rescale_battle_prop_decals_lower_safety_margin`
  (0.25; 0x122b6900) and `..._upper_safety_margin` (0.5; 0x122af650). After the prop markers are read (fn 0x104c1590, which calls the
  refit when the bool is on), fn 0x104c4250 goes through every decal marker (record +0x70 == 1) whose footprint is inside the map:
  - its model's bounding box (y from `ymin` to `ymax`; the decal models are the unit box, -0.5 .. 0.5) and its vertical axis (the matrix's
    y column, normalised; its length L is the box's height);
  - fn 0x1043b0c0: the lowest and highest terrain under the footprint's map-aligned bounds (the height map's cells over them and one
    beyond, each height measured along that axis from the decal's origin), widened by the walkable surfaces overlapping it whose slot
    0x30 answers true (type 0x100 in the surfaces' grid, fn 0x103645e0 with mask 0x100; each surface's plane height at its outline's
    points, to the lowest when the axis points down, else to the highest). Slot 0x30 is the "ground" bit (docs/re/platforms_and_bridges.md,
    "Platform objects"): **the building platforms with `flag_2`, or every platform of a `ground_platform` building**, and the surfaces of
    one non-building class (vtable 0x1b213f4, built by fn 0x102d3c20 from fns 0x102d72a0 / 0x10177930; what they are was not found).
    The other surfaces (raised platforms, a bridge's deck) are not counted: none on the test tile changed the box;
  - H = (highest + upper margin) - (lowest - lower margin), the new scale H / (ymax - ymin); **only when that is no more than L** the y
    column becomes the axis times it and the origin moves along the axis so that the box runs from lowest - 0.25 to highest + 0.5
    (constants 0.5 at RVA 0x1afbd68, 1.0 at 0x1afbd74). A shorter box is kept as it is.
  - So on flat ground every tall decal becomes 0.75 tall (0.25 under, 0.5 over: the capture's 0.75 and its origin 0.125 over the ground),
    on a slope it spans the slope; and what stands higher than 0.5 over the highest ground under a decal is never painted, a building or not.
    A decal whose own box is under 0.75 (scene units, the game's) is never refitted and paints what its box holds, at any height.
  - The rendering: fn 0x117f5b00 (the queue of a decal mesh) copies the decal's world matrix into its instance (fn 0x117c8400, 0x17
    floats: the three rows, the size and parallax from the decal, ...), so the refitted matrix is what the box is drawn with.
  - The editor does the same (2026-11-16): a decal's box is refitted when its entity is made or refreshed (`wce_tile::decal_refit`,
    `wce_tile_editor`'s `Tile::refit_decal`: the tile's decals and the roads' generated ones), and the G-buffer pass marks every model but
    trees and ropes for decals (`DECAL_RECEIVER`). On wce_decal_test the ruts paint the ground and not the bridge's planks, the house's
    roof is clean, as in game; on wce_bridge_test a generated bridge's deck takes its road's decals (V7's 0.7 boxes, kept). Not done: the
    ground surfaces of the measure (they lie at about the ground's height, so they would raise a box's top by little), and the map-bounds
    check (every decal is refitted).

## Shadow cascades: units and pull-back (2026-10-31, the coast memory dump)

- Fences, stone walls and carts cast no shadow in the editor (gen_dun_medium_final_2). The cause was the light's depth scale: at the
  camera one shadow-map depth unit (2^-24 near z = 1) was about 1e-4 per game unit along the light in the editor and 4e-3 in the game, so
  the shadow rasterizers' bias (DepthBias 2500, slope 1.8, the game's) pushed casters about 3 m back in the editor against 3 cm in the
  game, and anything lower than that lost its shadow. Two unit mix-ups and a wrong constant made it:
  - The Attila splits added the quality row (game units: 30, 90, 210, 450) to the camera's near and the depth analysis in scene units:
    every cascade was twice the game's length (the dump's `g_split_distances` 9.9, 41.0, 134.3, 352.0, 818.6 against the editor's 9.8,
    72.5, 260.6, 699.6, 1640.1 at the same view). They are computed in game units now and match (9.8, 41.0, 134.5).
  - The light fit took scene-unit corners and near distances with a game-unit pull-back; it is fitted in game units now (as Rome 2's).
  - The pull-back: the dump's `g_amHardSplit` gives, per cascade, the warp's n (from the y row: f / (f - n) and its constant), the
    slice's extent h = f - n and the box depth (from the depth row minus half the w row: 2n / depth along the light): n 64.5, 188.5,
    532.6, 1313.2; h 57.8, 187.9, 485.2, 1117.0; depth 241.5, 449.7, 935.6, 1976.7. Pull-back 150 (`shadow_cam_depth_offset_battle`)
    gives n 62.2, 179.0, 502.4, 1236.2 and depth 222.4, 387.5, 772.7, 1598.3 (the rest is the view's slightly other pitch); 5000
    (`g_battle_terrain_on_...`) gives n 249 and depth 5072 for the first cascade. `shadow_fit::DEPTH_OFFSET` is 150.
- Resolved 2026-11-16 (fn 0x117329a0 at 0x11732af2 - 0x11732b14): a battle always uses 150. The 5000 is taken when
  `terrain_enable_shadows` is set, and fns 0x117665e0 / 0x11794ca0 set it only at quality index 2 in the campaign mode, where the
  campaign's 14 wins anyway. The campaign's cascades: "The campaign map", "Shadows", below.


## Battle fog (2026-11-13)

`battlefog.hlsl` ps30_main (its DXBC disassembly, `fxc_dump`, and the pack's GLSL listing agree instruction for instruction), with
`fog_VS_PS` = volume_fog_colour, distance_start, distance_strength, distance_scale, height_bottom, height_top, height_strength,
colour_blend, clear_distance (as `environment_apply.rs` sets them: start less fog_precipitation_distance_modifier, bottom 0, top >= 0.001).
For a pixel at world position P, the camera at E, d = |P - E| and r = |(P - E).xz|:

- height = smoothstep(saturate((top - P.y - bottom) / (top - bottom)));
- distance = saturate(scale - scale / exp(d / L - S)) = saturate(scale * (1 - exp(-(d / L - S)))), L = (1 - saturate(strength)) * 1000,
  S = 8 * saturate(start) - 4: the files' start (0.52 in med_sp_land_day_dry) is not a distance but an offset; the distance fog begins
  at d = L * S, at the camera when start <= 0.5;
- f = height_strength * height + distance; the sky's share of the colour = saturate(f * colour_blend);
- alpha = saturate(f * smoothstep(min(r / max(clear_distance, 0.001), 1))): clear_distance (metres, horizontal) fades the whole fog in
  from the camera; in the charlemagne files it is 2500 to 2600 m, so it shapes the fog over the ground more than the distance term.

The Weather tool's fog rings (`wce_weather_tool` `visual.rs`, through `wce_scene::environment::fog_opacity`) evaluate this over the ground.

## The campaign frame (2026-11-19, RenderDoc)

Capture A (`camp_attila_frame6628.rdc`: vanilla, turn 1, Rome's coast at a middle zoom) read by `scripts/re/rd_campaign_frame.py`
and named by `wce_terrain_io`'s `campaign_frame` example (every one of its 127 shaders matched to the game's `fxc/` files by DXBC
checksum). The tooling and the comparison it serves: docs/plans/campaign_tilemap.md.

Pass order (event ranges of capture A):
- particles' GPU simulation and sort (24 - 252);
- four shadow cascades (308 - 5325), each: `rigidcommon` shadow (opaque and alpha test), `rigidcampaignvegetation` shadow (the tree
  clumps), `weightedcommon` shadow (the units), `rigidterrain vs41_geom_new` and `vs41_custom_geom` (tiles and cliff meshes);
- the terrain's depth (`rigidterrainblend` geom, `rigidterrain ps_depth`), the depth anti-resolve;
- the terrain surface into its three targets (`rigidterrainblend ps_terrain_material`, `rigidterrain ps30_blend`,
  `ps30_custom_geom_mat`), then the resolve into the G-buffer (`ps30_terrain_to_gbuffer_ss`, event 6762);
- the overlay (2048 x 1620): the rivers (`rigidriver main_river`, 2194 draws), the borders and trade routes (`campaignborder`);
- the G-buffer's models: `rigidcommon vs30_main` with `rigidstandard`, `rigidtileddirtmapmesh`, `rigidwithdirtmesh`,
  `rigidwithdecaldirtmesh` (buildings, props), `rigidcampaignvegetation ps41_gbuffer_ms` (the forests), `weightedcommon` (armies,
  agents: 110 draws in one run, 17547 - 17944), then the sea tiles' geometry (`rigidterrain ps_sea_geom`);
- SSAO, the projected decals, the directional light (`ps41_slm_highdef_ms`, then `_skinlm_`), water refraction, the lakes
  (`waterplane vs30_water_campaign_lake`), the sea (`waterplane ps30_campaign_high_water_quality`), sky / fog composites, `battlefog`,
  the shroud plane (twice), the VFX, luminance and exposure, the tone map (`hdr_to_screen`, 18985), the lens flare, the UI (19033 on).
- **Not drawn**: no grass pass, no `campaignterrain ps30_main_coast` (the coast strip) and no `ps30_main_trade_route`.

The sea tiles and the coast (capture A, pixel history and disassembly, 2026-11-19):
- A coast cell holds two tiles: a `sea_coast` tile (layer 0; flat, its `final_heights` all 0) and a `sea` tile over it (flat too). The
  terrain passes draw the coast tile on the land's LF (4064 x 2882, `g_height_map_lf_scale` 9.2868 = 712 m, offset 0.4993 = 38.28 m:
  0.013043 world units a metre); the sea tiles are not in them. In the open sea beside the cliffs the terrain passes draw nothing.
- The sea geometry pass (17966 - 17988, after the models) draws the sea tiles' 1-cell patches on the sea's LF (texture 2032 x 1441, 11
  mips, read on a 1016 x 720 grid: `g_height_map_dimension`; `g_height_map_lf_scale` 5.2173 = 400 m) into the G-buffer, depth
  LessEqual. `ps_sea_geom`: albedo = sqrt of `t_diffuse_array` layer `g_sea_texture_index.x * 4 + .y` (6, 0 here: the sea tiles'
  `sand_a`) at world xz over `g_blend_pixel_scale_array[layer / 4].x` (bias -0.66), darkened by the shroud; normal (0.5, 1, 0.5); the
  third target (1, 1, 1, 1/255); z / w. So where the coast tile's ground is higher than the sea floor it shows, else the sand: the
  shore follows the LF, not the cells (at the probed bay pixel: coast ground -15.9 m, sea floor -5.4 m, the shore +2.1 m).
- The terrain shader's LF lookup (`vs41_geom_new`): uv = (x / W + o, 1 - (z / H + o)), clamped below 1 - 1/N; texel k at k / N (a
  gather at (floor(p) + 1) / N and its own bilinear weights). The game's land texture has two texels a cell where the kit's
  `lf_heights.tif` has eight (16256 x 11528): its 4 x 4 box average, so a texel stands for the field 1.5 samples (3/16 cell) further
  south-east. The texture dumped (capture A, resource 6368) is exactly the field's 4 x 4 box averages from sample 0; read with the
  shader's lookup it gives the game's terrain depth at twelve bay points within 0.1 m (the field moved 3/16 cell but not averaged:
  1.2 m rms, the steep coast's curvature).
- The sea's texture (6372, 2032 x 1441, 11 mips, the view from mip 0) is the sea field's 2 x 2 box average; the sea pass's
  `g_height_map_dimension` is its second mip's (1016 x 720), so the lookup's floor and frac run on that grid while the gather reads
  mip 0: texels floor((floor(p) + 1) / 1016 * 2032 - 0.5) and the next. Its height stands about 0.75 cell off its ground and jumps
  every two cells; six sea-pass depths agree with this within 0.4 m (`wce_scene::lf_derive::game_sea`).
- The campaign water (`vs30_main_campaign`, `ps30_campaign_high_water_quality`) binds no sea-bed texture; its
  `gbuffer_channel_4_sampler` is the terrain passes' depth target (6150), written before the sea pass: its depth is to the land
  under the sea floor, not to the floor.

The world (the instance matrices, `instances.csv`):
- The tiles, the cliff meshes, the sea tiles and the river meshes carry the hex world's stretch: their matrices' z axis is 1.1548 x
  their x axis (0.8660 for tiles turned a quarter: the stretch is the world's, applied after the tile's turn).
- Buildings, props, decals and units are uniform (z / x 1.0000): unstretched models at their places in the stretched world.
- The tree clumps' instances are (x, y, z, angle) in world units, every angle a multiple of 60 degrees (the list's turn in sixths);
  no per-instance scale (the shader's 0.4).
- The main camera: eye (207.54, 9.29, 280.10), pitched 37.1 degrees down, vertical fov 80.2 degrees (`projection` [1][1] 1.1872),
  near 1, far 500, 1920 x 1080.

## The campaign map (2026-11-16 to 2026-11-21, Ghidra and RenderDoc)

What the game does on a campaign map, beside the frame above, gathered from the campaign plans (docs/plans/campaign_environment.md,
docs/plans/campaign_tilemap.md). Marks: **V** verified (data, disassembly, captures), **H** hypothesis. Captures are the user's
(`D:\renderdoc\camp_*.rdc`: A, rain, winter, G, E, C, F, T0 - T2, V1, V2; the list in campaign_tilemap.md, "Captures"). Addresses
in the lighting notes are written as offsets of Attila's `empire.retail.dll` from 0x10000000 (16c5a50 is fn 0x116c5a50), the others
as full addresses (Ghidra project `attila`). The files the lighting reads: docs/formats/weather_files.md, "Campaign and porthole
lighting".

### The world and the camera

- The campaign world is the hex grid's: z is 1.1547 (2/√3, `CAMPAIGN_Z_SCALE`) times x (V: the terrain patches' instance matrices in
  capture A, above; `map_data.esf`'s height 556.144 = 1441 rows x 0.33391 x 1.1547). The tiles, cliff meshes, sea tiles and river
  meshes carry it after a tile's turn; buildings, props, decals and units stand unstretched at their places in the stretched world.
- The lens: vertical fov 1.4 rad, near 1, far 500 world units (captures A and T1: the projection and `camera_near_far`).
- The camera limits come from `campaign_variables`: Thrones 7 - 23 world units and 0.75 - 1.15 rad, Attila 9 - 45 and 0.35 - 1.15.

### The far material (V, capture A event 6762)

The resolve (`ps30_terrain_to_gbuffer_ss`) blends from the tiles' ground to a map-wide far material between 9 and 37 world units
(`ps_height_map_cb.g_far_tile_distance` 0, `g_far_blend_range` (9, 37); docs/re/terrain.md, "Pass structure", item 3): `s_far_diffuse_map` and
`s_far_normal_map` (8192², BC3, made at load) and `s_mask_map` (the snow mask).

### The ground's inputs

- The LFs as the game reads them (V): the land texture is the field's 4 x 4 box averages from sample 0, the sea's the 2 x 2 read on its
  second mip's grid ("The campaign frame", above). The campaign sea reads its depth from the terrain's z / w before the sea pass.
- The LF normals (V): the terrain binds one `s_lf_normal_map`, the map's `lf_normal.dds` (BOB's DXT5 of the kit's `normal.dds`); no
  `normals/` chunk is bound.
- Cliffs (V, captures A, F, T0): every campaign cliff draw binds `terrain/textures/campaign/default/rock_b/3_su_diffuse.dds` whatever
  the climate or season. Where the engine names it (fn 0x117ca6f0) is not traced.
- Forests (V): `rigidcampaignvegetation.hlsl` (docs/re/terry.md, "Forests"); the game's reach is 186 to 236 world units ahead of the
  view, about 21 000 trees. No grass pass on a campaign map; TDD's reeds are the tiles' models.
- Samplers: the overlay's lines and the rivers' `s_lf_colour` wrap 16 x anisotropic (`s_wrap_aniso`); `shroud_cloud.dds` is BC3_UNORM,
  read linear (capture G).
- Constants: `g_refraction_depth` 36, the tone map's DOF focus and focal length 0. `g_blend_contrast`, `g_lq_infield_blend_range`,
  `g_snow_height`, `g_rock_*`, `g_foam_anim_data`, `g_wind_offset` and `g_amSoftSplit` are read by no shader that draws a campaign map
  (disassembled).

### Lighting and weather (V unless marked)

- The collection parser 16c5a50 / 16c5530 / 16bd080; its loader 1760cf0 multiplies each sphere's z by `CAMPAIGN_Z_SCALE` = 1.1547.
  Without a collection, `light_grid.xml` (1760e00), bilinear lookup 1791ea0.
- Season names (fn 0x118f6360): per season (`sp su au wi`) stage 0 is `<stem>_<season>.lighting`, stage n
  `<stem>_<season><n>.lighting`, else the name as written; the snow masks the same (`snow_mask_<season>[n].dds`). main_attila's names
  already end in `_sp`, so its lighting stays spring all year (the winter captures); TDD's and cha's bare stems change. Both startposes
  start at stage 0 (`WORLD/CLIMATE`).
- Blend (173d760, 17609c0): a sphere's weight w = 1 - clamp((d² - inner²) / (outer² - inner²), 0, 1) at the eye (3D distance, the
  sphere's raw y, z x 1.1547; the captures' weights reproduced to 0.001); sum w·env; if Σw > 1 divide by Σw, if Σw < 1 add
  (1 - Σw)·global. The sun colour blends linear (to the power 2.2). The weather is the heaviest input's definition (fn 0x11721160:
  strict >, the global's top-up competes), only its wind degrees and snowing intensity blended; the spheres are folded in their order,
  the global last (ASSUMPTION from Thrones' V2, 2026-11-21; Attila's Londinium agrees either way). No fade on a season change found (H).
- 161 of 163 campaign `.lighting` files have their own ambient; the engine's ambient is not read back.

### Sky, fog and the overlay (V)

- Skybox (afd250, drawn b29f60): the dome at the camera, scaled by `campaign_sky_scale` = 37.5, drawn in the G-buffer unlit
  (`rigid_config_PS.g_unlit` 1, the only such draw), fogged; `s_sky` the engine's white cube
  (`commontextures/default_white_cubemap.dds`), `s_environment` the map's `cubemap.dds`. No SKYGEN, cloud planes or sun mesh; the sun
  is static per file, no time of day. The game draws the dome into every cascade, but its four draws change no texel of capture A's
  shadow atlas: it lands outside each cascade, so in effect it casts no shadow (2026-11-20).
- Fog: `battlefog` fullscreen with the file's values ("Battle fog", above), then the shroud plane (`shroud_plane.hlsl`, set up
  190dd5c): a 256 x 256 grid over ±500 world units following the camera quantised by 1000 / 255, draped over the low-res land LF,
  drawn twice; the battle fog formula mixed with `s_sky`, the shroud's density (1 - a)^6.4 and four layers of `s_shroud_cloud`
  scrolled by time (scales 0.04 / 0.026 / 0.01333 / 0.008667) lit by the sun. Its depth write goes to a depth copy nothing reads
  after it (2026-11-20).
- `directionallight.hlsl` has a campaign branch: `s_overlay` through `g_overlay_view_projection`, and the shroud as a light factor
  (0.05..1) when `g_shroud_enabled`.
- The overlay camera (fn 0x11729930, `overlay_camera_push` 0.7): the view's rotation and fov moved back horizontally by
  d = 0.7 x 8 x |(ray_xz . forward_xz) / ray_y|, the ray from the eye to the near plane's bottom middle; near + d, far - d (the 8 fitted
  from five captures, its writer not traced).
- Lakes: the tiles' water planes through `waterplane.hlsl` `vs30_water_campaign_lake` / `ps30_campaign_lake`.

### Shadows (V)

- Fns 0x117665e0, 0x11784c00, 0x117329a0, 0x118debf0: the battle's cascade code with the table's campaign rows (mode * 4 + quality at
  dll+0x1c88aa0: (10, 30, 30, 30), (5, 15, 35, 35), (5, 10, 20, 40), (5, 10, 20, 40)), the depth analysis as in battle, pull-back
  `shadow_cam_depth_offset_campaign` 14 ("Shadow cascades: units and pull-back", above); the terrain casts only at the top quality.
  `campaign_shadow_split_scale`, `battle_shadow_split_scale`, `g_campaign_depth_bias` and `g_campaign_terrain_slope_scale_depth_bias`
  have no reader; `shadow_split_campaign_min_distance_delta` only gates the depth analysis's refresh. Capture A's splits 1, 6.25, 16.8,
  37.8, 79.8 are the rows x 1.05.
- The depth analysis (`shadow_map_depth_buffer_analysis`): `ps30_downsize_store_data` to 256² R32G32, then `ps30_downsize_data` to one
  texel, read two frames later. Its `gbuffer_channel_3_sampler` holds `commontextures/no_texture.dds` (the snow normals' slot left
  bound) through a top-mip-only sampler (`States::s_clamp_point_top`), whose alpha bit 2 leaves only a few edge columns (capture F).
- Alpha-tested materials cast through `rigidcommon`'s `ps30_shadow_alpha_test`.

### Campaign-only constants and tweakables (V)

- Campaign-only cbuffer fields: `shroud_PS` (enabled, `g_campaign_world_bounds`, `g_uv_offset`), `shroud_plane_buffer`,
  `vs_height_map_cb`, `g_overlay_view_projection`, `campaign_CB`, `campaign_ocean_cb`, `campaign_tree_ps`, `trade_route_PS`,
  `campaign_border_PS`, `g_is_campaign` = 1. Read by no shader: `g_season_type`, `g_campaign_autumn_timer`, `g_snow_height`. The
  campaign's weather constants are 0 (`g_snowing_intensity`, `g_weather_type`, `g_season_type`, `g_wind`, `wind_VS`) and its VFX wind
  a fixed (2, 0, 0) units / s in all five captures (its source not traced). Thrones' V2 has `g_rain_specular_*` / `g_rain_terrain_*`
  at 1.
- Tweakables: `terrain_campaign_waterplane_scale_*` 15/15/2/2, `CAMPAIGN_WATER_SEA_INTENSITY_SHROUD` 0.9, `shroud_padding` 20 /
  `shroud_edge_padding` 128 / `shroud_half_edge_padding` 64, `shroud_blur_kernel_size_v2` 10, `terrain_override_season` -1.

### Effects (V unless marked)

- The precipitation is the lighting's `WEATHER definition_name`, drawn around the eye at scale 1 in the campaign world's units (the rain
  capture: `rain_heavy_BTL`, the 30-unit recycle box).
- Placed effects (fns 0x10af7dc0, 0x10b053d0, 0x10acca10): an effect plays while in view (within 1.15 of the screen's half size, in
  front) and within 80 units of the eye, whatever its CullDistances; stopped, it stops emitting and its particles live on. `autoplay`
  and `use_in_outfield` are never read on the campaign. Snow zone: the season's snow mask at the marker. `burst_rigid` carriers start
  their `property_event OnRigidSpawn` effects once per rigid born (fns 0x1187aed0, 0x11863820, 0x1183fa30, 0x11895130). The
  `scene_view_type` 2 `cloud_heavy` is simulated but never drawn.
