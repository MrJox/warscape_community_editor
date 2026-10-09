#pragma once
// One tile opened for editing: its tile.ted scene, its height map and the tile database entry that gives the
// height scale and the footprint. Reads and writes the raw files only.
#include "formats/png_file.h"
#include "scene/ted_scene.h"
#include "terrain/height_field.h"

#include <filesystem>
#include <memory>
#include <string>
#include <vector>

namespace te {

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
    bool save(const std::filesystem::path& out_dir, std::string* message) const;      // writes tile.ted / height_map_0.png; false if nothing to save

    std::filesystem::path dir;
    TileInfo info;
    TedScene scene;
    std::vector<uint8_t> ted_original;
    bool has_height = false;
    PngFile height_png;
    std::unique_ptr<HeightField> height;
    bool height_dirty = false;
    double extent_m() const { return info.found ? info.cells_x * 128.0 : 256.0; }   // tile side in metres (square tiles; x extent)
    double unit_scale = 2.0;                                                           // scene units per metre horizontally
    bool scene_modified() const { return scene.modified(); }
};

// Locates the tile database entry of a raw tile folder (<...>/raw_data/terrain/tiles/<battle|campaign>/<set>/.../<variation>).
TileInfo find_tile_info(const std::filesystem::path& tile_dir);

} // namespace te
