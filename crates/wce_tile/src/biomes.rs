//! Biomes (docs/plans/auto_detail.md, C): a kind of ground laid in areas of a tile. A biome is its trees, undergrowth and rocks (a set by
//! name, or keys inline), how dense they are, how far apart the trees stand, an edge band (towards an area's boundary the trees get smaller
//! and sparser and the undergrowth denser, so that a wood does not end at a line) and texture rules (`texture_rules`) that apply inside its
//! areas.
//!
//! The editor's files, as the vegetation sets: a preset is `<name>.wce` in the projects' `raw_data/art/biomes`; a tile keeps the biomes its
//! areas use in [`FILE`] beside its `tile.wce`. Their text:
//!
//! ```text
//! <BIOME wce_version="1" name="Pine forest" trees="keys:pine.pine_h spruce.spruce_g" undergrowth="set:dry shrubs" rocks=""
//!        tree_density="0.02" tree_spacing="4" undergrowth_density="0.08" rock_density="0.004" edge="12" edge_tree_scale="0.6"
//!        edge_tree_density="0.4" edge_undergrowth="2" tree_scale="0.85 1.15" undergrowth_scale="0.8 1.2">
//!     <rule name="Needles" group="forest" variation="1"/>
//! </BIOME>
//! ```
//!
//! Densities are per square metre; a tile's file holds several `<BIOME>` elements under a `<BIOMES>` root.

use std::path::{Path, PathBuf};

use wce_formats::xml_doc::XmlDoc;

use crate::ground_coverage::Placed;
use crate::texture_rules::Rule;
use crate::vegetation_sets::{SetEntry, VegetationSet};

/// The biomes of a tile, beside its `tile.wce`.
pub const FILE: &str = "biomes.wce";

/// An area of a biome is a `VegetationExclusionPolygonInstance` with these (docs/plans/auto_detail.md, C): the biome's name and the area's
/// seed. TEd and BOB get a plain exclusion polygon (`project_files::to_ted` drops them).
pub const BIOME_ATTR: &str = "wce_biome";
pub const SEED_ATTR: &str = "wce_biome_seed";

/// The type an area is.
pub const AREA_TYPE: &str = "VegetationExclusionPolygonInstance";

/// The built-in biomes' folder name in the preset list: they have no file.
pub const BUILTIN_FOLDER: &str = "(built in)";

/// What a biome puts of one sort: a set by name (trees, or buildings for the rocks), or keys inline (each as likely).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Props {
    pub set: String,
    pub keys: Vec<String>,
}

impl Props {
    fn keys(keys: &str) -> Self {
        Self { set: String::new(), keys: keys.split_whitespace().map(str::to_owned).collect() }
    }

    /// `set:<name>`, `keys:<key> <key>...`, or empty.
    fn text(&self) -> String {
        if !self.set.is_empty() {
            format!("set:{}", self.set)
        } else if !self.keys.is_empty() {
            format!("keys:{}", self.keys.join(" "))
        } else {
            String::new()
        }
    }

    fn parse(text: &str) -> Self {
        if let Some(set) = text.strip_prefix("set:") {
            Self { set: set.trim().to_owned(), keys: vec![] }
        } else {
            Self::keys(text.strip_prefix("keys:").unwrap_or(text))
        }
    }

    /// As a set: the named one among `sets`, else the keys; empty when neither is there.
    #[must_use]
    pub fn resolve(&self, sets: &[VegetationSet]) -> VegetationSet {
        if self.set.is_empty() {
            VegetationSet::from_keys("", self.keys.iter().map(String::as_str))
        } else {
            sets.iter().find(|s| s.name == self.set).cloned().unwrap_or_default()
        }
    }
}

/// A biome.
#[derive(Clone, Debug, PartialEq)]
pub struct Biome {
    pub name: String,

    /// Its preset file and folder below `biomes` (none: built in, or a tile's copy).
    pub folder: String,
    pub path: Option<PathBuf>,

    pub trees: Props,
    pub undergrowth: Props,

    /// Rocks: buildings.
    pub rocks: Props,

    /// Per square metre, inside past the edge band.
    pub tree_density: f32,
    pub undergrowth_density: f32,
    pub rock_density: f32,

    /// Metres the trees keep from each other at least.
    pub tree_spacing: f32,

    /// Metres in from the boundary the edge band reaches. At the boundary the trees are `edge_tree_scale` times their size and
    /// `edge_tree_density` times as many, the undergrowth `edge_undergrowth` times as dense; inside, as the biome says. Smoothly between.
    pub edge: f32,
    pub edge_tree_scale: f32,
    pub edge_tree_density: f32,
    pub edge_undergrowth: f32,

    pub tree_scale: (f32, f32),
    pub undergrowth_scale: (f32, f32),

    /// Texture rules laid inside its areas.
    pub rules: Vec<Rule>,
}

impl Default for Biome {
    fn default() -> Self {
        Self {
            name: String::new(),
            folder: String::new(),
            path: None,
            trees: Props::default(),
            undergrowth: Props::default(),
            rocks: Props::default(),
            tree_density: 1.0 / 50.0,
            undergrowth_density: 1.0 / 12.0,
            rock_density: 0.0,
            tree_spacing: 4.0,
            edge: 12.0,
            edge_tree_scale: 0.6,
            edge_tree_density: 0.4,
            edge_undergrowth: 2.0,
            tree_scale: (0.85, 1.15),
            undergrowth_scale: (0.8, 1.2),
            rules: vec![],
        }
    }
}

/// The folder of the presets in a raw_data folder (the projects').
#[must_use]
pub fn presets_root(raw_data: &Path) -> PathBuf {
    raw_data.join("art").join("biomes")
}

/// Its sets, resolved: what [`generate`] puts.
#[derive(Clone, Debug, Default)]
pub struct Resolved {
    pub trees: VegetationSet,
    pub undergrowth: VegetationSet,
    pub rocks: VegetationSet,
}

impl Biome {
    /// Its sets resolved among `sets` (trees and undergrowth among the vegetation sets, rocks among the building sets: the caller's).
    #[must_use]
    pub fn resolve(&self, sets: &[VegetationSet]) -> Resolved {
        Resolved { trees: self.trees.resolve(sets), undergrowth: self.undergrowth.resolve(sets), rocks: self.rocks.resolve(sets) }
    }

    /// Its element's text, each line after `indent`.
    #[must_use]
    pub fn element_text(&self, indent: &str) -> String {
        let num = |v: f32| crate::ted_scene::format_number(f64::from(v));
        let pair = |(a, b): (f32, f32)| format!("{} {}", num(a), num(b));
        let mut out = format!(
            "{indent}<BIOME {}=\"{}\" name=\"{}\" trees=\"{}\" undergrowth=\"{}\" rocks=\"{}\" tree_density=\"{}\" tree_spacing=\"{}\" \
             undergrowth_density=\"{}\" rock_density=\"{}\" edge=\"{}\" edge_tree_scale=\"{}\" edge_tree_density=\"{}\" \
             edge_undergrowth=\"{}\" tree_scale=\"{}\" undergrowth_scale=\"{}\"",
            crate::project_files::WCE_VERSION_ATTR,
            crate::project_files::WCE_VERSION,
            escape(&self.name),
            escape(&self.trees.text()),
            escape(&self.undergrowth.text()),
            escape(&self.rocks.text()),
            num(self.tree_density),
            num(self.tree_spacing),
            num(self.undergrowth_density),
            num(self.rock_density),
            num(self.edge),
            num(self.edge_tree_scale),
            num(self.edge_tree_density),
            num(self.edge_undergrowth),
            pair(self.tree_scale),
            pair(self.undergrowth_scale),
        );
        if self.rules.is_empty() {
            out += "/>\n";
        } else {
            out += ">\n";
            out += &crate::texture_rules::rule_lines(&self.rules, &format!("{indent}    "));
            out += &format!("{indent}</BIOME>\n");
        }

        out
    }

    /// The biome of a `<BIOME>` element of a document (its rules: the `<rule>` elements after it, up to the next `<BIOME>`).
    fn from_element(doc: &XmlDoc, e: usize) -> Self {
        let d = Self::default();
        let num = |n: &str, or: f32| doc.get(e, n).and_then(|v| v.trim().parse::<f32>().ok()).unwrap_or(or);
        let pair = |n: &str, or: (f32, f32)| {
            doc.get(e, n)
                .and_then(|v| {
                    let mut p = v.split_whitespace().map(str::parse::<f32>);
                    Some((p.next()?.ok()?, p.next()?.ok()?))
                })
                .unwrap_or(or)
        };
        let els = doc.elements();
        let end = (e + 1..els.len()).find(|&i| els[i].name == "BIOME").unwrap_or(els.len());
        let rules =
            crate::texture_rules::rules_in(doc).into_iter().zip(rule_elements(doc)).filter(|(_, i)| (e..end).contains(i)).map(|(r, _)| r);
        Self {
            name: doc.get(e, "name").unwrap_or_default(),
            folder: String::new(),
            path: None,
            trees: Props::parse(&doc.get(e, "trees").unwrap_or_default()),
            undergrowth: Props::parse(&doc.get(e, "undergrowth").unwrap_or_default()),
            rocks: Props::parse(&doc.get(e, "rocks").unwrap_or_default()),
            tree_density: num("tree_density", d.tree_density),
            undergrowth_density: num("undergrowth_density", d.undergrowth_density),
            rock_density: num("rock_density", d.rock_density),
            tree_spacing: num("tree_spacing", d.tree_spacing),
            edge: num("edge", d.edge),
            edge_tree_scale: num("edge_tree_scale", d.edge_tree_scale),
            edge_tree_density: num("edge_tree_density", d.edge_tree_density),
            edge_undergrowth: num("edge_undergrowth", d.edge_undergrowth),
            tree_scale: pair("tree_scale", d.tree_scale),
            undergrowth_scale: pair("undergrowth_scale", d.undergrowth_scale),
            rules: rules.collect(),
        }
    }

    /// Reads a preset file.
    ///
    /// # Errors
    ///
    /// The file can't be read or is not a biome.
    pub fn load(path: &Path, root: &Path) -> Result<Self, String> {
        let mut all =
            parse(&std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?).map_err(|e| format!("{}: {e}", path.display()))?;
        let mut b = all.drain(..).next().ok_or_else(|| format!("{}: no biome", path.display()))?;
        b.name = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        b.folder =
            path.parent().and_then(|p| p.strip_prefix(root).ok()).map(|p| p.to_string_lossy().replace('\\', "/")).unwrap_or_default();
        b.path = Some(path.to_path_buf());
        Ok(b)
    }

    /// Writes the preset to its file (its folders too).
    ///
    /// # Errors
    ///
    /// A built-in biome (no file), or the file can't be written.
    pub fn save(&self) -> Result<(), String> {
        let path = self.path.as_ref().ok_or("a built-in biome has no file")?;
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        }

        wce_foundation::files::write_replacing(path, self.element_text("")).map_err(|e| format!("{}: {e}", path.display()))
    }
}

/// The indices of the `<rule>` elements, in order (as `texture_rules::rules_in` gives them).
fn rule_elements(doc: &XmlDoc) -> Vec<usize> {
    doc.elements().iter().enumerate().filter(|(_, el)| el.name == "rule").map(|(i, _)| i).collect()
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('"', "&quot;").replace('<', "&lt;").replace('>', "&gt;")
}

/// The biomes of a text: one `<BIOME>` (a preset's file) or several under `<BIOMES>` (a tile's).
///
/// # Errors
///
/// It is not XML, or holds no biome.
pub fn parse(bytes: &[u8]) -> Result<Vec<Biome>, String> {
    let doc = XmlDoc::parse(bytes.to_vec()).map_err(|e| e.to_string())?;
    let els = doc.elements();
    if els.first().is_none_or(|e| e.name != "BIOME" && e.name != "BIOMES") {
        return Err("not a biome".to_owned());
    }

    Ok((0..els.len()).filter(|&e| els[e].name == "BIOME").map(|e| Biome::from_element(&doc, e)).collect())
}

/// A tile's file of biomes.
#[must_use]
pub fn text(biomes: &[Biome]) -> Vec<u8> {
    let mut out = format!("<BIOMES {}=\"{}\">\n", crate::project_files::WCE_VERSION_ATTR, crate::project_files::WCE_VERSION);
    for b in biomes {
        out += &b.element_text("    ");
    }

    out += "</BIOMES>\n";
    out.into_bytes()
}

/// The built-in biomes, from Attila's kit vegetation (keys as the kit's tiles use them; the editor converts them for a tile's climate).
#[must_use]
pub fn builtin() -> Vec<Biome> {
    let rocks = Props::keys("gaul_rock_01 gaul_rock_02");
    let forest_floor =
        |name: &str| Rule { name: name.to_owned(), group: "forest".to_owned(), variation: 1.0, strength: 0.8, ..Rule::default() };
    vec![
        Biome {
            name: "Broadleaf wood".to_owned(),
            trees: Props::keys("oak.oak_g oak.oak_f beech.beech_e beech.beech_f birch.birch_d birch.birch_e"),
            undergrowth: Props::keys(
                "shrubs.shrub_rosec shrubs.shrub_burdockb shrubs.shrub_roseb shrubs.shrub_tansyb shrubs.shrub_flowerb",
            ),
            rocks: rocks.clone(),
            rock_density: 1.0 / 400.0,
            rules: vec![forest_floor("Leaf litter")],
            ..Biome::default()
        },
        Biome {
            name: "Pine forest".to_owned(),
            trees: Props::keys("pine.pine_h pine.pine_g spruce.spruce_g spruce.spruce_f spruce.spruce_e"),
            undergrowth: Props::keys("shrubs.shrub_dryshrubb shrubs.shrub_dryshrubc shrubs.shrub_tallflowerb"),
            rocks,
            tree_density: 1.0 / 40.0,
            undergrowth_density: 1.0 / 25.0,
            rock_density: 1.0 / 250.0,
            tree_spacing: 3.5,
            rules: vec![forest_floor("Needles")],
            ..Biome::default()
        },
        Biome {
            name: "Olive grove".to_owned(),
            trees: Props::keys("olive.olive_d olive.olive_c"),
            undergrowth: Props::keys("shrubs_med.shruba_a shrubs_med.yellow_flowera shrubs_med.white_flowera shrubs_med.shruba_c"),
            tree_density: 1.0 / 90.0,
            undergrowth_density: 1.0 / 30.0,
            tree_spacing: 7.0,
            edge: 6.0,
            edge_tree_scale: 0.85,
            edge_tree_density: 0.8,
            edge_undergrowth: 1.5,
            ..Biome::default()
        },
        Biome {
            name: "Reeds and marsh".to_owned(),
            trees: Props::keys("poplar.poplar_e rowan.rowan_d"),
            undergrowth: Props::keys("shrub_calamus.shrub_calamus_a shrubs_med.water_a"),
            tree_density: 1.0 / 400.0,
            undergrowth_density: 1.0 / 4.0,
            tree_spacing: 8.0,
            edge: 5.0,
            edge_undergrowth: 0.6,
            rules: vec![Rule {
                name: "Mud".to_owned(),
                group: "mud|mud_a|mud_b".to_owned(),
                variation: 0.0,
                strength: 0.7,
                ..Rule::default()
            }],
            ..Biome::default()
        },
        Biome {
            name: "Palm grove".to_owned(),
            trees: Props::keys("palm_date.palm_date_f palm_date.palm_date_g palm_date.palm_date_e palm_date.palm_date_d"),
            undergrowth: Props::keys("shrubs_desert.papyrus_a shrubs_desert.papyrus_b shrubs_med.shrubb_a"),
            tree_density: 1.0 / 60.0,
            undergrowth_density: 1.0 / 20.0,
            tree_spacing: 5.0,
            edge: 8.0,
            ..Biome::default()
        },
    ]
}

/// The built-in biomes, then every preset below `root` (`.wce` files) by folder and name; ones that don't read are left out.
#[must_use]
pub fn scan_presets(root: &Path) -> Vec<Biome> {
    let mut out = vec![];
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(read) = std::fs::read_dir(&dir) else { continue };
        for path in read.flatten().map(|e| e.path()) {
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|x| x == "wce")
                && let Ok(b) = Biome::load(&path, root)
            {
                out.push(b);
            }
        }
    }

    out.sort_by_cached_key(|b| (b.folder.to_lowercase(), b.name.to_lowercase()));
    let mut all = builtin().into_iter().map(|b| Biome { folder: BUILTIN_FOLDER.to_owned(), ..b }).collect::<Vec<_>>();
    all.extend(out);
    all
}

/// Whether a point is inside a polygon (x, z; closed implicitly).
#[must_use]
pub fn inside(poly: &[(f32, f32)], x: f32, z: f32) -> bool {
    crate::ground_coverage::inside(poly, x, z)
}

/// Metres from a point to a polygon's boundary.
#[must_use]
pub fn boundary_distance(poly: &[(f32, f32)], x: f32, z: f32) -> f32 {
    let n = poly.len();
    (0..n)
        .map(|i| {
            let (a, b) = (poly[i], poly[(i + 1) % n]);
            let (dx, dz) = (b.0 - a.0, b.1 - a.1);
            let l2 = (dx * dx + dz * dz).max(1e-12);
            let t = (((x - a.0) * dx + (z - a.1) * dz) / l2).clamp(0.0, 1.0);
            (x - (a.0 + dx * t)).hypot(z - (a.1 + dz * t))
        })
        .fold(f32::INFINITY, f32::min)
}

/// What an area holds.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Items {
    pub trees: Vec<Placed>,
    pub undergrowth: Vec<Placed>,
    pub rocks: Vec<Placed>,
}

/// An area's trees, undergrowth and rocks (metres; `height` the ground there): by the biome's densities over its area, thinned and
/// scaled through the edge band, the trees `tree_spacing` apart, none where `keep_off(x, z)` (a building, a road). The same seed gives the
/// same result.
pub fn generate(
    biome: &Biome,
    sets: &Resolved,
    poly: &[(f32, f32)],
    height: &dyn Fn(f32, f32) -> f32,
    keep_off: &dyn Fn(f32, f32) -> bool,
    seed: u64,
) -> Items {
    let mut out = Items::default();
    if poly.len() < 3 {
        return out;
    }

    let (mut x0, mut x1, mut z0, mut z1) = (f32::INFINITY, f32::NEG_INFINITY, f32::INFINITY, f32::NEG_INFINITY);
    for &(x, z) in poly {
        (x0, x1, z0, z1) = (x0.min(x), x1.max(x), z0.min(z), z1.max(z));
    }

    let area = polygon_area(poly);
    let mut rnd = Random::new(seed);
    let edge = biome.edge.max(0.0);
    // How far in a point is, 0 at the boundary, 1 from the edge band's inner side; smoothly between.
    let depth = |x: f32, z: f32| {
        if edge <= 0.0 {
            return 1.0;
        }

        let t = (boundary_distance(poly, x, z) / edge).clamp(0.0, 1.0);
        t * t * (3.0 - 2.0 * t)
    };
    let lerp = |a: f32, b: f32, t: f32| a + (b - a) * t;

    // Darts over the bounding box: each kept where it lands inside, with the chance its place's density gives against the most there is.
    let mut throw = |set: &VegetationSet,
                     density: f32,
                     peak: f32,
                     factor: &dyn Fn(f32) -> f32,
                     scale: (f32, f32),
                     size: &dyn Fn(f32) -> f32,
                     apart: f32,
                     out: &mut Vec<Placed>| {
        if set.entries.is_empty() || density <= 0.0 {
            return;
        }

        let wanted = (area * density * peak).ceil() as usize;
        let mut grid = std::collections::HashMap::<(i32, i32), Vec<usize>>::new();
        let cell = apart.max(0.5);
        let key = |x: f32, z: f32| ((x / cell).floor() as i32, (z / cell).floor() as i32);
        for _ in 0..wanted * 3 {
            let (x, z) = (lerp(x0, x1, rnd.unit()), lerp(z0, z1, rnd.unit()));
            let (keep, pick, s, yaw) = (rnd.unit(), rnd.unit(), rnd.unit(), rnd.unit());
            if !inside(poly, x, z) || keep_off(x, z) {
                continue;
            }

            let t = depth(x, z);
            if keep * peak >= factor(t) {
                continue;
            }

            let (cx, cz) = key(x, z);
            let near = |o: &Placed| (o.x - x).hypot(o.z - z) < apart;
            if apart > 0.0
                && (cx - 1..=cx + 1)
                    .any(|gx| (cz - 1..=cz + 1).any(|gz| grid.get(&(gx, gz)).is_some_and(|v| v.iter().any(|&i| near(&out[i])))))
            {
                continue;
            }

            let Some(entry) = set.pick(pick) else { continue };
            let (lo, hi) = entry.scale.unwrap_or(scale);
            grid.entry((cx, cz)).or_default().push(out.len());
            out.push(Placed { key: entry.key.clone(), x, y: height(x, z), z, scale: lerp(lo, hi, s) * size(t), yaw: yaw * 360.0 });
            if out.len() >= (area * density * peak).round() as usize {
                break;
            }
        }
    };

    // Trees: sparser and smaller at the edge.
    let tree_factor = |t: f32| lerp(biome.edge_tree_density.clamp(0.0, 1.0), 1.0, t);
    let tree_size = |t: f32| lerp(biome.edge_tree_scale.max(0.05), 1.0, t);
    throw(&sets.trees, biome.tree_density, 1.0, &tree_factor, biome.tree_scale, &tree_size, biome.tree_spacing, &mut out.trees);

    // Undergrowth: its density at the edge may be more than inside (the peak scales the darts).
    let peak = biome.edge_undergrowth.max(1.0);
    let under = |t: f32| lerp(biome.edge_undergrowth.max(0.0), 1.0, t);
    throw(&sets.undergrowth, biome.undergrowth_density, peak, &under, biome.undergrowth_scale, &|_| 1.0, 0.0, &mut out.undergrowth);
    out.undergrowth.retain(|u| !out.trees.iter().any(|t| (t.x - u.x).hypot(t.z - u.z) < 0.8));

    // Rocks: even, at scale 1 (the game ignores a building's scale).
    throw(&sets.rocks, biome.rock_density, 1.0, &|_| 1.0, (1.0, 1.0), &|_| 1.0, 2.0, &mut out.rocks);
    out
}

/// A polygon's area (square metres), whichever way round.
#[must_use]
pub fn polygon_area(poly: &[(f32, f32)]) -> f32 {
    let n = poly.len();
    ((0..n).map(|i| poly[i].0 * poly[(i + 1) % n].1 - poly[(i + 1) % n].0 * poly[i].1).sum::<f32>() * 0.5).abs()
}

/// A small random sequence (xorshift, seeded through splitmix), as the ground coverage's.
struct Random(u64);

impl Random {
    fn new(seed: u64) -> Self {
        let mut z = seed.wrapping_add(0x9e37_79b9_7f4a_7c15);
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        Self((z ^ (z >> 31)) | 1)
    }

    fn unit(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 40) as f32 / (1u64 << 24) as f32
    }
}

/// The trees of a set by keys, each once (for tests and the caller's convenience).
#[must_use]
pub fn set_of(keys: &[&str]) -> VegetationSet {
    VegetationSet {
        entries: keys.iter().map(|k| SetEntry { key: (*k).to_owned(), weight: 1.0, scale: None }).collect(),
        ..VegetationSet::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square(side: f32) -> Vec<(f32, f32)> {
        vec![(0.0, 0.0), (side, 0.0), (side, side), (0.0, side)]
    }

    fn resolved() -> Resolved {
        Resolved {
            trees: set_of(&["oak.oak_g", "beech.beech_e"]),
            undergrowth: set_of(&["shrubs.shrub_rosec"]),
            rocks: set_of(&["gaul_rock_01"]),
        }
    }

    fn wood() -> Biome {
        Biome { tree_density: 1.0 / 30.0, undergrowth_density: 1.0 / 10.0, rock_density: 1.0 / 300.0, edge: 15.0, ..Biome::default() }
    }

    #[test]
    fn everything_inside_and_on_the_ground() {
        let tri = vec![(0.0, 0.0), (100.0, 0.0), (0.0, 100.0)];
        let items = generate(&wood(), &resolved(), &tri, &|x, _| x * 0.1, &|_, _| false, 3);
        assert!(!items.trees.is_empty() && !items.undergrowth.is_empty() && !items.rocks.is_empty());
        for p in items.trees.iter().chain(&items.undergrowth).chain(&items.rocks) {
            assert!(inside(&tri, p.x, p.z), "{p:?} outside");
            assert!((p.y - p.x * 0.1).abs() < 1e-4);
        }

        assert!(items.rocks.iter().all(|r| r.scale == 1.0), "rocks at scale 1");
    }

    #[test]
    fn the_edge_band_has_smaller_sparser_trees_and_denser_undergrowth() {
        let poly = square(200.0);
        let b = Biome { edge: 30.0, ..wood() };
        let items = generate(&b, &resolved(), &poly, &|_, _| 0.0, &|_, _| false, 9);
        let band = |p: &Placed| boundary_distance(&poly, p.x, p.z) < 10.0;
        let (edge_area, inner_area) = (200.0f32 * 200.0 - 180.0 * 180.0, 140.0f32 * 140.0);
        let core = |p: &Placed| boundary_distance(&poly, p.x, p.z) >= 30.0;
        let per = |items: &[Placed], f: &dyn Fn(&Placed) -> bool, area: f32| items.iter().filter(|p| f(p)).count() as f32 / area;
        assert!(per(&items.trees, &band, edge_area) < 0.8 * per(&items.trees, &core, inner_area), "sparser trees at the edge");
        assert!(per(&items.undergrowth, &band, edge_area) > 1.2 * per(&items.undergrowth, &core, inner_area), "denser undergrowth there");
        let mean = |f: &dyn Fn(&Placed) -> bool| {
            let v = items.trees.iter().filter(|p| f(p)).map(|p| p.scale).collect::<Vec<_>>();
            v.iter().sum::<f32>() / v.len().max(1) as f32
        };
        assert!(mean(&band) < 0.9 * mean(&core), "smaller trees at the edge");
    }

    #[test]
    fn trees_keep_their_spacing_and_off_what_they_are_told() {
        let poly = square(80.0);
        let b = Biome { tree_density: 1.0 / 5.0, tree_spacing: 4.0, ..wood() };
        let items = generate(&b, &resolved(), &poly, &|_, _| 0.0, &|x, _| x < 20.0, 1);
        for (i, a) in items.trees.iter().enumerate() {
            assert!(a.x >= 20.0, "kept off");
            for c in &items.trees[i + 1..] {
                assert!((a.x - c.x).hypot(a.z - c.z) >= 4.0 - 1e-4);
            }
        }

        assert_eq!(generate(&b, &resolved(), &poly, &|_, _| 0.0, &|x, _| x < 20.0, 1), items, "the same seed, the same result");
        assert_ne!(generate(&b, &resolved(), &poly, &|_, _| 0.0, &|x, _| x < 20.0, 2), items);
    }

    #[test]
    fn biomes_read_back_from_their_text() {
        let mut all = builtin();
        all[0].rocks = Props { set: "my rocks".to_owned(), keys: vec![] };
        all[1].rules.push(Rule { name: "second".to_owned(), group: "rock".to_owned(), ..Rule::default() });
        let back = parse(&text(&all)).unwrap();
        assert_eq!(back, all);

        let dir = tempfile::tempdir().unwrap();
        let root = presets_root(dir.path());
        let mut p = all[1].clone();
        p.name = "dark pines".to_owned();
        p.path = Some(root.join("north").join("dark pines.wce"));
        p.save().unwrap();
        let listed = scan_presets(&root);
        assert_eq!(listed.len(), builtin().len() + 1);
        assert_eq!(listed.last().unwrap(), &Biome { folder: "north".to_owned(), ..p });
        assert!(listed[0].folder == BUILTIN_FOLDER && listed[0].save().is_err());
    }

    #[test]
    fn props_resolve_by_set_or_keys() {
        let sets = vec![VegetationSet { name: "pines".to_owned(), ..set_of(&["pine.pine_h"]) }];
        assert_eq!(Props::parse("set:pines").resolve(&sets).entries[0].key, "pine.pine_h");
        assert_eq!(Props::parse("keys:a b").resolve(&sets).entries.len(), 2);
        assert!(Props::parse("set:gone").resolve(&sets).entries.is_empty());
        assert_eq!(Props::parse(&Props::keys("x y").text()), Props::keys("x y"));
    }
}
