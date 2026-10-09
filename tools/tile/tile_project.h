#pragma once
// One tile opened for editing: its tile.ted scene, its height map and the tile database entry that gives the
// height scale and the footprint. Reads and writes the raw files only.
#include "formats/png_file.h"
#include "tile/ted_scene.h"
#include "tile/height_field.h"

#include <filesystem>
#include <memory>
#include <string>
#include <vector>

namespace wce {

struct TileInfo {
    std::string location;               // terrain\tiles\battle\<set>\<tile>     (TILE@location)
    std::string variation;              // leaf folder, e.g. "medium", "aa"
    uint32_t cells_x = 0, cells_y = 0;  // TILE width / height in cells (128 m each)
    double min_height = 0, scale = 100; // VARIATION min_height / scale (metres)
    bool found = false;
};

class TileProject {
public:
    static std::unique_ptr<TileProject> open(const std::filesystem::path& tile_dir); // throws std::runtime_error
    // Writes tile.ted (when the scene is modified, or force_scene) and height_map_0.png (when height_dirty, or force_height); false if nothing was written.
    bool save(const std::filesystem::path& out_dir, std::string* message, bool force_scene = false, bool force_height = false) const;

    std::filesystem::path dir;
    TileInfo info;
    TedScene scene;
    bool has_height = false;
    PngFile height_png;
    std::unique_ptr<HeightField> height;
    bool height_dirty = false;
    double extent_m() const { return info.found ? info.cells_x * 128.0 : 256.0; }   // tile side in metres along x
    double extent_x() const { return extent_m(); }
    double extent_z() const { return info.found && info.cells_y ? info.cells_y * 128.0 : extent_m(); }   // along z (the height map's rows)

    // Where the height map sits. Measured on the shipped tiles (docs/formats/tile_ted.md): the map has a border of one cell (128 m) on every side,
    // (cells + 2) * ppc pixels per axis, one pixel per 128 / ppc metres, pixel (0, 0) at (-128, -128) m, no flip; instance heights then sit on
    // the terrain (mean error 0.7 m on western_city_rome, 1.9 m with the map stretched over the tile). Without a tile database entry (or when the
    // sizes do not fit) the map is stretched over the tile as before.
    bool bordered() const { return has_height && info.found && info.cells_x && info.cells_y && height->width() % (info.cells_x + 2) == 0 &&
                                   height->height() % (info.cells_y + 2) == 0 && height->width() / (info.cells_x + 2) == height->height() / (info.cells_y + 2); }
    double border_m() const { return bordered() ? 128.0 : 0.0; }
    double mpp_x() const { return has_height ? (bordered() ? 128.0 / (height->width() / (info.cells_x + 2)) : extent_x() / (height->width() - 1)) : 1.0; }
    double mpp_z() const { return has_height ? (bordered() ? 128.0 / (height->height() / (info.cells_y + 2)) : extent_z() / (height->height() - 1)) : 1.0; }
    double pixel_x(double x_m) const { return (x_m + border_m()) / mpp_x(); }
    double pixel_z(double z_m) const { return (z_m + border_m()) / mpp_z(); }
    double height_at(double x_m, double z_m) const { return has_height ? height->sample(pixel_x(x_m), pixel_z(z_m)) : 0.0; }
    double unit_scale = 2.0;                                                           // scene units per metre horizontally
    bool scene_modified() const { return scene.modified(); }
};

// Locates the tile database entry of a raw tile folder (<...>/raw_data/terrain/tiles/<battle|campaign>/<set>/.../<variation>).
TileInfo find_tile_info(const std::filesystem::path& tile_dir);

} // namespace wce
