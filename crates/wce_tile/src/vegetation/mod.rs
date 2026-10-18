//! A tile's procedural vegetation as BOB generates it (stage C of docs/plans/tile.md): the trees of `<category>.tree_list.bin` from the raw
//! tile (heights, blend map, mask, the seeds of tile.ted) and the Assembly Kit's vegetation parameters. docs/re/vegetation.md has the RE.
//!
//! Not done yet: the props / decals / VFX outputs (`_procedural_bmd_data`), the grass list, merging the static trees
//! and the per-climate conversions into the written list.

pub mod ca_map;
pub mod ca_xml;
pub mod generate;
pub mod grass;
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

    let blend = tif.pixels.as_chunks::<2>().0[..n * 16].iter().map(|b| f32::from(u16::from_le_bytes(*b)) / 65535.0).collect();
    Ok((tif.width as usize, tif.height as usize, blend))
}

/// The generator's inputs from a raw tile.
pub fn tile_inputs(project: &TileProject) -> Result<TileInputs, String> {
    let entry = project.entry.as_ref().ok_or("no tile database entry")?;
    let variation = project.variation.and_then(|v| entry.variations().get(v).cloned()).ok_or("no tile variation")?;
    let hf = project.height.as_ref().ok_or("no height map")?;
    let (min, scale) = (variation.min_height as f32, variation.scale as f32);
    let (map_w, map_h) = (hf.width() as usize, hf.height() as usize);
    let mut heights = Vec::with_capacity(map_w * map_h);
    for y in 0..map_h as u32 {
        for x in 0..map_w as u32 {
            heights.push((f32::from(hf.raw(x, y)) / 65535.0) * scale + min);
        }
    }

    let (bw, bh, blend) = read_blend_map(&project.dir.join("blend_map.tif"))?;
    if (bw, bh) != (map_w, map_h) {
        return Err(format!("blend_map.tif is {bw}x{bh}, the height map {map_w}x{map_h}"));
    }

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
        cells: cells(entry.width(), entry.height(), &entry.mask()),
        render_scale: project.unit_scale as f32,
        exclusions: exclusions(&project.scene),
    })
}

/// The generated trees of a category's tree list (no static trees), in BOB's order.
pub fn generated_trees(inputs: &TileInputs, params: &GenerationParams, category: usize, seeds: Seeds, model_exists: &dyn Fn(&str) -> bool) -> Vec<ListTree> {
    let density = params.densities.get(category).map_or(0.0, |d| d.trees);
    let chosen = generate::generate(params, inputs, category, params::TREE | params::PROP, seeds.trees, density);
    generate::to_tree_list(inputs, &chosen, model_exists)
}
