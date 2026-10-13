#include "scene/environment.h"

#include "formats/env_value.h"
#include "formats/xml_doc.h"

#include <cstdlib>
#include <fstream>
#include <sstream>

#include <algorithm>
#include <cmath>
#ifdef _WIN32
#include <DirectXPackedVector.h>
#else
#include <bit>
#include <cstdint>
#endif

namespace wce {

namespace {

constexpr float kDeg = 3.14159265358979f / 180.0f;

// float to half, round to nearest even (what XMConvertFloatToHalf does)
uint16_t float_to_half(float v) {
#ifdef _WIN32
    return DirectX::PackedVector::XMConvertFloatToHalf(v);
#else
    return std::bit_cast<uint16_t>(static_cast<_Float16>(v));
#endif
}

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

GraphicsOptions read_graphics_options() {
    GraphicsOptions o;
    const char* appdata = std::getenv("APPDATA");
#ifdef _WIN32
    if (!appdata) return o;
    std::ifstream f(std::string(appdata) + "\\The Creative Assembly\\Attila\\scripts\\preferences.script.txt");
#else
    // The game runs under Proton: its %APPDATA% is in the prefix of app 325610 (default Steam library), unless $APPDATA points elsewhere.
    std::string roaming;
    if (appdata) roaming = appdata;
    else if (const char* home = std::getenv("HOME")) roaming = std::string(home) + "/.local/share/Steam/steamapps/compatdata/325610/pfx/drive_c/users/steamuser/AppData/Roaming";
    else return o;
    std::ifstream f(roaming + "/The Creative Assembly/Attila/scripts/preferences.script.txt");
#endif
    std::string line;
    while (std::getline(f, line)) {
        std::istringstream is(line);
        std::string key, value;
        if (!(is >> key >> value)) continue;
        if (!value.empty() && value.back() == ';') value.pop_back();
        const float v = float(std::atof(value.c_str()));
        const bool on = value == "true" || (value != "false" && v != 0.0f);
        if (key == "gfx_gamma_setting" && v > 0.1f) o.gamma = v;
        else if (key == "gfx_brightness_setting" && v > 0.0f) o.brightness = v;
        else if (key == "gfx_vignette") o.vignette = on;
        else if (key == "gfx_ssao") o.ssao = on;
        else if (key == "gfx_distortion") o.distortion = on;
        else if (key == "gfx_depth_of_field") o.depth_of_field = int(v);
        else if (key == "gfx_shadow_quality") o.shadow_quality = std::clamp(int(v), 0, 3);
    }
    return o;
}

void read_output_settings(float& gamma, float& brightness) {
    const GraphicsOptions o = read_graphics_options();
    gamma = o.gamma;
    brightness = o.brightness;
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
    // The battle HDR node reads only these names (property table at dll+0x1da5ba0); the single low/high_tones_bias,
    // black_point/white_point and high_pass_threshold of older files belong to another table and are ignored.
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
    w.b("wind_dir_random", e.wind_dir_random);
    w.f("wind_dir_degrees", e.wind_dir_degrees);
    w.s("definition_name", e.weather_definition);
    return e;
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
    // Verified (fn 0x172c100): row0 = (t+c, t+q, t-q), row1 = (t-q, t+c, t+q), row2 = (t+q, t-q, t+c).
    h.m[0][0] = h.m[1][1] = h.m[2][2] = t + cs;
    h.m[0][1] = t + q; h.m[0][2] = t - q;
    h.m[1][0] = t - q; h.m[1][2] = t + q;
    h.m[2][0] = t + q; h.m[2][1] = t - q;
    m = h * m;

    float4x4 o; // overlay multiplies the rows
    o.m[0][0] = e.overlay.x;
    o.m[1][1] = e.overlay.y;
    o.m[2][2] = e.overlay.z;
    return o * m;
}

void build_scurve(const EnvironmentParams& e, uint16_t out[256 * 4]) {
    // Verified (empire.retail.dll): HDR_DYNAMIC_TONE_MAP_VIEW fills its 256x1 RGBA16F "SCurve Texture" with fn 0x17940b0,
    // passing the HDR block's per-channel (low, high) tone biases (fn 0x17284a0). Per channel, fn 0x174a780 samples the
    // cubic Bezier (0, 0), (1/3, low), (2/3, high), (1, 1) at 5120 points (basis matrix -1 3 -3 1 / 3 -6 3 0 / -3 3 0 0 /
    // 1 0 0 0 on (t^3, t^2, t, 1)), bins them by round(x * 255) and keeps per bin the sample whose x is nearest to
    // bin / 255; the texel is max(y, 0) as half floats, alpha 0. lock_rgb_relative_tones only links the channels while
    // editing in the engine's debug panel (fn 0x176c1b0).
    constexpr int kBins = 256, kSamples = kBins * 20;
    for (int ch = 0; ch < 3; ++ch) {
        const float px[4] = {0, 1.0f / 3, 2.0f / 3, 1}, py[4] = {0, e.low_tones_bias[ch], e.high_tones_bias[ch], 1};
        float bx[kBins], by[kBins];
        for (int i = 0; i < kBins; ++i) bx[i] = by[i] = 3.402823466e38f;
        for (int k = 0; k < kSamples; ++k) {
            const float t = float(k) * (1.0f / float(kSamples - 1));
            const float b0 = (1 - t) * (1 - t) * (1 - t), b1 = 3 * t * (1 - t) * (1 - t), b2 = 3 * t * t * (1 - t), b3 = t * t * t;
            const float x = b0 * px[0] + b1 * px[1] + b2 * px[2] + b3 * px[3];
            const float y = b0 * py[0] + b1 * py[1] + b2 * py[2] + b3 * py[3];
            const int bin = int(std::floor(std::fmin(std::fmax(x, 0.0f), 1.0f) * float(kBins - 1) + 0.5f));
            const float centre = float(bin) * (1.0f / float(kBins - 1));
            if (std::fabs(bx[bin] - centre) > std::fabs(x - centre)) {
                bx[bin] = x;
                by[bin] = y;
            }
        }
        for (int i = 0; i < kBins; ++i) out[i * 4 + ch] = float_to_half(std::fmax(by[i], 0.0f));
    }
    for (int i = 0; i < kBins; ++i) out[i * 4 + 3] = 0;
}

} // namespace wce
