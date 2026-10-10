#pragma once
// The ground colour of a tile (Phase 3 of docs/plans/tile_ted_parity.md): the tile's blend map (blend_map.tif, 16 weights per pixel = the four TEXTURE_GROUPs of
// the tile database entry x their four variations, normalised to 1) mixes the textures of the climate (terrain/textures/battle/<climate>/<group>/<variation>_
// <season>_diffuse.dds), each repeating every `blend_pixel_scale` metres of the climate's texture list. The result is one RGBA8 image over the bordered tile
// (the area of the height map) that the renderer cuts into the terrain patches' textures. This is the CPU stand-in for the game's blend pass (docs/re/terrain.md):
// the same inputs and the same weights, baked once per tile / climate / season instead of evaluated per pixel; it carries no normal detail and no
// mid-distance layers yet.
#include "formats/tile_climate.h"
#include "formats/tile_entry.h"
#include "vfs/vfs.h"

#include <filesystem>
#include <memory>
#include <string>
#include <vector>

namespace wce {

// What the ground shows: the blended colour, or one of TEd's debug views of the tile's maps (Phase 3d).
enum class TerrainView { Colour, BlendGroups, GroundTypes, Protection };

struct TerrainLook {
    bool blended = true;                     // false: the interim tiled single texture
    std::string climate = "ROME_Continental";
    std::string season = "su";               // su | au | wi
    TerrainView view = TerrainView::Colour;  // anything but Colour needs `blended`
    bool operator==(const TerrainLook& o) const { return blended == o.blended && climate == o.climate && season == o.season && view == o.view; }
    bool operator!=(const TerrainLook& o) const { return !(*this == o); }
};

struct BakedColour {
    double x0 = 0, z0 = 0, w_m = 0, h_m = 0; // the world rectangle the image covers, metres (tile frame)
    uint32_t w = 0, h = 0;
    std::vector<uint8_t> rgba;               // sRGB, w * h * 4, rows along +z
    std::string climate, season;             // what was used
    int layers = 0;                          // (group, variation) layers with any weight
    int missing = 0;                         // of those, the textures that were not found (drawn as the weighted rest)
    std::string report;                      // one line for the log / the panel
    double texels_per_metre() const { return w_m > 0 ? double(w) / w_m : 0; }
};

struct BakeRequest {
    std::filesystem::path blend_map;         // blend_map.tif of the variation folder
    TileTextureGroup groups;                 // from the entry: red, green, blue, alpha group names
    std::filesystem::path folder;            // the variation folder (ground_type_map.png, protection_map.dds for the debug views)
    double x0 = 0, z0 = 0, w_m = 0, h_m = 0; // the area the blend map covers (the bordered tile)
    TerrainLook look;
    uint32_t max_dim = 2048;                 // largest side of the baked image
    double max_texels_per_metre = 4.0;
};

// The ground type map and the protection map as images (view GroundTypes / Protection), over the area they cover. Null with `why` when the file is missing.
std::shared_ptr<BakedColour> bake_terrain_map_view(const BakeRequest& req, std::string* why);

// Null with `why` when the tile has no usable blend map or no texture was found at all.
std::shared_ptr<BakedColour> bake_terrain_colour(const BakeRequest& req, const ClimateList& climates, IVfs& vfs, std::string* why);

} // namespace wce
