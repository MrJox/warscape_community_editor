//! The open tile in the context of a battle map (`wce_tile::battle_map::context`): the tiles around it (their terrain with the game's terrain
//! shaders, buildings, trees, decals, lights, effects, rivers) and the map's low-frequency ground, in the open tile's own frame, streamed in
//! chunks around a focus point (the camera's). Read-only: nothing of it can be picked or edited.
//!
//! As the game draws a battle: one LF height map for the whole map (`LfMap`, the map's LF laid into the open frame) under every terrain, each
//! tile's own heights in its vertices. So a placement is its tile's heights buffer and material turned and moved by a world matrix (the blend
//! shader turns the maps with its rotation index), and each distinct tile is opened once (`template`, cached on disk by `cache`).
//!
//! Chunks of CHUNK x CHUNK map cells own the placements whose origin cell they hold and the cells nobody covers; each is drawn at a level of
//! detail by its distance from the focus (`stream`): near everything, mid the tiles' coarse heights, low-resolution material, buildings and
//! rivers, far the LF ground alone (a placement's rectangle of it). Every cell has one owner at any level, so the levels never overlap.
//!
//! Heights (ASSUMPTION, see the module docs of `context` in wce_tile for the game's formula): the game puts every tile on the map's LF
//! (`LF * 3000 - 20 + the tile's height`). The open tile's ground is drawn on it too, relative to the mean LF under it (its objects and
//! picking keep its own heights: off by the LF's variation under it, which the report gives); the neighbours' objects are lifted by the LF at
//! their position.

pub mod cache;
pub mod stream;
pub mod template;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use rayon::prelude::*;
use wce_foundation::math::{Float3, Float4x4};
use wce_scene::render_scene::{LfMap, Payload, RenderScene, Terrain, TerrainImage, TerrainMaterial, Water, WaterVertex};
use wce_tile::battle_map::context::{Affine2, CELL, LF_HEIGHT, MapPlacement, PlacedMap, Spot, VERTICAL_OFFSET};

use super::{MATERIAL_IDS, NORMAL_IDS, next_id};
use crate::terrain_bake::TerrainLook;
use template::{BOX_ABOVE, BOX_BELOW, Template};

pub use stream::{ChunkChange, ChunkKey, Lod, Streamer};

/// Map cells a side of a chunk.
pub const CHUNK: i32 = 8;

/// The most texels a side of the LF normal map.
const LF_NORMAL_SIDE: i32 = 4096;

/// How far the plain ground's blend box reaches past its rectangle (scene units): next to the tiles of a chunk, and in the far ring (where
/// the depth the blend shader reads its positions from is coarser: a thinner margin left lines of pixels to no box, drawn black).
const PLAIN_MARGIN: f64 = 2.0;
const FAR_MARGIN: f64 = 16.0;

/// What to build.
#[derive(Clone)]
pub struct ContextRequest {
    pub map: Arc<PlacedMap>,
    pub spot: Spot,

    /// The open tile's size in cells and its scene units per metre.
    pub cells: (i32, i32),
    pub unit: f64,

    /// The rings around the focus, in cells: everything, the mid level, the LF ground.
    pub near: i32,
    pub mid: i32,
    pub far: i32,

    /// The kit's raw_data folder (the neighbours' tile folders).
    pub raw_root: PathBuf,
    pub look: TerrainLook,
    pub trees: bool,

    /// The material the LF ground's plain one is made from (the open tile's; else the first neighbour's).
    pub plain: Option<Arc<TerrainMaterial>>,

    /// The folder of the cached templates (none: always open the tiles).
    pub cache: Option<PathBuf>,
}

/// A tile location and variation.
pub type TileKey = (String, String);

/// A heights buffer the terrains of a size share.
type Heights = Arc<Vec<f32>>;

/// The tile folder of a placement: `<raw_data>/<location>/<variation>`.
pub fn tile_dir(raw_root: &Path, location: &str, variation: &str) -> PathBuf {
    let mut p = raw_root.to_path_buf();
    for part in location.split(['\\', '/']).filter(|s| !s.is_empty()) {
        p.push(part);
    }

    p.join(variation)
}

/// A 4 x 4 of the frame transform: x' = a x + b z + c, z' = d x + e z + f, y + dy.
fn matrix(a: &Affine2, dy: f32) -> Float4x4 {
    let m = &a.m;
    let mut r = Float4x4::IDENTITY;
    r.m[0] = [m[0][0] as f32, 0.0, m[0][1] as f32, m[0][2] as f32];
    r.m[1][3] = dy;
    r.m[2] = [m[1][0] as f32, 0.0, m[1][1] as f32, m[1][2] as f32];
    r
}

/// The bounding rectangle of a frame rectangle's image: x0, z0, x1, z1.
fn bounds(a: &Affine2, x0: f64, z0: f64, x1: f64, z1: f64) -> [f64; 4] {
    let mut b = [f64::MAX, f64::MAX, f64::MIN, f64::MIN];
    for (x, z) in [(x0, z0), (x1, z0), (x0, z1), (x1, z1)] {
        let (px, pz) = a.apply(x, z);
        b = [b[0].min(px), b[1].min(pz), b[2].max(px), b[3].max(pz)];
    }

    b
}

/// The x, z of an entity's position.
fn position(m: &Float4x4) -> (f64, f64) {
    (f64::from(m.m[0][3]), f64::from(m.m[2][3]))
}

/// The map's LF in the open frame, on the LF's own lattice (the frame's axes are the map's turned by whole quarter turns, so its samples are
/// the map's pixels): heights relative to the mean LF under the open tile.
fn lf_map(map: &PlacedMap, from_map: &Affine2, to_map: &Affine2, lf_mean: f64, unit: f64) -> Option<LfMap> {
    let f = map.lf.as_ref().filter(|f| f.w > 1 && f.h > 1)?;
    let (ppx, ppy) = (f64::from(f.w) / f64::from(map.width), f64::from(f.h) / f64::from(map.height));
    let s = CELL / ppx;
    let b = bounds(from_map, 0.0, 0.0, f64::from(f.w - 1) / ppx, f64::from(f.h - 1) / ppy);
    let (nx, nz) = (((b[2] - b[0]) / s).round() as i32 + 1, ((b[3] - b[1]) / s).round() as i32 + 1);
    let mut values = vec![0u16; (nx * nz) as usize];
    values.par_chunks_mut(nx as usize).enumerate().for_each(|(j, row)| {
        for (i, v) in row.iter_mut().enumerate() {
            let (cx, cy) = to_map.apply(b[0] + i as f64 * s, b[1] + j as f64 * s);
            let (px, py) = ((cx * ppx).clamp(0.0, f64::from(f.w - 1)), (cy * ppy).clamp(0.0, f64::from(f.h - 1)));
            let (x0, y0) = (px.floor() as u32, py.floor() as u32);
            let (x1, y1) = ((x0 + 1).min(f.w - 1), (y0 + 1).min(f.h - 1));
            let (fx, fy) = (px - f64::from(x0), py - f64::from(y0));
            let at = |x: u32, y: u32| f64::from(f.values[(y * f.w + x) as usize]);
            let top = at(x0, y0) * (1.0 - fx) + at(x1, y0) * fx;
            let bottom = at(x0, y1) * (1.0 - fx) + at(x1, y1) * fx;
            *v = (top * (1.0 - fy) + bottom * fy).round() as u16;
        }
    });

    let mut lf = LfMap {
        x0: b[0] as f32, z0: b[1] as f32, step: s as f32, nx, nz, values, scale: (LF_HEIGHT / unit) as f32, offset: ((VERTICAL_OFFSET + lf_mean) / unit) as f32,
        id: next_id(&NORMAL_IDS), ..Default::default()
    };

    lf_normals(&mut lf);
    Some(lf)
}

/// The LF map's normals as the blend shader reads `s_lf_normal_map`: at uv = (x, -z) / normal_world, wrapping, so the image lays the map's
/// normals around a torus of that size; .a = x, .g = -z of the unit normal * 0.5 + 0.5.
fn lf_normals(lf: &mut LfMap) {
    let k = ((lf.nx.max(lf.nz) + LF_NORMAL_SIDE - 1) / LF_NORMAL_SIDE).max(1);
    let (w, h) = ((lf.nx + k - 1) / k, (lf.nz + k - 1) / k);
    let s = lf.step * k as f32;
    let (wx, wz) = (w as f32 * s, h as f32 * s);
    let mut px = vec![0u8; (w * h * 4) as usize];
    let lfr = &*lf;
    px.par_chunks_mut((w * 4) as usize).enumerate().for_each(|(r, row)| {
        // The texel's centre wraps to a point of the map: x = u * wx, z = -v * wz, give or take whole turns.
        let v = (r as f32 + 0.5) / h as f32;
        let z = lfr.z0 + (-v * wz - lfr.z0).rem_euclid(wz);
        for (c, out) in row.chunks_mut(4).enumerate() {
            let u = (c as f32 + 0.5) / w as f32;
            let x = lfr.x0 + (u * wx - lfr.x0).rem_euclid(wx);
            let dx = (lfr.height_at(x + s, z) - lfr.height_at(x - s, z)) / (2.0 * s);
            let dz = (lfr.height_at(x, z + s) - lfr.height_at(x, z - s)) / (2.0 * s);
            let l = (dx * dx + dz * dz + 1.0).sqrt();
            let q = |v: f32| ((v * 0.5 + 0.5).clamp(0.0, 1.0) * 255.0).round() as u8;
            out.copy_from_slice(&[255, q(dz / l), 0, q(-dx / l)]);
        }
    });

    lf.normal = TerrainImage { w: w as u32, h: h as u32, rgba: Some(Arc::new(px)) };
    lf.normal_world = [wx, wz];
}

/// A material of one colour: `base` with its blend and index maps averaged into one pixel and a flat normal (the LF map's normals shade it).
fn plain_material(base: &TerrainMaterial, id: u64, normal_id: u64) -> TerrainMaterial {
    let average = |img: &TerrainImage| -> TerrainImage {
        let Some(px) = &img.rgba else { return img.clone() };
        let mut sum = [0u64; 4];
        for p in px.as_chunks::<4>().0 {
            for k in 0..4 {
                sum[k] += u64::from(p[k]);
            }
        }

        let n = (px.len() / 4).max(1) as u64;
        TerrainImage { w: 1, h: 1, rgba: Some(Arc::new(sum.iter().map(|v| (v / n) as u8).collect())) }
    };

    let mut m = base.clone();
    m.blend = average(&base.blend);
    m.index = average(&base.index);
    m.normal = TerrainImage { w: 1, h: 1, rgba: Some(Arc::new(vec![255, 127, 0, 127])) };
    m.alpha = TerrainImage::default();
    (m.y0, m.y1) = (-BOX_BELOW, BOX_ABOVE);
    m.id = id;
    m.normal_id = normal_id;
    m
}

/// The map seen from the open tile: its frame, its LF map, the placements by chunk and the cells they cover.
pub struct Frame {
    pub map: Arc<PlacedMap>,
    pub unit: f64,

    /// The open tile's cells, its frame -> map cells and back.
    pub open: [i32; 4],
    pub to_map: Affine2,
    pub from_map: Affine2,

    /// The mean and range of the LF under the open tile (metres).
    pub lf_mean: f64,
    pub lf_range: f64,
    pub lf: Option<Arc<LfMap>>,

    /// The placements by the chunk of their origin cell (those over the open tile's cells left out); who draws each cell (row by row: 0
    /// nobody, 1 the open tile, else the chunk of the placement over it: `owner_of`); the cells a tile reaches past its origin at most.
    pub by_chunk: HashMap<ChunkKey, Vec<usize>>,
    owner: Vec<u32>,
    reach: i32,

    /// The plain material's base and the LF ground's shared pieces: a heights buffer of zeros per grid size (every rectangle of that size
    /// draws it), the plain material per rectangle size (one id: one set of maps on the GPU).
    plain_base: Mutex<Option<Arc<TerrainMaterial>>>,
    plain_id: u64,
    plain_normal_id: u64,
    zeros: Mutex<HashMap<(i32, i32), Heights>>,
    plains: Mutex<HashMap<(i32, i32, bool), Arc<TerrainMaterial>>>,
}

impl Frame {

    /// The frame of a request (slow on the campaign map: its LF map; run it away from the UI thread).
    pub fn new(rq: &ContextRequest) -> Self {
        let map = rq.map.clone();
        let me = MapPlacement { location: String::new(), variation: String::new(), x: rq.spot.x, y: rq.spot.y, w: rq.cells.0, h: rq.cells.1, rotation: rq.spot.rotation, flipped: false };
        let open = me.rect();
        let to_map = me.to_map();
        let from_map = to_map.inverse();
        let (lf_mean, lf_range) = map.lf_stats(open);
        let lf = lf_map(&map, &from_map, &to_map, lf_mean, rq.unit).map(Arc::new);

        // The first to cover a cell owns it (the open tile, then the placements in the map's order).
        let (w, h) = (map.width as i32, map.height as i32);
        let cw = (w + CHUNK - 1) / CHUNK;
        let mut owner = vec![0u32; (w.max(0) * h.max(0)) as usize];
        let mut mark = |r: [i32; 4], who: u32| {
            for y in r[1].max(0)..r[3].min(h) {
                for x in r[0].max(0)..r[2].min(w) {
                    let o = &mut owner[(y * w + x) as usize];
                    if *o == 0 {
                        *o = who;
                    }
                }
            }
        };

        mark(open, 1);
        let mut by_chunk: HashMap<ChunkKey, Vec<usize>> = HashMap::new();
        let mut reach = 0;
        for (i, p) in map.placements.iter().enumerate() {
            let r = p.rect();
            if r[0] < open[2] && open[0] < r[2] && r[1] < open[3] && open[1] < r[3] {
                continue;
            }

            let key = (p.x.div_euclid(CHUNK), p.y.div_euclid(CHUNK));
            mark(r, (key.1 * cw + key.0) as u32 + 2);
            reach = reach.max(r[2] - r[0]).max(r[3] - r[1]);
            by_chunk.entry(key).or_default().push(i);
        }

        Self {
            map, unit: rq.unit, open, to_map, from_map, lf_mean, lf_range, lf, by_chunk, owner, reach, plain_base: Mutex::new(rq.plain.clone()),
            plain_id: next_id(&MATERIAL_IDS), plain_normal_id: next_id(&NORMAL_IDS), zeros: Mutex::default(), plains: Mutex::default(),
        }
    }

    /// The LF height under a frame point relative to the open tile (scene units).
    pub fn lift(&self, x: f64, z: f64) -> f32 {
        self.lf.as_ref().map_or(0.0, |lf| lf.height_at(x as f32, z as f32))
    }

    /// The map cell under a frame point.
    pub fn cell_of(&self, x: f64, z: f64) -> (f64, f64) {
        self.to_map.apply(x, z)
    }

    /// The chunks of the map.
    pub fn chunks(&self) -> (i32, i32) {
        ((self.map.width as i32 + CHUNK - 1) / CHUNK, (self.map.height as i32 + CHUNK - 1) / CHUNK)
    }

    /// A base for the plain material, when there is none yet.
    pub fn offer_plain(&self, m: &Option<Arc<TerrainMaterial>>) {
        let mut base = self.plain_base.lock().unwrap_or_else(|p| p.into_inner());
        if base.is_none() {
            base.clone_from(m);
        }
    }

    /// The cells of an area for which `take` holds, as rectangles: rows cut into runs, a run grown down while the rows below have it.
    fn runs(&self, area: [i32; 4], take: impl Fn(u32, i32, i32) -> bool) -> Vec<[i32; 4]> {
        let (w, h) = (self.map.width as i32, self.map.height as i32);
        let area = [area[0].max(0), area[1].max(0), area[2].min(w), area[3].min(h)];
        let (aw, ah) = ((area[2] - area[0]).max(0) as usize, (area[3] - area[1]).max(0) as usize);
        let mut free: Vec<bool> = (0..aw * ah).map(|k| {
            let (x, y) = (area[0] + (k % aw) as i32, area[1] + (k / aw) as i32);
            take(self.owner[(y * w + x) as usize], x, y)
        }).collect();

        let mut out = vec![];
        for y in 0..ah {
            let mut x = 0;
            while x < aw {
                if !free[y * aw + x] {
                    x += 1;
                    continue;
                }

                let x0 = x;
                while x < aw && free[y * aw + x] {
                    x += 1;
                }

                let mut y1 = y + 1;
                while y1 < ah && (x0..x).all(|k| free[y1 * aw + k]) {
                    y1 += 1;
                }

                for yy in y..y1 {
                    for k in x0..x {
                        free[yy * aw + k] = false;
                    }
                }

                out.push([area[0] + x0 as i32, area[1] + y as i32, area[0] + x as i32, area[1] + y1 as i32]);
            }
        }

        out
    }

    /// The owner value of a chunk's placements in `owner`.
    fn owner_of(&self, key: ChunkKey) -> u32 {
        (key.1 * self.chunks().0 + key.0) as u32 + 2
    }

    /// The cells of a chunk nobody covers.
    fn uncovered(&self, key: ChunkKey) -> Vec<[i32; 4]> {
        self.runs([key.0 * CHUNK, key.1 * CHUNK, (key.0 + 1) * CHUNK, (key.1 + 1) * CHUNK], |o, _, _| o == 0)
    }

    /// The cells a chunk draws: its own nobody covers and those of its placements (which reach out of it by up to the largest tile).
    fn owned(&self, key: ChunkKey) -> Vec<[i32; 4]> {
        let (me, c) = (self.owner_of(key), [key.0 * CHUNK, key.1 * CHUNK, (key.0 + 1) * CHUNK, (key.1 + 1) * CHUNK]);
        let inside = |x: i32, y: i32| x >= c[0] && x < c[2] && y >= c[1] && y < c[3];
        self.runs([c[0], c[1], c[2] + self.reach, c[3] + self.reach], |o, x, y| o == me || (o == 0 && inside(x, y)))
    }

    /// The LF ground over a rectangle of map cells: a grid of zero heights on the LF's lattice (the shader adds the LF), the plain material.
    fn ground(&self, r: [i32; 4], far: bool) -> (Float4x4, Payload) {
        let b = bounds(&self.from_map, f64::from(r[0]), f64::from(r[1]), f64::from(r[2]), f64::from(r[3]));
        let step = self.lf.as_ref().map_or(CELL / 4.0, |lf| f64::from(lf.step));
        let (nx, nz) = (((b[2] - b[0]) / step).round() as i32 + 1, ((b[3] - b[1]) / step).round() as i32 + 1);
        let heights = self.zeros.lock().unwrap_or_else(|p| p.into_inner()).entry((nx, nz)).or_insert_with(|| Arc::new(vec![0.0; (nx * nz) as usize])).clone();
        let base = self.plain_base.lock().unwrap_or_else(|p| p.into_inner()).clone();
        let material = base.map(|base| {
            self.plains.lock().unwrap_or_else(|p| p.into_inner()).entry((nx, nz, far)).or_insert_with(|| {
                // A little past its rectangle (one colour: its maps wrap onto themselves), so two of them side by side leave no pixel to
                // neither (the resolve would draw it black).
                // One id and one normal id for all sizes: one set of maps on the GPU (a normal id per size had the renderer make the normal
                // map again every time the next terrain's size differed).
                let mut m = plain_material(&base, self.plain_id, self.plain_normal_id);
                let (w, h) = (f64::from(nx - 1) * step, f64::from(nz - 1) * step);
                let e = if far { FAR_MARGIN } else { PLAIN_MARGIN };
                (m.x0, m.z0, m.x1, m.z1) = (-e, -e, w + e, h + e);
                Arc::new(m)
            }).clone()
        });

        let t = Terrain {
            nx, nz, origin_x: 0.0, origin_z: 0.0, step: step as f32, heights, lod: true, tile_metres: 32.0, material, lf: self.lf.clone(), legacy: false, ..Default::default()
        };
        let mut world = Float4x4::IDENTITY;
        (world.m[0][3], world.m[2][3]) = (b[0] as f32, b[1] as f32);
        (world, Payload::Terrain(t))
    }

    /// The sea: one plane at 0 m over the cells within `reach` of a frame point (ASSUMPTION: the battle sea is at 0 m; the LF's lowest value is
    /// -vertical_offset, -20 m, under the open sea), drawn relative to the open tile like the rest. Where the ground is higher it hides it.
    pub fn sea(&self, at: (f64, f64), reach: i32) -> (Float4x4, Payload) {
        let (cx, cy) = self.cell_of(at.0, at.1);
        let r = f64::from(reach);
        let c = |v: f64, n: u32| v.clamp(0.0, f64::from(n));
        let b = bounds(&self.from_map, c(cx - r, self.map.width), c(cy - r, self.map.height), c(cx + r, self.map.width), c(cy + r, self.map.height));
        let y = (-self.lf_mean / self.unit) as f32;
        let v = |x: f64, z: f64| WaterVertex { x: x as f32, y, z: z as f32, u: x as f32, v: z as f32, fade: 1.0, ..Default::default() };
        // Its version follows its rectangle (the renderer lays a plane's chunks out again when it changes).
        let version = b.iter().fold(0xCBF2_9CE4_8422_2325u64, |h, v| (h ^ v.to_bits()).wrapping_mul(0x0100_0000_01B3));
        let sea = Water { river: false, foam: 0.0, verts: vec![v(b[0], b[1]), v(b[2], b[1]), v(b[0], b[3]), v(b[2], b[3])], indices: vec![0, 2, 1, 1, 2, 3], version };
        (Float4x4::IDENTITY, Payload::Water(sea))
    }

    /// A placement drawn from its tile at a level: its terrain (the tile's heights buffer and material, turned and moved), then its entities
    /// moved into the frame and lifted by the LF at their position.
    fn place(&self, i: usize, t: &Template, lod: Lod, out: &mut Vec<(Float4x4, Payload)>) {
        let a = self.from_map.then_after(&self.map.placements[i].to_map());
        let frame = matrix(&a, 0.0);
        let (grid, material) = if lod == Lod::Near { (&t.near, &t.material) } else { (&t.coarse, &t.material_low) };
        let terrain = Terrain {
            nx: grid.nx, nz: grid.nz, origin_x: grid.ox, origin_z: grid.oz, step: grid.step, heights: grid.heights.clone(), lod: true, material: material.clone(), lf: self.lf.clone(),
            legacy: false, ..Default::default()
        };

        out.push((frame, Payload::Terrain(terrain)));
        let n = if lod == Lod::Near { t.entities.len() } else { t.mid };
        for (w, payload) in &t.entities[..n] {
            // The renderer draws water vertices as they are (no world matrix): moved into the frame here, lifted per vertex.
            if let Payload::Water(water) = payload {
                let m = frame * *w;
                let mut water = water.clone();
                for v in &mut water.verts {
                    let q = m.transform_point(Float3::new(v.x, v.y, v.z));
                    (v.x, v.y, v.z) = (q.x, q.y + self.lift(f64::from(q.x), f64::from(q.z)), q.z);
                }

                out.push((Float4x4::IDENTITY, Payload::Water(water)));
                continue;
            }

            let mut world = frame * *w;
            let (x, z) = position(&world);
            world.m[1][3] += self.lift(x, z);
            out.push((world, payload.clone()));
        }
    }

    /// A chunk at a level: its placements from their tiles (`template` gives a placement's tile, none when it could not be opened: its
    /// rectangle gets LF ground), and LF ground over its cells nobody covers.
    pub fn build_chunk(&self, key: ChunkKey, lod: Lod, template: &dyn Fn(usize) -> Option<Arc<Template>>) -> Vec<(Float4x4, Payload)> {
        // Far: the LF ground over everything it draws, in as few rectangles as its cells make.
        if lod == Lod::Far {
            return self.owned(key).into_iter().map(|r| self.ground(r, true)).collect();
        }

        let mut out = vec![];
        for &i in self.by_chunk.get(&key).map_or(&[][..], Vec::as_slice) {
            match template(i) {
                Some(t) => self.place(i, &t, lod, &mut out),
                None => out.push(self.ground(self.map.placements[i].rect(), false)),
            }
        }

        for r in self.uncovered(key) {
            out.push(self.ground(r, false));
        }

        out
    }

    /// A line for the report: the LF under the open tile.
    pub fn lf_note(&self) -> String {
        format!(
            "LF under the tile {:.1} m mean, {:.1} m range{}", self.lf_mean, self.lf_range,
            if self.lf_range > 2.0 { format!(": its objects may be off the ground by up to {:.1} m", self.lf_range * 0.5 / self.unit) } else { String::new() }
        )
    }
}

/// Puts entities on a layer of the scene (cleared first); they can't be picked.
pub fn publish(scene: &mut RenderScene, layer: wce_scene::render_scene::LayerId, entities: Vec<(Float4x4, Payload)>, doc: wce_scene::render_scene::DocumentId) {
    scene.clear_layer(layer);
    scene.reserve(entities.len());
    for (world, payload) in entities {
        let e = scene.add(layer, world, payload, wce_scene::render_scene::SourceRef { doc, item: u64::MAX });
        if let Some(en) = scene.find_mut(e) {
            en.locked = true;
        }
    }
}
