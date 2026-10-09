#include "weather_vfx.h"

#include "vfs/vfs.h"
#include "formats/xml_doc.h"

#include <cstdlib>

namespace wce {

namespace {

float num(const std::optional<std::string>& s, float def) { return s ? float(std::atof(s->c_str())) : def; }

std::optional<WeatherDefinition> find_definition(const VfxReader& read, const std::string& name) {
    XmlDoc doc = XmlDoc::parse(read("weather/weather_definitions.xml"));
    const auto& els = doc.elements();
    for (int i = 0; i < int(els.size()); ++i) {
        if (els[i].name != "WEATHER_DEFINITION" || doc.get(i, "name").value_or("") != name) continue;
        WeatherDefinition d;
        d.name = name;
        d.type = doc.get(i, "type").value_or("");
        d.vfx1 = doc.get(i, "vfx1").value_or("");
        d.vfx2 = doc.get(i, "vfx2").value_or("");
        d.severity = int(num(doc.get(i, "severity"), 0));
        d.wind_size_min = num(doc.get(i, "wind_size_min"), d.wind_size_min);
        d.wind_size_max = num(doc.get(i, "wind_size_max"), d.wind_size_max);
        d.wind_gust_size_variation = num(doc.get(i, "wind_gust_size_variation"), d.wind_gust_size_variation);
        d.wind_gust_duration = num(doc.get(i, "wind_gust_duration"), d.wind_gust_duration);
        d.wind_gust_duration_variation = num(doc.get(i, "wind_gust_duration_variation"), d.wind_gust_duration_variation);
        d.wind_gust_interval = num(doc.get(i, "wind_gust_interval"), d.wind_gust_interval);
        d.wind_gust_probability = num(doc.get(i, "wind_gust_probability"), d.wind_gust_probability);
        d.wind_gust_curve = num(doc.get(i, "wind_gust_curve"), d.wind_gust_curve);
        d.wind_vegetation_multiplier = num(doc.get(i, "wind_vegetation_multiplier"), d.wind_vegetation_multiplier);
        return d;
    }
    return std::nullopt;
}

VfxFile load_vfx(const VfxReader& read, const std::string& name) { return parse_vfx(XmlDoc::parse(read("vfx/" + name + ".xml"))); }

// An emitter is drawn when it is enabled and some render stack pairs it with a chain that has a texture.
const VfxChain* render_chain(const VfxFile& lib, const std::string& emitter, const VfxStack** stack) {
    for (auto& s : lib.stacks)
        for (auto& e : s.emitters)
            if (e == emitter)
                for (auto& c : s.chains)
                    if (const VfxChain* ch = lib.chain(c); ch && ch->find("ps_texture")) {
                        *stack = &s;
                        return ch;
                    }
    return nullptr;
}

} // namespace

WeatherVfx load_weather_vfx(Vfs& vfs, const std::string& definition_name) {
    return load_weather_vfx([&vfs](const std::string& p) { return vfs.read(p); }, definition_name);
}

WeatherVfx load_weather_vfx(const VfxReader& vfs, const std::string& definition_name) {
    WeatherVfx out;
    // An empty name looks up "none" (no effect, no wind), like the engine's default definition list entry.
    auto def = find_definition(vfs, definition_name.empty() ? std::string("none") : definition_name);
    if (!def) {
        out.notes.push_back("weather definition '" + definition_name + "' not in weather/weather_definitions.xml");
        return out;
    }
    out.definition = *def;
    std::map<std::string, VfxFile> libs;
    for (const std::string& top_name : {def->vfx1, def->vfx2}) { // the engine plays both (loader fn 0x1738a60: "vfx 1", "vfx 2")
        if (top_name.empty()) continue;
        VfxFile top;
        try {
            top = load_vfx(vfs, top_name);
        } catch (const std::exception& e) {
            out.notes.push_back("vfx/" + top_name + ".xml: " + e.what());
            continue;
        }
        for (const VfxRef& ref : top.refs) {
            auto it = libs.find(ref.library);
            if (it == libs.end()) {
                try {
                    it = libs.emplace(ref.library, load_vfx(vfs, ref.library)).first;
                } catch (const std::exception& e) {
                    out.notes.push_back("vfx/" + ref.library + ".xml: " + e.what());
                    continue;
                }
            }
            const VfxFile& lib = it->second;
            for (const VfxEmitterDef& em : lib.emitters) {
                const VfxRefEmitter* ov = ref.find(em.id);
                if ((ov && !ov->enable) || !em.b("Enable", true)) continue;
                if (em.type != "continuous_mesh") {
                    out.notes.push_back(ref.inst_name + "/" + em.id + ": emitter type " + em.type + " not supported");
                    continue;
                }
                const VfxStack* stack = nullptr;
                const VfxChain* chain = render_chain(lib, em.id, &stack);
                if (!chain) continue; // sound / logic-only emitters draw nothing
                if (stack->scene_view_type > 1) {
                    out.notes.push_back(ref.inst_name + "/" + em.id + ": scene_view_type " + std::to_string(stack->scene_view_type) +
                                        " not supported");
                    continue;
                }
                ActiveEmitter a;
                a.library = ref.library;
                a.instance = ref.inst_name;
                a.name = em.id;
                a.def = em;
                a.chain = *chain;
                a.k = build_emitter_constants(em, chain);
                if (stack->scene_view_type == 1) a.k.behaviour_mask[3] = kVfxDistortionViewId; // dump: heat haze rows have w = 12
                if (ov) {
                    for (int c = 0; c < 4; ++c) a.tint_min[c] = ov->tint_min[c], a.tint_max[c] = ov->tint_max[c];
                    a.particle_lifetime_bias[0] = ov->particle_lifetime_bias[0];
                    a.particle_lifetime_bias[1] = ov->particle_lifetime_bias[1];
                }
                a.scale = 0.5f * (ref.scale_min + ref.scale_max);
                for (int c = 0; c < 3; ++c) a.pos[c] = ref.pos[c];
                a.camera_following = lib.camera_following() || top.camera_following();
                out.emitters.push_back(std::move(a));
            }
        }
    }
    return out;
}

} // namespace wce
