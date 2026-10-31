//! A neighbour tile, opened once for all its placements: its own heights (the game's high-frequency ones: the LF comes from the map), its ground
//! material over the tile without its border, and everything else it publishes, all in its own frame. Its placements draw the same heights
//! buffers and materials (the renderer turns and moves them with the placement, the blend shader turns the maps).

use std::path::Path;
use std::sync::Arc;

use rayon::prelude::*;
use wce_foundation::math::{Float3, Float4x4};
use wce_scene::render_scene::{Kind, Payload, RenderScene, TerrainImage, TerrainMaterial};
use wce_tile::tile_project::TileProject;
use wce_vfs::files::Files;

use super::super::view::crop_material;
use super::super::{MATERIAL_IDS, NORMAL_IDS, TileDocument, next_id};
use crate::terrain_bake::TerrainLook;
use crate::terrain_material::terrain_normal_image;

/// The neighbours' height spacing (scene units).
pub const NEIGHBOUR_STEP: f64 = 2.0;

/// The vertical extent of a context terrain's blend box around its heights (scene units): the box is drawn over the screen and every pixel
/// of it runs the blend shader, so it stays close to the ground (enough for the buildings' terrain skirts).
pub const BOX_BELOW: f64 = 5.0;
pub const BOX_ABOVE: f64 = 30.0;

/// The most cells a side of the coarse grid (the renderer's patch: one draw for the whole tile).
const COARSE_CELLS: i32 = 64;

/// Grid steps a tile's ground reaches into its neighbours' (its border's heights): their grounds overlap instead of meeting at an edge
/// whose two sides need not have the same heights.
const OVERLAP_STEPS: i32 = 4;

/// How far the ring of vertices around a tile's ground goes down (scene units): a skirt that closes what the overlap leaves open.
const SKIRT: f32 = 20.0;

/// The alpha map's texel (scene units): the width over which a tile fades into its neighbour past its edge.
const ALPHA_TEXEL: f64 = 4.0;

/// A tile's heights on a regular grid of its own frame from (ox, oz), scene units.
#[derive(Clone, Debug, Default)]
pub struct Grid {
    pub nx: i32,
    pub nz: i32,
    pub ox: f32,
    pub oz: f32,
    pub step: f32,
    pub heights: Arc<Vec<f32>>,
}

impl Grid {
    fn sample(nx: i32, nz: i32, ox: f64, oz: f64, step: f64, h: &(dyn Fn(f64, f64) -> f32 + Sync)) -> Self {
        let mut heights = vec![0.0f32; (nx * nz) as usize];
        heights.par_chunks_mut(nx as usize).enumerate().for_each(|(j, row)| {
            for (i, v) in row.iter_mut().enumerate() {
                *v = h(ox + i as f64 * step, oz + j as f64 * step);
            }
        });

        Self { nx, nz, ox: ox as f32, oz: oz as f32, step: step as f32, heights: Arc::new(heights) }
    }

    /// Every `k`-th sample inside a margin of `m` samples (`k` divides what is left).
    fn every(&self, m: i32, k: i32) -> Self {
        let (nx, nz) = ((self.nx - 1 - 2 * m) / k + 1, (self.nz - 1 - 2 * m) / k + 1);
        let heights = (0..nz).flat_map(|j| (0..nx).map(move |i| (i, j))).map(|(i, j)| self.heights[((m + j * k) * self.nx + m + i * k) as usize]).collect();
        let (ox, oz) = (self.ox + m as f32 * self.step, self.oz + m as f32 * self.step);
        Self { nx, nz, ox, oz, step: self.step * k as f32, heights: Arc::new(heights) }
    }

    /// The grid with a ring of vertices one step outside it, SKIRT below its edge.
    fn skirted(&self) -> Self {
        let (nx, nz) = (self.nx + 2, self.nz + 2);
        let heights = (0..nz).flat_map(|j| (0..nx).map(move |i| (i, j))).map(|(i, j)| {
            let (si, sj) = ((i - 1).clamp(0, self.nx - 1), (j - 1).clamp(0, self.nz - 1));
            let h = self.heights[(sj * self.nx + si) as usize];
            if i == 0 || j == 0 || i == nx - 1 || j == nz - 1 { h - SKIRT } else { h }
        }).collect();

        Self { nx, nz, ox: self.ox - self.step, oz: self.oz - self.step, step: self.step, heights: Arc::new(heights) }
    }

    pub fn range(&self) -> (f32, f32) {
        self.heights.iter().fold((f32::MAX, f32::MIN), |(lo, hi), &v| (lo.min(v), hi.max(v)))
    }
}

/// One neighbour tile.
#[derive(Debug, Default)]
pub struct Template {
    /// Its extent (scene units of its own frame).
    pub extent: (f64, f64),

    /// Its heights every NEIGHBOUR_STEP over the tile and OVERLAP_STEPS around it; what the near ring draws (with its skirt), and coarse
    /// enough for one patch (the mid ring).
    pub own: Grid,
    pub near: Grid,
    pub coarse: Grid,

    /// Its ground material over the tile with its border (its alpha map fades the border out a little past the tile's edge, where the
    /// neighbour's fades in), and the same at a quarter of the resolution.
    pub material: Option<Arc<TerrainMaterial>>,
    pub material_low: Option<Arc<TerrainMaterial>>,

    /// A custom-mesh tile's mesh (the cliffs), drawn instead of its heights' patches.
    pub custom: Option<Arc<wce_scene::render_scene::CustomTerrainMesh>>,

    /// Everything but the terrain, the markers and the grass; the first `mid` of them are drawn in the mid ring too (buildings, rivers).
    pub entities: Vec<(Float4x4, Payload)>,
    pub mid: usize,

    /// Its size in memory, about (bytes).
    pub bytes: usize,

    /// Seconds to open and publish it (0 when it came from the cache).
    pub load_s: f64,
}

/// Whether an entity is drawn in the mid ring: the buildings and the rivers (not the vegetation, decals, lights and effects).
fn is_mid(p: &Payload) -> bool {
    match p {
        Payload::MeshInstance(mi) => {
            let m = mi.model.to_ascii_lowercase();
            !(m.contains("vegetation") || m.contains("/tree") || m.contains("\\tree") || m.contains("/plants/"))
        }

        Payload::Water(w) => w.river,
        _ => false,
    }
}

/// The smallest power of two that brings `cells` within the coarse grid's size (it divides them: a tile is 64 grid cells a map cell).
fn coarse_factor(cells: i32) -> i32 {
    let mut k = 1;
    while cells / k > COARSE_CELLS && cells % (2 * k) == 0 {
        k *= 2;
    }

    k
}

/// An image at a quarter of its size (the mean of each 4 x 4 block).
fn quarter(img: &TerrainImage) -> TerrainImage {
    let Some(px) = &img.rgba else { return img.clone() };
    let (w, h) = ((img.w / 4).max(1), (img.h / 4).max(1));
    let mut out = vec![0u8; (w * h * 4) as usize];
    out.par_chunks_mut((w * 4) as usize).enumerate().for_each(|(y, row)| {
        for x in 0..w as usize {
            let mut sum = [0u32; 4];
            let mut n = 0;
            for dy in 0..4usize {
                for dx in 0..4usize {
                    let (sx, sy) = ((x * 4 + dx).min(img.w as usize - 1), (y * 4 + dy).min(img.h as usize - 1));
                    let s = (sy * img.w as usize + sx) * 4;
                    for k in 0..4 {
                        sum[k] += u32::from(px[s + k]);
                    }

                    n += 1;
                }
            }

            for k in 0..4 {
                row[x * 4 + k] = (sum[k] / n) as u8;
            }
        }
    });

    TerrainImage { w, h, rgba: Some(Arc::new(out)) }
}

/// The weight of a tile over its material's rectangle: 1 over the tile (0 .. ex, 0 .. ez), 0 over its border, on texels of ALPHA_TEXEL whose
/// edges fall on the tile's: sampled bilinearly, it fades from 1 to 0 over a texel across the tile's edge, as the neighbour's rises from 0 to
/// 1 (the resolve divides by the weights' sum; the blend shader discards below 1 / 11).
fn alpha_map(m: &TerrainMaterial, ex: f64, ez: f64) -> TerrainImage {
    let (w, h) = (((m.x1 - m.x0) / ALPHA_TEXEL).round().max(1.0) as u32, ((m.z1 - m.z0) / ALPHA_TEXEL).round().max(1.0) as u32);
    let (sx, sz) = ((m.x1 - m.x0) / f64::from(w), (m.z1 - m.z0) / f64::from(h));
    let mut px = vec![0u8; (w * h * 4) as usize];
    for (k, p) in px.chunks_mut(4).enumerate() {
        let (i, j) = ((k as u32 % w) as f64, (k as u32 / w) as f64);
        let (x, z) = (m.x0 + (i + 0.5) * sx, m.z0 + (j + 0.5) * sz);
        let inside = x > 0.0 && x < ex && z > 0.0 && z < ez;
        p.copy_from_slice(&[if inside { 255 } else { 0 }, 0, 0, 255]);
    }

    TerrainImage { w, h, rgba: Some(Arc::new(px)) }
}

/// The game's alpha of a tile (`s_alpha_map`; `ps30_blend` reads its R): the entry's `custom_alpha_blend_texture`, else the one of its size,
/// `terrain/textures/<kind>/alpha/<w>x<h>.dds` (the engine's `%dx%d`; 2x3.dds is 512 wide, 640 high, as a 2 x 3 tile's blend map). It covers
/// the tile with its border (one cell around, (cells + 2) * 128 pixels a side), 1 over the tile and an irregular part of the border, so
/// neighbours overlap and fade into each other over 32 .. 96 m in organic shapes (the resolve divides by the weights' sum). None when the
/// game has no such texture.
fn game_alpha(entry: &wce_formats::tile_entry::TileEntry, cells: (u32, u32), files: &dyn Files) -> Option<TerrainImage> {
    let custom = entry.custom_alpha_blend_texture().replace('\\', "/").to_lowercase();
    let kind = Some(entry.kind()).filter(|k| !k.is_empty()).unwrap_or_else(|| "battle".to_owned());
    let path = if custom.is_empty() { format!("terrain/textures/{kind}/alpha/{}x{}.dds", cells.0, cells.1) } else { custom };
    let img = wce_formats::dds_decode::dds_decode_mip(&files.read(&path).ok()?, 0).ok()?;
    Some(TerrainImage { w: img.w, h: img.h, rgba: Some(Arc::new(img.px)) })
}

fn image_bytes(img: &TerrainImage) -> usize {
    img.rgba.as_ref().map_or(0, |p| p.len())
}

impl Template {

    /// Fills in what follows from the rest (the coarse grid, the low material, the size): after a load or a cache read.
    pub fn finish(&mut self) {
        // The coarse grid over the tile alone (its skirt closes what the overlap would): its cells divide by the factor.
        let m = OVERLAP_STEPS.min((self.own.nx - 2) / 2).max(0);
        let k = coarse_factor((self.own.nx - 1 - 2 * m).max(self.own.nz - 1 - 2 * m));
        self.near = self.own.skirted();
        self.coarse = if k > 1 { self.own.every(m, k).skirted() } else { self.near.clone() };

        // The blend box reaches down past the skirt (pixels of it outside every box were drawn black).
        let (mut lo, mut hi) = self.own.range();
        if let Some(c) = &self.custom {
            (lo, hi) = (lo.min(c.bounds[1]), hi.max(c.bounds[4]));
        }

        self.material = self.material.take().map(|m| {
            let mut m = (*m).clone();
            (m.y0, m.y1) = (f64::from(lo - SKIRT) - BOX_BELOW, f64::from(hi) + BOX_ABOVE);
            Arc::new(m)
        });

        self.material_low = self.material.as_ref().map(|m| {
            let mut low = (**m).clone();
            low.blend = quarter(&m.blend);
            low.index = quarter(&m.index);
            low.normal = quarter(&m.normal);
            low.alpha = quarter(&m.alpha);
            low.id = next_id(&MATERIAL_IDS);
            low.normal_id = next_id(&NORMAL_IDS);
            Arc::new(low)
        });

        let images = |m: &Option<Arc<TerrainMaterial>>| m.as_ref().map_or(0, |m| image_bytes(&m.blend) + image_bytes(&m.index) + image_bytes(&m.normal) + image_bytes(&m.alpha));
        let entities: usize = self.entities.iter().map(|(_, p)| match p {
            Payload::Water(w) => w.verts.len() * 48 + w.indices.len() * 4,
            Payload::VfxEmitter(v) => v.mesh.as_ref().map_or(0, |m| m.len() * 36),
            _ => 0,
        } + 160).sum();
        let custom: usize = self.custom.iter().flat_map(|m| &m.lods).map(|l| l.vertices.len() * size_of::<wce_scene::render_scene::CustomTerrainVertex>() + l.indices.len() * 4).sum();
        self.bytes = (self.own.heights.len() + self.near.heights.len() + self.coarse.heights.len()) * 4 + images(&self.material) + images(&self.material_low) + entities + custom;
    }
}

/// The material of a tile over `[x0, z0, x1, z1]` (its border cropped off past what the heights cover), with its normal map from the
/// heights, its alpha map (the game's, `game_alpha`, when the rectangle is the tile with its whole border; else 1 over the tile) and its
/// box; new ids.
fn context_material(
    m: &TerrainMaterial, (x0, z0, x1, z1): (f64, f64, f64, f64), (ex, ez): (f64, f64), own: &Grid, height: &(dyn Fn(f64, f64) -> f32 + Sync),
    alpha: Option<TerrainImage>,
) -> TerrainMaterial {
    let step = NEIGHBOUR_STEP;
    let mut m = m.clone();
    crop_material(&mut m, x0, z0, x1, z1);
    let (gx, gz) = (((m.x1 - m.x0) / step).round().max(1.0) as i32 + 1, ((m.z1 - m.z0) / step).round().max(1.0) as i32 + 1);
    let over = Grid::sample(gx, gz, m.x0, m.z0, step, height);
    m.normal = terrain_normal_image(&over.heights, over.nx, over.nz, step, step, m.blend.w.max(1), m.blend.h.max(1));
    let bordered = (x0 + 128.0).abs() < 1e-6 && (z0 + 128.0).abs() < 1e-6 && (x1 - ex - 128.0).abs() < 1e-6 && (z1 - ez - 128.0).abs() < 1e-6;
    m.alpha = alpha.filter(|_| bordered).unwrap_or_else(|| alpha_map(&m, ex, ez));
    let (lo, hi) = own.range();
    (m.y0, m.y1) = (f64::from(lo) - BOX_BELOW, f64::from(hi) + BOX_ABOVE);
    m.id = next_id(&MATERIAL_IDS);
    m.normal_id = next_id(&NORMAL_IDS);
    m
}

/// Opens a neighbour tile: a scene's ([`load_template`]) or a custom mesh's ([`load_custom_template`]).
pub fn load_any_template(dir: &Path, look: &TerrainLook, trees: bool, game_kit: Option<&Path>, files: Option<Arc<dyn Files>>) -> Result<Template, String> {
    if wce_tile::sources::custom_mesh(dir).is_some() { load_custom_template(dir, look, files) } else { load_template(dir, look, trees, game_kit, files) }
}

/// Opens a custom-mesh tile (the `cliff_custom` set: no scene, no height map): its mesh (the kit's CS2), on the map's LF, with the material
/// of its blend map projected onto it. Its heights are zero: they draw nothing (the game draws no patches for such a tile) but the floor
/// under the sea beside the mesh (`Frame::place`).
/// TODO: the packs' form (`custom_mesh.rigid_model_v2`, `blend.dds`, `index.dds`) for a tile no kit has; every Attila cliff is in the kit.
pub fn load_custom_template(dir: &Path, look: &TerrainLook, files: Option<Arc<dyn Files>>) -> Result<Template, String> {
    let t0 = std::time::Instant::now();
    let found = wce_tile::tile_project::find_tile_info(dir);
    let entry = found.entry.as_ref().ok_or_else(|| format!("no tile database entry for {}", dir.display()))?;
    let (cx, cz) = (found.info.cells_x.max(1), found.info.cells_y.max(1));
    let (ex, ez) = (f64::from(cx) * 128.0, f64::from(cz) * 128.0);
    let step = NEIGHBOUR_STEP;
    let o = f64::from(OVERLAP_STEPS) * step;
    let (nx, nz) = (((ex + 2.0 * o) / step).round() as i32 + 1, ((ez + 2.0 * o) / step).round() as i32 + 1);

    let cs2 = wce_tile::sources::file_any_case(dir, wce_tile::sources::CUSTOM_MESH_CS2).ok_or_else(|| format!("no custom_mesh.cs2 in {}", dir.display()))?;
    let parsed = wce_formats::cas2::Cas2File::parse(&std::fs::read(&cs2).map_err(|e| format!("{}: {e}", cs2.display()))?).map_err(|e| format!("{}: {e}", cs2.display()))?;
    let files = files.ok_or("no game files (the ground's textures)")?;
    let albedo = super::custom_mesh::albedo(super::custom_mesh::CLIFF_TEXTURE, &look.climate, files.as_ref());
    let mesh = super::custom_mesh::from_cs2(&parsed, albedo, next_id(&MATERIAL_IDS), ez as f32).map_err(|e| format!("{}: {e}", cs2.display()))?;
    let height = |_: f64, _: f64| 0.0f32;
    let own = Grid::sample(nx, nz, -o, -o, step, &height);

    // The blend map is the bordered tile's (a cell around it), as a scene tile's.
    let tif = wce_tile::sources::file_any_case(dir, "blend_map.tif").ok_or_else(|| format!("no blend_map.tif in {}", dir.display()))?;
    let blend = wce_tile::blend_field::BlendField::read(&tif).map_err(|e| format!("{}: {e}", tif.display()))?;
    let settings = entry.file().parent().and_then(Path::parent).map(|p| p.join("_settings.xml"));
    let kit = settings.and_then(|s| wce_formats::tile_climate::ClimateList::load(&s).ok()).unwrap_or_default();
    let kind = Some(entry.kind()).filter(|k| !k.is_empty()).unwrap_or_else(|| "battle".to_owned());
    let climates = crate::climate_db::effective_climates(kit, &kind, Some(files.as_ref()));
    let b = 128.0;
    let rq = crate::terrain_material::MaterialRequest {
        folder: dir.to_path_buf(), groups: entry.texture_group(), x0: -b, z0: -b, x1: ex + b, z1: ez + b, y0: -BOX_BELOW, y1: BOX_ABOVE, look: look.clone(), id: 0,
    };

    let (m, _) = crate::terrain_material::build_terrain_material(&rq, &blend, &climates, files.as_ref())?;
    // Its box reaches over the mesh too (`finish`).
    let alpha = game_alpha(entry, (cx, cz), files.as_ref());
    let material = Arc::new(context_material(&m, (-b, -b, ex + b, ez + b), (ex, ez), &own, &height, alpha));
    let mut tpl = Template { extent: (ex, ez), own, material: Some(material), custom: Some(Arc::new(mesh)), load_s: t0.elapsed().as_secs_f64(), ..Default::default() };
    tpl.finish();
    Ok(tpl)
}

/// Opens a neighbour tile (slow: the whole tile document; run it away from the UI thread); `game_kit` is `TileProject::game_kit`.
pub fn load_template(dir: &Path, look: &TerrainLook, trees: bool, game_kit: Option<&Path>, files: Option<Arc<dyn Files>>) -> Result<Template, String> {
    let t0 = std::time::Instant::now();
    let mut project = TileProject::open(dir)?;
    project.game_kit = game_kit.map(Path::to_path_buf);
    let mut doc = TileDocument::new(project, files.clone());
    let t = doc.tile_mut();
    t.look = look.clone();
    t.display.show_grass = false;
    t.display.show_links = false;
    t.display.show_polygons = false;
    t.display.show_border = false;
    t.display.lod = false;
    t.display.tree_list_auto = false;
    if !trees {
        t.display.tree_list.clear();
    } else {
        // The climate's own tree list when the tile has one (the packed tiles have `rome_med.tree_list.bin` and the like), as the game
        // picks it; else the default one.
        let own = look.climate.to_lowercase();
        if t.tree_lists().contains(&own) {
            t.display.tree_list = own;
        }
    }

    let mut scratch = RenderScene::default();
    let layer = scratch.add_layer(t.id, "context");
    t.publish_items(&mut scratch, layer, false);
    t.update_view(&mut scratch, Float3::new(0.0, 0.0, 0.0));

    // The heights over the tile and a little of its border, every NEIGHBOUR_STEP.
    let (ex, ez) = (t.extent_x(), t.extent_z());
    let step = NEIGHBOUR_STEP;
    let o = f64::from(OVERLAP_STEPS) * step;
    let (nx, nz) = (((ex + 2.0 * o) / step).round() as i32 + 1, ((ez + 2.0 * o) / step).round() as i32 + 1);
    // The heights the game draws: BOB's final_heights.dds (the tile's own faded to 0 over the last 64 m of a land edge, so neighbours
    // meet on the LF; docs/re/battle_maps.md, tile seams) when it is not older than the tile, else the editing heights.
    let (project, unit) = (&t.project, t.unit());
    let baked = project.final_heights();
    let height = |x: f64, z: f64| (baked.as_ref().map_or_else(|| project.height_at(x, z), |f| f.at(x, z)) / unit) as f32;
    let own = Grid::sample(nx, nz, -o, -o, step, &height);

    // The material with its border (at most what the height map has), its normal map from the heights over it, its alpha map, its box.
    let border = project.border_m();
    let material = scratch.find(t.terrain_entity).and_then(|e| match &e.payload {
        Payload::Terrain(tr) => tr.material.clone(),
        _ => None,
    }).map(|m| {
        let alpha = files.as_deref().zip(project.entry.as_ref()).and_then(|(f, e)| game_alpha(e, (project.info.cells_x.max(1), project.info.cells_y.max(1)), f));
        Arc::new(context_material(&m, (-border, -border, ex + border, ez + border), (ex, ez), &own, &height, alpha))
    });

    let mut entities: Vec<(Float4x4, Payload)> = scratch
        .entities()
        .iter()
        .filter(|e| e.id != t.terrain_entity && !matches!(e.kind(), Kind::Overlay | Kind::Grass | Kind::Terrain | Kind::CameraRig))
        // The tiles' sea planes give way to the context's one sea level.
        .filter(|e| !matches!(&e.payload, Payload::Water(w) if !w.river))
        .map(|e| (e.world, e.payload.clone()))
        .collect();

    // The mid ring's first.
    entities.sort_by_key(|(_, p)| !is_mid(p));
    let mid = entities.iter().take_while(|(_, p)| is_mid(p)).count();
    let mut tpl = Template { extent: (ex, ez), own, material, entities, mid, load_s: t0.elapsed().as_secs_f64(), ..Default::default() };
    tpl.finish();
    Ok(tpl)
}
