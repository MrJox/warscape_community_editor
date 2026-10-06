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
| `skydome_PS` colours | verified copy of precomputed sky-node fields (fn 0x1923b50); "linear*scale" = **assumption** pow 2.2 * scale |
| `lighting_VS_PS.sun_direction/sun_colour` | verified: `set_sun(dir, colour)` normalises dir, copies colour (fn 0x178a230); callers not yet traced |
| sun azimuth/elevation convention | **assumption** (azimuth from +Z towards +X) |
| ambient cube | **assumption** linear(colour) * ambient_cube_hdr_scaler * ambient_scale; +x = right, +y = top, +z = front |
| `luma_stats_buffer` | verified by names: absolute/max black, min/absolute white 1:1 from HDR |
| `colour_matrix` | **verified** fn 0x172c100: Overlay * Hue * Saturation(Haeberli 0.3086/0.6094/0.0820) * Contrast(pivot 0.5) * Brightness(x engine factor [this+0xe0]); hue rotation sign layout assumption |
| S-curve LUT | 256x1 RGBA8 texture created in fn 0x178c710 ("HDR_CONSTANT_TONE_MAP_VIEW_COMMON SCurve Texture"); **fill code not found** — placeholder Schlick-bias curve from low/high_tones_bias |
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
- Ambient cube for custom = true: needs a second dump with such a file (formula currently assumed).
- S-curve LUT fill code; shadow cascades (dump: 4 splits, 2048 atlas, fade 0.1/0.296), clouds, sun, SSAO.
