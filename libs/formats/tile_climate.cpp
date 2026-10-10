#include "formats/tile_climate.h"

#include "formats/xml_doc.h"
#include "foundation/process.h"

#include <algorithm>
#include <cstdlib>
#include <fstream>
#include <iterator>
#include <stdexcept>

namespace wce {

namespace {

std::string lower(std::string s) {
    for (char& c : s) c = char(std::tolower(static_cast<unsigned char>(c)));
    return s;
}

double num(const XmlDoc& d, int e, const char* a, double dflt) {
    const auto v = d.get(e, a);
    if (!v || v->empty()) return dflt;
    char* end = nullptr;
    const double x = std::strtod(v->c_str(), &end);
    return end != v->c_str() ? x : dflt;
}

} // namespace

const ClimateTexture* Climate::find(const std::string& group) const {
    for (const ClimateTexture& t : textures)
        if (t.name == group) return &t;
    return nullptr;
}

std::string Climate::folder() const { return "terrain/textures/battle/" + lower(name); }

ClimateList ClimateList::parse(const std::vector<uint8_t>& bytes) {
    const XmlDoc doc = XmlDoc::parse(bytes);
    ClimateList out;
    const auto& els = doc.elements();
    for (size_t i = 0; i < els.size(); ++i) {
        if (els[i].name != "CLIMATE" || els[i].parent < 0 || els[size_t(els[i].parent)].name != "CLIMATES") continue;
        Climate c;
        c.name = doc.get(int(i), "name").value_or("");
        c.texture_set = doc.get(int(i), "texture_set").value_or("");
        c.destruction_climate = doc.get(int(i), "destruction_climate").value_or("");
        for (int ch : els[i].children) {
            if (els[size_t(ch)].name != "TEXTURES") continue;
            for (int t : els[size_t(ch)].children) {
                if (els[size_t(t)].name != "TEXTURE") continue;
                ClimateTexture ct;
                ct.name = doc.get(t, "name").value_or("");
                ct.blend_pixel_scale = num(doc, t, "blend_pixel_scale", ct.blend_pixel_scale);
                ct.outfield_blend_pixel_scale = num(doc, t, "outfield_blend_pixel_scale", ct.outfield_blend_pixel_scale);
                ct.mid_distance_strength = num(doc, t, "mid_distance_strength", ct.mid_distance_strength);
                c.textures.push_back(std::move(ct));
            }
        }
        out.climates.push_back(std::move(c));
    }
    return out;
}

ClimateList ClimateList::load(const std::string& path) {
    std::ifstream f(path, std::ios::binary);
    if (!f) return {};
    std::vector<uint8_t> bytes((std::istreambuf_iterator<char>(f)), std::istreambuf_iterator<char>());
    return parse(bytes);
}

const Climate* ClimateList::find(const std::string& name) const {
    const std::string n = lower(name);
    for (const Climate& c : climates)
        if (lower(c.name) == n) return &c;
    return nullptr;
}

std::string climate_texture_path(const std::string& folder, const std::string& group, int variation, const std::string& season, bool normal,
                                 const std::function<bool(const std::string&)>& exists) {
    const std::string kind = normal ? "_normal.dds" : "_diffuse.dds";
    auto path = [&](const std::string& root, const std::string& s) { return root + "/" + group + "/" + std::to_string(variation) + "_" + s + kind; };
    const std::string def = "terrain/textures/battle/default";
    for (const std::string& root : {folder, def})
        for (const std::string& s : {season, std::string("su")}) {
            const std::string p = path(root, s);
            if (exists(p)) return p;
        }
    return {};
}

} // namespace wce
