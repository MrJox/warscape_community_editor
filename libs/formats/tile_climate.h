#pragma once
// The climates of the tile database (_tile_database/_settings.xml, <CLIMATES>): a climate names the ground texture set a battle map is drawn with and lists
// the texture groups it has (the names a tile's TEXTURE_GROUP red / green / blue / alpha refer to) with the size of one texture repeat in metres.
// The textures themselves are in the packs: terrain/textures/battle/<climate>/<group>/<variation>_<su|au|wi>_<diffuse|normal>.dds with four variations
// (0..3) per group; the folder `default` has every group in summer and is what a climate falls back to (docs/re/terrain.md, docs/formats/tile_database.md).
#include <cstdint>
#include <functional>
#include <string>
#include <vector>

namespace wce {

struct ClimateTexture {
    std::string name;                        // the texture group: rock, mud, grass, forest, settlement_1, ...
    double blend_pixel_scale = 40.1;         // metres per repeat of the texture inside the tile
    double outfield_blend_pixel_scale = 1200.1;
    double mid_distance_strength = 1.0;
};

struct Climate {
    std::string name;                        // default, ROME_Continental, ROME_Med, ROME_Atlantic, ROME_Desert, ...
    std::string texture_set;
    std::string destruction_climate;
    std::vector<ClimateTexture> textures;
    const ClimateTexture* find(const std::string& group) const;
    std::string folder() const;              // terrain/textures/battle/<lower case name>
};

struct ClimateList {
    std::vector<Climate> climates;
    static ClimateList parse(const std::vector<uint8_t>& settings_xml);   // throws std::runtime_error on malformed XML; no <CLIMATES> gives an empty list
    static ClimateList load(const std::string& settings_xml_path);        // empty list when the file is missing
    const Climate* find(const std::string& name) const;                   // case-insensitive
    const Climate* first() const { return climates.empty() ? nullptr : &climates.front(); }
};

// The texture path of a group's variation: `climate` folder first, then `default`; season "su" | "au" | "wi" falls back to summer. `exists` tells
// whether a path is in the packs. Empty when neither has it.
std::string climate_texture_path(const std::string& climate_folder, const std::string& group, int variation, const std::string& season, bool normal,
                                 const std::function<bool(const std::string&)>& exists);

} // namespace wce
