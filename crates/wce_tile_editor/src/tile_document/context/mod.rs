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
use template::{BOX_ABOVE, BOX_BELOW, Grid, SKIRT, Template, mean_of_material};
use wce_formats::tile_entry::TileTextureGroup;
use wce_vfs::files::Files;

pub use stream::{ChunkChange, ChunkKey, Lod, Streamer};
pub use wce_tile::sources::tile_dir;

/// Map cells a side of a chunk.
pub const CHUNK: i32 = 8;

/// Chunks a side of a block ([`Lod::Block`], [`Lod::Merged`]).
pub const BLOCK: i32 = 8;

/// How far a block's blend box reaches past its heights (scene units).
const BLOCK_BOX: f64 = 500.0;

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
/// coasts' sea tiles' medians are -14 .. -20 m, TDD's Long Lake's 626 m (its lowest too); the lowest sample under the tile is compared.
const SEA_LF_REACH: f64 = 10.0;

/// How deep a custom-mesh tile's floor lies under the sea (metres): see `Frame::place`.
const FLOOR_DEPTH: f64 = 40.0;

/// How far under the land LF a campaign map's cliff floor lies where it is land (metres): under the cliff mesh's top, on the LF too.
const CAMPAIGN_FLOOR_UNDER: f64 = 2.0;

/// What to build.
#[derive(Clone)]
pub struct ContextRequest {
    pub map: Arc<PlacedMap>,
    pub spot: Spot,

    /// The open tile's size in cells and its scene units per metre. No cells (0, 0): the map's own frame (the Terrain tool), its origin at
    /// `spot` with the map's axes (x along the columns, z up the rows), nothing left out and the heights measured from the sea.
    pub cells: (i32, i32),
    pub unit: f64,

    /// The frame's z (north) over the map's cells' (1: square cells). A campaign map is drawn in the game's world, the hex grid's: 2 / sqrt 3
    /// longer north to south (docs/re/renderer.md, "The campaign frame"): its ground (tiles, the LF, the sea) is stretched by it, the
    /// models only moved.
    pub z_stretch: f64,

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

/// What a chunk or a block is built from: a placement's tile (none when it could not be opened), whether a placement's tile is a custom-mesh
/// one, and a ground texture's mean colour.
pub struct BlockSource<'a> {
    pub template: &'a dyn Fn(usize) -> Option<Arc<Template>>,
    pub is_custom: &'a dyn Fn(usize) -> bool,
    pub mean: &'a dyn Fn(&str) -> Option<[f32; 3]>,

    /// Whether a chunk is drawn by a block at the moment.
    pub in_block: &'a dyn Fn(ChunkKey) -> bool,

    /// The tiles' own heights over the LF at a map cell point, when known without opening them (a campaign map's far material's
    /// relief): the unpainted blocks' ground.
    pub relief: &'a dyn Fn(f64, f64) -> Option<f32>,
}

/// A cell no placement gives a climate.
const NO_CLIMATE: u8 = u8::MAX;

/// A campaign map's far material as the game's terrain resolve reads it (`s_far_diffuse_map`, `s_far_normal_map`: docs/re/terrain.md,
/// "Far blend"): the whole map's ground in one square image over the map's rectangle, row 0 its north edge. The game's are 8192 a side,
/// made at load (RenderDoc, capture A); `size` here (DEVIATION: smaller, the blend starts 9 world units from the eye).
#[derive(Clone, Debug, Default)]
pub struct FarMap {
    pub size: u32,

    /// RGBA8 rows: the colour as the tiles' far bakes store it (the square root of the linear colour: the shader squares it), the alpha
    /// their detail byte.
    pub diffuse: Arc<Vec<u8>>,

    /// RGBA8 rows: the normal as the game packs it (alpha its x, green its up component, red and blue 255; the shader makes the third).
    pub normal: Arc<Vec<u8>>,

    /// The tiles' own heights over the LF (scene units; 0 where no tile is): the far blocks' relief without opening their tiles.
    pub relief: Arc<Vec<f32>>,
}

impl FarMap {
    /// The relief at a map cell point (x right, y down: the map's cells over its width and height), bilinear.
    pub fn relief_at(&self, x: f64, y: f64, w: u32, h: u32) -> f32 {
        let n = self.size as usize;
        if n == 0 || self.relief.len() < n * n {
            return 0.0;
        }
        let (u, v) = (
            (x / f64::from(w) * n as f64 - 0.5).clamp(0.0, (n - 1) as f64),
            (y / f64::from(h) * n as f64 - 0.5).clamp(0.0, (n - 1) as f64),
        );
        let (i, j) = (u.floor() as usize, v.floor() as usize);
        let (i1, j1) = ((i + 1).min(n - 1), (j + 1).min(n - 1));
        let (fu, fv) = ((u - i as f64) as f32, (v - j as f64) as f32);
        let r = |a: usize, b: usize| self.relief[b * n + a];
        let top = r(i, j) + (r(i1, j) - r(i, j)) * fu;
        let bottom = r(i, j1) + (r(i1, j1) - r(i, j1)) * fu;
        top + (bottom - top) * fv
    }
}

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
    let det = r(0, 0) * (r(1, 1) * r(2, 2) - r(1, 2) * r(2, 1)) - r(0, 1) * (r(1, 0) * r(2, 2) - r(1, 2) * r(2, 0))
        + r(0, 2) * (r(1, 0) * r(2, 1) - r(1, 1) * r(2, 0));
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
/// `k`: the frame's z stretch (its z step is the x step times it).
#[allow(clippy::too_many_arguments)]
fn lf_map(
    map: &PlacedMap,
    f: Option<&LfField>,
    height: f64,
    from_map: &Affine2,
    to_map: &Affine2,
    lf_level: f64,
    unit: f64,
    k: f64,
    campaign: bool,
) -> Option<LfMap> {
    let vertical_offset = map.vertical_offset;
    let f = f.filter(|f| f.w > 1 && f.h > 1)?;
    let (ppx, ppy) = (f64::from(f.w) / f64::from(map.width), f64::from(f.h) / f64::from(map.height));
    let s = CELL / ppx;
    let sz = s * k;
    let mut b = bounds(from_map, 0.0, 0.0, f64::from(f.w - 1) / ppx, f64::from(f.h - 1) / ppy);
    // A campaign map's game LF has two samples a cell, the field's box averages (main_attila: the field eight a cell, the game's texture
    // 4064 x 2882, BOB's 4 x 4 `lf_height_map_lq.dds`, `campaign_level`), and the terrain shader puts its texel k at k / 2 cells: the
    // mean of samples 4k .. 4k + 3, centred a sample and a half further on. The samples move back by that (3/16 of a cell, north-west),
    // so a point's height is the game's: at the main_attila coast below capture A's camera, eleven points off by 6.8 m rms on the field's
    // own lattice, 3.0 m with it moved so (the best fit then 1/16 of a cell further south, 1.2 m: not explained).
    // `shift` (samples) takes a node back to its own sample.
    let mut shift = (0.0, 0.0);
    if campaign && ppx > 2.0 && ppy > 2.0 {
        let (dx, dy) = ((ppx / 2.0 - 1.0) * 0.5 / ppx, (ppy / 2.0 - 1.0) * 0.5 / ppy);
        let (a, o) = (from_map.apply(-dx, -dy), from_map.apply(0.0, 0.0));
        let (ox, oz) = (a.0 - o.0, a.1 - o.1);
        b = [b[0] + ox, b[1] + oz, b[2] + ox, b[3] + oz];
        shift = (dx * ppx, dy * ppy);
    }
    let (nx, nz) = (((b[2] - b[0]) / s).round() as i32 + 1, ((b[3] - b[1]) / sz).round() as i32 + 1);
    // The frame lays the field's samples out one to one (the Terrain tool's: the map's own frame, rows the other way up): the field's
    // samples themselves, shared (the campaign's LF is 375 MB), else resampled.
    let sample = |i: f64, j: f64| {
        let (cx, cy) = to_map.apply(b[0] + i * s, b[1] + j * sz);
        (cx * ppx + shift.0, cy * ppy + shift.1)
    };
    let near = |a: (f64, f64), b: (f64, f64)| (a.0 - b.0).abs() < 1e-3 && (a.1 - b.1).abs() < 1e-3;
    let same_size = u32::try_from(nx).ok() == Some(f.w) && u32::try_from(nz).ok() == Some(f.h);
    let last_y = f64::from(f.h - 1);
    let shared = if !same_size {
        None
    } else if near(sample(0.0, 0.0), (0.0, last_y)) && near(sample(1.0, 0.0), (1.0, last_y)) && near(sample(0.0, 1.0), (0.0, last_y - 1.0))
    {
        Some(true)
    } else if near(sample(0.0, 0.0), (0.0, 0.0)) && near(sample(1.0, 0.0), (1.0, 0.0)) && near(sample(0.0, 1.0), (0.0, 1.0)) {
        Some(false)
    } else {
        None
    };

    if let Some(flip) = shared {
        let mut lf = LfMap {
            x0: b[0] as f32,
            z0: b[1] as f32,
            step: s as f32,
            step_z: sz as f32,
            nx,
            nz,
            values: LfSamples::shared(f.values.clone(), flip),
            scale: (height / unit) as f32,
            offset: ((vertical_offset + lf_level) / unit) as f32,
            id: next_id(&NORMAL_IDS),
            ..Default::default()
        };

        lf_normals(&mut lf);
        return Some(lf);
    }

    let field = f.values.read();
    let mut values = vec![0u16; (nx * nz) as usize];
    values.par_chunks_mut(nx as usize).enumerate().for_each(|(j, row)| {
        for (i, v) in row.iter_mut().enumerate() {
            let (cx, cy) = to_map.apply(b[0] + i as f64 * s, b[1] + j as f64 * sz);
            let (px, py) = ((cx * ppx + shift.0).clamp(0.0, f64::from(f.w - 1)), (cy * ppy + shift.1).clamp(0.0, f64::from(f.h - 1)));
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
        x0: b[0] as f32,
        z0: b[1] as f32,
        step: s as f32,
        step_z: sz as f32,
        nx,
        nz,
        values: LfSamples::new(values),
        scale: (height / unit) as f32,
        offset: ((vertical_offset + lf_level) / unit) as f32,
        id: next_id(&NORMAL_IDS),
        ..Default::default()
    };

    lf_normals(&mut lf);
    Some(lf)
}

/// The LF map's normals as the blend shader reads `s_lf_normal_map`: at uv = (x, -z) / normal_world, wrapping, so the image lays the map's
/// normals around a torus of that size; .a = x, .g = -z of the unit normal * 0.5 + 0.5.
fn lf_normals(lf: &mut LfMap) {
    lf.normal_k = ((lf.nx.max(lf.nz) + LF_NORMAL_SIDE - 1) / LF_NORMAL_SIDE).max(1);
    let (w, h) = lf.normal_size();
    let (s, sz) = (lf.step * lf.normal_k as f32, lf.dz() * lf.normal_k as f32);
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
    lf.normal_world = [w as f32 * s, h as f32 * sz];
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

    /// [`ContextRequest::z_stretch`]: part of `from_map` / `to_map`.
    pub z_stretch: f64,

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

    /// A campaign map's sea material: a `sea` tile's own (every one is `sand_a`, the texture the game's sea pass draws), for the sea's
    /// ground past the rings (`on_sea`); offered by the first such tile read.
    sea_material: Mutex<Option<Arc<TerrainMaterial>>>,

    /// The climates' plain colours (`plain_colour`).
    plain_colours: Mutex<HashMap<u8, [u8; 4]>>,
    plains: Mutex<Plains>,
}

impl Frame {
    /// The frame of a request (slow on the campaign map: its LF map; run it away from the UI thread).
    pub fn new(rq: &ContextRequest, files: Option<Arc<dyn Files>>) -> Self {
        let map = rq.map.clone();
        let me = MapPlacement {
            location: String::new(),
            variation: String::new(),
            x: rq.spot.x,
            y: rq.spot.y,
            w: rq.cells.0,
            h: rq.cells.1,
            rotation: rq.spot.rotation,
            flipped: false,
            climate: 0,
            any_variation: false,
        };
        let open = me.rect();
        let k = if rq.z_stretch > 0.0 { rq.z_stretch } else { 1.0 };
        let from_map = Affine2 { m: [[1.0, 0.0, 0.0], [0.0, k, 0.0]] }.then_after(&me.to_map().inverse());
        let to_map = from_map.inverse();
        let (lf_level, lf_range) = if rq.cells == (0, 0) { (0.0, 0.0) } else { map.lf_stats(open) };
        // A campaign map's tiles (`terrain/tiles/campaign/...`) take the campaign database's climates and textures, and its LF is the game's
        // (`lf_map`).
        let campaign =
            map.placements.iter().take(64).any(|p| p.location.replace('\\', "/").to_ascii_lowercase().contains("/tiles/campaign/"));
        let lf = lf_map(&map, map.lf.as_ref(), map.lf_height, &from_map, &to_map, lf_level, rq.unit, k, campaign).map(Arc::new);
        let sea_lf = lf_map(&map, map.sea_lf.as_ref(), SEA_LF_HEIGHT, &from_map, &to_map, lf_level, rq.unit, k, false).map(Arc::new);
        wce_foundation::log::info(format!("map context: the frame's LF maps made, {}", wce_foundation::heap::note()));

        let index = MapIndex::new(&map, open);
        let kind = if campaign { "campaign" } else { "battle" };
        let kit_climates =
            ClimateList::load(&rq.raw_root.join(format!("terrain/tiles/{kind}/_tile_database/_settings.xml"))).unwrap_or_default();
        let climates = effective_climates(kit_climates, kind, files.as_deref());
        let MapIndex { owner, climate, by_chunk, reach } = index;
        Self {
            map,
            unit: rq.unit,
            z_stretch: k,
            open,
            to_map,
            from_map,
            lf_level,
            lf_range,
            lf,
            sea_lf,
            by_chunk,
            owner,
            reach,
            climate,
            climates,
            look: rq.look.clone(),
            groups: rq.groups.clone(),
            files,
            plain_base: Mutex::new(rq.plain.clone()),
            plain_normal_id: next_id(&NORMAL_IDS),
            zeros: Mutex::default(),
            plain_bases: Mutex::default(),
            sea_material: Mutex::default(),
            plain_colours: Mutex::default(),
            plains: Mutex::default(),
        }
    }

    /// The same frame over other placements of the same map (the Terrain tool's fill run again after an edit): its LF maps, climates
    /// and materials kept, the placements' chunks and cells worked out again (a while on the campaign map: away from the UI thread).
    pub fn with_map(&self, map: Arc<PlacedMap>) -> Self {
        let MapIndex { owner, climate, by_chunk, reach } = MapIndex::new(&map, self.open);
        Self {
            map,
            unit: self.unit,
            z_stretch: self.z_stretch,
            open: self.open,
            to_map: self.to_map,
            from_map: self.from_map,
            lf_level: self.lf_level,
            lf_range: self.lf_range,
            lf: self.lf.clone(),
            sea_lf: self.sea_lf.clone(),
            by_chunk,
            owner,
            reach,
            climate,
            climates: self.climates.clone(),
            look: self.look.clone(),
            groups: self.groups.clone(),
            files: self.files.clone(),
            plain_base: Mutex::new(self.plain_base.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone()),
            plain_normal_id: self.plain_normal_id,
            zeros: Mutex::default(),
            plain_bases: Mutex::default(),
            sea_material: Mutex::new(self.sea_material.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone()),
            plain_colours: Mutex::default(),
            plains: Mutex::default(),
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

    /// The drawn ground at a frame point: the LF plus the tile's own heights there (the highest of the placements over its cell whose
    /// template `template` has ready: a masked tile's ground slopes down where another lies), the LF alone where none is ready. A campaign
    /// map's props and trees stand on it: on the LF alone they floated over the tiles that sink below it (TDD's Emyn Muil, 2026-11-16).
    pub fn ground_at(&self, x: f64, z: f64, template: &dyn Fn(usize) -> Option<Arc<Template>>) -> f32 {
        let (mx, my) = self.to_map.apply(x, z);
        let (cx, cy) = (mx.floor() as i32, my.floor() as i32);
        let back = (self.reach + CHUNK - 1) / CHUNK;
        let (kx, ky) = (cx.div_euclid(CHUNK), cy.div_euclid(CHUNK));
        let mut own: Option<f32> = None;
        for gy in ky - back..=ky {
            for gx in kx - back..=kx {
                for &i in self.by_chunk.get(&(gx, gy)).map_or(&[][..], Vec::as_slice) {
                    let p = &self.map.placements[i];
                    let r = p.rect();
                    if cx < r[0] || cx >= r[2] || cy < r[1] || cy >= r[3] {
                        continue;
                    }

                    if let Some(t) = template(i).filter(|t| t.custom.is_none()) {
                        let (lx, lz) = p.to_map().inverse().apply(mx, my);
                        let h = t.own.height(lx, lz);
                        own = Some(own.map_or(h, |o| o.max(h)));
                    }
                }
            }
        }

        self.lift(x, z) + own.unwrap_or(0.0)
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
        self.map
            .climate_of(&self.map.placements[i])
            .filter(|c| self.climates.find(c).is_some())
            .map_or_else(|| self.look.climate.clone(), str::to_owned)
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

    /// A campaign map's sea material (`sea_material`), when there is none yet.
    pub fn offer_sea(&self, m: &Option<Arc<TerrainMaterial>>) {
        let mut sea = self.sea_material.lock().unwrap_or_else(|p| p.into_inner());
        if sea.is_none() {
            sea.clone_from(m);
        }
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
        let mut free: Vec<bool> = (0..aw * ah)
            .map(|k| {
                let (x, y) = (area[0] + (k % aw) as i32, area[1] + (k / aw) as i32);
                take(self.owner[(y * w + x) as usize], x, y)
            })
            .collect();

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
        self.runs_by_climate([c[0], c[1], c[2] + self.reach, c[3] + self.reach], |o, x, y| {
            (if placed { o == me } else { o == 0 && inside(x, y) }) && !cut(x, y)
        })
    }

    /// The LF ground over a rectangle of map cells: a grid of zero heights on the LF's lattice (the shader adds the LF), the plain material
    /// of the cells' climate.
    fn ground(&self, r: [i32; 4], far: bool, climate: u8) -> (Float4x4, Payload) {
        self.ground_with(r, far, climate, None)
    }

    /// The heights of an LF ground grid on a campaign map from `source`: its points over the sea (sea tiles, cliffs, uncovered cells)
    /// at the sea bed, the others at the far material's relief (none yet: 0); none when every point is 0.
    fn ground_heights(&self, r: [i32; 4], b: [f64; 4], nx: i32, nz: i32, step: f64, source: &BlockSource) -> Option<Vec<f32>> {
        // Each cell's once (`cell_on_sea` looks at every placement over it).
        let rw = (r[2] - r[0]).max(0);
        let on_sea: Vec<bool> =
            (r[1]..r[3]).flat_map(|y| (r[0]..r[2]).map(move |x| (x, y))).map(|(x, y)| self.cell_on_sea(x, y, source.is_custom)).collect();
        let sea = on_sea.iter().any(|&s| s);
        if !sea && (source.relief)(f64::from(r[0]), f64::from(r[1])).is_none() {
            return None;
        }
        let bed = self.sea_y() - (self.map.vertical_offset / self.unit) as f32;
        let k = self.z_stretch;
        let mut heights = vec![0.0f32; (nx * nz) as usize];
        for j in 0..nz {
            for i in 0..nx {
                let (x, z) = (b[0] + f64::from(i) * step, b[1] + f64::from(j) * step * k);
                let (mx, my) = self.to_map.apply(x, z);
                let (cx, cy) = ((mx.floor() as i32).clamp(r[0], r[2] - 1), (my.floor() as i32).clamp(r[1], r[3] - 1));
                heights[(j * nx + i) as usize] = if sea && on_sea[((cy - r[1]) * rw + (cx - r[0])) as usize] {
                    bed - self.lift(x, z)
                } else {
                    (source.relief)(mx, my).unwrap_or(0.0)
                };
            }
        }
        Some(heights)
    }

    /// [`Self::ground`]; with `source`, on a campaign map its heights from it (`ground_heights`: the sea at its bed, the land at the far
    /// material's relief): for ground on the land's LF that a sea tile's own (on the sea's LF) does not replace.
    fn ground_with(&self, r: [i32; 4], far: bool, climate: u8, source: Option<&BlockSource>) -> (Float4x4, Payload) {
        let b = bounds(&self.from_map, f64::from(r[0]), f64::from(r[1]), f64::from(r[2]), f64::from(r[3]));
        // Its grid square in its own units (the LF's x step), the frame's z stretch in its world matrix.
        let step = self.lf.as_ref().map_or(CELL / 4.0, |lf| f64::from(lf.step));
        let k = self.z_stretch;
        let (nx, nz) = (((b[2] - b[0]) / step).round() as i32 + 1, ((b[3] - b[1]) / k / step).round() as i32 + 1);
        // On a campaign map the points over sea tiles (and uncovered cells: the open sea) at the sea bed, as the blocks have them: the
        // land LF stands out of the sea under some (35 m beside main_attila's islands), and the ground there showed as dark flats.
        let sea_heights = source.filter(|_| self.is_campaign()).and_then(|s| self.ground_heights(r, b, nx, nz, step, s));
        let heights = match sea_heights {
            Some(h) => Arc::new(h),
            None => self
                .zeros
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .entry((nx, nz))
                .or_insert_with(|| Arc::new(vec![0.0; (nx * nz) as usize]))
                .clone(),
        };
        let (base, plain_id) = self.plain_for(climate);
        let material = base.map(|base| {
            self.plains
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .entry((nx, nz, far, climate))
                .or_insert_with(|| {
                    // A little past its rectangle (one colour: its maps wrap onto themselves), so two of them side by side leave no pixel to
                    // neither (the resolve would draw it black).
                    // One id per climate and one normal id for all sizes: one set of maps on the GPU (a normal id per size had the renderer make
                    // the normal map again every time the next terrain's size differed).
                    let mut m = plain_material(&base, plain_id, self.plain_normal_id);
                    let (w, h) = (f64::from(nx - 1) * step, f64::from(nz - 1) * step);
                    let e = if far { FAR_MARGIN } else { PLAIN_MARGIN };
                    (m.x0, m.z0, m.x1, m.z1) = (-e, -e, w + e, h + e);
                    Arc::new(m)
                })
                .clone()
        });

        let t = Terrain {
            nx,
            nz,
            origin_x: 0.0,
            origin_z: 0.0,
            step: step as f32,
            heights,
            lod: true,
            tile_metres: 32.0,
            material,
            lf: self.lf.clone(),
            legacy: false,
            ..Default::default()
        };
        let mut world = Float4x4::IDENTITY;
        (world.m[0][3], world.m[2][3]) = (b[0] as f32, b[1] as f32);
        world.m[2][2] = k as f32;
        (world, Payload::Terrain(t))
    }

    /// Whether a map cell is the sea on a campaign map: no placement over it, a cliff's (`custom`), or a sea tile's (`on_sea_lf`) with no
    /// land tile under it. A coast tile under a sea tile (`sea_coast` on layer 0, `sea` over it) is the land: the game draws it on the
    /// land's LF, its sea tile on the sea's, and the higher shows (the shore; RenderDoc, capture A).
    fn cell_on_sea(&self, x: i32, y: i32, custom: &dyn Fn(usize) -> bool) -> bool {
        let (w, h) = (self.map.width as i32, self.map.height as i32);
        if x < 0 || y < 0 || x >= w || y >= h {
            return true;
        }
        match self.owner[(y * w + x) as usize] {
            0 => true,
            1 => false,
            _ => {
                let (mut sea, mut land) = (false, false);
                for i in self.over_cell(x, y) {
                    if custom(i) {
                        return true;
                    }
                    if self.on_sea_lf(i) {
                        sea = true;
                    } else {
                        land = true;
                    }
                }
                sea && !land
            }
        }
    }

    /// A map cell as the ground sees it, for scripts: who draws it and whether it is the sea (custom-mesh tiles by `custom`).
    pub fn cell_note(&self, x: i32, y: i32, custom: &dyn Fn(usize) -> bool) -> String {
        let (w, h) = (self.map.width as i32, self.map.height as i32);
        let owner = if x < 0 || y < 0 || x >= w || y >= h { u32::MAX } else { self.owner[(y * w + x) as usize] };
        let over: Vec<String> = self.over_cell(x, y).map(|i| format!("{i}{}", if self.on_sea_lf(i) { " sea" } else { "" })).collect();
        format!("owner {owner}, over [{}], sea {}", over.join(", "), self.cell_on_sea(x, y, custom))
    }

    /// The placements over a map cell, every layer's (`by_chunk` holds a placement in its origin's chunk).
    fn over_cell(&self, x: i32, y: i32) -> impl Iterator<Item = usize> + '_ {
        let back = (self.reach + CHUNK - 1) / CHUNK;
        let (kx, ky) = (x.div_euclid(CHUNK), y.div_euclid(CHUNK));
        (ky - back..=ky)
            .flat_map(move |gy| (kx - back..=kx).map(move |gx| (gx, gy)))
            .flat_map(move |k| self.by_chunk.get(&k).map_or(&[][..], Vec::as_slice).iter().copied())
            .filter(move |&i| {
                let r = self.map.placements[i].rect();
                x >= r[0] && x < r[2] && y >= r[1] && y < r[3]
            })
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
        let sea = Water {
            river: false,
            foam: 0.0,
            verts: vec![v(b[0], b[1]), v(b[2], b[1]), v(b[0], b[3]), v(b[2], b[3])],
            indices: vec![0, 2, 1, 1, 2, 3],
            version,
        };
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
            nx: grid.nx,
            nz: grid.nz,
            origin_x: grid.ox,
            origin_z: grid.oz,
            step: grid.step,
            heights: grid.heights.clone(),
            lod: true,
            material: material.clone(),
            lf: self.lf.clone(),
            legacy: false,
            custom: t.custom.clone(),
            ..Default::default()
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
        // A cliff without a blend map of its own (`load_custom_template`) has no material to lend it: the plain one of its climate.
        // A campaign map's: on the land LF, the parts of the tile its mesh leaves bare that lie against land (`custom_floor_heights`) a
        // little under it, the rest (under the mesh, against the sea) at the battle's depth under it. At the battle's fixed depth for all,
        // main_attila's inner cliff south-east of Rome (`6x4_inner_right_t1_r3`, its mesh 47 % of its cells) left a flat hole showing the
        // sea where the game has land.
        if let (Some(_), Some((f, covered)), "campaign") = (&t.custom, &t.floor, self.climates.kind.as_str()) {
            let material = terrain.material.clone().or_else(|| self.plain_over(i, grid));
            let heights = self.custom_floor_heights(i, f, covered, frame.m[1][1]);
            let ground = Terrain {
                nx: f.nx,
                nz: f.nz,
                origin_x: f.ox,
                origin_z: f.oz,
                step: f.step,
                heights: Arc::new(heights),
                custom: None,
                material,
                ..terrain.clone()
            };
            out.push((frame, Payload::Terrain(ground)));
        } else if t.custom.is_some() {
            let mut floor = frame;
            floor.m[1][3] = self.sea_y() - (FLOOR_DEPTH / self.unit) as f32;
            let material = terrain.material.clone().or_else(|| self.plain_over(i, grid));
            let ground = Terrain { custom: None, lf: None, material, ..terrain.clone() };
            out.push((floor, Payload::Terrain(ground)));
        }

        out.push((frame, Payload::Terrain(terrain)));
        self.place_entities(t, lod, frame, out);
    }

    /// A campaign cliff floor's heights over the LF (tile units; `scale_y` the frame's): the points its mesh leaves bare whose nearest edge
    /// of the tile has a land tile beyond it, a little under the LF (`CAMPAIGN_FLOOR_UNDER`: the land behind an inner cliff); the others
    /// (under the mesh, against sea tiles or the open sea) twice `FLOOR_DEPTH` under it.
    /// ASSUMPTION: what the game draws where a cliff's mesh does not reach was not traced (its screenshots: land behind that cliff, sea in
    /// front of the others).
    fn custom_floor_heights(&self, i: usize, f: &Grid, covered: &[bool], scale_y: f32) -> Vec<f32> {
        let p = &self.map.placements[i];
        let to_map = p.to_map();
        let me = p.rect();
        let (nx, nz) = (f.nx as usize, f.nz as usize);
        let under = |m: f64| -((m / self.unit) as f32) / scale_y.max(1e-6);
        // The sea side twice the battle's depth under the LF: the land LF stands 20 - 30 m up under some sea in front of the cliffs, and
        // at one depth the floor there showed lighter through the water than the open sea's.
        let (land_h, sea_h) = (under(CAMPAIGN_FLOOR_UNDER), under(2.0 * FLOOR_DEPTH));
        // Whether a map cell beside the tile is under a land tile (not of the sea set, not a cliff); none: the open sea.
        let land_at = |x: i32, y: i32| {
            if x >= me[0] && x < me[2] && y >= me[1] && y < me[3] {
                return false;
            }

            let (cx, cy) = (x.div_euclid(CHUNK), y.div_euclid(CHUNK));
            let r = (self.reach + CHUNK - 1) / CHUNK;
            (cy - r..=cy + r).flat_map(|ky| (cx - r..=cx + r).map(move |kx| (kx, ky))).any(|k| {
                self.by_chunk.get(&k).is_some_and(|list| {
                    list.iter().any(|&j| {
                        let q = &self.map.placements[j];
                        let rr = q.rect();
                        x >= rr[0] && x < rr[2] && y >= rr[1] && y < rr[3] && !is_sea_set(&q.location) && !q.location.contains("cliff")
                    })
                })
            })
        };

        // Each bare point is what lies beyond the tile's nearest edge: the cell just outside it, straight out (a bare region can reach round a
        // cliff's end from the sea to the land, so regions mislead).
        let (x0, z0) = (f64::from(f.ox), f64::from(f.oz));
        let (x1, z1) = (x0 + (nx - 1) as f64 * f64::from(f.step), z0 + (nz - 1) as f64 * f64::from(f.step));
        let mut heights = vec![sea_h; nx * nz];
        for k in 0..nx * nz {
            if covered[k] {
                continue;
            }

            let (x, z) = (x0 + (k % nx) as f64 * f64::from(f.step), z0 + (k / nx) as f64 * f64::from(f.step));
            let out = 64.0;
            let edges = [(x - x0, (x0 - out, z)), (x1 - x, (x1 + out, z)), (z - z0, (x, z0 - out)), (z1 - z, (x, z1 + out))];
            let (_, (ox, oz)) = edges.into_iter().min_by(|a, b| a.0.total_cmp(&b.0)).expect("four edges");
            let (u, v) = to_map.apply(ox, oz);
            if land_at(u.floor() as i32, v.floor() as i32) {
                heights[k] = land_h;
            }
        }

        heights
    }

    /// The plain material of a placement's climate (that of its first cell) over a grid of its tile.
    fn plain_over(&self, i: usize, grid: &Grid) -> Option<Arc<TerrainMaterial>> {
        let r = self.map.placements[i].rect();
        let (w, h) = (self.map.width as i32, self.map.height as i32);
        let climate = self.climate.get((r[1].clamp(0, h - 1) * w + r[0].clamp(0, w - 1)) as usize).copied().unwrap_or(NO_CLIMATE);
        let (base, id) = self.plain_for(climate);
        let base = base?;
        let mut m = plain_material(&base, id, self.plain_normal_id);
        let e = PLAIN_MARGIN;
        let (x0, z0) = (f64::from(grid.ox), f64::from(grid.oz));
        let (x1, z1) = (x0 + f64::from(grid.nx - 1) * f64::from(grid.step), z0 + f64::from(grid.nz - 1) * f64::from(grid.step));
        (m.x0, m.z0, m.x1, m.z1) = (x0 - e, z0 - e, x1 + e, z1 + e);
        Some(Arc::new(m))
    }

    /// A sea tile's ground (`frame`: where it stands on the land's LF) on the sea's LF: the map's sea LF (`LF * 400 - 20` m), else a
    /// flat -20 m (the sea LF taken as 0).
    fn on_sea(&self, mut frame: Float4x4, t: Terrain) -> (Float4x4, Payload) {
        // A campaign map's sea tiles are drawn as the game's sea pass draws them (`Terrain::sea_floor`; RenderDoc, capture A): sand with
        // an up normal, none of the terrain's blend, so the coast tiles' ground (on the land's LF) shows where it is higher: the shore.
        let campaign = self.is_campaign();
        // Past the rings its ground is a plain one (its climate's grass): the sea tiles' own material once one has been read.
        let material = if campaign && t.material.as_ref().is_some_and(|m| m.baked || m.diffuse[0] != self.sea_diffuse()) {
            self.sea_material.lock().unwrap_or_else(|p| p.into_inner()).clone().or(t.material)
        } else {
            t.material
        };
        let t = Terrain { sea_floor: campaign, material, ..t };
        match &self.sea_lf {
            Some(sea) => (frame, Payload::Terrain(Terrain { lf: Some(sea.clone()), ..t })),
            None => {
                frame.m[1][3] = self.sea_y() - (self.map.vertical_offset / self.unit) as f32;
                (frame, Payload::Terrain(Terrain { lf: None, ..t }))
            }
        }
    }

    /// The sea material's first texture (`sea_material`; empty without one).
    fn sea_diffuse(&self) -> String {
        self.sea_material.lock().unwrap_or_else(|p| p.into_inner()).as_ref().map_or_else(String::new, |m| m.diffuse[0].clone())
    }

    /// Whether a placement is drawn on the sea's LF (`place`): a tile of the `sea` set whose land LF reaches the sea somewhere (the lowest
    /// of its cells' quarter-cell samples at most SEA_LF_REACH m above 0; the cliff coasts' are -14 .. -20 m). TDD builds the Long Lake (`rom_third_age_map`) of sea tiles on a flat 626 m LF, and
    /// its battle sea LF is -20 m there: drawn on it, the lake's tiles sank 646 m into pits between their coast tiles. Not the median:
    /// Thrones' campaign has land LF ridges up to 38 m under sea tiles off its coast (`sea/1x1` at (377, 807) of `vik_attila_map`: lowest
    /// -1 m, median 11.5 m), which the game draws as open sea; on the land's LF they rose out of it as sand banks (2026-11-16).
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
        (r[1] * 4..r[3] * 4)
            .flat_map(|y| (r[0] * 4..r[2] * 4).map(move |x| (x, y)))
            .map(|(x, y)| self.map.lf_metres((f64::from(x) + 0.5) / 4.0, (f64::from(y) + 0.5) / 4.0))
            .min_by(f64::total_cmp)
            .is_none_or(|m| m <= SEA_LF_REACH)
    }

    /// A model's matrix moved by the frame but not stretched by it: the z row of its 3 x 3 part over the frame's z stretch (its position
    /// stays where the stretched frame puts it).
    fn unstretch(&self, m: &mut Float4x4) {
        if self.z_stretch != 1.0 {
            let k = self.z_stretch as f32;
            for c in 0..3 {
                m.m[2][c] /= k;
            }
        }
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

            // A model is moved into the stretched frame, not stretched itself (the game's buildings and props are uniform in its
            // stretched world: RenderDoc, docs/re/renderer.md "The campaign frame"): the z row of its 3 x 3 part without the stretch. A
            // lake's water planes (`water_plane` models scaled over it) are the ground's: stretched with their shores.
            let mut world = frame * *w;
            let water_plane = matches!(payload, Payload::MeshInstance(mi) if mi.model.to_ascii_lowercase().contains("water_plane"));
            if !water_plane {
                self.unstretch(&mut world);
            }
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
                self.unstretch(&mut m);
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
        let mut cells: Vec<Cell> = (0..aw * ah)
            .map(|k| {
                let (x, y) = (area[0] + k % aw, area[1] + k / aw);
                let sea = match self.owner[(y * w + x) as usize] {
                    0 => self.climates.kind == "campaign",
                    o => o == 1,
                };

                if sea { Cell::Sea } else { Cell::Land }
            })
            .collect();

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

        // A sea tile's cell that a land tile shares (a coast tile under it) is the land's, as `cell_on_sea` has it (the shore follows the
        // LF; else the coasts stepped from cell to cell).
        for k in 0..(aw * ah) as usize {
            let (x, y) = (area[0] + k as i32 % aw, area[1] + k as i32 / aw);
            if matches!(cells[k], Cell::Sea) && self.owner[(y * w + x) as usize] > 1 && !self.cell_on_sea(x, y, source.is_custom) {
                cells[k] = Cell::Land;
            }
        }

        let b = bounds(&self.from_map, f64::from(area[0]), f64::from(area[1]), f64::from(area[2]), f64::from(area[3]));
        let cell_at = |x: f64, z: f64| {
            let (mx, my) = self.to_map.apply(x, z);
            let (cx, cy) = ((mx.floor() as i32).clamp(area[0], area[2] - 1), (my.floor() as i32).clamp(area[1], area[3] - 1));
            ((mx, my), (cx, cy), cells[((cy - area[1]) * aw + cx - area[0]) as usize])
        };

        // The heights at twice the grid's density: the fine ones are for the normal map. The grid's rows are square in its own units and
        // its world matrix stretches them (a campaign map's hex z): a row is z_stretch frame units on. Taken a frame unit apart, its rows
        // read the map ever further south of where they were drawn (six cells at a block's north edge, and a block 15 % taller than its
        // square): main_attila's land south of the islands west of Rome stood as flats over their sea tiles.
        let bed = self.sea_y() - (self.map.vertical_offset / self.unit) as f32;
        let k = self.z_stretch;
        let (fw, fh) = (((b[2] - b[0]) / (CELL / 2.0)).round() as i32 + 1, ((b[3] - b[1]) / k / (CELL / 2.0)).round() as i32 + 1);
        let mut grid = Vec::with_capacity((fw / 2 + 1) as usize * (fh / 2 + 1) as usize);
        let mut relief = vec![0.0f32; (fw * fh) as usize];
        for j in 0..fh {
            for i in 0..fw {
                let (x, z) = (b[0] + f64::from(i) * CELL / 2.0, b[1] + f64::from(j) * CELL / 2.0 * k);
                let (at, _, cell) = cell_at(x, z);
                let own = match cell {
                    Cell::Tile(k) => {
                        let (t, inverse) = &tiles[&k];
                        let (lx, lz) = inverse.apply(at.0, at.1);
                        t.own.height(lx, lz)
                    }

                    // A block not painted: its tiles' relief from the far material, when there is one (the mountains past the rings).
                    Cell::Land => (source.relief)(at.0, at.1).unwrap_or(0.0),
                    Cell::Sea => 0.0,
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
            for (i, texel) in row.as_chunks_mut::<4>().0.iter_mut().enumerate() {
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
                let c = painted
                    .unwrap_or_else(|| if under { SEA_BED } else { self.plain_colour(self.climate[(cy * w + cx) as usize], source.mean) });
                texel.copy_from_slice(&c);
            }
        }

        let climate = self.climate[(area[1] * w + area[0]) as usize];
        let Some(base) = self.plain_for(climate).0 else { return out };
        let mut m = (*base).clone();
        // The grid in its own units (square cells), the frame's z stretch in its world matrix.
        let (gw, gh) = (b[2] - b[0], (b[3] - b[1]) / self.z_stretch);
        m.baked = true;
        m.blend = TerrainImage { w: tw as u32, h: th as u32, rgba: Some(Arc::new(colour)) };
        m.index = TerrainImage::default();
        m.alpha = TerrainImage::default();
        m.normal = terrain_normal_image(&relief, fw, fh, CELL / 2.0, CELL / 2.0, fw as u32, fh as u32);
        m.diffuse = Default::default();
        m.normal_tex = Default::default();
        (m.x0, m.z0, m.x1, m.z1) = (-FAR_MARGIN, -FAR_MARGIN, gw + FAR_MARGIN, gh + FAR_MARGIN);
        // Tall: the blend pass finds a pixel's height from its depth, which is tens of units off from where a block is seen, and a pixel out of
        // every box is black (lines along the blocks' edges, from high up). Only what the box covers on the screen costs, and that is little.
        (m.y0, m.y1) = (f64::from(lo - SKIRT) - BLOCK_BOX, f64::from(hi) + BLOCK_BOX);
        m.id = next_id(&MATERIAL_IDS);
        m.normal_id = next_id(&NORMAL_IDS);
        // A skirt, as a tile's: two blocks read different tiles along their shared edge, and the gap between them showed as a line. No level
        // of detail: at a vertex a cell it is coarse already, the shader takes the LF at the vertices, and two blocks at different strides
        // parted by far more than a skirt closes (black lines, block-wide, in the distance).
        let g = Grid { nx, nz, ox: 0.0, oz: 0.0, step: CELL as f32, heights: Arc::new(grid) }.skirted();
        let t = Terrain {
            nx: g.nx,
            nz: g.nz,
            origin_x: g.ox,
            origin_z: g.oz,
            step: g.step,
            heights: g.heights,
            lod: false,
            tile_metres: 32.0,
            material: Some(Arc::new(m)),
            lf: self.lf.clone(),
            legacy: false,
            ..Default::default()
        };
        let mut world = Float4x4::IDENTITY;
        (world.m[0][3], world.m[2][3]) = (b[0] as f32, b[1] as f32);
        world.m[2][2] = self.z_stretch as f32;
        out.push((world, Payload::Terrain(t)));
        out
    }

    /// Whether this is a campaign map's frame (its tiles in the campaign database).
    pub fn is_campaign(&self) -> bool {
        self.climates.kind == "campaign"
    }

    /// The map's south-west and north-east corners in the frame (map cells (0, h) and (w, 0)).
    pub fn corners(&self) -> ((f64, f64), (f64, f64)) {
        let (w, h) = (f64::from(self.map.width), f64::from(self.map.height));
        (self.from_map.apply(0.0, h), self.from_map.apply(w, 0.0))
    }

    /// The map's far material (`FarMap`), `size` texels a side: each texel the far colour of the tile drawing its cell (the first one over
    /// it, as the blocks take them; a cliff's or a sea tile's cells are the sea), else the sea bed under the sea or the climate's plain
    /// colour; the normal from the LF and the tiles' own heights. Every tile is opened once, in placement order (slow on a whole map:
    /// run it away from the UI thread). `stop` ends it early (None).
    pub fn far_map(&self, size: u32, source: &BlockSource, stop: &dyn Fn() -> bool) -> Option<FarMap> {
        let n = size as usize;
        let (w, h) = (self.map.width as i32, self.map.height as i32);
        let (sw, ne) = self.corners();
        let (fw, fh) = (ne.0 - sw.0, ne.1 - sw.1);
        // A texel's point in the frame and its map cell.
        let frame_of = |i: usize, j: usize| (sw.0 + (i as f64 + 0.5) / n as f64 * fw, ne.1 - (j as f64 + 0.5) / n as f64 * fh);

        // The tiles drawn: not a cliff, not a sea tile. A cell takes the highest of their own heights over it, as `ground_at` does (a
        // masked tile's ground slopes down where another lies: Attila's `large_oval_60_r` over the grass tiles in its mask's holes,
        // taken alone, sank them 256 m).
        let drawn = |i: usize| !(source.is_custom)(i) && !self.on_sea_lf(i);

        let mut diffuse = vec![0u8; n * n * 4];
        let mut heights = vec![f32::NAN; n * n];
        let mut relief = vec![0.0f32; n * n];
        let mut painted = vec![false; n * n];
        let mut best = vec![f32::NEG_INFINITY; n * n];
        for (i, p) in self.map.placements.iter().enumerate() {
            if stop() {
                return None;
            }
            if !drawn(i) {
                continue;
            }
            let r = p.rect();
            let Some(t) = (source.template)(i) else { continue };
            let inverse = p.to_map().inverse();
            // The texels whose cells lie in its rectangle.
            let (u0, u1) =
                ((f64::from(r[0]) / f64::from(w) * n as f64).floor() as i64, (f64::from(r[2]) / f64::from(w) * n as f64).ceil() as i64);
            let (v0, v1) =
                ((f64::from(r[1]) / f64::from(h) * n as f64).floor() as i64, (f64::from(r[3]) / f64::from(h) * n as f64).ceil() as i64);
            for j in v0.max(0)..v1.min(n as i64) {
                for k in u0.max(0)..u1.min(n as i64) {
                    let (x, z) = frame_of(k as usize, j as usize);
                    let (mx, my) = self.to_map.apply(x, z);
                    let (cx, cy) = (mx.floor() as i32, my.floor() as i32);
                    if cx < r[0] || cy < r[1] || cx >= r[2] || cy >= r[3] || cx < 0 || cy < 0 || cx >= w || cy >= h {
                        continue;
                    }
                    if self.owner[(cy * w + cx) as usize] <= 1 {
                        continue;
                    }
                    let at = j as usize * n + k as usize;
                    let (lx, lz) = inverse.apply(mx, my);
                    let own = t.own.height(lx, lz);
                    if own <= best[at] {
                        continue;
                    }
                    best[at] = own;
                    if let Some(c) = t.far_colour(lx, lz) {
                        diffuse[at * 4..at * 4 + 4].copy_from_slice(&c);
                        painted[at] = true;
                    }
                    relief[at] = own;
                    heights[at] = own + self.lift(x, z);
                }
            }
        }

        // The rest: the sea bed under the sea (the LF under it, or the cells of sea tiles and cliffs with no land tile: Attila's sea
        // tiles north of the Tiber's mouth lie on land LF up to 52 m, which baked the climate's green there, a flat far off where the
        // game has open sea), else the climate's plain ground (each climate's worked out once: `mean` is not shared between threads);
        // their heights the LF's.
        let mut plain: HashMap<u8, [u8; 4]> = HashMap::new();
        for &c in &self.climate {
            plain.entry(c).or_insert_with(|| self.plain_colour(c, source.mean));
        }
        let (mut sea_tile, mut land_tile) = (vec![false; (w * h) as usize], vec![false; (w * h) as usize]);
        for (i, p) in self.map.placements.iter().enumerate() {
            let mark = if (source.is_custom)(i) || self.on_sea_lf(i) { &mut sea_tile } else { &mut land_tile };
            let r = p.rect();
            for y in r[1].max(0)..r[3].min(h) {
                for x in r[0].max(0)..r[2].min(w) {
                    mark[(y * w + x) as usize] = true;
                }
            }
        }
        diffuse.par_chunks_mut(n * 4).zip(heights.par_chunks_mut(n)).zip(painted.par_chunks(n)).enumerate().for_each(
            |(j, ((row, hs), done))| {
                for k in 0..n {
                    let (x, z) = frame_of(k, j);
                    if hs[k].is_nan() {
                        hs[k] = self.lift(x, z);
                    }
                    if done[k] {
                        continue;
                    }
                    let (mx, my) = self.to_map.apply(x, z);
                    let (cx, cy) = ((mx.floor() as i32).clamp(0, w - 1), (my.floor() as i32).clamp(0, h - 1));
                    let at = (cy * w + cx) as usize;
                    let c = if self.lift(x, z) < self.sea_y() || (sea_tile[at] && !land_tile[at]) {
                        SEA_BED
                    } else {
                        plain.get(&self.climate[(cy * w + cx) as usize]).copied().unwrap_or(SEA_BED)
                    };
                    row[k * 4..k * 4 + 4].copy_from_slice(&c);
                }
            },
        );

        // The normals: the heights' central differences (scene units both ways; rows go south, the frame's z north).
        let (dx, dz) = (fw / n as f64, fh / n as f64);
        let mut normal = vec![255u8; n * n * 4];
        normal.par_chunks_mut(n * 4).enumerate().for_each(|(j, row)| {
            for k in 0..n {
                let hgt = |a: usize, b: usize| f64::from(heights[b.min(n - 1) * n + a.min(n - 1)]);
                let gx = (hgt(k + 1, j) - hgt(k.saturating_sub(1), j)) / (2.0 * dx);
                let gz = (hgt(k, j.saturating_sub(1)) - hgt(k, j + 1)) / (2.0 * dz);
                let l = (gx * gx + gz * gz + 1.0).sqrt();
                let q = |v: f64| ((v * 0.5 + 0.5).clamp(0.0, 1.0) * 255.0).round() as u8;
                row[k * 4 + 1] = q(1.0 / l);
                row[k * 4 + 3] = q(-gx / l);
            }
        });

        Some(FarMap { size, diffuse: Arc::new(diffuse), normal: Arc::new(normal), relief: Arc::new(relief) })
    }

    /// The colour of a climate's plain ground, as a baked material stores it: what its material averages to.
    fn plain_colour(&self, climate: u8, mean: &dyn Fn(&str) -> Option<[f32; 3]>) -> [u8; 4] {
        let mut known = self.plain_colours.lock().unwrap_or_else(|p| p.into_inner());
        *known
            .entry(climate)
            .or_insert_with(|| self.plain_for(climate).0.as_deref().map_or([128, 128, 128, 255], |m| mean_of_material(m, mean)))
    }

    /// A chunk at a level: its placements from their tiles (`template` gives a placement's tile, none when it could not be opened: its
    /// rectangle gets LF ground), and LF ground over its cells nobody covers. `is_custom` says whether a placement's tile is a custom-mesh one.
    pub fn build_chunk(&self, key: ChunkKey, lod: Lod, source: &BlockSource) -> Vec<(Float4x4, Payload)> {
        if matches!(lod, Lod::Block | Lod::Merged) {
            return self.build_block(key, lod == Lod::Merged, source);
        }

        let mut out = self.chunk_levels(key, lod, source);

        // Cells of its own that belong to a tile drawn by a block: a chunk keeps to its tiles and a block to its square, and a tile that
        // starts in a block and reaches in here was neither's (holes down to the sea bed along the blocks' edges).
        let (cw, _) = self.chunks();
        let by_block = |o: u32| o > 1 && (source.in_block)(((o as i32 - 2) % cw, (o as i32 - 2) / cw));
        let area = [key.0 * CHUNK, key.1 * CHUNK, (key.0 + 1) * CHUNK, (key.1 + 1) * CHUNK];
        let me = self.owner_of(key);
        // On the land's LF whatever tile they belong to: a campaign map's sea there at its bed (beside main_attila's islands the land LF
        // stands 35 m out of the sea under sea tiles, and these showed as dark flats).
        out.extend(
            self.runs_by_climate(area, |o, _, _| o != me && by_block(o))
                .into_iter()
                .map(|(r, c)| self.ground_with(r, true, c, Some(source))),
        );
        out
    }

    /// A chunk at a level, as [`Self::build_chunk`] says, without the cells of tiles the blocks draw.
    fn chunk_levels(&self, key: ChunkKey, lod: Lod, source: &BlockSource) -> Vec<(Float4x4, Payload)> {
        let (template, is_custom) = (source.template, source.is_custom);
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

                    // Its cells but those a land tile shares (a coast tile under it): their ground is the land's too (the shore).
                    let r = p.rect();
                    let sea: Vec<(i32, i32)> = (r[1]..r[3])
                        .flat_map(|y| (r[0]..r[2]).map(move |x| (x, y)))
                        .filter(|&(x, y)| self.cell_on_sea(x, y, is_custom))
                        .collect();
                    if sea.len() as i32 == (r[2] - r[0]) * (r[3] - r[1]) {
                        cut.push(r);
                    } else {
                        cut.extend(sea.into_iter().map(|(x, y)| [x, y, x + 1, y + 1]));
                    }
                }
            }

            // The tiles' ground with their relief from the far material, once there is one (a campaign map's mountains past the rings).
            out.extend(self.owned(key, &cut, true).into_iter().map(|(r, c)| self.ground_with(r, true, c, Some(source))));
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
            "LF under the tile {:.1} m median, {:.1} m range{}",
            self.lf_level,
            self.lf_range,
            if self.lf_range > 2.0 {
                format!(": its objects may be off the ground by up to {:.1} m", self.lf_range * 0.5 / self.unit)
            } else {
                String::new()
            }
        )
    }
}

/// Puts entities on a layer of the scene (cleared first); they can't be picked.
pub fn publish(
    scene: &mut RenderScene,
    layer: wce_scene::render_scene::LayerId,
    entities: Vec<(Float4x4, Payload)>,
    doc: wce_scene::render_scene::DocumentId,
) {
    scene.clear_layer(layer);
    scene.reserve(entities.len());
    for (world, payload) in entities {
        let e = scene.add(layer, world, payload, wce_scene::render_scene::SourceRef { doc, item: u64::MAX });
        if let Some(en) = scene.find_mut(e) {
            en.locked = true;
        }
    }
}
