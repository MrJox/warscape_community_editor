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
Resolved on 2026-10-08 (comments and docs updated, details in `docs/re/`): lighting cross-fade (linear, fn 0x175f4e0), colour
conversion (pow 2.2), emitter rotation order (qz*qy*qx, fn 0x1180d730), soft-edge property name (`soft_factor`), cloud
timer reset and death limit, env-pass shadows (< 1% effect, left off). Decisions rather than open items: VFX quality level
(base level = gfx_effects_quality 3), SSAO jitter content (blurred away).

Still open `ASSUMPTION`s / known gaps:

| Area | Location | What is missing |
| --- | --- | --- |
| Env-cube cloud brightness | renderer.cpp `render_env_cube` | Near plane is 2-3 units too bright on the side faces; not a uniform scale (see Known problems) |
| Cloud spawn interval | clouds.h:56 | Object +0x8 assumed cell_angle / velocity; the dry dump has velocity 0 |
| CPU particle emission | particles.cpp:366 | Game adds ring + box + SpawnPosition and uses one min/max radius lerp (decoded in particles.md), editor still picks box OR ring; accumulate/spawn alternation (fn 0x1188e240) not modelled |
| Particle tint | particles.cpp:522 | Tint = middle of the range |
| VFX constants | vfx_constants.cpp | Scale variation rows, colour lerp shape, ParticlesLocalSpace -> 2 mapping |
| VFX curves | vfx_def.h | Key interpolation while baking the 128-entry tables |
| Shadow light fit | renderer.cpp `pass_shadows` | Engine fits a tight box (fn 0x117329a0); editor uses a bounding sphere; dump matrix layout not understood yet |

## Important decisions and why
- **Game shaders run as-is.** This is the only way to get pixel-close output, so the editor's job is to reproduce the engine's constants and states exactly.
- **Measure, don't guess.** Every value comes from DLL decompilation (Ghidra, base 0x10000000) or the three memory dumps (DLL at 0x78040000). Anything not yet measured is marked `ASSUMPTION` in the code and listed in `docs/re/`.
- **RPFM for all pack I/O.** It avoids writing and maintaining our own pack reader/writer.
- **Release builds.** Develop and test with `--config Release`, because the user runs `build/RelWithDebInfo/weather_editor.exe` and that exe is locked while it runs.
- **Engine enums are D3D values.** Sampler, blend, rasterizer and depth fields map 1:1 to D3D11/D3D9 values; the TTF texture-format enum follows the order of the `TTF_*` strings.

## Known bugs / problems
- **Environment cube is too bright.** The editor's top (+y) face is about 7.2 / 7.1 / 6.1; the game's is 4.0 / 4.6 / 4.9 (dry dump).
  - **Ruled out:** cloud layout (checked with the dumped sprites via `AWE_CLOUD_SPRITES`), the computed ambient, and the EnvMapGen cloud calls (they only advance and cull clouds).
  - **Update:** the env pass now draws clouds on the +y face only; side faces are within 10-15% of the game, the top is still 7.2/7.2/6.2 vs 4.0/4.6/4.9.
- **Narrowed (see `docs/re/renderer.md`, "cloud PS inputs ruled out"):** only the near plane contributes; no uniform scale of sun, ambient or alpha fits the game's faces (sides ~0, top warm), so the difference is structural (which sprites reach the cube, or a different cloud-PS input). Next: decode the EnvMapGen caller of fn 0x118b40e0 and compare its sprite list with the main view's.
- **Particle count mismatch.** The rain dump's emitter accumulator (9.025 at rate 3000) is unexplained; the spawn-or-accumulate alternation isn't modelled.
- **Line endings.** `docs/re/renderer.md` triggers Git's CRLF→LF warning; Python edits should use `newline=''`.

## Files currently being worked on
- `src/render/renderer.cpp` / `renderer.h` (`render_env_cube`, cloud pass, SSAO)
- `src/render/clouds.h` / `clouds.cpp` (column spawn and death)
- `src/vfx/particles.cpp`, `src/vfx/vfx_constants.cpp`, `src/vfx/vfx_def.h`
- `docs/re/renderer.md`, `docs/re/particles.md`
- Decompiles: `../tools/decomp/` (`envgen.c`, `cloudsubmit.c`, `cloudjob*.c`, `cloudper.c`, `clouddraw.c`)

## Exact next steps
1. **Env-cube clouds.** Decode the EnvMapGen caller of fn 0x118b40e0 and compare its sprite list with the main view's; check whether the env pass binds a different `cloud_plane_PS` block (fn 0x1191e430). Target faces: +x 4.31/4.69/4.63, -x 2.88/3.22/3.31, +y 3.97/4.63/4.88, +z 2.41/2.81/3.06, -z 2.69/3.06/3.19. Measure with `--open weather/charlemagne/med_sp_land_day_dry.environment --shot x.png` (face averages in `x.png.txt`) and `AWE_CLOUD_SPRITES=<scratchpad>/dry_sprites_u.bin`.
2. **Shadow fit.** Work out the layout of `g_amHardSplit` (values in `docs/re/renderer.md`), then replace the sphere fit with the engine's box fit.
3. **Particles.** Reproduce fn 0x1895130's spawn composition (`docs/re/particles.md`, "Spawn code"), then the batch alternation.
4. **Cloud interval.** Needs a dump taken with a file whose near plane has cloud_velocity > 0.
5. **After each item:** build Release (stop any running `build/Release/weather_editor.exe` first, it locks the file), compare against the dumps, document in `docs/re/`, commit.
