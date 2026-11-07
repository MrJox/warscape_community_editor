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
//! rivers, ground the same without the buildings and with a smaller material, far the LF ground alone (a placement's rectangle of it), and
//! past the rings blocks of BLOCK x BLOCK chunks in one piece, with their tiles' ground merged out to the ground ring and the LF past it. Every cell has one owner at any level, so the levels never overlap.
//!
//! Heights (ASSUMPTION, see the module docs of `context` in wce_tile for the game's formula): the game puts every tile on the map's LF
//! (`LF * 3000 - 20 + the tile's height`). The open tile's ground is drawn on it too, relative to the median LF under it (its objects and
//! picking keep its own heights: off by the LF's variation under it, which the report gives); the neighbours' objects are lifted by the LF at
//! their position.

pub mod cache;
pub mod custom_mesh;
pub mod stream;
pub mod template;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use rayon::prelude::*;
use wce_foundation::math::{Float3, Float4x4};
use wce_scene::render_scene::{LfMap, LfSamples, Payload, RenderScene, Terrain, TerrainImage, TerrainMaterial, Water, WaterVertex};
use wce_tile::battle_map::context::{Affine2, CELL, LfField, MapPlacement, PlacedMap, SEA_LF_HEIGHT, Spot};

use super::{MATERIAL_IDS, NORMAL_IDS, next_id};
use crate::terrain_bake::TerrainLook;
use crate::terrain_material::{retexture, terrain_normal_image};
use wce_formats::tile_climate::ClimateList;

use crate::climate_db::effective_climates;
use wce_formats::tile_entry::TileTextureGroup;
use wce_vfs::files::Files;
use template::{BOX_ABOVE, BOX_BELOW, Grid, SKIRT, Template, mean_of_material};

pub use stream::{ChunkChange, ChunkKey, Lod, Streamer};
pub use wce_tile::sources::tile_dir;

/// Map cells a side of a chunk.
pub const CHUNK: i32 = 8;

/// Chunks a side of a block ([`Lod::Block`], [`Lod::Merged`]).
pub const BLOCK: i32 = 8;

/// Texels of a block's colour a cell.
const BLOCK_TEXELS: i32 = 4;

/// The sea bed in a block, as a baked material stores it: dark, as the sea tiles' own ground is. ASSUMPTION: one colour for all of them.
const SEA_BED: [u8; 4] = [46, 54, 66, 255];

/// The most texels a side of the LF normal map.
const LF_NORMAL_SIDE: i32 = 4096;

/// How far the plain ground's blend box reaches past its rectangle (scene units): next to the tiles of a chunk, and in the far ring (where
/// the depth the blend shader reads its positions from is coarser: a thinner margin left lines of pixels to no box, drawn black).
const PLAIN_MARGIN: f64 = 2.0;
const FAR_MARGIN: f64 = 16.0;

/// Whether a tile location is of the `sea` set (`terrain\tiles\battle\sea\...`; not `sea_coast` and the others).
fn is_sea_set(location: &str) -> bool {
    location.split(['\\', '/']).filter(|s| !s.is_empty()).nth(3).is_some_and(|set| set.eq_ignore_ascii_case("sea"))
}

/// How far above the sea (metres) the land LF under a sea tile may be for the tile to go on the sea's LF (`Frame::on_sea_lf`): the cliff
/// coasts' sea tiles' medians are -14 .. -20 m, TDD's Long Lake's 626 m.
const SEA_LF_REACH: f64 = 10.0;

/// How deep a custom-mesh tile's floor lies under the sea (metres): see `Frame::place`.
const FLOOR_DEPTH: f64 = 40.0;

/// What to build.
#[derive(Clone)]
pub struct ContextRequest {
    pub map: Arc<PlacedMap>,
    pub spot: Spot,

    /// The open tile's size in cells and its scene units per metre. No cells (0, 0): the map's own frame (the Terrain tool), its origin at
    /// `spot` with the map's axes (x along the columns, z up the rows), nothing left out and the heights measured from the sea.
    pub cells: (i32, i32),
    pub unit: f64,

    /// The rings around the focus, in cells: everything, the mid level, the tiles' ground, the LF ground.
    pub near: i32,
    pub mid: i32,
    pub ground: i32,
    pub far: i32,

    /// The kit's raw_data folder (the tile database's settings).
    pub raw_root: PathBuf,

    /// The raw_data folders a placement's tile is looked for in, the first that has it wins (`wce_tile::sources`: the projects, then the
    /// Assembly Kits), then the `provider` (the packs); without a provider `raw_root` after them.
    pub tile_roots: Vec<PathBuf>,

    /// The game's Assembly Kit with the tiles (`TileProject::game_kit`): the neighbours' trees are generated with its parameters when
    /// `raw_root`'s kit has none (the projects' kit layout).
    pub game_kit: Option<PathBuf>,
    pub look: TerrainLook,
    pub trees: bool,

    /// The ground ring's material is baked (`Template::bake_far`): the renderer has a blend pass for it (not Rome 2's).
    pub bake_ground: bool,

    /// The material the LF ground's plain one is made from (the open tile's; else the first neighbour's), and its tile's texture groups
    /// (to give it other climates' textures).
    pub plain: Option<Arc<TerrainMaterial>>,
    pub groups: Option<TileTextureGroup>,

    /// The folder of the cached templates (none: always open the tiles).
    pub cache: Option<PathBuf>,

    /// The fallback when no root has a placement's tile: makes its folder from the packs (decompiled into the kit's layout) from its
    /// location and variation.
    pub provider: Option<TileProvider>,
}

/// Makes a tile's folder from its location and variation (see [`ContextRequest::provider`]).
pub type TileProvider = Arc<dyn Fn(&str, &str) -> Result<PathBuf, String> + Send + Sync>;

/// A tile location, variation and climate.
pub type TileKey = (String, String, String);

/// A plain material's base and its id.
type PlainBase = (Option<Arc<TerrainMaterial>>, u64);

/// The plain materials by grid size, far ring or not, and climate.
type Plains = HashMap<(i32, i32, bool, u8), Arc<TerrainMaterial>>;

/// What a block is built from: a placement's tile (none when it could not be opened), whether a placement's tile is a custom-mesh one, and
/// a ground texture's mean colour.
pub struct BlockSource<'a> {
    pub template: &'a dyn Fn(usize) -> Option<Arc<Template>>,
    pub is_custom: &'a dyn Fn(usize) -> bool,
    pub mean: &'a dyn Fn(&str) -> Option<[f32; 3]>,
}

/// A cell no placement gives a climate.
const NO_CLIMATE: u8 = u8::MAX;

/// A heights buffer the terrains of a size share.
type Heights = Arc<Vec<f32>>;

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

/// A mesh instance's POSITION5..7 as a tree's untilted matrix (rows), when they are one: a 3 x 3 part that is not degenerate.
fn untilted(rows: &[[f32; 4]; 3]) -> Option<Float4x4> {
    let r = |i: usize, j: usize| f64::from(rows[i][j]);
    let det = r(0, 0) * (r(1, 1) * r(2, 2) - r(1, 2) * r(2, 1)) - r(0, 1) * (r(1, 0) * r(2, 2) - r(1, 2) * r(2, 0)) + r(0, 2) * (r(1, 0) * r(2, 1) - r(1, 1) * r(2, 0));
    if det.abs() < 1e-9 {
        return None;
    }

    let mut m = Float4x4::IDENTITY;
    m.m[..3].copy_from_slice(rows);
    Some(m)
}

/// The x, z of an entity's position.
fn position(m: &Float4x4) -> (f64, f64) {
    (f64::from(m.m[0][3]), f64::from(m.m[2][3]))
}

/// A field of the map (its LF, its sea LF) in the open frame, on the field's own lattice (the frame's axes are the map's turned by whole
/// quarter turns, so its samples are the map's pixels): `LF * height - VERTICAL_OFFSET` m, relative to the median LF under the open tile.
fn lf_map(map: &PlacedMap, f: Option<&LfField>, height: f64, from_map: &Affine2, to_map: &Affine2, lf_level: f64, unit: f64) -> Option<LfMap> {
    let vertical_offset = map.vertical_offset;
    let f = f.filter(|f| f.w > 1 && f.h > 1)?;
    let (ppx, ppy) = (f64::from(f.w) / f64::from(map.width), f64::from(f.h) / f64::from(map.height));
    let s = CELL / ppx;
    let b = bounds(from_map, 0.0, 0.0, f64::from(f.w - 1) / ppx, f64::from(f.h - 1) / ppy);
    let (nx, nz) = (((b[2] - b[0]) / s).round() as i32 + 1, ((b[3] - b[1]) / s).round() as i32 + 1);
    // The frame lays the field's samples out one to one (the Terrain tool's: the map's own frame, rows the other way up): the field's
    // samples themselves, shared (the campaign's LF is 375 MB), else resampled.
    let sample = |i: f64, j: f64| { let (cx, cy) = to_map.apply(b[0] + i * s, b[1] + j * s); (cx * ppx, cy * ppy) };
    let near = |a: (f64, f64), b: (f64, f64)| (a.0 - b.0).abs() < 1e-3 && (a.1 - b.1).abs() < 1e-3;
    let same_size = u32::try_from(nx).ok() == Some(f.w) && u32::try_from(nz).ok() == Some(f.h);
    let last_y = f64::from(f.h - 1);
    let shared = if !same_size {
        None
    } else if near(sample(0.0, 0.0), (0.0, last_y)) && near(sample(1.0, 0.0), (1.0, last_y)) && near(sample(0.0, 1.0), (0.0, last_y - 1.0)) {
        Some(true)
    } else if near(sample(0.0, 0.0), (0.0, 0.0)) && near(sample(1.0, 0.0), (1.0, 0.0)) && near(sample(0.0, 1.0), (0.0, 1.0)) {
        Some(false)
    } else {
        None
    };

    if let Some(flip) = shared {
        let mut lf = LfMap {
            x0: b[0] as f32, z0: b[1] as f32, step: s as f32, nx, nz, values: LfSamples::shared(f.values.clone(), flip), scale: (height / unit) as f32,
            offset: ((vertical_offset + lf_level) / unit) as f32, id: next_id(&NORMAL_IDS), ..Default::default()
        };

        lf_normals(&mut lf);
        return Some(lf);
    }

    let field = f.values.read();
    let mut values = vec![0u16; (nx * nz) as usize];
    values.par_chunks_mut(nx as usize).enumerate().for_each(|(j, row)| {
        for (i, v) in row.iter_mut().enumerate() {
            let (cx, cy) = to_map.apply(b[0] + i as f64 * s, b[1] + j as f64 * s);
            let (px, py) = ((cx * ppx).clamp(0.0, f64::from(f.w - 1)), (cy * ppy).clamp(0.0, f64::from(f.h - 1)));
            let (x0, y0) = (px.floor() as u32, py.floor() as u32);
            let (x1, y1) = ((x0 + 1).min(f.w - 1), (y0 + 1).min(f.h - 1));
            let (fx, fy) = (px - f64::from(x0), py - f64::from(y0));
            let at = |x: u32, y: u32| f64::from(field[(y * f.w + x) as usize]);
            let top = at(x0, y0) * (1.0 - fx) + at(x1, y0) * fx;
            let bottom = at(x0, y1) * (1.0 - fx) + at(x1, y1) * fx;
            *v = (top * (1.0 - fy) + bottom * fy).round() as u16;
        }
    });
    drop(field);

    let mut lf = LfMap {
        x0: b[0] as f32, z0: b[1] as f32, step: s as f32, nx, nz, values: LfSamples::new(values), scale: (height / unit) as f32, offset: ((vertical_offset + lf_level) / unit) as f32,
        id: next_id(&NORMAL_IDS), ..Default::default()
    };

    lf_normals(&mut lf);
    Some(lf)
}

/// The LF map's normals as the blend shader reads `s_lf_normal_map`: at uv = (x, -z) / normal_world, wrapping, so the image lays the map's
/// normals around a torus of that size; .a = x, .g = -z of the unit normal * 0.5 + 0.5.
fn lf_normals(lf: &mut LfMap) {
    lf.normal_k = ((lf.nx.max(lf.nz) + LF_NORMAL_SIDE - 1) / LF_NORMAL_SIDE).max(1);
    let (w, h) = lf.normal_size();
    let s = lf.step * lf.normal_k as f32;
    let mut px = vec![0u8; (w * h * 4) as usize];
    let lfr = &*lf;
    let values = lfr.values.read();
    px.par_chunks_mut((w * 4) as usize).enumerate().for_each(|(r, row)| {
        for (c, out) in row.chunks_mut(4).enumerate() {
            out.copy_from_slice(&lfr.normal_texel(&values, c as i32, r as i32));
        }
    });

    drop(values);
    lf.normal = TerrainImage { w: w as u32, h: h as u32, rgba: Some(Arc::new(px)) };
    lf.normal_world = [w as f32 * s, h as f32 * s];
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

/// What a frame knows of the placements: who owns each cell, each cell's climate, the placements by the chunk of their origin, the most
/// cells one reaches.
struct MapIndex {
    owner: Vec<u32>,
    climate: Vec<u8>,
    by_chunk: HashMap<ChunkKey, Vec<usize>>,
    reach: i32,
}

impl MapIndex {
    fn new(map: &PlacedMap, open: [i32; 4]) -> Self {
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

        // The climates, the same way (the open tile's own placement gives its cells theirs).
        let mut climate = vec![NO_CLIMATE; (w.max(0) * h.max(0)) as usize];
        for p in map.placements.iter().filter(|p| map.climate_of(p).is_some()) {
            let r = p.rect();
            for y in r[1].max(0)..r[3].min(h) {
                for x in r[0].max(0)..r[2].min(w) {
                    let c = &mut climate[(y * w + x) as usize];
                    if *c == NO_CLIMATE {
                        *c = p.climate;
                    }
                }
            }
        }

        let mut by_chunk: HashMap<ChunkKey, Vec<usize>> = HashMap::new();
        let mut reach = 0;
        for (i, p) in map.placements.iter().enumerate() {
            // Not under the open tile; no open tile (the Terrain tool's frame: an empty rectangle at the map's centre) hides nothing.
            let r = p.rect();
            if open[0] < open[2] && open[1] < open[3] && r[0] < open[2] && open[0] < r[2] && r[1] < open[3] && open[1] < r[3] {
                continue;
            }

            let key = (p.x.div_euclid(CHUNK), p.y.div_euclid(CHUNK));
            mark(r, (key.1 * cw + key.0) as u32 + 2);
            reach = reach.max(r[2] - r[0]).max(r[3] - r[1]);
            by_chunk.entry(key).or_default().push(i);
        }

        Self { owner, climate, by_chunk, reach }
    }
}

/// The map seen from the open tile: its frame, its LF map, the placements by chunk and the cells they cover.
pub struct Frame {
    pub map: Arc<PlacedMap>,
    pub unit: f64,

    /// The open tile's cells, its frame -> map cells and back.
    pub open: [i32; 4],
    pub to_map: Affine2,
    pub from_map: Affine2,

    /// The median and range of the LF under the open tile (metres).
    pub lf_level: f64,
    pub lf_range: f64,
    pub lf: Option<Arc<LfMap>>,

    /// The sea tiles' LF (`PlacedMap::sea_lf`): none, the sea tiles on a flat -20 m.
    pub sea_lf: Option<Arc<LfMap>>,

    /// The placements by the chunk of their origin cell (those over the open tile's cells left out); who draws each cell (row by row: 0
    /// nobody, 1 the open tile, else the chunk of the placement over it: `owner_of`); the cells a tile reaches past its origin at most.
    pub by_chunk: HashMap<ChunkKey, Vec<usize>>,
    owner: Vec<u32>,
    reach: i32,

    /// Each cell's climate (row by row, the first placement over it; NO_CLIMATE: none), the database's climates, the look and the
    /// files the plain material is given another climate's textures with.
    climate: Vec<u8>,
    climates: ClimateList,
    pub look: TerrainLook,
    groups: Option<TileTextureGroup>,
    files: Option<Arc<dyn Files>>,

    /// The plain material's base and the LF ground's shared pieces: a heights buffer of zeros per grid size (every rectangle of that size
    /// draws it), the plain material per rectangle size (one id: one set of maps on the GPU).
    plain_base: Mutex<Option<Arc<TerrainMaterial>>>,
    plain_normal_id: u64,
    zeros: Mutex<HashMap<(i32, i32), Heights>>,

    /// Per climate (NO_CLIMATE: the look's): the plain material's base with its textures and its id; the plain material per rectangle
    /// size and climate.
    plain_bases: Mutex<HashMap<u8, PlainBase>>,

    /// The climates' plain colours (`plain_colour`).
    plain_colours: Mutex<HashMap<u8, [u8; 4]>>,
    plains: Mutex<Plains>,
}

impl Frame {

    /// The frame of a request (slow on the campaign map: its LF map; run it away from the UI thread).
    pub fn new(rq: &ContextRequest, files: Option<Arc<dyn Files>>) -> Self {
        let map = rq.map.clone();
        let me = MapPlacement { location: String::new(), variation: String::new(), x: rq.spot.x, y: rq.spot.y, w: rq.cells.0, h: rq.cells.1, rotation: rq.spot.rotation, flipped: false, climate: 0, any_variation: false };
        let open = me.rect();
        let to_map = me.to_map();
        let from_map = to_map.inverse();
        let (lf_level, lf_range) = if rq.cells == (0, 0) { (0.0, 0.0) } else { map.lf_stats(open) };
        let lf = lf_map(&map, map.lf.as_ref(), map.lf_height, &from_map, &to_map, lf_level, rq.unit).map(Arc::new);
        let sea_lf = lf_map(&map, map.sea_lf.as_ref(), SEA_LF_HEIGHT, &from_map, &to_map, lf_level, rq.unit).map(Arc::new);
        wce_foundation::log::info(format!("map context: the frame's LF maps made, {}", wce_foundation::heap::note()));

        let index = MapIndex::new(&map, open);
        // A campaign map's tiles (`terrain/tiles/campaign/...`) take the campaign database's climates and textures.
        let kind = if map.placements.iter().take(64).any(|p| p.location.replace('\\', "/").to_ascii_lowercase().contains("/tiles/campaign/")) { "campaign" } else { "battle" };
        let kit_climates = ClimateList::load(&rq.raw_root.join(format!("terrain/tiles/{kind}/_tile_database/_settings.xml"))).unwrap_or_default();
        let climates = effective_climates(kit_climates, kind, files.as_deref());
        let MapIndex { owner, climate, by_chunk, reach } = index;
        Self {
            map, unit: rq.unit, open, to_map, from_map, lf_level, lf_range, lf, sea_lf, by_chunk, owner, reach, climate, climates, look: rq.look.clone(),
            groups: rq.groups.clone(), files, plain_base: Mutex::new(rq.plain.clone()), plain_normal_id: next_id(&NORMAL_IDS), zeros: Mutex::default(),
            plain_bases: Mutex::default(), plain_colours: Mutex::default(), plains: Mutex::default(),
        }
    }

    /// The same frame over other placements of the same map (the Terrain tool's fill run again after an edit): its LF maps, climates
    /// and materials kept, the placements' chunks and cells worked out again (a while on the campaign map: away from the UI thread).
    pub fn with_map(&self, map: Arc<PlacedMap>) -> Self {
        let MapIndex { owner, climate, by_chunk, reach } = MapIndex::new(&map, self.open);
        Self {
            map, unit: self.unit, open: self.open, to_map: self.to_map, from_map: self.from_map, lf_level: self.lf_level, lf_range: self.lf_range,
            lf: self.lf.clone(), sea_lf: self.sea_lf.clone(), by_chunk, owner, reach, climate, climates: self.climates.clone(), look: self.look.clone(),
            groups: self.groups.clone(), files: self.files.clone(), plain_base: Mutex::new(self.plain_base.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone()),
            plain_normal_id: self.plain_normal_id, zeros: Mutex::default(), plain_bases: Mutex::default(), plain_colours: Mutex::default(), plains: Mutex::default(),
        }
    }

    /// The most cells a placement reaches past its origin.
    pub fn reach(&self) -> i32 {
        self.reach
    }

    /// The LF height under a frame point relative to the open tile (scene units).
    pub fn lift(&self, x: f64, z: f64) -> f32 {
        self.lf.as_ref().map_or(0.0, |lf| lf.height_at(x as f32, z as f32))
    }

    /// Where the map's north (smaller rows) is in the frame (x, z): +z unless the open tile is placed turned.
    pub fn north(&self) -> [f32; 2] {
        let (a, b) = (self.from_map.apply(0.0, 1.0), self.from_map.apply(0.0, 0.0));
        [(b.0 - a.0) as f32, (b.1 - a.1) as f32]
    }

    /// The map cell under a frame point.
    pub fn cell_of(&self, x: f64, z: f64) -> (f64, f64) {
        self.to_map.apply(x, z)
    }

    /// The chunks of the map.
    pub fn chunks(&self) -> (i32, i32) {
        ((self.map.width as i32 + CHUNK - 1) / CHUNK, (self.map.height as i32 + CHUNK - 1) / CHUNK)
    }

    /// A placement's climate: the map's when the database has its textures, else the look's.
    pub fn climate_name(&self, i: usize) -> String {
        self.map.climate_of(&self.map.placements[i]).filter(|c| self.climates.find(c).is_some()).map_or_else(|| self.look.climate.clone(), str::to_owned)
    }

    /// The plain material's base for a climate and its id: the base itself for the look's climate (or NO_CLIMATE), else the base with that
    /// climate's textures (made once).
    fn plain_for(&self, climate: u8) -> PlainBase {
        let mut bases = self.plain_bases.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(b) = bases.get(&climate).filter(|b| b.0.is_some()) {
            return b.clone();
        }

        let base = self.plain_base.lock().unwrap_or_else(|p| p.into_inner()).clone();
        let name = self.map.climates.get(usize::from(climate)).filter(|c| self.climates.find(c).is_some());
        let made = match (&base, name, &self.groups, &self.files) {
            (Some(base), Some(name), Some(groups), Some(files)) if *name != self.look.climate => {
                let look = TerrainLook { climate: name.clone(), ..self.look.clone() };
                let id = next_id(&MATERIAL_IDS);
                (Some(Arc::new(retexture(base, groups, &look, &self.climates, files.as_ref(), id))), id)
            }

            _ => (base, bases.get(&climate).map_or_else(|| next_id(&MATERIAL_IDS), |b| b.1)),
        };

        bases.insert(climate, made.clone());
        made
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

    /// [`Self::runs`] cut by climate: each rectangle with the climate of its cells.
    fn runs_by_climate(&self, area: [i32; 4], take: impl Fn(u32, i32, i32) -> bool) -> Vec<([i32; 4], u8)> {
        let (w, h) = (self.map.width as i32, self.map.height as i32);
        let at = |x: i32, y: i32| self.climate[(y * w + x) as usize];
        let mut seen = vec![];
        for y in area[1].max(0)..area[3].min(h) {
            for x in area[0].max(0)..area[2].min(w) {
                let c = at(x, y);
                if !seen.contains(&c) && take(self.owner[(y * w + x) as usize], x, y) {
                    seen.push(c);
                }
            }
        }

        seen.into_iter().flat_map(|c| self.runs(area, |o, x, y| at(x, y) == c && take(o, x, y)).into_iter().map(move |r| (r, c))).collect()
    }

    /// The cells of a chunk nobody covers.
    fn uncovered(&self, key: ChunkKey) -> Vec<([i32; 4], u8)> {
        self.runs_by_climate([key.0 * CHUNK, key.1 * CHUNK, (key.0 + 1) * CHUNK, (key.1 + 1) * CHUNK], |o, _, _| o == 0)
    }

    /// The cells a chunk draws, but those in `except`: those of its placements (which reach out of it by up to the largest tile) when
    /// `placed`, else its own nobody covers.
    fn owned(&self, key: ChunkKey, except: &[[i32; 4]], placed: bool) -> Vec<([i32; 4], u8)> {
        let (me, c) = (self.owner_of(key), [key.0 * CHUNK, key.1 * CHUNK, (key.0 + 1) * CHUNK, (key.1 + 1) * CHUNK]);
        let inside = |x: i32, y: i32| x >= c[0] && x < c[2] && y >= c[1] && y < c[3];
        let cut = |x: i32, y: i32| except.iter().any(|r| x >= r[0] && x < r[2] && y >= r[1] && y < r[3]);
        self.runs_by_climate([c[0], c[1], c[2] + self.reach, c[3] + self.reach], |o, x, y| (if placed { o == me } else { o == 0 && inside(x, y) }) && !cut(x, y))
    }

    /// The LF ground over a rectangle of map cells: a grid of zero heights on the LF's lattice (the shader adds the LF), the plain material
    /// of the cells' climate.
    fn ground(&self, r: [i32; 4], far: bool, climate: u8) -> (Float4x4, Payload) {
        let b = bounds(&self.from_map, f64::from(r[0]), f64::from(r[1]), f64::from(r[2]), f64::from(r[3]));
        let step = self.lf.as_ref().map_or(CELL / 4.0, |lf| f64::from(lf.step));
        let (nx, nz) = (((b[2] - b[0]) / step).round() as i32 + 1, ((b[3] - b[1]) / step).round() as i32 + 1);
        let heights = self.zeros.lock().unwrap_or_else(|p| p.into_inner()).entry((nx, nz)).or_insert_with(|| Arc::new(vec![0.0; (nx * nz) as usize])).clone();
        let (base, plain_id) = self.plain_for(climate);
        let material = base.map(|base| {
            self.plains.lock().unwrap_or_else(|p| p.into_inner()).entry((nx, nz, far, climate)).or_insert_with(|| {
                // A little past its rectangle (one colour: its maps wrap onto themselves), so two of them side by side leave no pixel to
                // neither (the resolve would draw it black).
                // One id per climate and one normal id for all sizes: one set of maps on the GPU (a normal id per size had the renderer make
                // the normal map again every time the next terrain's size differed).
                let mut m = plain_material(&base, plain_id, self.plain_normal_id);
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

    /// The height of the sea (0 m) in the frame.
    pub fn sea_y(&self) -> f32 {
        (-self.lf_level / self.unit) as f32
    }

    /// The height the shaders measure from (the scene's `sea_level`): the sea, plus a step and a half of the 16-bit LF (6.9 cm). The terrain
    /// shader lifts land and sinks sea bed by 9 cm by the sign of that height; the map's coasts sit a step above 0 m (the LF holds 0 m to
    /// 4.6 cm), and the tiles, built against their own sea at 0, lay their quays on ground just below it: measured from the sea itself, that
    /// ground was land, rose 18 cm against the quays and fought with them as the camera moved. A deviation from the game's arithmetic: ground
    /// less than 6.9 cm above the sea counts as sea bed.
    pub fn bias_y(&self) -> f32 {
        ((self.map.lf_height / 65535.0 * 1.5 - self.lf_level) / self.unit) as f32
    }

    /// The sea: one plane at 0 m over the cells within `reach` of a frame point (ASSUMPTION: the battle sea is at 0 m; the LF's lowest value is
    /// -vertical_offset, -20 m, under the open sea), drawn relative to the open tile like the rest. Where the ground is higher it hides it.
    /// It goes past the map's edges: the game's sea mesh reaches 28.8 km around the camera whatever the map (docs/re/water.md); cut at the
    /// edge, the sea ended short of the horizon near one (sea_coast/2x2_bb_straight's campaign placement is 101 cells from the top) and the
    /// far ground's last edge and skirts showed beyond it as dark lines.
    pub fn sea(&self, at: (f64, f64), reach: i32) -> (Float4x4, Payload) {
        let (cx, cy) = self.cell_of(at.0, at.1);
        let r = f64::from(reach);
        let b = bounds(&self.from_map, cx - r, cy - r, cx + r, cy + r);
        let y = self.sea_y();
        let v = |x: f64, z: f64| WaterVertex { x: x as f32, y, z: z as f32, u: x as f32, v: z as f32, fade: 1.0, ..Default::default() };
        // Its version follows its rectangle (the renderer lays a plane's chunks out again when it changes).
        let version = b.iter().fold(0xCBF2_9CE4_8422_2325u64, |h, v| (h ^ v.to_bits()).wrapping_mul(0x0100_0000_01B3));
        let sea = Water { river: false, foam: 0.0, verts: vec![v(b[0], b[1]), v(b[2], b[1]), v(b[0], b[3]), v(b[2], b[3])], indices: vec![0, 2, 1, 1, 2, 3], version };
        (Float4x4::IDENTITY, Payload::Water(sea))
    }

    /// A floor under the open tile (its cells, off the LF, no lower than the sea tiles' bed or `deepest` m, whichever is lower, less a metre),
    /// as a custom-mesh tile's (`place`). Its sea edges stand over the sea tiles' bed (Rome 2's Carthage: -20 m against their -40 m), and
    /// through that gap the views under the tile reached the sky, which the sea refracted as a bright strip along the edges. A skirt on
    /// the tile's grid closed it but showed on its land edges, a dotted line where a neighbour's ground lay a little lower than the edge.
    pub fn open_floor(&self, deepest: f64) -> Option<(Float4x4, Payload)> {
        let depth = FLOOR_DEPTH.max(1.0 - deepest);
        let (mut world, ground) = self.ground(self.open, false, NO_CLIMATE);
        let Payload::Terrain(t) = ground else { return None };
        world.m[1][3] = self.sea_y() - (depth / self.unit) as f32;
        Some((world, Payload::Terrain(Terrain { lf: None, ..t })))
    }

    /// A placement drawn from its tile at a level: its terrain (the tile's heights buffer and material, turned and moved), then its entities
    /// moved into the frame and lifted by the LF at their position.
    fn place(&self, i: usize, t: &Template, lod: Lod, out: &mut Vec<(Float4x4, Payload)>) {
        let a = self.from_map.then_after(&self.map.placements[i].to_map());
        let frame = matrix(&a, 0.0);
        let (grid, material) = match lod {
            Lod::Near => (&t.near, &t.material),
            Lod::Ground => (&t.coarse, &t.material_far),
            _ => (&t.coarse, &t.material_low),
        };

        let terrain = Terrain {
            nx: grid.nx, nz: grid.nz, origin_x: grid.ox, origin_z: grid.oz, step: grid.step, heights: grid.heights.clone(), lod: true, material: material.clone(), lf: self.lf.clone(),
            legacy: false, custom: t.custom.clone(), ..Default::default()
        };

        // A sea tile (the `sea` set) on the sea's LF, not the land's: the game draws its sea geometry pass with the battle's
        // `lf_sea_height_map.dds` bound (fn 0x119256e0 with sea = 1, pass 0x15), 0 (-20 m) on all but 5246 of its pixels; on the land's LF a
        // sea tile next to a cliff rose up to 40 m out of the sea over its last 32 m, where the game (same camera, from a memory dump)
        // has open sea. ASSUMPTION: that the game's sea pass is what draws the sea tiles was not traced (which patches it draws); the
        // sea LF is taken as 0 everywhere (its few other pixels are not read). Only a sea tile at the sea (`on_sea_lf`): a mod's lake of
        // sea tiles high up (TDD's Long Lake, 626 m) stays on the land's LF.
        if self.on_sea_lf(i) {
            out.push(self.on_sea(frame, terrain));
            return self.place_entities(t, lod, frame, out);
        }

        // A custom-mesh tile's floor: the same grid as its own terrain (whose patches are not drawn) at a fixed depth under the sea, off
        // the LF. Beside the mesh is the sea, and the sea is transparent: with nothing under it the background showed through. The LF
        // there is the land the cliff drops from, so the floor cannot follow it; it is where the sea tiles' bed is on the open sea's LF
        // (LF 0: -20 m, and their own -20 m), so their skirts (40 m) reach it. ASSUMPTION: what the game has there was not traced.
        if t.custom.is_some() {
            let mut floor = frame;
            floor.m[1][3] = self.sea_y() - (FLOOR_DEPTH / self.unit) as f32;
            let ground = Terrain { custom: None, lf: None, ..terrain.clone() };
            out.push((floor, Payload::Terrain(ground)));
        }

        out.push((frame, Payload::Terrain(terrain)));
        self.place_entities(t, lod, frame, out);
    }

    /// A sea tile's ground (`frame`: where it stands on the land's LF) on the sea's LF: the map's sea LF (`LF * 400 - 20` m), else a
    /// flat -20 m (the sea LF taken as 0).
    fn on_sea(&self, mut frame: Float4x4, t: Terrain) -> (Float4x4, Payload) {
        match &self.sea_lf {
            Some(sea) => (frame, Payload::Terrain(Terrain { lf: Some(sea.clone()), ..t })),
            None => {
                frame.m[1][3] = self.sea_y() - (self.map.vertical_offset / self.unit) as f32;
                (frame, Payload::Terrain(Terrain { lf: None, ..t }))
            }
        }
    }

    /// Whether a placement is drawn on the sea's LF (`place`): a tile of the `sea` set whose land LF is at or under the sea (the median of
    /// its cells' quarter-cell samples at most SEA_LF_REACH m above 0; the cliff coasts' are -14 .. -20 m). TDD builds the Long Lake (`rom_third_age_map`) of sea tiles on a flat 626 m LF, and
    /// its battle sea LF is -20 m there: drawn on it, the lake's tiles sank 646 m into pits between their coast tiles.
    fn on_sea_lf(&self, i: usize) -> bool {
        let p = &self.map.placements[i];
        if !is_sea_set(&p.location) {
            return false;
        }

        // A campaign map's sea tiles are its open sea, wherever the land LF is (south of Rome it stands out of the water under some).
        if self.climates.kind == "campaign" {
            return true;
        }

        let r = p.rect();
        let mut v: Vec<f64> = (r[1] * 4..r[3] * 4).flat_map(|y| (r[0] * 4..r[2] * 4).map(move |x| (x, y)))
            .map(|(x, y)| self.map.lf_metres((f64::from(x) + 0.5) / 4.0, (f64::from(y) + 0.5) / 4.0))
            .collect();
        v.sort_by(f64::total_cmp);
        v.get(v.len() / 2).is_none_or(|&m| m <= SEA_LF_REACH)
    }

    /// A placement's entities moved into the frame (`frame`) and lifted by the LF at their position.
    fn place_entities(&self, t: &Template, lod: Lod, frame: Float4x4, out: &mut Vec<(Float4x4, Payload)>) {
        let n = if lod == Lod::Near { t.entities.len() } else { t.mid };
        for (w, payload) in &t.entities[..n] {
            // The ground ring keeps the rivers (they are part of the land) and leaves what stands on it.
            if lod == Lod::Ground && !matches!(payload, Payload::Water(_)) {
                continue;
            }

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
            let lift = self.lift(x, z);
            world.m[1][3] += lift;

            // A tree's untilted matrix (POSITION5..7: the tree shaders blend it with the world one per vertex, `untilted_rows`) moves with
            // it: left in the tile's frame, every vertex was pulled towards a place hundreds of metres off (long streaks). Other models'
            // POSITION5..7 are shader parameters (their 3 x 3 part is zero).
            let mut payload = payload.clone();
            if let Payload::MeshInstance(mi) = &mut payload
                && let Some(rows) = untilted(&[mi.p5, mi.p6, mi.p7])
            {
                let mut m = frame * rows;
                m.m[1][3] += lift;
                [mi.p5, mi.p6, mi.p7] = [m.m[0], m.m[1], m.m[2]];
            }

            out.push((world, payload));
        }
    }

    /// A block as one terrain ([`Lod::Block`], [`Lod::Merged`]; `key` its first chunk): one grid, one colour image, one draw, where its tiles
    /// and the LF's rectangles were hundreds. The grid holds what a tile's own does, the heights over the LF (the shader adds it): a tile's
    /// where `paint` opens the tiles, none where it doesn't. The sea tiles' and the uncovered cells' stand at the sea bed, which is off the
    /// land's LF (`on_sea`), so the one grid takes them in too; what the sea leaves of the bed is not seen from where a block is drawn. A
    /// custom-mesh tile keeps its mesh. Every cell is drawn by the block it is in, whichever chunk owns its tile.
    fn build_block(&self, key: ChunkKey, paint: bool, source: &BlockSource) -> Vec<(Float4x4, Payload)> {
        #[derive(Clone, Copy, PartialEq)]
        enum Cell {
            Land,
            Sea,
            Tile(usize),
        }

        let (w, h) = (self.map.width as i32, self.map.height as i32);
        let side = BLOCK * CHUNK;
        let area = [key.0 * CHUNK, key.1 * CHUNK, (key.0 * CHUNK + side).min(w), (key.1 * CHUNK + side).min(h)];
        let (aw, ah) = (area[2] - area[0], area[3] - area[1]);
        let mut cells: Vec<Cell> = (0..aw * ah).map(|k| {
            let (x, y) = (area[0] + k % aw, area[1] + k / aw);
            let sea = match self.owner[(y * w + x) as usize] {
                0 => self.climates.kind == "campaign",
                o => o == 1,
            };

            if sea { Cell::Sea } else { Cell::Land }
        }).collect();

        // The placements over its cells, from their chunks: up to the largest tile before the block.
        let mut out = vec![];
        let mut tiles: HashMap<usize, (Arc<Template>, Affine2)> = HashMap::new();
        let back = (self.reach + CHUNK - 1) / CHUNK;
        for cy in key.1 - back..key.1 + BLOCK {
            for cx in key.0 - back..key.0 + BLOCK {
                for &i in self.by_chunk.get(&(cx, cy)).map_or(&[][..], Vec::as_slice) {
                    let p = &self.map.placements[i];
                    let r = p.rect();
                    if r[0] >= area[2] || r[2] <= area[0] || r[1] >= area[3] || r[3] <= area[1] {
                        continue;
                    }

                    let kind = if (source.is_custom)(i) {
                        // Its mesh, once: by the block of its chunk.
                        let mine = cx >= key.0 && cy >= key.1;
                        match (source.template)(i) {
                            Some(t) => {
                                if mine {
                                    self.place(i, &t, Lod::Far, &mut out);
                                }

                                Cell::Sea
                            }

                            None => continue,
                        }
                    } else if self.on_sea_lf(i) {
                        Cell::Sea
                    } else if paint {
                        match (source.template)(i) {
                            Some(t) => {
                                tiles.insert(i, (t, p.to_map().inverse()));
                                Cell::Tile(i)
                            }

                            None => continue,
                        }
                    } else {
                        continue;
                    };

                    for y in r[1].max(area[1])..r[3].min(area[3]) {
                        for x in r[0].max(area[0])..r[2].min(area[2]) {
                            let at = &mut cells[((y - area[1]) * aw + x - area[0]) as usize];
                            if self.owner[(y * w + x) as usize] > 1 && !matches!(at, Cell::Tile(_)) {
                                *at = kind;
                            }
                        }
                    }
                }
            }
        }

        let b = bounds(&self.from_map, f64::from(area[0]), f64::from(area[1]), f64::from(area[2]), f64::from(area[3]));
        let cell_at = |x: f64, z: f64| {
            let (mx, my) = self.to_map.apply(x, z);
            let (cx, cy) = ((mx.floor() as i32).clamp(area[0], area[2] - 1), (my.floor() as i32).clamp(area[1], area[3] - 1));
            ((mx, my), (cx, cy), cells[((cy - area[1]) * aw + cx - area[0]) as usize])
        };

        // The heights at twice the grid's density: the fine ones are for the normal map.
        let bed = self.sea_y() - (self.map.vertical_offset / self.unit) as f32;
        let (fw, fh) = (((b[2] - b[0]) / (CELL / 2.0)).round() as i32 + 1, ((b[3] - b[1]) / (CELL / 2.0)).round() as i32 + 1);
        let mut grid = Vec::with_capacity((fw / 2 + 1) as usize * (fh / 2 + 1) as usize);
        let mut relief = vec![0.0f32; (fw * fh) as usize];
        for j in 0..fh {
            for i in 0..fw {
                let (x, z) = (b[0] + f64::from(i) * CELL / 2.0, b[1] + f64::from(j) * CELL / 2.0);
                let (at, _, cell) = cell_at(x, z);
                let own = match cell {
                    Cell::Tile(k) => {
                        let (t, inverse) = &tiles[&k];
                        let (lx, lz) = inverse.apply(at.0, at.1);
                        t.own.height(lx, lz)
                    }

                    _ => 0.0,
                };

                relief[(j * fw + i) as usize] = own;
                if i % 2 == 0 && j % 2 == 0 {
                    grid.push(if matches!(cell, Cell::Sea) { bed - self.lift(x, z) } else { own });
                }
            }
        }

        let (nx, nz) = (fw / 2 + 1, fh / 2 + 1);
        let (lo, hi) = grid.iter().fold((f32::MAX, f32::MIN), |(lo, hi), &v| (lo.min(v), hi.max(v)));

        // The colour.
        let (tw, th) = (((fw - 1) * BLOCK_TEXELS / 2) as usize, ((fh - 1) * BLOCK_TEXELS / 2) as usize);
        let mut colour = vec![255u8; tw * th * 4];
        for (j, row) in colour.chunks_exact_mut(tw * 4).enumerate() {
            for (i, texel) in row.chunks_exact_mut(4).enumerate() {
                let x = b[0] + (i as f64 + 0.5) / tw as f64 * (b[2] - b[0]);
                let z = b[1] + (j as f64 + 0.5) / th as f64 * (b[3] - b[1]);
                let (at, (cx, cy), cell) = cell_at(x, z);
                let painted = match cell {
                    Cell::Tile(k) => {
                        let (t, inverse) = &tiles[&k];
                        let (lx, lz) = inverse.apply(at.0, at.1);
                        t.far_colour(lx, lz)
                    }

                    _ => None,
                };

                // Under the sea its bed, not the climate's ground: the sea shows what is under it, and a bright bed took the coast away.
                let under = matches!(cell, Cell::Sea) || self.lift(x, z) < self.sea_y();
                let c = painted.unwrap_or_else(|| if under { SEA_BED } else { self.plain_colour(self.climate[(cy * w + cx) as usize], source.mean) });
                texel.copy_from_slice(&c);
            }
        }

        let climate = self.climate[(area[1] * w + area[0]) as usize];
        let Some(base) = self.plain_for(climate).0 else { return out };
        let mut m = (*base).clone();
        let (gw, gh) = (b[2] - b[0], b[3] - b[1]);
        m.baked = true;
        m.blend = TerrainImage { w: tw as u32, h: th as u32, rgba: Some(Arc::new(colour)) };
        m.index = TerrainImage::default();
        m.alpha = TerrainImage::default();
        m.normal = terrain_normal_image(&relief, fw, fh, CELL / 2.0, CELL / 2.0, fw as u32, fh as u32);
        m.diffuse = Default::default();
        m.normal_tex = Default::default();
        (m.x0, m.z0, m.x1, m.z1) = (-FAR_MARGIN, -FAR_MARGIN, gw + FAR_MARGIN, gh + FAR_MARGIN);
        (m.y0, m.y1) = (f64::from(lo - SKIRT) - BOX_BELOW, f64::from(hi) + BOX_ABOVE);
        m.id = next_id(&MATERIAL_IDS);
        m.normal_id = next_id(&NORMAL_IDS);
        // A skirt, as a tile's: two blocks read different tiles along their shared edge, and the gap between them showed as a line. No level
        // of detail: at a vertex a cell it is coarse already, the shader takes the LF at the vertices, and two blocks at different strides
        // parted by far more than a skirt closes (black lines, block-wide, in the distance).
        let g = Grid { nx, nz, ox: 0.0, oz: 0.0, step: CELL as f32, heights: Arc::new(grid) }.skirted();
        let t = Terrain {
            nx: g.nx, nz: g.nz, origin_x: g.ox, origin_z: g.oz, step: g.step, heights: g.heights, lod: false, tile_metres: 32.0, material: Some(Arc::new(m)),
            lf: self.lf.clone(), legacy: false, ..Default::default()
        };
        let mut world = Float4x4::IDENTITY;
        (world.m[0][3], world.m[2][3]) = (b[0] as f32, b[1] as f32);
        out.push((world, Payload::Terrain(t)));
        out
    }

    /// The colour of a climate's plain ground, as a baked material stores it: what its material averages to.
    fn plain_colour(&self, climate: u8, mean: &dyn Fn(&str) -> Option<[f32; 3]>) -> [u8; 4] {
        let mut known = self.plain_colours.lock().unwrap_or_else(|p| p.into_inner());
        *known.entry(climate).or_insert_with(|| self.plain_for(climate).0.as_deref().map_or([128, 128, 128, 255], |m| mean_of_material(m, mean)))
    }

    /// A chunk at a level: its placements from their tiles (`template` gives a placement's tile, none when it could not be opened: its
    /// rectangle gets LF ground), and LF ground over its cells nobody covers. `is_custom` says whether a placement's tile is a custom-mesh one.
    pub fn build_chunk(&self, key: ChunkKey, lod: Lod, template: &dyn Fn(usize) -> Option<Arc<Template>>, is_custom: &dyn Fn(usize) -> bool, mean: &dyn Fn(&str) -> Option<[f32; 3]>) -> Vec<(Float4x4, Payload)> {
        if matches!(lod, Lod::Block | Lod::Merged) {
            return self.build_block(key, lod == Lod::Merged, &BlockSource { template, is_custom, mean });
        }

        // Far: the LF ground over everything it draws, in as few rectangles as its cells make; but a custom-mesh tile (the cliffs) keeps
        // its mesh (its coarsest level by distance): the LF under it is the land its cliff drops from, which drawn plain stood as a slab
        // over the sea. Their tiles are few (48 cliffs), opened once for all their placements. A sea tile's rectangle is on the sea's LF.
        if lod == Lod::Far {
            let mut out = vec![];
            let mut cut = vec![];
            for &i in self.by_chunk.get(&key).map_or(&[][..], Vec::as_slice) {
                let p = &self.map.placements[i];
                if is_custom(i)
                    && let Some(t) = template(i)
                {
                    self.place(i, &t, Lod::Far, &mut out);
                    cut.push(p.rect());
                } else if self.on_sea_lf(i) {
                    // A sea tile's ground on the sea's LF (`place`): on the land's, next to a cliff it rose out of the sea.
                    let c = if self.map.climate_of(p).is_some() { p.climate } else { NO_CLIMATE };
                    let (world, ground) = self.ground(p.rect(), true, c);
                    if let Payload::Terrain(t) = ground {
                        out.push(self.on_sea(world, t));
                    }

                    cut.push(p.rect());
                }
            }

            out.extend(self.owned(key, &cut, true).into_iter().map(|(r, c)| self.ground(r, true, c)));
            out.extend(self.owned(key, &cut, false).into_iter().map(|(r, c)| self.uncovered_ground(r, true, c)));
            return out;
        }

        let mut out = vec![];
        for &i in self.by_chunk.get(&key).map_or(&[][..], Vec::as_slice) {
            match template(i) {
                Some(t) => self.place(i, &t, lod, &mut out),
                None => {
                    let p = &self.map.placements[i];
                    let c = if self.map.climate_of(p).is_some() { p.climate } else { NO_CLIMATE };
                    out.push(self.ground(p.rect(), false, c));
                }
            }
        }

        for (r, c) in self.uncovered(key) {
            out.push(self.uncovered_ground(r, false, c));
        }

        out
    }

    /// The ground of cells no tile covers. On a campaign map those are its open sea (the tile map's deep sea, no tile of its own): on the
    /// sea's LF as a sea tile is (`on_sea`). The land LF there holds what its painters left under the water: south of Rome a dome of
    /// sand stood out of the sea where the game has open water.
    fn uncovered_ground(&self, r: [i32; 4], far: bool, c: u8) -> (Float4x4, Payload) {
        let (world, ground) = self.ground(r, far, c);
        match ground {
            Payload::Terrain(t) if self.climates.kind == "campaign" => self.on_sea(world, t),
            g => (world, g),
        }
    }

    /// A line for the report: the LF under the open tile.
    pub fn lf_note(&self) -> String {
        format!(
            "LF under the tile {:.1} m median, {:.1} m range{}", self.lf_level, self.lf_range,
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
