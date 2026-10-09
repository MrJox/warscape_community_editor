#pragma once
// A tile's entry in the tile database: _tile_database/TILES/<set>_<tile>.xml. The entry is the root of a tile (docs/plans/tile_ted_parity.md, section 0):
// it says where the files are (TILE@location + VARIATION@location -> the leaf folder that holds tile.ted, height_map_0.png, blend_map.tif ...), how big
// the tile is, its colour on the map tools, the four ground texture groups, per variation the height and normal map parameters, and the tile links.
// tile_database.bin is BOB's compiled form of all entries (formats/tile_database).
//
// The document is the lossless XmlDoc (byte spans, nothing re-serialised): an unedited entry saves byte for byte, an edit later changes only its
// span. Typed access reads the attributes on demand; `unknown_names` lists what the typed access does not cover (nothing in the 1,855 shipped files).
#include "formats/xml_doc.h"

#include <cstdint>
#include <filesystem>
#include <string>
#include <vector>

namespace wce {

struct TileTextureGroup {
    std::string red, green, blue, alpha;   // texture group names of the four blend layers (e.g. settlement_1, forest)
};

struct TileVariation {
    int elem = -1;                          // element index in the entry's XmlDoc
    int serialise_version = 0;
    std::string location;                   // leaf folder name under the tile's location (battlefield, minor, medium, ...)
    double min_height = 0, scale = 100;     // metres: height = min_height + raw / 65535 * scale
    double normal_strength = 0, overlap_border_size = 0;   // overlap_border_size: cells of overlap with the neighbours (the height map's border)
    uint32_t raw_data_tri_density = 0;
    std::string blend_common, index_common, normal_common;
    double red = 0, green = 0, blue = 0;
    bool barbarian = false;
};

struct TileLinkTarget {
    int elem = -1;
    int serialise_version = 0;
    std::string tile_set;                   // the kind of join this tile offers at its edge cell
    int x = 0, y = 0;                       // cell of the tile (128 m cells)
};

struct TileLink {
    int elem = -1;
    int serialise_version = 0;
    std::string link_set;                   // sea, river, roads, sea_coast, ...
    int x = 0, y = 0;                       // the cell that has to (TLT_EQUALS) or must not (TLT_NOT_EQUALS) belong to link_set; x = -1 or width is the neighbour column
    int base_x = 0, base_y = 0;
    bool is_entry = false;
    uint32_t blend_size = 0;                // pixels the neighbour's terrain is blended into this tile at that edge
    bool no_offline_blend = false;
    std::string test;                       // TLT_EQUALS | TLT_NOT_EQUALS
    bool has_blend_quad = false;            // the <blend_quad/> child (empty in every shipped file)
};

class TileEntry {
public:
    static TileEntry parse(std::vector<uint8_t> bytes);           // throws std::runtime_error (not a TILE document, malformed XML)
    static TileEntry load(const std::filesystem::path& xml);      // remembers the path; throws
    std::vector<uint8_t> save() const { return doc_.save(); }
    bool modified() const { return doc_.modified(); }
    const XmlDoc& doc() const { return doc_; }
    XmlDoc& doc() { return doc_; }
    const std::filesystem::path& file() const { return file_; }   // empty when parsed from bytes

    // ---- the TILE element
    int root() const { return root_; }
    int serialise_version() const { return int_attr(root_, "serialise_version"); }
    std::string location() const { return str_attr(root_, "location"); }          // terrain\tiles\battle\<set>\<tile> (as written, backslashes)
    std::string location_generic() const;                                          // the same with '/'
    std::string kind() const;                                                      // "battle" | "campaign" (the folder under terrain/tiles)
    std::string tile_set() const { return str_attr(root_, "tile_set"); }
    std::string mask() const { return str_attr(root_, "mask"); }
    uint32_t width() const { return uint32_t(int_attr(root_, "width")); }          // cells of 128 m
    uint32_t height() const { return uint32_t(int_attr(root_, "height")); }
    double red() const { return num_attr(root_, "red"); }                          // the tile's colour on the map tools (0..255)
    double green() const { return num_attr(root_, "green"); }
    double blue() const { return num_attr(root_, "blue"); }
    bool requires_infield_lodding() const { return bool_attr(root_, "requires_infield_lodding"); }
    bool random_rotatable() const { return bool_attr(root_, "random_rotatable"); }
    std::string custom_alpha_blend_texture() const { return str_attr(root_, "custom_alpha_blend_texture"); }
    bool scalable() const { return bool_attr(root_, "scalable"); }
    bool encampable() const { return bool_attr(root_, "encampable"); }
    std::string custom_blend_tile() const { return str_attr(root_, "custom_blend_tile"); }
    TileTextureGroup texture_group() const;
    std::vector<TileVariation> variations() const;
    std::vector<TileLinkTarget> link_targets() const;
    std::vector<TileLink> links() const;

    // ---- where things are. `raw_root` is the folder that holds terrain/tiles (an assembly kit's raw_data); raw_root() is derived from file()
    // (<raw_root>/terrain/tiles/<kind>/_tile_database/TILES/<name>.xml), empty when the entry was not loaded from a kit.
    std::filesystem::path raw_root() const;
    std::filesystem::path tile_folder(const std::filesystem::path& raw_root) const;                       // <raw_root>/<location>
    std::filesystem::path variation_folder(const std::filesystem::path& raw_root, size_t variation) const;   // .../<variation location>
    std::filesystem::path ted_path(const std::filesystem::path& raw_root, size_t variation) const { return variation_folder(raw_root, variation) / "tile.ted"; }
    // Index of the variation whose leaf folder is `name` (case-insensitive), -1 when there is none.
    int find_variation(const std::string& name) const;

    // Names of attributes and children (as "TILE/VARIATIONS/VARIATION@foo", "TILE/bar") that the typed access above does not know.
    std::vector<std::string> unknown_names() const;

    // The same normalisation everywhere: lower case, '/' separators, no trailing separator.
    static std::string normalize_location(std::string_view location);

private:
    std::string str_attr(int elem, const char* name) const { return doc_.get(elem, name).value_or(""); }
    double num_attr(int elem, const char* name) const;
    int int_attr(int elem, const char* name) const;
    bool bool_attr(int elem, const char* name) const { return doc_.get(elem, name).value_or("") == "true"; }
    int child(int elem, const char* name) const;      // first child element with this name, -1 when absent

    XmlDoc doc_;
    int root_ = -1;
    std::filesystem::path file_;
};

} // namespace wce
