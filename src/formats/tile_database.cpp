#include "formats/tile_database.h"
#include "formats/registry.h"
#include "formats/xml_doc.h"

#include <algorithm>
#include <cstdlib>
#include <cstring>
#include <map>
#include <stdexcept>

namespace te {
namespace {

struct Out {
    std::vector<uint8_t> b;
    void raw(const void* d, size_t n) { const uint8_t* s = static_cast<const uint8_t*>(d); b.insert(b.end(), s, s + n); }
    void u16(uint32_t v) { uint16_t x = static_cast<uint16_t>(v); raw(&x, 2); }
    void u32(uint32_t v) { raw(&v, 4); }
    void i32(int32_t v) { raw(&v, 4); }
    void f32(float v) { raw(&v, 4); }
    void u8(bool v) { b.push_back(v ? 1 : 0); }
    void str(const std::string& s) { u16(static_cast<uint32_t>(s.size())); raw(s.data(), s.size()); }
};

// Access to the attributes and children of one XmlDoc element.
struct El {
    const we::XmlDoc& d;
    int i;
    std::string attr(const char* name) const {
        auto v = d.get(i, name);
        if (!v) throw std::runtime_error(std::string("missing attribute '") + name + "' on <" + d.elements()[i].name + ">");
        return *v;
    }
    bool has(const char* name) const { return d.get(i, name).has_value(); }
    float f(const char* name) const { return std::strtof(attr(name).c_str(), nullptr); }
    uint32_t u(const char* name) const { return static_cast<uint32_t>(std::strtoul(attr(name).c_str(), nullptr, 10)); }
    int32_t s(const char* name) const { return static_cast<int32_t>(std::strtol(attr(name).c_str(), nullptr, 10)); }
    bool b(const char* name) const { return attr(name) == "true"; }
    std::vector<El> kids() const { std::vector<El> v; for (int c : d.elements()[i].children) v.push_back({d, c}); return v; }
    El child(const char* name) const {
        for (int c : d.elements()[i].children) if (d.elements()[c].name == name) return {d, c};
        throw std::runtime_error(std::string("missing <") + name + ">");
    }
};

const char* kRenderSlots[25] = {
    "lf_height", "hf_height", "blend_pixel_scale", "unit_scale", "mid_distance_detail_scale",
    "mid_distance_detail_strength", "mid_distance_normal_strength", "mid_distance_detail_near", "mid_distance_detail_far",
    "mid_distance_detail_slope_low", "mid_distance_detail_slope_high", "vertical_offset", "normal_lf_scale",
    "normal_tile_scale", "normal_terrain_scale", "blend_contrast", "layer_exempt_0", "layer_exempt_1",
    "campaign_sea_transparency_scale", "campaign_sea_uv_scale", "campaign_lake_transparency_scale",
    "near_distance_detail_distance", "near_distance_detail_scale", "near_distance_detail_strength",
    "mid_distance_detail_near_lq"};

void write_tile(Out& o, const El& t) {
    o.u16(t.u("serialise_version"));
    o.str(t.attr("location"));
    o.str(t.attr("tile_set"));
    o.str(t.attr("mask"));
    o.u32(t.u("width"));
    o.u32(t.u("height"));
    o.f32(t.f("red")); o.f32(t.f("green")); o.f32(t.f("blue"));
    o.u8(t.b("requires_infield_lodding"));
    o.u8(t.b("random_rotatable"));
    o.str(t.attr("custom_alpha_blend_texture"));
    o.u8(t.b("scalable"));
    o.u8(t.b("encampable"));
    o.str(t.attr("custom_blend_tile"));
    El tg = t.child("TEXTURE_GROUP");
    for (const char* k : {"red", "green", "blue", "alpha"}) o.str(tg.attr(k));
    auto vars = t.child("VARIATIONS").kids();
    o.u32(static_cast<uint32_t>(vars.size()));
    for (const El& v : vars) {
        o.u16(v.u("serialise_version"));
        o.str(v.attr("location"));
        o.f32(v.f("min_height")); o.f32(v.f("scale")); o.f32(v.f("normal_strength")); o.f32(v.f("overlap_border_size"));
        o.u32(v.u("raw_data_tri_density"));
        o.str(v.attr("blend_common")); o.str(v.attr("index_common")); o.str(v.attr("normal_common"));
        o.f32(v.f("red")); o.f32(v.f("green")); o.f32(v.f("blue"));
        o.u8(v.b("barbarian"));
    }
    auto targets = t.child("TILE_LINK_TARGETS").kids();
    o.u32(static_cast<uint32_t>(targets.size()));
    for (const El& x : targets) { o.u16(x.u("serialise_version")); o.str(x.attr("tile_set")); o.i32(x.s("x")); o.i32(x.s("y")); }
    auto links = t.child("TILE_LINKS").kids();
    o.u32(static_cast<uint32_t>(links.size()));
    for (const El& l : links) {
        o.u16(l.u("serialise_version"));
        o.str(l.attr("link_set"));
        o.i32(l.s("x")); o.i32(l.s("y")); o.i32(l.s("base_x")); o.i32(l.s("base_y"));
        o.u8(l.b("is_entry"));
        o.u32(0); // blend_quad (empty in all shipped data)
        o.u32(l.u("blend_size"));
        o.u8(l.b("no_offline_blend"));
        o.str(l.attr("test"));
    }
}

} // namespace

std::vector<uint8_t> build_tile_database(const TileDbInput& in) {
    we::XmlDoc settings = we::XmlDoc::parse(in.settings_xml);
    El root{settings, settings.roots().at(0)};
    Out o;
    o.raw("FASTBIN0", 8);
    o.u16(1);
    El rp = root.child("RENDER_PARAMS");
    o.u16(6); // BOB always writes the current RENDER_PARAMS version (the campaign XML still says 3)
    // 16 floats, the four layer_exempt strings, then 7 more floats (kRenderSlots lists them in file order; the two
    // placeholders after blend_contrast stand for the string block)
    for (int k = 0; k < 16; ++k) o.f32(rp.f(kRenderSlots[k]));
    for (int k = 0; k < 4; ++k) o.str(rp.attr(("layer_exempt_" + std::to_string(k)).c_str()));
    for (int k = 18; k < 25; ++k) o.f32(rp.f(kRenderSlots[k]));
    El cp = root.child("CONVERSION_PARAMS");
    o.u16(cp.u("serialise_version"));
    o.u32(cp.u("triangle_density"));
    o.u32(cp.u("max_lf_heights_per_pixel"));
    for (int k = 0; k < 6; ++k) o.f32(cp.f(("triangle_decimation_size_factors" + std::to_string(k)).c_str()));
    for (int k = 0; k < 6; ++k) o.f32(cp.f(("triangle_decimation_angle_factors" + std::to_string(k)).c_str()));
    auto climates = root.child("CLIMATES").kids();
    o.u32(static_cast<uint32_t>(climates.size()));
    for (const El& c : climates) {
        o.u16(c.u("serialise_version"));
        o.str(c.attr("name")); o.str(c.attr("texture_set"));
        o.f32(c.f("red")); o.f32(c.f("green")); o.f32(c.f("blue"));
        auto tex = c.child("TEXTURES").kids();
        o.u32(static_cast<uint32_t>(tex.size()));
        for (const El& t : tex) {
            o.u16(t.u("serialise_version"));
            o.str(t.attr("name"));
            o.f32(t.f("mid_distance_strength"));
            for (int k = 0; k < 4; ++k) {
                o.f32(t.f(("lerp_reflectivity" + std::to_string(k)).c_str()));
                o.f32(t.f(("lerp_smoothness" + std::to_string(k)).c_str()));
            }
            o.f32(t.f("blend_pixel_scale"));
            o.f32(t.f("outfield_blend_pixel_scale"));
        }
        o.f32(c.f("grass_alpha_add")); o.f32(c.f("grass_alpha_mul"));
        o.str(c.attr("destruction_climate"));
    }
    auto sets = root.child("TILE_SETS").kids();
    // tiles are parsed first: sets they reference but _settings.xml does not define are appended to the set list
    std::vector<we::XmlDoc> docs;
    docs.reserve(in.tile_xml.size());
    for (const auto& x : in.tile_xml) docs.push_back(we::XmlDoc::parse(x));
    std::vector<std::string> extra;
    {
        std::map<std::string, bool> known;
        for (const El& t : sets) known[t.attr("name")] = true;
        for (const auto& d : docs) {
            std::string ts = El{d, d.roots().at(0)}.attr("tile_set");
            if (!known[ts]) { known[ts] = true; extra.push_back(ts); }
        }
        if (in.add_user_tile_set && !known["user"]) extra.push_back("user");
    }
    o.u32(static_cast<uint32_t>(sets.size() + extra.size()));
    for (const El& t : sets) {
        o.u16(t.u("serialise_version"));
        o.str(t.attr("name"));
        for (const char* k : {"linking_tile", "shared_geometry", "also_place_tile_set", "link_as_set"}) o.str(t.attr(k));
        o.f32(t.f("red")); o.f32(t.f("green")); o.f32(t.f("blue"));
    }
    for (const std::string& name : extra) {
        o.u16(1); o.str(name);
        for (int k = 0; k < 4; ++k) o.str("");
        o.f32(0); o.f32(0); o.f32(0);
    }
    // tiles
    struct Item { size_t doc; std::string loc; long area; };
    std::vector<Item> items;
    for (size_t k = 0; k < docs.size(); ++k) {
        El e{docs[k], docs[k].roots().at(0)};
        items.push_back({k, e.attr("location"), long(e.u("width")) * long(e.u("height"))});
    }
    if (!in.order.empty()) {
        std::map<std::string, size_t> rank;
        for (size_t k = 0; k < in.order.size(); ++k) rank[in.order[k]] = k;
        for (const Item& it : items) if (!rank.count(it.loc)) throw std::runtime_error("tile missing from the order list: " + it.loc);
        std::stable_sort(items.begin(), items.end(), [&](const Item& a, const Item& b) { return rank[a.loc] < rank[b.loc]; });
    } else {
        std::stable_sort(items.begin(), items.end(), [](const Item& a, const Item& b) { return a.area != b.area ? a.area > b.area : a.loc < b.loc; });
    }
    o.u32(static_cast<uint32_t>(items.size()));
    for (const Item& it : items) write_tile(o, El{docs[it.doc], docs[it.doc].roots().at(0)});
    return o.b;
}

// --- reading the shipped binary far enough to list the tile locations
namespace {
struct In {
    const std::vector<uint8_t>& b;
    size_t p = 0;
    template <class T> T get() { if (p + sizeof(T) > b.size()) throw std::runtime_error("truncated"); T v; std::memcpy(&v, &b[p], sizeof(T)); p += sizeof(T); return v; }
    std::string str() { uint16_t n = get<uint16_t>(); if (p + n > b.size()) throw std::runtime_error("truncated"); std::string s(reinterpret_cast<const char*>(&b[p]), n); p += n; return s; }
    void skip(size_t n) { if (p + n > b.size()) throw std::runtime_error("truncated"); p += n; }
};
} // namespace

std::vector<std::string> tile_database_order(const std::vector<uint8_t>& bin) {
    In r{bin};
    r.skip(8); r.get<uint16_t>();
    r.get<uint16_t>(); r.skip(16 * 4); for (int k = 0; k < 4; ++k) r.str(); r.skip(7 * 4);
    r.get<uint16_t>(); r.skip(8 + 12 * 4);
    for (uint32_t n = r.get<uint32_t>(); n; --n) {          // climates
        r.get<uint16_t>(); r.str(); r.str(); r.skip(12);
        for (uint32_t k = r.get<uint32_t>(); k; --k) { r.get<uint16_t>(); r.str(); r.skip(11 * 4); }
        r.skip(8); r.str();
    }
    for (uint32_t n = r.get<uint32_t>(); n; --n) { r.get<uint16_t>(); r.str(); for (int k = 0; k < 4; ++k) r.str(); r.skip(12); }
    std::vector<std::string> order;
    for (uint32_t n = r.get<uint32_t>(); n; --n) {
        r.get<uint16_t>();
        order.push_back(r.str());
        r.str(); r.str();                                   // tile_set, mask
        r.skip(8 + 12 + 2); r.str(); r.skip(2); r.str();    // w h rgb flags alpha_texture scalable encampable custom_blend_tile
        for (int k = 0; k < 4; ++k) r.str();
        for (uint32_t v = r.get<uint32_t>(); v; --v) { r.get<uint16_t>(); r.str(); r.skip(20); r.str(); r.str(); r.str(); r.skip(13); }
        for (uint32_t v = r.get<uint32_t>(); v; --v) { r.get<uint16_t>(); r.str(); r.skip(8); }
        for (uint32_t v = r.get<uint32_t>(); v; --v) { r.get<uint16_t>(); r.str(); r.skip(16 + 1 + 4 + 4 + 1); r.str(); }
    }
    return order;
}

} // namespace te
