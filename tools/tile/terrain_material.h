#pragma once
// The ground of a tile for the game's own terrain shaders (docs/re/terrain.md, libs/render/terrain_pass.cpp). The shaders read, per tile: a blend map (the weights of
// the entry's four TEXTURE_GROUPs), an index map (which of a group's four variations), a normal map, and the climate's diffuse / normal textures. BOB makes the first
// two from the tile's blend_map.tif (16 weights per pixel = group x variation): blend = the sum of a group's four weights, index = the weighted mean variation / 3
// (checked against BOB's blend.dds / index.dds of the osgiliath test map: correlation 0.998, the rest is the DXT5 compression); the normal map comes from the
// heights (TEd's tile_normal.tga is the same thing: R = -dh/dx, G = -dh/dz of the same grid, correlation 0.97 / 0.98 with the height map's gradient), so it follows
// a brush stroke.
#include "formats/tile_climate.h"
#include "formats/tile_entry.h"
#include "scene/render_scene.h"
#include "tile/terrain_bake.h"
#include "vfs/vfs.h"

#include <filesystem>
#include <functional>
#include <memory>
#include <string>

namespace wce {

struct MaterialRequest {
    std::filesystem::path folder;            // the variation folder: blend_map.tif, tile_normal.tga
    TileTextureGroup groups;                 // from the entry: red, green, blue, alpha group names
    double x0 = 0, z0 = 0, x1 = 0, z1 = 0;   // the area the maps cover (the bordered tile), tile frame metres
    double y0 = -100, y1 = 1000;             // vertical extent of the terrain (for the box that draws it)
    TerrainLook look;
    uint64_t id = 1;
};

// Null with `why` when the tile has no usable blend map. `report` gets one line for the log / the panel. The normal map is left flat: set it with terrain_normal_image().
std::shared_ptr<TerrainMaterial> build_terrain_material(const MaterialRequest& rq, const ClimateList& climates, IVfs& vfs, std::string* why, std::string* report);

// The normal map (.a = x, .g = z of the unit normal * 0.5 + 0.5) of a height grid (nx x nz samples, `step_x` / `step_z` metres apart, row j along +z), as a w x h image
// of the same grid. Central differences like TEd's.
TerrainImage terrain_normal_image(const std::vector<float>& heights, int nx, int nz, double step_x, double step_z, uint32_t w, uint32_t h);

} // namespace wce
