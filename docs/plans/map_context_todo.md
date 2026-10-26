# Map context: to do

Status (2026-10-26). Done: neighbour tiles (the Tile panel's Map context, docs/re/battle_maps.md). The context draws each neighbour's
editing heights (`height_map_0.png`) and a synthetic weight feather (`ALPHA_TEXEL` 4.0 in
`crates/wce_tile_editor/src/tile_document/context/template.rs`; cache `VERSION` 3 in `context/cache.rs`), so neighbours meet with steps
and seams the game does not have. The export's edge blend (heights pulled to the links' profile within 64 m, `final_alpha` fading to 0
along land edges) is in docs/re/battle_maps.md, "Tile seams". This plan owns all map-level terrain.

## Tile seams

1. **Context tiles from the export's maps.** Templates read `final_heights.dds` (1025 x 1025 R32F, the tile without its border, 1 m a
   pixel) and `final_alpha.dds` when the tile folder has them (900 of 915 kit tiles); the 15 never-exported tiles keep the current
   heights and feather. Bump the cache `VERSION`.
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
  `crates/wce_render/src/renderer/terrain.rs` binds `g_layer1_texture_index` 0 and `s_mask_map` black.
- Optional: the virtual texture path (docs/re/terrain.md).

## Large map contexts: performance

Measured 2026-10-21 on western_city_rome/medium, sea on, 64 neighbour cells (192 tiles, 1.38 M draw items): 23 fps, CPU-bound; 42 fps
after the terrain blend's constants / boxes set once and the sea as a quadtree. Left:
- Culling by area: PARTIAL. 43e983c made removal cheap (dead items skipped and their slots reused, the incremental update 3.1 -> 0.6 ms); culling still walks every item per view (13 views a frame, ~6.5 ms):
  skip a whole far tile at once.
- The environment cube redrawn every 4th frame at ~9 ms (a stutter): spread its faces over frames, or skip far items in it.
- The shadow cascades' instance upload (2.2 ms) and the terrain shadow draws (one per terrain entity per cascade, 3 ms).
- The GPU side is unmeasured (the GPU timer item is in rendering_remaining.md).
