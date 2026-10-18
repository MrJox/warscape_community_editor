# Rendering: what's left (2026-10-18)

The tile view draws the game's look with the game's shaders: terrain, buildings (all mesh materials that tiles use), vegetation, grass,
decals, water (rivers, sea, lakes and the models' water planes), point lights, the placed VFX with their lights, and the weather.
This file lists what is not done.

## Next: terrain open points

From docs/re/terrain.md ("Assumptions / open"):
- **Map-level (later tilemaps scope):** neighbour tiles, the outfield, the tile's `final_alpha` feather into the map's base terrain, the
  far maps (docs/re/terrain.md, "Seasons and snow"), and the resolve's second and third layers (`s_mask_map` and
  `g_layer1/2_texture_index` come with the far maps, fn 0x118839d0; black / off for a lone tile).
- Done 2026-10-18: seasons (spring added; the game's fallback matches) and snow (the global snow normals, the instance snow switch).
- Done 2026-10-18: terrain shadow casting (the game's shadow_30_new; there are no receiving variants), and with it the game's
  depth-only shadow map and shadow depth bias for every caster.
- Done 2026-10-18: the maps' sampler states (from the shaders' declarations) and `g_overlay_view_projection` (the declared identity).
- Optional: the virtual texture path.

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

## Smaller open rendering items

- **Engine items** (docs/re/renderer.md, "Remaining engine items"), not done yet:
  - the env cube's top-face cloud colour (needs the EnvMapGen draw traced or a dump);
  - the SSAO jitter texture (made at the SSAO view +0x7c, CPU-written; the code that fills it is not found yet, leads in renderer.md);
  - the cloud columns' spawn interval (no dump with a moving cloud field).
  Done 2026-10-18: the shadow cascades' light fit (the game's LiSPSM, verified against the dump).
- **Placed VFX:** the game's use of the marker's season mask (+0x60) wasn't traced (every vanilla instance has all four seasons).
- **Buildings:** the `destruction_level` saved in tile.ted is not applied when a tile opens (upstream's piece_model convention).
- **Two-tile session:** opening a second tile keeps showing the first (predates the merge).
