#include "environment.h"

#include "formats/env_value.h"
#include "formats/xml_doc.h"
#include "render/shader_lib.h"

#include <cstdlib>
#include <fstream>
#include <sstream>

#include <algorithm>
#include <cmath>

namespace we {

namespace {

constexpr float kDeg = 3.14159265358979f / 180.0f;

struct Reader {
    const XmlDoc& doc;
    int elem = -1;

    bool has(const char* name) const { return elem >= 0 && doc.find_attr(elem, name); }
    EnvValue value(const char* name) const {
        if (elem < 0) return {};
        auto v = doc.get(elem, name);
        return v ? parse_value(*v) : EnvValue{};
    }
    void f(const char* name, float& out) const {
        EnvValue v = value(name);
        if ((v.kind == ValueKind::Float || v.kind == ValueKind::Int) && !v.f.empty()) out = v.f[0];
    }
    void b(const char* name, bool& out) const {
        EnvValue v = value(name);
        if (v.kind == ValueKind::Bool) out = v.b;
    }
    void rgb(const char* name, float3& out) const {
        EnvValue v = value(name);
        if (v.kind == ValueKind::Rgb) out = {v.f[0], v.f[1], v.f[2]};
    }
    void s(const char* name, std::string& out) const {
        if (auto v = elem >= 0 ? doc.get(elem, name) : std::nullopt) out = *v;
    }
};

// First element with this name anywhere in the document (each appears once in practice).
int find_element(const XmlDoc& doc, const char* name) {
    const auto& els = doc.elements();
    for (int i = 0; i < int(els.size()); ++i)
        if (els[i].name == name) return i;
    return -1;
}

} // namespace

float3 EnvironmentParams::to_sun() const {
    // Verified (memory dump 2026-10-06): azimuth from +Z towards +X, elevation above the horizon.
    float az = sun_azimuth * kDeg, el = sun_elevation * kDeg;
    return normalize(float3{std::cos(el) * std::sin(az), std::sin(el), std::cos(el) * std::cos(az)});
}

void read_output_settings(float& gamma, float& brightness) {
    const char* appdata = std::getenv("APPDATA");
    if (!appdata) return;
    std::ifstream f(std::string(appdata) + "\\The Creative Assembly\\Attila\\scripts\\preferences.script.txt");
    std::string key;
    float v;
    std::string line;
    while (std::getline(f, line)) {
        std::istringstream is(line);
        if (!(is >> key >> v)) continue;
        if (key == "gfx_gamma_setting" && v > 0.1f) gamma = v;
        else if (key == "gfx_brightness_setting" && v > 0.0f) brightness = v;
    }
}

EnvironmentParams blend_environments(const EnvironmentParams& a, const EnvironmentParams& b, float t) {
    EnvironmentParams r = t < 0.5f ? a : b;
    auto f = [t](float x, float y) { return x + (y - x) * t; };
    auto v = [&](const float3& x, const float3& y) { return float3{f(x.x, y.x), f(x.y, y.y), f(x.z, y.z)}; };
    auto arr = [&](float* o, const float* x, const float* y, int n) { for (int i = 0; i < n; ++i) o[i] = f(x[i], y[i]); };
    float daz = std::fmod(b.sun_azimuth - a.sun_azimuth + 540.0f, 360.0f) - 180.0f;
    r.sun_azimuth = a.sun_azimuth + daz * t;
    r.sun_elevation = f(a.sun_elevation, b.sun_elevation);
    r.sun_colour = v(a.sun_colour, b.sun_colour);
    r.sun_colour_scale = f(a.sun_colour_scale, b.sun_colour_scale);
    r.ambient_hdr_scaler = f(a.ambient_hdr_scaler, b.ambient_hdr_scaler);
    r.ambient_scale = f(a.ambient_scale, b.ambient_scale);
    r.ambient_left = v(a.ambient_left, b.ambient_left);
    r.ambient_right = v(a.ambient_right, b.ambient_right);
    r.ambient_front = v(a.ambient_front, b.ambient_front);
    r.ambient_back = v(a.ambient_back, b.ambient_back);
    r.ambient_top = v(a.ambient_top, b.ambient_top);
    r.ambient_bottom = v(a.ambient_bottom, b.ambient_bottom);
    r.deep_water = v(a.deep_water, b.deep_water);
    r.shallow_water = v(a.shallow_water, b.shallow_water);
    r.sea_bed_scatter = v(a.sea_bed_scatter, b.sea_bed_scatter);
    r.refraction_scatter = f(a.refraction_scatter, b.refraction_scatter);
    r.cubemap_sun_colour_scale = f(a.cubemap_sun_colour_scale, b.cubemap_sun_colour_scale);
    r.sky_colour = v(a.sky_colour, b.sky_colour);
    r.circumsolar_colour = v(a.circumsolar_colour, b.circumsolar_colour);
    r.horizon_colour = v(a.horizon_colour, b.horizon_colour);
    r.horizon_opposite_colour = v(a.horizon_opposite_colour, b.horizon_opposite_colour);
    r.backscatter_colour = v(a.backscatter_colour, b.backscatter_colour);
    r.sky_colour_scale = f(a.sky_colour_scale, b.sky_colour_scale);
    r.circumsolar_colour_scale = f(a.circumsolar_colour_scale, b.circumsolar_colour_scale);
    r.horizon_colour_scale = f(a.horizon_colour_scale, b.horizon_colour_scale);
    r.horizon_opposite_colour_scale = f(a.horizon_opposite_colour_scale, b.horizon_opposite_colour_scale);
    r.backscatter_colour_scale = f(a.backscatter_colour_scale, b.backscatter_colour_scale);
    r.horizon_decay = f(a.horizon_decay, b.horizon_decay);
    r.circumsolar_decay = f(a.circumsolar_decay, b.circumsolar_decay);
    r.backscatter_decay = f(a.backscatter_decay, b.backscatter_decay);
    r.fog_distance_start = f(a.fog_distance_start, b.fog_distance_start);
    r.fog_distance_strength = f(a.fog_distance_strength, b.fog_distance_strength);
    r.fog_distance_scale = f(a.fog_distance_scale, b.fog_distance_scale);
    r.fog_height_top = f(a.fog_height_top, b.fog_height_top);
    r.fog_height_strength = f(a.fog_height_strength, b.fog_height_strength);
    r.fog_clear_distance = f(a.fog_clear_distance, b.fog_clear_distance);
    r.fog_colour_blend = f(a.fog_colour_blend, b.fog_colour_blend);
    arr(r.volume_fog_colour, a.volume_fog_colour, b.volume_fog_colour, 4);
    r.fog_precipitation_distance_modifier = f(a.fog_precipitation_distance_modifier, b.fog_precipitation_distance_modifier);
    r.absolute_black_point = f(a.absolute_black_point, b.absolute_black_point);
    r.max_black_point = f(a.max_black_point, b.max_black_point);
    r.min_white_point = f(a.min_white_point, b.min_white_point);
    r.absolute_white_point = f(a.absolute_white_point, b.absolute_white_point);
    r.auto_exposure_speed = f(a.auto_exposure_speed, b.auto_exposure_speed);
    r.high_pass_threshold = f(a.high_pass_threshold, b.high_pass_threshold);
    arr(r.low_tones_bias, a.low_tones_bias, b.low_tones_bias, 3);
    arr(r.high_tones_bias, a.high_tones_bias, b.high_tones_bias, 3);
    r.overlay = v(a.overlay, b.overlay);
    r.brightness = v(a.brightness, b.brightness);
    r.contrast = v(a.contrast, b.contrast);
    r.saturation = v(a.saturation, b.saturation);
    r.hue = f(a.hue, b.hue);
    arr(r.dof_focus, a.dof_focus, b.dof_focus, 2);
    r.dof_blur_size = f(a.dof_blur_size, b.dof_blur_size);
    r.sun_scale = f(a.sun_scale, b.sun_scale);
    r.sun_flare = f(a.sun_flare, b.sun_flare);
    r.snowing_intensity = f(a.snowing_intensity, b.snowing_intensity);
    r.output_gamma = a.output_gamma;
    r.output_brightness = a.output_brightness;
    return r;
}

float3 colour_to_linear(const float3& c) {
    auto l = [](float x) { return x > 0 ? std::pow(x, 2.2f) : 0.0f; };
    return {l(c.x), l(c.y), l(c.z)};
}

EnvironmentParams read_environment(const XmlDoc& doc) {
    EnvironmentParams e;
    Reader lighting{doc, find_element(doc, "LIGHTING")};
    lighting.f("sun_direction_azimuth", e.sun_azimuth);
    lighting.f("sun_direction_elevation", e.sun_elevation);
    lighting.rgb("sun_colour", e.sun_colour);
    lighting.f("sun_colour_scale", e.sun_colour_scale);
    lighting.b("use_custom_ambient_cube_colours", e.use_custom_ambient);
    lighting.f("ambient_cube_hdr_scaler", e.ambient_hdr_scaler);
    lighting.f("ambient_scale", e.ambient_scale);
    lighting.rgb("ambient_cube_left", e.ambient_left);
    lighting.rgb("ambient_cube_right", e.ambient_right);
    lighting.rgb("ambient_cube_front", e.ambient_front);
    lighting.rgb("ambient_cube_back", e.ambient_back);
    lighting.rgb("ambient_cube_top", e.ambient_top);
    lighting.rgb("ambient_cube_bottom", e.ambient_bottom);
    lighting.rgb("deep_water_colour", e.deep_water);
    lighting.rgb("shallow_water_colour", e.shallow_water);
    lighting.f("sea_bed_light_scatter_red", e.sea_bed_scatter.x);
    lighting.f("sea_bed_light_scatter_green", e.sea_bed_scatter.y);
    lighting.f("sea_bed_light_scatter_blue", e.sea_bed_scatter.z);
    lighting.f("refraction_light_scatter", e.refraction_scatter);
    lighting.f("cubemap_sun_colour_scale", e.cubemap_sun_colour_scale);

    Reader sky{doc, find_element(doc, "SKYGEN")};
    e.has_sky = sky.elem >= 0;
    sky.rgb("sky_colour", e.sky_colour);
    sky.f("sky_colour_scale", e.sky_colour_scale);
    sky.rgb("circumsolar_colour", e.circumsolar_colour);
    sky.f("circumsolar_colour_scale", e.circumsolar_colour_scale);
    sky.f("circumsolar_decay", e.circumsolar_decay);
    sky.rgb("horizon_colour", e.horizon_colour);
    sky.f("horizon_colour_scale", e.horizon_colour_scale);
    sky.rgb("horizon_opposite_colour", e.horizon_opposite_colour);
    sky.f("horizon_opposite_colour_scale", e.horizon_opposite_colour_scale);
    sky.f("horizon_decay", e.horizon_decay);
    sky.rgb("backscatter_colour", e.backscatter_colour);
    sky.f("backscatter_colour_scale", e.backscatter_colour_scale);
    sky.f("backscatter_decay", e.backscatter_decay);

    Reader sunm{doc, find_element(doc, "SUN_MESH")};
    sunm.f("sun_scale", e.sun_scale);
    sunm.f("sun_flare", e.sun_flare);
    sunm.s("sun_texture", e.sun_texture);

    Reader fog{doc, find_element(doc, "FOG")};
    fog.f("fog_distance_start", e.fog_distance_start);
    fog.f("fog_distance_strength", e.fog_distance_strength);
    fog.f("fog_distance_scale", e.fog_distance_scale);
    fog.f("fog_height_top", e.fog_height_top);
    fog.f("fog_height_strength", e.fog_height_strength);
    fog.f("fog_clear_distance", e.fog_clear_distance);
    fog.f("fog_colour_blend", e.fog_colour_blend);
    fog.b("terrain_relative_height_fog", e.terrain_relative_height_fog);
    fog.f("fog_precipitation_distance_modifier", e.fog_precipitation_distance_modifier);
    if (EnvValue v = fog.value("volume_fog_colour"); v.kind == ValueKind::Argb)
        for (int i = 0; i < 4; ++i) e.volume_fog_colour[i] = v.f[i];

    Reader hdr{doc, find_element(doc, "HDR")};
    hdr.f("absolute_black_point", e.absolute_black_point);
    hdr.f("max_black_point", e.max_black_point);
    hdr.f("min_white_point", e.min_white_point);
    hdr.f("absolute_white_point", e.absolute_white_point);
    hdr.f("auto_exposure_speed", e.auto_exposure_speed);
    hdr.f("high_pass_threshold", e.high_pass_threshold);
    // Older files have single low/high_tones_bias; newer ones per channel.
    float lo = -1, hi = -1;
    hdr.f("low_tones_bias", lo);
    hdr.f("high_tones_bias", hi);
    for (int i = 0; i < 3; ++i) {
        if (lo >= 0) e.low_tones_bias[i] = lo;
        if (hi >= 0) e.high_tones_bias[i] = hi;
    }
    hdr.f("low_tones_bias_red", e.low_tones_bias[0]);
    hdr.f("low_tones_bias_green", e.low_tones_bias[1]);
    hdr.f("low_tones_bias_blue", e.low_tones_bias[2]);
    hdr.f("high_tones_bias_red", e.high_tones_bias[0]);
    hdr.f("high_tones_bias_green", e.high_tones_bias[1]);
    hdr.f("high_tones_bias_blue", e.high_tones_bias[2]);

    Reader pp{doc, find_element(doc, "POST_PROCESSING")};
    pp.rgb("overlay", e.overlay);
    pp.f("brightness_r", e.brightness.x); pp.f("brightness_g", e.brightness.y); pp.f("brightness_b", e.brightness.z);
    pp.f("contrast_r", e.contrast.x); pp.f("contrast_g", e.contrast.y); pp.f("contrast_b", e.contrast.z);
    pp.f("saturation_r", e.saturation.x); pp.f("saturation_g", e.saturation.y); pp.f("saturation_b", e.saturation.z);
    pp.f("hue", e.hue);

    Reader dof{doc, find_element(doc, "DEPTH_OF_FIELD")};
    dof.b("use_DOF", e.use_dof);
    if (EnvValue v = dof.value("DOF_focus"); v.kind == ValueKind::Variance) { e.dof_focus[0] = v.f[0]; e.dof_focus[1] = v.f[1]; }
    dof.f("DOF_blur_size", e.dof_blur_size);

    e.cloud_planes = read_cloud_planes(doc);

    Reader w{doc, find_element(doc, "WEATHER")};
    w.f("snowing_intensity", e.snowing_intensity);
    w.s("definition_name", e.weather_definition);
    return e;
}

void apply_environment(const EnvironmentParams& e, ConstantBuffers& cbs) {
    const float3 to_sun = e.to_sun();
    auto set3 = [&](const char* cb, const char* var, const float3& v) { cbs.set_f(cb, var, {v.x, v.y, v.z}); };

    // skydome_PS (verified: copied from precomputed sky-node fields, see docs/re/environment_to_shader.md)
    // Verified (memory dump + fn 0x18e3700): colours = pow(c, 2.2) * scale * 7.5 (HDR sky factor);
    // horizon decay * -0.5, circumsolar decay * -10, backscatter decay copied unchanged.
    const float sky = kSkyHdrScale;
    set3("skydome_PS", "g_linear_sky_colour_scaled", colour_to_linear(e.sky_colour) * (e.sky_colour_scale * sky));
    set3("skydome_PS", "g_linear_circumsolar_color_scaled", colour_to_linear(e.circumsolar_colour) * (e.circumsolar_colour_scale * sky));
    set3("skydome_PS", "g_linear_horizon_colour_scaled", colour_to_linear(e.horizon_colour) * (e.horizon_colour_scale * sky));
    set3("skydome_PS", "g_linear_horizon_opposite_colour_scaled", colour_to_linear(e.horizon_opposite_colour) * (e.horizon_opposite_colour_scale * sky));
    set3("skydome_PS", "g_linear_backscatter_colour_scaled", colour_to_linear(e.backscatter_colour) * (e.backscatter_colour_scale * sky));
    cbs.set_f("skydome_PS", "g_horizon_decay", {e.horizon_decay * -0.5f});
    cbs.set_f("skydome_PS", "g_circumsolar_decay", {e.circumsolar_decay * -10.0f});
    cbs.set_f("skydome_PS", "g_backscatter_decay", {e.backscatter_decay});
    set3("skydome_PS", "g_sun_direction", to_sun);

    // lighting_VS_PS. sun_direction is the direction light travels (shader uses -sun_direction as L).
    // Verified (memory dump): sun colour = pow(c, 2.2) * sun_colour_scale * 5000 (shaders multiply by 0.004 in HDR).
    // ASSUMPTION (custom cubes not yet in a dump): ambient = pow(c, 2.2) * ambient_cube_hdr_scaler * ambient_scale.
    // With use_custom_ambient_cube_colours = false the engine takes the cube from elsewhere (open item).
    // (The HDR white point is >= min_white_point (9 in most files), so scene light must reach several units.)
    set3("lighting_VS_PS", "sun_direction", -to_sun);
    set3("lighting_VS_PS", "sun_colour", colour_to_linear(e.sun_colour) * (e.sun_colour_scale * kSunHdrScale));
    float amb = e.ambient_scale * e.ambient_hdr_scaler;
    float3 lr[2] = {colour_to_linear(e.ambient_right) * amb, colour_to_linear(e.ambient_left) * amb};
    float3 tb[2] = {colour_to_linear(e.ambient_top) * amb, colour_to_linear(e.ambient_bottom) * amb};
    float3 fb[2] = {colour_to_linear(e.ambient_front) * amb, colour_to_linear(e.ambient_back) * amb};
    auto set_pair = [&](const char* var, const float3 v[2]) {
        float data[7] = {v[0].x, v[0].y, v[0].z, 0, v[1].x, v[1].y, v[1].z}; // float3[2]: 16-byte stride
        cbs.set("lighting_VS_PS", var, data, sizeof data);
    };
    set_pair("ambient_cube_lr", lr);
    set_pair("ambient_cube_tb", tb);
    set_pair("ambient_cube_fb", fb);
    set3("lighting_VS_PS", "g_deep_water_colour", colour_to_linear(e.deep_water));
    set3("lighting_VS_PS", "g_shallow_water_colour", colour_to_linear(e.shallow_water));
    set3("lighting_VS_PS", "g_sea_bed_light_scatter", e.sea_bed_scatter);
    cbs.set_f("lighting_VS_PS", "g_refraction_light_scatter", {e.refraction_scatter});
    int one = 1;
    cbs.set("lighting_VS_PS", "g_apply_environment_specular", &one, 4);
    cbs.set_f("lighting_VS_PS", "g_hdr_on", {1.0f});

    // fog_VS_PS. Verified (memory dump): colour = pow(2.2) of the first three components of "argb(...)"
    // (which are r, g, b; the fourth is alpha); start = start - fog_precipitation_distance_modifier; top >= 0.001.
    // Height bottom = 0 on the flat test map.
    set3("fog_VS_PS", "g_volume_fog_colour", colour_to_linear({e.volume_fog_colour[0], e.volume_fog_colour[1], e.volume_fog_colour[2]}));
    cbs.set_f("fog_VS_PS", "g_fog_distance_start", {e.fog_distance_start - e.fog_precipitation_distance_modifier});
    cbs.set_f("fog_VS_PS", "g_fog_distance_strength", {e.fog_distance_strength});
    cbs.set_f("fog_VS_PS", "g_fog_distance_scale", {e.fog_distance_scale});
    cbs.set_f("fog_VS_PS", "g_fog_height_bottom", {0.0f});
    cbs.set_f("fog_VS_PS", "g_fog_height_top", {std::max(e.fog_height_top, 0.001f)});
    cbs.set_f("fog_VS_PS", "g_fog_height_strength", {e.fog_height_strength});
    cbs.set_f("fog_VS_PS", "g_fog_colour_blend", {e.fog_colour_blend});
    cbs.set_f("fog_VS_PS", "g_fog_clear_distance", {e.fog_clear_distance});

    cbs.set_f("weather_shared", "g_snowing_intensity", {e.snowing_intensity});

    // HDR: the four limits go 1:1 into luma_stats_buffer (names match; used by the GPU luma statistics).
    cbs.set_f("luma_stats_buffer", "g_absolute_luma_black_point", {e.absolute_black_point});
    cbs.set_f("luma_stats_buffer", "g_max_luma_black_point", {e.max_black_point});
    cbs.set_f("luma_stats_buffer", "g_min_luma_white_point", {e.min_white_point});
    cbs.set_f("luma_stats_buffer", "g_absolute_luma_white_point", {e.absolute_white_point});
    cbs.set_f("auto_exposure_buffer", "g_auto_exposure_speed", {e.auto_exposure_speed});

    // Output colorimetry = the game's graphics options (verified: gfx_gamma_setting / gfx_brightness_setting
    // from preferences.script.txt appear unchanged in colorimetry_VS_PS).
    cbs.set_f("colorimetry_VS_PS", "g_brightness", {e.output_brightness});
    cbs.set_f("colorimetry_VS_PS", "g_gamma_output", {e.output_gamma});
    cbs.set_f("colorimetry_VS_PS", "g_inv_gamma_output", {1.0f / e.output_gamma});

    float4x4 cm = colour_matrix(e);
    cbs.set_m("hdr_to_screen_PS", "colour_matrix", cm.data());
}

float4x4 colour_matrix(const EnvironmentParams& e, float global_brightness) {
    // Verified from empire.retail.dll fn 0x172c100 (2026-10-06):
    //   M = Overlay * Hue * Saturation * Contrast * Brightness, applied to gamma-space colour as M * (rgb, 1).
    // Brightness is scaled by an engine factor ([this+0xe0]; 1.0 in the dump).
    float4x4 m;
    m.m[0][0] = e.brightness.x * global_brightness;
    m.m[1][1] = e.brightness.y * global_brightness;
    m.m[2][2] = e.brightness.z * global_brightness;

    float4x4 c; // contrast around 0.5
    const float con[3] = {e.contrast.x, e.contrast.y, e.contrast.z};
    for (int i = 0; i < 3; ++i) {
        c.m[i][i] = con[i];
        c.m[i][3] = 0.5f - 0.5f * con[i];
    }
    m = c * m;

    float4x4 s; // Haeberli saturation, per channel
    const float sat[3] = {e.saturation.x, e.saturation.y, e.saturation.z};
    const float w[3] = {0.3086f, 0.6094f, 0.0820f};
    float off[3];
    for (int i = 0; i < 3; ++i) off[i] = (1.0f - sat[i]) * w[i];
    for (int r = 0; r < 3; ++r)
        for (int col = 0; col < 3; ++col) s.m[r][col] = off[col] + (r == col ? sat[col] : 0.0f);
    m = s * m;

    float4x4 h; // hue rotation about the grey axis
    float a = e.hue * 0.0174532924f, cs = std::cos(a), sn = std::sin(a);
    float t = (1.0f - cs) / 3.0f, q = 0.577350259f * sn;
    // ASSUMPTION: sign layout of the off-diagonals (standard right-handed rotation); hue is 0 in 652/658 files.
    h.m[0][0] = h.m[1][1] = h.m[2][2] = t + cs;
    h.m[0][1] = t - q; h.m[0][2] = t + q;
    h.m[1][0] = t + q; h.m[1][2] = t - q;
    h.m[2][0] = t - q; h.m[2][1] = t + q;
    m = h * m;

    float4x4 o; // overlay multiplies the rows
    o.m[0][0] = e.overlay.x;
    o.m[1][1] = e.overlay.y;
    o.m[2][2] = e.overlay.z;
    return o * m;
}

void build_scurve(const EnvironmentParams& e, uint8_t out[256 * 4]) {
    // PLACEHOLDER: the engine builds a 256x1 texture ("HDR_CONSTANT_TONE_MAP_VIEW_COMMON SCurve Texture",
    // fn 0x178c710) but the code that fills it is not found yet. Until then: Schlick bias for the low half
    // and the high half, per channel (bias 0.5 = straight line).
    auto bias = [](float x, float b) {
        b = std::fmin(std::fmax(b, 0.001f), 0.999f);
        return x / ((1.0f / b - 2.0f) * (1.0f - x) + 1.0f);
    };
    const float lo[3] = {e.low_tones_bias[0], e.low_tones_bias[1], e.low_tones_bias[2]};
    const float hi[3] = {e.high_tones_bias[0], e.high_tones_bias[1], e.high_tones_bias[2]};
    for (int i = 0; i < 256; ++i) {
        float x = i / 255.0f;
        for (int ch = 0; ch < 3; ++ch) {
            float y = x < 0.5f ? 0.5f * bias(2 * x, lo[ch]) : 0.5f + 0.5f * bias(2 * x - 1, hi[ch]);
            out[i * 4 + ch] = uint8_t(std::lround(std::fmin(std::fmax(y, 0.0f), 1.0f) * 255));
        }
        out[i * 4 + 3] = 255;
    }
}

} // namespace we
