#pragma once
// VFX XML files (vfx/*.xml, UTF-16): emitters, behaviour chains, effect render stacks and vfx_references.
//
// A weather effect file (weather_definitions.xml vfx1, e.g. weather_vfx_rain_heavy_daytime) is usually an empty
// <vfx> plus <vfx_references>: each reference instantiates a library effect (library_vfx_weather_rain) and
// overrides per emitter enable, tint and lifetime biases. See docs/re/particles.md.
#include <map>
#include <optional>
#include <string>
#include <vector>

namespace wce {

class XmlDoc;

// "alpha_min,16711680;0.0,0.000212;0.204703,0.801828|alpha_max,65280;..." -> one curve per name.
// Linear interpolation between keys, clamped outside them. The game bakes every curve into 128 entries and reads it
// with linear lookup (fn 0x11829530 / 0x117ba1a0); how the keys are interpolated while baking is not decoded.
struct VfxCurve {
    std::vector<std::pair<float, float>> keys; // (t, value), t in 0..1
    float eval(float t) const;
};

// One <property>, <ranged_property> or <texture_property>.
struct VfxProp {
    std::string value;       // value="..." (or texture path)
    float v[4] = {0, 0, 0, 0}; // x y z w
    int components = 0;      // number of x/y/z/w attributes present
    float min = 0, max = 0;  // ranged_property (numbers); colour ranges keep the raw text in min_text/max_text
    bool ranged = false;
    std::string min_text, max_text;
    std::map<std::string, VfxCurve> curves;
};

struct VfxProps {
    std::map<std::string, VfxProp> props;
    const VfxProp* find(const std::string& name) const;
    bool b(const std::string& name, bool def) const;
    float f(const std::string& name, float def) const;
    int i(const std::string& name, int def) const;
    void range(const std::string& name, float& lo, float& hi, float def_lo, float def_hi) const;
    // vector property (x, y[, z, w]); keeps the defaults for missing components
    void vec(const std::string& name, float* out, int n) const;
    // colour range "float4(r,g,b,a)": index 0 = min, 1 = max
    void colour(const std::string& name, float out_min[4], float out_max[4]) const;
    // value="float2(1.0,2.0)" style vectors; keeps the defaults for missing components
    void float_n(const std::string& name, float* out, int n) const;
    const VfxCurve* curve(const std::string& name, const std::string& sub) const;
    std::string text(const std::string& name) const;
};

struct VfxEmitterDef : VfxProps {
    std::string id, type; // type: continuous_mesh, burst_rigid, ...
};

struct VfxModifier : VfxProps {
    std::string fragment; // vs_translation, vs_colour, ps_texture, ...
};

struct VfxChain {
    std::string id;
    std::vector<VfxModifier> modifiers;
    const VfxModifier* find(const std::string& fragment) const;
};

struct VfxStack {
    std::string id;
    std::vector<std::string> emitters, chains;
    int scene_view_type = 0;
    bool cast_shadows = false;
};

struct VfxRefEmitter {
    std::string name;
    bool enable = true;
    float tint_min[4] = {1, 1, 1, 1}, tint_max[4] = {1, 1, 1, 1};
    float particle_lifetime_bias[2] = {0, 0}, emitter_lifetime_bias[2] = {0, 0};
};

struct VfxRef {
    std::string inst_name, library;
    float scale_min = 1, scale_max = 1, pos[3] = {0, 0, 0}, start_time = 0;
    bool infinite_life = true;
    std::vector<VfxRefEmitter> emitters;
    const VfxRefEmitter* find(const std::string& emitter) const;
};

struct VfxFile {
    std::map<std::string, std::string> vfx_attrs; // camera_following, affected_by_wind, construction_type, ...
    std::vector<VfxEmitterDef> emitters;
    std::vector<VfxChain> chains;
    std::vector<VfxStack> stacks;
    std::vector<VfxRef> refs;
    bool camera_following() const;
    const VfxEmitterDef* emitter(const std::string& id) const;
    const VfxChain* chain(const std::string& id) const;
};

VfxFile parse_vfx(const XmlDoc& doc);
std::map<std::string, VfxCurve> parse_curves(const std::string& text);

} // namespace wce
