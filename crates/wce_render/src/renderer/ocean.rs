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
//! - The mesh is a grid instead of the game's 80 x 164 radial mesh around the camera: each sea plane in chunks of up to 128 m, the
//!   chunks outside the view skipped and the others 4 m apart near the camera, coarser with distance (8 m from 400 m, 16 m from 800 m
//!   ... one cell from 6400 m); a finer chunk's edge follows its coarser neighbour's (no cracks). Its vertices are displaced by the sea
//!   and swell heights (the frames' height images, blended like the angles) on the CPU every frame. (A fixed 4 m grid over the sea bed
//!   square was 260 000 vertices a frame: ~2 ms, and 9 MB uploaded.)
//!   The game's vertex shader takes a displaced and a rest position, and the surfaces carry heights whose range is in the shader's
//!   constants (`g_vs_sea_info.zw`), but how the engine combines them (and whether it uses the frames' slope images, unused here) is
//!   not known.
//! - The sea bed info texture (`sea_bed_info_texture`, 512 x 512 over x, z in [-1024, 1024]: the vertex shader reads red as the ground
//!   height, `r * g_vs_sea_info.y + g_vs_sea_info.x`, -20 .. 5 m) is built from the scene's terrains; what the engine writes into its own
//!   512 x 512 texture has not been traced. Near the shore the vertex shader flattens the waves into a small ripple, and pushes the sea
//!   4 m down where the ground is above it.
//! - `g_transparency_exponential_scale` (4) and the sea bed range (-20, 25) are the dumps' values; they are not in the tables.

use std::collections::BTreeMap;

use rayon::prelude::*;
use wce_foundation::wce_prof;
use wce_scene::render_scene::{Entity, EntityId, Payload, RenderScene};

use super::*;
use crate::sea_surface::{SeaRow, SeaSurface, parse_sea_surfaces, parse_wind_levels};
use crate::shader_lib::{bind_sampler, bind_srv};

const OCEAN_VS: &str = "dx11_sm_4_0_ocean.hlsl_vs_vs30_main";

/// The "ultra" quality level.
const OCEAN_PS: &str = "dx11_sm_4_0_ocean.hlsl_ps_ps30_main_ultra_water_quality";

/// The sea bed info texture: its size and the square it covers (the vertex shader's `(x + 1024) / 2048`).
const SEA_BED_SIZE: u32 = 512;
const SEA_BED_HALF: f32 = 1024.0;

/// The sea bed's height range (`g_vs_sea_info.xy`) and `g_transparency_exponential_scale`: the dumps' values, not in the tables.
const SEA_BED_RANGE: [f32; 2] = [-20.0, 25.0];
const TRANSPARENCY_EXPONENTIAL_SCALE: f32 = 4.0;

/// The sea grid's chunks: at most this wide (game units), in at most `CHUNK_CELLS` cells a side (4 m, the sea bed's texel), halved
/// per level of distance: level k from `LOD_DISTANCE * 2^k`.
const CHUNK_SIZE: f32 = 128.0;
const CHUNK_CELLS: u32 = 32;
const CHUNK_LEVELS: u32 = 6;
const LOD_DISTANCE: f32 = 200.0;

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

/// A chunk to draw this frame: its vertices' first index in the frame's buffer and its level.
struct OceanChunk {
    base: u32,
    level: u32,
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

    /// The frame's chunk vertices (dynamic, grown when needed), and per level its index buffer and index count.
    vb: Option<ID3D11Buffer>,
    vb_floats: usize,
    ibs: Vec<(ID3D11Buffer, u32)>,

    /// The sea of `sea_state` (None when it could not be loaded: flat surfaces), and the key it was loaded for.
    sea: Option<SeaAssets>,
    sea_state: Option<String>,

    /// The keys of `wind_levels_tables`, in their sort order (read once).
    winds: Option<Vec<String>>,
    sea_bed: Option<ID3D11ShaderResourceView>,

    /// What the sea bed was built from: the scene's address and terrain revision.
    sea_bed_key: (usize, u64),

    /// The game units per scene unit the grids and the sea bed were built with (both are in game units).
    units: f32,

    /// Neutral sea / swell surfaces (0.5: no tilt) and an empty whitecap array.
    neutral: Option<ID3D11ShaderResourceView>,
    no_foam: Option<ID3D11ShaderResourceView>,
    clear: Option<ID3D11ShaderResourceView>,
}

/// The ground height of the scene's terrains at a point, if one covers it (the first that does).
fn ground_at(terrains: &[(&Entity, &wce_scene::render_scene::Terrain)], x: f32, z: f32) -> Option<f32> {
    for (e, t) in terrains {
        if let Some(sh) = &t.shared {
            let (fi, fj) = ((x - sh.origin) / sh.step, (z - sh.origin) / sh.step);
            if fi < 0.0 || fj < 0.0 || fi > (sh.n - 1) as f32 || fj > (sh.n - 1) as f32 {
                continue;
            }

            let (i, j) = (fi.floor() as i32, fj.floor() as i32);
            let (u, v) = (fi - i as f32, fj - j as f32);
            let h = |a: i32, b: i32| sh.height_at(a, b);
            return Some((h(i, j) * (1.0 - u) + h(i + 1, j) * u) * (1.0 - v) + (h(i, j + 1) * (1.0 - u) + h(i + 1, j + 1) * u) * v);
        }

        // The terrain's own frame: translated and scaled, never rotated.
        let m = &e.world.m;
        let (sx, sy, sz) = (m[0][0], m[1][1], m[2][2]);
        if sx == 0.0 || sz == 0.0 {
            continue;
        }

        let (lx, lz) = ((x - m[0][3]) / sx, (z - m[2][3]) / sz);
        let (fi, fj) = ((lx - t.origin_x) / t.step, (lz - t.origin_z) / t.step);
        if fi < 0.0 || fj < 0.0 || fi > (t.nx - 1) as f32 || fj > (t.nz - 1) as f32 {
            continue;
        }

        let (i, j) = ((fi.floor() as i32).min(t.nx - 2).max(0), (fj.floor() as i32).min(t.nz - 2).max(0));
        let (u, v) = (fi - i as f32, fj - j as f32);
        let h = |a: i32, b: i32| t.height_at(a, b);
        let local = (h(i, j) * (1.0 - u) + h(i + 1, j) * u) * (1.0 - v) + (h(i, j + 1) * (1.0 - u) + h(i + 1, j + 1) * u) * v;
        return Some(local * sy + m[1][3]);
    }

    None
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

    /// The battle winds a sea can be drawn with (`wind_levels_tables`, in its order; empty without the table).
    pub fn sea_states(&mut self) -> Vec<String> {
        if self.water.ocean.winds.is_none() {
            let winds = self.files.read("db/wind_levels_tables/wind_levels").ok().and_then(|b| parse_wind_levels(&b)).unwrap_or_default();
            self.water.ocean.winds = Some(winds.into_iter().map(|w| w.key).collect());
        }

        self.water.ocean.winds.clone().unwrap_or_default()
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

    /// The sea bed info texture of the scene's terrains (rebuilt when they change).
    fn ocean_sea_bed(&mut self, scene: &RenderScene) {
        let key = (scene as *const RenderScene as usize, scene.terrain_revision());
        if self.water.ocean.sea_bed.is_some() && self.water.ocean.sea_bed_key == key {
            return;
        }

        // The texture covers game units; the terrains are in scene units.
        let u = self.camera.units;
        wce_prof!("render.ocean_sea_bed");
        let terrains = scene.entities().iter()
            .filter(|e| scene.drawable(e))
            .filter_map(|e| match &e.payload {
                Payload::Terrain(t) => Some((e, t)),
                _ => None,
            })
            .collect::<Vec<_>>();

        let n = SEA_BED_SIZE as usize;
        let texel = 2.0 * SEA_BED_HALF / SEA_BED_SIZE as f32;
        let [lo, range] = SEA_BED_RANGE;
        let mut data = vec![0.0f32; n * n];
        data.par_chunks_mut(n).enumerate().for_each(|(j, row)| {
            let z = -SEA_BED_HALF + (j as f32 + 0.5) * texel;
            for (i, out) in row.iter_mut().enumerate() {
                let x = -SEA_BED_HALF + (i as f32 + 0.5) * texel;
                let h = ground_at(&terrains, x / u, z / u).map_or(lo, |h| h * u);
                *out = ((h - lo) / range).clamp(0.0, 1.0);
            }
        });

        self.water.ocean.sea_bed = immutable_srv(&self.dev, SEA_BED_SIZE, SEA_BED_SIZE, DXGI_FORMAT_R32_FLOAT, &f32_bytes(&data));
        self.water.ocean.sea_bed_key = key;
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

        let (Some(vs), Some(ps)) = (self.shader(OCEAN_VS), self.shader(OCEAN_PS)) else { return false };
        let f4 = DXGI_FORMAT_R32G32B32A32_FLOAT;
        let el = [element(c"POSITION", 0, f4, 0, 0, false), element(c"POSITION", 1, f4, 0, 16, false), element(c"TEXCOORD", 0, DXGI_FORMAT_R32_FLOAT, 0, 32, false)];
        let Some(layout) = self.layout(&vs, "ocean", &el) else { return false };

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

        // The planes' chunk layouts, made again when a plane's version moves on.
        let mut keep = BTreeMap::new();
        for e in &seas {
            let Payload::Water(w) = &e.payload else { continue };
            if let Some(p) = self.water.ocean.planes.remove(&e.id).filter(|p| p.version == w.version) {
                keep.insert(e.id, p);
                continue;
            }

            let (mut x0, mut z0, mut x1, mut z1, mut y) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN, 0.0);
            for v in &w.verts {
                (x0, z0, x1, z1, y) = (x0.min(v.x), z0.min(v.z), x1.max(v.x), z1.max(v.z), v.y);
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
        if self.water.ocean.ibs.is_empty() {
            for level in 0..CHUNK_LEVELS {
                let idx = chunk_indices(CHUNK_CELLS >> level);
                let bytes = idx.iter().flat_map(|i| i.to_le_bytes()).collect::<Vec<_>>();
                if let Some(ib) = immutable_buffer(&self.dev, &bytes, D3D11_BIND_INDEX_BUFFER) {
                    self.water.ocean.ibs.push((ib, idx.len() as u32));
                }
            }
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
        let eye = self.camera.eye * u;
        let mut jobs = vec![];
        let mut chunks = vec![];
        let mut floats = 0usize;
        for p in self.water.ocean.planes.values() {
            let centre = |ci: i64, cj: i64| (p.x0 + (ci as f32 + 0.5) * p.cw, p.z0 + (cj as f32 + 0.5) * p.ch);
            let level = |ci: i64, cj: i64| {
                (ci >= 0 && cj >= 0 && ci < i64::from(p.nx) && cj < i64::from(p.nz)).then(|| {
                    let (cx, cz) = centre(ci, cj);
                    let d = wce_foundation::math::length(Float3::new(cx, p.y, cz) - eye);
                    ((d / LOD_DISTANCE).max(1.0).log2().floor() as u32).min(CHUNK_LEVELS - 1)
                })
            };

            let r = 0.5 * (p.cw * p.cw + p.ch * p.ch + 4.0 * WAVE_MARGIN * WAVE_MARGIN).sqrt();
            for cj in 0..i64::from(p.nz) {
                for ci in 0..i64::from(p.nx) {
                    let (cx, cz) = centre(ci, cj);
                    if frustum.outside(&[cx / u, p.y / u, cz / u], r / u) {
                        continue;
                    }

                    let l = level(ci, cj).unwrap_or(0);
                    let cells = CHUNK_CELLS >> l;
                    let ratio = |n: Option<u32>| n.filter(|&n| n > l).map_or(1, |n| 1u32 << (n - l));
                    let edges = [ratio(level(ci - 1, cj)), ratio(level(ci + 1, cj)), ratio(level(ci, cj - 1)), ratio(level(ci, cj + 1))];
                    chunks.push(OceanChunk { base: (floats / 9) as u32, level: l });
                    jobs.push(ChunkJob {
                        x0: p.x0 + ci as f32 * p.cw, z0: p.z0 + cj as f32 * p.ch, sx: p.cw / cells as f32, sz: p.ch / cells as f32, cells,
                        y: p.y, edges,
                    });

                    floats += ((cells + 1) * (cells + 1) * 9) as usize;
                }
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

        let rtvs = [self.gbuffer.channels[0].rtv.clone(), self.gbuffer.channels[1].rtv.clone(), self.gbuffer.channels[2].rtv.clone(), self.gbuffer.hdr.rtv.clone()];
        null_ps_srvs(&self.ctx, 16);
        self.ctx.set_render_targets(&rtvs, self.gbuffer.dsv.as_ref());
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
        bind_srv(ctx, &ps, "gbuffer_channel_4_sampler", self.gbuffer.channels[3].srv.as_ref());
        bind_srv(ctx, &ps, "s_environment", if self.env.valid { self.env.cube.srv.as_ref() } else { self.sky.cube.srv.as_ref() });
        bind_srv(ctx, &ps, "refraction_texture_sampler", self.water.refr.srv.as_ref());
        bind_srv(ctx, &ps, "colour_overlay_texture_sampler", o.clear.as_ref());
        bind_srv(ctx, &ps, "ui_colour_overlay_texture_sampler", o.clear.as_ref());
        bind_srv(ctx, &ps, "g_foam_anim_texture", whitecaps.as_ref());
        bind_srv(ctx, &ps, "g_static_foam_texture", foam.as_ref());

        if o.vb.is_some() {
            ctx.set_vertex_buffers(0, &[o.vb.clone()], &[OCEAN_VERTEX], &[0]);
            let mut bound = None;
            for c in &chunks {
                let Some((ib, count)) = o.ibs.get(c.level as usize) else { continue };
                if bound != Some(c.level) {
                    ctx.set_index_buffer(Some(ib), DXGI_FORMAT_R32_UINT, 0);
                    bound = Some(c.level);
                }

                ctx.draw_indexed(*count, 0, c.base as i32);
                self.draw_stats.draw_calls += 1;
            }
        }

        null_ps_srvs(&self.ctx, 16);
        true
    }
}
