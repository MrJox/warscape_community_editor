//! A tile's procedural vegetation as BOB generates it (stage C, docs/plans/tile_roundtrip.md) from the raw tile (heights, blend map, mask, the seeds
//! and static trees of tile.ted) and the Assembly Kit's vegetation parameters: the trees of `<category>.tree_list.bin` (static trees
//! included, converted per climate), the props, decals and VFX of `<category>_procedural_bmd_data.bin`, and the grass list.
//! docs/re/vegetation.md has the RE.

pub mod ca_map;
pub mod ca_xml;
pub mod generate;
pub mod grass;
pub mod height_alpha;
pub mod msvc_sort;
pub mod params;

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use wce_formats::image_containers::TiffFile;

use crate::ted_scene::TedScene;
use crate::tile_project::TileProject;
use crate::world_transform::world_transform;

pub use generate::{Chosen, ListTree, TileInputs};
pub use params::GenerationParams;

/// The seeds of `<Vegetation>` in tile.ted.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Seeds {
    pub trees: u32,
    pub decals: u32,
    pub vfx: u32,
}

pub fn seeds(scene: &TedScene) -> Option<Seeds> {
    let doc = scene.doc();
    let elem = doc.elements().iter().position(|e| e.name == "Vegetation")?;
    let get = |name: &str| doc.get(elem, name).and_then(|v| v.trim().parse::<u32>().ok()).unwrap_or(0);
    Some(Seeds { trees: get("generation_seed"), decals: get("decal_generation_seed"), vfx: get("vfx_generation_seed") })
}

/// `<Grass generation_seed>` of tile.ted.
pub fn grass_seed(scene: &TedScene) -> Option<u32> {
    let doc = scene.doc();
    let elem = doc.elements().iter().position(|e| e.name == "Grass")?;
    doc.get(elem, "generation_seed").and_then(|v| v.trim().parse::<u32>().ok())
}

/// The Assembly Kit's `max_grass.xml` for a tile under its `raw_data` (BOB names it `<raw>/terrain/...`; the kit has it in working_data).
pub fn grass_params_path(project: &TileProject) -> Option<PathBuf> {
    let kit = project.raw_root.as_ref()?.parent()?;
    ["working_data", "raw_data"].iter().map(|d| kit.join(d).join("terrain/vegetation/battle/grass/max_grass.xml")).find(|p| p.is_file())
}

/// The `VegetationExclusionPolygonInstance` outlines in the generator's frame, which is tile.ted's scene (x, z); hidden ones too.
pub fn exclusions(scene: &TedScene) -> Vec<Vec<(f32, f32)>> {
    let doc = scene.doc();
    let mut out = vec![];
    for (elem, e) in doc.elements().iter().enumerate() {
        if e.name != "VegetationExclusionPolygonInstance" {
            continue;
        }

        let w = world_transform(scene, elem);
        let points: Vec<(f32, f32)> = scene
            .polygon_points(elem)
            .into_iter()
            .filter_map(|p| scene.point_position(p))
            .map(|v| {
                let p = w.position + w.rotation * v;
                (p.x as f32, p.z as f32)
            })
            .collect();
        if points.len() >= 3 {
            out.push(points);
        }
    }

    out
}

/// The Assembly Kit's `working_data/BattleTerrain/vegetation` for a tile under its `raw_data`.
pub fn params_dir(project: &TileProject) -> Option<PathBuf> {
    let raw = project.raw_root.as_ref()?;
    let dir = raw.parent()?.join("working_data").join("BattleTerrain").join("vegetation");
    dir.is_dir().then_some(dir)
}

/// The subtiles generation may use: all of them without a mask, else the mask's '1's (row-major, width first), as BOB stores them: (x,
/// height - 1 - y).
pub fn cells(width: u32, height: u32, mask: &str) -> HashSet<(i32, i32)> {
    let bits = mask.as_bytes();
    let mut out = HashSet::new();
    for x in 0..width {
        for y in 0..height {
            let valid = bits.is_empty() || bits.get((y * width + x) as usize) == Some(&b'1');
            if valid {
                out.insert((x as i32, (height - 1 - y) as i32));
            }
        }
    }

    out
}

pub fn read_blend_map(path: &Path) -> Result<(usize, usize, Vec<f32>), String> {
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let tif = TiffFile::parse(&bytes).map_err(|e| format!("{}: {e}", path.display()))?;
    let n = tif.width as usize * tif.height as usize;
    if tif.samples_per_pixel != 16 || tif.bits_per_sample != 16 || tif.pixels.len() < n * 32 {
        return Err(format!("{}: not 16 samples of 16 bits", path.display()));
    }

    // As the heights: BOB's image conversion multiplies by 1 / 65535.
    let blend = tif.pixels.as_chunks::<2>().0[..n * 16].iter().map(|b| f32::from(u16::from_le_bytes(*b)) * (1.0 / 65535.0)).collect();
    Ok((tif.width as usize, tif.height as usize, blend))
}

/// The cells of the tile's links that blend with the neighbour (`build_height_alpha_map`): the link's tile set has a linking tile (in the
/// kit's `_settings.xml`), the test is not `TLT_NOT_EQUALS` and the link doesn't have `no_offline_blend`.
fn blending_links(entry: &wce_formats::tile_entry::TileEntry, settings: Option<&Path>) -> Vec<(i32, i32)> {
    let links = entry.links();
    if links.is_empty() {
        return vec![];
    }

    let Some(doc) = settings.and_then(|p| std::fs::read(p).ok()).and_then(|b| wce_formats::xml_doc::XmlDoc::parse(b).ok()) else { return vec![] };
    let linking: HashSet<String> = doc
        .elements()
        .iter()
        .enumerate()
        .filter(|(i, e)| e.name == "TILE_SET" && doc.get(*i, "linking_tile").is_some_and(|t| !t.is_empty()))
        .filter_map(|(i, _)| doc.get(i, "name"))
        .collect();
    links
        .iter()
        .filter(|l| linking.contains(&l.link_set) && l.test != "TLT_NOT_EQUALS" && !l.no_offline_blend)
        .map(|l| (l.x, l.y))
        .collect()
}

/// The generator's inputs from a raw tile.
pub fn tile_inputs(project: &TileProject) -> Result<TileInputs, String> {
    let entry = project.entry.as_ref().ok_or("no tile database entry")?;
    let variation = project.variation.ok_or("no tile variation")?;
    let hf = project.height.as_ref().ok_or("no height map")?;
    let (map_w, map_h) = (hf.width() as usize, hf.height() as usize);

    // The heights as the next save writes them: in the range they are fitted to, which the entry then has.
    let (min, scale) = hf.fit_range();
    let raw = hf.quantized(min, scale);
    let refitted;
    let entry = if (min, scale) == (hf.min_height(), hf.scale()) {
        entry
    } else {
        let mut e = entry.clone();
        e.set_height_range(variation, min, scale);
        refitted = e;
        &refitted
    };

    // The painted blend map when the tile has one in memory (as the next save writes it).
    let (bw, bh, blend) = match &project.blend {
        Some(b) => (b.width() as usize, b.height() as usize, b.weights().iter().map(|&v| f32::from(v) * (1.0 / 65535.0)).collect()),
        None => read_blend_map(&project.dir.join("blend_map.tif"))?,
    };

    if (bw, bh) != (map_w, map_h) {
        return Err(format!("blend_map.tif is {bw}x{bh}, the height map {map_w}x{map_h}"));
    }

    // The tile database's `_settings.xml`, next to the entry's TILES folder.
    let settings = entry.file().parent().and_then(Path::parent).map(|d| d.join("_settings.xml"));
    tile_inputs_from(entry, variation, &raw, (map_w, map_h), blend, exclusions(&project.scene), settings.as_deref())
}

/// The generator's inputs from a raw tile's parts: its entry and variation, `height_map_0`'s samples (`size` = width, height), the blend
/// map's 16 values per pixel (0..1, as [`read_blend_map`] gives them), the exclusion polygons ([`exclusions`]) and the tile database's
/// `_settings.xml` (for the links that blend).
///
/// # Errors
///
/// The entry has no such variation, or the blend map is not the height map's size.
pub fn tile_inputs_from(
    entry: &wce_formats::tile_entry::TileEntry,
    variation: usize,
    height: &[u16],
    size: (usize, usize),
    blend: Vec<f32>,
    exclusions: Vec<Vec<(f32, f32)>>,
    settings: Option<&Path>,
) -> Result<TileInputs, String> {
    let variation = entry.variations().get(variation).cloned().ok_or("no tile variation")?;
    let (min, scale) = (variation.min_height as f32, variation.scale as f32);
    let (map_w, map_h) = size;
    if height.len() != map_w * map_h || blend.len() != map_w * map_h * 16 {
        return Err(format!("the height map is {map_w}x{map_h} but has {} samples, the blend map {} values", height.len(), blend.len()));
    }

    // BOB's image conversion multiplies by 1 / 65535 (dividing is an ulp off in places).
    let heights = height.iter().map(|&h| (f32::from(h) * (1.0 / 65535.0)) * scale + min).collect();
    let cells = cells(entry.width(), entry.height(), &entry.mask());
    let height_alpha = height_alpha::HeightAlpha::build(entry.width(), entry.height(), variation.raw_data_tri_density, &cells, &blending_links(entry, settings));
    let tg = entry.texture_group();
    Ok(TileInputs {
        heights,
        blend,
        map_w,
        map_h,
        cells_x: entry.width(),
        cells_y: entry.height(),
        tri_density: variation.raw_data_tri_density,
        texture_groups: vec![tg.red, tg.green, tg.blue, tg.alpha],
        cells,
        height_alpha,
        render_scale: 2.0,
        exclusion_y: exclusions.iter().map(|p| p.iter().fold((f32::MAX, f32::MIN), |(lo, hi), &(_, y)| (lo.min(y), hi.max(y)))).collect(),
        exclusions,
        candidate_cache: Default::default(),
    })
}

/// The generated trees of a category's tree list (no static trees), in BOB's order.
pub fn generated_trees(inputs: &TileInputs, params: &GenerationParams, category: usize, seeds: Seeds, model_exists: &dyn Fn(&str) -> bool) -> Vec<ListTree> {
    tree_list(inputs, params, category, seeds, &[], model_exists)
}

/// A category's tree list as BOB writes it: the generated trees and the static ones (`statics`, from [`static_trees`]) converted for the
/// category, in the list's order.
pub fn tree_list(inputs: &TileInputs, params: &GenerationParams, category: usize, seeds: Seeds, statics: &[StaticTree], model_exists: &dyn Fn(&str) -> bool) -> Vec<ListTree> {
    let density = params.densities.get(category).map_or(0.0, |d| d.trees);
    let name = params.categories.get(category).map(String::as_str).unwrap_or_default();
    let statics: Vec<Chosen> = statics.iter().map(|s| static_chosen(params, name, s, model_exists)).collect();
    let chosen = generate::generate_with(params, inputs, category, params::TREE | params::PROP, seeds.trees, density, &statics);
    generate::to_tree_list(inputs, &chosen, model_exists)
}

/// A category's tree list (as [`tree_list`]) and the tree generator's props (the first part of [`generated_props`]) from one run of the
/// tree generator: BOB runs it twice with the same category, mask, seed and density, which give the same map (the generator depends on
/// nothing else), and the tree list's static trees are added to a copy of it. Each `None` when not asked for.
pub fn trees_and_props(
    inputs: &TileInputs, params: &GenerationParams, category: usize, seeds: Seeds, statics: &[StaticTree], model_exists: &dyn Fn(&str) -> bool, trees: bool,
    props: bool,
) -> (Option<Vec<ListTree>>, Option<Vec<generate::ListProp>>) {
    if !trees && !props {
        return (None, None);
    }

    let density = params.densities.get(category).map_or(0.0, |d| d.trees);
    let live = generate::generate_map(params, inputs, category, params::TREE | params::PROP, seeds.trees, density);
    let props = props.then(|| {
        let chosen: Vec<Chosen> = live.iter().map(|(_, c)| c.clone()).collect();
        generate::to_prop_list(inputs, &chosen)
    });

    let trees = trees.then(|| {
        let name = params.categories.get(category).map(String::as_str).unwrap_or_default();
        let statics: Vec<Chosen> = statics.iter().map(|s| static_chosen(params, name, s, model_exists)).collect();
        generate::to_tree_list(inputs, &generate::with_statics(live, &statics), model_exists)
    });

    (trees, props)
}

/// Fills the tile's candidate cache ([`TileInputs::candidate_cache`]) for every generator of the categories (trees and props, decals, VFX:
/// each seed and density once), on the thread pool, so that the categories' generators, run side by side, find their points made.
pub fn prepare_candidates(inputs: &TileInputs, params: &GenerationParams, seeds: Seeds) {
    use rayon::prelude::*;
    let mut keys: Vec<(u32, f32)> = vec![];
    for d in &params.densities {
        for key in [(seeds.trees, d.trees), (seeds.decals, d.trees), (seeds.vfx, d.vfx)] {
            if !keys.iter().any(|k| k.0 == key.0 && k.1.to_bits() == key.1.to_bits()) {
                keys.push(key);
            }
        }
    }

    keys.par_iter().for_each(|&(seed, density)| {
        let _ = generate::cached_candidates(inputs, density, &mut generate::Rand(seed));
    });
}

/// The decal generator's props of a category (the second part of [`generated_props`]), grouped with the tree generator's `props` by key.
pub fn with_decals(inputs: &TileInputs, params: &GenerationParams, category: usize, seeds: Seeds, props: Vec<generate::ListProp>) -> Vec<generate::ListProp> {
    let d = params.densities.get(category).copied().unwrap_or_default();
    let decals = generate::to_decal_list(inputs, &generate::generate(params, inputs, category, params::DECAL, seeds.decals, d.trees));
    by_key(props.into_iter().chain(decals), |p| p.key.clone())
}

/// A `VEGETATION_INSTANCE` BOB adds to every tree list (FUN_1003f390), in the generator's frame.
#[derive(Clone, Debug, PartialEq)]
pub struct StaticTree {
    /// `species.variant` (`beech.beech_f`).
    pub key: String,
    pub x: f32,
    pub y: f32,

    /// The instance's world y.
    pub height: f32,

    /// `scale` (1 when missing).
    pub size: f32,

    /// `surface_mode` is `MSM_FREEFORM`.
    pub freeform: bool,
}

/// `QTU::TedEntity::world_transform`'s translation, in BOB's float arithmetic (QtToolUtility): each element's local matrix from its
/// `position` and `orientation` (`sscanf` floats; `FUN_100af1e0` for the rotation; unit scale, as a `scale` that isn't `vector(..)` leaves
/// it), and the world matrix as the parent's times the local one, the products summed left to right.
fn ted_world_position(scene: &TedScene, elem: usize) -> [f32; 3] {
    let w = crate::world_transform::ted_world_matrix_f32(scene, elem);
    [w[3], w[7], w[11]]
}

/// The `VEGETATION_INSTANCE`s BOB leaves out of the game: those whose world position is in no `VegetationExclusionPolygonInstance` (every
/// one when the tile has none; see `static_trees`). The same test as BOB's, a bounding box per polygon first. Document order.
pub fn trees_outside_exclusions(scene: &TedScene) -> Vec<usize> {
    let polys = exclusions(scene)
        .into_iter()
        .map(|p| {
            let (mut lo, mut hi) = ((f32::MAX, f32::MAX), (f32::MIN, f32::MIN));
            for &(x, z) in &p {
                lo = (lo.0.min(x), lo.1.min(z));
                hi = (hi.0.max(x), hi.1.max(z));
            }

            (lo, hi, p)
        })
        .collect::<Vec<_>>();

    let doc = scene.doc();
    (0..doc.elements().len())
        .filter(|&e| doc.elements()[e].name == "VEGETATION_INSTANCE")
        .filter(|&e| {
            let [x, _, z] = ted_world_position(scene, e);
            !polys.iter().any(|(lo, hi, p)| x >= lo.0 && x <= hi.0 && z >= lo.1 && z <= hi.1 && generate::point_in(p, x, z))
        })
        .collect()
}

/// The tile's static trees as BOB collects them: only when the tile has a `VegetationExclusionPolygonInstance`, every `VEGETATION_INSTANCE`
/// (hidden ones too) whose world position is inside one of the exclusion polygons, the first one at a position; in the order of that
/// position map (a CA map), which is the order BOB adds them to each list in.
pub fn static_trees(scene: &TedScene, inputs: &TileInputs) -> Vec<StaticTree> {
    let doc = scene.doc();
    if !doc.elements().iter().any(|e| e.name == "VegetationExclusionPolygonInstance") {
        return vec![];
    }

    let mut map: ca_map::CaMap<ca_map::Vec2Key, StaticTree> = ca_map::CaMap::default();
    for (elem, e) in doc.elements().iter().enumerate() {
        if e.name != "VEGETATION_INSTANCE" {
            continue;
        }

        let [x, y, z] = ted_world_position(scene, elem);
        if !inputs.exclusions.iter().any(|poly| generate::point_in(poly, x, z)) {
            continue;
        }

        let size = doc.get(elem, "scale").and_then(|s| wce_formats::c_num::strtod(s.trim())).map_or(1.0, |v| v as f32);
        let st = StaticTree {
            key: doc.get(elem, "key").unwrap_or_default(),
            x,
            y: z,
            height: y,
            size,
            freeform: doc.get(elem, "surface_mode").as_deref() == Some("MSM_FREEFORM"),
        };
        map.insert(ca_map::Vec2Key(x, z), st);
    }

    map.iter().map(|(_, s)| s.clone()).collect()
}

/// FUN_1003fb00: a static tree as the category's list has it. The key goes through the category's `per_climate_tree_conversions.xml` table
/// (lower case: a converted key is lower case, an unconverted one keeps its case); the model is `shrubs\<species>\<variant>` if that exists,
/// else `trees\<species>\<variant>` (also when neither does: the list writer then leaves it out).
fn static_chosen(params: &GenerationParams, category: &str, s: &StaticTree, model_exists: &dyn Fn(&str) -> bool) -> Chosen {
    let split = |key: &str| match key.split_once('.') {
        Some((a, b)) => (a.to_owned(), b.to_owned()),
        None => (key.to_owned(), key.to_owned()),
    };

    let (mut species, mut variant) = split(&s.key);
    if let Some(table) = params.conversions.get(&category.to_lowercase())
        && let Some(new) = table.get(&format!("{}.{}", species.to_lowercase(), variant.to_lowercase()))
    {
        (species, variant) = split(new);
    }

    let shrub = format!("shrubs\\{species}\\{variant}");
    let model = if model_exists(&format!("battleterrain/vegetation/{shrub}.rigid_model_v2")) { shrub } else { format!("trees\\{species}\\{variant}") };
    Chosen {
        model,
        x: s.x,
        y: s.y,
        size: s.size,
        min_separation: 0.0,
        grouping_probability: 0.0,
        grouping_radius: 0.0,
        width: 0.0,
        group: String::new(),
        include_in_outfield: false,
        kind: params::TREE,
        height: s.height,
        freeform: s.freeform,
    }
}

/// The static trees as a list of a game without vegetation parameters (Rome 2: no categories, no conversions), in the list frame: the
/// model `trees\<species>\<variant>` (the folder of the older lists' paths, `battleterrain\vegetation\trees\shrubs\<variant>` for the key
/// `shrubs.<variant>`), else `shrubs\<species>\<variant>` when only that exists; trees without a model are left out.
pub fn static_tree_list(inputs: &TileInputs, statics: &[StaticTree], model_exists: &dyn Fn(&str) -> bool) -> Vec<ListTree> {
    let chosen = statics
        .iter()
        .map(|s| {
            let (species, variant) = s.key.split_once('.').unwrap_or((&s.key, &s.key));
            let tree = format!("trees\\{species}\\{variant}");
            let shrub = format!("shrubs\\{species}\\{variant}");
            let path = |m: &str| format!("battleterrain/vegetation/{}.rigid_model_v2", m.replace('\\', "/")).to_lowercase();
            let model = if !model_exists(&path(&tree)) && model_exists(&path(&shrub)) { shrub } else { tree };
            Chosen {
                model,
                x: s.x,
                y: s.y,
                size: s.size,
                min_separation: 0.0,
                grouping_probability: 0.0,
                grouping_radius: 0.0,
                width: 0.0,
                group: String::new(),
                include_in_outfield: false,
                kind: params::TREE,
                height: s.height,
                freeform: s.freeform,
            }
        })
        .collect::<Vec<_>>();
    generate::to_tree_list(inputs, &chosen, model_exists)
}

/// The generated props of a category (the props of its `_procedural_bmd_data`): the tree generator's props, then the decal generator's
/// decals (props of type 1), grouped by key ([`by_key`]). The decal generator runs with the decal seed but the trees' density
/// (`max_trees_per_metre_squared`, not `max_decals`), as BOB does.
pub fn generated_props(inputs: &TileInputs, params: &GenerationParams, category: usize, seeds: Seeds) -> Vec<generate::ListProp> {
    let d = params.densities.get(category).copied().unwrap_or_default();
    let props = generate::to_prop_list(inputs, &generate::generate(params, inputs, category, params::TREE | params::PROP, seeds.trees, d.trees));
    let decals = generate::to_decal_list(inputs, &generate::generate(params, inputs, category, params::DECAL, seeds.decals, d.trees));
    by_key(props.into_iter().chain(decals), |p| p.key.clone())
}

/// Entries grouped by key, each key's in the order they were added, as BOB's marker tables hold them. The keys go in the order they first
/// appear; the writer (`wce_tile_io::attila::stage_c`) puts them in BOB's `MARKER_TABLE` order (`compile::marker_table_order`). The
/// order has no effect in the game.
fn by_key<T>(items: impl Iterator<Item = T>, key: impl Fn(&T) -> String) -> Vec<T> {
    let mut groups: Vec<(String, Vec<T>)> = vec![];
    for item in items {
        let k = key(&item);
        match groups.iter_mut().find(|(g, _)| *g == k) {
            Some((_, v)) => v.push(item),
            None => groups.push((k, vec![item])),
        }
    }

    groups.into_iter().flat_map(|(_, v)| v).collect()
}

/// The generated VFX of a category (the particle emitters of its `_procedural_bmd_data`): the VFX generator, with the tile's VFX seed and
/// the category's `max_vfx_per_metre_squared`.
pub fn generated_vfx(inputs: &TileInputs, params: &GenerationParams, category: usize, seeds: Seeds) -> Vec<generate::ListVfx> {
    let density = params.densities.get(category).map_or(0.0, |d| d.vfx);
    let chosen = generate::generate(params, inputs, category, params::VFX, seeds.vfx, density);
    by_key(generate::to_vfx_list(inputs, &chosen).into_iter(), |v| v.key.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tree_outside_every_exclusion_polygon_is_left_out() {
        let tile = |polygon: &str| format!(
            "<project version=\"16\">\n    <scene>\n        <SCENE_GRAPH name=\"root\" position=\"vector(0,0,0)\" orientation=\"euler(0,0,0)\">\n            <children>\n{polygon}                <VEGETATION_INSTANCE key=\"oak.oak_a\" name=\"in\" position=\"vector(10,0,10)\" orientation=\"euler(0,0,0)\"/>\n                <VEGETATION_INSTANCE key=\"oak.oak_a\" name=\"out\" position=\"vector(100,0,100)\" orientation=\"euler(0,0,0)\"/>\n            </children>\n        </SCENE_GRAPH>\n    </scene>\n</project>\n"
        );
        let polygon = "                <VegetationExclusionPolygonInstance name=\"p\" position=\"vector(0,0,0)\" orientation=\"euler(0,0,0)\">\n                    <points>\n                        <point position=\"0,0,0\"/>\n                        <point position=\"20,0,0\"/>\n                        <point position=\"20,0,20\"/>\n                        <point position=\"0,0,20\"/>\n                    </points>\n                </VegetationExclusionPolygonInstance>\n";
        let scene = TedScene::parse(tile(polygon).into_bytes()).unwrap();
        let out = trees_outside_exclusions(&scene);
        assert_eq!(out.iter().map(|&e| scene.name(e)).collect::<Vec<_>>(), ["out"]);

        let none = TedScene::parse(tile("").into_bytes()).unwrap();
        assert_eq!(trees_outside_exclusions(&none).len(), 2, "without a polygon BOB keeps no manual tree");
    }
}
