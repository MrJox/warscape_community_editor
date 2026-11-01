# MSAA (gfx_aa 2..4)

Status 2026-11-01: steps 1 and 2 done (Attila, ToB; not Rome 2): the multisampled scene, its lighting and the coverage techniques.
Measured on the ToB city tile's near view, hard edge steps (neighbours more than 60 apart): 31007 without, 27746 at 2x, 25243 at 4x,
23475 at 8x. Step 3 is what remains (below). MLAA (gfx_aa 1) is separate (`Renderer::pass_mlaa`).

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
   channel 4 for the s1 readers (SSAO, refraction, particles, depth analysis). Done together with step 2 (see "Design").
2. **Lighting into an MS light buffer.** Directional and point lights, decals, environment specular and its blur, fog with their `_ms`
   techniques; rivers, sea, sky, clouds, sun and particles drawn into the MS light buffer; `ResolveSubresource` before tone mapping;
   `hdr_to_screen`'s `_ms` variant.
3. **The rest.** Done: water refraction's `_ms` read, MLAA off with MSAA, the environment cube (one sample), the overlays (on the
   resolved depth), picking (its own view). Open:
   - Done 2026-11-01: snowy terrain tiles (the terrain's depth is expanded into the MS depth right after its depth pass, and a snowy
     tile's boxes draw again into the MS depth-stencil alone, so their mark reaches the samples the snowy resolve tests; checked with
     every tile forced snowy: the same snowy ground with and without MSAA). `hdr_to_screen`: the game's `ps50_main_high_ms` of each
     variant (depth of field, distortion, ToB's grain), reading the MS depth, the one-sample shader if missing.
   - OIT with MSAA: the fragments are stored per pixel against the MS depth and resolved into every sample; the game's OIT with MSAA
     was not checked.
   - The selection outline and the editor's own passes on the MS targets (they draw after the image, on the one-sample depth).
   - A capture of the game at 4x / 8x to compare.

Each step: captures at 2x / 4x / 8x against MSAA off and against `rome.rdc`.

## Design (2026-11-01)

The game's helpers, `resolve_depth_buffer.hlsl` (DEPTH_BUFFER_RESOLVE_SHADER, fn at 0x116d3d44's object): `ps_resolve_2x/4x/8x`
(sm_5_0) write the farthest sample (max) of `t_multisampled_depthbuffer` as SV_Depth; `ps_antiresolve_with_fixed_stencil` (sm_3_0)
writes `t_depthbuffer` as SV_Depth where `t_stencilbuffer`.y is at least 1 (else discards), the stencil from the state's reference;
`ps_copy` copies a depth.

The game's G-buffer channel 4 is its depth buffer: with MSAA the editor reads the MS depth-stencil (an SRV on it) as
`gbuffer_channel_4_texture_ms`, and the resolved s1 depth as the s1 channel 4.

Steps 1 and 2 go together: the lighting tests the G-buffer's stencil marks (geometry, terrain, decals), which a depth resolve can't bring
back to s1, so an "MS geometry, s1 lighting" stage would not draw right. With MSAA (and not for the environment cube):
- `GBuffer` keeps its s1 targets and gains MS ones: channels 1..3, the light buffer, the depth-stencil (with depth and stencil SRVs).
  Accessors pick the MS or s1 ones; every pass takes its targets through them.
- Terrain: its depth and intermediates stay s1; `antiresolve` writes its depth into the MS depth (depth-tested, the terrain's stencil as
  the reference); its resolve draws into the MS channels.
- Meshes, grass, blend meshes, decals: MS targets; the `_ms` techniques where the game has them.
- After the decals: `ps_resolve_Nx` into the s1 depth: SSAO, refraction, particles' soft depth, the depth analysis, the overlays read it.
- Lights, environment specular (its stencil stand-in MS too), blur y, fog: `_ms` variants into the MS light buffer; sky, sun, clouds,
  water, OIT's resolve, faded meshes, particles into the MS light buffer with the MS depth.
- `ResolveSubresource` of the light buffer before the tone mapping; `hdr_to_screen`'s `_ms` variant; no MLAA.
