//! The Inspector of an .environment / .lighting file: its settings grouped by what they do (sun, ambient light, sky, fog, exposure, ...)
//! instead of by the XML element they sit in, each with a tooltip. The sections list every setting the game reads, not only the ones the file
//! holds: a setting the file lacks shows the value the viewport assumes (dim) and is written into the file when it is edited. Attributes of
//! these elements that no section lists follow under "Other attributes"; cloud sprites, scene nodes and the like stay in the Properties tree.

use wce_editor_core::document::Document;
use wce_editor_core::inspector::Inspector;
use wce_foundation::log;
use wce_formats::env_value::{EnvValue, NumStyle, ValueKind, format_value, parse_value};
use wce_formats::xml_doc::{decode_entities, encode_entities};
use wce_foundation::math::Float3;
use wce_scene::environment::EnvironmentParams;

use crate::xml_file_document::{Op, XmlFileDocument, edit_array};

/// How a setting is edited.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Bool,

    /// A float, or an integer the file may write either way.
    Number,

    /// A whole number (a count, an index).
    Int,

    /// `rgb(r,g,b)`.
    Colour,

    /// `argb(r,g,b,a)` (the files list r, g, b, a).
    ColourA,

    /// `variance(a,b)`.
    Pair,

    Text,

    /// Three numbers in three attributes named `<attr><suffix>`: one row of three boxes.
    Trio([&'static str; 3]),
}

/// One setting of a section.
#[derive(Clone, Copy, Debug)]
pub struct Field {
    /// The element it sits in.
    pub elem: &'static str,

    /// The attribute (the common start of the three attributes of a `Trio`).
    pub attr: &'static str,
    pub label: &'static str,
    pub kind: Kind,
    pub tip: &'static str,
}

/// A group of settings with one effect.
#[derive(Debug)]
pub struct Section {
    pub title: &'static str,
    pub fields: &'static [Field],

    /// One block per element of the fields' element (a file holds several cloud planes) instead of one for the first.
    pub each_element: bool,
}

const fn f(elem: &'static str, attr: &'static str, label: &'static str, kind: Kind, tip: &'static str) -> Field {
    Field { elem, attr, label, kind, tip }
}

const RGB: Kind = Kind::Trio(["_r", "_g", "_b"]);
const RGB_LONG: Kind = Kind::Trio(["_red", "_green", "_blue"]);

pub static SECTIONS: &[Section] = &[
    Section {
        title: "Sun",
        each_element: false,
        fields: &[
            f("LIGHTING", "sun_direction_azimuth", "azimuth (\u{b0})", Kind::Number, "Where the sun is, in degrees turned from the map's +Z axis towards +X. Shadows fall the opposite way."),
            f("LIGHTING", "sun_direction_elevation", "elevation (\u{b0})", Kind::Number, "Height of the sun above the horizon in degrees: 0 on the horizon, 90 straight overhead."),
            f("LIGHTING", "sun_colour", "sun_colour", Kind::Colour, "Colour of the direct sunlight. Stored as seen on screen; the game converts it to linear light."),
            f("LIGHTING", "sun_colour_scale", "sun_colour_scale", Kind::Number, "Brightness of the sunlight (HDR multiplier of sun_colour). Also scales the sky when cubemap_sun_colour_scale is 0."),
            f("SUN_MESH", "sun_texture", "sun_texture", Kind::Text, "Texture of the visible sun disc (a .dds in the game's textures)."),
            f("SUN_MESH", "sun_scale", "sun_scale", Kind::Number, "Size of the visible sun disc."),
            f("SUN_MESH", "sun_flare", "sun_flare", Kind::Number, "Strength of the sun's lens flare. Needs 'Lens flare' on in the Settings menu."),
        ],
    },
    Section {
        title: "Ambient light",
        each_element: false,
        fields: &[
            f("LIGHTING", "use_custom_ambient_cube_colours", "custom ambient cube", Kind::Bool, "On: the six ambient cube colours below are the ambient light. Off: they are not used."),
            f("LIGHTING", "ambient_scale", "ambient_scale", Kind::Number, "Overall strength of the ambient light."),
            f("LIGHTING", "ambient_cube_hdr_scaler", "ambient_cube_hdr_scaler", Kind::Number, "Multiplier that lifts the ambient cube colours into the HDR range."),
            f("LIGHTING", "ambient_cube_top", "top", Kind::Colour, "Ambient light reaching surfaces that face up (the sky's light)."),
            f("LIGHTING", "ambient_cube_bottom", "bottom", Kind::Colour, "Ambient light reaching surfaces that face down (light bounced from the ground)."),
            f("LIGHTING", "ambient_cube_left", "left", Kind::Colour, "Ambient light reaching surfaces that face left."),
            f("LIGHTING", "ambient_cube_right", "right", Kind::Colour, "Ambient light reaching surfaces that face right."),
            f("LIGHTING", "ambient_cube_front", "front", Kind::Colour, "Ambient light reaching surfaces that face front."),
            f("LIGHTING", "ambient_cube_back", "back", Kind::Colour, "Ambient light reaching surfaces that face back."),
        ],
    },
    Section {
        title: "Sky",
        each_element: false,
        fields: &[
            f("LIGHTING", "cubemap_sun_colour_scale", "cubemap_sun_colour_scale", Kind::Number, "Sun brightness used for the sky and the reflections (the sky is scaled by at most 5 of it). 0: sun_colour_scale is used instead."),
            f("SKYGEN", "sky_colour", "sky_colour", Kind::Colour, "Colour of the open sky overhead."),
            f("SKYGEN", "sky_colour_scale", "sky_colour_scale", Kind::Number, "Brightness of sky_colour."),
            f("SKYGEN", "horizon_colour", "horizon_colour", Kind::Colour, "Colour of the sky near the horizon, on the sun's side."),
            f("SKYGEN", "horizon_colour_scale", "horizon_colour_scale", Kind::Number, "Brightness of horizon_colour."),
            f("SKYGEN", "horizon_opposite_colour", "opposite_colour", Kind::Colour, "Colour of the sky near the horizon, away from the sun."),
            f("SKYGEN", "horizon_opposite_colour_scale", "opposite_colour_scale", Kind::Number, "Brightness of horizon_opposite_colour."),
            f("SKYGEN", "horizon_decay", "horizon_decay", Kind::Number, "How fast the horizon colour fades going up into the sky."),
            f("SKYGEN", "circumsolar_colour", "circumsolar_colour", Kind::Colour, "Colour of the glow around the sun."),
            f("SKYGEN", "circumsolar_colour_scale", "circumsolar_colour_scale", Kind::Number, "Brightness of the glow around the sun."),
            f("SKYGEN", "circumsolar_decay", "circumsolar_decay", Kind::Number, "How fast the glow fades with distance from the sun."),
            f("SKYGEN", "backscatter_colour", "backscatter_colour", Kind::Colour, "Tint of the light the sky scatters back, away from the sun."),
            f("SKYGEN", "backscatter_colour_scale", "backscatter_colour_scale", Kind::Number, "Brightness of backscatter_colour; 0 turns it off."),
            f("SKYGEN", "backscatter_decay", "backscatter_decay", Kind::Number, "How fast the backscatter fades across the sky."),
        ],
    },
    Section {
        title: "Fog",
        each_element: false,
        fields: &[
            f("FOG", "volume_fog_colour", "volume_fog_colour", Kind::ColourA, "Colour of the fog (r, g, b) and its alpha."),
            f("FOG", "fog_colour_blend", "fog_colour_blend", Kind::Number, "How much of the fog colour is mixed into the fog; usually 0 to 1."),
            f("FOG", "fog_distance_start", "fog_distance_start", Kind::Number, "Where the distance fog starts to build up."),
            f("FOG", "fog_distance_strength", "fog_distance_strength", Kind::Number, "How strong the distance fog gets; 0 is none."),
            f("FOG", "fog_distance_scale", "fog_distance_scale", Kind::Number, "Stretches or shrinks the distance over which the fog builds up."),
            f("FOG", "fog_height_top", "fog_height_top", Kind::Number, "Top of the height-fog layer: the fog is thickest below it."),
            f("FOG", "fog_height_strength", "fog_height_strength", Kind::Number, "How strong the height fog is; 0 is none."),
            f("FOG", "terrain_relative_height_fog", "terrain_relative_height_fog", Kind::Bool, "On: the height fog is measured from the terrain under each point. Off: from a fixed level."),
            f("FOG", "fog_clear_distance", "fog_clear_distance", Kind::Number, "Fog clear distance. Most files use 10000, far beyond any battle map, so it has no visible effect."),
            f("FOG", "fog_precipitation_distance_modifier", "precipitation distance", Kind::Number, "How much rain or snow changes the fog distance; 0 means not at all."),
        ],
    },
    Section {
        title: "Exposure and tone mapping",
        each_element: false,
        fields: &[
            f("HDR", "auto_exposure_speed", "auto_exposure_speed", Kind::Number, "How fast the eye adapts to a brighter or darker view; larger is faster."),
            f("HDR", "absolute_black_point", "absolute_black_point", Kind::Number, "The darkest level the exposure may ever treat as black."),
            f("HDR", "max_black_point", "max_black_point", Kind::Number, "The highest level the black point may rise to as the exposure adapts."),
            f("HDR", "min_white_point", "min_white_point", Kind::Number, "The lowest level the white point may fall to as the exposure adapts."),
            f("HDR", "absolute_white_point", "absolute_white_point", Kind::Number, "The brightest level the exposure may ever treat as white."),
            f("HDR", "lock_rgb_relative_tones", "lock_rgb_relative_tones", Kind::Bool, "Tone curve option: keeps the relative tones of the red, green and blue channels locked together."),
            f("HDR", "low_tones_bias", "low_tones_bias", RGB_LONG, "Shapes the tone curve of the dark tones, per colour channel (red, green, blue)."),
            f("HDR", "high_tones_bias", "high_tones_bias", RGB_LONG, "Shapes the tone curve of the bright tones, per colour channel (red, green, blue)."),
        ],
    },
    Section {
        title: "Colour grading",
        each_element: false,
        fields: &[
            f("POST_PROCESSING", "overlay", "overlay", Kind::Colour, "Tint the whole picture is multiplied by; white leaves it unchanged."),
            f("POST_PROCESSING", "brightness", "brightness", RGB, "Brightness per colour channel (red, green, blue); 1 leaves it unchanged."),
            f("POST_PROCESSING", "contrast", "contrast", RGB, "Contrast per colour channel (red, green, blue); 1 leaves it unchanged."),
            f("POST_PROCESSING", "saturation", "saturation", RGB, "Colour saturation per channel (red, green, blue); 1 leaves it unchanged, 0 is grey."),
            f("POST_PROCESSING", "hue", "hue", Kind::Number, "Rotates the hues of the whole picture; 0 leaves them unchanged."),
        ],
    },
    Section {
        title: "Depth of field",
        each_element: false,
        fields: &[
            f("DEPTH_OF_FIELD", "use_DOF", "use_DOF", Kind::Bool, "Blurs what is out of focus. The game only does it when 'Depth of field' is on in its graphics options (Settings menu)."),
            f("DEPTH_OF_FIELD", "DOF_focus", "DOF_focus", Kind::Pair, "Focus distance (first) and focal length (second)."),
            f("DEPTH_OF_FIELD", "DOF_blur_size", "DOF_blur_size", Kind::Number, "How big the blur is outside the focus."),
        ],
    },
    Section {
        title: "Water",
        each_element: false,
        fields: &[
            f("LIGHTING", "deep_water_colour", "deep_water_colour", Kind::Colour, "Colour of deep water."),
            f("LIGHTING", "shallow_water_colour", "shallow_water_colour", Kind::Colour, "Colour of shallow water, where the bed shows through."),
            f("LIGHTING", "sea_bed_light_scatter", "sea_bed_light_scatter", RGB_LONG, "How much light the sea bed scatters back, per colour channel (red, green, blue)."),
            f("LIGHTING", "refraction_light_scatter", "refraction_light_scatter", Kind::Number, "How much the light is scattered as it passes through the water."),
        ],
    },
    Section {
        title: "Wind and snow",
        each_element: false,
        fields: &[
            f("WEATHER", "definition_name", "definition_name", Kind::Text, "The wind and precipitation definition of weather_definitions.xml this environment uses."),
            f("WEATHER", "wind_dir_random", "wind_dir_random", Kind::Bool, "On: the wind blows from a random direction each battle. Off: from wind_dir_degrees."),
            f("WEATHER", "wind_dir_degrees", "wind_dir_degrees", Kind::Number, "Wind direction in degrees, used when wind_dir_random is off."),
            f("WEATHER", "snowing_intensity", "snowing_intensity", Kind::Number, "Above 0 the terrain, trees and buildings are covered in snow; 0 is none (most files)."),
        ],
    },
    Section {
        title: "Precipitation node",
        each_element: false,
        fields: &[
            f("WEATHER_SCENE_NODE", "Enabled", "Enabled", Kind::Bool, "Whether the weather node is active."),
            f("WEATHER_SCENE_NODE", "weather_type", "weather_type", Kind::Text, "The kind of weather the node is (WT_...)."),
            f("WEATHER_SCENE_NODE", "precipitation_land_height", "precipitation_land_height", Kind::Number, "Height above the land where precipitation starts."),
            f("WEATHER_SCENE_NODE", "precipitation_sea_height", "precipitation_sea_height", Kind::Number, "Height above the sea where precipitation starts."),
            f("WEATHER_SCENE_NODE", "lightning_strike_interval", "lightning_strike_interval", Kind::Number, "Time between lightning strikes; 0 is none."),
            f("WEATHER_SCENE_NODE", "lightning_sheet_interval", "lightning_sheet_interval", Kind::Number, "Time between sheet lightning flashes in the clouds; 0 is none."),
            f("WEATHER_SCENE_NODE", "lightning_light_scale", "lightning_light_scale", Kind::Number, "How much light a lightning flash adds to the scene."),
            f("WEATHER_SCENE_NODE", "lightning_exposure_scale", "lightning_exposure_scale", Kind::Number, "How much a lightning flash raises the exposure."),
        ],
    },
    Section {
        title: "Cloud layer",
        each_element: true,
        fields: &[
            f("CLOUD_PLANE_MESH", "cloudset", "cloudset", Kind::Text, "The set of cloud sprites this layer draws from."),
            f("CLOUD_PLANE_MESH", "cloud_density", "cloud_density", Kind::Number, "How many clouds the layer holds."),
            f("CLOUD_PLANE_MESH", "cloud_velocity", "cloud_velocity", Kind::Number, "How fast the layer drifts across the sky."),
            f("CLOUD_PLANE_MESH", "cloud_rotation", "cloud_rotation", Kind::Number, "How much the clouds are turned."),
            f("CLOUD_PLANE_MESH", "height_scale", "height_scale", Kind::Number, "Scales the height of the cloud layer."),
            f("CLOUD_PLANE_MESH", "cloud_height_variation", "cloud_height_variation", Kind::Number, "How much the height of the clouds varies."),
            f("CLOUD_PLANE_MESH", "flatten_factor", "flatten_factor", Kind::Number, "How flat the layer is: the dome it forms is pressed down by this."),
            f("CLOUD_PLANE_MESH", "grid_size", "grid_size", Kind::Int, "Resolution of the layer's mesh, in cells along a side."),
            f("CLOUD_PLANE_MESH", "horizon_alpha_start", "horizon_alpha_start", Kind::Number, "Where the clouds start to fade out towards the horizon."),
            f("CLOUD_PLANE_MESH", "horizon_alpha_end", "horizon_alpha_end", Kind::Number, "Where the clouds have faded out completely at the horizon."),
            f("CLOUD_PLANE_MESH", "sun_reflected_colour", "sun_reflected_colour", Kind::Colour, "Colour of the sunlight the clouds reflect."),
            f("CLOUD_PLANE_MESH", "sun_transmitted_colour", "sun_transmitted_colour", Kind::Colour, "Colour of the sunlight that shines through the clouds."),
            f("CLOUD_PLANE_MESH", "sun_transmit_falloff", "sun_transmit_falloff", Kind::Number, "How fast the light shining through fades away from the sun."),
            f("CLOUD_PLANE_MESH", "cloud_shadow_colour", "cloud_shadow_colour", Kind::Colour, "Colour of the shaded side of the clouds."),
            f("CLOUD_PLANE_MESH", "cloud_shadow_falloff", "cloud_shadow_falloff", Kind::Number, "How fast the shading of the clouds fades."),
            f("CLOUD_PLANE_MESH", "cloud_shadow_gradient", "cloud_shadow_gradient", Kind::Number, "How the shading of the clouds grades from light to dark."),
        ],
    },
];

/// Attributes the battle game does not read (so editing them changes nothing there), with why.
const IGNORED: [(&str, &str, &str); 4] = [
    ("HDR", "low_tones_bias", "Not read by the battle game: it reads low_tones_bias_red, _green and _blue (Exposure and tone mapping)."),
    ("HDR", "high_tones_bias", "Not read by the battle game: it reads high_tones_bias_red, _green and _blue (Exposure and tone mapping)."),
    ("HDR", "high_pass_threshold", "Not read by the battle game's HDR settings."),
    ("LIGHTING", "sun_direction", "Older form of the sun direction: the sun's azimuth and elevation (Sun) are what the viewport reads."),
];

/// Elements whose attributes no section lists are shown under "Other attributes" when they are one of these.
const SECTION_ELEMENTS: [&str; 10] =
    ["LIGHTING", "SKYGEN", "SUN_MESH", "FOG", "HDR", "POST_PROCESSING", "DEPTH_OF_FIELD", "WEATHER", "WEATHER_SCENE_NODE", "CLOUD_PLANE_MESH"];

fn num(x: f32) -> EnvValue {
    EnvValue { kind: ValueKind::Float, f: vec![x], ..Default::default() }
}

fn rgb(c: Float3) -> EnvValue {
    EnvValue { kind: ValueKind::Rgb, f: vec![c.x, c.y, c.z], ..Default::default() }
}

/// What the viewport assumes for a setting the file lacks (`EnvironmentParams::default`), as a value of the setting's kind; a setting the
/// viewport does not read starts at zero, white or empty.
fn default_of(p: &EnvironmentParams, f: &Field, component: usize) -> EnvValue {
    let a = f.attr;
    let trio = |t: [f32; 3]| num(t[component]);
    let v = |c: Float3| [c.x, c.y, c.z][component];
    match (f.elem, a) {
        ("LIGHTING", "sun_direction_azimuth") => num(p.sun_azimuth),
        ("LIGHTING", "sun_direction_elevation") => num(p.sun_elevation),
        ("LIGHTING", "sun_colour") => rgb(p.sun_colour),
        ("LIGHTING", "sun_colour_scale") => num(p.sun_colour_scale),
        ("LIGHTING", "use_custom_ambient_cube_colours") => EnvValue { kind: ValueKind::Bool, b: p.use_custom_ambient, ..Default::default() },
        ("LIGHTING", "ambient_scale") => num(p.ambient_scale),
        ("LIGHTING", "ambient_cube_hdr_scaler") => num(p.ambient_hdr_scaler),
        ("LIGHTING", "ambient_cube_top") => rgb(p.ambient_top),
        ("LIGHTING", "ambient_cube_bottom") => rgb(p.ambient_bottom),
        ("LIGHTING", "ambient_cube_left") => rgb(p.ambient_left),
        ("LIGHTING", "ambient_cube_right") => rgb(p.ambient_right),
        ("LIGHTING", "ambient_cube_front") => rgb(p.ambient_front),
        ("LIGHTING", "ambient_cube_back") => rgb(p.ambient_back),
        ("LIGHTING", "cubemap_sun_colour_scale") => num(p.cubemap_sun_colour_scale),
        ("LIGHTING", "deep_water_colour") => rgb(p.deep_water),
        ("LIGHTING", "shallow_water_colour") => rgb(p.shallow_water),
        ("LIGHTING", "sea_bed_light_scatter") => num(v(p.sea_bed_scatter)),
        ("LIGHTING", "refraction_light_scatter") => num(p.refraction_scatter),
        ("SUN_MESH", "sun_texture") => EnvValue { kind: ValueKind::String, s: p.sun_texture.clone(), ..Default::default() },
        ("SUN_MESH", "sun_scale") => num(p.sun_scale),
        ("SUN_MESH", "sun_flare") => num(p.sun_flare),
        ("SKYGEN", "sky_colour") => rgb(p.sky_colour),
        ("SKYGEN", "sky_colour_scale") => num(p.sky_colour_scale),
        ("SKYGEN", "horizon_colour") => rgb(p.horizon_colour),
        ("SKYGEN", "horizon_colour_scale") => num(p.horizon_colour_scale),
        ("SKYGEN", "horizon_opposite_colour") => rgb(p.horizon_opposite_colour),
        ("SKYGEN", "horizon_opposite_colour_scale") => num(p.horizon_opposite_colour_scale),
        ("SKYGEN", "horizon_decay") => num(p.horizon_decay),
        ("SKYGEN", "circumsolar_colour") => rgb(p.circumsolar_colour),
        ("SKYGEN", "circumsolar_colour_scale") => num(p.circumsolar_colour_scale),
        ("SKYGEN", "circumsolar_decay") => num(p.circumsolar_decay),
        ("SKYGEN", "backscatter_colour") => rgb(p.backscatter_colour),
        ("SKYGEN", "backscatter_colour_scale") => num(p.backscatter_colour_scale),
        ("SKYGEN", "backscatter_decay") => num(p.backscatter_decay),
        ("FOG", "volume_fog_colour") => EnvValue { kind: ValueKind::Argb, f: p.volume_fog_colour.to_vec(), ..Default::default() },
        ("FOG", "fog_colour_blend") => num(p.fog_colour_blend),
        ("FOG", "fog_distance_start") => num(p.fog_distance_start),
        ("FOG", "fog_distance_strength") => num(p.fog_distance_strength),
        ("FOG", "fog_distance_scale") => num(p.fog_distance_scale),
        ("FOG", "fog_height_top") => num(p.fog_height_top),
        ("FOG", "fog_height_strength") => num(p.fog_height_strength),
        ("FOG", "terrain_relative_height_fog") => EnvValue { kind: ValueKind::Bool, b: p.terrain_relative_height_fog, ..Default::default() },
        ("FOG", "fog_clear_distance") => num(p.fog_clear_distance),
        ("FOG", "fog_precipitation_distance_modifier") => num(p.fog_precipitation_distance_modifier),
        ("HDR", "auto_exposure_speed") => num(p.auto_exposure_speed),
        ("HDR", "absolute_black_point") => num(p.absolute_black_point),
        ("HDR", "max_black_point") => num(p.max_black_point),
        ("HDR", "min_white_point") => num(p.min_white_point),
        ("HDR", "absolute_white_point") => num(p.absolute_white_point),
        ("HDR", "low_tones_bias") => trio(p.low_tones_bias),
        ("HDR", "high_tones_bias") => trio(p.high_tones_bias),
        ("POST_PROCESSING", "overlay") => rgb(p.overlay),
        ("POST_PROCESSING", "brightness") => num(v(p.brightness)),
        ("POST_PROCESSING", "contrast") => num(v(p.contrast)),
        ("POST_PROCESSING", "saturation") => num(v(p.saturation)),
        ("POST_PROCESSING", "hue") => num(p.hue),
        ("DEPTH_OF_FIELD", "use_DOF") => EnvValue { kind: ValueKind::Bool, b: p.use_dof, ..Default::default() },
        ("DEPTH_OF_FIELD", "DOF_focus") => EnvValue { kind: ValueKind::Variance, f: p.dof_focus.to_vec(), ..Default::default() },
        ("DEPTH_OF_FIELD", "DOF_blur_size") => num(p.dof_blur_size),
        ("WEATHER", "definition_name") => EnvValue { kind: ValueKind::String, s: p.weather_definition.clone(), ..Default::default() },
        ("WEATHER", "wind_dir_random") => EnvValue { kind: ValueKind::Bool, b: p.wind_dir_random, ..Default::default() },
        ("WEATHER", "wind_dir_degrees") => num(p.wind_dir_degrees),
        ("WEATHER", "snowing_intensity") => num(p.snowing_intensity),
        _ => match f.kind {
            Kind::Bool => EnvValue { kind: ValueKind::Bool, ..Default::default() },
            Kind::Colour => rgb(Float3::new(1.0, 1.0, 1.0)),
            Kind::ColourA => EnvValue { kind: ValueKind::Argb, f: vec![1.0; 4], ..Default::default() },
            Kind::Pair => EnvValue { kind: ValueKind::Variance, f: vec![0.0; 2], ..Default::default() },
            Kind::Text => EnvValue { kind: ValueKind::String, ..Default::default() },
            Kind::Number | Kind::Int | Kind::Trio(_) => num(0.0),
        },
    }
}

/// True when a value read from the file is of the kind the setting is edited as.
fn fits(kind: Kind, v: &EnvValue) -> bool {
    match kind {
        Kind::Bool => v.kind == ValueKind::Bool,
        Kind::Number | Kind::Int | Kind::Trio(_) => matches!(v.kind, ValueKind::Int | ValueKind::Float),
        Kind::Colour => v.kind == ValueKind::Rgb,
        Kind::ColourA => v.kind == ValueKind::Argb,
        Kind::Pair => v.kind == ValueKind::Variance,
        Kind::Text => true,
    }
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

/// The float widgets show a number the way the file writes it, not rounded to 3 decimals.
const FLOAT_FORMAT: &str = "%.6g";

impl XmlFileDocument {
    /// The value of an attribute as the file holds it (`None`: no such attribute).
    fn attr_value(&self, elem: usize, attr: &str) -> Option<(usize, EnvValue)> {
        let a = self.xml().find_attr(elem, attr)?;
        Some((a, parse_value(&text(&decode_entities(self.xml().attr_raw(elem, a))))))
    }

    /// Writes a value into an attribute; an attribute the element lacks is added.
    fn write_value(&mut self, elem: usize, attr: &str, at: Option<usize>, v: &EnvValue, gesture: u64) {
        let out = format_value(v);
        match at {
            Some(a) => {
                let quote = self.xml().elements()[elem].attrs[a].quote;
                self.set_attr(elem, a, encode_entities(out.as_bytes(), quote), gesture);
            }

            None => {
                if let Err(err) = self.structural(Op::AddAttr, elem, 0, attr, &out) {
                    log::warn(format!("cannot add @{attr}: {err}"));
                }
            }
        }
    }

    /// One row of a section for the element `elem`.
    fn environment_row(&mut self, ins: &mut dyn Inspector, defaults: &EnvironmentParams, elem: usize, f: &Field, field: &str, gesture: u64) {
        let hint = |present: bool, elem_name: &str| {
            if present { f.tip.to_owned() } else { format!("{}\n\nNot in this file yet: shown at the value the viewport assumes. Editing it adds @{} to <{elem_name}>.", f.tip, f.attr) }
        };

        if let Kind::Trio(suffixes) = f.kind {
            let names = suffixes.map(|s| format!("{}{s}", f.attr));
            let found = names.each_ref().map(|n| self.attr_value(elem, n));
            let present = found.iter().any(Option::is_some);
            let mut v = [0.0f32; 3];
            for (i, slot) in v.iter_mut().enumerate() {
                *slot = match &found[i] {
                    Some((_, x)) if fits(f.kind, x) => x.f[0],
                    _ => default_of(defaults, f, i).f[0],
                };
            }

            let before = v;
            if !present {
                ins.dim_next();
            }

            ins.hint(&hint(present, f.elem));
            ins.format_next(FLOAT_FORMAT);
            if ins.edit_float3(field, f.label, &mut v, 0.01) {
                for i in 0..3 {
                    if v[i] == before[i] {
                        continue;
                    }

                    let was_int = matches!(&found[i], Some((_, x)) if x.kind == ValueKind::Int);
                    let style = found[i].as_ref().map(|(_, x)| x.style).unwrap_or_default();
                    self.write_value(elem, &names[i], found[i].as_ref().map(|(a, _)| *a), &number(v[i], was_int, style), gesture ^ ((i as u64 + 1) << 56));
                }
            }

            return;
        }

        let found = self.attr_value(elem, f.attr);
        let present = found.is_some();
        let at = found.as_ref().map(|(a, _)| *a);
        let mut v = match found {
            Some((_, x)) if f.kind != Kind::Text && fits(f.kind, &x) => x,
            Some((_, x)) => EnvValue { kind: ValueKind::String, s: x.s, ..Default::default() },
            None => default_of(defaults, f, 0),
        };

        // A value of another kind than the setting's (hand-written text where a number belongs) is edited as text.
        let kind = if v.kind == ValueKind::String { Kind::Text } else { f.kind };
        if !present {
            ins.dim_next();
        }

        ins.hint(&hint(present, f.elem));
        let was_int = v.kind == ValueKind::Int;
        let style = v.style;
        let changed = match kind {
            Kind::Bool => ins.edit_bool(field, f.label, &mut v.b),
            Kind::Int if was_int => ins.edit_int(field, f.label, &mut v.i, 0.2),
            Kind::Number | Kind::Int | Kind::Trio(_) => {
                let mut x = v.f.first().copied().unwrap_or(0.0);
                ins.format_next(FLOAT_FORMAT);
                let speed = (x.abs() * 0.005).max(0.001);
                let changed = ins.edit_float(field, f.label, &mut x, speed, -1e9, 1e9);
                if changed {
                    v = number(x, was_int, style);
                }

                changed
            }

            Kind::Colour => edit_array::<3>(&mut v.f, |c| ins.edit_color3(field, f.label, c)),
            Kind::ColourA => edit_array::<4>(&mut v.f, |c| ins.edit_color4(field, f.label, c)),
            Kind::Pair => {
                ins.format_next(FLOAT_FORMAT);
                edit_array::<2>(&mut v.f, |c| ins.edit_float2(field, f.label, c, 0.01))
            }

            Kind::Text => ins.edit_string(field, f.label, &mut v.s),
        };

        if changed {
            self.write_value(elem, f.attr, at, &v, gesture);
        }
    }

    /// The Inspector of an environment or lighting file: every section, then the attributes no section lists.
    pub fn inspect_environment(&mut self, ins: &mut dyn Inspector) {
        let id = self.id();
        ins.begin(id, 0);
        let defaults = EnvironmentParams::default();
        let gesture_base = (u64::from(id) << 40) ^ 0x1d;
        let mut n = 0u64;
        for s in SECTIONS {
            let element = s.fields[0].elem;
            let elems = self.elements_named(element);
            if elems.is_empty() {
                ins.section(s.title);
                ins.text("missing", &format!("no <{element}>"), "this file has none");
                continue;
            }

            for (k, &e) in elems.iter().enumerate().take(if s.each_element { usize::MAX } else { 1 }) {
                let name = self.xml().get(e, "name").filter(|_| s.each_element);
                ins.section(&name.map_or_else(|| s.title.to_owned(), |name| format!("{} '{name}'", s.title)));
                let mut missing = vec![];
                for fld in s.fields {
                    // A section may span elements (the Sun: LIGHTING and SUN_MESH).
                    let target = if fld.elem == element { Some(e) } else { self.elements_named(fld.elem).first().copied() };
                    let Some(target) = target else {
                        if !missing.contains(&fld.elem) {
                            missing.push(fld.elem);
                        }

                        continue;
                    };

                    n += 1;
                    let field = format!("{}#{target}.{}", fld.elem, fld.attr);
                    self.environment_row(ins, &defaults, target, fld, &field, gesture_base ^ (n << 20) ^ ((k as u64) << 8));
                    if ins.gesture_ended() {
                        self.history_mut().break_merge();
                    }
                }

                for elem in missing {
                    ins.text("missing", &format!("no <{elem}>"), "this file has none");
                }
            }
        }

        self.inspect_other_attributes(ins, &mut n, gesture_base);
    }

    /// The elements of a name, in file order.
    fn elements_named(&self, name: &str) -> Vec<usize> {
        self.xml().elements().iter().enumerate().filter(|(_, e)| e.name == name).map(|(i, _)| i).collect()
    }

    /// The attributes of the section elements that no section lists, edited as they are written.
    fn inspect_other_attributes(&mut self, ins: &mut dyn Inspector, n: &mut u64, gesture_base: u64) {
        let listed = |elem: &str, attr: &str| {
            attr == "name"
                || SECTIONS.iter().flat_map(|s| s.fields).any(|f| {
                    f.elem == elem
                        && match f.kind {
                            Kind::Trio(suffixes) => suffixes.iter().any(|s| attr == format!("{}{s}", f.attr)),
                            _ => f.attr == attr,
                        }
                })
        };

        let mut rows = vec![];
        for (e, el) in self.xml().elements().iter().enumerate() {
            if !SECTION_ELEMENTS.contains(&el.name.as_str()) {
                continue;
            }

            for attr in &el.attrs {
                if !listed(&el.name, &attr.name) {
                    rows.push((e, el.name.clone(), attr.name.clone()));
                }
            }
        }

        if rows.is_empty() {
            return;
        }

        ins.section("Other attributes");
        for (e, elem, attr) in rows {
            let Some((_, mut v)) = self.attr_value(e, &attr) else { continue };
            *n += 1;
            let gesture = gesture_base ^ (*n << 20);
            let field = format!("other.{elem}#{e}.{attr}");
            let label = format!("{elem}.{attr}");
            let note = IGNORED.iter().find(|(el, at, _)| *el == elem && *at == attr).map_or("Not one of the settings the game reads for an effect.", |i| i.2);
            ins.hint(note);
            let was_int = v.kind == ValueKind::Int;
            let changed = match v.kind {
                ValueKind::Bool => ins.edit_bool(&field, &label, &mut v.b),
                ValueKind::Int | ValueKind::Float => {
                    let mut x = v.f.first().copied().unwrap_or(0.0);
                    ins.format_next(FLOAT_FORMAT);
                    let speed = (x.abs() * 0.005).max(0.001);
                    let changed = ins.edit_float(&field, &label, &mut x, speed, -1e9, 1e9);
                    if changed {
                        v = number(x, was_int, v.style);
                    }

                    changed
                }

                ValueKind::Rgb => edit_array::<3>(&mut v.f, |c| ins.edit_color3(&field, &label, c)),
                ValueKind::Argb => edit_array::<4>(&mut v.f, |c| ins.edit_color4(&field, &label, c)),
                ValueKind::Variance => edit_array::<2>(&mut v.f, |c| ins.edit_float2(&field, &label, c, 0.01)),
                ValueKind::Euler => edit_array::<3>(&mut v.f, |c| ins.edit_float3(&field, &label, c, 0.01)),
                ValueKind::String => ins.edit_string(&field, &label, &mut v.s),
            };

            if changed && let Some(at) = self.xml().find_attr(e, &attr) {
                self.write_value(e, &attr, Some(at), &v, gesture);
            }

            if ins.gesture_ended() {
                self.history_mut().break_merge();
            }
        }
    }
}

/// A number written back: a whole number stays an integer when the file wrote it as one.
fn number(x: f32, was_int: bool, style: NumStyle) -> EnvValue {
    if was_int && x == x.round() && x.abs() < 1e15 {
        EnvValue { kind: ValueKind::Int, i: x as i64, f: vec![x], ..Default::default() }
    } else {
        EnvValue { style, ..num(x) }
    }
}
