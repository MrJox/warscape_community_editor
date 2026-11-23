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

/// What a material averages to, as a baked one stores it (`Template::bake_far`).
pub(super) fn mean_of_material(m: &TerrainMaterial, mean: &dyn Fn(&str) -> Option<[f32; 3]>) -> [u8; 4] {
    let slots = slot_colours(m, mean);
    let detail = detail_byte(m);
    let (Some(blend), Some(index)) = (&m.blend.rgba, &m.index.rgba) else { return [128, 128, 128, detail] };
    let n = (blend.len() / 4).min(index.len() / 4).max(1);
    let sum = blend.as_chunks::<4>().0.iter().zip(index.chunks_exact(4)).fold([0.0f32; 3], |s, (b, i)| {
        let c = blended(b, i, &slots);
        std::array::from_fn(|k| s[k] + c[k] / n as f32)
    });

    let [r, g, b] = sum.map(baked_byte);
    [r, g, b, detail]
}

/// A material's mid-distance detail strength (per layer in `mid_strength`), as a baked material stores it, in its colour's alpha: the
/// resolve lays the detail map over the ground by it, and without it distant ground came out brighter and yellower than the tiles beside it.
fn detail_byte(m: &TerrainMaterial) -> u8 {
    let mean = m.mid_strength.iter().sum::<f32>() / 4.0;
    (mean.clamp(0.0, 1.0) * 255.0).round() as u8
}

/// A linear colour channel as a baked material stores it.
fn baked_byte(v: f32) -> u8 {
    (v.max(0.0).sqrt() * 255.0).round().min(255.0) as u8
}

/// The most cells a side of the coarse grid (the renderer's patch: one draw for the whole tile).
const COARSE_CELLS: i32 = 64;

/// Grid steps a tile's ground reaches into its neighbours' (its border's heights): their grounds overlap instead of meeting at an edge
/// whose two sides need not have the same heights.
const OVERLAP_STEPS: i32 = 4;

/// How fast (metres per metre past the edge) the overlap ring of a tile without heights of its own there goes down (`load_template`).
const RING_DROP: f64 = 2.0;

/// A decompiled tile's carried binary files (`wce_tile_io`'s `stage_b::CARRIED_DIR`).
const TERRAIN_CARRIED_DIR: &str = "decompiled_terrain";

/// A tile entry's mask (`mask='0011…'`): which of its cells (128 m subtiles) are the tile's. Row y counts the cells from the tile's -z
/// edge in the map's terms (v = h - z / 128 of the tile's frame), x its columns.
struct Mask {
    w: i32,
    h: i32,
    cells: Vec<bool>,
}

impl Mask {
    /// The entry's mask when it has one of the tile's size (`extent` in metres) with a cell left out; none otherwise.
    fn of(entry: Option<&wce_formats::tile_entry::TileEntry>, extent: (f64, f64)) -> Option<Self> {
        let (w, h) = ((extent.0 / 128.0).round() as i32, (extent.1 / 128.0).round() as i32);
        let cells: Vec<bool> = entry?.mask().bytes().map(|c| c == b'1').collect();
        (w > 0 && h > 0 && cells.len() == (w * h) as usize && cells.iter().any(|v| !v)).then_some(Self { w, h, cells })
    }

    fn valid(&self, i: i32, j: i32) -> bool {
        (0..self.w).contains(&i) && (0..self.h).contains(&j) && self.cells[(j * self.w + i) as usize]
    }

    /// How far (metres) a point of the tile's frame lies inside the cells left out: 0 in the tile's own cells and on their edges, else the
    /// distance to the nearest of the eight cells around that is the tile's (a cell's side, 128, when none is).
    fn depth(&self, x: f64, z: f64) -> f64 {
        let v = f64::from(self.h) * 128.0 - z;
        let (i, j) = (((x / 128.0).floor() as i32).clamp(0, self.w - 1), ((v / 128.0).floor() as i32).clamp(0, self.h - 1));
        if self.valid(i, j) {
            return 0.0;
        }

        let mut d = 128.0f64;
        for (di, dj) in [(-1, -1), (0, -1), (1, -1), (-1, 0), (1, 0), (-1, 1), (0, 1), (1, 1)] {
            let (a, b) = (i + di, j + dj);
            if !self.valid(a, b) {
                continue;
            }

            let (x0, v0) = (f64::from(a) * 128.0, f64::from(b) * 128.0);
            let dx = (x0 - x).max(x - (x0 + 128.0)).max(0.0);
            let dv = (v0 - v).max(v - (v0 + 128.0)).max(0.0);
            d = d.min(dx.hypot(dv));
        }

        d
    }
}

/// How far the ring of vertices around a tile's ground goes down (scene units): a skirt that closes what the overlap leaves open.
pub(super) const SKIRT: f32 = 20.0;

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
    /// The height at a point of its frame, between its vertices.
    pub fn height(&self, x: f64, z: f64) -> f32 {
        let (u, v) = ((x as f32 - self.ox) / self.step, (z as f32 - self.oz) / self.step);
        let (i, j) = ((u.floor() as i32).clamp(0, self.nx - 2), (v.floor() as i32).clamp(0, self.nz - 2));
        let (fu, fv) = ((u - i as f32).clamp(0.0, 1.0), (v - j as f32).clamp(0.0, 1.0));
        let at = |a: i32, b: i32| self.heights[((j + b) * self.nx + i + a) as usize];
        let (top, bottom) = (at(0, 0) + (at(1, 0) - at(0, 0)) * fu, at(0, 1) + (at(1, 1) - at(0, 1)) * fu);
        top + (bottom - top) * fv
    }

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
        let heights = (0..nz)
            .flat_map(|j| (0..nx).map(move |i| (i, j)))
            .map(|(i, j)| self.heights[((m + j * k) * self.nx + m + i * k) as usize])
            .collect();
        let (ox, oz) = (self.ox + m as f32 * self.step, self.oz + m as f32 * self.step);
        Self { nx, nz, ox, oz, step: self.step * k as f32, heights: Arc::new(heights) }
    }

    /// The grid with a ring of vertices one step outside it, SKIRT below its edge.
    pub(super) fn skirted(&self) -> Self {
        let (nx, nz) = (self.nx + 2, self.nz + 2);
        let heights = (0..nz)
            .flat_map(|j| (0..nx).map(move |i| (i, j)))
            .map(|(i, j)| {
                let (si, sj) = ((i - 1).clamp(0, self.nx - 1), (j - 1).clamp(0, self.nz - 1));
                let h = self.heights[(sj * self.nx + si) as usize];
                if i == 0 || j == 0 || i == nx - 1 || j == nz - 1 { h - SKIRT } else { h }
            })
            .collect();

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

    /// The cells along each edge of `near` and `coarse` that a coarser stride keeps whole across the edge (`Terrain::rim`): their skirt,
    /// and the near grid's overlap.
    pub near_rim: i32,
    pub coarse_rim: i32,

    /// Its ground material over the tile with its border (its alpha map fades the border out a little past the tile's edge, where the
    /// neighbour's fades in), the same at a quarter of the resolution, and at a quarter of that for the ground ring, baked by
    /// [`Template::bake_far`] where the renderer can draw that.
    pub material: Option<Arc<TerrainMaterial>>,
    pub material_low: Option<Arc<TerrainMaterial>>,
    pub material_far: Option<Arc<TerrainMaterial>>,

    /// A custom-mesh tile's mesh (the cliffs), drawn instead of its heights' patches.
    pub custom: Option<Arc<wce_scene::render_scene::CustomTerrainMesh>>,

    /// A custom-mesh tile's floor on a campaign map (`custom_floor`): a grid over its cells and each point's state: covered by the mesh
    /// (in x, z: `FLOOR_COVERED`), else bare on the cliff's land side (1) or sea side (0); the map context raises the land side's
    /// (`Frame::place`).
    pub floor: Option<(Grid, Arc<Vec<u8>>)>,

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
pub(crate) fn game_alpha(entry: &wce_formats::tile_entry::TileEntry, cells: (u32, u32), files: &dyn Files) -> Option<TerrainImage> {
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
    /// Makes the ground ring's material a baked one: its colour instead of its blend and index maps, from the mean colours of its textures
    /// (`mean` gives a path's). What the ring costs is the climate texture arrays, one for each different set of 16 textures (414 at 128
    /// cells, 9 GB of files), and from where it is seen they come down to these means anyway.
    pub fn bake_far(&mut self, mean: &dyn Fn(&str) -> Option<[f32; 3]>) {
        let (Some(m), Some(far)) = (&self.material, &self.material_far) else { return };
        let (Some(blend), Some(index)) = (&m.blend.rgba, &m.index.rgba) else { return };
        if m.blend.w == 0 || m.blend.w != m.index.w || m.blend.h != m.index.h {
            return;
        }

        let slots = slot_colours(m, mean);
        let (w, h, bw, bh) = (far.blend.w as usize, far.blend.h as usize, m.blend.w as usize, m.blend.h as usize);
        let detail = detail_byte(m);
        let mut out = vec![255u8; w * h * 4];
        out.par_chunks_mut(w * 4).enumerate().for_each(|(y, row)| {
            for x in 0..w {
                // A few pixels of what the texel covers.
                let mut sum = [0.0f32; 3];
                for (sx, sy) in (0..4).flat_map(|b| (0..4).map(move |a| ((x * 4 + a) * bw / (w * 4), (y * 4 + b) * bh / (h * 4)))) {
                    let at = (sy * bw + sx) * 4;
                    let c = blended(&blend[at..at + 4], &index[at..at + 4], &slots);
                    sum = std::array::from_fn(|k| sum[k] + c[k] / 16.0);
                }

                for k in 0..3 {
                    row[x * 4 + k] = baked_byte(sum[k]);
                }

                row[x * 4 + 3] = detail;
            }
        });

        let mut baked = (**far).clone();
        baked.baked = true;
        baked.blend.rgba = Some(Arc::new(out));
        baked.index = TerrainImage::default();

        // No fade into the neighbours: at this size the alpha map's feather was gone, and the seams between the tiles showed dark.
        baked.alpha = TerrainImage::default();
        baked.diffuse = Default::default();
        baked.normal_tex = Default::default();
        baked.id = next_id(&MATERIAL_IDS);
        self.material_far = Some(Arc::new(baked));
    }

    /// The baked colour at a point of the tile's frame, as a baked material stores it.
    pub fn far_colour(&self, x: f64, z: f64) -> Option<[u8; 4]> {
        let m = self.material_far.as_ref().filter(|m| m.baked)?;
        let px = m.blend.rgba.as_ref()?;
        let (w, h) = (i64::from(m.blend.w), i64::from(m.blend.h));
        let (u, v) = ((x - m.x0) / (m.x1 - m.x0), (z - m.z0) / (m.z1 - m.z0));
        let (i, j) = (((u * w as f64) as i64).clamp(0, w - 1), ((v * h as f64) as i64).clamp(0, h - 1));
        let at = ((j * w + i) * 4) as usize;
        Some([px[at], px[at + 1], px[at + 2], px[at + 3]])
    }

    /// Fills in what follows from the rest (the coarse grid, the low material, the size): after a load or a cache read.
    pub fn finish(&mut self) {
        // The coarse grid over the tile alone (its skirt closes what the overlap would): its cells divide by the factor.
        let m = OVERLAP_STEPS.min((self.own.nx - 2) / 2).max(0);
        let k = coarse_factor((self.own.nx - 1 - 2 * m).max(self.own.nz - 1 - 2 * m));
        self.near = self.own.skirted();
        self.coarse = if k > 1 { self.own.every(m, k).skirted() } else { self.near.clone() };
        self.near_rim = m + 1;
        self.coarse_rim = if k > 1 { 1 } else { self.near_rim };

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

        let smaller = |m: &Arc<TerrainMaterial>| {
            let mut low = (**m).clone();
            low.blend = quarter(&m.blend);
            low.index = quarter(&m.index);
            low.normal = quarter(&m.normal);
            low.alpha = quarter(&m.alpha);
            low.id = next_id(&MATERIAL_IDS);
            low.normal_id = next_id(&NORMAL_IDS);
            Arc::new(low)
        };

        self.material_low = self.material.as_ref().map(smaller);
        self.material_far = self.material_low.as_ref().map(smaller);

        let images = |m: &Option<Arc<TerrainMaterial>>| {
            m.as_ref().map_or(0, |m| image_bytes(&m.blend) + image_bytes(&m.index) + image_bytes(&m.normal) + image_bytes(&m.alpha))
        };
        let entities: usize = self.entities.iter().map(|(_, p)| match p {
            Payload::Water(w) => w.verts.len() * 48 + w.indices.len() * 4,
            Payload::VfxEmitter(v) => v.mesh.as_ref().map_or(0, |m| m.len() * 36),
            _ => 0,
        } + 160).sum();
        let custom: usize = self
            .custom
            .iter()
            .flat_map(|m| &m.lods)
            .map(|l| l.vertices.len() * size_of::<wce_scene::render_scene::CustomTerrainVertex>() + l.indices.len() * 4)
            .sum();
        self.bytes = (self.own.heights.len() + self.near.heights.len() + self.coarse.heights.len()) * 4
            + images(&self.material)
            + images(&self.material_low)
            + images(&self.material_far)
            + entities
            + custom;
    }
}

/// The colour of each of a material's 16 texture slots (linear, the means of the textures), from `mean`, which gives a path's; a slot
/// without one has the mean of the others.
fn slot_colours(m: &TerrainMaterial, mean: &dyn Fn(&str) -> Option<[f32; 3]>) -> [[f32; 3]; 16] {
    let found = m.diffuse.each_ref().map(|path| if path.is_empty() { None } else { mean(path) });
    let n = found.iter().flatten().count().max(1) as f32;
    let all = found.iter().flatten().fold([0.0f32; 3], |s, c| std::array::from_fn(|k| s[k] + c[k] / n));
    found.map(|c| c.unwrap_or(all))
}

/// What the blend pass draws at a pixel of a material's maps, in one colour per texture slot (ps30_blend): the four layers' weights made
/// unit length, each layer's selector between two of its four variations.
fn blended(blend: &[u8], index: &[u8], slots: &[[f32; 3]; 16]) -> [f32; 3] {
    let length = blend.iter().map(|&b| f32::from(b).powi(2)).sum::<f32>().sqrt().max(1.0);
    let mut out = [0.0f32; 3];
    for layer in 0..4 {
        let selector = f32::from(index[layer]) / 255.0 * 3.0;
        let (first, part) = (selector.floor() as usize, selector.fract());
        let (a, b) = (slots[layer * 4 + first.min(3)], slots[layer * 4 + (first + 1).min(3)]);
        let weight = f32::from(blend[layer]) / length;
        for k in 0..3 {
            out[k] += weight * (a[k] + (b[k] - a[k]) * part);
        }
    }

    out
}

/// The material of a tile over `[x0, z0, x1, z1]` (its border cropped off past what the heights cover), with its normal map from the
/// heights, its alpha map (the game's, `game_alpha`, when the rectangle is the tile with its whole border; else 1 over the tile) and its
/// box; new ids.
fn context_material(
    m: &TerrainMaterial,
    (x0, z0, x1, z1): (f64, f64, f64, f64),
    (ex, ez): (f64, f64),
    own: &Grid,
    height: &(dyn Fn(f64, f64) -> f32 + Sync),
    alpha: Option<TerrainImage>,
) -> TerrainMaterial {
    let step = NEIGHBOUR_STEP;
    let mut m = m.clone();
    crop_material(&mut m, x0, z0, x1, z1);
    let (gx, gz) = (((m.x1 - m.x0) / step).round().max(1.0) as i32 + 1, ((m.z1 - m.z0) / step).round().max(1.0) as i32 + 1);
    let over = Grid::sample(gx, gz, m.x0, m.z0, step, height);
    m.normal = terrain_normal_image(&over.heights, over.nx, over.nz, step, step, m.blend.w.max(1), m.blend.h.max(1));
    let bordered =
        (x0 + 128.0).abs() < 1e-6 && (z0 + 128.0).abs() < 1e-6 && (x1 - ex - 128.0).abs() < 1e-6 && (z1 - ez - 128.0).abs() < 1e-6;
    m.alpha = alpha.filter(|_| bordered).unwrap_or_else(|| alpha_map(&m, ex, ez));
    let (lo, hi) = own.range();
    (m.y0, m.y1) = (f64::from(lo) - BOX_BELOW, f64::from(hi) + BOX_ABOVE);
    m.id = next_id(&MATERIAL_IDS);
    m.normal_id = next_id(&NORMAL_IDS);
    m
}

/// Opens a neighbour tile: a scene's ([`load_template`]) or a custom mesh's ([`load_custom_template`]).
pub fn load_any_template(
    dir: &Path,
    look: &TerrainLook,
    trees: bool,
    game_kit: Option<&Path>,
    files: Option<Arc<dyn Files>>,
) -> Result<Template, String> {
    if wce_tile::sources::custom_mesh(dir).is_some() {
        load_custom_template(dir, look, files)
    } else {
        load_template(dir, look, trees, game_kit, files)
    }
}

/// The step of a custom-mesh tile's floor (tile units: 8 a cell).
const FLOOR_STEP: f64 = 16.0;

/// A custom-mesh tile's floor for a campaign map: a flat grid over its cells (and the `o` around them) and which of its points the mesh's
/// first level covers in x, z. A cliff's mesh need not cover its tile (main_attila's `6x4_inner_right_t1_r3` south-east of Rome: 47 %
/// of it, an inner cliff with land behind it).
fn custom_floor(mesh: &wce_scene::render_scene::CustomTerrainMesh, ex: f64, ez: f64, o: f64) -> (Grid, Arc<Vec<u8>>) {
    let (nx, nz) = (((ex + 2.0 * o) / FLOOR_STEP).round() as i32 + 1, ((ez + 2.0 * o) / FLOOR_STEP).round() as i32 + 1);
    let grid = Grid::sample(nx, nz, -o, -o, FLOOR_STEP, &|_, _| 0.0);
    // The triangles in the tile's frame (x, depth + z; y).
    let tris: Vec<[[f32; 3]; 3]> = mesh.lods.first().map_or_else(Vec::new, |l| {
        let at = |i: u32| {
            let p = l.vertices[i as usize].position;
            [p[0], mesh.depth + p[2], p[1]]
        };
        l.indices.chunks_exact(3).map(|f| [at(f[0]), at(f[1]), at(f[2])]).collect()
    });
    // A point's height on a triangle over it, if it is over it.
    let on = |x: f32, z: f32, t: &[[f32; 3]; 3]| -> Option<f32> {
        let side = |a: [f32; 3], b: [f32; 3]| (b[0] - a[0]) * (z - a[1]) - (b[1] - a[1]) * (x - a[0]);
        let (d0, d1, d2) = (side(t[0], t[1]), side(t[1], t[2]), side(t[2], t[0]));
        let over = (d0 >= 0.0 && d1 >= 0.0 && d2 >= 0.0) || (d0 <= 0.0 && d1 <= 0.0 && d2 <= 0.0);
        let area = d0 + d1 + d2;
        if !over {
            return None;
        }
        // Barycentric weights: each vertex's opposite edge's share.
        Some(if area.abs() < 1e-9 { t[0][2].min(t[1][2]).min(t[2][2]) } else { (d1 * t[0][2] + d2 * t[1][2] + d0 * t[2][2]) / area })
    };
    let (y_lo, y_hi) = tris.iter().flatten().fold((f32::MAX, f32::MIN), |(lo, hi), v| (lo.min(v[2]), hi.max(v[2])));
    let mid = 0.5 * (y_lo + y_hi);
    let (w, h) = (nx as usize, nz as usize);
    // Each point's lowest height of the mesh over it (none: bare).
    let lowest: Vec<Option<f32>> = (0..w * h)
        .map(|k| {
            let (x, z) = ((-o + (k % w) as f64 * FLOOR_STEP) as f32, (-o + (k / w) as f64 * FLOOR_STEP) as f32);
            tris.iter().filter_map(|t| on(x, z, t)).reduce(f32::min)
        })
        .collect();
    // A bare point takes the side of the covered point nearest it (breadth first over the grid): where the mesh's lowest surface over
    // that point is high, it is the cliff's top (the land behind it); where low, its face reaches down there (the sea in front). By the
    // nearest vertex, points just in front of a face's top took the land's side and the floor stood out of the face (capture F, south-
    // east of Rome: the game sees the face 0.3 units further and lower there).
    let mut side: Vec<Option<bool>> = lowest.iter().map(|l| l.map(|y| y > mid)).collect();
    let mut queue: std::collections::VecDeque<usize> = (0..w * h).filter(|&k| side[k].is_some()).collect();
    while let Some(k) = queue.pop_front() {
        let (i, j) = (k % w, k / w);
        for (di, dj) in [(-1i32, 0i32), (1, 0), (0, -1), (0, 1)] {
            let (a, b) = (i as i32 + di, j as i32 + dj);
            if a < 0 || b < 0 || a >= w as i32 || b >= h as i32 {
                continue;
            }
            let n = b as usize * w + a as usize;
            if side[n].is_none() {
                side[n] = side[k];
                queue.push_back(n);
            }
        }
    }
    // And never near a face (`FACE_REACH` points): a steep face's triangles project to thin bands, and grid points between them and the
    // top are bare; taking the top's side, they rose to the land's height in spikes through the face (capture F: strips one or two
    // points wide, the face two or three points off).
    const FACE_REACH: i32 = 3;
    let by_face = |k: usize| {
        let (i, j) = ((k % w) as i32, (k / w) as i32);
        let r = FACE_REACH;
        (-r..=r)
            .flat_map(|dj| (-r..=r).map(move |di| (i + di, j + dj)))
            .any(|(a, b)| a >= 0 && b >= 0 && a < w as i32 && b < h as i32 && lowest[b as usize * w + a as usize].is_some_and(|y| y <= mid))
    };
    let mut state: Vec<u8> = (0..w * h)
        .map(|k| match lowest[k] {
            Some(_) => FLOOR_COVERED,
            None => u8::from(side[k].unwrap_or(false) && !by_face(k)),
        })
        .collect();
    // A land region smaller than `MIN_LAND` points is a gap in the mesh, not land behind it (an inner cliff's: hundreds of points):
    // spots of one or two points stood at the land's height in front of capture F's cliffs.
    const MIN_LAND: usize = 16;
    let mut seen = vec![false; w * h];
    for start in 0..w * h {
        if state[start] != 1 || seen[start] {
            continue;
        }
        let (mut region, mut stack) = (vec![], vec![start]);
        seen[start] = true;
        while let Some(k) = stack.pop() {
            region.push(k);
            let (i, j) = ((k % w) as i32, (k / w) as i32);
            for (a, b) in [(i - 1, j), (i + 1, j), (i, j - 1), (i, j + 1)] {
                if a < 0 || b < 0 || a >= w as i32 || b >= h as i32 {
                    continue;
                }
                let n = b as usize * w + a as usize;
                if state[n] == 1 && !seen[n] {
                    seen[n] = true;
                    stack.push(n);
                }
            }
        }
        if region.len() < MIN_LAND {
            for k in region {
                state[k] = 0;
            }
        }
    }
    (grid, Arc::new(state))
}

/// A custom-mesh tile's floor point the mesh covers (`Template::floor`); 0 a bare point on the cliff's sea side, 1 on its land side.
pub const FLOOR_COVERED: u8 = 2;

/// Opens a custom-mesh tile (the `cliff_custom` set: no scene, no height map): its mesh (the kit's CS2, else the packs' model), on the map's LF, with the material
/// of its blend map projected onto it. Its heights are zero: they draw nothing (the game draws no patches for such a tile) but the floor
/// under the sea beside the mesh (`Frame::place`).
pub fn load_custom_template(dir: &Path, look: &TerrainLook, files: Option<Arc<dyn Files>>) -> Result<Template, String> {
    let t0 = std::time::Instant::now();
    let found = wce_tile::tile_project::find_tile_info(dir);
    let entry = found.entry.as_ref().ok_or_else(|| format!("no tile database entry for {}", dir.display()))?;
    let (cx, cz) = (found.info.cells_x.max(1), found.info.cells_y.max(1));
    let (ex, ez) = (f64::from(cx) * 128.0, f64::from(cz) * 128.0);
    let step = NEIGHBOUR_STEP;
    let o = f64::from(OVERLAP_STEPS) * step;
    let (nx, nz) = (((ex + 2.0 * o) / step).round() as i32 + 1, ((ez + 2.0 * o) / step).round() as i32 + 1);

    let files = files.ok_or("no game files (the ground's textures)")?;
    let albedo = if entry.kind() == "campaign" && files.exists(super::custom_mesh::CAMPAIGN_CLIFF_ALBEDO) {
        super::custom_mesh::CAMPAIGN_CLIFF_ALBEDO.to_owned()
    } else {
        super::custom_mesh::albedo(super::custom_mesh::CLIFF_TEXTURE, &look.climate, files.as_ref())
    };
    // The kit's CS2, else the packs' model (a tile no kit has: Rome 2's).
    let mesh = match wce_tile::sources::file_any_case(dir, wce_tile::sources::CUSTOM_MESH_CS2) {
        Some(cs2) => {
            let parsed = wce_formats::cas2::Cas2File::parse(&std::fs::read(&cs2).map_err(|e| format!("{}: {e}", cs2.display()))?)
                .map_err(|e| format!("{}: {e}", cs2.display()))?;
            super::custom_mesh::from_cs2(&parsed, albedo, next_id(&MATERIAL_IDS), ez as f32)
                .map_err(|e| format!("{}: {e}", cs2.display()))?
        }

        None => {
            let model = wce_tile::sources::file_any_case(dir, wce_tile::sources::CUSTOM_MESH_MODEL)
                .ok_or_else(|| format!("no custom mesh in {}", dir.display()))?;
            let bytes = std::fs::read(&model).map_err(|e| format!("{}: {e}", model.display()))?;
            super::custom_mesh::from_model(&bytes, albedo, next_id(&MATERIAL_IDS), ez as f32)
                .map_err(|e| format!("{}: {e}", model.display()))?
        }
    };
    let height = |_: f64, _: f64| 0.0f32;
    let own = Grid::sample(nx, nz, -o, -o, step, &height);
    let floor = Some(custom_floor(&mesh, ex, ez, o));

    // The blend map is the bordered tile's (a cell around it), as a scene tile's. Some variations have none (Attila's `cliff_custom`
    // `6x2_l0_r0/ad`: only the mesh; the packs ship no blend for any of them): the mesh alone, no ground material.
    let Some(tif) = wce_tile::sources::file_any_case(dir, "blend_map.tif") else {
        let mut tpl = Template {
            extent: (ex, ez),
            own,
            material: None,
            custom: Some(Arc::new(mesh)),
            floor,
            load_s: t0.elapsed().as_secs_f64(),
            ..Default::default()
        };
        tpl.finish();
        return Ok(tpl);
    };
    let blend = wce_tile::blend_field::BlendField::read(&tif).map_err(|e| format!("{}: {e}", tif.display()))?;
    let settings = entry.file().parent().and_then(Path::parent).map(|p| p.join("_settings.xml"));
    let kit = settings.and_then(|s| wce_formats::tile_climate::ClimateList::load(&s).ok()).unwrap_or_default();
    let kind = Some(entry.kind()).filter(|k| !k.is_empty()).unwrap_or_else(|| "battle".to_owned());
    let climates = crate::climate_db::effective_climates(kit, &kind, Some(files.as_ref()));
    let b = 128.0;
    let rq = crate::terrain_material::MaterialRequest {
        folder: dir.to_path_buf(),
        groups: entry.texture_group(),
        x0: -b,
        z0: -b,
        x1: ex + b,
        z1: ez + b,
        y0: -BOX_BELOW,
        y1: BOX_ABOVE,
        look: look.clone(),
        id: 0,
    };

    let (m, _) = crate::terrain_material::build_terrain_material(&rq, &blend, &climates, files.as_ref())?;
    // Its box reaches over the mesh too (`finish`).
    let alpha = game_alpha(entry, (cx, cz), files.as_ref());
    let material = Arc::new(context_material(&m, (-b, -b, ex + b, ez + b), (ex, ez), &own, &height, alpha));
    let mut tpl = Template {
        extent: (ex, ez),
        own,
        material: Some(material),
        custom: Some(Arc::new(mesh)),
        floor,
        load_s: t0.elapsed().as_secs_f64(),
        ..Default::default()
    };
    tpl.finish();
    Ok(tpl)
}

/// Opens a neighbour tile (slow: the whole tile document; run it away from the UI thread); `game_kit` is `TileProject::game_kit`.
pub fn load_template(
    dir: &Path,
    look: &TerrainLook,
    trees: bool,
    game_kit: Option<&Path>,
    files: Option<Arc<dyn Files>>,
) -> Result<Template, String> {
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
    // The trees generated for the look's climate, as the game picks it (its own list when the kit has one, else the default one).
    t.display.show_trees = trees;

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
    // In the tile's units (`TileProject::unit_scale`: a campaign tile's 0.2, as its objects').
    let (project, unit) = (&t.project, t.unit());
    let baked = project.final_heights();
    let height = |x: f64, z: f64| (baked.as_ref().map_or_else(|| project.height_at(x, z), |f| f.at(x, z)) / unit) as f32;
    // The ring past the edge where the tile has no heights there of its own: BOB's final heights end at the edge, and a tile decompiled
    // from the packs has its border filled from the edge. Held at the edge, it stood over a neighbour whose ground falls away from the
    // seam, as a shelf ending in a step (Thrones' port_b over its beach's sea_coast tile: 0.5 m, cutting the beach's decals). Those tiles
    // meet their neighbours exactly at the edge (the export's seams), so their ring slopes down out of the way instead.
    let held = baked.is_some() || project.own_dir().join(TERRAIN_CARRIED_DIR).is_dir();
    // The entry's mask: the subtiles the tile is made of. BOB's terrain mesh leaves out the squares over the others (docs/formats/
    // tile_compile.md, "The mask"), where other placements lie: Thrones' `atlantic_mountains/large_short_30_l` (24 x 20 cells) covers a
    // loch with its mask's zeros, and drawn whole its ground stood over the loch's sea tiles as a sand bank where the game has open water
    // (a RenderDoc capture, 2026-11-16: the game's ground there is the sea and coast tiles'). Its ground over those subtiles slopes down
    // out of the way, as the ring does. Row y of the mask is the tile's cell row from its -z edge in the map's terms: v = h - z / 128.
    let mask = Mask::of(project.entry.as_ref(), (ex, ez));
    let ring = |x: f64, z: f64| {
        let (cx, cz) = (x.clamp(0.0, ex), z.clamp(0.0, ez));
        let out = if held { (x - cx).abs().max((z - cz).abs()) } else { 0.0 };
        let inside = mask.as_ref().map_or(0.0, |m| m.depth(cx, cz));
        let h = if held || inside > 0.0 { height(cx, cz) } else { height(x, z) };
        h - (RING_DROP * (out + inside) / unit) as f32
    };
    let own = Grid::sample(nx, nz, -o, -o, step, &ring);

    // The material with its border (at most what the height map has), its normal map from the heights over it, its alpha map, its box.
    let border = project.border_m();
    let material = scratch
        .find(t.terrain_entity)
        .and_then(|e| match &e.payload {
            Payload::Terrain(tr) => tr.material.clone(),
            _ => None,
        })
        .map(|m| {
            let alpha = files
                .as_deref()
                .zip(project.entry.as_ref())
                .and_then(|(f, e)| game_alpha(e, (project.info.cells_x.max(1), project.info.cells_y.max(1)), f));
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pixel_mixes_as_the_blend_pass_does() {
        let mut slots = [[0.0f32; 3]; 16];
        (slots[0], slots[1], slots[4]) = ([1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]);

        // One layer, its selector at the first variation: that texture alone.
        assert_eq!(blended(&[255, 0, 0, 0], &[0, 0, 0, 0], &slots), [1.0, 0.0, 0.0]);

        // A selector halfway between the first two variations (a sixth of the way up: it reads 0 to 3).
        let c = blended(&[255, 0, 0, 0], &[43, 0, 0, 0], &slots);
        assert!((c[0] - 0.494).abs() < 0.01 && (c[1] - 0.506).abs() < 0.01, "{c:?}");

        // Two equal layers: their weights have unit length, not unit sum, as ps30_blend's.
        let c = blended(&[255, 255, 0, 0], &[0, 0, 0, 0], &slots);
        assert!((c[0] - 0.707).abs() < 0.001 && (c[2] - 0.707).abs() < 0.001, "{c:?}");
    }

    #[test]
    fn a_slot_without_a_texture_has_the_mean_of_the_others() {
        let mut m = TerrainMaterial::default();
        (m.diffuse[0], m.diffuse[1]) = ("a".to_owned(), "b".to_owned());
        let slots = slot_colours(&m, &|p| Some(if p == "a" { [1.0, 0.0, 0.0] } else { [0.0, 1.0, 0.0] }));
        assert_eq!(slots[1], [0.0, 1.0, 0.0]);
        assert_eq!(slots[7], [0.5, 0.5, 0.0]);
    }

    #[test]
    fn a_mask_rows_count_from_the_tiles_minus_z_edge_in_map_terms() {
        // 2 x 2 cells, row 0 (v 0..128: z 128..256 of the tile's frame) "01", row 1 "11": the cell at x < 128, z > 128 is left out.
        let m = Mask { w: 2, h: 2, cells: vec![false, true, true, true] };
        assert_eq!(m.depth(64.0, 64.0), 0.0);
        assert_eq!(m.depth(200.0, 200.0), 0.0);
        assert_eq!(m.depth(64.0, 200.0), 64.0);
        // On the edge with a cell of the tile: not inside.
        assert_eq!(m.depth(128.0, 200.0), 0.0);
    }

    #[test]
    fn a_grid_is_read_between_its_vertices_and_held_at_its_edge() {
        let g = Grid { nx: 2, nz: 2, ox: 10.0, oz: 20.0, step: 2.0, heights: Arc::new(vec![0.0, 2.0, 4.0, 6.0]) };
        assert_eq!(g.height(11.0, 21.0), 3.0);
        assert_eq!(g.height(100.0, 20.0), 2.0);
    }
}
