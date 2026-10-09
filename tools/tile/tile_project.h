#pragma once
// One tile opened for editing. Its root is its tile database entry (_tile_database/TILES/*.xml, formats/tile_entry): the entry says where the files are
// (location + variation -> the folder with tile.ted, height_map_0.png, blend_map.tif), how big the tile is, its colour, the ground texture groups, the
// height and normal map parameters of the variation and the tile links. The project holds the entry, tile.ted (TedScene) and the height map; opening a
// bare folder or tile.ted looks its entry up from the path (indexed: formats/tile_index) and warns when there is none. Reads and writes the raw files only.
#include "formats/png_file.h"
#include "formats/tile_entry.h"
#include "tile/ted_scene.h"
#include "tile/height_field.h"

#include <filesystem>
#include <memory>
#include <string>
#include <vector>

namespace wce {

// What the tool needs from the entry every frame, copied out of it (the entry itself is TileProject::entry).
struct TileInfo {
    std::string location;               // terrain\tiles\battle\<set>\<tile>     (TILE@location)
    std::string variation;              // leaf folder, e.g. "medium", "aa"
    uint32_t cells_x = 0, cells_y = 0;  // TILE width / height in cells (128 m each)
    double min_height = 0, scale = 100; // VARIATION min_height / scale (metres)
    double normal_strength = 0, overlap_border_size = 0;   // VARIATION values (overlap: cells of the height map's border)
    uint32_t raw_data_tri_density = 0;
    bool found = false;                 // the entry and the variation were found
};

class TileProject {
public:
    // A variation folder, a tile.ted or a tile folder: the entry is resolved from the path (warning when there is none). Throws std::runtime_error.
    static std::unique_ptr<TileProject> open(const std::filesystem::path& tile_dir);
    // The entry file (_tile_database/TILES/x.xml) and one of its variations (-1 = the first; a name picks by leaf folder). Throws std::runtime_error.
    static std::unique_ptr<TileProject> open_entry(const std::filesystem::path& entry_xml, int variation = -1);
    // Writes tile.ted (when the scene is modified, or force_scene) and height_map_0.png (when height_dirty, or force_height); false if nothing was written.
    bool save(const std::filesystem::path& out_dir, std::string* message, bool force_scene = false, bool force_height = false) const;

    std::filesystem::path dir;                           // the variation folder (holds tile.ted)
    TileInfo info;
    std::unique_ptr<TileEntry> entry;                    // the tile's database entry, null when there is none (a tile.ted outside a kit)
    int variation = -1;                                  // index in entry->variations(), -1 when unknown
    std::filesystem::path raw_root;                      // the kit's raw_data folder, empty outside a kit
    std::string warning;                                 // what is off about how it opened (no entry, ...), empty when nothing
    bool has_entry() const { return entry != nullptr; }
private:
    static std::unique_ptr<TileProject> open_entry_resolved(std::unique_ptr<TileEntry> entry, int variation, const std::filesystem::path& raw_root);
public:
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

// Locates the tile database entry of a raw tile folder (<...>/raw_data/terrain/tiles/<battle|campaign>/<set>/.../<variation>) through the kit's
// index (no scan per call). The numbers of TileInfo come from the entry; `entry_out`, when given, receives the entry itself.
TileInfo find_tile_info(const std::filesystem::path& tile_dir, std::unique_ptr<TileEntry>* entry_out = nullptr, int* variation_out = nullptr, std::filesystem::path* raw_root_out = nullptr);
// The raw_data folder an entry file or a tile path lives under (the part before terrain/tiles), empty when there is none.
std::filesystem::path kit_raw_root(const std::filesystem::path& path);

} // namespace wce
