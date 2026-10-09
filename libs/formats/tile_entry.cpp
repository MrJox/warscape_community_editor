#include "formats/tile_entry.h"
#include "foundation/process.h"
#include "foundation/text_encoding.h"

#include <algorithm>
#include <cctype>
#include <cstdlib>
#include <set>
#include <stdexcept>

namespace fs = std::filesystem;

namespace wce {

namespace {

const std::set<std::string> kTileAttrs = {"serialise_version", "location", "tile_set", "mask", "width", "height", "red", "green", "blue", "requires_infield_lodding",
                                          "random_rotatable", "custom_alpha_blend_texture", "scalable", "encampable", "custom_blend_tile"};
const std::set<std::string> kGroupAttrs = {"red", "green", "blue", "alpha"};
const std::set<std::string> kVariationAttrs = {"serialise_version", "location", "min_height", "scale", "normal_strength", "overlap_border_size", "raw_data_tri_density",
                                               "blend_common", "index_common", "normal_common", "red", "green", "blue", "barbarian"};
const std::set<std::string> kTargetAttrs = {"serialise_version", "tile_set", "x", "y"};
const std::set<std::string> kLinkAttrs = {"serialise_version", "link_set", "x", "y", "base_x", "base_y", "is_entry", "blend_size", "no_offline_blend", "test"};

std::string lower(std::string s) {
    for (char& c : s) c = char(std::tolower(static_cast<unsigned char>(c)));
    return s;
}

} // namespace

std::string TileEntry::normalize_location(std::string_view location) {
    std::string s(location);
    for (char& c : s) {
        if (c == '\\') c = '/';
        c = char(std::tolower(static_cast<unsigned char>(c)));
    }
    while (!s.empty() && s.back() == '/') s.pop_back();
    return s;
}

TileEntry TileEntry::parse(std::vector<uint8_t> bytes) {
    TileEntry e;
    e.doc_ = XmlDoc::parse(std::move(bytes));
    for (int r : e.doc_.roots())
        if (e.doc_.elements()[size_t(r)].name == "TILE") { e.root_ = r; break; }
    if (e.root_ < 0) throw std::runtime_error("not a tile database entry (no <TILE> element)");
    return e;
}

TileEntry TileEntry::load(const fs::path& xml) {
    TileEntry e;
    try {
        e = parse(read_file(narrow(xml.wstring())));
    } catch (const std::exception& ex) {
        throw std::runtime_error(narrow(xml.wstring()) + ": " + ex.what());
    }
    e.file_ = xml;
    return e;
}

double TileEntry::num_attr(int elem, const char* name) const {
    const auto v = doc_.get(elem, name);
    return v ? std::strtod(v->c_str(), nullptr) : 0.0;
}

int TileEntry::int_attr(int elem, const char* name) const {
    const auto v = doc_.get(elem, name);
    return v ? int(std::strtol(v->c_str(), nullptr, 10)) : 0;
}

int TileEntry::child(int elem, const char* name) const {
    for (int c : doc_.elements()[size_t(elem)].children)
        if (doc_.elements()[size_t(c)].name == name) return c;
    return -1;
}

std::string TileEntry::location_generic() const {
    std::string s = location();
    for (char& c : s)
        if (c == '\\') c = '/';
    return s;
}

std::string TileEntry::kind() const {
    // terrain/tiles/<kind>/...
    const std::string loc = normalize_location(location());
    const std::string prefix = "terrain/tiles/";
    if (loc.compare(0, prefix.size(), prefix) != 0) return {};
    const size_t slash = loc.find('/', prefix.size());
    return loc.substr(prefix.size(), slash == std::string::npos ? std::string::npos : slash - prefix.size());
}

TileTextureGroup TileEntry::texture_group() const {
    TileTextureGroup g;
    const int c = child(root_, "TEXTURE_GROUP");
    if (c < 0) return g;
    g.red = str_attr(c, "red");
    g.green = str_attr(c, "green");
    g.blue = str_attr(c, "blue");
    g.alpha = str_attr(c, "alpha");
    return g;
}

std::vector<TileVariation> TileEntry::variations() const {
    std::vector<TileVariation> out;
    const int list = child(root_, "VARIATIONS");
    if (list < 0) return out;
    for (int c : doc_.elements()[size_t(list)].children) {
        if (doc_.elements()[size_t(c)].name != "VARIATION") continue;
        TileVariation v;
        v.elem = c;
        v.serialise_version = int_attr(c, "serialise_version");
        v.location = str_attr(c, "location");
        v.min_height = num_attr(c, "min_height");
        v.scale = doc_.get(c, "scale") ? num_attr(c, "scale") : 100.0;
        v.normal_strength = num_attr(c, "normal_strength");
        v.overlap_border_size = num_attr(c, "overlap_border_size");
        v.raw_data_tri_density = uint32_t(int_attr(c, "raw_data_tri_density"));
        v.blend_common = str_attr(c, "blend_common");
        v.index_common = str_attr(c, "index_common");
        v.normal_common = str_attr(c, "normal_common");
        v.red = num_attr(c, "red");
        v.green = num_attr(c, "green");
        v.blue = num_attr(c, "blue");
        v.barbarian = bool_attr(c, "barbarian");
        out.push_back(std::move(v));
    }
    return out;
}

std::vector<TileLinkTarget> TileEntry::link_targets() const {
    std::vector<TileLinkTarget> out;
    const int list = child(root_, "TILE_LINK_TARGETS");
    if (list < 0) return out;
    for (int c : doc_.elements()[size_t(list)].children) {
        if (doc_.elements()[size_t(c)].name != "TILE_LINK_TARGET") continue;
        TileLinkTarget t;
        t.elem = c;
        t.serialise_version = int_attr(c, "serialise_version");
        t.tile_set = str_attr(c, "tile_set");
        t.x = int_attr(c, "x");
        t.y = int_attr(c, "y");
        out.push_back(std::move(t));
    }
    return out;
}

std::vector<TileLink> TileEntry::links() const {
    std::vector<TileLink> out;
    const int list = child(root_, "TILE_LINKS");
    if (list < 0) return out;
    for (int c : doc_.elements()[size_t(list)].children) {
        if (doc_.elements()[size_t(c)].name != "TILE_LINK") continue;
        TileLink l;
        l.elem = c;
        l.serialise_version = int_attr(c, "serialise_version");
        l.link_set = str_attr(c, "link_set");
        l.x = int_attr(c, "x");
        l.y = int_attr(c, "y");
        l.base_x = int_attr(c, "base_x");
        l.base_y = int_attr(c, "base_y");
        l.is_entry = bool_attr(c, "is_entry");
        l.blend_size = uint32_t(int_attr(c, "blend_size"));
        l.no_offline_blend = bool_attr(c, "no_offline_blend");
        l.test = str_attr(c, "test");
        l.has_blend_quad = this->child(c, "blend_quad") >= 0;
        out.push_back(std::move(l));
    }
    return out;
}

fs::path TileEntry::raw_root() const {
    // <raw_root>/terrain/tiles/<kind>/_tile_database/TILES/<name>.xml
    if (file_.empty()) return {};
    const fs::path tiles_dir = file_.parent_path(), db = tiles_dir.parent_path(), kind = db.parent_path(), tiles = kind.parent_path(), terrain = tiles.parent_path();
    if (lower(tiles_dir.filename().string()) != "tiles" || lower(db.filename().string()) != "_tile_database" || lower(tiles.filename().string()) != "tiles" ||
        lower(terrain.filename().string()) != "terrain")
        return {};
    return terrain.parent_path();
}

static fs::path append_location(fs::path p, const std::string& loc) {   // a path with either separator -> p/a/b/c
    size_t at = 0;
    while (at <= loc.size()) {
        size_t e = loc.find_first_of("\\/", at);
        if (e == std::string::npos) e = loc.size();
        if (e > at) p /= widen(loc.substr(at, e - at));
        at = e + 1;
    }
    return p;
}

fs::path TileEntry::tile_folder(const fs::path& raw_root) const { return append_location(raw_root, location()); }

fs::path TileEntry::variation_folder(const fs::path& raw_root, size_t variation) const {
    const auto v = variations();
    fs::path p = tile_folder(raw_root);
    return variation < v.size() ? append_location(p, v[variation].location) : p;
}

int TileEntry::find_variation(const std::string& name) const {
    const std::string want = normalize_location(name);
    const auto v = variations();
    for (size_t i = 0; i < v.size(); ++i)
        if (normalize_location(v[i].location) == want) return int(i);
    return -1;
}

std::vector<std::string> TileEntry::unknown_names() const {
    std::vector<std::string> out;
    auto attrs = [&](int e, const std::set<std::string>& known, const std::string& path) {
        for (const XmlAttr& a : doc_.elements()[size_t(e)].attrs)
            if (!known.count(a.name)) out.push_back(path + "@" + a.name);
    };
    attrs(root_, kTileAttrs, "TILE");
    for (int c : doc_.elements()[size_t(root_)].children) {
        const std::string& n = doc_.elements()[size_t(c)].name;
        if (n == "TEXTURE_GROUP") {
            attrs(c, kGroupAttrs, "TILE/TEXTURE_GROUP");
            for (int g : doc_.elements()[size_t(c)].children) out.push_back("TILE/TEXTURE_GROUP/" + doc_.elements()[size_t(g)].name);
        } else if (n == "VARIATIONS" || n == "TILE_LINK_TARGETS" || n == "TILE_LINKS") {
            attrs(c, {}, "TILE/" + n);
            const char* item = n == "VARIATIONS" ? "VARIATION" : n == "TILE_LINK_TARGETS" ? "TILE_LINK_TARGET" : "TILE_LINK";
            const std::set<std::string>& known = n == "VARIATIONS" ? kVariationAttrs : n == "TILE_LINK_TARGETS" ? kTargetAttrs : kLinkAttrs;
            for (int i : doc_.elements()[size_t(c)].children) {
                if (doc_.elements()[size_t(i)].name != item) { out.push_back("TILE/" + n + "/" + doc_.elements()[size_t(i)].name); continue; }
                attrs(i, known, "TILE/" + n + "/" + item);
                for (int k : doc_.elements()[size_t(i)].children)
                    if (!(n == "TILE_LINKS" && doc_.elements()[size_t(k)].name == "blend_quad" && doc_.elements()[size_t(k)].children.empty()))
                        out.push_back("TILE/" + n + "/" + item + "/" + doc_.elements()[size_t(k)].name);
            }
        } else {
            out.push_back("TILE/" + n);
        }
    }
    return out;
}

} // namespace wce
