# Rendering: what's left

Status (2026-10-26). The tile view draws the game's look with the game's shaders: terrain, buildings (every mesh material tiles use),
vegetation, grass, decals, water (rivers, sea, lakes, the models' water planes), point lights, placed VFX with their lights, and the
weather. Done by 2026-10-20: seasons and snow, terrain shadow casting (depth-only shadow map and bias for every caster), the maps'
sampler states, the shaders' world in game units (`RenderScene::game_units`, docs/re/renderer.md), the battle sea (ocean.hlsl, surfaces,
whitecaps, wind) and the game's LiSPSM shadow fit. Map-level terrain (feather into the base terrain, far maps, outfield, the resolve's
2nd / 3rd layers, virtual textures, large map context performance) is in docs/plans/map_context_todo.md; the weather's engine leftovers
in docs/plans/weather_current_state.md.

## Battle sea

Two stand-ins in `crates/wce_render/src/renderer/ocean.rs` / `crates/wce_render/src/sea_surface.rs`, both GPU-only in the game; the
capture recipe is in docs/re/water.md ("The battle sea of a coastal battle"):
- the radial mesh (80 x 164 around the camera) and its displacement: the editor uses a quadtree grid displaced by the sea and swell
  heights;
- the sea bed info texture (Sea Display slot 0x1a, likely a cut of `lf_sea_height_map.dds`): the editor builds it from the terrain.

## Water (from docs/plans/tile_ted_parity.md 4b)

Settled 2026-10-27 (docs/re/water.md, "Settled"): the river's uv is metres across / along (BOB's vertices, the shader's 0.01 / 0.001),
the battle river shader has no fade at the ends and no foam, `g_refraction_depth` is 0.75, and the water now takes shadows (the
`enable_shadows` variants of the river and water plane shaders). Settled 2026-10-29 in game: winter rivers stay open water in a battle
(no ice), so the engine's `BattleTerrain/river/ice_diffuse_*` mode is not a battle feature (docs/re/water.md).

## To check in game

- **Graphics settings, all together** (Attila and ToB; implemented 2026-10-31 .. 11-01 from static RE and RenderDoc captures, none
  compared with the game at each level; docs/re/graphics_options.md, docs/plans/msaa.md). With a fixed battle camera, capture the game
  at each level and the editor with the same `[graphics]` overrides:
  - Shadows (the split rows per level), building and unit detail (the LODs switched off), water (the shader per level), texture
    filtering, sky (the cube's size), screen space reflections (off / half / full), particle effects (the quality variants, the
    particle limit).
  - ToB: film grain, cloud shadows (their speed and direction with the wind), god rays (the glow near the sun, the strip along the
    bottom edge from the blur's wrap).
  - MLAA (threshold and line lengths).
  - Alpha blending: the near leaves and grass the OIT draws (forward lit) come out lighter than the same leaves through the G-buffer.
    Compare a forest edge within 25 m with alpha blending on and off: whether the game shows the same step at 25 m, and which
    brightness is right; AOIT is drawn as the standard OIT.
  - Tessellation: no vanilla model has a displacement map (RMV2 texture type 6), so a mod's model is needed (or one with a type 6
    texture added) to see the game's displacement against the editor's.
  - MSAA 2x / 4x / 8x: the coverage edges of the trees and grass, the brightness change from the resolve (the tone mapper's white point),
    snowy terrain tiles, OIT with MSAA; and a RenderDoc capture at 4x / 8x against the 2x one the port follows.
- Building collapses: the dust emission mesh and box, the effect group size, the debris, the bursts' timing, `.meta` effects on bones,
  the playback speed (docs/re/buildings.md, "Collapses between levels"). Not done: `SOUND_BUILDING` (the editor has no audio).
- TDD's dead tree bark is near black: `tdd_pack6_weather_1.0.0.pack` ships dark 512 x 2048 `dead_tree_{dry,wet}_diffuse` (and `_ab`)
  where vanilla's are light grey; the editor draws what the pack holds. Check the stumps in game with the same packs (e.g.
  gen_dun_medium_final_2) and brighten the mod's texture if dark there too.

## Shadow stability

The game's shadows flicker and "retreat" (worse at low fps); the renderer reproduces it. Causes: docs/re/renderer.md ("Shadow
instability"). 1ecaf5e and 8d8bee2 fixed related flicker only; there is no stable-shadow mode or texel snapping. To try, editor first
(scripted zoom at a forced low frame rate, before / after): 1. start the first cascade at the near plane, keep the measured max; 2. pad
the range by the camera's movement since the measurement; 3. an optional stable-shadows mode (sphere fit snapped to texels).

## Smaller items

- `s_fire_map` of the plain rigid G-buffer shaders: rigidstandard samples it when `cb1[1].y == 1` (a weather type: wet streaks scrolled
  by time); the engine binds `commontextures/vfx_fire_diffuse` there for every rigid material, the renderer binds black outside the
  burning passes. Find the weather type, then bind it everywhere.

- Blended forward pass for rigid meshes (from docs/plans/tile_ted_parity.md 5d): a building below opacity 1 is filed apart by the engine
  (render flag 0x2000) and drawn through the blended forward shaders (`rigidstandard ps30_alpha_blend_40`, `rigid_forward_lighting`),
  which multiply the alpha by floor(POSITION4.w) * 0.001. The renderer has none of them: faded buildings with opaque materials show no
  fade, and alpha-blended materials (alpha_mode 2) draw opaque. Needs the forward lighting cbuffers (sun, shadows, env cube).

- GPU timer: `perf` shows "no GPU timing yet" (`crates/wce_render/src/gpu_timer.rs:196`); find why.
- Engine items (docs/re/renderer.md, "Remaining engine items"): the env cube's top-face cloud colour (needs the EnvMapGen draw traced or
  a dump); the SSAO jitter texture's generator; the cloud columns' spawn interval (assumed, `wce_scene` clouds.rs:255).
- VFX rigid lights' flicker phase: 0 for every VFX light (all fires flicker in sync); lead in docs/re/particles.md. The map's
  POINT_LIGHT_INSTANCE phase is done (`light_phase`, tile_document/view.rs).
- VFX season mask (+0x60): PARTIAL, the editor filters by `seasons`; the game's check is not traced.
- Grass black pixels: some grass pixels come out (0, 0, 0) after lighting (~23k in a 1600 x 900 forest view), likely a zero normal
  normalised to NaN. Not traced.
- Terrain LOD: distance LOD and draw-set LOD exist; geomorphing and impostors do not.
- Nice to have: VFX decal particles (burnt ground, blood and impact decals, order icons). Battle-time effects only; no tile effect or
  weather uses them. The debris rigids (pebbles, splinters) are drawn, ballistic by assumption (`particles.rs` `effect_rigids`;
  docs/re/buildings.md, "Collapses between levels"); the engine's rigid simulation is not traced (docs/re/particles.md).
- Two-tile session (second tile showed the first): likely fixed by multi-document support and 6bb1172; re-test, then drop.
