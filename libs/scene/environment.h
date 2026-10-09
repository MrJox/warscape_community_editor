#pragma once
// Environment parameters (.environment / .lighting) and their mapping to shader constants.
//
// Reading: every value comes from the XmlDoc, so edits in the inspector show up on the next frame.
// Mapping: apply_environment() writes the cbuffers. Every transform that is not yet verified against
// the game is marked ASSUMPTION here and listed in docs/re/environment_to_shader.md.
#include "scene/clouds.h"
#include "foundation/math.h"

#include <string>
#include <vector>

namespace wce {

class XmlDoc;

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
    float volume_fog_colour[4] = {1, 1, 1, 1}; // r, g, b, a (the files say "argb(...)" but list r, g, b, a)
    bool terrain_relative_height_fog = true;
    float fog_precipitation_distance_modifier = 0;

    // HDR
    // Defaults = the engine's HDR block constructor (fn 0x177cd80).
    float absolute_black_point = 0.01f, max_black_point = 2, min_white_point = 6, absolute_white_point = 40;
    float auto_exposure_speed = 0.02f, high_pass_threshold = 10; // high_pass_threshold: not read by the battle HDR node
    float low_tones_bias[3] = {0.33f, 0.33f, 0.33f}, high_tones_bias[3] = {0.66f, 0.66f, 0.66f};

    // POST_PROCESSING
    float3 overlay{1, 1, 1}, brightness{1, 1, 1}, contrast{1, 1, 1}, saturation{1, 1, 1};
    float hue = 0;

    // DEPTH_OF_FIELD
    bool use_dof = false;
    float dof_focus[2] = {0.3f, 40};
    float dof_blur_size = 4.5f;

    // SUN_MESH
    float sun_scale = 0.1f, sun_flare = 1.0f;
    std::string sun_texture = "sundisk1.dds";

    // SKY_DOME > CLOUD_PLANE_MESH
    std::vector<CloudPlane> cloud_planes;

    // WEATHER (engine block at environment +0x2178: +0x2c wind_dir_random, +0x30 wind_dir_degrees,
    // +0x34 snowing_intensity -> weather_shared.g_snowing_intensity)
    float snowing_intensity = 0;
    bool wind_dir_random = false;
    float wind_dir_degrees = 0;
    std::string weather_definition;

    // Output settings (graphics options, not part of the file): preferences.script.txt
    float output_gamma = 2.2f, output_brightness = 1.0f;

    // derived
    float3 to_sun() const; // unit vector pointing at the sun
};

EnvironmentParams read_environment(const XmlDoc& doc);

// Engine HDR factors, verified against a memory dump of the game (docs/re/renderer.md).
constexpr float kSunHdrScale = 5000.0f; // lighting_VS_PS.sun_colour = linear * sun_colour_scale * 5000
// skydome_PS colours = linear * scale * min(sky_light_scale, 5) * 5000 / 1200 (fn 0x18e3700 via fn 0x17594b0),
// where sky_light_scale = cubemap_sun_colour_scale, or sun_colour_scale when that is 0. Verified on 3 dumps.
constexpr float kSkyHdrScale = 5000.0f / 1200.0f;

// Cross-fade between two environments (t = 0: a, 1: b). Numeric parameters interpolate linearly (azimuth
// along the shorter arc); switches, strings and cloud layouts take the nearer side.
// Verified (fn 0x175f4e0): the engine's lighting-block cross-fade is (b - a) * t + a on every numeric field.
EnvironmentParams blend_environments(const EnvironmentParams& a, const EnvironmentParams& b, float t);

// The player's graphics options (%APPDATA%/The Creative Assembly/Attila/scripts/preferences.script.txt) that change
// how weather renders: gamma/brightness (colorimetry), vignette (lens-flare vignette pass), depth of field, SSAO,
// distortion and shadow quality (selects the cascade split row, see Renderer::pass_shadows).
struct GraphicsOptions {
    float gamma = 2.2f, brightness = 1.0f;
    bool vignette = true, ssao = true, distortion = true;
    int depth_of_field = 1, shadow_quality = 3;
};
GraphicsOptions read_graphics_options();
void read_output_settings(float& gamma, float& brightness);

// sRGB-ish colour from the files to linear light: pow 2.2 (verified against the sun, sky, fog and ambient constants of 3 dumps).
float3 colour_to_linear(const float3& c);

// POST_PROCESSING -> hdr_to_screen_PS.colour_matrix (row-major, rows = output r, g, b, w).
float4x4 colour_matrix(const EnvironmentParams& e, float global_brightness = 1.0f);
// 256x1 RGBA16F tone curve (g_scurve_texture) as the engine builds it (half floats), see environment.cpp.
void build_scurve(const EnvironmentParams& e, uint16_t out[256 * 4]);

} // namespace wce
