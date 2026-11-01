//! The battle sea as the game draws it: the Sea Display's surface with the game's ocean shaders (`ocean.hlsl`, sm_4_0: vs30_main,
//! ps30_main_ultra_water_quality), not waterplane (docs/re/water.md, "The battle sea of a coastal battle": decompiled empire.retail.dll
//! and memory dumps of a coastal battle). The pixel shader puts foam where the water over the ground (from the depth channel) is less
//! than 1.6 m deep (the static foam texture `sea/combined_foam.dds`, two layers scrolling against each other) and whitecaps from the
//! animated foam array.
//!
//! The battle's wind (`RenderSettings::sea_state`, a key of `wind_levels_tables`) picks a row of `sea_surfaces_tables` and its two
//! prebaked surfaces (crate::sea_surface): every frame the sea (256 x 256, a 70 m patch) and swell (128 x 128, 800 m) textures are their
//! frame pair's angles, blended; the whitecap array (a slice per sea frame) is baked once per wind, and `g_foam_anim_data` is the sea's
//! frame pair and blend. Without the files (another game, a broken pack) the surfaces stay flat and there are no whitecaps (logged).
//!
//! ASSUMPTIONS (not traced: the code that fills the engine's radial mesh, fn 0x116e47e0's class, was not found):
//! - The mesh is a grid instead of the game's 80 x 164 radial mesh around the camera: each sea plane is a quadtree whose leaves are
//!   chunks of 32 x 32 cells, split while the camera is nearer than 1.5 times a node's size down to 128 m (4 m cells near the camera,
//!   coarser with distance), nodes outside the view skipped; a finer chunk's edge follows its coarser neighbour's (no cracks). Its
//!   vertices are displaced by the sea and swell heights (the frames' height images, blended like the angles) on the CPU every frame.
//!   (A fixed 4 m grid over the sea bed square was 260 000 vertices a frame; fixed 128 m chunks were 40 000 draws for a map context's
//!   sea: the quadtree keeps a few hundred.)
//!   The game's vertex shader takes a displaced and a rest position, and the surfaces carry heights whose range is in the shader's
//!   constants (`g_vs_sea_info.zw`), but how the engine combines them (and whether it uses the frames' slope images, unused here) is
//!   not known.
//! - The sea bed info texture (`sea_bed_info_texture`, 1024 x 1024 over x, z in [-1024, 1024]: the vertex shader reads red as the ground
//!   height, `r * g_vs_sea_info.y + g_vs_sea_info.x`, -20 .. 5 m) is built from the scene's terrains, as the game's is the ground's height
//!   (grey, `r * 25 - 20`; a RenderDoc capture of a coastal battle, docs/re/water.md). Near the shore the vertex shader flattens the waves into a small ripple, and pushes the sea
//!   4 m down where the ground is above it.
//! - `g_transparency_exponential_scale` (4) and the sea bed range (-20, 25) are the dumps' values; they are not in the tables.

use std::collections::BTreeMap;

use rayon::prelude::*;
use wce_foundation::wce_prof;
use wce_scene::render_scene::{Entity, EntityId, LfMap, Payload, RenderScene, Terrain};

use super::*;
use crate::sea_surface::{SeaRow, SeaSurface, parse_sea_surfaces, parse_wind_levels};
use crate::shader_lib::{bind_sampler, bind_srv};

const OCEAN_VS: &str = "dx11_sm_4_0_ocean.hlsl_vs_vs30_main";

/// Rome 2's (`sm_3_0`; a forward pass).
const R2_OCEAN_VS: &str = "dx11_sm_3_0_ocean.hlsl_vs_vs30_main";

/// The sea's pixel shader at a gfx_water_quality level (`sm_4_0`; Rome 2's `sm_3_0`).
fn ocean_ps(quality: i32, rome2: bool) -> String {
    format!("dx11_sm_{}_0_ocean.hlsl_ps_ps30_main_{}_water_quality", if rome2 { 3 } else { 4 }, super::water::water_level(quality))
}

/// The sea bed info texture: its size and the square it covers (the vertex shader's `(x + 1024) / 2048`); the game's is 1024 x 1024
/// (2 m texels; a RenderDoc capture at the lowest graphics settings, docs/re/water.md).
const SEA_BED_SIZE: u32 = 1024;
const SEA_BED_HALF: f32 = 1024.0;

/// The sea bed's height range (`g_vs_sea_info.xy`) and `g_transparency_exponential_scale`: the dumps' values, not in the tables.
const SEA_BED_RANGE: [f32; 2] = [-20.0, 25.0];
const TRANSPARENCY_EXPONENTIAL_SCALE: f32 = 4.0;

/// The sea quadtree: its leaves are `CHUNK_CELLS` cells a side; a node is split while the camera is nearer than `SPLIT_DISTANCE`
/// times its size (game units), down to `MIN_CHUNK` (0.5 m cells). The planes are laid out in `CHUNK_SIZE` squares first.
const CHUNK_SIZE: f32 = 128.0;

/// The smallest leaf: 0.5 m cells within about 24 m of the camera, 1 m to 48, 2 m to 96, 4 m to 192, as the game's radial mesh's rings
/// (0.5 m to 32 m from its centre, 1 m to 64, 2 m to 96, 4 m to 128; RenderDoc, docs/re/water.md). With 4 m cells everywhere near the
/// camera the shore's waterline (where the vertex shader sinks the sea under land) was smeared over 4 m triangles: the sea reached up the
/// beach as a thin, blurred film where the game's edge is sharp.
const MIN_CHUNK: f32 = 16.0;
const CHUNK_CELLS: u32 = 32;
const SPLIT_DISTANCE: f32 = 1.5;

/// How far the waves move the surface up or down at most (game units): the chunks' culling margin.
const WAVE_MARGIN: f32 = 8.0;

/// P0 (displaced position, w 1), P1 (rest position, w 1), TEXCOORD0.x.
const OCEAN_VERTEX: u32 = 36;

/// A sea plane in chunks (game units).
struct OceanPlane {
    version: u64,
    x0: f32,
    z0: f32,
    y: f32,
    nx: u32,
    nz: u32,
    cw: f32,
    ch: f32,
}

/// A chunk to draw this frame: its vertices' first index in the frame's buffer.
struct OceanChunk {
    base: u32,
}

/// A node of a sea plane's quadtree: its rectangle (game units).
#[derive(Clone, Copy)]
struct SeaNode {
    x0: f32,
    z0: f32,
    w: f32,
    h: f32,
}

impl SeaNode {
    fn children(&self) -> [SeaNode; 4] {
        let (w, h) = (self.w * 0.5, self.h * 0.5);
        [(0.0, 0.0), (w, 0.0), (0.0, h), (w, h)].map(|(dx, dz)| SeaNode { x0: self.x0 + dx, z0: self.z0 + dz, w, h })
    }

    /// Whether it is split: the camera nearer than SPLIT_DISTANCE times its size (to its rectangle at the sea's height).
    fn splits(&self, eye: Float3, y: f32) -> bool {
        let size = self.w.max(self.h);
        let (dx, dz) = ((self.x0 - eye.x).max(eye.x - self.x0 - self.w).max(0.0), (self.z0 - eye.z).max(eye.z - self.z0 - self.h).max(0.0));
        size > MIN_CHUNK && (dx * dx + dz * dz + (eye.y - y) * (eye.y - y)).sqrt() < SPLIT_DISTANCE * size
    }

    fn contains(&self, x: f32, z: f32) -> bool {
        x >= self.x0 && x < self.x0 + self.w && z >= self.z0 && z < self.z0 + self.h
    }
}

/// The width of the leaf of a quadtree (`root`) holding a point, if the point is in it.
fn leaf_width_at(root: SeaNode, x: f32, z: f32, eye: Float3, y: f32) -> Option<f32> {
    let mut n = root;
    if !n.contains(x, z) {
        return None;
    }

    while n.splits(eye, y) {
        n = n.children().into_iter().find(|c| c.contains(x, z))?;
    }

    Some(n.w)
}

/// A chunk's vertices to make: its corner, cell size, cells a side, height and per edge (west, east,
/// south, north) how many of its cells one of the neighbour's spans.
struct ChunkJob {
    x0: f32,
    z0: f32,
    sx: f32,
    sz: f32,
    cells: u32,
    y: f32,
    edges: [u32; 4],
}

/// The index list of a chunk of `cells` x `cells` cells.
fn chunk_indices(cells: u32) -> Vec<u32> {
    let mut out = Vec::with_capacity((cells * cells * 6) as usize);
    for j in 0..cells {
        for i in 0..cells {
            let a = j * (cells + 1) + i;
            let b = a + cells + 1;
            out.extend([a, b, a + 1, a + 1, b, b + 1]);
        }
    }

    out
}

/// A chunk's vertices (P0 displaced, P1 at rest, TEXCOORD0.x 0): `height` at each, except on an edge next to a coarser chunk, where a
/// vertex between two of the neighbour's lies on the line between their heights.
fn chunk_vertices(job: &ChunkJob, out: &mut [f32], height: &(dyn Fn(f32, f32) -> f32 + Sync)) {
    let cells = job.cells;
    let at = |i: u32, j: u32| (job.x0 + i as f32 * job.sx, job.z0 + j as f32 * job.sz);
    let along = |on: bool, k: u32, r: u32, point: &dyn Fn(u32) -> (f32, f32)| -> Option<f32> {
        (on && r > 1 && k % r != 0).then(|| {
            let lo = k - k % r;
            let (a, b) = (point(lo), point(lo + r));
            let w = (k % r) as f32 / r as f32;
            height(a.0, a.1) * (1.0 - w) + height(b.0, b.1) * w
        })
    };

    for j in 0..=cells {
        for i in 0..=cells {
            let (x, z) = at(i, j);
            let h = along(i == 0, j, job.edges[0], &|j| at(0, j))
                .or_else(|| along(i == cells, j, job.edges[1], &|j| at(cells, j)))
                .or_else(|| along(j == 0, i, job.edges[2], &|i| at(i, 0)))
                .or_else(|| along(j == cells, i, job.edges[3], &|i| at(i, cells)))
                .unwrap_or_else(|| height(x, z));
            let v = ((j * (cells + 1) + i) * 9) as usize;
            out[v..v + 9].copy_from_slice(&[x, job.y + h, z, 1.0, x, job.y, z, 1.0, 0.0]);
        }
    }
}

/// What a terrain gives the sea bed (`Terrain::ground_at`): its heights, LF map, grid and matrix. The buffers are held, so the same buffer
/// is the same ground: a paint stroke publishes the terrain again every dab with the heights it had (rebuilding the sea bed each time cost
/// a millisecond a frame).
struct SeaBedGround {
    heights: Arc<Vec<f32>>,
    lf: Option<Arc<LfMap>>,
    shape: [u32; 21],
}

impl SeaBedGround {
    fn of(e: &Entity, t: &Terrain) -> Self {
        let mut shape = [0u32; 21];
        for (k, v) in e.world.m.iter().flatten().chain(&[t.origin_x, t.origin_z, t.step]).enumerate() {
            shape[k] = v.to_bits();
        }

        (shape[19], shape[20]) = (t.nx as u32, t.nz as u32);
        Self { heights: t.heights.clone(), lf: t.lf.clone(), shape }
    }

    fn same(&self, o: &Self) -> bool {
        let lf = match (&self.lf, &o.lf) {
            (Some(a), Some(b)) => Arc::ptr_eq(a, b),
            (a, b) => a.is_none() && b.is_none(),
        };

        lf && Arc::ptr_eq(&self.heights, &o.heights) && self.shape == o.shape
    }
}

/// A wind's sea: the table row, the surfaces, their textures and the whitecaps.
struct SeaAssets {
    row: SeaRow,
    sea: SeaSurface,
    swell: SeaSurface,
    sea_tex: (ID3D11Texture2D, ID3D11ShaderResourceView),
    swell_tex: (ID3D11Texture2D, ID3D11ShaderResourceView),
    whitecaps: Option<ID3D11ShaderResourceView>,
}

#[derive(Default)]
pub(crate) struct OceanGpu {
    planes: BTreeMap<EntityId, OceanPlane>,

    /// The render origin the planes, the chunks and the sea bed are relative to (game units relative to it, as the shaders get them).
    origin: Float3,

    /// The frame's chunk vertices (dynamic, grown when needed), and a chunk's index buffer and index count.
    vb: Option<ID3D11Buffer>,
    vb_floats: usize,
    ib: Option<(ID3D11Buffer, u32)>,

    /// The sea of `sea_state` (None when it could not be loaded: flat surfaces), and the key it was loaded for.
    sea: Option<SeaAssets>,
    sea_state: Option<String>,

    /// The keys of `wind_levels_tables`, in their sort order (read once).
    winds: Option<Vec<String>>,
    sea_bed: Option<ID3D11ShaderResourceView>,

    /// What the sea bed was built from: the scene's address, its terrains and the render origin, and the terrains' ground.
    sea_bed_key: (usize, u64, u32, u32),
    sea_bed_ground: Vec<SeaBedGround>,

    /// The scene's terrain entities (a hash of their ids) as of a structure revision: a streamed map context adds and drops terrains
    /// without touching the terrain revision.
    sea_bed_terrains: (u64, u64),

    /// The game units per scene unit the grids and the sea bed were built with (both are in game units).
    units: f32,

    /// Neutral sea / swell surfaces (0.5: no tilt) and an empty whitecap array.
    neutral: Option<ID3D11ShaderResourceView>,
    no_foam: Option<ID3D11ShaderResourceView>,
    clear: Option<ID3D11ShaderResourceView>,
}

/// The ground height of the scene's terrains at a point, if one covers it (the first that does).
fn ground_at(terrains: &[(&Entity, &wce_scene::render_scene::Terrain)], x: f32, z: f32) -> Option<f32> {
    terrains.iter().find_map(|(e, t)| t.ground_at(&e.world, x, z))
}

/// A dynamic R16G16 texture the CPU writes every frame.
fn dynamic_rg16(dev: &ID3D11Device, n: u32) -> Option<(ID3D11Texture2D, ID3D11ShaderResourceView)> {
    let td = D3D11_TEXTURE2D_DESC {
        Width: n, Height: n, MipLevels: 1, ArraySize: 1, Format: DXGI_FORMAT_R16G16_UNORM, SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
        Usage: D3D11_USAGE_DYNAMIC, BindFlags: D3D11_BIND_SHADER_RESOURCE as u32, CPUAccessFlags: D3D11_CPU_ACCESS_WRITE as u32, MiscFlags: 0,
    };

    let tex = dev.create_texture_2d(&td, None).ok()?;
    let srv = dev.create_shader_resource_view(&tex, None).ok()?;
    Some((tex, srv))
}

/// The whitecap array: a slice per frame, RG8.
fn whitecap_array(dev: &ID3D11Device, n: u32, slices: &[u8]) -> Option<ID3D11ShaderResourceView> {
    let size = (n * n * 2) as usize;
    let count = slices.len() / size;
    let td = D3D11_TEXTURE2D_DESC {
        Width: n, Height: n, MipLevels: 1, ArraySize: count as u32, Format: DXGI_FORMAT_R8G8_UNORM, SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
        Usage: D3D11_USAGE_IMMUTABLE, BindFlags: D3D11_BIND_SHADER_RESOURCE as u32, CPUAccessFlags: 0, MiscFlags: 0,
    };

    let init = slices.chunks_exact(size).map(|s| InitData::new(s, n * 2)).collect::<Vec<_>>();
    let mut sd = D3D11_SHADER_RESOURCE_VIEW_DESC { Format: DXGI_FORMAT_R8G8_UNORM, ViewDimension: D3D11_SRV_DIMENSION(D3D11_SRV_DIMENSION_TEXTURE2DARRAY), ..Default::default() };
    sd.Anonymous.Texture2DArray.MipLevels = 1;
    sd.Anonymous.Texture2DArray.ArraySize = count as u32;
    dev.create_texture_2d(&td, Some(&init)).and_then(|tex| dev.create_shader_resource_view(&tex, Some(&sd))).ok()
}

/// Writes rows of texels into a dynamic texture.
fn write_rows(ctx: &ID3D11DeviceContext, tex: &ID3D11Texture2D, texels: &[u16], row_texels: usize) {
    let Ok(mut m) = ctx.map(tex, 0, D3D11_MAP_WRITE_DISCARD, 0) else { return };
    let pitch = m.row_pitch();
    for (y, row) in texels.chunks_exact(row_texels).enumerate() {
        m.write(y * pitch, row);
    }
}

impl Renderer {

    /// The game shaders' constant buffers as the last frame left them (for comparisons with the game's: a memory dump, a capture).
    pub fn constant_buffers(&self) -> &crate::cbuffers::ConstantBuffers {
        &self.cbs
    }

    /// The battle winds a sea can be drawn with (`wind_levels_tables`, in its order; empty without the table).
    pub fn sea_states(&mut self) -> Vec<String> {
        if self.water.ocean.winds.is_none() {
            let winds = self.files.read("db/wind_levels_tables/wind_levels").ok().and_then(|b| parse_wind_levels(&b)).unwrap_or_default();
            self.water.ocean.winds = Some(winds.into_iter().map(|w| w.key).collect());
        }

        self.water.ocean.winds.clone().unwrap_or_default()
    }

    /// The sea surface (a key of `sea_surfaces_tables`, e.g. `wind_level_2`) a battle wind uses; `None` without the table or the row.
    pub fn sea_surface_of(&self, wind: &str) -> Option<String> {
        let winds = self.files.read("db/wind_levels_tables/wind_levels").ok().and_then(|b| parse_wind_levels(&b))?;
        winds.into_iter().find(|w| w.key == wind).map(|w| w.sea_surface).filter(|s| !s.is_empty())
    }

    /// The sea of the settings' wind: the table rows, the two surfaces, their textures and the whitecaps (once per wind).
    fn ocean_sea_state(&mut self) {
        let key = self.settings.sea_state.clone();
        if self.water.ocean.sea_state.as_deref() == Some(key.as_str()) {
            return;
        }

        wce_prof!("render.ocean_sea_state");
        self.water.ocean.sea_state = Some(key.clone());
        self.water.ocean.sea = None;
        let files = self.files.clone();
        let read = |path: &str| files.read(path).map_err(|e| format!("{path}: {e}"));
        let load = || -> Result<(SeaRow, SeaSurface, SeaSurface), String> {
            let winds = parse_wind_levels(&read("db/wind_levels_tables/wind_levels")?).ok_or("wind_levels_tables: not read")?;
            let wind = winds.iter().find(|w| w.key == key).ok_or_else(|| format!("no wind level '{key}'"))?;
            let rows = parse_sea_surfaces(&read("db/sea_surfaces_tables/sea_surfaces")?).ok_or("sea_surfaces_tables: not read")?;
            let row = rows.into_iter().find(|r| r.key == wind.sea_surface).ok_or_else(|| format!("no sea surface '{}'", wind.sea_surface))?;
            let sea = SeaSurface::parse(&read(&format!("{}_sea", row.key))?).ok_or_else(|| format!("{}_sea: not a sea surface", row.key))?;
            let swell = SeaSurface::parse(&read(&format!("{}_swell", row.key))?).ok_or_else(|| format!("{}_swell: not a sea surface", row.key))?;
            Ok((row, sea, swell))
        };

        let (row, sea, swell) = match load() {
            Ok(loaded) => loaded,
            Err(error) => {
                self.error(format!("sea '{key}': {error} (the sea is drawn flat)"));
                return;
            }
        };

        let (Some(sea_tex), Some(swell_tex)) = (dynamic_rg16(&self.dev, sea.n as u32), dynamic_rg16(&self.dev, swell.n as u32)) else { return };
        let whitecaps = if row.foam_enabled { whitecap_array(&self.dev, sea.n as u32, &sea.foam_slices(row.foam_decay_rate)) } else { None };
        self.water.ocean.sea = Some(SeaAssets { row, sea, swell, sea_tex, swell_tex, whitecaps });
    }

    /// The sea bed info texture of the scene's terrains over it (rebuilt when they change).
    fn ocean_sea_bed(&mut self, scene: &RenderScene) {
        // The texture covers game units; the terrains are in scene units. The terrains over it (a map context has hundreds, most farther).
        let (u, o) = (self.camera.units, self.camera.origin);
        let half = SEA_BED_HALF / u;
        let over = |e: &Entity, t: &wce_scene::render_scene::Terrain| {
            let m = &e.world.m;
            let (x1, z1) = (t.origin_x + (t.nx - 1) as f32 * t.step, t.origin_z + (t.nz - 1) as f32 * t.step);
            let (mut lo, mut hi) = ([f32::MAX; 2], [f32::MIN; 2]);
            for (x, z) in [(t.origin_x, t.origin_z), (x1, t.origin_z), (t.origin_x, z1), (x1, z1)] {
                let (wx, wz) = (m[0][0] * x + m[0][2] * z + m[0][3], m[2][0] * x + m[2][2] * z + m[2][3]);
                (lo[0], lo[1], hi[0], hi[1]) = (lo[0].min(wx), lo[1].min(wz), hi[0].max(wx), hi[1].max(wz));
            }

            // Around the render origin.
            hi[0] >= o.x - half && lo[0] <= o.x + half && hi[1] >= o.z - half && lo[1] <= o.z + half
        };

        let terrains = || scene.entities().iter()
            .filter(|e| scene.drawable(e))
            .filter_map(|e| match &e.payload {
                // A custom-mesh tile's own terrain is not its ground (its heights draw nothing; its floor is a terrain of its own).
                Payload::Terrain(t) if t.custom.is_none() => Some((e, t)),
                _ => None,
            })
            .filter(|(e, t)| over(e, t));

        // Which they are, as of a structure revision (a streamed map context adds and drops terrains without touching the terrain revision).
        if self.water.ocean.sea_bed_terrains.0 != scene.structure_revision() {
            let ids = terrains().fold(0xCBF2_9CE4_8422_2325u64, |h, (e, _)| (h ^ u64::from(e.id)).wrapping_mul(0x0100_0000_01B3));
            self.water.ocean.sea_bed_terrains = (scene.structure_revision(), ids);
        }

        let key = (scene as *const RenderScene as usize, self.water.ocean.sea_bed_terrains.1, o.x.to_bits(), o.y.to_bits() ^ o.z.to_bits().rotate_left(16));
        let terrains = terrains().collect::<Vec<_>>();
        let ground = terrains.iter().map(|(e, t)| SeaBedGround::of(e, t)).collect::<Vec<_>>();
        let same_ground = ground.len() == self.water.ocean.sea_bed_ground.len() && ground.iter().zip(&self.water.ocean.sea_bed_ground).all(|(a, b)| a.same(b));
        if self.water.ocean.sea_bed.is_some() && self.water.ocean.sea_bed_key == key && same_ground {
            return;
        }

        wce_prof!("render.ocean_sea_bed");
        let n = SEA_BED_SIZE as usize;
        let texel = 2.0 * SEA_BED_HALF / SEA_BED_SIZE as f32;
        let [lo, range] = SEA_BED_RANGE;
        let mut data = vec![0.0f32; n * n];
        data.par_chunks_mut(n).enumerate().for_each(|(j, row)| {
            let z = -SEA_BED_HALF + (j as f32 + 0.5) * texel;
            for (i, out) in row.iter_mut().enumerate() {
                let x = -SEA_BED_HALF + (i as f32 + 0.5) * texel;
                let h = ground_at(&terrains, x / u + o.x, z / u + o.z).map_or(lo, |h| (h - o.y) * u);
                *out = ((h - lo) / range).clamp(0.0, 1.0);
            }
        });

        self.water.ocean.sea_bed = immutable_srv(&self.dev, SEA_BED_SIZE, SEA_BED_SIZE, DXGI_FORMAT_R32_FLOAT, &f32_bytes(&data));
        self.water.ocean.sea_bed_key = key;
        self.water.ocean.sea_bed_ground = ground;
    }

    /// The neutral inputs that stand in for what is not traced yet.
    fn ocean_neutral(&mut self) {
        let o = &mut self.water.ocean;
        if o.neutral.is_none() {
            // f16 0.5: an angle of 0 after the shader's (x - 0.5) * 2 pi.
            let half = 0x3800u16.to_le_bytes();
            o.neutral = solid_srv(&self.dev, DXGI_FORMAT_R16G16B16A16_FLOAT, &[half, half, half, half].concat());
            o.clear = solid_srv(&self.dev, DXGI_FORMAT_R8G8B8A8_UNORM, &[0, 0, 0, 0]);
        }

        if o.no_foam.is_none() {
            let td = D3D11_TEXTURE2D_DESC {
                Width: 1, Height: 1, MipLevels: 1, ArraySize: 1, Format: DXGI_FORMAT_R8G8B8A8_UNORM, SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
                Usage: D3D11_USAGE_IMMUTABLE, BindFlags: D3D11_BIND_SHADER_RESOURCE as u32, CPUAccessFlags: 0, MiscFlags: 0,
            };

            let mut sd = D3D11_SHADER_RESOURCE_VIEW_DESC { Format: DXGI_FORMAT_R8G8B8A8_UNORM, ViewDimension: D3D11_SRV_DIMENSION(D3D11_SRV_DIMENSION_TEXTURE2DARRAY), ..Default::default() };
            sd.Anonymous.Texture2DArray.MipLevels = 1;
            sd.Anonymous.Texture2DArray.ArraySize = 1;
            o.no_foam = self.dev.create_texture_2d(&td, Some(&[InitData::new(&[0u8; 4], 4)]))
                .and_then(|tex| self.dev.create_shader_resource_view(&tex, Some(&sd)))
                .ok();
        }
    }

    /// Draws the sea planes of the scene with the ocean shaders, into the same targets as the other water (the G-buffer channels 1 .. 3 and
    /// the lit colour), stencil 1. The caller has made the refraction copy and set the depth-stencil state.
    pub(crate) fn draw_ocean(&mut self, scene: &RenderScene, seas: &[&Entity]) -> bool {
        let seas = seas.iter().copied().filter(|e| matches!(&e.payload, Payload::Water(w) if !w.river) && scene.drawable(e)).collect::<Vec<_>>();
        if seas.is_empty() {
            return false;
        }

        // Rome 2's ocean (sm_3_0 ocean.hlsl) is a forward pass: the lit colour only, its constants and textures named otherwise
        // (docs/re/rome2_renderer.md, "Sea"); the same chunks and surfaces.
        let rome2 = self.game == super::game::RenderGame::Rome2;
        let ocean_ps = ocean_ps(if rome2 { 3 } else { self.settings.water_quality }, rome2);
        let (Some(vs), Some(ps)) = (self.shader(if rome2 { R2_OCEAN_VS } else { OCEAN_VS }), self.shader(&ocean_ps)) else { return false };
        let f4 = DXGI_FORMAT_R32G32B32A32_FLOAT;
        let el = [element(c"POSITION", 0, f4, 0, 0, false), element(c"POSITION", 1, f4, 0, 16, false), element(c"TEXCOORD", 0, DXGI_FORMAT_R32_FLOAT, 0, 32, false)];
        let Some(layout) = self.layout(&vs, if rome2 { "ocean_rome2" } else { "ocean" }, &el) else { return false };

        wce_prof!("render.ocean");
        let u = self.camera.units;
        if self.water.ocean.units != u {
            self.water.ocean.units = u;
            self.water.ocean.planes.clear();
            self.water.ocean.sea_bed = None;
        }

        self.ocean_sea_bed(scene);
        self.ocean_neutral();
        self.ocean_sea_state();

        // The planes' chunk layouts, made again when a plane's version (or the render origin) moves on.
        let o = self.camera.origin;
        if self.water.ocean.origin != o {
            self.water.ocean.origin = o;
            self.water.ocean.planes.clear();
        }

        let mut keep = BTreeMap::new();
        for e in &seas {
            let Payload::Water(w) = &e.payload else { continue };
            if let Some(p) = self.water.ocean.planes.remove(&e.id).filter(|p| p.version == w.version) {
                keep.insert(e.id, p);
                continue;
            }

            let (mut x0, mut z0, mut x1, mut z1, mut y) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN, 0.0);
            for v in &w.verts {
                let (x, z) = (v.x - o.x, v.z - o.z);
                (x0, z0, x1, z1, y) = (x0.min(x), z0.min(z), x1.max(x), z1.max(z), v.y - o.y);
            }

            // In game units, as the ocean shaders' distances and the sea bed texture.
            let (x0, z0, x1, z1) = (x0 * u, z0 * u, x1 * u, z1 * u);
            if x1 <= x0 || z1 <= z0 {
                continue;
            }

            let (nx, nz) = (((x1 - x0) / CHUNK_SIZE).ceil().max(1.0) as u32, ((z1 - z0) / CHUNK_SIZE).ceil().max(1.0) as u32);
            keep.insert(e.id, OceanPlane { version: w.version, x0, z0, y: y * u, nx, nz, cw: (x1 - x0) / nx as f32, ch: (z1 - z0) / nz as f32 });
        }

        self.water.ocean.planes = keep;
        if self.water.ocean.ib.is_none() {
            let idx = chunk_indices(CHUNK_CELLS);
            let bytes = idx.iter().flat_map(|i| i.to_le_bytes()).collect::<Vec<_>>();
            self.water.ocean.ib = immutable_buffer(&self.dev, &bytes, D3D11_BIND_INDEX_BUFFER).map(|ib| (ib, idx.len() as u32));
        }

        // The surfaces at this frame's time: their textures.
        let t = self.clock.last_time.max(0.0);
        let at = self.water.ocean.sea.as_ref().map(|s| (s.sea.frame_at(t), s.swell.frame_at(t)));
        if let (Some(s), Some((sea_at, swell_at))) = (&self.water.ocean.sea, at) {
            wce_prof!("render.ocean_surfaces");
            write_rows(&self.ctx, &s.sea_tex.0, &s.sea.angle_texels(sea_at), s.sea.n * 2);
            write_rows(&self.ctx, &s.swell_tex.0, &s.swell.angle_texels(swell_at), s.swell.n * 2);
        }

        // The chunks in view, their levels by distance, and their vertices displaced by the surfaces' heights.
        let frustum = crate::draw_set::Frustum::from_view_projection(&self.camera.view_proj);
        let eye = (self.camera.eye - o) * u;
        let mut jobs = vec![];
        let mut chunks = vec![];
        let mut floats = 0usize;
        for p in self.water.ocean.planes.values() {
            let root = SeaNode { x0: p.x0, z0: p.z0, w: p.cw * p.nx as f32, h: p.ch * p.nz as f32 };
            let mut stack = vec![root];
            while let Some(n) = stack.pop() {
                let r = 0.5 * (n.w * n.w + n.h * n.h + 4.0 * WAVE_MARGIN * WAVE_MARGIN).sqrt();
                if frustum.outside(&[(n.x0 + 0.5 * n.w) / u + o.x, p.y / u + o.y, (n.z0 + 0.5 * n.h) / u + o.z], r / u) {
                    continue;
                }

                if n.splits(eye, p.y) {
                    stack.extend(n.children());
                    continue;
                }

                // Per edge (west, east, south, north): how many of this chunk's cells one of the neighbour's spans (the neighbour leaf at
                // the edge's middle; a larger one spans the whole edge).
                let (step, half) = (0.25 * MIN_CHUNK / CHUNK_CELLS as f32, (0.5 * n.w, 0.5 * n.h));
                let ratio = |x: f32, z: f32| leaf_width_at(root, x, z, eye, p.y).filter(|&w| w > n.w * 1.001).map_or(1, |w| (w / n.w).round() as u32);
                let edges = [
                    ratio(n.x0 - step, n.z0 + half.1), ratio(n.x0 + n.w + step, n.z0 + half.1),
                    ratio(n.x0 + half.0, n.z0 - step), ratio(n.x0 + half.0, n.z0 + n.h + step),
                ];

                let cells = CHUNK_CELLS;
                chunks.push(OceanChunk { base: (floats / 9) as u32 });
                jobs.push(ChunkJob { x0: n.x0, z0: n.z0, sx: n.w / cells as f32, sz: n.h / cells as f32, cells, y: p.y, edges });
                floats += ((cells + 1) * (cells + 1) * 9) as usize;
            }
        }

        let mut verts = vec![0.0f32; floats];
        {
            wce_prof!("render.ocean_chunks");
            // The surfaces only (the textures beside them are not shared across threads).
            let sea = self.water.ocean.sea.as_ref().map(|s| (&s.sea, &s.swell)).zip(at);
            let height = |x: f32, z: f32| sea.map_or(0.0, |((sea, swell), (sea_at, swell_at))| sea.height_at(sea_at, x, z) + swell.height_at(swell_at, x, z));
            let mut parts = vec![];
            let mut rest = verts.as_mut_slice();
            for job in &jobs {
                let (head, tail) = rest.split_at_mut(((job.cells + 1) * (job.cells + 1) * 9) as usize);
                parts.push((job, head));
                rest = tail;
            }

            parts.into_par_iter().for_each(|(job, out)| chunk_vertices(job, out, &height));
        }

        if floats > self.water.ocean.vb_floats {
            let size = floats.next_power_of_two().max(1 << 16);
            self.water.ocean.vb = dynamic_buffer(&self.dev, size * 4, D3D11_BIND_VERTEX_BUFFER);
            self.water.ocean.vb_floats = if self.water.ocean.vb.is_some() { size } else { 0 };
        }

        if floats > 0
            && let Some(vb) = &self.water.ocean.vb
            && let Ok(mut map) = self.ctx.map(vb, 0, D3D11_MAP_WRITE_DISCARD, 0)
        {
            map.write(0, &verts);
        }

        // Without the sea's files: the calm row's values (the dumps').
        let row = self.water.ocean.sea.as_ref().map(|s| s.row.clone()).unwrap_or_else(|| SeaRow {
            froth_distortion_speed: 0.35, froth_distortion_amount: 0.05, sea_decay: 0.0006, detail_normal: [7.8, 6.8, 15.8, 5.0], ..Default::default()
        });

        let (sea_uv, swell_uv, heights) = self.water.ocean.sea.as_ref().map_or((1.0 / 70.0, 1.0 / 800.0, [0.0, 0.0]), |s| {
            (1.0 / s.sea.patch, 1.0 / s.swell.patch, [s.sea.height_min, s.sea.height_max - s.sea.height_min])
        });

        let info = [SEA_BED_RANGE[0], SEA_BED_RANGE[1], heights[0], heights[1]];
        let foam_anim = match (&self.water.ocean.sea, at) {
            (Some(s), Some((sea_at, _))) if s.whitecaps.is_some() => [sea_at.frame as f32, sea_at.next as f32, sea_at.blend, 0.0],
            _ => [0.0; 4],
        };

        let detail = row.detail_normal_constants();
        let cbs = &mut self.cbs;
        cbs.set_f("ocean_VS", "g_sea_decay", &[row.sea_decay]);
        cbs.set_f("ocean_VS", "g_sea_uv_scale", &[sea_uv]);
        cbs.set_f("ocean_VS", "g_swell_uv_scale", &[swell_uv]);
        cbs.set_f("ocean_VS", "g_vs_sea_info", &info);
        cbs.set_f("ocean_VS", "g_vs_sea_bed_info", &info);
        cbs.set_f("ocean_PS", "g_ps_sea_bed_info", &info);
        cbs.set_f("ocean_PS", "g_detail_normal_uv_scale1", &[detail[0]]);
        cbs.set_f("ocean_PS", "g_detail_normal_uv_scale2", &[detail[1]]);
        cbs.set_f("ocean_PS", "g_detail_normal_uv_speed1", &[detail[2]]);
        cbs.set_f("ocean_PS", "g_detail_normal_uv_speed2", &[detail[3]]);
        cbs.set_f("OceanSharedConstants", "g_froth_distortion_speed", &[row.froth_distortion_speed]);
        cbs.set_f("OceanSharedConstants", "g_froth_distortion_amount", &[row.froth_distortion_amount]);
        cbs.set_f("OceanSharedConstants", "g_lowest_wave_trough_y_pos", &[row.wave_trough_y_value]);
        cbs.set_f("OceanSharedConstants", "g_ps_sea_info", &info);
        cbs.set_f("OceanSharedConstants", "g_detail_normal_uv_scale1", &[detail[0]]);
        cbs.set_f("OceanSharedConstants", "g_detail_normal_uv_scale2", &[detail[1]]);
        cbs.set_f("OceanSharedConstants", "g_detail_normal_uv_speed1", &[detail[2]]);
        cbs.set_f("OceanSharedConstants", "g_detail_normal_uv_speed2", &[detail[3]]);
        cbs.set_f("OceanSharedConstants", "g_transparency_exponential_scale", &[TRANSPARENCY_EXPONENTIAL_SCALE]);
        cbs.set_f("OceanSharedConstants", "g_foam_anim_data", &foam_anim);
        self.upload();

        let rtvs = if rome2 {
            vec![self.gbuffer.scene_hdr_rtv()]
        } else {
            self.gbuffer.scene_rtvs()[..3].iter().cloned().chain([self.gbuffer.scene_hdr_rtv()]).collect()
        };
        null_ps_srvs(&self.ctx, 16);
        self.ctx.set_render_targets(&rtvs, self.gbuffer.scene_dsv().as_ref());
        self.ctx.set_depth_stencil_state(self.water.ds.as_ref(), 1);
        self.ctx.set_blend_state(self.states.bs_opaque.as_ref());
        self.ctx.set_rasterizer_state(self.states.rs_none.as_ref());
        self.ctx.set_input_layout(Some(&layout));
        self.ctx.set_topology(D3D11_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
        self.bind(&vs);
        self.bind(&ps);

        let srv = |t: Option<crate::model_cache::SharedTexture>| t.and_then(|t| t.read().ok().and_then(|t| t.srv.clone()));
        let detail = srv(self.textures.get("sea/water_detail_normal.dds", false)).or_else(|| self.res.black_srv.clone());
        let foam = srv(self.textures.get("sea/combined_foam.dds", false)).or_else(|| self.res.black_srv.clone());
        let ctx = &self.ctx;
        let o = &self.water.ocean;

        // Samplers as the engine declares them (docs/re/samplers.tsv, the ocean material fn 0x116e1f20): trilinear, the foam / surfaces /
        // detail normals wrapped, the overlays and the sea bed clamped; the depth channel and the refraction as the other water passes.
        bind_sampler(ctx, &vs, "sea_bed_info_texture_sampler", self.states.s_clamp_linear.as_ref());
        bind_srv(ctx, &vs, "sea_bed_info_texture_sampler", o.sea_bed.as_ref());
        ps_samplers(ctx, &ps, &self.states.s_wrap_linear);
        for s in ["colour_overlay_texture_sampler", "ui_colour_overlay_texture_sampler", "refraction_texture_sampler"] {
            bind_sampler(ctx, &ps, s, self.states.s_clamp_linear.as_ref());
        }

        bind_sampler(ctx, &ps, "gbuffer_channel_4_sampler", self.states.s_clamp_point.as_ref());
        bind_srv(ctx, &ps, "detail_normal_sampler", detail.as_ref());
        let (sea_srv, swell_srv) = o.sea.as_ref().map_or((o.neutral.clone(), o.neutral.clone()), |s| (Some(s.sea_tex.1.clone()), Some(s.swell_tex.1.clone())));
        let whitecaps = o.sea.as_ref().and_then(|s| s.whitecaps.clone()).or_else(|| o.no_foam.clone());
        bind_srv(ctx, &ps, "sea_surface_sampler", sea_srv.as_ref());
        bind_srv(ctx, &ps, "swell_surface_sampler", swell_srv.as_ref());
        bind_srv(ctx, &ps, "gbuffer_channel_4_sampler", self.gbuffer.depth_srv().as_ref());
        bind_srv(ctx, &ps, "s_environment", if self.env.valid { self.env.cube.srv.as_ref() } else { self.sky.cube.srv.as_ref() });
        bind_srv(ctx, &ps, "refraction_texture_sampler", self.water.refr.srv.as_ref());
        bind_srv(ctx, &ps, "colour_overlay_texture_sampler", o.clear.as_ref());
        bind_srv(ctx, &ps, "ui_colour_overlay_texture_sampler", o.clear.as_ref());
        bind_srv(ctx, &ps, "g_foam_anim_texture", whitecaps.as_ref());
        bind_srv(ctx, &ps, "g_static_foam_texture", foam.as_ref());

        // Rome 2's names: the sky cube, the foam texture; its planar reflection is not rendered (black: no reflected objects, the sky's
        // reflection comes from the cube).
        let sky = if self.env.valid { self.env.cube.srv.as_ref() } else { self.sky.cube.srv.as_ref() };
        bind_srv(ctx, &ps, "s_sky", sky);
        bind_srv(ctx, &ps, "combined_foam_texture_sampler", foam.as_ref());
        bind_srv(ctx, &ps, "reflection_texture_sampler", self.res.black_srv.as_ref());
        bind_sampler(ctx, &ps, "sea_bed_info_texture_sampler", self.states.s_clamp_linear.as_ref());
        bind_srv(ctx, &ps, "sea_bed_info_texture_sampler", o.sea_bed.as_ref());

        if o.vb.is_some()
            && let Some((ib, count)) = &o.ib
        {
            ctx.set_vertex_buffers(0, &[o.vb.clone()], &[OCEAN_VERTEX], &[0]);
            ctx.set_index_buffer(Some(ib), DXGI_FORMAT_R32_UINT, 0);
            for c in &chunks {
                ctx.draw_indexed(*count, 0, c.base as i32);
                self.draw_stats.draw_calls += 1;
            }
        }

        null_ps_srvs(&self.ctx, 16);
        true
    }
}
