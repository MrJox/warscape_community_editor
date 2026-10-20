# Rendering: what's left (2026-10-18)

The tile view draws the game's look with the game's shaders: terrain, buildings (all mesh materials that tiles use), vegetation, grass,
decals, water (rivers, sea, lakes and the models' water planes), point lights, the placed VFX with their lights, and the weather.
This file lists what is not done.

## Next: terrain open points

From docs/re/terrain.md ("Assumptions / open"):
- **Map-level (later tilemaps scope):** neighbour tiles (done 2026-10-19 as the Tile panel's Map context, docs/re/battle_maps.md; their
  shading normals without the LF slope, no borders / feathering), the outfield, the tile's `final_alpha` feather into the map's base terrain, the
  far maps (docs/re/terrain.md, "Seasons and snow"), and the resolve's second and third layers (`s_mask_map` and
  `g_layer1/2_texture_index` come with the far maps, fn 0x118839d0; black / off for a lone tile).
- Done 2026-10-18: seasons (spring added; the game's fallback matches) and snow (the global snow normals, the instance snow switch).
- Done 2026-10-18: terrain shadow casting (the game's shadow_30_new; there are no receiving variants), and with it the game's
  depth-only shadow map and shadow depth bias for every caster.
- Done 2026-10-18: the maps' sampler states (from the shaders' declarations) and `g_overlay_view_projection` (the declared identity).
- Optional: the virtual texture path.
- **Done (2026-10-20): terrain detail.** The ground was blurry against the game's (rock, beaches) because the editor drew the game's
  shaders in its half-size world: the tile scene is the battle world / unit_scale (2), the game's shaders work in game units, so every
  world-space value (texture repeats, detail and fade distances, fog, the grass reach, decal tiling) ran at half frequency. The renderer
  now gives the shaders the world in game units (`RenderScene::game_units`, docs/re/renderer.md, "Scene units and game units").
- **Done (2026-10-20): the battle sea (ocean.hlsl, its surfaces, whitecaps and wind); open: the radial mesh and its displacement, the
  sea bed texture's real content** (docs/re/water.md). Was: **shore foam on the battle sea.** The game draws a band of breaking-wave foam where the sea meets the
  shore; the battle `waterplane` shader has no foam input. The engine has a coast-foam ocean (`ocean2.hlsl`, the "SeaV2" component:
  `terrain/ocean/coast_foam_mips`, `land_1024_coast`, `coast_amplitude_mask_1`, `ocean_v2_coast` cbuffer), but a memory dump of a coastal
  battle shows the sea there is drawn by the Sea Display with `ocean.hlsl` (foam, sea and swell surfaces, a sea bed info texture), not
  waterplane and not ocean2: docs/re/water.md, "The battle sea of a coastal battle".

## Battle sea: needs a GPU capture of the game (later)

The battle sea draws with the game's shaders and data (docs/re/water.md, "The battle sea of a coastal battle"; crates/wce_render/src/
renderer/ocean.rs and sea_surface.rs). Two inputs are still stand-ins, because their content only exists on the GPU: neither the
decompiled code (the fill wasn't found) nor a CPU memory dump shows it.

- **The radial mesh and its displacement.** The game draws the sea as an 80 x 164 radial mesh around the camera (fn 0x116e47e0's
  class: a sin / cos table of 80 segments, 164 rings, `[0x24]`, `[0x25]` = 43.2, 2.72 at the dumped camera). The editor uses a grid
  (4 m near, 64 m beyond the sea bed square), displaced by the blended sea and swell heights. Unknown: the rings' radii, and how the
  vertices are displaced (heights only, or the slope images too; scaling).
- **The sea bed info texture.** The ocean material binds a 1024 x 1024 texture loaded from a file (Sea Display slot 0x1a), most likely
  a 2048 m cut of `terrain/battles/<map>/lf_sea_height_map.dds`. The editor builds it from the scene's terrain heights (512 x 512),
  decoded the same way by the shader (`r * 25 - 20`). It only changes how the waves flatten near the shore.

**How to settle both:** a RenderDoc (or PIX) capture of Attila (DX11) in a coastal battle, the shore in view (as the 2026-10-19
screenshots). In the capture, find the ocean draw (`ocean.hlsl` vs30_main / ps30_main_*_water_quality) and save:
1. its vertex buffer (POSITION0 displaced, POSITION1 rest, TEXCOORD0) with the camera at that draw: gives the rings' radii and, against
   the sea / swell frames of the same time (`g_foam_anim_data` gives the sea frame), the displacement formula;
2. the texture bound to `g_sea_bed_info_texture` (VS t0): its format and pixels, to compare with the battle map's
   `lf_sea_height_map.dds` and with the terrain heights;
3. the constant buffers `ocean_VS` and `OceanSharedConstants`, to recheck the values the editor takes from the tables.

## Buildings: to verify in game (later)

Siege engines posed in their animations' first frame, walls' bridging columns and neighbour destruction, and the pieces' `.meta`
effects (torch fires, burning buildings) are implemented from the files alone: the findings and every assumption, each with how to
check it, are in docs/re/buildings.md.

## Mod assets to review (later)

- **TDD's dead tree bark is near black (2026-10-20).** `tdd_pack6_weather_1.0.0.pack` overrides
  `battleterrain/vegetation/trees/textures/dead_tree/dead_tree_{dry,wet}_diffuse.dds` and their `_ab` variants with 512 x 2048 textures
  of dark mossy bark (mean colour about 40, 39, 32) where vanilla's are 256 x 1024 light grey (dry about 133, 128, 119; wet 86, 84, 71).
  The dead tree stumps of the TDD tiles (710 `dead_tree.dry_dead_treek` on gen_dun_medium_final_2) render near black in the editor;
  with vanilla packs only, the same camera shows light grey bark, as TEd does. The editor draws what the pack holds (checked: the model,
  normal map, seasonal lookup, burn map, shadows and SSAO play no part). To do: check the stumps in game with the same packs, and tweak
  (brighten) the mod's texture if they are dark there too.
- Side finding, not acted on: the dead tree models are wound the other way round from the pines (normals along the triangle winding,
  the pines' against it). Drawing trees two-sided (most of the tree material's rasterizer descriptions, fn 0x116e5130, are CULL_NONE)
  changed nothing visible; which description the G-buffer pass uses is not traced.

## Nice to have (future update, not done)

- **VFX debris and decal particles:** rigid emitters whose model isn't a point light, i.e. pebbles, splinters, mud, ship debris, the
  burnt-ground, blood and impact decals, and the order icons. Each rigid is a body of the engine's rigid simulation (fn 0x11829920:
  velocity times owner +0x38, orientation quaternion, angular velocity, placed by fn 0x1184cf80). They only appear in battle-time effects
  (impacts, explosions, burning ground, naval ramming / beaching, blood, UI icons). No tile effect and no battle weather uses them, so a
  tile never shows one unless a modder places such an effect.
- **Fire light flicker phase:** the phase of a rigid point light's animation comes from the render parameters (+0x30) handed to the
  rigid model render (fn 0x117f9800). These are only passed through virtual calls, and the static trace didn't reach where they are
  filled. The editor uses 0, so all fire lights flicker in sync; each one's brightness pattern is right. Quickest way to settle it: in a
  battle with fires on screen, break on fn 0x118dc650 (the named-light submit) and log its fourth argument, or take a memory dump and
  read the live rigid-model instances.

## Shadow stability (later)

The game's shadows flicker and "retreat" (worse at low fps); the renderer reproduces it, since it follows the game's code. Causes from the
code (not yet isolated one by one in captures):
- **Stale split distances:** the cascades are scaled to the depth buffer analysis (min / max visible depth), read back two frames late
  (as ported; the game's own latency not verified) and converted with the current camera. At low fps that is more camera movement:
  zooming in, the first cascade starts past the nearest geometry and the near shadows come back over the next frames.
- **Per-frame fit:** the LiSPSM warp and the tight box are refitted every frame, not snapped to texels: edges crawl as the camera moves.
- A shader patch can't fix either (the matrices and splits come from the CPU); it could only widen the cascade blend / end fade.
- To try, editor first (scripted zoom at a forced low frame rate, before / after): 1. start the first cascade at the camera's near
  plane, keep the measured max; 2. pad the range by the camera's movement since the measurement; 3. an optional "stable shadows" mode
  (sphere fit snapped to texels, as before the LiSPSM port). In the game: the `warp_shadows` / `split_d0..3` tweakables (if a release
  build can set them), or a DLL patch of the split setup (fn 0x11784c00).

## Smaller open rendering items

- **Engine items** (docs/re/renderer.md, "Remaining engine items"), not done yet:
  - the env cube's top-face cloud colour (needs the EnvMapGen draw traced or a dump);
  - the SSAO jitter texture (made at the SSAO view +0x7c, CPU-written; the code that fills it is not found yet, leads in renderer.md);
  - the cloud columns' spawn interval (no dump with a moving cloud field).
  Done 2026-10-18: the shadow cascades' light fit (the game's LiSPSM, verified against the dump).
- **Placed VFX:** the game's use of the marker's season mask (+0x60) wasn't traced (every vanilla instance has all four seasons).
- **Grass black pixels:** some grass pixels come out pure black (0, 0, 0) after lighting (~23k pixels in a 1600 x 900 forest view),
  likely a zero normal (the grass normal is alpha-blended into the G-buffer) normalised to NaN. Not traced yet.
- **Two-tile session:** opening a second tile keeps showing the first (predates the merge).
