//! The games' graphics options: the `gfx_*` lines of a game's `scripts/preferences.script.txt` that change what the battle renderer draws,
//! with their levels as the game's options screen names them, and the game's presets (docs/re/graphics_options.md). The editor follows
//! the game's file, or its own copy of the options per game (`settings::GraphicsSettings`); the renderer applies the options it reproduces
//! ([`GfxOption::applied`]) and draws the others at the game's highest level.

use std::collections::BTreeMap;

/// How an option's value is written in the preferences file and shown.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum GfxKind {
    /// `true` / `false`.
    Toggle,

    /// `0` / `1` (an on / off option the game stores as a number, like `gfx_depth_of_field`).
    Switch,

    /// `0..n`, a name per level.
    Levels(&'static [&'static str]),

    /// A number in a range.
    Float { min: f32, max: f32 },
}

// The ranges are constants, never NaN.
impl Eq for GfxKind {}

/// A graphics option of a game.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GfxOption {
    /// The preferences key (`gfx_shadow_quality`).
    pub key: &'static str,

    /// The name on the game's options screen.
    pub label: &'static str,
    pub kind: GfxKind,

    /// The value of a preferences file without the line.
    pub default: &'static str,

    /// Whether the editor's renderer draws what the game does at each value; otherwise it draws the game's highest level (or the effect
    /// off, for the effects it does not draw).
    pub applied: bool,
}

/// A value of a preset.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PresetValue {
    Fixed(i32),

    /// From the graphics card's video memory ([`GraphicsCatalogue::texture_quality_for`]).
    ByVideoMemory,
}

/// A preset of the game's options screen: the options it sets (the others keep their values).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GfxPreset {
    pub name: &'static str,
    pub values: &'static [(&'static str, PresetValue)],
}

/// A game's graphics options and presets.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GraphicsCatalogue {
    pub options: &'static [GfxOption],
    pub presets: &'static [GfxPreset],

    /// The texture quality the presets set by video memory: the first level whose bound (MB) is at least the card's, else the last.
    pub texture_quality_by_memory: &'static [(u64, i32)],
}

impl GraphicsCatalogue {
    /// An option by key.
    pub fn option(&self, key: &str) -> Option<&'static GfxOption> {
        self.options.iter().find(|o| o.key == key)
    }

    /// The texture quality the presets give a card with this much video memory (bytes).
    pub fn texture_quality_for(&self, video_memory: u64) -> i32 {
        let mb = video_memory >> 20;
        self.texture_quality_by_memory.iter().find(|(bound, _)| mb <= *bound).or(self.texture_quality_by_memory.last()).map_or(0, |(_, q)| *q)
    }

    /// A preset's values as preferences text, for a card with this much video memory.
    pub fn preset_values(&self, preset: &GfxPreset, video_memory: u64) -> BTreeMap<String, String> {
        preset
            .values
            .iter()
            .filter_map(|(key, v)| {
                let option = self.option(key)?;
                let n = match v {
                    PresetValue::Fixed(n) => *n,
                    PresetValue::ByVideoMemory => self.texture_quality_for(video_memory),
                };
                Some(((*key).to_owned(), format_value(option.kind, n)))
            })
            .collect()
    }

    /// The preset these values match (every option it sets), if one does.
    pub fn matching_preset(&self, values: &BTreeMap<String, String>, video_memory: u64) -> Option<&'static GfxPreset> {
        self.presets.iter().find(|p| {
            self.preset_values(p, video_memory).iter().all(|(k, v)| values.get(k).is_some_and(|x| parse_number(x) == parse_number(v)))
        })
    }
}

/// An integer value as the preferences file writes it for an option's kind.
pub fn format_value(kind: GfxKind, n: i32) -> String {
    match kind {
        GfxKind::Toggle => (n != 0).to_string(),
        _ => n.to_string(),
    }
}

/// A preferences value as a number: `true` 1, `false` 0, else the number (0 when it isn't one).
pub fn parse_number(value: &str) -> f32 {
    match value.trim() {
        "true" => 1.0,
        "false" => 0.0,
        v => v.parse().unwrap_or(0.0),
    }
}

/// The `gfx_*` values of a preferences text (`key value;` lines; a value in quotes is kept as it is).
pub fn parse_preferences(text: &str) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    for line in text.lines() {
        let line = line.split('#').next().unwrap_or_default().trim();
        let Some((key, value)) = line.split_once(char::is_whitespace) else { continue };
        if key.starts_with("gfx_") {
            out.insert(key.to_owned(), value.trim().trim_end_matches(';').trim().to_owned());
        }
    }

    out
}

const LEVELS: &[&str] = &["Low", "Medium", "High", "Ultra"];
const LEVELS_ROME_2: &[&str] = &["Low", "Medium", "High", "Very High", "Ultra", "Extreme"];
const TEXTURE_LEVELS: &[&str] = &["Low", "Medium", "High", "Ultra"];
const AA: &[&str] = &["Off", "MLAA", "MSAA 2x", "MSAA 4x", "MSAA 8x"];
const FILTERING: &[&str] = &["Trilinear", "Anisotropic 2x", "Anisotropic 4x", "Anisotropic 8x", "Anisotropic 16x"];
const REFLECTIONS: &[&str] = &["Off", "Half resolution", "Full resolution"];
const ALPHA_BLEND: &[&str] = &["Alpha test", "Standard OIT", "Accelerated AOIT"];
/// Attila's and Thrones of Britannia's: the editor draws the standard OIT for both (AOIT orders its fragments with Intel's pixel
/// synchronisation, `g_IntelExt`).
const ALPHA_BLEND_OIT: &[&str] = &["Alpha test", "Standard OIT", "Accelerated AOIT (drawn as standard OIT)"];

const fn opt(key: &'static str, label: &'static str, kind: GfxKind, default: &'static str, applied: bool) -> GfxOption {
    GfxOption { key, label, kind, default, applied }
}

/// Attila's options that change the battle renderer (not the units', the fleets' or the window's).
const ATTILA_OPTIONS: &[GfxOption] = &[
    opt("gfx_aa", "Anti-aliasing", GfxKind::Levels(AA), "0", true),
    opt("gfx_texture_filtering", "Texture filtering", GfxKind::Levels(FILTERING), "3", true),
    opt("gfx_texture_quality", "Texture quality", GfxKind::Levels(TEXTURE_LEVELS), "3", false),
    opt("gfx_shadow_quality", "Shadows", GfxKind::Levels(LEVELS), "3", true),
    opt("gfx_water_quality", "Water", GfxKind::Levels(LEVELS), "3", true),
    opt("gfx_tree_quality", "Trees", GfxKind::Levels(LEVELS), "2", false),
    opt("gfx_grass_quality", "Grass", GfxKind::Levels(LEVELS), "2", false),
    opt("gfx_terrain_quality", "Terrain", GfxKind::Levels(LEVELS), "3", false),
    opt("gfx_sky_quality", "Sky", GfxKind::Levels(LEVELS), "3", true),
    opt("gfx_building_quality", "Building detail", GfxKind::Levels(LEVELS), "3", true),
    opt("gfx_unit_quality", "Unit detail", GfxKind::Levels(LEVELS), "3", true),
    opt("gfx_effects_quality", "Particle effects", GfxKind::Levels(LEVELS), "3", true),
    opt("gfx_screen_space_reflections", "Screen space reflections", GfxKind::Levels(REFLECTIONS), "2", true),
    opt("gfx_ssao", "SSAO", GfxKind::Toggle, "true", true),
    opt("gfx_depth_of_field", "Depth of field", GfxKind::Switch, "1", true),
    opt("gfx_distortion", "Distortion effects", GfxKind::Toggle, "true", true),
    opt("gfx_vignette", "Vignette", GfxKind::Toggle, "true", true),
    opt("gfx_alpha_blend", "Alpha blending", GfxKind::Levels(ALPHA_BLEND_OIT), "0", true),
    opt("gfx_tesselation", "Tessellation", GfxKind::Toggle, "false", true),
    opt("gfx_gamma_setting", "Gamma", GfxKind::Float { min: 1.0, max: 3.0 }, "2.2", true),
    opt("gfx_brightness_setting", "Brightness", GfxKind::Float { min: 0.5, max: 2.0 }, "1", true),
];

/// Thrones of Britannia's: Attila's and three effects of its own (docs/re/thrones_renderer.md).
const THRONES_OPTIONS: &[GfxOption] = &[
    opt("gfx_aa", "Anti-aliasing", GfxKind::Levels(AA), "0", true),
    opt("gfx_texture_filtering", "Texture filtering", GfxKind::Levels(FILTERING), "3", true),
    opt("gfx_texture_quality", "Texture quality", GfxKind::Levels(TEXTURE_LEVELS), "3", false),
    opt("gfx_shadow_quality", "Shadows", GfxKind::Levels(LEVELS), "3", true),
    opt("gfx_water_quality", "Water", GfxKind::Levels(LEVELS), "3", true),
    opt("gfx_tree_quality", "Trees", GfxKind::Levels(LEVELS), "2", false),
    opt("gfx_grass_quality", "Grass", GfxKind::Levels(LEVELS), "2", false),
    opt("gfx_terrain_quality", "Terrain", GfxKind::Levels(LEVELS), "3", false),
    opt("gfx_sky_quality", "Sky", GfxKind::Levels(LEVELS), "3", true),
    opt("gfx_building_quality", "Building detail", GfxKind::Levels(LEVELS), "3", true),
    opt("gfx_unit_quality", "Unit detail", GfxKind::Levels(LEVELS), "3", true),
    opt("gfx_effects_quality", "Particle effects", GfxKind::Levels(LEVELS), "3", true),
    opt("gfx_screen_space_reflections", "Screen space reflections", GfxKind::Levels(REFLECTIONS), "2", true),
    opt("gfx_ssao", "SSAO", GfxKind::Toggle, "true", true),
    opt("gfx_depth_of_field", "Depth of field", GfxKind::Switch, "1", true),
    opt("gfx_distortion", "Distortion effects", GfxKind::Toggle, "true", true),
    opt("gfx_vignette", "Vignette", GfxKind::Toggle, "true", true),
    opt("gfx_grain", "Film grain", GfxKind::Toggle, "false", true),
    opt("gfx_cloud_shadows", "Cloud shadows", GfxKind::Toggle, "false", true),
    opt("gfx_god_rays", "God rays", GfxKind::Toggle, "false", true),
    opt("gfx_alpha_blend", "Alpha blending", GfxKind::Levels(ALPHA_BLEND_OIT), "0", true),
    opt("gfx_tesselation", "Tessellation", GfxKind::Toggle, "false", true),
    opt("gfx_gamma_setting", "Gamma", GfxKind::Float { min: 1.0, max: 3.0 }, "2.2", true),
    opt("gfx_brightness_setting", "Brightness", GfxKind::Float { min: 0.5, max: 2.0 }, "1", true),
];

/// Rome 2's: six levels (to Extreme), no screen space reflections, and its effects of its own (lightning, the foliage hiding). Its
/// water quality (four techniques for six levels) and texture filtering are not mapped yet: the editor draws high water (the game's sea at
/// Extreme, from a capture) and 8x.
const ROME_2_OPTIONS: &[GfxOption] = &[
    opt("gfx_aa", "Anti-aliasing", GfxKind::Levels(AA), "0", false),
    opt("gfx_texture_filtering", "Texture filtering", GfxKind::Levels(FILTERING), "3", false),
    opt("gfx_texture_quality", "Texture quality", GfxKind::Levels(TEXTURE_LEVELS), "3", false),
    opt("gfx_shadow_quality", "Shadows", GfxKind::Levels(LEVELS_ROME_2), "4", false),
    opt("gfx_water_quality", "Water", GfxKind::Levels(LEVELS_ROME_2), "4", false),
    opt("gfx_tree_quality", "Trees", GfxKind::Levels(LEVELS_ROME_2), "4", false),
    opt("gfx_grass_quality", "Grass", GfxKind::Levels(LEVELS_ROME_2), "4", false),
    opt("gfx_terrain_quality", "Terrain", GfxKind::Levels(LEVELS_ROME_2), "4", false),
    opt("gfx_sky_quality", "Sky", GfxKind::Levels(LEVELS_ROME_2), "4", false),
    opt("gfx_building_quality", "Building detail", GfxKind::Levels(LEVELS_ROME_2), "4", false),
    opt("gfx_unit_quality", "Unit detail", GfxKind::Levels(LEVELS_ROME_2), "4", false),
    opt("gfx_effects_quality", "Particle effects", GfxKind::Levels(LEVELS_ROME_2), "4", false),
    opt("gfx_ssao", "SSAO", GfxKind::Toggle, "true", true),
    opt("gfx_depth_of_field", "Depth of field", GfxKind::Switch, "1", true),
    opt("gfx_distortion", "Distortion effects", GfxKind::Toggle, "true", true),
    opt("gfx_vignette", "Vignette", GfxKind::Toggle, "true", true),
    opt("gfx_grain", "Film grain", GfxKind::Toggle, "false", true),
    opt("gfx_cloud_shadows", "Cloud shadows", GfxKind::Toggle, "false", false),
    opt("gfx_god_rays", "God rays", GfxKind::Toggle, "false", false),
    opt("gfx_lightning", "Lightning", GfxKind::Toggle, "false", false),
    opt("gfx_hide_foliage", "Hide foliage", GfxKind::Toggle, "false", false),
    opt("gfx_alpha_blend", "Alpha blending", GfxKind::Levels(ALPHA_BLEND), "0", false),
    opt("gfx_tesselation", "Tessellation", GfxKind::Toggle, "false", false),
    opt("gfx_gamma_setting", "Gamma", GfxKind::Float { min: 1.0, max: 3.0 }, "2.2", true),
    opt("gfx_brightness_setting", "Brightness", GfxKind::Float { min: 0.5, max: 2.0 }, "1", true),
];

use PresetValue::{ByVideoMemory as Vram, Fixed as F};

/// The quality options a preset sets to its level.
const QUALITIES: [&str; 9] = [
    "gfx_shadow_quality", "gfx_water_quality", "gfx_tree_quality", "gfx_grass_quality", "gfx_terrain_quality", "gfx_sky_quality", "gfx_building_quality",
    "gfx_effects_quality", "gfx_unit_quality",
];

/// An Attila preset (fn 0x1175c2f0 fills them; docs/re/graphics_options.md): anti-aliasing, filtering, the level of every quality option,
/// SSAO, depth of field, distortion, reflections; the texture quality by video memory, the vignette off.
const fn attila_preset(aa: i32, filtering: i32, level: i32, ssao: i32, dof: i32, distortion: i32, reflections: i32) -> [(&'static str, PresetValue); 17] {
    [
        ("gfx_aa", F(aa)),
        ("gfx_texture_filtering", F(filtering)),
        ("gfx_texture_quality", Vram),
        (QUALITIES[0], F(level)),
        (QUALITIES[1], F(level)),
        (QUALITIES[2], F(level)),
        (QUALITIES[3], F(level)),
        (QUALITIES[4], F(level)),
        (QUALITIES[5], F(level)),
        (QUALITIES[6], F(level)),
        (QUALITIES[7], F(level)),
        (QUALITIES[8], F(level)),
        ("gfx_screen_space_reflections", F(reflections)),
        ("gfx_ssao", F(ssao)),
        ("gfx_depth_of_field", F(dof)),
        ("gfx_distortion", F(distortion)),
        ("gfx_vignette", F(0)),
    ]
}

/// A Thrones of Britannia preset (fn 0x117ce2f0): Attila's and its grain (always off), cloud shadows and god rays.
const fn thrones_preset(a: [(&'static str, PresetValue); 17], cloud_shadows: i32, god_rays: i32) -> [(&'static str, PresetValue); 20] {
    let mut out = [("", F(0)); 20];
    let mut i = 0;
    while i < 17 {
        out[i] = a[i];
        i += 1;
    }

    out[17] = ("gfx_grain", F(0));
    out[18] = ("gfx_cloud_shadows", F(cloud_shadows));
    out[19] = ("gfx_god_rays", F(god_rays));
    out
}

const ATTILA_LOW: [(&str, PresetValue); 17] = attila_preset(0, 0, 0, 0, 0, 0, 0);
const ATTILA_MEDIUM: [(&str, PresetValue); 17] = attila_preset(0, 0, 1, 0, 0, 1, 0);
const ATTILA_HIGH: [(&str, PresetValue); 17] = attila_preset(0, 0, 2, 0, 0, 1, 1);
const ATTILA_ULTRA: [(&str, PresetValue); 17] = attila_preset(1, 1, 3, 1, 0, 1, 2);
const ATTILA_EXTREME: [(&str, PresetValue); 17] = attila_preset(3, 2, 3, 1, 1, 1, 2);

const ATTILA_PRESETS: &[GfxPreset] = &[
    GfxPreset { name: "Low", values: &ATTILA_LOW },
    GfxPreset { name: "Medium", values: &ATTILA_MEDIUM },
    GfxPreset { name: "High", values: &ATTILA_HIGH },
    GfxPreset { name: "Ultra", values: &ATTILA_ULTRA },
    GfxPreset { name: "Extreme", values: &ATTILA_EXTREME },
];

const THRONES_LOW: [(&str, PresetValue); 20] = thrones_preset(ATTILA_LOW, 0, 0);
const THRONES_MEDIUM: [(&str, PresetValue); 20] = thrones_preset(ATTILA_MEDIUM, 1, 0);
const THRONES_HIGH: [(&str, PresetValue); 20] = thrones_preset(ATTILA_HIGH, 1, 1);
const THRONES_ULTRA: [(&str, PresetValue); 20] = thrones_preset(ATTILA_ULTRA, 1, 1);
const THRONES_EXTREME: [(&str, PresetValue); 20] = thrones_preset(ATTILA_EXTREME, 1, 1);

const THRONES_PRESETS: &[GfxPreset] = &[
    GfxPreset { name: "Low", values: &THRONES_LOW },
    GfxPreset { name: "Medium", values: &THRONES_MEDIUM },
    GfxPreset { name: "High", values: &THRONES_HIGH },
    GfxPreset { name: "Ultra", values: &THRONES_ULTRA },
    GfxPreset { name: "Extreme", values: &THRONES_EXTREME },
];

/// Attila's and Thrones of Britannia's presets' texture quality: by the card's video memory in MB (fn 0x1175c2f0: above 800, 1600, 2600).
const TEXTURE_BY_MEMORY: &[(u64, i32)] = &[(800, 0), (1600, 1), (2600, 2), (u64::MAX, 3)];

/// Attila's options and presets.
pub const ATTILA: GraphicsCatalogue = GraphicsCatalogue { options: ATTILA_OPTIONS, presets: ATTILA_PRESETS, texture_quality_by_memory: TEXTURE_BY_MEMORY };

/// Thrones of Britannia's.
pub const THRONES_OF_BRITANNIA: GraphicsCatalogue =
    GraphicsCatalogue { options: THRONES_OPTIONS, presets: THRONES_PRESETS, texture_quality_by_memory: TEXTURE_BY_MEMORY };

/// A Rome 2 preset (fn 0x1144c3b0; docs/re/graphics_options.md): anti-aliasing, filtering, the level of the quality options, the
/// shadows' level, SSAO, distortion, cloud shadows, god rays, the texture quality; depth of field, grain and the vignette off.
#[allow(clippy::too_many_arguments)]
const fn rome2_preset(
    aa: i32, filtering: i32, level: i32, shadows: i32, ssao: i32, distortion: i32, cloud_shadows: i32, god_rays: i32, texture: PresetValue,
) -> [(&'static str, PresetValue); 21] {
    [
        ("gfx_aa", F(aa)),
        ("gfx_texture_filtering", F(filtering)),
        ("gfx_texture_quality", texture),
        ("gfx_shadow_quality", F(shadows)),
        (QUALITIES[1], F(level)),
        (QUALITIES[2], F(level)),
        (QUALITIES[3], F(level)),
        (QUALITIES[4], F(level)),
        (QUALITIES[5], F(level)),
        (QUALITIES[6], F(level)),
        (QUALITIES[7], F(level)),
        (QUALITIES[8], F(level)),
        ("gfx_ssao", F(ssao)),
        ("gfx_depth_of_field", F(0)),
        ("gfx_distortion", F(distortion)),
        ("gfx_vignette", F(0)),
        ("gfx_grain", F(0)),
        ("gfx_cloud_shadows", F(cloud_shadows)),
        ("gfx_god_rays", F(god_rays)),
        ("gfx_hide_foliage", F(0)),
        ("gfx_alpha_blend", F(0)),
    ]
}

const ROME_2_LOW: [(&str, PresetValue); 21] = rome2_preset(0, 0, 0, 0, 0, 0, 0, 0, Vram);
const ROME_2_MEDIUM: [(&str, PresetValue); 21] = rome2_preset(0, 0, 1, 0, 0, 0, 1, 0, Vram);
const ROME_2_HIGH: [(&str, PresetValue); 21] = rome2_preset(0, 0, 2, 2, 0, 0, 1, 1, Vram);
const ROME_2_VERY_HIGH: [(&str, PresetValue); 21] = rome2_preset(0, 0, 3, 3, 0, 1, 1, 1, F(2));
const ROME_2_ULTRA: [(&str, PresetValue); 21] = rome2_preset(0, 0, 4, 4, 0, 1, 1, 1, Vram);
const ROME_2_EXTREME: [(&str, PresetValue); 21] = rome2_preset(3, 1, 5, 5, 1, 1, 1, 1, Vram);

const ROME_2_PRESETS: &[GfxPreset] = &[
    GfxPreset { name: "Low", values: &ROME_2_LOW },
    GfxPreset { name: "Medium", values: &ROME_2_MEDIUM },
    GfxPreset { name: "High", values: &ROME_2_HIGH },
    GfxPreset { name: "Very High", values: &ROME_2_VERY_HIGH },
    GfxPreset { name: "Ultra", values: &ROME_2_ULTRA },
    GfxPreset { name: "Extreme", values: &ROME_2_EXTREME },
];

/// Rome 2's presets' texture quality by video memory (fn 0x1144c3b0: above 600, 800, 1200 MB); its Very High preset sets 2 whatever the
/// card.
const ROME_2_TEXTURE_BY_MEMORY: &[(u64, i32)] = &[(600, 0), (800, 1), (1200, 2), (u64::MAX, 3)];

/// Rome 2's.
pub const ROME_2: GraphicsCatalogue = GraphicsCatalogue { options: ROME_2_OPTIONS, presets: ROME_2_PRESETS, texture_quality_by_memory: ROME_2_TEXTURE_BY_MEMORY };
