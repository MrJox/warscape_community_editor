//! Writes an environment's values into the game shaders' constant buffers: skydome_PS, lighting_VS_PS, fog_VS_PS, luma_stats_buffer,
//! auto_exposure_buffer, colorimetry_VS_PS and hdr_to_screen_PS. Transforms not yet verified against the game are marked ASSUMPTION.

use wce_foundation::math::Float3;
use wce_scene::environment::{EnvironmentParams, SKY_HDR_SCALE, SUN_HDR_SCALE, colour_matrix, colour_to_linear};

use crate::cbuffers::ConstantBuffers;

fn set3(cbs: &mut ConstantBuffers, cb: &str, var: &str, v: Float3) {
    cbs.set_f(cb, var, &[v.x, v.y, v.z]);
}

/// A float3[2]: 16-byte stride.
fn set_pair(cbs: &mut ConstantBuffers, var: &str, v: [Float3; 2]) {
    cbs.set_f("lighting_VS_PS", var, &[v[0].x, v[0].y, v[0].z, 0.0, v[1].x, v[1].y, v[1].z]);
}

pub fn apply_environment(e: &EnvironmentParams, cbs: &mut ConstantBuffers) {
    let to_sun = e.to_sun();

    // skydome_PS: copied from precomputed sky-node fields. Verified (3 memory dumps + fn 0x18e3700): colours = pow(c, 2.2) * scale *
    // the sky light factor (see SKY_HDR_SCALE); horizon decay * -0.5, circumsolar decay * -10, backscatter decay copied unchanged.
    let sky_light = if e.cubemap_sun_colour_scale != 0.0 { e.cubemap_sun_colour_scale } else { e.sun_colour_scale };
    let sky = sky_light.min(5.0) * SKY_HDR_SCALE;
    set3(cbs, "skydome_PS", "g_linear_sky_colour_scaled", colour_to_linear(e.sky_colour) * (e.sky_colour_scale * sky));
    set3(cbs, "skydome_PS", "g_linear_circumsolar_color_scaled", colour_to_linear(e.circumsolar_colour) * (e.circumsolar_colour_scale * sky));
    set3(cbs, "skydome_PS", "g_linear_horizon_colour_scaled", colour_to_linear(e.horizon_colour) * (e.horizon_colour_scale * sky));
    set3(cbs, "skydome_PS", "g_linear_horizon_opposite_colour_scaled", colour_to_linear(e.horizon_opposite_colour) * (e.horizon_opposite_colour_scale * sky));
    set3(cbs, "skydome_PS", "g_linear_backscatter_colour_scaled", colour_to_linear(e.backscatter_colour) * (e.backscatter_colour_scale * sky));
    cbs.set_f("skydome_PS", "g_horizon_decay", &[e.horizon_decay * -0.5]);
    cbs.set_f("skydome_PS", "g_circumsolar_decay", &[e.circumsolar_decay * -10.0]);
    cbs.set_f("skydome_PS", "g_backscatter_decay", &[e.backscatter_decay]);
    set3(cbs, "skydome_PS", "g_sun_direction", to_sun);

    // lighting_VS_PS. sun_direction is the direction light travels (the shader uses -sun_direction as L).
    // Verified (memory dump): sun colour = pow(c, 2.2) * sun_colour_scale * 5000 (the shaders multiply by 0.004 in HDR).
    // Verified (rain + snow dumps, use_custom_ambient_cube_colours = true): ambient = pow(c, 2.2) * ambient_cube_hdr_scaler (ambient_scale
    // isn't applied), stored as lr = (left, right), tb = (top, bottom), fb = (front, back). With use_custom_ambient_cube_colours = false
    // the engine takes the cube from elsewhere (open item).
    set3(cbs, "lighting_VS_PS", "sun_direction", -to_sun);
    set3(cbs, "lighting_VS_PS", "sun_colour", colour_to_linear(e.sun_colour) * (e.sun_colour_scale * SUN_HDR_SCALE));
    let amb = e.ambient_hdr_scaler;
    set_pair(cbs, "ambient_cube_lr", [colour_to_linear(e.ambient_left) * amb, colour_to_linear(e.ambient_right) * amb]);
    set_pair(cbs, "ambient_cube_tb", [colour_to_linear(e.ambient_top) * amb, colour_to_linear(e.ambient_bottom) * amb]);
    set_pair(cbs, "ambient_cube_fb", [colour_to_linear(e.ambient_front) * amb, colour_to_linear(e.ambient_back) * amb]);
    set3(cbs, "lighting_VS_PS", "g_deep_water_colour", colour_to_linear(e.deep_water));
    set3(cbs, "lighting_VS_PS", "g_shallow_water_colour", colour_to_linear(e.shallow_water));
    set3(cbs, "lighting_VS_PS", "g_sea_bed_light_scatter", e.sea_bed_scatter);
    cbs.set_f("lighting_VS_PS", "g_refraction_light_scatter", &[e.refraction_scatter]);
    cbs.set_i("lighting_VS_PS", "g_apply_environment_specular", 1);
    cbs.set_f("lighting_VS_PS", "g_hdr_on", &[1.0]);

    // fog_VS_PS. Verified (memory dump): colour = pow(2.2) of the first three components of "argb(...)" (which are r, g, b; the fourth
    // is alpha); start = start - fog_precipitation_distance_modifier; top >= 0.001. The height bottom is 0 on the flat test map.
    let fog = e.volume_fog_colour;
    set3(cbs, "fog_VS_PS", "g_volume_fog_colour", colour_to_linear(Float3::new(fog[0], fog[1], fog[2])));
    cbs.set_f("fog_VS_PS", "g_fog_distance_start", &[e.fog_distance_start - e.fog_precipitation_distance_modifier]);
    cbs.set_f("fog_VS_PS", "g_fog_distance_strength", &[e.fog_distance_strength]);
    cbs.set_f("fog_VS_PS", "g_fog_distance_scale", &[e.fog_distance_scale]);
    cbs.set_f("fog_VS_PS", "g_fog_height_bottom", &[0.0]);
    cbs.set_f("fog_VS_PS", "g_fog_height_top", &[e.fog_height_top.max(0.001)]);
    cbs.set_f("fog_VS_PS", "g_fog_height_strength", &[e.fog_height_strength]);
    cbs.set_f("fog_VS_PS", "g_fog_colour_blend", &[e.fog_colour_blend]);
    cbs.set_f("fog_VS_PS", "g_fog_clear_distance", &[e.fog_clear_distance]);

    // HDR: the four limits go 1:1 into luma_stats_buffer (the names match; used by the GPU luma statistics).
    cbs.set_f("luma_stats_buffer", "g_absolute_luma_black_point", &[e.absolute_black_point]);
    cbs.set_f("luma_stats_buffer", "g_max_luma_black_point", &[e.max_black_point]);
    cbs.set_f("luma_stats_buffer", "g_min_luma_white_point", &[e.min_white_point]);
    cbs.set_f("luma_stats_buffer", "g_absolute_luma_white_point", &[e.absolute_white_point]);
    cbs.set_f("auto_exposure_buffer", "g_auto_exposure_speed", &[e.auto_exposure_speed]);

    // The output colorimetry is the game's graphics options (verified: gfx_gamma_setting / gfx_brightness_setting from
    // preferences.script.txt appear unchanged in colorimetry_VS_PS).
    cbs.set_f("colorimetry_VS_PS", "g_brightness", &[e.output_brightness]);
    cbs.set_f("colorimetry_VS_PS", "g_gamma_output", &[e.output_gamma]);
    cbs.set_f("colorimetry_VS_PS", "g_inv_gamma_output", &[1.0 / e.output_gamma]);
    cbs.set_m("hdr_to_screen_PS", "colour_matrix", colour_matrix(e, 1.0).data());
}
