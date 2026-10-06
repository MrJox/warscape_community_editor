#pragma once
// Environment parameters (.environment / .lighting) and their mapping to shader constants.
//
// Reading: every value comes from the XmlDoc, so edits in the inspector show up on the next frame.
// Mapping: apply_environment() writes the cbuffers. Every transform that is not yet verified against
// the game is marked ASSUMPTION here and listed in docs/re/environment_to_shader.md.
#include "render/math.h"

#include <string>

namespace we {

class XmlDoc;
class ConstantBuffers;

struct EnvironmentParams {
    // LIGHTING
    float sun_azimuth = 0, sun_elevation = 45;
    float3 sun_colour{1, 1, 1};
    float sun_colour_scale = 1;
    bool use_custom_ambient = true;
    float ambient_hdr_scaler = 1, ambient_scale = 1;
    float3 ambient_left, ambient_right, ambient_front, ambient_back, ambient_top, ambient_bottom;
    float3 deep_water{0.1f, 0.2f, 0.3f}, shallow_water{0.3f, 0.3f, 0.2f};
    float3 sea_bed_scatter{1, 1, 1};
    float refraction_scatter = 0.5f;
    float cubemap_sun_colour_scale = 0;

    // SKYGEN
    bool has_sky = false;
    float3 sky_colour, circumsolar_colour, horizon_colour, horizon_opposite_colour, backscatter_colour;
    float sky_colour_scale = 1, circumsolar_colour_scale = 1, horizon_colour_scale = 1, horizon_opposite_colour_scale = 1,
          backscatter_colour_scale = 0;
    float horizon_decay = 0.2f, circumsolar_decay = 0.3f, backscatter_decay = 0.5f;

    // FOG
    float fog_distance_start = 0, fog_distance_strength = 0, fog_distance_scale = 1;
    float fog_height_top = 0, fog_height_strength = 0, fog_clear_distance = 10000, fog_colour_blend = 0;
    float volume_fog_colour[4] = {1, 1, 1, 1}; // a, r, g, b as written
    bool terrain_relative_height_fog = true;
    float fog_precipitation_distance_modifier = 0;

    // HDR
    float absolute_black_point = 0.001f, max_black_point = 1, min_white_point = 9, absolute_white_point = 18;
    float auto_exposure_speed = 0.035f, high_pass_threshold = 10;
    float low_tones_bias[3] = {0.5f, 0.5f, 0.5f}, high_tones_bias[3] = {0.5f, 0.5f, 0.5f};

    // POST_PROCESSING
    float3 overlay{1, 1, 1}, brightness{1, 1, 1}, contrast{1, 1, 1}, saturation{1, 1, 1};
    float hue = 0;

    // DEPTH_OF_FIELD
    bool use_dof = false;
    float dof_focus[2] = {0.3f, 40};
    float dof_blur_size = 4.5f;

    // WEATHER
    float snowing_intensity = 0;
    std::string weather_definition;

    // derived
    float3 to_sun() const; // unit vector pointing at the sun
};

EnvironmentParams read_environment(const XmlDoc& doc);

// sRGB-ish colour from the files to linear light. ASSUMPTION: pow 2.2.
float3 colour_to_linear(const float3& c);

// Writes skydome_PS, lighting_VS_PS, fog_VS_PS, weather_shared, auto exposure / tone mapping cbuffers.
void apply_environment(const EnvironmentParams& e, ConstantBuffers& cbs);

// POST_PROCESSING -> hdr_to_screen_PS.colour_matrix (row-major, rows = output r, g, b, w).
float4x4 colour_matrix(const EnvironmentParams& e, float global_brightness = 1.0f);
// 256x1 RGBA8 tone curve (g_scurve_texture). PLACEHOLDER formula, see environment.cpp.
void build_scurve(const EnvironmentParams& e, uint8_t out[256 * 4]);

} // namespace we
