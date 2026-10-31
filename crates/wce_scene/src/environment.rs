//! Environment parameters (.environment / .lighting) and their mapping to shader constants.
//!
//! Every value comes from the lossless XML document, so inspector edits show up on the next frame. Transforms not yet verified
//! against the game are marked ASSUMPTION.

use std::collections::BTreeMap;
use std::f32::consts::PI;
use std::path::PathBuf;

use wce_formats::c_num::strtod;
use wce_formats::env_value::{EnvValue, ValueKind, parse_value};
use wce_formats::xml_doc::XmlDoc;
use wce_foundation::math::{Float3, Float4x4, normalize};
use wce_foundation::game::GameProfile;

use crate::clouds::{CloudPlane, read_cloud_planes};
use crate::half::f32_to_f16;

const DEG: f32 = PI / 180.0;

/// Engine HDR factor, verified against a memory dump of the game: `lighting_VS_PS.sun_colour = linear * sun_colour_scale * 5000`.
pub const SUN_HDR_SCALE: f32 = 5000.0;

/// skydome_PS colours = linear * scale * min(sky_light_scale, 5) * 5000 / 1200 (fn 0x18e3700 via fn 0x17594b0), where sky_light_scale
/// is cubemap_sun_colour_scale, or sun_colour_scale when that is 0. Verified on 3 dumps.
pub const SKY_HDR_SCALE: f32 = 5000.0 / 1200.0;

#[derive(Clone, Debug, PartialEq)]
pub struct EnvironmentParams {
    // LIGHTING
    pub sun_azimuth: f32,
    pub sun_elevation: f32,
    pub sun_colour: Float3,
    pub sun_colour_scale: f32,
    pub use_custom_ambient: bool,
    pub ambient_hdr_scaler: f32,
    pub ambient_scale: f32,
    pub ambient_left: Float3,
    pub ambient_right: Float3,
    pub ambient_front: Float3,
    pub ambient_back: Float3,
    pub ambient_top: Float3,
    pub ambient_bottom: Float3,
    pub deep_water: Float3,
    pub shallow_water: Float3,
    pub sea_bed_scatter: Float3,
    pub refraction_scatter: f32,
    pub cubemap_sun_colour_scale: f32,

    // SKYGEN
    pub has_sky: bool,
    pub sky_colour: Float3,
    pub circumsolar_colour: Float3,
    pub horizon_colour: Float3,
    pub horizon_opposite_colour: Float3,
    pub backscatter_colour: Float3,
    pub sky_colour_scale: f32,
    pub circumsolar_colour_scale: f32,
    pub horizon_colour_scale: f32,
    pub horizon_opposite_colour_scale: f32,
    pub backscatter_colour_scale: f32,
    pub horizon_decay: f32,
    pub circumsolar_decay: f32,
    pub backscatter_decay: f32,

    // FOG
    pub fog_distance_start: f32,
    pub fog_distance_strength: f32,
    pub fog_distance_scale: f32,
    pub fog_height_top: f32,
    pub fog_height_strength: f32,
    pub fog_clear_distance: f32,
    pub fog_colour_blend: f32,

    /// r, g, b, a (the files say `argb(...)` but list r, g, b, a).
    pub volume_fog_colour: [f32; 4],
    pub terrain_relative_height_fog: bool,
    pub fog_precipitation_distance_modifier: f32,

    // HDR, with the defaults of the engine's HDR block constructor (fn 0x177cd80).
    pub absolute_black_point: f32,
    pub max_black_point: f32,
    pub min_white_point: f32,
    pub absolute_white_point: f32,
    pub auto_exposure_speed: f32,

    /// Not read by the battle HDR node.
    pub high_pass_threshold: f32,
    pub low_tones_bias: [f32; 3],
    pub high_tones_bias: [f32; 3],

    /// The single low / high tones biases of a file that has them (Rome 2's battle HDR node reads them; Attila's ignores them).
    pub tones_bias_single: Option<[f32; 2]>,

    // POST_PROCESSING
    pub overlay: Float3,
    pub brightness: Float3,
    pub contrast: Float3,
    pub saturation: Float3,
    pub hue: f32,

    // DEPTH_OF_FIELD
    pub use_dof: bool,
    pub dof_focus: [f32; 2],
    pub dof_blur_size: f32,

    // SUN_MESH
    pub sun_scale: f32,
    pub sun_flare: f32,
    pub sun_texture: String,

    /// SKY_DOME > CLOUD_PLANE_MESH.
    pub cloud_planes: Vec<CloudPlane>,

    // WEATHER (engine block at environment +0x2178: +0x2c wind_dir_random, +0x30 wind_dir_degrees,
    // +0x34 snowing_intensity -> weather_shared.g_snowing_intensity)
    pub snowing_intensity: f32,
    pub wind_dir_random: bool,
    pub wind_dir_degrees: f32,
    pub weather_definition: String,

    /// Output settings (graphics options, not part of the file): preferences.script.txt.
    pub output_gamma: f32,
    pub output_brightness: f32,
}

impl Default for EnvironmentParams {
    fn default() -> Self {
        let one = Float3::new(1.0, 1.0, 1.0);
        Self {
            sun_azimuth: 0.0,
            sun_elevation: 45.0,
            sun_colour: one,
            sun_colour_scale: 1.0,
            use_custom_ambient: true,
            ambient_hdr_scaler: 1.0,
            ambient_scale: 1.0,
            ambient_left: Float3::default(),
            ambient_right: Float3::default(),
            ambient_front: Float3::default(),
            ambient_back: Float3::default(),
            ambient_top: Float3::default(),
            ambient_bottom: Float3::default(),
            deep_water: Float3::new(0.1, 0.2, 0.3),
            shallow_water: Float3::new(0.3, 0.3, 0.2),
            sea_bed_scatter: one,
            refraction_scatter: 0.5,
            cubemap_sun_colour_scale: 0.0,
            has_sky: false,
            sky_colour: Float3::default(),
            circumsolar_colour: Float3::default(),
            horizon_colour: Float3::default(),
            horizon_opposite_colour: Float3::default(),
            backscatter_colour: Float3::default(),
            sky_colour_scale: 1.0,
            circumsolar_colour_scale: 1.0,
            horizon_colour_scale: 1.0,
            horizon_opposite_colour_scale: 1.0,
            backscatter_colour_scale: 0.0,
            horizon_decay: 0.2,
            circumsolar_decay: 0.3,
            backscatter_decay: 0.5,
            fog_distance_start: 0.0,
            fog_distance_strength: 0.0,
            fog_distance_scale: 1.0,
            fog_height_top: 0.0,
            fog_height_strength: 0.0,
            fog_clear_distance: 10000.0,
            fog_colour_blend: 0.0,
            volume_fog_colour: [1.0; 4],
            terrain_relative_height_fog: true,
            fog_precipitation_distance_modifier: 0.0,
            absolute_black_point: 0.01,
            max_black_point: 2.0,
            min_white_point: 6.0,
            absolute_white_point: 40.0,
            auto_exposure_speed: 0.02,
            high_pass_threshold: 10.0,
            low_tones_bias: [0.33; 3],
            high_tones_bias: [0.66; 3],
            tones_bias_single: None,
            overlay: one,
            brightness: one,
            contrast: one,
            saturation: one,
            hue: 0.0,
            use_dof: false,
            dof_focus: [0.3, 40.0],
            dof_blur_size: 4.5,
            sun_scale: 0.1,
            sun_flare: 1.0,
            sun_texture: "sundisk1.dds".to_owned(),
            cloud_planes: vec![],
            snowing_intensity: 0.0,
            wind_dir_random: false,
            wind_dir_degrees: 0.0,
            weather_definition: String::new(),
            output_gamma: 2.2,
            output_brightness: 1.0,
        }
    }
}

impl EnvironmentParams {

    /// The environment as seen in a scene turned `yaw` degrees about the vertical from the battle's frame (the battle's +z, its north, is
    /// the scene's (sin yaw, cos yaw)): the sun's azimuth turns with it, and the ambient cube's four sides are read again along the turned
    /// directions (exact for quarter turns; between them each side mixes its two neighbours by cos^2 / sin^2). ASSUMPTION: left / right
    /// are -x / +x, back / front -z / +z (a half turn swaps both pairs whatever they are). The wind's direction is turned by the renderer.
    pub fn turned(&self, yaw: f32) -> Self {
        let mut e = self.clone();
        if yaw.abs() < 1e-4 {
            return e;
        }

        e.sun_azimuth += yaw;

        // The cube of the battle's frame read along a scene direction: battle direction = R(-yaw) * scene direction.
        let (s, c) = (yaw * DEG).sin_cos();
        let side = |x: f32, z: f32| {
            let (bx, bz) = (c * x - s * z, s * x + c * z);
            let px = if bx >= 0.0 { self.ambient_right } else { self.ambient_left };
            let pz = if bz >= 0.0 { self.ambient_front } else { self.ambient_back };
            px * (bx * bx) + pz * (bz * bz)
        };

        (e.ambient_right, e.ambient_left, e.ambient_front, e.ambient_back) = (side(1.0, 0.0), side(-1.0, 0.0), side(0.0, 1.0), side(0.0, -1.0));
        e
    }

    /// The unit vector pointing at the sun. Verified (memory dump): azimuth from +Z towards +X, elevation above the horizon.
    pub fn to_sun(&self) -> Float3 {
        let (az, el) = (self.sun_azimuth * DEG, self.sun_elevation * DEG);
        normalize(Float3::new(el.cos() * az.sin(), el.sin(), el.cos() * az.cos()))
    }
}

/// Typed reads of one element's attributes; a missing element or a value of another kind leaves the target as it is.
struct Reader<'a> {
    doc: &'a XmlDoc,
    elem: Option<usize>,
}

impl<'a> Reader<'a> {

    /// The first element with this name anywhere in the document (each appears once in practice).
    fn new(doc: &'a XmlDoc, name: &str) -> Self {
        Self { doc, elem: doc.elements().iter().position(|el| el.name == name) }
    }

    fn value(&self, name: &str) -> EnvValue {
        self.elem.and_then(|elem| self.doc.get(elem, name)).map(|value| parse_value(&value)).unwrap_or_default()
    }

    fn f(&self, name: &str, out: &mut f32) {
        let v = self.value(name);
        if matches!(v.kind, ValueKind::Float | ValueKind::Int) && !v.f.is_empty() {
            *out = v.f[0];
        }
    }

    fn b(&self, name: &str, out: &mut bool) {
        let v = self.value(name);
        if v.kind == ValueKind::Bool {
            *out = v.b;
        }
    }

    fn rgb(&self, name: &str, out: &mut Float3) {
        let v = self.value(name);
        if v.kind == ValueKind::Rgb {
            *out = Float3::new(v.f[0], v.f[1], v.f[2]);
        }
    }

    fn s(&self, name: &str, out: &mut String) {
        if let Some(value) = self.elem.and_then(|elem| self.doc.get(elem, name)) {
            *out = value;
        }
    }
}

/// Reads the environment parameters of a .environment / .lighting document.
pub fn read_environment(doc: &XmlDoc) -> EnvironmentParams {
    let mut e = EnvironmentParams::default();
    let lighting = Reader::new(doc, "LIGHTING");
    lighting.f("sun_direction_azimuth", &mut e.sun_azimuth);
    lighting.f("sun_direction_elevation", &mut e.sun_elevation);
    lighting.rgb("sun_colour", &mut e.sun_colour);
    lighting.f("sun_colour_scale", &mut e.sun_colour_scale);
    lighting.b("use_custom_ambient_cube_colours", &mut e.use_custom_ambient);
    lighting.f("ambient_cube_hdr_scaler", &mut e.ambient_hdr_scaler);
    lighting.f("ambient_scale", &mut e.ambient_scale);
    lighting.rgb("ambient_cube_left", &mut e.ambient_left);
    lighting.rgb("ambient_cube_right", &mut e.ambient_right);
    lighting.rgb("ambient_cube_front", &mut e.ambient_front);
    lighting.rgb("ambient_cube_back", &mut e.ambient_back);
    lighting.rgb("ambient_cube_top", &mut e.ambient_top);
    lighting.rgb("ambient_cube_bottom", &mut e.ambient_bottom);
    lighting.rgb("deep_water_colour", &mut e.deep_water);
    lighting.rgb("shallow_water_colour", &mut e.shallow_water);
    lighting.f("sea_bed_light_scatter_red", &mut e.sea_bed_scatter.x);
    lighting.f("sea_bed_light_scatter_green", &mut e.sea_bed_scatter.y);
    lighting.f("sea_bed_light_scatter_blue", &mut e.sea_bed_scatter.z);
    lighting.f("refraction_light_scatter", &mut e.refraction_scatter);
    lighting.f("cubemap_sun_colour_scale", &mut e.cubemap_sun_colour_scale);

    let sky = Reader::new(doc, "SKYGEN");
    e.has_sky = sky.elem.is_some();
    sky.rgb("sky_colour", &mut e.sky_colour);
    sky.f("sky_colour_scale", &mut e.sky_colour_scale);
    sky.rgb("circumsolar_colour", &mut e.circumsolar_colour);
    sky.f("circumsolar_colour_scale", &mut e.circumsolar_colour_scale);
    sky.f("circumsolar_decay", &mut e.circumsolar_decay);
    sky.rgb("horizon_colour", &mut e.horizon_colour);
    sky.f("horizon_colour_scale", &mut e.horizon_colour_scale);
    sky.rgb("horizon_opposite_colour", &mut e.horizon_opposite_colour);
    sky.f("horizon_opposite_colour_scale", &mut e.horizon_opposite_colour_scale);
    sky.f("horizon_decay", &mut e.horizon_decay);
    sky.rgb("backscatter_colour", &mut e.backscatter_colour);
    sky.f("backscatter_colour_scale", &mut e.backscatter_colour_scale);
    sky.f("backscatter_decay", &mut e.backscatter_decay);

    let sun_mesh = Reader::new(doc, "SUN_MESH");
    sun_mesh.f("sun_scale", &mut e.sun_scale);
    sun_mesh.f("sun_flare", &mut e.sun_flare);
    sun_mesh.s("sun_texture", &mut e.sun_texture);

    let fog = Reader::new(doc, "FOG");
    fog.f("fog_distance_start", &mut e.fog_distance_start);
    fog.f("fog_distance_strength", &mut e.fog_distance_strength);
    fog.f("fog_distance_scale", &mut e.fog_distance_scale);
    fog.f("fog_height_top", &mut e.fog_height_top);
    fog.f("fog_height_strength", &mut e.fog_height_strength);
    fog.f("fog_clear_distance", &mut e.fog_clear_distance);
    fog.f("fog_colour_blend", &mut e.fog_colour_blend);
    fog.b("terrain_relative_height_fog", &mut e.terrain_relative_height_fog);
    fog.f("fog_precipitation_distance_modifier", &mut e.fog_precipitation_distance_modifier);
    let v = fog.value("volume_fog_colour");
    if v.kind == ValueKind::Argb {
        e.volume_fog_colour.copy_from_slice(&v.f[..4]);
    }

    // The battle HDR node reads only these names (property table at dll+0x1da5ba0); the single low / high_tones_bias, black_point /
    // white_point and high_pass_threshold of older files belong to another table and are ignored.
    let hdr = Reader::new(doc, "HDR");
    hdr.f("absolute_black_point", &mut e.absolute_black_point);
    hdr.f("max_black_point", &mut e.max_black_point);
    hdr.f("min_white_point", &mut e.min_white_point);
    hdr.f("absolute_white_point", &mut e.absolute_white_point);
    hdr.f("auto_exposure_speed", &mut e.auto_exposure_speed);
    for (i, channel) in ["red", "green", "blue"].iter().enumerate() {
        hdr.f(&format!("low_tones_bias_{channel}"), &mut e.low_tones_bias[i]);
    }

    for (i, channel) in ["red", "green", "blue"].iter().enumerate() {
        hdr.f(&format!("high_tones_bias_{channel}"), &mut e.high_tones_bias[i]);
    }

    // Read for the renderer of a game that uses them (Rome 2).
    let (mut low, mut high) = (f32::NAN, f32::NAN);
    hdr.f("low_tones_bias", &mut low);
    hdr.f("high_tones_bias", &mut high);
    if !low.is_nan() || !high.is_nan() {
        e.tones_bias_single = Some([if low.is_nan() { 0.33 } else { low }, if high.is_nan() { 0.66 } else { high }]);
    }

    let pp = Reader::new(doc, "POST_PROCESSING");
    pp.rgb("overlay", &mut e.overlay);
    for (name, target) in [("brightness", &mut e.brightness), ("contrast", &mut e.contrast), ("saturation", &mut e.saturation)] {
        pp.f(&format!("{name}_r"), &mut target.x);
        pp.f(&format!("{name}_g"), &mut target.y);
        pp.f(&format!("{name}_b"), &mut target.z);
    }

    pp.f("hue", &mut e.hue);

    let dof = Reader::new(doc, "DEPTH_OF_FIELD");
    dof.b("use_DOF", &mut e.use_dof);
    let v = dof.value("DOF_focus");
    if v.kind == ValueKind::Variance {
        e.dof_focus = [v.f[0], v.f[1]];
    }

    dof.f("DOF_blur_size", &mut e.dof_blur_size);
    e.cloud_planes = read_cloud_planes(doc);

    let weather = Reader::new(doc, "WEATHER");
    weather.f("snowing_intensity", &mut e.snowing_intensity);
    weather.b("wind_dir_random", &mut e.wind_dir_random);
    weather.f("wind_dir_degrees", &mut e.wind_dir_degrees);
    weather.s("definition_name", &mut e.weather_definition);
    e
}

/// Cross-fade between two environments (t = 0: a, 1: b). Numbers interpolate linearly (the azimuth along the shorter arc); switches,
/// strings and cloud layouts take the nearer side. Verified (fn 0x175f4e0): the engine's lighting-block cross-fade is (b - a) * t + a
/// on every numeric field.
pub fn blend_environments(a: &EnvironmentParams, b: &EnvironmentParams, t: f32) -> EnvironmentParams {
    let mut r = if t < 0.5 { a.clone() } else { b.clone() };
    let f = |x: f32, y: f32| x + (y - x) * t;
    let v = |x: Float3, y: Float3| Float3::new(f(x.x, y.x), f(x.y, y.y), f(x.z, y.z));
    let arr = |x: &[f32], y: &[f32], out: &mut [f32]| out.iter_mut().zip(x.iter().zip(y)).for_each(|(o, (x, y))| *o = f(*x, *y));

    let daz = (b.sun_azimuth - a.sun_azimuth + 540.0) % 360.0 - 180.0;
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
    arr(&a.volume_fog_colour, &b.volume_fog_colour, &mut r.volume_fog_colour);
    r.fog_precipitation_distance_modifier = f(a.fog_precipitation_distance_modifier, b.fog_precipitation_distance_modifier);
    r.absolute_black_point = f(a.absolute_black_point, b.absolute_black_point);
    r.max_black_point = f(a.max_black_point, b.max_black_point);
    r.min_white_point = f(a.min_white_point, b.min_white_point);
    r.absolute_white_point = f(a.absolute_white_point, b.absolute_white_point);
    r.auto_exposure_speed = f(a.auto_exposure_speed, b.auto_exposure_speed);
    r.high_pass_threshold = f(a.high_pass_threshold, b.high_pass_threshold);
    arr(&a.low_tones_bias, &b.low_tones_bias, &mut r.low_tones_bias);
    arr(&a.high_tones_bias, &b.high_tones_bias, &mut r.high_tones_bias);
    r.tones_bias_single = match (a.tones_bias_single, b.tones_bias_single) {
        (Some(x), Some(y)) => Some([f(x[0], y[0]), f(x[1], y[1])]),
        (x, y) => x.or(y),
    };
    r.overlay = v(a.overlay, b.overlay);
    r.brightness = v(a.brightness, b.brightness);
    r.contrast = v(a.contrast, b.contrast);
    r.saturation = v(a.saturation, b.saturation);
    r.hue = f(a.hue, b.hue);
    arr(&a.dof_focus, &b.dof_focus, &mut r.dof_focus);
    r.dof_blur_size = f(a.dof_blur_size, b.dof_blur_size);
    r.sun_scale = f(a.sun_scale, b.sun_scale);
    r.sun_flare = f(a.sun_flare, b.sun_flare);
    r.snowing_intensity = f(a.snowing_intensity, b.snowing_intensity);
    r.output_gamma = a.output_gamma;
    r.output_brightness = a.output_brightness;
    r
}

/// The graphics options the renderer applies (`wce_foundation::graphics`): gamma and brightness (colorimetry), vignette (lens-flare
/// vignette pass), depth of field, SSAO, distortion, shadow quality (which selects the cascade split row), water quality (the water
/// shaders' level) and texture filtering; and every `gfx_*` value read, for the options screen.
#[derive(Clone, Debug, PartialEq)]
pub struct GraphicsOptions {
    pub gamma: f32,
    pub brightness: f32,
    pub vignette: bool,
    pub ssao: bool,
    pub distortion: bool,
    pub depth_of_field: i32,
    pub shadow_quality: i32,
    pub water_quality: i32,
    pub building_quality: i32,
    pub unit_quality: i32,
    pub tree_quality: i32,
    pub sky_quality: i32,
    pub reflections: i32,
    pub effects_quality: i32,
    pub grain: bool,

    /// 0 trilinear, n anisotropic 2^n (fn 0x1177f410).
    pub texture_filtering: i32,

    /// The `gfx_*` values these come from.
    pub values: BTreeMap<String, String>,
}

impl Default for GraphicsOptions {
    fn default() -> Self {
        Self {
            gamma: 2.2, brightness: 1.0, vignette: true, ssao: true, distortion: true, depth_of_field: 1, shadow_quality: 3, water_quality: 3, building_quality: 3, unit_quality: 3, tree_quality: 3, sky_quality: 3, reflections: 2, effects_quality: 3, grain: false, texture_filtering: 3,
            values: BTreeMap::new(),
        }
    }
}

impl GraphicsOptions {
    /// The options of `gfx_*` values (a value missing keeps its default).
    pub fn from_values(values: BTreeMap<String, String>) -> Self {
        let mut o = Self::default();
        for (key, value) in &values {
            let v = strtod(value).unwrap_or(0.0) as f32;
            let on = value == "true" || (value != "false" && v != 0.0);
            match key.as_str() {
                "gfx_gamma_setting" if v > 0.1 => o.gamma = v,
                "gfx_brightness_setting" if v > 0.0 => o.brightness = v,
                "gfx_vignette" => o.vignette = on,
                "gfx_grain" => o.grain = on,
                "gfx_ssao" => o.ssao = on,
                "gfx_distortion" => o.distortion = on,
                "gfx_depth_of_field" => o.depth_of_field = v as i32,
                "gfx_shadow_quality" => o.shadow_quality = (v as i32).clamp(0, 3),
                "gfx_water_quality" => o.water_quality = (v as i32).clamp(0, 4),
                "gfx_building_quality" => o.building_quality = (v as i32).clamp(0, 5),
                "gfx_unit_quality" => o.unit_quality = (v as i32).clamp(0, 5),
                "gfx_tree_quality" => o.tree_quality = (v as i32).clamp(0, 5),
                "gfx_sky_quality" => o.sky_quality = (v as i32).clamp(0, 5),
                "gfx_screen_space_reflections" => o.reflections = (v as i32).clamp(0, 2),
                "gfx_effects_quality" => o.effects_quality = (v as i32).clamp(0, 5),
                "gfx_texture_filtering" => o.texture_filtering = (v as i32).clamp(0, 4),
                _ => {}
            }
        }

        o.values = values;
        o
    }
}

/// The game's preferences file. On Linux the game runs under Proton: its `%APPDATA%` is in the prefix of the game's app (the default
/// Steam library), unless `$APPDATA` points elsewhere.
fn preferences_path(game: &GameProfile) -> Option<PathBuf> {
    let appdata = std::env::var("APPDATA").ok();
    let roaming = if cfg!(windows) {
        PathBuf::from(appdata?)
    } else {
        match appdata {
            Some(appdata) => PathBuf::from(appdata),
            None => PathBuf::from(std::env::var("HOME").ok()?)
                .join(format!(".local/share/Steam/steamapps/compatdata/{}/pfx/drive_c/users/steamuser/AppData/Roaming", game.steam_app_id)),
        }
    };

    Some(roaming.join("The Creative Assembly").join(game.appdata_folder).join("scripts").join("preferences.script.txt"))
}

/// Reads the graphics options from a preferences text (`key value;` lines).
pub fn parse_graphics_options(text: &str) -> GraphicsOptions {
    GraphicsOptions::from_values(wce_foundation::graphics::parse_preferences(text))
}

/// The `gfx_*` values of the player's preferences file in a game; none when it can't be read.
pub fn read_game_graphics_values(game: &GameProfile) -> BTreeMap<String, String> {
    preferences_path(game)
        .and_then(|path| std::fs::read(path).ok())
        .map(|bytes| wce_foundation::graphics::parse_preferences(&String::from_utf8_lossy(&bytes)))
        .unwrap_or_default()
}

/// The player's graphics options in a game (its preferences file); the defaults when it can't be read.
pub fn read_graphics_options(game: &GameProfile) -> GraphicsOptions {
    GraphicsOptions::from_values(read_game_graphics_values(game))
}

/// A game's graphics options as the editor's settings choose them: the game's file, or the editor's own values over it.
pub fn graphics_options(game: &GameProfile, settings: &wce_foundation::settings::GraphicsSettings) -> GraphicsOptions {
    let mut values = read_game_graphics_values(game);
    if !settings.use_game {
        values.extend(settings.options.iter().map(|(k, v)| (k.clone(), v.clone())));
    }

    GraphicsOptions::from_values(values)
}

/// Where the game's preferences file is (for the Settings window).
pub fn preferences_file(game: &GameProfile) -> Option<PathBuf> {
    preferences_path(game)
}

/// The output gamma and brightness of the graphics options in a game.
pub fn read_output_settings(game: &GameProfile) -> (f32, f32) {
    let o = read_graphics_options(game);
    (o.gamma, o.brightness)
}

/// A colour of the files to linear light: pow 2.2 (verified against the sun, sky, fog and ambient constants of 3 dumps).
pub fn colour_to_linear(c: Float3) -> Float3 {
    let l = |x: f32| if x > 0.0 { x.powf(2.2) } else { 0.0 };
    Float3::new(l(c.x), l(c.y), l(c.z))
}

/// POST_PROCESSING -> hdr_to_screen_PS.colour_matrix (row-major; rows = output r, g, b, w).
///
/// Verified from empire.retail.dll fn 0x172c100: M = Overlay * Hue * Saturation * Contrast * Brightness, applied to gamma-space colour
/// as M * (rgb, 1). Brightness is scaled by an engine factor (`[this+0xe0]`, 1.0 in the dump).
pub fn colour_matrix(e: &EnvironmentParams, global_brightness: f32) -> Float4x4 {
    let mut m = Float4x4::IDENTITY;
    m.m[0][0] = e.brightness.x * global_brightness;
    m.m[1][1] = e.brightness.y * global_brightness;
    m.m[2][2] = e.brightness.z * global_brightness;

    // Contrast around 0.5.
    let mut c = Float4x4::IDENTITY;
    for (i, con) in [e.contrast.x, e.contrast.y, e.contrast.z].into_iter().enumerate() {
        c.m[i][i] = con;
        c.m[i][3] = 0.5 - 0.5 * con;
    }

    m = c * m;

    // Haeberli saturation, per channel.
    let sat = [e.saturation.x, e.saturation.y, e.saturation.z];
    let w = [0.3086f32, 0.6094, 0.0820];
    let off: [f32; 3] = std::array::from_fn(|i| (1.0 - sat[i]) * w[i]);
    let mut s = Float4x4::IDENTITY;
    for r in 0..3 {
        for col in 0..3 {
            s.m[r][col] = off[col] + if r == col { sat[col] } else { 0.0 };
        }
    }

    m = s * m;

    // Hue rotation about the grey axis. Verified (fn 0x172c100): row0 = (t+c, t+q, t-q), row1 = (t-q, t+c, t+q), row2 = (t+q, t-q, t+c).
    let a = e.hue * 0.017_453_292;
    let (cs, sn) = (a.cos(), a.sin());
    let (t, q) = ((1.0 - cs) / 3.0, 0.577_350_26 * sn);
    let mut h = Float4x4::IDENTITY;
    h.m[0][0] = t + cs;
    h.m[1][1] = t + cs;
    h.m[2][2] = t + cs;
    h.m[0][1] = t + q;
    h.m[0][2] = t - q;
    h.m[1][0] = t - q;
    h.m[1][2] = t + q;
    h.m[2][0] = t + q;
    h.m[2][1] = t - q;
    m = h * m;

    // The overlay multiplies the rows.
    let mut o = Float4x4::IDENTITY;
    o.m[0][0] = e.overlay.x;
    o.m[1][1] = e.overlay.y;
    o.m[2][2] = e.overlay.z;
    o * m
}

/// The 256x1 RGBA16F tone curve (g_scurve_texture) as the engine builds it, as half floats.
///
/// Verified (empire.retail.dll): HDR_DYNAMIC_TONE_MAP_VIEW fills its "SCurve Texture" with fn 0x17940b0, passing the HDR block's per
/// channel (low, high) tone biases (fn 0x17284a0). Per channel, fn 0x174a780 samples the cubic Bezier (0, 0), (1/3, low), (2/3, high),
/// (1, 1) at 5120 points, bins them by round(x * 255) and keeps per bin the sample whose x is nearest to bin / 255; the texel is
/// max(y, 0), alpha 0. lock_rgb_relative_tones only links the channels while editing in the engine's debug panel (fn 0x176c1b0).
pub fn build_scurve(e: &EnvironmentParams) -> [u16; 256 * 4] {
    const BINS: usize = 256;
    const SAMPLES: usize = BINS * 20;
    let mut out = [0u16; BINS * 4];
    for ch in 0..3 {
        let px = [0.0f32, 1.0 / 3.0, 2.0 / 3.0, 1.0];
        let py = [0.0, e.low_tones_bias[ch], e.high_tones_bias[ch], 1.0];
        let mut bx = [f32::MAX; BINS];
        let mut by = [f32::MAX; BINS];
        for k in 0..SAMPLES {
            let t = k as f32 * (1.0 / (SAMPLES - 1) as f32);
            let b0 = (1.0 - t) * (1.0 - t) * (1.0 - t);
            let b1 = 3.0 * t * (1.0 - t) * (1.0 - t);
            let b2 = 3.0 * t * t * (1.0 - t);
            let b3 = t * t * t;
            let x = b0 * px[0] + b1 * px[1] + b2 * px[2] + b3 * px[3];
            let y = b0 * py[0] + b1 * py[1] + b2 * py[2] + b3 * py[3];
            let bin = (x.clamp(0.0, 1.0) * (BINS - 1) as f32 + 0.5).floor() as usize;
            let centre = bin as f32 * (1.0 / (BINS - 1) as f32);
            if (bx[bin] - centre).abs() > (x - centre).abs() {
                bx[bin] = x;
                by[bin] = y;
            }
        }

        for (i, y) in by.iter().enumerate() {
            out[i * 4 + ch] = f32_to_f16(y.max(0.0));
        }
    }

    out
}
