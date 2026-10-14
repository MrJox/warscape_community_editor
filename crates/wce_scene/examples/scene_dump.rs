//! Differential dump of the environment parameters, colour matrix, tone curve, cross-fade and cloud planes, for comparing with the
//! C++ code fed the same files.
//!
//!   scene_dump <file with one .environment / .lighting path per line>

use wce_formats::xml_doc::XmlDoc;
use wce_foundation::math::Float3;
use wce_scene::clouds::{CloudField, CloudInstance, cloud_texture, generate_clouds};
use wce_scene::environment::*;
use wce_scene::render_scene::overlay_colour;

fn b(f: f32) -> String {
    format!("{:08x}", f.to_bits())
}

fn v3(v: Float3) -> String {
    format!("{},{},{}", b(v.x), b(v.y), b(v.z))
}

fn arr(f: &[f32]) -> String {
    f.iter().map(|f| format!("{},", b(*f))).collect()
}

fn fnv(bytes: impl IntoIterator<Item = u8>) -> u64 {
    bytes.into_iter().fold(0xCBF2_9CE4_8422_2325, |h, c| (h ^ u64::from(c)).wrapping_mul(0x0100_0000_01B3))
}

fn clouds_hash(instances: &[CloudInstance]) -> u64 {
    fnv(instances.iter().flat_map(|c| c.t1.iter().chain(&c.t2).chain(&c.t3).chain(&c.t4).chain(&c.t5).flat_map(|f| f.to_le_bytes()).collect::<Vec<_>>()))
}

fn env(tag: &str, e: &EnvironmentParams) {
    let mut line = tag.to_owned();
    line += &format!(" sun_azimuth={}", b(e.sun_azimuth));
    line += &format!(" sun_elevation={}", b(e.sun_elevation));
    line += &format!(" sun_colour={}", v3(e.sun_colour));
    line += &format!(" sun_colour_scale={}", b(e.sun_colour_scale));
    line += &format!(" use_custom_ambient={}", i32::from(e.use_custom_ambient));
    line += &format!(" ambient_hdr_scaler={}", b(e.ambient_hdr_scaler));
    line += &format!(" ambient_scale={}", b(e.ambient_scale));
    line += &format!(" ambient_left={}", v3(e.ambient_left));
    line += &format!(" ambient_right={}", v3(e.ambient_right));
    line += &format!(" ambient_front={}", v3(e.ambient_front));
    line += &format!(" ambient_back={}", v3(e.ambient_back));
    line += &format!(" ambient_top={}", v3(e.ambient_top));
    line += &format!(" ambient_bottom={}", v3(e.ambient_bottom));
    line += &format!(" deep_water={}", v3(e.deep_water));
    line += &format!(" shallow_water={}", v3(e.shallow_water));
    line += &format!(" sea_bed_scatter={}", v3(e.sea_bed_scatter));
    line += &format!(" refraction_scatter={}", b(e.refraction_scatter));
    line += &format!(" cubemap_sun_colour_scale={}", b(e.cubemap_sun_colour_scale));
    line += &format!(" has_sky={}", i32::from(e.has_sky));
    line += &format!(" sky_colour={}", v3(e.sky_colour));
    line += &format!(" circumsolar_colour={}", v3(e.circumsolar_colour));
    line += &format!(" horizon_colour={}", v3(e.horizon_colour));
    line += &format!(" horizon_opposite_colour={}", v3(e.horizon_opposite_colour));
    line += &format!(" backscatter_colour={}", v3(e.backscatter_colour));
    line += &format!(" sky_colour_scale={}", b(e.sky_colour_scale));
    line += &format!(" circumsolar_colour_scale={}", b(e.circumsolar_colour_scale));
    line += &format!(" horizon_colour_scale={}", b(e.horizon_colour_scale));
    line += &format!(" horizon_opposite_colour_scale={}", b(e.horizon_opposite_colour_scale));
    line += &format!(" backscatter_colour_scale={}", b(e.backscatter_colour_scale));
    line += &format!(" horizon_decay={}", b(e.horizon_decay));
    line += &format!(" circumsolar_decay={}", b(e.circumsolar_decay));
    line += &format!(" backscatter_decay={}", b(e.backscatter_decay));
    line += &format!(" fog_distance_start={}", b(e.fog_distance_start));
    line += &format!(" fog_distance_strength={}", b(e.fog_distance_strength));
    line += &format!(" fog_distance_scale={}", b(e.fog_distance_scale));
    line += &format!(" fog_height_top={}", b(e.fog_height_top));
    line += &format!(" fog_height_strength={}", b(e.fog_height_strength));
    line += &format!(" fog_clear_distance={}", b(e.fog_clear_distance));
    line += &format!(" fog_colour_blend={}", b(e.fog_colour_blend));
    line += &format!(" volume_fog_colour={}", arr(&e.volume_fog_colour));
    line += &format!(" terrain_relative_height_fog={}", i32::from(e.terrain_relative_height_fog));
    line += &format!(" fog_precipitation_distance_modifier={}", b(e.fog_precipitation_distance_modifier));
    line += &format!(" absolute_black_point={}", b(e.absolute_black_point));
    line += &format!(" max_black_point={}", b(e.max_black_point));
    line += &format!(" min_white_point={}", b(e.min_white_point));
    line += &format!(" absolute_white_point={}", b(e.absolute_white_point));
    line += &format!(" auto_exposure_speed={}", b(e.auto_exposure_speed));
    line += &format!(" high_pass_threshold={}", b(e.high_pass_threshold));
    line += &format!(" low_tones_bias={}", arr(&e.low_tones_bias));
    line += &format!(" high_tones_bias={}", arr(&e.high_tones_bias));
    line += &format!(" overlay={}", v3(e.overlay));
    line += &format!(" brightness={}", v3(e.brightness));
    line += &format!(" contrast={}", v3(e.contrast));
    line += &format!(" saturation={}", v3(e.saturation));
    line += &format!(" hue={}", b(e.hue));
    line += &format!(" use_dof={}", i32::from(e.use_dof));
    line += &format!(" dof_focus={}", arr(&e.dof_focus));
    line += &format!(" dof_blur_size={}", b(e.dof_blur_size));
    line += &format!(" sun_scale={}", b(e.sun_scale));
    line += &format!(" sun_flare={}", b(e.sun_flare));
    line += &format!(" sun_texture={}", e.sun_texture);
    line += &format!(" snowing_intensity={}", b(e.snowing_intensity));
    line += &format!(" wind_dir_random={}", i32::from(e.wind_dir_random));
    line += &format!(" wind_dir_degrees={}", b(e.wind_dir_degrees));
    line += &format!(" weather_definition={}", e.weather_definition);
    line += &format!(" output_gamma={}", b(e.output_gamma));
    line += &format!(" output_brightness={}", b(e.output_brightness));
    println!("{line}");
    println!("  to_sun={} linear={}", v3(e.to_sun()), v3(colour_to_linear(e.sun_colour)));
    for gb in [1.0, 1.3] {
        println!("  matrix {}", arr(colour_matrix(e, gb).data()));
    }

    println!("  scurve {:016x}", fnv(build_scurve(e).iter().flat_map(|v| v.to_le_bytes())));
    for p in &e.cloud_planes {
        println!("  plane far={} grid={} {} {} {} {} {} {} {} {} {} {} {} {} {} {} {} {} n={}", i32::from(p.far_plane), p.grid_size, b(p.height_scale), b(p.velocity),
            b(p.density), b(p.flatten), arr(&p.transmitted), arr(&p.reflected), arr(&p.shadow), b(p.transmit_falloff), b(p.shadow_gradient), b(p.shadow_falloff),
            b(p.horizon_alpha_start), b(p.horizon_alpha_end), b(p.height_variation), b(p.rotation), p.cloudset, cloud_texture(p, 1), p.clouds.len());
        for t in &p.clouds {
            let elements = t.elements.iter().map(|el| format!(" [{} {} {} {} {} {}]", i32::from(el.enabled), b(el.scale), arr(&el.offset), el.texture_index,
                el.texture_row, el.texture_column)).collect::<String>();
            println!("   cloud {} {}{elements}", i32::from(t.this_cloud_only), b(t.probability));
        }

        let g = generate_clouds(p, 12345);
        println!("   generated {} {:016x}", g.len(), clouds_hash(&g));
        let mut field = CloudField::default();
        field.reset(p, 777);
        for s in 0..300 {
            field.update(p, if s % 7 == 0 { 0.5 } else { 0.1 });
        }

        println!("   field {} {:016x}", field.instances().len(), clouds_hash(field.instances()));
    }
}

fn main() {
    let list = std::fs::read_to_string(std::env::args().nth(1).unwrap_or_default()).unwrap();
    let mut prev = EnvironmentParams::default();
    for path in list.lines() {
        println!("== {path}");
        let doc = match XmlDoc::parse(std::fs::read(path).unwrap()) {
            Ok(doc) => doc,
            Err(error) => {
                println!("error {error}");
                continue;
            }
        };

        let e = read_environment(&doc);
        env("env", &e);
        env("blend3", &blend_environments(&prev, &e, 0.3));
        env("blend7", &blend_environments(&prev, &e, 0.7));
        prev = e;
    }

    let o = read_graphics_options();
    println!("options {} {} {} {} {} {} {}", b(o.gamma), b(o.brightness), i32::from(o.vignette), i32::from(o.ssao), i32::from(o.distortion), o.depth_of_field, o.shadow_quality);
    println!("overlay {:08x} {:08x}", overlay_colour(0.25, 0.5, 1.2, 1.0), overlay_colour(-1.0, 0.999, 0.5, 0.5));
}
