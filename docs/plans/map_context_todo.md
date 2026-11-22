# Map context: to do

Status (2026-11-22). Done: neighbour tiles (the Tile panel's Map context, docs/re/battle_maps.md). Since 2026-10-31 a neighbour draws
BOB's `final_heights.dds` when it is not older than the tile (else its editing heights, `height_map_0.png`) and the game's alpha texture
(`template.rs` `game_alpha`: the entry's custom one, else `terrain/textures/<kind>/alpha/<w>x<h>.dds`), so tiles fade into each other as the
game's do; the synthetic feather (`ALPHA_TEXEL` 4.0 in `crates/wce_tile_editor/src/tile_document/context/template.rs`) is left only for a
tile the game has no alpha texture for. Tiles whose heights end at their edge (final heights, decompiled) and a tile's masked subtiles
slope down out of the way (2026-11-16). The context's frame, rings and streamer serve the Terrain tool too ([terrain_editor.md](terrain_editor.md)).
The export's edge blend (heights pulled to the links' profile within 64 m, `final_alpha` fading to 0 along land edges) is in
docs/re/battle_maps.md, "Tile seams". This plan owns all map-level terrain.

## Tile seams

1. Done 2026-10-31 (above): heights from `final_heights.dds`; the alpha from the game's alpha texture rather than `final_alpha.dds`.
2. **Where both neighbours' weights are near 0.** Something map-wide shows through in the game: `config.xml`'s default texture groups in
   the cell's climate, the far / outfield maps, or another layer. Trace the engine side first (`empire.retail.dll`: the terrain blend and
   resolve passes, how `final_alpha` combines with what lies under the tiles).
3. **The open tile's edges.** The user edits pre-blend heights, so its edges don't match the game until exported. Options: draw
   `final_heights.dds` while the tile is unchanged since its export, or a live blend preview (needs how BOB uses the links'
   `blend_size`, `no_offline_blend`, `<blend_quad/>` and the tile sets' `linking_tile`; BOB_Tile / BOB_Terrain's
   `blend_border_size > 0 && blend_border_size < m_settings.m_grid_size.x/2` assertion is a starting point).

## Map-level terrain (from docs/plans/rendering_remaining.md)

- The tile's `final_alpha` feather into the map's base terrain.
- The far maps (docs/re/terrain.md, "Seasons and snow") and the outfield.
- The resolve's second and third layers: `s_mask_map` and `g_layer1/2_texture_index` come with the far maps (fn 0x118839d0);
  `crates/wce_render/src/renderer/terrain.rs` binds `g_layer1/2_texture_index` 0, and `s_mask_map` black but on a campaign map, where it
  is the season snow layer's mask with the campaign's far material (2026-11-05).
- Optional: the virtual texture path (docs/re/terrain.md).

## Large map contexts: performance

Measured 2026-10-21 on western_city_rome/medium, sea on, 64 neighbour cells (192 tiles, 1.38 M draw items): 23 fps, CPU-bound; 42 fps
after the terrain blend's constants / boxes set once and the sea as a quadtree. Left:
- Culling by area: PARTIAL. 43e983c made removal cheap (dead items skipped and their slots reused, the incremental update 3.1 -> 0.6 ms).
  2026-11-02 (docs/plans/terrain_editor.md, Phase 2 "Frame time"): the terrain entities are made ready once a frame and culled by their
  box before their patches; the sea bed, the selection outline and the environment cube no longer go over every entity a frame. The draw
  set's culling is 0.18 ms a view around Rome (1.8 ms for 10 views). 2026-11-07: past the tiles' ring the far ground goes by blocks of
  chunks, a block's tiles merged into one terrain (rom_third_age_map whole at 22 fps); skipping a whole far tile of the near rings at once
  is what is left of this item.
- The environment cube redrawn every 4th frame at ~9 ms (a stutter): its cost was finding the ground under the battle centre over every
  entity (cached by the scene's revision since 2026-11-02; the cube draws the sky and clouds only).
- The shadow cascades' instance upload (2.2 ms) and the terrain shadow draws (one per terrain entity per cascade, 3 ms).
- The GPU side is unmeasured (the GPU timer item is in rendering_remaining.md).
