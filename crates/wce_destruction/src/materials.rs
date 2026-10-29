//! What a building's materials are made of, its culture, and the materials of its broken faces and rubble. The game's damaged forts give
//! the templates: `roman_fort_straight`'s damaged level has a block of `roman_damage_bricks_all` (exposed masonry) and one of
//! `roman_wall_rubble` (heaps), both `DefaultMaterial`; another culture's set is the same block with its textures (the ones the packs
//! have). Measured on those blocks (destruction_calibrate): a damage texture spans about 10.5 m, a rubble texture 5.5 m.

use rpfm_lib::files::rigidmodel::MeshBlock;
use rpfm_lib::files::rigidmodel::materials::{Material, MaterialType, TextureType};

use crate::params::{MaterialClass, MaterialOverride};

pub const TEXTURES: &str = "rigidmodels/buildings/textures/";

/// Metres a damage texture and a rubble texture span (one UV unit).
pub const DAMAGE_SPAN: f32 = 10.5;
pub const RUBBLE_SPAN: f32 = 5.5;

/// Per culture: the names it goes by (in keys and texture names), its broken masonry and its rubble texture sets.
const CULTURES: &[(&str, &[&str], &str, &str)] = &[
    ("roman", &["roman", "carthag", "rome_"], "roman_damage_bricks_all", "roman_wall_rubble"),
    ("eastern", &["eastern", "byzant", "east_rom"], "eastern_damage_bricks_all", "eastern_wall_rubble"),
    ("greek", &["greek", "hellen"], "greek_damage_bricks_all", "greek_wall_rubble_test"),
    ("egyptian", &["egypt"], "egyptian_damage_bricks_all", "egyptian_wall_rubble"),
    ("persian", &["persian"], "persian_damage_bricks_all", "persian_wall_rubble"),
    ("sassanid", &["sassanid", "arab", "desert"], "sassanid_damage_bricks_all", "sassanid_wall_rubble"),
    ("gondor", &["gondor"], "gondor_damage_bricks_all", "gondor_wall_rubble"),
    ("barbarian", &["barbarian", "celt", "german", "goth", "frank", "saxon", "hun", "slav", "nomad", "rohan"], "roman_damage_bricks_all", "nordic_rubble01"),
    ("nordic", &["nordic", "norse", "dane", "viking"], "roman_damage_bricks_all", "nordic_rubble01"),
    ("western", &["western", "briton", "pict"], "roman_damage_bricks_all", "western_wall_rubble_test"),
];

/// Broken wood, whatever the culture.
pub const WOOD_DAMAGE: &str = "barbarian_fort_wood_damage";

/// Broken earth (a fort's bank), whatever the culture.
pub const EARTH_DAMAGE: &str = "temp_barbarian_fort_damaged_soil";

/// A material's class, from its diffuse texture's name; a name that tells nothing is the culture's way of building (the barbarian and
/// nordic buildings are timber: `barbarian_fort_walls`, `nordic_atlas`).
pub fn classify(material: &Material, culture: &str) -> MaterialClass {
    let name = diffuse(material).to_lowercase();
    let file = name.rsplit('/').next().unwrap_or(&name);
    let has = |words: &[&str]| words.iter().any(|w| file.contains(w));
    if has(&["soil", "grass", "earth", "mud", "dirt", "ground", "bank", "turf"]) {
        MaterialClass::Earth
    } else if has(&["thatch", "straw", "roof", "tile", "shingle", "reed"]) {
        MaterialClass::Roof
    } else if has(&["wood", "timber", "plank", "beam", "log", "palisade", "stake", "splinter", "bark", "fence"]) {
        MaterialClass::Wood
    } else if has(&["plaster", "stucco", "render", "whitewash", "daub"]) {
        MaterialClass::Plaster
    } else if has(&["cloth", "banner", "fabric", "canvas", "tent", "metal", "iron", "glass", "flag", "rope"]) {
        MaterialClass::Other
    } else if has(&["stone", "brick", "rock", "marble", "masonry", "column"]) || !matches!(culture, "barbarian" | "nordic") {
        MaterialClass::Stone
    } else {
        MaterialClass::Wood
    }
}

/// A material's diffuse texture path.
pub fn diffuse(material: &Material) -> String {
    material.textures().iter().find(|t| *t.tex_type() == TextureType::Diffuse).map(|t| t.path().clone()).unwrap_or_default()
}

/// A building's culture, from its key, else its textures' names; Roman when nothing tells.
pub fn culture<'a>(key: &str, textures: impl IntoIterator<Item = &'a str>) -> &'static str {
    let key = key.to_lowercase();
    let by = |s: &str| CULTURES.iter().find(|(_, names, ..)| names.iter().any(|n| s.contains(n))).map(|c| c.0);
    if let Some(c) = by(&key) {
        return c;
    }

    let mut counts = std::collections::BTreeMap::<&str, usize>::new();
    for t in textures {
        if let Some(c) = by(&t.to_lowercase()) {
            *counts.entry(c).or_default() += 1;
        }
    }

    counts.into_iter().max_by_key(|(_, n)| *n).map_or("roman", |(c, _)| c)
}

/// The cultures the editor knows (for the panel's list).
pub fn cultures() -> impl Iterator<Item = &'static str> {
    CULTURES.iter().map(|c| c.0)
}

/// The damage texture set of a culture's broken faces of a class, and its rubble's.
pub fn damage_sets(culture: &str, class: MaterialClass) -> (&'static str, &'static str) {
    let c = CULTURES.iter().find(|c| c.0 == culture).unwrap_or(&CULTURES[0]);
    let caps = match class {
        MaterialClass::Wood => WOOD_DAMAGE,
        MaterialClass::Earth => EARTH_DAMAGE,
        _ => c.2,
    };
    (caps, c.3)
}

/// The override of a material, if the artist set one.
pub fn override_of<'a>(overrides: &'a [MaterialOverride], material: &Material) -> Option<&'a MaterialOverride> {
    let d = diffuse(material);
    overrides.iter().find(|o| o.diffuse.eq_ignore_ascii_case(&d))
}

/// A block of a texture set (`<set>_diffuse.dds`, normal, specular, gloss map, and the game's `test_mask`), made from a template block of
/// the same kind (`DefaultMaterial`); only the textures the packs have (`exists`) are named.
pub fn set_block(template: &MeshBlock, set: &str, exists: &dyn Fn(&str) -> bool) -> MeshBlock {
    let mut b = template.clone();
    b.mesh_mut().vertices_mut().clear();
    b.mesh_mut().indices_mut().clear();
    b.mesh_mut().set_material_type(MaterialType::DefaultMaterial);
    let m = b.material_mut();
    m.set_vertex_format(rpfm_lib::files::rigidmodel::vertices::VertexFormat::Static);
    m.set_i_matrix_index(-1);
    m.set_i_parent_matrix_index(-1);
    let mut textures = template.material().textures().iter().filter(|t| *t.tex_type() == TextureType::Mask).cloned().collect::<Vec<_>>();
    let wanted = [(TextureType::Diffuse, "diffuse"), (TextureType::Normal, "normal"), (TextureType::Specular, "specular"), (TextureType::GlossMap, "gloss_map")];
    let mut tex = template.material().textures().first().cloned().unwrap_or_default();
    let mut out = vec![];
    for (kind, suffix) in wanted {
        let path = format!("{TEXTURES}{set}_{suffix}.dds");
        if exists(&path) {
            tex.set_tex_type(kind);
            tex.set_path(path);
            out.push(tex.clone());
        }
    }

    // The template's order: diffuse, normal, specular, mask, gloss.
    let gloss = out.iter().position(|t| *t.tex_type() == TextureType::GlossMap).map(|i| out.remove(i));
    out.append(&mut textures);
    out.extend(gloss);
    m.set_textures(out);
    b
}

/// Whether a texture set has its diffuse texture in the packs.
pub fn has_set(set: &str, exists: &dyn Fn(&str) -> bool) -> bool {
    exists(&format!("{TEXTURES}{set}_diffuse.dds"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cultures_and_classes() {
        assert_eq!(culture("roman_fort_straight", []), "roman");
        assert_eq!(culture("wce_x", ["rigidmodels/buildings/textures/sassanid_city_atlas_diffuse.dds"]), "sassanid");
        assert_eq!(culture("barbarian_ahouse_1", []), "barbarian");
        assert_eq!(culture("easterling_fort_straight", []), "roman", "easterling is not eastern");
        assert_eq!(damage_sets("greek", MaterialClass::Stone), ("greek_damage_bricks_all", "greek_wall_rubble_test"));
        assert_eq!(damage_sets("greek", MaterialClass::Wood).0, WOOD_DAMAGE);
        let mut m = Material::default();
        let mut t = rpfm_lib::files::rigidmodel::materials::Texture::default();
        t.set_path("rigidmodels/buildings/textures/barbarian_thatch_roof_diffuse.dds".into());
        m.set_textures(vec![t]);
        assert_eq!(classify(&m, "roman"), MaterialClass::Roof);
        let mut t = rpfm_lib::files::rigidmodel::materials::Texture::default();
        t.set_path("rigidmodels/buildings/textures/barbarian_fort_walls_diffuse.dds".into());
        m.set_textures(vec![t]);
        assert_eq!(classify(&m, "barbarian"), MaterialClass::Wood);
        assert_eq!(classify(&m, "roman"), MaterialClass::Stone);
    }
}
