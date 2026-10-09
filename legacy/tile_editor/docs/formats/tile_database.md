# tile_database.bin (FASTBIN0)

Produced by BOB's Tile processor, action "Process tile database" (consumer: `_tile_database/_settings.xml`; 90 ms). Reimplemented in `src/formats/tile_database.*`, driven by `tools/tdb_build`.

Sources: `_tile_database/_settings.xml` (RENDER_PARAMS, CONVERSION_PARAMS, CLIMATES, TILE_SETS) and `_tile_database/TILES/*.xml`; for battle also the 6 generated `working_data/.../_ak_bf_tile_database/TILES/assembly_kit_<map>.xml`.

Layout: see the header comment of `tile_database.h` (all fields). Findings:
- The binary field order is the C++ struct order, not the XML attribute order (RENDER_PARAMS: lf_height, hf_height, blend_pixel_scale, unit_scale, mid_distance_*, vertical_offset, normal_*, blend_contrast, 4 layer_exempt strings, sea/lake/near_distance values, mid_distance_detail_near_lq). CONVERSION_PARAMS stores `triangle_density` before `max_lf_heights_per_pixel` (battle has 128 for both, the campaign file disambiguates: 4 and 128).
- RENDER_PARAMS is always written as version 6, even when the XML says 3 (campaign `_settings.xml`).
- TILE_SET string slots after the name: linking_tile, shared_geometry, also_place_tile_set, link_as_set (the middle two are not told apart by the data: always empty or `also_place_tile_set`; the battle slot 2 value is `river`/`sea_coast`/`roads...` = also_place_tile_set).
- Tile sets referenced by a tile but missing in `_settings.xml` are appended in order of first reference (battle: `assembly_kit`); the battle database additionally ends with a `user` set, the campaign one does not.
- TILE: u16 ver 3, str location, str tile_set, str mask, u32 width, u32 height, 3 x f32 colour, u8 requires_infield_lodding, u8 random_rotatable, str custom_alpha_blend_texture, u8 scalable, u8 encampable, str custom_blend_tile, 4 x str TEXTURE_GROUP (red, green, blue, alpha), u32 n variations {u16 ver 2, str location, f32 min_height, scale, normal_strength, overlap_border_size, u32 raw_data_tri_density, 3 x str (blend_common, index_common, normal_common), 3 x f32 colour, u8 barbarian}, u32 n link targets {u16 ver, str tile_set, i32 x, i32 y}, u32 n links {u16 ver, str link_set, i32 x, y, base_x, base_y, u8 is_entry, u32 0 (blend_quad, empty in all shipped XML), u32 blend_size, u8 no_offline_blend, str test}.
- **Tile order**: footprint area (width x height) descending, ties in an order that no tested rule reproduces (input order alphabetical by file or location, std::sort / stable_sort / heap sort with MSVC 2022: at most 276 of 699 in place). The builder therefore takes an explicit order list (`TileDbInput::order`); new tiles default to area descending then location. The game most likely does not depend on it.

Verification (2026-10-09):
- Battle, 699 TILES xml as sources, real BOB run in a scratch kit vs `tdb_build` with BOB's order: **0 differing bytes** (355,209 bytes).
- Shipped battle file (705 tiles incl. 6 assembly_kit): 283 differing bytes, all inside the first two assembly_kit tiles (the generated XML on disk is newer than the binary).
- Shipped campaign file (526 tiles): 5 differing bytes, one float in one tile's variation (XML newer than the binary).
