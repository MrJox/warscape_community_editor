#include "render/environment_apply.h"

#include "render/shader_lib.h"

#include <algorithm>
#include <cmath>

namespace wce {

void apply_environment(const EnvironmentParams& e, ConstantBuffers& cbs) {
    const float3 to_sun = e.to_sun();
    auto set3 = [&](const char* cb, const char* var, const float3& v) { cbs.set_f(cb, var, {v.x, v.y, v.z}); };

    // skydome_PS (verified: copied from precomputed sky-node fields, see docs/re/environment_to_shader.md)
    // Verified (3 memory dumps + fn 0x18e3700): colours = pow(c, 2.2) * scale * sky light factor (see kSkyHdrScale);
    // horizon decay * -0.5, circumsolar decay * -10, backscatter decay copied unchanged.
    const float sky_light = e.cubemap_sun_colour_scale != 0.0f ? e.cubemap_sun_colour_scale : e.sun_colour_scale;
    const float sky = std::min(sky_light, 5.0f) * kSkyHdrScale;
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
    // Verified (rain + snow dumps, use_custom_ambient_cube_colours = true): ambient = pow(c, 2.2) * ambient_cube_hdr_scaler
    // (ambient_scale is not applied), stored as lr = (left, right), tb = (top, bottom), fb = (front, back).
    // With use_custom_ambient_cube_colours = false the engine takes the cube from elsewhere (open item).
    // (The HDR white point is >= min_white_point (9 in most files), so scene light must reach several units.)
    set3("lighting_VS_PS", "sun_direction", -to_sun);
    set3("lighting_VS_PS", "sun_colour", colour_to_linear(e.sun_colour) * (e.sun_colour_scale * kSunHdrScale));
    float amb = e.ambient_hdr_scaler;
    float3 lr[2] = {colour_to_linear(e.ambient_left) * amb, colour_to_linear(e.ambient_right) * amb};
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

} // namespace wce
