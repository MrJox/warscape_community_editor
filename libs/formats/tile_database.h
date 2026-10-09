#pragma once
// Tile database: _tile_database/_settings.xml + TILES/*.xml  ->  tile_database.bin (FASTBIN0 version 1).
//
// This is BOB_Tile's "Process tile database" action reimplemented. Layout (little endian, `str` = u16 length + bytes):
//   "FASTBIN0" u16 1
//   RENDER_PARAMS:    u16 6 (BOB writes the current version whatever the XML says), 16 x f32, 4 x str layer_exempt_0..3, 7 x f32
//                     (attribute order in kRenderSlots, tile_database.cpp)
//   CONVERSION_PARAMS u16 version, u32 triangle_density, u32 max_lf_heights_per_pixel, 6 x f32 size factors, 6 x f32 angle factors
//   u32 n_climates, per CLIMATE: u16 ver, str name, str texture_set, f32 red, green, blue, u32 n_textures,
//        per TEXTURE: u16 ver, str name, 11 x f32 (mid_distance_strength, 4 x (lerp_reflectivity, lerp_smoothness),
//        blend_pixel_scale, outfield_blend_pixel_scale); then f32 grass_alpha_add, f32 grass_alpha_mul, str destruction_climate
//   u32 n_tile_sets, per TILE_SET: u16 ver, str name, str linking_tile, str shared_geometry, str also_place_tile_set,
//        str link_as_set, f32 red, green, blue   (sets referenced by tiles but undefined, e.g. assembly_kit, are appended, then "user" for the battle database)
//   u32 n_tiles, per TILE (see tile_database.cpp)
// Verified: byte-identical to real BOB output for the same sources (battle, 699 tiles), and to the shipped battle and campaign
// files except where the XML on disk is newer than the binary (2 assembly_kit tiles; one float in a campaign tile)
// (docs/formats/tile_database.md). The tile order in the shipped file follows no recoverable
// rule; build() takes an explicit order and defaults to footprint area descending, then location.
#include <cstdint>
#include <string>
#include <vector>

namespace wce {

struct TileDbInput {
    std::vector<uint8_t> settings_xml;
    std::vector<std::vector<uint8_t>> tile_xml;       // one per TILES/*.xml (and per generated assembly_kit tile)
    std::vector<std::string> order;                   // tile locations in output order; empty = default order
    bool add_user_tile_set = true;                    // the battle database always ends with a "user" set, the campaign one does not
    // Tile sets that tiles reference but _settings.xml does not define (e.g. assembly_kit) are appended automatically.
};

// Returns the bytes of tile_database.bin. Throws std::runtime_error on malformed input.
std::vector<uint8_t> build_tile_database(const TileDbInput& in);

// Locations of the tiles of a shipped tile_database.bin in file order (to reuse its ordering).
std::vector<std::string> tile_database_order(const std::vector<uint8_t>& bin);

} // namespace wce
