//! A tile's grass as BOB generates it (`ACTION_GRASS::run` in BOB_Vegetation, `EMPIREUTILITY::PROCEDURAL_GRASS`): `default.grass_list.bin`
//! from the blend map, the tile's `<Grass generation_seed>` and the kit's `max_grass.xml`. docs/re/vegetation.md has the RE.
//!
//! For each of the tile's texture groups, in name order, the generator draws that group's density of points (`max_grass.xml`: per group, or
//! the default) with the trees' point generator, the random stream running on from group to group. Each point takes the texture group with
//! the largest summed blend at its pixel (the first on ties) and that group's strongest channel as the index (0..3, within its texture);
//! where that channel's value reaches the group / index threshold the point becomes a patch of `<group>/<index>`. The patches are kept by
//! position (the first one at a position stays); the position map's order, sorted by key with MSVC's (unstable) std::sort, is the list's.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::Path;

use wce_formats::c_num::strtod;

use super::ca_map::{CaMap, Vec2Key};
use super::ca_xml;
use super::generate::{Rand, TileInputs, candidates_with};

/// `GRASS_GENERATION_PARAMETERS` (`terrain/vegetation/battle/grass/max_grass.xml`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GrassParams {
    pub density: f32,
    pub channel_density: HashMap<String, f32>,
    pub threshold: f32,
    pub thresholds: HashMap<(String, u32), f32>,
}

impl GrassParams {
    pub fn load(path: &Path) -> Result<Self, String> {
        let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let doc = ca_xml::parse(&bytes);
        let root = doc.child("dataroot").ok_or("max_grass.xml: no <dataroot>")?;
        let num = |t: &str| strtod(t.trim()).unwrap_or(0.0) as f32;
        let mut p = Self { density: root.child_text("max_grass_per_metre_squared").map_or(0.0, num), ..Self::default() };
        p.threshold = root.child_text("default_blendmap_value_threshold").map_or(0.0, num);
        if let Some(per) = root.child("max_grass_per_channel") {
            for c in per.children.iter().filter(|c| c.name == "channel_max_grass") {
                if let Some(name) = c.attribute("channel") {
                    p.channel_density.insert(name.to_owned(), num(&c.text));
                }
            }
        }

        if let Some(per) = root.child("channel_thresholds") {
            for c in per.children.iter().filter(|c| c.name == "channel") {
                let Some(name) = c.attribute("name") else { continue };
                for t in c.children.iter().filter(|t| t.name == "threshold") {
                    if let Some(index) = t.attribute("index").and_then(|i| i.trim().parse::<u32>().ok()) {
                        p.thresholds.insert((name.to_owned(), index), num(&t.text));
                    }
                }
            }
        }

        Ok(p)
    }

    /// `channel_max_grass`.
    pub fn density_of(&self, group: &str) -> f32 {
        self.channel_density.get(group).copied().unwrap_or(self.density)
    }

    /// `channel_index_threshold`.
    pub fn threshold_of(&self, group: &str, index: u32) -> f32 {
        self.thresholds.get(&(group.to_owned(), index)).copied().unwrap_or(self.threshold)
    }
}

/// A patch of a generated list, in the list's frame (x and z in metres, z from the far edge).
#[derive(Clone, Debug, PartialEq)]
pub struct GrassItem {
    /// `<group>/<index>`.
    pub key: String,
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

/// A texture group of the tile, resolved once: its name, its textures (in order) and the threshold of each channel index.
struct GrassGroup<'a> {
    name: &'a String,
    textures: Vec<usize>,
    thresholds: [f32; 4],
}

/// `choose_grass_for_pixel`: the group (its place in `groups`) and channel index of a pixel, if its channel passes the threshold.
fn choose(pixel: &super::generate::Pixel, groups: &[GrassGroup]) -> Option<(usize, u32)> {
    let mut best = 0.0f32;
    let mut chosen: Option<(usize, u32, f32)> = None;
    for (gi, g) in groups.iter().enumerate() {
        let (mut sum, mut max, mut index) = (0.0f32, 0.0f32, 0u32);
        for &t in &g.textures {
            for c in 0..4u32 {
                let v = pixel.norm[t * 4 + c as usize];
                if v > 0.0 {
                    sum += v;
                    if max < v {
                        max = v;
                        index = c;
                    }
                }
            }
        }

        if best < sum {
            best = sum;
            chosen = Some((gi, index, max));
        }

        if best == 1.0 {
            break;
        }
    }

    let (gi, index, max) = chosen?;
    (groups[gi].thresholds[index as usize] <= max).then_some((gi, index))
}

/// The generated grass of a tile from its seed (`<Grass generation_seed>`).
pub fn generate_grass(inputs: &TileInputs, params: &GrassParams, seed: u32) -> Vec<GrassItem> {
    let mut groups: BTreeMap<String, BTreeSet<usize>> = BTreeMap::new();
    for (i, g) in inputs.texture_groups.iter().enumerate() {
        groups.entry(g.clone()).or_default().insert(i);
    }

    let groups: Vec<GrassGroup> = groups
        .iter()
        .map(|(name, textures)| GrassGroup { name, textures: textures.iter().copied().collect(), thresholds: [0, 1, 2, 3].map(|i| params.threshold_of(name, i)) })
        .collect();

    let mut rand = Rand(seed);
    let mut placed: CaMap<Vec2Key, (usize, u32)> = CaMap::default();
    for g in &groups {
        let points = candidates_with(inputs, params.density_of(g.name), &mut rand, &|_, _| false);
        for p in points {
            if let Some(chosen) = choose(&inputs.pixel(p.px, p.py), &groups) {
                placed.insert(Vec2Key(p.x, p.y), chosen);
            }
        }
    }

    // ACTION_GRASS::run: to the list's frame; positions off the tile or off the height map are left out. The height is the list writer's
    // (the height map times the height alpha map).
    let rs = inputs.render_scale;
    let w_scene = inputs.cells_x as f32 * 128.0 * rs;
    let h_scene = inputs.cells_y as f32 * 128.0 * rs;
    let mut out = vec![];
    for (k, (group, index)) in placed.iter() {
        let x = (1.0 / rs) * k.0;
        let z = (h_scene - k.1) * (1.0 / rs);
        if x <= 0.0 || w_scene < x || z <= 0.0 || h_scene <= z {
            continue;
        }

        let Some((col, row)) = inputs.list_pixel(k.0, k.1) else { continue };
        out.push(GrassItem { key: format!("{}/{index}", groups[*group].name), x, y: inputs.list_height(col, row), z });
    }

    // Written through a vector sorted by key with MSVC's std::sort (not stable: it decides the order of a key's patches).
    super::msvc_sort::sort(&mut out, &|a: &GrassItem, b: &GrassItem| a.key < b.key);
    out
}
