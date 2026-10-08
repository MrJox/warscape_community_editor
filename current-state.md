# Current state — Attila Weather Editor

## What we're building
A standalone C++20 / D3D11 / Dear ImGui editor for Total War: Attila (and TDD) weather and lighting:
- **Files edited:** battle `.environment`, campaign `.lighting`, `weather_definitions.xml` and VFX XML.
- **Rendering:** uses the game's own compiled `.fxc` shaders, so the preview matches the game pixel-close.
- **Input:** game data is read only through `rpfm_cli`.
- **Output:** loose XML files that round-trip byte-identically.

Scope and phases are in [PLAN.md](PLAN.md); build and run instructions are in [README.md](README.md).

## Current implementation state
The full battle weather pipeline renders with values taken from the game's DLL, checked against three memory dumps (dry, rain and snow). The dumps were made from `empire.retail.dll` 1.6.0.
- **Pipeline stages:** G-buffer, SSAO, cascaded shadows, directional light, sky, sun, clouds, fog, particles and distortion, HDR with the S-curve tone map, depth of field, hdr_to_screen, lens flare and vignette.
- **Environment cube:** generated from the scene (EnvMapGen) every 4th frame.
- **Graphics options:** read from `preferences.script.txt` and adjustable in the Settings menu.

Everything is committed (latest commit `991811e`), and the working tree is clean.

## Completed (verified against DLL code and dumps)
- **Wind:** gusts, random battle direction, `weather_shared` / `wind_VS` / `wind_PS` constants, VFX wind and variance. Every `WEATHER_DEFINITION` field is supported, including `vfx2`.
- **HDR:** S-curve tone LUT (per-channel cubic Bezier, RGBA16F); battle HDR property names and engine defaults.
- **Lens flare and vignette:** shaders, textures, blend states, `sun_position` and `sun_occlusion` (occlusion query against an analytic total).
- **Sun:** mesh size (2000 × `sun_scale`) and alpha blend.
- **Depth of field:** the ENABLE_DEPTH_OF_FIELD variant, its constants and the 3×3 / 5×5 / 9×9 Gaussian kernels.
- **Environment cube:** the game's camera, face setup and far plane; it feeds reflections and the computed (non-custom) ambient.
- **Particles:**
  - Burst gate (`BurstInterval` / `BurstIntervalLength`).
  - Emission rate (per-instance min/max blend, 0.1 s dt cap).
- **Shadows:**
  - Cascade splits come from depth-buffer analysis and the quality row table.
  - 10% cascade overlap and the fade range.
  - Casters use CULL_NONE with depth clip.
- **Render states:** all engine blend, sampler, rasterizer and depth states use the engine's D3D11 enums ([docs/re/samplers.tsv](docs/re/samplers.tsv)), plus the sRGB flags.
- **Smaller verified items:**
  - Horizon alpha remap.
  - Hue matrix.
  - Render-target formats (G-buffer A8R8G8B8 / R32F, HDR A16B16G16R16F, SSAO R16F).
  - Graphics options: vignette, DOF, SSAO, distortion, shadow quality.

## What remains
These are the remaining `ASSUMPTION` markers in `src/`:

| Area | Location | Assumption to replace by measurement |
| --- | --- | --- |
| Env-cube cloud brightness | renderer.cpp `render_env_cube` | The editor's top face is about 2× the game's (see Known problems) |
| Env-cube shadows | renderer.cpp:525 | No shadows while generating the env cube |
| SSAO jitter texture | renderer.cpp:222 | Uses the HBAO default; the engine generates a 256² texture whose generator hasn't been found (SSAO view +0x7c) |
| Cloud columns | clouds.h:55 | Spawn and death timing; fn 0x1192a900 and fn 0x1192aa40 are now decoded, so this can be implemented |
| Lighting cross-fade | environment.h:91 | Interpolation method |
| Colour conversion | environment.h:105 | File colour to linear uses pow 2.2 |
| CPU particle emission | particles.cpp:366 | Spawn shapes; emitter alternation (fn 0x1188e240) not modelled |
| Particle rotation | particles.cpp:445 | `EmitterRotation` order |
| Particle tint | particles.cpp:522 | Tint = middle of the range |
| VFX constants | vfx_constants.cpp:46/54/62/112/177/188 | Local space, follow terrain, scale variation, colour lerp, legacy lighting, soft-edge property name |
| VFX curves | vfx_def.h:17 | Linear key interpolation |
| VFX quality level | vfx_def.cpp:70 | The editor shows the base quality level |
| Shadow light matrix | — | Fitting (fn 0x117329a0) not decoded |

## Important decisions and why
- **Game shaders run as-is.** This is the only way to get pixel-close output, so the editor's job is to reproduce the engine's constants and states exactly.
- **Measure, don't guess.** Every value comes from DLL decompilation (Ghidra, base 0x10000000) or the three memory dumps (DLL at 0x78040000). Anything not yet measured is marked `ASSUMPTION` in the code and listed in `docs/re/`.
- **RPFM for all pack I/O.** It avoids writing and maintaining our own pack reader/writer.
- **Release builds.** Develop and test with `--config Release`, because the user runs `build/RelWithDebInfo/weather_editor.exe` and that exe is locked while it runs.
- **Engine enums are D3D values.** Sampler, blend, rasterizer and depth fields map 1:1 to D3D11/D3D9 values; the TTF texture-format enum follows the order of the `TTF_*` strings.

## Known bugs / problems
- **Environment cube is too bright.** The editor's top (+y) face is about 7.2 / 7.1 / 6.1; the game's is 4.0 / 4.6 / 4.9 (dry dump).
  - **Ruled out:** cloud layout (checked with the dumped sprites via `AWE_CLOUD_SPRITES`), the computed ambient, and the EnvMapGen cloud calls (they only advance and cull clouds).
  - **Remaining suspects:** cloud pixel-shader inputs during the env pass: `ambient_cube_tb[0]`, `sun_colour`, `camera_position`.
- **Particle count mismatch.** The rain dump's emitter accumulator (9.025 at rate 3000) is unexplained; the spawn-or-accumulate alternation isn't modelled.
- **Line endings.** `docs/re/renderer.md` triggers Git's CRLF→LF warning; Python edits should use `newline=''`.

## Files currently being worked on
- `src/render/renderer.cpp` / `renderer.h` (`render_env_cube`, cloud pass, SSAO)
- `src/render/clouds.h` / `clouds.cpp` (column spawn and death)
- `src/vfx/particles.cpp`, `src/vfx/vfx_constants.cpp`, `src/vfx/vfx_def.h`
- `docs/re/renderer.md`, `docs/re/particles.md`
- Decompiles: `../tools/decomp/` (`envgen.c`, `cloudsubmit.c`, `cloudjob*.c`, `cloudper.c`, `clouddraw.c`)

## Exact next steps
1. **Env-cube cloud brightness.** Find which constant buffers the game binds for the cloud draw during EnvMapGen.
   - Start from the cloud draw function (`tools/decomp/clouddraw.c`).
   - Compare `ambient_cube_tb`, `sun_colour` and `camera_position` in the dry dump against the editor's env-pass values.
   - Fix `render_env_cube` and re-measure all five faces with `AWE_CLOUD_SPRITES=<scratchpad>/dry_sprites_u.bin`. Target values per face: +x 4.31/4.69/4.63, −x 2.88/3.22/3.31, +y 3.97/4.63/4.88, +z 2.41/2.81/3.06, −z 2.69/3.06/3.19.
2. **Cloud columns.** Implement spawn and death in `clouds.cpp` from fn 0x1192a900 / 0x118f0bf0 / 0x1192aa40:
   - Spawn timer with probability per column (LCG 0x343fd / 0x269ec3).
   - Each cloud's angle advances by speed × dt; a cloud dies past the end angle.
3. **Env-pass shadows.** Check whether EnvMapGen binds the shadow map (`g_iSplitCount` in the dump during the env pass).
4. **Particle emission.** Decode fn 0x1188e240 (emitter alternation) and the spawn-shape functions; replace the particle ASSUMPTIONs.
5. **Remaining assumptions.** VFX curve interpolation, lighting cross-fade, SSAO jitter generator, light-matrix fitting (fn 0x117329a0).
6. **After each item:** build Release, compare against the dumps, document in `docs/re/`, commit.
