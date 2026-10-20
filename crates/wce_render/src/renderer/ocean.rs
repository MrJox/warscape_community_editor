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
//! - The mesh is a grid (4 m apart where the sea bed texture reaches, 64 m beyond) instead of the game's 80 x 164 radial mesh around the
//!   camera, and its vertices are displaced by the sea and swell heights (the frames' height images, blended like the angles) on the CPU.
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

/// Sea grid spacing: where the sea bed texture reaches (its texel size), and beyond.
const FINE_STEP: f32 = 4.0;
const COARSE_STEP: f32 = 64.0;

/// P0 (displaced position, w 1), P1 (rest position, w 1), TEXCOORD0.x.
const OCEAN_VERTEX: u32 = 36;

/// A sea plane's grid: its rest vertices (game units) and the buffers.
struct OceanMesh {
    version: u64,
    rest: Vec<f32>,
    vb: ID3D11Buffer,
    ib: ID3D11Buffer,
    count: u32,
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
    meshes: BTreeMap<EntityId, OceanMesh>,

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

/// The grid of a sea plane: 4 m apart inside the sea bed square, 64 m outside it.
fn sea_grid(x0: f32, z0: f32, x1: f32, z1: f32, y: f32) -> (Vec<f32>, Vec<u32>) {
    let (mut verts, mut indices) = (vec![], vec![]);
    let mut rect = |x0: f32, z0: f32, x1: f32, z1: f32, step: f32| {
        if x1 <= x0 || z1 <= z0 {
            return;
        }

        let (nx, nz) = (((x1 - x0) / step).ceil().max(1.0) as u32, ((z1 - z0) / step).ceil().max(1.0) as u32);
        let first = (verts.len() / 9) as u32;
        for j in 0..=nz {
            let z = (z0 + j as f32 * step).min(z1);
            for i in 0..=nx {
                let x = (x0 + i as f32 * step).min(x1);
                verts.extend([x, y, z, 1.0, x, y, z, 1.0, 0.0]);
            }
        }

        for j in 0..nz {
            for i in 0..nx {
                let a = first + j * (nx + 1) + i;
                let b = a + nx + 1;
                indices.extend([a, b, a + 1, a + 1, b, b + 1]);
            }
        }
    };

    // The fine square, then the coarse bands around it.
    let (fx0, fz0, fx1, fz1) = (x0.max(-SEA_BED_HALF), z0.max(-SEA_BED_HALF), x1.min(SEA_BED_HALF), z1.min(SEA_BED_HALF));
    if fx1 > fx0 && fz1 > fz0 {
        rect(fx0, fz0, fx1, fz1, FINE_STEP);
        rect(x0, z0, x1, fz0, COARSE_STEP);
        rect(x0, fz1, x1, z1, COARSE_STEP);
        rect(x0, fz0, fx0, fz1, COARSE_STEP);
        rect(fx1, fz0, x1, fz1, COARSE_STEP);
    } else {
        rect(x0, z0, x1, z1, COARSE_STEP);
    }

    (verts, indices)
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
            self.water.ocean.meshes.clear();
            self.water.ocean.sea_bed = None;
        }

        self.ocean_sea_bed(scene);
        self.ocean_neutral();
        self.ocean_sea_state();

        // The grids, rebuilt when a plane's version moves on.
        let mut keep = BTreeMap::new();
        for e in &seas {
            let Payload::Water(w) = &e.payload else { continue };
            if let Some(m) = self.water.ocean.meshes.remove(&e.id).filter(|m| m.version == w.version) {
                keep.insert(e.id, m);
                continue;
            }

            let (mut x0, mut z0, mut x1, mut z1, mut y) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN, 0.0);
            for v in &w.verts {
                (x0, z0, x1, z1, y) = (x0.min(v.x), z0.min(v.z), x1.max(v.x), z1.max(v.z), v.y);
            }

            // In game units, as the ocean shaders' distances and the sea bed texture.
            let (rest, indices) = sea_grid(x0 * u, z0 * u, x1 * u, z1 * u, y * u);
            let idx = indices.iter().flat_map(|i| i.to_le_bytes()).collect::<Vec<_>>();
            let (Some(vb), Some(ib)) = (dynamic_buffer(&self.dev, rest.len() * 4, D3D11_BIND_VERTEX_BUFFER), immutable_buffer(&self.dev, &idx, D3D11_BIND_INDEX_BUFFER)) else {
                continue;
            };

            keep.insert(e.id, OceanMesh { version: w.version, rest, vb, ib, count: indices.len() as u32 });
        }

        self.water.ocean.meshes = keep;

        // The surfaces at this frame's time: their textures, and the grids displaced by their heights.
        let t = self.clock.last_time.max(0.0);
        let at = self.water.ocean.sea.as_ref().map(|s| (s.sea.frame_at(t), s.swell.frame_at(t)));
        if let (Some(s), Some((sea_at, swell_at))) = (&self.water.ocean.sea, at) {
            write_rows(&self.ctx, &s.sea_tex.0, &s.sea.angle_texels(sea_at), s.sea.n * 2);
            write_rows(&self.ctx, &s.swell_tex.0, &s.swell.angle_texels(swell_at), s.swell.n * 2);
        }

        for m in self.water.ocean.meshes.values() {
            let mut verts = m.rest.clone();
            if let (Some(s), Some((sea_at, swell_at))) = (&self.water.ocean.sea, at) {
                let (sea, swell) = (&s.sea, &s.swell);
                verts.par_chunks_mut(9).for_each(|v| v[1] += sea.height_at(sea_at, v[0], v[2]) + swell.height_at(swell_at, v[0], v[2]));
            }

            if let Ok(mut map) = self.ctx.map(&m.vb, 0, D3D11_MAP_WRITE_DISCARD, 0) {
                map.write(0, &verts);
            }
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

        for e in &seas {
            let Some(m) = self.water.ocean.meshes.get(&e.id) else { continue };
            ctx.set_vertex_buffers(0, &[Some(m.vb.clone())], &[OCEAN_VERTEX], &[0]);
            ctx.set_index_buffer(Some(&m.ib), DXGI_FORMAT_R32_UINT, 0);
            ctx.draw_indexed(m.count, 0, 0);
            self.draw_stats.draw_calls += 1;
        }

        null_ps_srvs(&self.ctx, 16);
        true
    }
}
