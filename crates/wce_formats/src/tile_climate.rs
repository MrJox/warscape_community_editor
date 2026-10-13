//! The climates of the tile database (`_tile_database/_settings.xml`, `<CLIMATES>`).
//!
//! A climate names the ground texture set a battle map is drawn with and lists the texture groups it has (the names a tile's
//! `TEXTURE_GROUP` red / green / blue / alpha refer to), with the size of one texture repeat in metres. The textures themselves are
//! in the packs: `terrain/textures/battle/<climate>/<group>/<variation>_<su|au|wi>_<diffuse|normal>.dds`, with four variations
//! (0..3) per group; the folder `default` has every group in summer and is what a climate falls back to.

use std::path::Path;

use crate::c_num::strtod;
use crate::xml_doc::{XmlDoc, XmlError};

/// A texture group of a climate.
#[derive(Clone, Debug, PartialEq)]
pub struct ClimateTexture {
    /// `rock`, `mud`, `grass`, `forest`, `settlement_1`...
    pub name: String,

    /// Metres per repeat of the texture inside the tile.
    pub blend_pixel_scale: f64,
    pub outfield_blend_pixel_scale: f64,
    pub mid_distance_strength: f64,

    /// Per variation: the game's light parameters of the texture (what ps30_blend reads from s_light_params).
    pub lerp_smoothness: [f64; 4],
    pub lerp_reflectivity: [f64; 4],
}

impl Default for ClimateTexture {
    fn default() -> Self {
        Self {
            name: String::new(),
            blend_pixel_scale: 40.1,
            outfield_blend_pixel_scale: 1200.1,
            mid_distance_strength: 1.0,
            lerp_smoothness: [0.1; 4],
            lerp_reflectivity: [0.009; 4],
        }
    }
}

/// A climate.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Climate {
    /// `default`, `ROME_Continental`, `ROME_Med`, `ROME_Atlantic`, `ROME_Desert`...
    pub name: String,
    pub texture_set: String,
    pub destruction_climate: String,
    pub textures: Vec<ClimateTexture>,
}

impl Climate {

    pub fn find(&self, group: &str) -> Option<&ClimateTexture> {
        self.textures.iter().find(|texture| texture.name == group)
    }

    /// `terrain/textures/battle/<lower case name>`.
    pub fn folder(&self) -> String {
        format!("terrain/textures/battle/{}", self.name.to_ascii_lowercase())
    }
}

/// The climates of a settings file.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ClimateList {
    pub climates: Vec<Climate>,
}

/// A number attribute: its default when missing, empty or not a number.
fn num(doc: &XmlDoc, elem: usize, name: &str, default: f64) -> f64 {
    doc.get(elem, name).and_then(|value| strtod(&value)).unwrap_or(default)
}

impl ClimateList {

    /// Reads the climates of a settings file; no `<CLIMATES>` gives an empty list.
    ///
    /// # Errors
    ///
    /// Fails for malformed XML.
    pub fn parse(settings_xml: &[u8]) -> Result<Self, XmlError> {
        let doc = XmlDoc::parse(settings_xml.to_vec())?;
        let els = doc.elements();
        let text = |elem: usize, name: &str| doc.get(elem, name).unwrap_or_default();
        let climates = els.iter().enumerate()
            .filter(|(_, el)| el.name == "CLIMATE" && el.parent.is_some_and(|parent| els[parent].name == "CLIMATES"))
            .map(|(i, el)| Climate {
                name: text(i, "name"),
                texture_set: text(i, "texture_set"),
                destruction_climate: text(i, "destruction_climate"),
                textures: el.children.iter()
                    .filter(|&&list| els[list].name == "TEXTURES")
                    .flat_map(|&list| els[list].children.iter().copied().filter(|&t| els[t].name == "TEXTURE"))
                    .map(|t| {
                        let default = ClimateTexture::default();
                        ClimateTexture {
                            name: text(t, "name"),
                            blend_pixel_scale: num(&doc, t, "blend_pixel_scale", default.blend_pixel_scale),
                            outfield_blend_pixel_scale: num(&doc, t, "outfield_blend_pixel_scale", default.outfield_blend_pixel_scale),
                            mid_distance_strength: num(&doc, t, "mid_distance_strength", default.mid_distance_strength),
                            lerp_smoothness: std::array::from_fn(|v| num(&doc, t, &format!("lerp_smoothness{v}"), default.lerp_smoothness[v])),
                            lerp_reflectivity: std::array::from_fn(|v| num(&doc, t, &format!("lerp_reflectivity{v}"), default.lerp_reflectivity[v])),
                        }
                    }).collect(),
            }).collect();

        Ok(Self { climates })
    }

    /// Reads the climates of a settings file; an empty list when the file is missing.
    ///
    /// # Errors
    ///
    /// Fails for malformed XML.
    pub fn load(settings_xml: &Path) -> Result<Self, XmlError> {
        match std::fs::read(settings_xml) {
            Ok(bytes) => Self::parse(&bytes),
            Err(_) => Ok(Self::default()),
        }
    }

    /// A climate by name, case-insensitive.
    pub fn find(&self, name: &str) -> Option<&Climate> {
        self.climates.iter().find(|climate| climate.name.eq_ignore_ascii_case(name))
    }

    pub fn first(&self) -> Option<&Climate> {
        self.climates.first()
    }
}

/// The texture path of a group's variation: the climate's folder first, then `default`; a season (`su`, `au`, `wi`) falls back to
/// summer. `exists` tells whether a path is in the packs. `None` when neither folder has it.
pub fn climate_texture_path(climate_folder: &str, group: &str, variation: i32, season: &str, normal: bool, exists: impl Fn(&str) -> bool) -> Option<String> {
    let kind = if normal { "_normal.dds" } else { "_diffuse.dds" };
    [climate_folder, "terrain/textures/battle/default"].iter()
        .flat_map(|root| [season, "su"].map(|season| format!("{root}/{group}/{variation}_{season}{kind}")))
        .find(|path| exists(path))
}
