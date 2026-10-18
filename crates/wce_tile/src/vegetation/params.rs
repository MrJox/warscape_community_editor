//! The generator's parameters (`EMPIREUTILITY::PROCEDURAL_GENERATION_PARAMETERS`), read from the Assembly Kit's
//! `working_data/BattleTerrain/vegetation/`: `generation_categories.xml` (the categories, one tree list each), `per_climate_densities.xml`
//! (objects per square metre) and every `*tree_parameters.xml` (groups and their objects), in file name order. The maps keep the first
//! definition of a key, and their order matters (see `ca_map`). How BOB takes the malformed files is in `ca_xml`.

use std::path::Path;

use wce_formats::c_num::strtod;

use super::ca_map::CaMap;
use super::ca_xml::{self, Element};

/// What a generator places: the object types of `tree_parameters.xml`.
pub const TREE: u32 = 1;
pub const PROP: u32 = 2;
pub const DECAL: u32 = 4;
pub const VFX: u32 = 8;

/// A range with a falloff: probability `p` on (inner_low, inner_high), a ramp from `low` and to `high`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Range {
    pub low: f32,
    pub high: f32,
    pub inner_low: f32,
    pub inner_high: f32,
}

impl Range {
    /// An object's range: when the falloff is wider than the range, the plateau gets wider than the range.
    fn object(low: f32, high: f32, falloff: f32) -> Self {
        let r = high - low;
        if r < falloff {
            let d = (r - falloff) * 0.5;
            Self { low, high, inner_low: d + low, inner_high: high - d }
        } else {
            Self { low, high, inner_low: low, inner_high: high }
        }
    }

    /// A group's range: when the falloff is narrower than the range, the plateau is the falloff wide, in the middle.
    fn group(low: f32, high: f32, falloff: f32) -> Self {
        let r = high - low;
        if falloff < r {
            let d = (r - falloff) * 0.5;
            Self { low, high, inner_low: d + low, inner_high: high - d }
        } else {
            Self { low, high, inner_low: low, inner_high: high }
        }
    }

    /// `get_blendmap_range_probability` / `get_group_channel_range_probability`.
    pub fn blend_probability(&self, v: f32, p: f32) -> f32 {
        if self.inner_low <= v && v <= self.inner_high {
            return p;
        }

        if self.low <= v && v != self.low && v < self.inner_low {
            return (p / (self.inner_low - self.low)) * (v - self.low);
        }

        if v < self.high && self.inner_high < v {
            return (self.high - v) * (p / (self.high - self.inner_high));
        }

        0.0
    }

    /// `get_height_range_probability`: a height on the low end counts as 0.001 above it.
    pub fn height_probability(&self, v: f32, p: f32) -> f32 {
        let v = if v == self.low { (f64::from(v) + 0.001) as f32 } else { v };
        if self.inner_low <= v && v <= self.inner_high {
            return p;
        }

        if self.low <= v && v < self.inner_low {
            return (p / (self.inner_low - self.low)) * (v - self.low);
        }

        if v <= self.high && self.inner_high < v {
            return (self.high - v) * (p / (self.high - self.inner_high));
        }

        0.0
    }
}

/// A `tree` / `prop` / `decal` / `vfx` of a group (`SINGLE_GENERATION_OBJECT_PARAMETERS`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ObjectParams {
    /// As written: `trees\oak\oak_A`.
    pub model: String,
    pub group: String,
    pub kind: u32,
    pub size_low: f32,
    pub size_range: f32,
    pub blend: Range,
    pub height: Range,
    pub probability: f32,
    pub min_separation: f32,
    pub width: f32,
    pub include_in_outfield: bool,
}

impl ObjectParams {
    /// `get_random_size`: `hi * k * range * blend + low` on the x87, so every step is rounded to double (MSVC's 53-bit precision) and
    /// only the result to f32.
    pub fn random_size(&self, rand: &mut super::generate::Rand, blend: f32) -> f32 {
        let k = f64::from(f32::from_bits(0x3780_0080));
        (f64::from(rand.hi()) * k * f64::from(self.size_range) * f64::from(blend) + f64::from(self.size_low)) as f32
    }
}

/// A `group` (`VEGETATION_GROUP_PARAMETERS`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GroupParams {
    pub name: String,
    pub channel: Range,
    pub height: Range,
    pub probability: f32,
    pub grouping_probability: f32,
    pub grouping_radius: f32,

    /// The tile's texture group whose channels decide where the group may go, and the one (and its channel) whose value weighs it.
    pub channel_group: String,
    pub channel_group_for_probability: String,
    pub channel_number: u32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Densities {
    pub trees: f32,
    pub decals: f32,
    pub vfx: f32,
}

#[derive(Clone, Debug, Default)]
pub struct GenerationParams {
    pub categories: Vec<String>,
    pub densities: Vec<Densities>,

    /// Every object, keyed `<group>_<model>`; the first definition of a key wins.
    pub objects: Vec<ObjectParams>,
    object_index: CaMap<String, usize>,
    pub groups: CaMap<String, GroupParams>,

    /// Per humidity (arid, humid) and per category: key -> object index, in the order BOB inserted them.
    pub by_humidity: [CaMap<String, usize>; 2],
    pub by_category: Vec<CaMap<String, usize>>,

    /// The parameter files read, in order.
    pub files: Vec<String>,

    /// `per_climate_tree_conversions.xml`: per lower-case climate, lower-case `species.variant` -> its replacement (a later entry wins).
    pub conversions: std::collections::HashMap<String, std::collections::HashMap<String, String>>,
}

fn value_f32(el: &Element, name: &str) -> f32 {
    el.child_text(name).and_then(strtod).unwrap_or(0.0) as f32
}

fn text(el: &Element, name: &str) -> Option<String> {
    el.child_text(name).map(|t| t.trim().to_owned())
}

impl GenerationParams {
    /// From the Assembly Kit's `working_data/BattleTerrain/vegetation` folder.
    pub fn load(dir: &Path) -> Result<Self, String> {
        let read = |name: &str| std::fs::read(dir.join(name)).map_err(|e| format!("{name}: {e}"));
        let mut p = Self::default();

        let categories = ca_xml::parse(&read("generation_categories.xml")?);
        let root = categories.child("categories").ok_or("generation_categories.xml: no <categories>")?;
        p.categories = root.children.iter().filter(|c| c.name == "category").map(|c| c.text.trim().to_owned()).collect();
        p.densities = vec![Densities::default(); p.categories.len()];
        p.by_category = (0..p.categories.len()).map(|_| CaMap::default()).collect();

        let densities = ca_xml::parse(&read("per_climate_densities.xml")?);
        if let Some(root) = densities.child("climates") {
            for c in root.children.iter().filter(|c| c.name == "climate") {
                let Some(name) = text(c, "name") else { continue };
                if let Some(i) = p.categories.iter().position(|k| *k == name) {
                    p.densities[i] = Densities {
                        trees: value_f32(c, "max_trees_per_metre_squared"),
                        decals: value_f32(c, "max_decals_per_metre_squared"),
                        vfx: value_f32(c, "max_vfx_per_metre_squared"),
                    };
                }
            }
        }

        // FUN_1004e440 (QtXml: the climate's `name`, each `conversion`'s `old_model` / `new_model`); a missing file is no conversions.
        if let Ok(bytes) = read("per_climate_tree_conversions.xml") {
            let doc = ca_xml::parse(&bytes);
            if let Some(root) = doc.child("dataroot") {
                for c in root.children.iter().filter(|c| c.name == "climate") {
                    let climate = c.attribute("name").unwrap_or_default().to_lowercase();
                    let map = p.conversions.entry(climate).or_default();
                    for conv in c.children.iter().filter(|c| c.name == "conversion") {
                        let old = conv.attribute("old_model").unwrap_or_default().to_lowercase();
                        let new = conv.attribute("new_model").unwrap_or_default().to_lowercase();
                        map.insert(old, new);
                    }
                }
            }
        }

        let mut files: Vec<String> = std::fs::read_dir(dir)
            .map_err(|e| format!("{}: {e}", dir.display()))?
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.to_lowercase().ends_with("tree_parameters.xml"))
            .collect();
        files.sort_by_key(|n| n.to_lowercase());

        for f in &files {
            let doc = ca_xml::parse(&read(f)?);
            let Some(trees) = doc.child("trees") else { continue };
            for el in &trees.children {
                match el.name.as_str() {
                    "group" => {
                        p.group(el);
                    }
                    name => {
                        if let Some(kind) = kind_of(name) {
                            p.object(el, "", kind);
                        }
                    }
                }
            }
        }

        p.files = files;
        Ok(p)
    }

    pub fn category(&self, name: &str) -> Option<usize> {
        self.categories.iter().position(|c| c.eq_ignore_ascii_case(name))
    }

    /// `parse_group_xml`: false stops at the first object that can't be read (the group stays, with the objects before it).
    fn group(&mut self, el: &Element) -> bool {
        let Some(name) = el.attribute("group_name").map(str::to_owned) else { return false };
        let required = ["group_channel_range_low", "group_channel_range_high", "group_channel_range_falloff", "height_range_low", "height_range_high", "height_range_falloff", "probability", "channel_group"];
        if required.iter().any(|r| el.child(r).is_none()) {
            return false;
        }

        let channel_group = text(el, "channel_group").unwrap_or_default();
        let g = GroupParams {
            name: name.clone(),
            channel: Range::group(value_f32(el, "group_channel_range_low"), value_f32(el, "group_channel_range_high"), value_f32(el, "group_channel_range_falloff")),
            height: Range::group(value_f32(el, "height_range_low"), value_f32(el, "height_range_high"), value_f32(el, "height_range_falloff")),
            probability: value_f32(el, "probability"),
            grouping_probability: value_f32(el, "grouping_probability"),
            grouping_radius: value_f32(el, "grouping_radius"),
            channel_group_for_probability: text(el, "channel_group_for_probability").unwrap_or_else(|| channel_group.clone()),
            channel_group,
            channel_number: value_f32(el, "channel_number") as u32,
        };

        self.groups.insert(name.clone(), g);
        for c in &el.children {
            if let Some(kind) = kind_of(&c.name)
                && !self.object(c, &name, kind)
            {
                return false;
            }
        }

        true
    }

    /// `parse_object_parameters_xml`.
    fn object(&mut self, el: &Element, group: &str, kind: u32) -> bool {
        let required = [
            "model", "size_low", "size_high", "blendmap_range_low", "blendmap_range_high", "blendmap_range_falloff", "height_low", "height_high", "height_falloff",
            "probability", "humidities", "categories",
        ];
        if required.iter().any(|r| el.child(r).is_none()) {
            return false;
        }

        let mut humidities = Vec::new();
        for h in &el.child("humidities").expect("checked").children {
            match (h.name.as_str(), h.text.trim()) {
                ("humidity", "arid") => humidities.push(0),
                ("humidity", "humid") => humidities.push(1),
                _ => return false,
            }
        }

        let mut categories = Vec::new();
        for c in &el.child("categories").expect("checked").children {
            if c.name != "category" {
                return false;
            }

            let Some(i) = self.categories.iter().position(|k| k == c.text.trim()) else { return false };
            categories.push(i);
        }

        humidities.sort_unstable();
        humidities.dedup();
        categories.sort_unstable();
        categories.dedup();

        let size_low = value_f32(el, "size_low");
        let o = ObjectParams {
            model: text(el, "model").unwrap_or_default(),
            group: group.to_owned(),
            kind,
            size_low,
            size_range: value_f32(el, "size_high") - size_low,
            blend: Range::object(value_f32(el, "blendmap_range_low"), value_f32(el, "blendmap_range_high"), value_f32(el, "blendmap_range_falloff")),
            height: Range::object(value_f32(el, "height_low"), value_f32(el, "height_high"), value_f32(el, "height_falloff")),
            probability: value_f32(el, "probability"),
            min_separation: value_f32(el, "minimum_seperation"),
            width: value_f32(el, "object_width"),
            include_in_outfield: el.child("include_in_outfield").is_some(),
        };

        let key = format!("{group}_{}", o.model);
        let next = self.objects.len();
        let (&mut index, inserted) = self.object_index.insert(key.clone(), next);
        if inserted {
            self.objects.push(o);
        }

        for h in humidities {
            self.by_humidity[h].insert(key.clone(), index);
        }

        for c in categories {
            self.by_category[c].insert(key.clone(), index);
        }

        true
    }
}

fn kind_of(name: &str) -> Option<u32> {
    match name {
        "tree" => Some(TREE),
        "prop" => Some(PROP),
        "decal" => Some(DECAL),
        "vfx" => Some(VFX),
        _ => None,
    }
}
