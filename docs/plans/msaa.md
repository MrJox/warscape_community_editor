# MSAA (gfx_aa 2..4)

Status 2026-11-01: mapped from the game, not implemented yet. MLAA (gfx_aa 1) is done (`Renderer::pass_mlaa`).

## The game's frame at 2x (`D:\renderdoc\rome.rdc`, Rome at its campaign placement, MSAA 2x)

Listed by `%TEMP%\wce_scratch\rd\msaa_frame2.py` (each run of draws with the same targets and pixel shader inputs; s1 / s2 = samples).
Event numbers are the capture's.

| Events | What | Targets | Reads |
|---|---|---|---|
| 324 .. 5784 | shadow cascades | 2048 depth, s1 | |
| 5809 .. 6059 | the terrain's depth (and view depth) | R32F view depth + D24S8 #2938, s1 | |
| 6071 | the terrain's depth and stencil expanded into the MS depth | MS D24S8 #2897 | `t_depthbuffer`, `t_stencilbuffer` (s1) |
| 6107 .. 6272 | the terrain's blend meshes and blend boxes | 3 RGBA16F intermediates + D24S8 #2933, s1 | |
| 6301 | the terrain resolve into the G-buffer | 3 BGRA8 + D24S8, s2 | the s1 intermediates |
| 6327 .. 6398 | grass (z-prefill, then G-buffer) | s2 | |
| 6425 .. 9843 | rigid meshes, trees, billboards (the `_ms` G-buffer techniques: alpha as coverage) | 3 BGRA8 + D24S8, s2 | |
| 9857 | depth resolve: MS depth -> s1 depth #2938 (and a copy: the s1 `gbuffer_channel_4`) | D24S8 s1 | `t_multisampled_depthbuffer` s2 |
| 9875 .. 9898 | SSAO | s1 | s1 depth |
| 9926 .. 10153 | projected decals (`_ms`) | G-buffer s2 | `gbuffer_channel_4_texture_ms`, `t_gbuffer_normal_ms` |
| 10178 | directional light (`ps41_..._ms`) | R11G11B10 light buffer #2914, s2 | `gbuffer_channel_1..4_texture_ms` |
| 10198 | point lights (`_ms`) | light buffer s2 | the MS G-buffer |
| 10215 | water refraction (half size) | RGBA16F 960x540 s1 | `t_light_accumulation_buffer_ms`, s1 depth |
| 10239 .. 10332 | rivers, sea | G-buffer + light buffer, s2 | s1 depth |
| 10347 | sky | light buffer s2 | |
| 10369 | environment specular (`..._full_res_ms`) | R11G11B10 + R8G8, s1 | the MS G-buffer, `t_light_accumulation_buffer_ms`, `t_depth_stencil_int_ms` |
| 10386 / 10397 | reflection blur x (s1) / y (`_ms`, added) | s1 / light buffer s2 | `gbuffer_channel_1/3_texture_ms` |
| 10412 | fog (`battlefog ps41_main_ms`) | light buffer s2 | `gbuffer_channel_4_texture_ms` |
| 10440 .. 10592 | grid splines, clouds, sun, particles | light buffer + MS depth | s1 depth |
| 10593 | `ResolveSubresource` of the light buffer | | |
| 10613 .. 10647 | shadow depth analysis | s1 | s1 depth |
| 10672 | distortion | s1 + s1 depth | |
| 10689 .. 10810 | tone mapping (statistics, bloom, the tone map) | s1 | the resolved light buffer |
| 10828 | `hdr_to_screen` (`ps50_main_high_ms`) | final, s1 | the frame and `gbuffer_channel_4_texture_ms` |

No MLAA with MSAA. The tone mapper's `..._with_bloom_and_gamma_ms_Nx` techniques are not used here (the hardware resolve instead).

## Plan

1. **G-buffer and geometry.** MS G-buffer channels 1..4 and depth-stencil at the level's count (2 / 4 / 8). The terrain stays s1 (its
   intermediates and view depth) and is resolved into the MS G-buffer; its depth and stencil expanded into the MS depth. Grass, rigid
   meshes, trees and billboards drawn into the MS targets with their `_ms` techniques (`ps41_gbuffer_alpha_test_ms`,
   `ps50_gbuffer_ms`, `ps50_gbuffer_grass_ms`: alpha as coverage, `g_num_of_samples`). Then the depth resolve into s1 depth and an s1
   channel 4 for the s1 readers (SSAO, refraction, particles, depth analysis). Until step 2, a sample-0 copy of channels 1..3 keeps the
   s1 lighting working.
2. **Lighting into an MS light buffer.** Directional and point lights, decals, environment specular and its blur, fog with their `_ms`
   techniques; rivers, sea, sky, clouds, sun and particles drawn into the MS light buffer; `ResolveSubresource` before tone mapping;
   `hdr_to_screen`'s `_ms` variant.
3. **The rest.** Water refraction's `_ms` read, the editor's overlays and picking (the id channel), OIT and MLAA with MSAA (MLAA off),
   the environment cube (stays s1), the render scale.

Each step: captures at 2x / 4x / 8x against MSAA off and against `rome.rdc`.
