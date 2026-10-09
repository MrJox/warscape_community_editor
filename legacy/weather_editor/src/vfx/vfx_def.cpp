#include "vfx_def.h"

#include "formats/xml_doc.h"

#include <algorithm>
#include <cstdlib>
#include <sstream>

namespace wce {

namespace {

float num(const std::optional<std::string>& s, float def) { return s ? float(std::atof(s->c_str())) : def; }

bool truthy(const std::string& s) { return s == "true" || s == "1"; }

// Text between an element's start and end tag.
std::string inner_text(const XmlDoc& doc, int e) {
    const XmlElement& el = doc.elements()[e];
    if (el.self_closing || !el.children.empty()) return {};
    const std::string& t = doc.original_text();
    size_t end = t.rfind("</", el.end_off);
    if (end == std::string::npos || end < el.start_end) return {};
    return XmlDoc::decode_entities(std::string_view(t).substr(el.start_end, end - el.start_end));
}

void parse_float4(const std::string& s, float out[4]) { // "float4(1.0,0.9,0.8,1.0)"
    size_t p = s.find('(');
    std::string body = p == std::string::npos ? s : s.substr(p + 1);
    std::istringstream is(body);
    for (int i = 0; i < 4; ++i) {
        std::string tok;
        if (!std::getline(is, tok, ',')) break;
        out[i] = float(std::atof(tok.c_str()));
    }
}

VfxProp read_prop(const XmlDoc& doc, int e) {
    VfxProp p;
    if (auto v = doc.get(e, "value")) p.value = *v;
    if (auto v = doc.get(e, "path")) p.value = *v;
    const char* comps[4] = {"x", "y", "z", "w"};
    for (int i = 0; i < 4; ++i)
        if (auto v = doc.get(e, comps[i])) {
            p.v[i] = float(std::atof(v->c_str()));
            p.components = i + 1;
        }
    if (auto lo = doc.get(e, "min")) {
        p.ranged = true;
        p.min_text = *lo;
        p.min = float(std::atof(lo->c_str()));
    }
    if (auto hi = doc.get(e, "max")) {
        p.ranged = true;
        p.max_text = *hi;
        p.max = float(std::atof(hi->c_str()));
    }
    std::string text = inner_text(doc, e);
    if (!text.empty()) p.curves = parse_curves(text);
    return p;
}

void read_props(const XmlDoc& doc, int e, VfxProps& out) {
    for (int c : doc.elements()[e].children) {
        const std::string& n = doc.elements()[c].name;
        if (n != "property" && n != "ranged_property" && n != "texture_property") continue;
        auto name = doc.get(c, "name");
        if (!name) continue;
        // Emitter properties repeat per graphics quality: quality="0" is the base value, higher levels are
        // lower-quality overrides (fewer particles, shorter culling). Decision: the editor shows the base level, which is what
        // gfx_effects_quality 3 (the maximum, the default) selects; the mapping of lower settings to levels 1 / 2 is not decoded.
        int quality = std::atoi(doc.get(c, "quality").value_or("0").c_str());
        if (quality != 0 && out.props.count(*name)) continue;
        if (quality != 0) {
            out.props[*name] = read_prop(doc, c); // only a quality override exists: keep it until a base one appears
            continue;
        }
        out.props[*name] = read_prop(doc, c);
    }
}

int child(const XmlDoc& doc, int e, const char* name) {
    for (int c : doc.elements()[e].children)
        if (doc.elements()[c].name == name) return c;
    return -1;
}

} // namespace

float VfxCurve::eval(float t) const {
    if (keys.empty()) return 0;
    if (t <= keys.front().first) return keys.front().second;
    for (size_t i = 1; i < keys.size(); ++i)
        if (t <= keys[i].first) {
            float t0 = keys[i - 1].first, t1 = keys[i].first;
            float a = t1 > t0 ? (t - t0) / (t1 - t0) : 1.0f;
            return keys[i - 1].second + (keys[i].second - keys[i - 1].second) * a;
        }
    return keys.back().second;
}

std::map<std::string, VfxCurve> parse_curves(const std::string& text) {
    std::map<std::string, VfxCurve> out;
    std::istringstream curves(text);
    std::string curve;
    while (std::getline(curves, curve, '|')) {
        std::istringstream parts(curve);
        std::string head, key;
        if (!std::getline(parts, head, ';')) continue;
        std::string name = head.substr(0, head.find(','));
        VfxCurve c;
        while (std::getline(parts, key, ';')) {
            size_t comma = key.find(',');
            if (comma == std::string::npos) continue;
            c.keys.push_back({float(std::atof(key.substr(0, comma).c_str())), float(std::atof(key.substr(comma + 1).c_str()))});
        }
        std::sort(c.keys.begin(), c.keys.end(), [](auto& a, auto& b) { return a.first < b.first; });
        out[name] = std::move(c);
    }
    return out;
}

const VfxProp* VfxProps::find(const std::string& name) const {
    auto it = props.find(name);
    return it == props.end() ? nullptr : &it->second;
}
bool VfxProps::b(const std::string& name, bool def) const {
    auto* p = find(name);
    return p && !p->value.empty() ? truthy(p->value) : def;
}
float VfxProps::f(const std::string& name, float def) const {
    auto* p = find(name);
    return p && !p->value.empty() ? float(std::atof(p->value.c_str())) : def;
}
int VfxProps::i(const std::string& name, int def) const {
    auto* p = find(name);
    if (!p || p->value.empty()) return def;
    if (p->value == "true") return 1;
    if (p->value == "false") return 0;
    return std::atoi(p->value.c_str());
}
void VfxProps::range(const std::string& name, float& lo, float& hi, float def_lo, float def_hi) const {
    auto* p = find(name);
    lo = p && p->ranged ? p->min : def_lo;
    hi = p && p->ranged ? p->max : def_hi;
}
void VfxProps::vec(const std::string& name, float* out, int n) const {
    if (auto* p = find(name))
        for (int k = 0; k < n && k < p->components; ++k) out[k] = p->v[k];
}
void VfxProps::colour(const std::string& name, float out_min[4], float out_max[4]) const {
    auto* p = find(name);
    if (!p || !p->ranged) return;
    parse_float4(p->min_text, out_min);
    parse_float4(p->max_text, out_max);
}
void VfxProps::float_n(const std::string& name, float* out, int n) const {
    auto* p = find(name);
    if (!p || p->value.empty()) return;
    float v[4] = {out[0], n > 1 ? out[1] : 0, n > 2 ? out[2] : 0, n > 3 ? out[3] : 0};
    parse_float4(p->value, v);
    for (int k = 0; k < n && k < 4; ++k) out[k] = v[k];
}
const VfxCurve* VfxProps::curve(const std::string& name, const std::string& sub) const {
    auto* p = find(name);
    if (!p) return nullptr;
    auto it = p->curves.find(sub);
    return it == p->curves.end() ? nullptr : &it->second;
}
std::string VfxProps::text(const std::string& name) const {
    auto* p = find(name);
    return p ? p->value : std::string();
}

const VfxModifier* VfxChain::find(const std::string& fragment) const {
    for (auto& m : modifiers)
        if (m.fragment == fragment) return &m;
    return nullptr;
}

const VfxRefEmitter* VfxRef::find(const std::string& emitter) const {
    for (auto& e : emitters)
        if (e.name == emitter) return &e;
    return nullptr;
}

bool VfxFile::camera_following() const {
    auto it = vfx_attrs.find("camera_following");
    return it != vfx_attrs.end() && truthy(it->second);
}
const VfxEmitterDef* VfxFile::emitter(const std::string& id) const {
    for (auto& e : emitters)
        if (e.id == id) return &e;
    return nullptr;
}
const VfxChain* VfxFile::chain(const std::string& id) const {
    for (auto& c : chains)
        if (c.id == id) return &c;
    return nullptr;
}

VfxFile parse_vfx(const XmlDoc& doc) {
    VfxFile f;
    const auto& els = doc.elements();
    for (int r : doc.roots()) {
        if (els[r].name == "vfx") {
            for (size_t a = 0; a < els[r].attrs.size(); ++a)
                f.vfx_attrs[els[r].attrs[a].name] = XmlDoc::decode_entities(doc.attr_raw(r, a));
            if (int em = child(doc, r, "emitters"); em >= 0)
                for (int e : els[em].children) {
                    if (els[e].name != "emitter") continue;
                    VfxEmitterDef d;
                    d.id = doc.get(e, "id").value_or("");
                    d.type = doc.get(e, "type").value_or("");
                    read_props(doc, e, d);
                    f.emitters.push_back(std::move(d));
                }
            if (int bc = child(doc, r, "behaviour_chains"); bc >= 0)
                for (int c : els[bc].children) {
                    if (els[c].name != "behaviour_chain") continue;
                    VfxChain ch;
                    ch.id = doc.get(c, "id").value_or("");
                    for (int m : els[c].children) {
                        if (els[m].name != "modifier") continue;
                        VfxModifier mod;
                        mod.fragment = doc.get(m, "fragment").value_or("");
                        read_props(doc, m, mod);
                        ch.modifiers.push_back(std::move(mod));
                    }
                    f.chains.push_back(std::move(ch));
                }
            if (int rs = child(doc, r, "effect_render_stacks"); rs >= 0)
                for (int s : els[rs].children) {
                    if (els[s].name != "effect_render_stack") continue;
                    VfxStack st;
                    st.id = doc.get(s, "id").value_or("");
                    if (int es = child(doc, s, "emitters_selected"); es >= 0)
                        for (int e : els[es].children) st.emitters.push_back(doc.get(e, "id").value_or(""));
                    if (int cs = child(doc, s, "behaviour_chains_selected"); cs >= 0)
                        for (int c : els[cs].children) st.chains.push_back(doc.get(c, "id").value_or(""));
                    if (int rp = child(doc, s, "render_params"); rp >= 0) {
                        st.scene_view_type = int(num(doc.get(rp, "scene_view_type"), 0));
                        st.cast_shadows = truthy(doc.get(rp, "cast_shadows").value_or("false"));
                    }
                    f.stacks.push_back(std::move(st));
                }
        } else if (els[r].name == "vfx_references") {
            for (int v : els[r].children) {
                if (els[v].name != "vfx") continue;
                VfxRef ref;
                ref.inst_name = doc.get(v, "inst_name").value_or("");
                ref.library = doc.get(v, "vfx_ref").value_or("");
                ref.scale_min = num(doc.get(v, "scale_min"), 1);
                ref.scale_max = num(doc.get(v, "scale_max"), 1);
                ref.pos[0] = num(doc.get(v, "pos_x"), 0);
                ref.pos[1] = num(doc.get(v, "pos_y"), 0);
                ref.pos[2] = num(doc.get(v, "pos_z"), 0);
                ref.start_time = num(doc.get(v, "start_time"), 0);
                ref.infinite_life = truthy(doc.get(v, "infinite_life").value_or("true"));
                for (int e : els[v].children) {
                    if (els[e].name != "emitter") continue;
                    VfxRefEmitter re;
                    re.name = doc.get(e, "name").value_or("");
                    re.enable = truthy(doc.get(e, "enable").value_or("true"));
                    const char* ch = "rgba";
                    for (int k = 0; k < 4; ++k) {
                        re.tint_min[k] = num(doc.get(e, std::string("tint_min_") + ch[k]), 1);
                        re.tint_max[k] = num(doc.get(e, std::string("tint_max_") + ch[k]), 1);
                    }
                    re.particle_lifetime_bias[0] = num(doc.get(e, "particle_lifetime_bias_min"), 0);
                    re.particle_lifetime_bias[1] = num(doc.get(e, "particle_lifetime_bias_max"), 0);
                    re.emitter_lifetime_bias[0] = num(doc.get(e, "emitter_lifetime_bias_min"), 0);
                    re.emitter_lifetime_bias[1] = num(doc.get(e, "emitter_lifetime_bias_max"), 0);
                    ref.emitters.push_back(re);
                }
                f.refs.push_back(std::move(ref));
            }
        }
    }
    return f;
}

} // namespace wce
