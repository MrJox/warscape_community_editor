# Battle renderer: passes, shaders and findings

Status legend: **verified** (disassembly of the shader and/or empire.retail.dll), **assumption** (to check against a memory dump with `tools/re/dump_cbs.py`), **todo**.

## Frame (as implemented in `src/render/renderer.cpp`)

| # | Pass | Game shader(s) | Notes |
| --- | --- | --- | --- |
| 1 | Sky cubemap | `skydome.hlsl` vs30_main + ps30_skygen | verified: engine renders the sky into a cubemap ("EnvMapGen", fn 0x18e00e0). 256² RGBA16F here (size: assumption) |
| 2 | G-buffer | `rigidcommon.hlsl` vs30_main + `rigidstandard.hlsl` ps30_gbuffer / _alpha_test | 5 targets, layout below |
| 3 | Sky (forward) | `skydome.hlsl` ps30_main | samples the sky cube; below-horizon directions clamp to the horizon |
| 4 | Directional light | `directionallight.hlsl` ps30_slm_highdef (shadows_hardware_gather_5_0, sm_5_0) | full-screen, stencil = geometry. Ambient cube + env. specular + sun |
| 4b | Fog | `battlefog.hlsl` ps30_main | over geometry (stencil), alpha-blended (assumption); sky cube + sun-lit fog colour |
| 5 | Luma stats | `hdr_tone_mapper.hlsl` downsample_image_store_luma, hdr_luma_to_quarter_size_luma_stats, downsample_quarter_size_luma_stats | quarter image (rgb + luma), then min/max/avg pyramid to 1x1 |
| 6 | Exposure | determine_black_and_white_points | 1x1 ping-pong; `prev + (target-prev)*(1-speed)^(30*dt)`; read back (4-frame ring) into `constant_tone_mapping_buffer` |
| 7 | Bloom | high_pass_luminance_cutoff, blur_x, blur_y | quarter size, 11-tap |
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

## Test map

A battle building is a folder of pieces; `<name>_pieceNN_destruct01.rigid_model_v2` is each piece's intact state. Piece placement nodes in `<name>_tech.cs2.parsed` were identity in every file checked, so pieces are drawn at the building origin.

## DLL addresses (empire.retail.dll 1.6.0)

- shader manager: `*( *(dll+0x2a21d18) + 0x71558 )` (fn 0x178c460); shader registry init fn 0x16f1a30
- manager + 0x634 `lighting_VS_PS`; + 0xd31ec SKYDOME_SHADER (cb `skydome_PS` at +0x298); + 0xb804 tone mapper (`g_scurve_texture` var at +0x838); + 0x30380 hdr_to_screen
- property name tables: SKYGEN 0x1da74a4, HDR 0x1da5f80 (accessor vfuncs 0x19081f0, 0x176af70)

## Verified against a memory dump (2026-10-06, med_sp_land_day_dry.environment, tdd_pack6)

`tools/env_vs_dump` compares the editor's mapping with `tools/re/dump_cbs.py` output. All file-driven values match:
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
`tools/re/bias_scan.py` disassembles functions and handles them.

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
