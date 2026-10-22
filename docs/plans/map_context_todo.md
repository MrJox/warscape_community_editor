# Map context: to do

## Tile seams (the export's edge blend)

Findings: `docs/re/battle_maps.md`, "Tile seams". TEd's / BOB's export pulls a tile's heights within 64 m of its edges to a profile of the
edge's links (0 m on land, the sea's -20 on sea links, a ramp on `sea_coast`) and fades its weight (`final_alpha`) to 0 along land edges;
the results are in every exported kit tile (`final_heights.dds`, `final_alpha.dds`; 900 of 915). The context draws the editing heights
(`height_map_0.png`) and a synthetic 4-unit alpha feather instead, so neighbours meet with steps and seams the game does not have.

1. **Context tiles from the export's maps.** Templates read `final_heights.dds` (1025 x 1025 R32F, the tile without its border, 1 m a
   pixel) and `final_alpha.dds` when the tile folder has them; the 15 never-exported tiles keep the current heights and feather. A cache
   format version step.
2. **The band where neighbours' weights are both near 0.** Something map-wide shows through there in the game: the map's default texture
   groups (`config.xml`: grass, forest, mud, rock) in the cell's climate, the far / outfield maps, or another layer. Engine side
   (`empire.retail.dll`: the terrain blend and resolve passes, how a tile's `final_alpha` combines with what lies under the tiles):
   trace it before implementing.
3. **The open tile.** The user edits the pre-blend heights, so its edges do not match the game until it is exported. Options: draw
   `final_heights.dds` while the tile is unchanged since its export, or preview the blend live (needs how BOB uses the links'
   `blend_size`, `no_offline_blend`, `<blend_quad/>` and the tile sets' `linking_tile`: BOB_Tile / BOB_Terrain, the
   `blend_border_size > 0 && blend_border_size < m_settings.m_grid_size.x/2` assertion is a starting point).
