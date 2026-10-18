# Rendering: what's left (2026-10-18)

The tile view draws the game's look with the game's shaders: terrain, buildings (all mesh materials that tiles use), vegetation, grass,
decals, water (rivers, sea, lakes and the models' water planes), point lights, the placed VFX with their lights, and the weather.
This file lists what is not done.

## Next: terrain open points

From docs/re/terrain.md ("Assumptions / open"):
- **Neighbour tiles and the outfield:** the kit's `final_alpha.dds` feather (0 at the edge, ramping to 1 over about 25 m), and the overlap
  between tiles. The alpha map is 1 everywhere for now.
- **Far maps:** the map-wide normal map, the far diffuse and normal maps and the mask map (BOB's outfield data). These are map-level and
  switched off (`g_far_tile_distance` / `g_far_blend_range` very large, `s_mask_map` black).
- **Season mixing:** `g_array_section_sizes` is (1, 1, 0, 0), so the season and climate indices don't move the slot. The game uses
  (8, 5, 13).
- **Snow:** `s_snow_normals` is a flat map. The snow branch needs `g_snowing_intensity` > 0.
- **Shadow-receiving terrain shader variants**, and the resolve's second and third layer indices (the mask is black, so they stay off).
- **Unverified state:** the maps' sampler states (clamp linear here) and `g_overlay_view_projection` (identity). No in-game capture to
  compare against yet.
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

- **Engine items** (docs/re/renderer.md, "Remaining engine items"): the shadow cascades' light fit (the engine fits a tight box, the
  editor a bounding sphere), the env cube's top-face cloud colour, the SSAO jitter texture, the cloud columns' spawn interval.
- **Placed VFX:** the game's use of the marker's season mask (+0x60) wasn't traced (every vanilla instance has all four seasons).
- **Buildings:** the `destruction_level` saved in tile.ted is not applied when a tile opens (upstream's piece_model convention).
- **Two-tile session:** opening a second tile keeps showing the first (predates the merge).
