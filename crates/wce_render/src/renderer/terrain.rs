//! The game's terrain colour pipeline: the compiled shaders of rigidterrain.hlsl, fed the tile's maps.
//!
//! 1. surface: vs30_geom_new + ps30_geom, the terrain grid displaced by the height texture: depth z / w into `zw` (+ hardware depth, stencil
//!    2); vs30_geom_new + ps_depth, the same surface's view-space depth into `viewz`;
//! 2. blend: vs30_blend + ps30_blend, one box per terrain tile; for every pixel it covers, the tile's blend / index / alpha / normal maps and the
//!    climate's diffuse / normal texture arrays make premultiplied colour, normal and light parameters, accumulated into three targets;
//! 3. resolve: vs30_terrain_to_gbuffer + ps30_terrain_to_gbuffer_ss, fullscreen: the accumulated terrain targets (+ mid-distance detail, far
//!    maps, snow) become the G-buffer channels 1 .. 4 of the terrain pixels (stencil 2 -> 1, so the lights treat them as geometry).
//!
//! TerrainBlend meshes (the ground skirts of buildings) join step 1 (rigidterrainblend vs30_main + rigidterrain ps30_geom into `zw`, stencil
//! 2; vs30_geom + ps30_geom, their view-space depth, into `viewz`), so steps 2 and 3 give them the terrain's colour where they stand.
//!
//! Objects are drawn before (pass_gbuffer); the terrain only fills the pixels it wins the depth test for. The legacy terrain patches (rigid
//! G-buffer, flat colour) remain for picking, shadows and the environment cube, and draw the ground of a scene whose terrain has no material.

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use wce_foundation::math::{Float3, Float4x4};
use wce_foundation::wce_prof;
use wce_scene::render_scene::{EntityId, LfMap, Payload, RenderScene, Terrain, TerrainImage, TerrainMaterial};

use super::*;
use crate::shader_lib::{bind_sampler, bind_srv};
use super::terrain_custom::CustomDraw;
use wce_foundation::jobs::io_pool;
use crate::texture::{Texture, load_dds_array, make_rgba_texture, make_solid_texture};

const GEOM_VS: &str = "dx11_sm_4_0_rigidterrain.hlsl_vs_vs30_geom_new";
const GEOM_PS: &str = "dx11_sm_3_0_rigidterrain.hlsl_ps_ps30_geom";
const DEPTH_PS: &str = "dx11_sm_4_0_rigidterrain.hlsl_ps_ps_depth";
const BLEND_VS: &str = "dx11_sm_4_0_rigidterrain.hlsl_vs_vs30_blend";
const BLEND_PS: &str = "dx11_sm_4_0_rigidterrain.hlsl_ps_ps30_blend";
const RESOLVE_VS: &str = "dx11_sm_3_0_rigidterrain.hlsl_vs_vs30_terrain_to_gbuffer";
const RESOLVE_PS: &str = "dx11_sm_3_0_rigidterrain.hlsl_ps_ps30_terrain_to_gbuffer_ss";
const SHADOW_NEW_VS: &str = "dx11_sm_3_0_rigidterrain.hlsl_vs_vs30_shadow_new";
const SHADOW_TERRAIN_PS: &str = "dx11_sm_3_0_rigidterrain.hlsl_ps_ps30_shadow";

/// The grid vertex (slot 0) and the instance's three world rows (slot 1), for geom_new and shadow_new.
fn geom_layout() -> [InputElement; 4] {
    [
        element(c"POSITION", 0, DXGI_FORMAT_R32G32B32_FLOAT, 0, 0, false),
        element(c"POSITION", 1, DXGI_FORMAT_R32G32B32A32_FLOAT, 1, 0, true),
        element(c"POSITION", 2, DXGI_FORMAT_R32G32B32A32_FLOAT, 1, 16, true),
        element(c"POSITION", 3, DXGI_FORMAT_R32G32B32A32_FLOAT, 1, 32, true),
    ]
}

/// Cells a side of a patch (65 x 65 vertices). Its level of detail is chosen per patch: larger patches change how a tile looks at a
/// distance (the map context's mid ring draws its tiles' coarse heights, one patch a tile).
pub const PATCH_CELLS: i32 = 64;

/// A patch at this many grid steps from the eye starts to skip vertices (see draw_terrain_patches).
const LOD_STEPS: f32 = 128.0;

/// Material updates a terrain material no terrain uses stays on the GPU for.
const MATERIAL_KEEP: u64 = 32;

/// The instance buffer's size.
pub(crate) const INSTANCE_BYTES: usize = 64 * 4096;

/// A patch of a heights buffer: its cells (`cx` x `cz` from cell (pi, pj) * PATCH_CELLS), its height range and its vertex buffer, the 65 x 65
/// grid of (x, height, z) with x, z in cells of the patch (the game's `vsIn_P0`: geom_new takes the high-frequency height from the vertex).
#[derive(Clone, Debug, Default)]
pub(crate) struct HfPatch {
    pi: i32,
    pj: i32,
    cx: i32,
    cz: i32,
    lo: f32,
    hi: f32,
    vb: Option<ID3D11Buffer>,
}

/// The patches of one heights buffer: every placement of a tile draws the same ones with its own instance rows.
#[derive(Clone, Debug, Default)]
pub(crate) struct HfGeo {
    /// The buffer it was built from (held: its address is the key) and its grid.
    heights: Arc<Vec<f32>>,
    nx: i32,
    nz: i32,
    step: f32,
    ox: f32,
    oz: f32,
    patches: Vec<HfPatch>,
}

impl HfGeo {
    fn same_grid(&self, t: &Terrain) -> bool {
        self.nx == t.nx && self.nz == t.nz && self.step == t.step && self.ox == t.origin_x && self.oz == t.origin_z && self.heights.len() == t.heights.len()
    }
}

/// A patch as an entity places it: its world box (scene units), its size and its instance rows (the 3 x 4 world matrix of the patch grid in
/// game units; the draw moves them into its LF group's frame).
#[derive(Clone, Copy, Debug, Default)]
struct PlacedPatch {
    bmin: [f32; 3],
    bmax: [f32; 3],
    cx: i32,
    cz: i32,
    patch: usize,
    row: [f32; 12],
}

/// A terrain entity's geometry as its world matrix places it (cached until the matrix, the buffer or the LF change).
#[derive(Clone, Debug, Default)]
pub(crate) struct Placed {
    key: usize,
    pub(crate) world: Float4x4,
    lf: u64,
    units: f32,
    lod: bool,
    step: f32,
    patches: Vec<PlacedPatch>,

    /// The material's blend box: its id, the world box (min x y z, max x y z, scene units) and the rotation index of the material's maps.
    pub(crate) blend_box: Option<(u64, [f32; 6], f32)>,
}

/// An LF map on the GPU: a square window of its heights around the camera (R16 unorm, rows from the highest z down, as the shader addresses
/// them: `LF_WINDOW` samples a side at most, the whole map when it is smaller) and its normals.
#[derive(Clone, Debug, Default)]
pub(crate) struct LfGpu {
    window: Option<ID3D11ShaderResourceView>,

    /// The window's first sample (column, row of the map) and its side.
    i0: i32,
    j0: i32,
    n: i32,

    /// The normals rolled for a render origin (the shader reads them at the relative position, wrapping), copied on the GPU from the
    /// unrolled ones (`normal_src`) when the origin moves.
    pub(crate) normal: Texture,
    normal_origin: Float3,
    normal_src: Option<ID3D11Texture2D>,
    normal_dst: Option<ID3D11Texture2D>,
}

/// The most samples a side of an LF window (8 MB; 64 km of the campaign map's 32-unit samples).
const LF_WINDOW: i32 = 2048;

/// An LF group's draw: its height texture's view, side, the world position of its column 0 and (row-wise) its sample past the last row, the
/// shader's uv offset and the frame shift that go with them.
struct LfDraw {
    srv: Option<ID3D11ShaderResourceView>,
    n: f32,
    width: f32,
    scale: f32,
    offset: f32,
    uv_offset: f32,
    shift: [f32; 2],
}

/// A material's blend boxes this frame (with the LF map their terrains read): the instance rows.
type MaterialBoxes = (Arc<TerrainMaterial>, Option<Arc<LfMap>>, Vec<f32>, bool);

/// One patch's vertex buffer: the 65 x 65 grid (x, height, z), x and z in cells of the patch; past the grid's last row or column the vertices
/// repeat its edge (the index buffers of a smaller patch never reach them).
fn hf_patch_vb(dev: &ID3D11Device, t: &Terrain, pi: i32, pj: i32) -> Option<ID3D11Buffer> {
    let mut v = Vec::with_capacity(((PATCH_CELLS + 1) * (PATCH_CELLS + 1) * 3) as usize);
    for z in 0..=PATCH_CELLS {
        let j = (pj * PATCH_CELLS + z).min(t.nz - 1);
        for x in 0..=PATCH_CELLS {
            let i = (pi * PATCH_CELLS + x).min(t.nx - 1);
            v.extend([x as f32, t.height_at(i, j), z as f32]);
        }
    }

    immutable_buffer(dev, &f32_bytes(&v), D3D11_BIND_VERTEX_BUFFER)
}

/// A heights buffer's patches from `g` (empty, or the same grid's from an older buffer: only the patches whose heights differ are made
/// again). The device is free-threaded: a loading thread may run it.
fn fill_hf(dev: &ID3D11Device, mut g: HfGeo, t: &Terrain) -> HfGeo {
    let (cx, cz) = (t.nx - 1, t.nz - 1);
    let (px, pz) = ((cx + PATCH_CELLS - 1) / PATCH_CELLS, (cz + PATCH_CELLS - 1) / PATCH_CELLS);
    let reuse = g.patches.len() == (px * pz) as usize;
    if !reuse {
        g.patches = (0..px * pz).map(|k| HfPatch { pi: k % px, pj: k / px, ..Default::default() }).collect();
    }

    for p in &mut g.patches {
        (p.cx, p.cz) = (PATCH_CELLS.min(cx - p.pi * PATCH_CELLS), PATCH_CELLS.min(cz - p.pj * PATCH_CELLS));
    }

    let (old_h, nx) = (g.heights.clone(), t.nx as usize);
    let patches = std::mem::take(&mut g.patches);
    g.patches = patches.into_iter().map(|mut p| {
        let (i0, i1) = ((p.pi * PATCH_CELLS) as usize, (p.pi * PATCH_CELLS + p.cx) as usize);
        let (j0, j1) = ((p.pj * PATCH_CELLS) as usize, (p.pj * PATCH_CELLS + p.cz) as usize);
        let dirty = !reuse || p.vb.is_none() || t.heights.is_empty() || old_h.len() != t.heights.len()
            || (j0..=j1).any(|j| t.heights[j * nx + i0..=j * nx + i1].iter().zip(&old_h[j * nx + i0..=j * nx + i1]).any(|(a, b)| a.to_bits() != b.to_bits()));
        if dirty {
            let (mut lo, mut hi) = (f32::MAX, f32::MIN);
            for j in j0..=j1 {
                for i in i0..=i1 {
                    let h = t.height_at(i as i32, j as i32);
                    (lo, hi) = (lo.min(h), hi.max(h));
                }
            }

            (p.lo, p.hi) = (lo, hi);
            p.vb = hf_patch_vb(dev, t, p.pi, p.pj);
        }

        p
    }).collect();

    g.heights = t.heights.clone();
    g
}

/// What a loading thread made of a material: its GPU side, the texture arrays it read (for the cache) and what went wrong.
struct MaterialBuilt {
    id: u64,
    gpu: TerrainMatGpu,
    arrays: Vec<((String, bool), Texture)>,
    errors: Vec<String>,
}

/// The GPU side of a terrain material: the tile's maps (mipmapped), the climate's texture arrays (`have` gives the ones already made), the
/// light parameter texture. The device is free-threaded: a loading thread may run it.
fn material_gpu(dev: &ID3D11Device, files: &dyn crate::files::Files, m: &TerrainMaterial, have: &dyn Fn(&(String, bool)) -> Option<Texture>) -> MaterialBuilt {
    let mut errors = vec![];
    let mut img = |im: &TerrainImage, what: &str| {
        let t = im.rgba.as_ref().map(|rgba| make_rgba_texture(dev, im.w, im.h, rgba, false)).unwrap_or_default();
        if t.srv.is_none() {
            errors.push(format!("terrain material: {what} map did not upload"));
        }

        t
    };

    // The blend and index maps change under a paint stroke: written in place, not made again.
    let patchable = match (&m.blend.rgba, &m.index.rgba) {
        (Some(b), Some(i)) => crate::texture::PatchableRgba::new(dev, m.blend.w, m.blend.h, b).zip(crate::texture::PatchableRgba::new(dev, m.index.w, m.index.h, i)),
        _ => None,
    };

    let (blend, index) = match &patchable {
        Some((b, i)) => (b.texture.clone(), i.texture.clone()),
        None => (img(&m.blend, "blend"), img(&m.index, "index")),
    };

    let patchable_normal = m.normal.rgba.as_ref().and_then(|n| crate::texture::PatchableRgba::new(dev, m.normal.w, m.normal.h, n));
    let normal = patchable_normal.as_ref().map_or_else(|| img(&m.normal, "normal"), |p| p.texture.clone());
    let mut g = TerrainMatGpu {
        normal_id: m.normal_id,
        blend_id: m.blend_id,
        patchable: patchable.map(|p| std::sync::Arc::new(std::sync::Mutex::new(p))),
        patchable_normal: patchable_normal.map(|p| std::sync::Arc::new(std::sync::Mutex::new(p))),
        blend,
        index,
        normal,

        // The tile's weight where it overlaps its neighbours (a map context's); alone, 1 over the whole rectangle.
        alpha: m.alpha.rgba.as_ref().map_or_else(|| make_solid_texture(dev, 0xFFFF_FFFF), |rgba| make_rgba_texture(dev, m.alpha.w, m.alpha.h, rgba, false)),

        // A flat normal (the far-distance normal map is switched off with g_far_tile_distance).
        lf_normal: make_solid_texture(dev, 0x7F00_7FFF),
        black: make_solid_texture(dev, 0),
        ..Default::default()
    };

    // The diffuse / normal texture arrays: slot = 4 * group + variation.
    let mut arrays = vec![];
    let mut array = |paths: &[String; 16], srgb: bool| {
        let key = (paths.join("|"), srgb);
        if let Some(t) = have(&key) {
            return t;
        }

        let mut bytes = vec![None; 16];
        for (i, path) in paths.iter().enumerate() {
            if path.is_empty() || !files.exists(path) {
                continue;
            }

            match files.read(path) {
                Ok(data) => bytes[i] = Some(data),
                Err(error) => errors.push(format!("terrain material: {path}: {error}")),
            }
        }

        let slices = bytes.iter().map(|b| b.as_deref()).collect::<Vec<_>>();
        let (t, bad) = load_dds_array(dev, &slices, srgb, !srgb);
        if bad > 0 {
            errors.push(format!("terrain material: {bad} texture(s) of the climate differ in size / format from the others and are drawn neutral"));
        }

        arrays.push((key, t.clone()));
        t
    };

    g.diffuse = array(&m.diffuse, true);
    g.normals = array(&m.normal_tex, false);
    if g.diffuse.srv.is_none() || g.normals.srv.is_none() {
        errors.push("terrain material: the texture arrays did not build".to_owned());
    }

    // s_light_params: 16 texels (smoothness, reflectivity) per slot.
    let lp: Vec<f32> = (0..16).flat_map(|i| [m.smoothness[i], m.reflectivity[i]]).collect();
    let td = D3D11_TEXTURE2D_DESC {
        Width: 16, Height: 1, MipLevels: 1, ArraySize: 1, Format: DXGI_FORMAT_R32G32_FLOAT, SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
        Usage: D3D11_USAGE_IMMUTABLE, BindFlags: D3D11_BIND_SHADER_RESOURCE as u32, CPUAccessFlags: 0, MiscFlags: 0,
    };

    let sd = [InitData::new(bytemuck::cast_slice(&lp), 16 * 8)];
    if let Ok(tx) = dev.create_texture_2d(&td, Some(&sd)) {
        g.light_params = dev.create_shader_resource_view(&tx, None).ok();
    }

    MaterialBuilt { id: m.id, gpu: g, arrays, errors }
}

/// What the loading threads finished for the terrain pass (taken in at the next frame).
#[derive(Default)]
pub(crate) struct TerrainJobs {
    materials: Vec<MaterialBuilt>,
    hf: Vec<(usize, HfGeo)>,
}

// SAFETY: D3D11 device children (buffers, textures, views) are free-threaded and reference counted atomically (see SharedDevice); the
// immediate context never goes through these.
unsafe impl Send for TerrainMatGpu {}
// SAFETY: as above.
unsafe impl Send for HfGeo {}
// SAFETY: as above.
unsafe impl Send for MaterialBuilt {}

/// The key of a heights buffer (its address: held by the geometry while it is kept, so it is not reused).
fn hf_key(t: &Terrain) -> usize {
    Arc::as_ptr(&t.heights) as usize
}


/// The rotation index of the blend shader's table (`icb`: 0 none, 1 (u, v) -> (-v, u), 2 (-u, -v), 3 (v, -u)) that takes a world point of a
/// box back to the material's own uv under a world matrix (a turn about y in steps of 90 degrees).
fn rotation_index(w: &Float4x4) -> f32 {
    let (a, c) = (w.m[0][0], w.m[2][0]);
    let l = (a * a + c * c).sqrt().max(1e-12);
    let (a, c) = (a / l, c / l);
    if a > 0.5 { 0.0 } else if a < -0.5 { 2.0 } else if c < 0.0 { 1.0 } else { 3.0 }
}

/// The GPU side of a terrain material.
#[derive(Clone, Debug, Default)]
pub(crate) struct TerrainMatGpu {
    pub(crate) blend: Texture,
    pub(crate) index: Texture,
    pub(crate) normal: Texture,
    pub(crate) alpha: Texture,
    pub(crate) lf_normal: Texture,
    black: Texture,
    pub(crate) diffuse: Texture,
    pub(crate) normals: Texture,
    normal_id: u64,
    blend_id: u64,
    light_params: Option<ID3D11ShaderResourceView>,

    /// The blend and index maps as textures written in place (`TerrainMaterial::blend_dirty`), and the normal map (`normal_dirty`).
    patchable: Option<crate::texture::PatchableMaps>,
    patchable_normal: Option<std::sync::Arc<std::sync::Mutex<crate::texture::PatchableRgba>>>,

    /// The material update it was last used in.
    used: u64,
}

/// The terrain pass's state.
#[derive(Default)]
pub(crate) struct TerrainGpuState {
    pub active: bool,

    /// The frame camera's eye, for the patches' level of detail (the shadow pass runs before the camera is set).
    pub lod_eye: Float3,
    pub failed: bool,

    /// The patches of the heights buffers by key, and each terrain entity's placement of its buffer.
    hf: HashMap<usize, HfGeo>,

    /// The loading threads' work: what they finished, the buffers and materials they are making, and when a buffer they made came (kept a
    /// while for its chunk, which goes on the scene after).
    jobs: Arc<std::sync::Mutex<TerrainJobs>>,
    hf_pending: std::collections::HashSet<usize>,
    hf_made: HashMap<usize, u64>,
    mat_pending: std::collections::HashSet<u64>,
    pub(crate) placed: BTreeMap<EntityId, Placed>,
    pub(crate) mat: BTreeMap<u64, TerrainMatGpu>,

    /// The custom meshes by id (`terrain_custom`).
    pub(crate) custom: HashMap<u64, super::terrain_custom::CustomGpu>,

    /// The map context terrains left out for want of a material or heights, said once each.
    unusable_logged: std::collections::HashSet<EntityId>,

    /// The LF maps by id, and a zero one for the terrains without (what the unused height texture reads).
    pub(crate) lf: BTreeMap<u64, LfGpu>,
    lf_zero: Option<ID3D11ShaderResourceView>,

    /// What the geometry was last pruned for: the scene (address), its structure and terrain revisions.
    prune_key: (usize, u64, u64),

    /// The climates' texture arrays by (paths, sRGB): the materials of a scene's tiles mostly use the same ones.
    arrays: BTreeMap<(String, bool), Texture>,

    /// Material updates so far (a material no terrain uses stays for MATERIAL_KEEP more: a streamed chunk that goes from mid to near and
    /// back would make its maps again).
    material_epoch: u64,

    /// What the materials and arrays were last brought up to date for: the scene (address), its structure and terrain revisions. The
    /// bookkeeping (texture paths joined per terrain) cost 3 ms a frame with a map context's 192 tiles.
    materials_key: (usize, u64, u64),
    /// Index buffers of a patch of cx x cz cells at a stride, and their index counts.
    grid_ib: BTreeMap<(i32, i32, i32), (Option<ID3D11Buffer>, u32)>,
    entities: Vec<EntityId>,
    pub(crate) viewz: Target,
    pub(crate) acc: [Target; 3],
    rt_w: u32,
    rt_h: u32,
    pub(crate) box_vb: Option<ID3D11Buffer>,
    pub(crate) box_ib: Option<ID3D11Buffer>,
    pub(crate) resolve_vb: Option<ID3D11Buffer>,
    pub(crate) inst_vb: Option<ID3D11Buffer>,
    pub(crate) ds_depth: Option<ID3D11DepthStencilState>,
    pub(crate) ds_equal: Option<ID3D11DepthStencilState>,
    pub(crate) ds_lequal: Option<ID3D11DepthStencilState>,
    pub(crate) ds_stencil: Option<ID3D11DepthStencilState>,
    pub(crate) ds_resolve: Option<ID3D11DepthStencilState>,

    /// Snow regions: a snowy tile's boxes mark its pixels 3 (2 | 1), resolved apart with the snow's intensity (3 -> 1).
    ds_stencil_snow: Option<ID3D11DepthStencilState>,
    ds_resolve_snow: Option<ID3D11DepthStencilState>,
    pub(crate) bs_add: Option<ID3D11BlendState>,

    /// Rome 2's terrain-blend meshes over the resolved ground: colour blended by the source's alpha (their vertex alpha), alpha kept.
    pub(crate) bs_r2_overlay: Option<ID3D11BlendState>,
    pub(crate) rs_front_cull: Option<ID3D11RasterizerState>,

    /// Rome 2's composite targets (albedo + alpha, normal, gloss) and their size.
    pub(crate) r2_comp: [Target; 3],
    pub(crate) r2_w: u32,
    pub(crate) r2_h: u32,

    /// Rome 2's resolve inputs the grass reads too: the mid-distance detail map (`viewz` holds the depth channel's copy), set by a frame's
    /// resolve.
    pub(crate) r2_detail: Option<ID3D11ShaderResourceView>,
    pub(crate) r2_resolved: bool,
}

/// The frustum test of an axis-aligned box against a view-projection matrix (D3D clip volume: -w <= x, y <= w, 0 <= z <= w).
pub(crate) fn box_in_frustum(vp: &Float4x4, bmin: [f32; 3], bmax: [f32; 3]) -> bool {
    (0..6).all(|plane| {
        let p: [f32; 4] = std::array::from_fn(|c| {
            let w = vp.m[3][c];
            match plane {
                0 => w + vp.m[0][c],
                1 => w - vp.m[0][c],
                2 => w + vp.m[1][c],
                3 => w - vp.m[1][c],
                4 => vp.m[2][c],
                _ => w - vp.m[2][c],
            }
        });

        let (x, y, z) = (if p[0] >= 0.0 { bmax[0] } else { bmin[0] }, if p[1] >= 0.0 { bmax[1] } else { bmin[1] }, if p[2] >= 0.0 { bmax[2] } else { bmin[2] });
        p[0] * x + p[1] * y + p[2] * z + p[3] >= 0.0
    })
}

/// The entity transforms the material terrain takes: a turn about y (the context's placements, in steps of 90 degrees for the blend boxes),
/// a translation and a scale of z (the tile tool: the height map's rows are not always as far apart as its columns); y stays up.
fn is_ground_world(w: &Float4x4) -> bool {
    let m = &w.m;
    let det = m[0][0] * m[2][2] - m[0][2] * m[2][0];
    m[0][1].abs() < 1e-5 && m[2][1].abs() < 1e-5 && m[1][0].abs() < 1e-5 && m[1][2].abs() < 1e-5 && (m[1][1] - 1.0).abs() < 1e-5 && det > 1e-8
}

fn depth_stencil(dev: &ID3D11Device, d: &D3D11_DEPTH_STENCIL_DESC) -> Option<ID3D11DepthStencilState> {
    dev.create_depth_stencil_state(d).ok()
}

impl Renderer {

    fn make_terrain_states(&mut self) {
        if self.terrain.ds_depth.is_some() {
            return;
        }

        let dev = self.dev.clone();
        let t = &mut self.terrain;
        let replace = D3D11_DEPTH_STENCILOP_DESC { StencilFailOp: D3D11_STENCIL_OP_KEEP, StencilDepthFailOp: D3D11_STENCIL_OP_KEEP, StencilPassOp: D3D11_STENCIL_OP_REPLACE, StencilFunc: D3D11_COMPARISON_ALWAYS };

        // The surface wins where it is nearest: stencil 2 (ref).
        t.ds_depth = depth_stencil(&dev, &D3D11_DEPTH_STENCIL_DESC {
            DepthEnable: true.into(), DepthWriteMask: D3D11_DEPTH_WRITE_MASK_ALL, DepthFunc: D3D11_COMPARISON_LESS, StencilEnable: true.into(),
            StencilReadMask: 0xFF, StencilWriteMask: 0xFF, FrontFace: replace, BackFace: replace,
        });

        // The same surface again (view-space depth): exactly the pixels it won.
        t.ds_equal = depth_stencil(&dev, &D3D11_DEPTH_STENCIL_DESC { DepthEnable: true.into(), DepthWriteMask: D3D11_DEPTH_WRITE_MASK_ZERO, DepthFunc: D3D11_COMPARISON_EQUAL, ..Default::default() });

        // The terrain-blend meshes' view-space depth comes from another vertex shader than their G-buffer pass: not bit for bit the same depth.
        t.ds_lequal = depth_stencil(&dev, &D3D11_DEPTH_STENCIL_DESC { DepthEnable: true.into(), DepthWriteMask: D3D11_DEPTH_WRITE_MASK_ZERO, DepthFunc: D3D11_COMPARISON_LESS_EQUAL, ..Default::default() });

        // Only the terrain pixels (stencil 2, or 3 once a snowy tile marked them: bit 1 is what is tested) ...
        let keep = D3D11_DEPTH_STENCILOP_DESC { StencilPassOp: D3D11_STENCIL_OP_KEEP, StencilFunc: D3D11_COMPARISON_EQUAL, ..replace };
        let mut s = D3D11_DEPTH_STENCIL_DESC { DepthEnable: false.into(), StencilEnable: true.into(), StencilReadMask: 0x02, StencilWriteMask: 0xFF, FrontFace: keep, BackFace: keep, ..Default::default() };
        t.ds_stencil = depth_stencil(&dev, &s);

        // A snowy tile's boxes (ref 3) also set bit 0 of the pixels they draw.
        let mark = D3D11_DEPTH_STENCILOP_DESC { StencilPassOp: D3D11_STENCIL_OP_REPLACE, ..keep };
        t.ds_stencil_snow = depth_stencil(&dev, &D3D11_DEPTH_STENCIL_DESC { StencilWriteMask: 0x01, FrontFace: mark, BackFace: mark, ..s });

        // ... and they become geometry (stencil 1) for the lights: 2 -> 1 here, 3 -> 1 (bit 1 cleared) in the snowy resolve; the decal bit
        // (`DECAL_RECEIVER`, set with the 2) is not compared and stays.
        s.StencilReadMask = STENCIL_LOW;
        s.FrontFace.StencilPassOp = D3D11_STENCIL_OP_DECR;
        s.BackFace = s.FrontFace;
        t.ds_resolve = depth_stencil(&dev, &s);
        let clear = D3D11_DEPTH_STENCILOP_DESC { StencilPassOp: D3D11_STENCIL_OP_ZERO, ..keep };
        t.ds_resolve_snow = depth_stencil(&dev, &D3D11_DEPTH_STENCIL_DESC { StencilWriteMask: 0x02, FrontFace: clear, BackFace: clear, ..s });

        // The tiles accumulate (premultiplied by their alpha).
        t.bs_add = crate::particles::blend_state(&dev, D3D11_RENDER_TARGET_BLEND_DESC {
            BlendEnable: true.into(), SrcBlend: D3D11_BLEND_ONE, DestBlend: D3D11_BLEND_ONE, BlendOp: D3D11_BLEND_OP_ADD, SrcBlendAlpha: D3D11_BLEND_ONE,
            DestBlendAlpha: D3D11_BLEND_ONE, BlendOpAlpha: D3D11_BLEND_OP_ADD, RenderTargetWriteMask: D3D11_COLOR_WRITE_ENABLE_ALL as u8,
        });

        t.bs_r2_overlay = crate::particles::blend_state(&dev, D3D11_RENDER_TARGET_BLEND_DESC {
            BlendEnable: true.into(), SrcBlend: D3D11_BLEND_SRC_ALPHA, DestBlend: D3D11_BLEND_INV_SRC_ALPHA, BlendOp: D3D11_BLEND_OP_ADD, SrcBlendAlpha: D3D11_BLEND_ZERO,
            DestBlendAlpha: D3D11_BLEND_ONE, BlendOpAlpha: D3D11_BLEND_OP_ADD,
            RenderTargetWriteMask: (D3D11_COLOR_WRITE_ENABLE_RED | D3D11_COLOR_WRITE_ENABLE_GREEN | D3D11_COLOR_WRITE_ENABLE_BLUE) as u8,
        });

        // The blend boxes: their back faces cover the pixels once, also from inside the box. Not clipped by depth (clamped instead): the
        // back face of a box at the horizon lies past the far plane, and clipped, the ground there got no material (black: the light
        // pass's 0, which Rome 2's log-luminance tone mapping spread into dark blocks).
        let rd = D3D11_RASTERIZER_DESC { FillMode: D3D11_FILL_SOLID, CullMode: D3D11_CULL_FRONT, DepthClipEnable: false.into(), ..Default::default() };
        t.rs_front_cull = dev.create_rasterizer_state(&rd).ok();

        // The unit box: corner i = (i & 1, i >> 1 & 1, i >> 2 & 1); triangles wound so that (b - a) x (c - a) points outwards, which is clockwise
        // (front) seen from outside.
        let corners: [[f32; 3]; 8] = std::array::from_fn(|i| [(i & 1) as f32, ((i >> 1) & 1) as f32, ((i >> 2) & 1) as f32]);

        // -z, +z, -y, +y, -x, +x.
        let quads: [[u16; 4]; 6] = [[0, 2, 3, 1], [4, 5, 7, 6], [0, 1, 5, 4], [2, 6, 7, 3], [0, 4, 6, 2], [1, 3, 7, 5]];
        let mut bi = vec![];
        for q in &quads {
            for (a, b, c) in [(0, 1, 2), (0, 2, 3)] {
                // Oriented by the outward direction of the quad's own centre.
                let (pa, pb, pc) = (corners[q[a] as usize], corners[q[b] as usize], corners[q[c] as usize]);
                let u = [pb[0] - pa[0], pb[1] - pa[1], pb[2] - pa[2]];
                let v = [pc[0] - pa[0], pc[1] - pa[1], pc[2] - pa[2]];
                let n = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]];
                let mid: [f32; 3] = std::array::from_fn(|k| (pa[k] + pb[k] + pc[k]) / 3.0 - 0.5);
                let dot = n[0] * mid[0] + n[1] * mid[1] + n[2] * mid[2];
                bi.push(q[a]);
                if dot > 0.0 {
                    bi.extend([q[b], q[c]]);
                } else {
                    bi.extend([q[c], q[b]]);
                }
            }
        }

        t.box_vb = immutable_buffer(&dev, &f32_bytes(corners.as_flattened()), D3D11_BIND_VERTEX_BUFFER);
        t.box_ib = immutable_buffer(&dev, &bi.iter().flat_map(|i| i.to_le_bytes()).collect::<Vec<_>>(), D3D11_BIND_INDEX_BUFFER);

        // x y z w u v.
        let tri: [[f32; 6]; 6] = [[-1.0, -1.0, 0.0, 1.0, 0.0, 1.0], [-1.0, 1.0, 0.0, 1.0, 0.0, 0.0], [1.0, 1.0, 0.0, 1.0, 1.0, 0.0], [-1.0, -1.0, 0.0, 1.0, 0.0, 1.0], [1.0, 1.0, 0.0, 1.0, 1.0, 0.0], [1.0, -1.0, 0.0, 1.0, 1.0, 1.0]];
        t.resolve_vb = immutable_buffer(&dev, &f32_bytes(tri.as_flattened()), D3D11_BIND_VERTEX_BUFFER);
        t.inst_vb = dynamic_buffer(&dev, INSTANCE_BYTES, D3D11_BIND_VERTEX_BUFFER);
    }

    /// The patches of a terrain's heights buffer, shared by the entities that hold the same buffer. A new buffer for an entity whose old one
    /// nobody else draws (a brush stroke on the open tile) rebuilds only the patches whose heights changed. One a loading thread is making
    /// (`terrain_ready`) is left to it.
    fn build_hf(&mut self, id: EntityId, t: &Terrain) {
        let key = hf_key(t);
        if self.terrain.hf.contains_key(&key) || self.terrain.hf_pending.contains(&key) {
            return;
        }

        wce_prof!("terrain.build_hf");
        let old = self.terrain.placed.get(&id).map(|p| p.key).filter(|&k| k != key);
        let exclusive = old.is_some_and(|k| !self.terrain.placed.iter().any(|(&other, p)| other != id && p.key == k));
        let g = match old.filter(|_| exclusive).and_then(|k| self.terrain.hf.remove(&k)) {
            Some(g) if g.same_grid(t) => g,
            _ => HfGeo { nx: t.nx, nz: t.nz, step: t.step, ox: t.origin_x, oz: t.origin_z, ..Default::default() },
        };

        self.terrain.hf.insert(key, fill_hf(&self.dev, g, t));

        // Whoever placed the old buffer places the new one again.
        if let Some(p) = self.terrain.placed.get_mut(&id) {
            p.key = usize::MAX;
        }
    }

    /// An entity's placement of its patches: world boxes for culling, instance rows, its blend box (cached until something it depends on
    /// changes).
    fn place_terrain(&mut self, id: EntityId, world: &Float4x4, t: &Terrain) {
        let (key, lf, units) = (hf_key(t), t.lf.as_ref().map_or(0, |lf| lf.id), self.camera.units);
        let material = t.material.as_ref().map(|m| m.id);
        if self.terrain.placed.get(&id).is_some_and(|p| p.key == key && p.world == *world && p.lf == lf && p.units == units && p.lod == t.lod
            && p.blend_box.map(|b| b.0) == material)
        {
            return;
        }

        let Some(g) = self.terrain.hf.get(&key) else { return };
        wce_prof!("terrain.place");
        let w = &world.m;
        let mut placed = Placed { key, world: *world, lf, units, lod: t.lod, step: g.step, patches: Vec::with_capacity(g.patches.len()), blend_box: None };
        for (k, p) in g.patches.iter().enumerate() {
            if p.vb.is_none() {
                continue;
            }

            // The patch grid's point (x, h, z) in cells is the entity's (ox + x step, h, oz + z step).
            let (ox, oz) = (g.ox + (p.pi * PATCH_CELLS) as f32 * g.step, g.oz + (p.pj * PATCH_CELLS) as f32 * g.step);
            let mut row = [0.0f32; 12];
            for r in 0..3 {
                row[r * 4..r * 4 + 4].copy_from_slice(&[w[r][0] * g.step, w[r][1], w[r][2] * g.step, w[r][0] * ox + w[r][2] * oz + w[r][3]].map(|v| v * units));
            }

            // The world box of the patch's corners; the shader moves the surface by up to 9 cm.
            let (x1, z1) = (ox + p.cx as f32 * g.step, oz + p.cz as f32 * g.step);
            let (mut bmin, mut bmax) = ([f32::MAX; 3], [f32::MIN; 3]);
            for (x, y, z) in [(ox, p.lo, oz), (x1, p.lo, oz), (ox, p.lo, z1), (x1, p.lo, z1), (ox, p.hi, oz), (x1, p.hi, oz), (ox, p.hi, z1), (x1, p.hi, z1)] {
                for r in 0..3 {
                    let v = w[r][0] * x + w[r][1] * y + w[r][2] * z + w[r][3];
                    (bmin[r], bmax[r]) = (bmin[r].min(v), bmax[r].max(v));
                }
            }

            if let Some(lf) = &t.lf {
                let (lo, hi) = lf.range(bmin[0], bmin[2], bmax[0], bmax[2]);
                (bmin[1], bmax[1]) = (bmin[1] + lo, bmax[1] + hi);
            }

            (bmin[1], bmax[1]) = (bmin[1] - 0.2, bmax[1] + 0.2);
            placed.patches.push(PlacedPatch { bmin, bmax, cx: p.cx, cz: p.cz, patch: k, row });
        }

        // The blend box: the material's rectangle turned and moved with the entity (not scaled: the tile tool's z scale is in the rectangle),
        // its height range around the terrain's lifted by the LF under it.
        if let Some(m) = &t.material {
            let (a, c) = (w[0][0], w[2][0]);
            let l = (a * a + c * c).sqrt().max(1e-12);
            let (cos, sin) = (a / l, c / l);
            let (mut lo, mut hi) = ([f32::MAX; 2], [f32::MIN; 2]);
            for (x, z) in [(m.x0, m.z0), (m.x1, m.z0), (m.x0, m.z1), (m.x1, m.z1)] {
                let (x, z) = (x as f32, z as f32);
                let (wx, wz) = (cos * x - sin * z + w[0][3], sin * x + cos * z + w[2][3]);
                (lo[0], lo[1], hi[0], hi[1]) = (lo[0].min(wx), lo[1].min(wz), hi[0].max(wx), hi[1].max(wz));
            }

            let (lf_lo, lf_hi) = t.lf.as_ref().map_or((0.0, 0.0), |lf| lf.range(lo[0], lo[1], hi[0], hi[1]));
            let (y0, y1) = (m.y0 as f32 + w[1][3] + lf_lo, m.y1 as f32 + w[1][3] + lf_hi);
            placed.blend_box = Some((m.id, [lo[0], y0, lo[1], hi[0], y1, hi[1]], rotation_index(world)));
        }

        self.terrain.placed.insert(id, placed);
    }

    /// The GPU side of an LF map: its normal map for the render origin (the heights' window is made when drawn, around the camera).
    fn build_lf(&mut self, lf: &LfMap) {
        let o = self.camera.origin;
        if self.terrain.lf.get(&lf.id).is_some_and(|g| g.normal_origin == o) {
            return;
        }

        let mut g = self.terrain.lf.remove(&lf.id).unwrap_or_default();
        let (w, h) = (lf.normal.w, lf.normal.h);
        if g.normal_src.is_none()
            && let Some(rgba) = lf.normal.rgba.as_ref().filter(|p| w > 0 && h > 0 && p.len() >= (w * h * 4) as usize)
        {
            // One level: its texel is 64 units or so, more than a pixel covers within the far plane (the smaller levels are never read),
            // and a level can be rolled exactly by any whole number of texels.
            let mut td = D3D11_TEXTURE2D_DESC {
                Width: w, Height: h, MipLevels: 1, ArraySize: 1, Format: DXGI_FORMAT_R8G8B8A8_UNORM, SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
                Usage: D3D11_USAGE_IMMUTABLE, BindFlags: D3D11_BIND_SHADER_RESOURCE as u32, CPUAccessFlags: 0, MiscFlags: 0,
            };

            g.normal_src = self.dev.create_texture_2d(&td, Some(&[InitData::new(&rgba[..(w * h * 4) as usize], w * 4)])).ok();
            td.Usage = D3D11_USAGE_DEFAULT;
            g.normal_dst = self.dev.create_texture_2d(&td, None).ok();
            let srv = g.normal_dst.as_ref().and_then(|t| self.dev.create_shader_resource_view(t, None).ok());
            g.normal = Texture { srv, width: w, height: h, mips: 1, depth: 1, format: DXGI_FORMAT_R8G8B8A8_UNORM, ..Default::default() };
        }

        // The shader reads it at uv = (x, -z) / normal_world of the position relative to the origin: rolled by the origin's texels (it is
        // on their grid), column c holds the source's c + ox / texel, row r its r - oz / texel. Four copies at most (the wrap splits both
        // axes).
        if let (Some(src), Some(dst)) = (&g.normal_src, &g.normal_dst) {
            let (tx, tz) = (f64::from(lf.normal_world[0]) / f64::from(w), f64::from(lf.normal_world[1]) / f64::from(h));
            let sx = ((f64::from(o.x) / tx).round() as i64).rem_euclid(i64::from(w)) as u32;
            let sy = (-(f64::from(o.z) / tz).round() as i64).rem_euclid(i64::from(h)) as u32;
            for (dx0, cx0, cx1) in [(0, sx, w), (w - sx, 0, sx)] {
                for (dy0, cy0, cy1) in [(0, sy, h), (h - sy, 0, sy)] {
                    if cx1 > cx0 && cy1 > cy0 {
                        let b = D3D11_BOX { left: cx0, top: cy0, front: 0, right: cx1, bottom: cy1, back: 1 };
                        self.ctx.copy_subresource_region(dst, 0, [dx0, dy0, 0], src, 0, Some(&b));
                    }
                }
            }
        }

        g.normal_origin = o;
        self.terrain.lf.insert(lf.id, g);
    }

    /// An LF group's height texture and addressing for the camera at `eye`. The game's vertex shader reads it at `world xz /
    /// g_world_dimensions + g_lf_uv_offset`: ONE offset for both axes, so a texture's column 0 and its rows must start at the same x and z.
    /// The terrain is drawn in a frame moved by `shift` (the camera constants moved with it) where they do; the offset slides that frame's
    /// origin along the texture's diagonal, to the camera: the terrain's coordinates stay small where it is seen (moved to the map's corner,
    /// 80 km away on the campaign map, they lost the depth's precision: the sea showed through the ground). The window is a square of the map
    /// around the camera, made again when the camera leaves its middle.
    fn lf_draw(&mut self, lf: &LfMap, eye: Float3) -> LfDraw {
        let s = lf.step;
        let n = LF_WINDOW.min((lf.nx.max(lf.nz) as u32).next_power_of_two() as i32);
        let Some(g) = self.terrain.lf.get(&lf.id).cloned() else {
            return LfDraw { srv: None, n: 2.0, width: 1.0e6, scale: 0.0, offset: 0.0, uv_offset: 0.0, shift: [0.0, 0.0] };
        };

        // The camera's sample; the window whole when the map fits, else centred on it once it is a quarter of the window off its middle.
        let (ci, cj) = (((eye.x - lf.x0) / s).round() as i32, ((eye.z - lf.z0) / s).round() as i32);
        let whole = n >= lf.nx && n >= lf.nz;
        let stale = g.window.is_none() || g.n != n || (!whole && ((ci - (g.i0 + n / 2)).abs() > n / 4 || (cj - (g.j0 + n / 2)).abs() > n / 4));
        let g = if stale {
            let (i0, j0) = if whole { (0, 0) } else { (ci - n / 2, cj - n / 2) };
            let mut tex = vec![0u16; (n * n) as usize];
            if lf.values.len() == (lf.nx * lf.nz) as usize {
                for (r, row) in tex.chunks_mut(n as usize).enumerate() {
                    let j = (j0 + n - 1 - r as i32).clamp(0, lf.nz - 1);
                    for (c, v) in row.iter_mut().enumerate() {
                        *v = lf.values[(j * lf.nx + (i0 + c as i32).clamp(0, lf.nx - 1)) as usize];
                    }
                }
            }

            let td = D3D11_TEXTURE2D_DESC {
                Width: n as u32, Height: n as u32, MipLevels: 1, ArraySize: 1, Format: DXGI_FORMAT_R16_UNORM, SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
                Usage: D3D11_USAGE_IMMUTABLE, BindFlags: D3D11_BIND_SHADER_RESOURCE as u32, CPUAccessFlags: 0, MiscFlags: 0,
            };

            let sd = [InitData::new(bytemuck::cast_slice(&tex), (n * 2) as u32)];
            let window = self.dev.create_texture_2d(&td, Some(&sd)).and_then(|tx| self.dev.create_shader_resource_view(&tx, None)).ok();
            if window.is_none() {
                self.error("terrain: the LF height window did not upload".to_owned());
            }

            let g = LfGpu { window, i0, j0, n, ..g };
            self.terrain.lf.insert(lf.id, g.clone());
            g
        } else {
            g
        };

        // Column 0 at x_t, the row past the last at z_t; the shift is the point of the diagonal through them nearest the camera.
        let (x_t, z_t, w) = (lf.x0 + g.i0 as f32 * s, lf.z0 + (g.j0 - 1) as f32 * s, n as f32 * s);
        let uv_offset = ((eye.x - x_t) + (eye.z - z_t)) / (2.0 * w);
        LfDraw { srv: g.window, n: n as f32, width: w, scale: lf.scale, offset: lf.offset, uv_offset, shift: [x_t + uv_offset * w, z_t + uv_offset * w] }
    }

    /// The height texture of the terrains without an LF map: zero everywhere.
    fn lf_zero(&mut self) -> Option<ID3D11ShaderResourceView> {
        if self.terrain.lf_zero.is_none() {
            let td = D3D11_TEXTURE2D_DESC {
                Width: 2, Height: 2, MipLevels: 1, ArraySize: 1, Format: DXGI_FORMAT_R16_UNORM, SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
                Usage: D3D11_USAGE_IMMUTABLE, BindFlags: D3D11_BIND_SHADER_RESOURCE as u32, CPUAccessFlags: 0, MiscFlags: 0,
            };

            let sd = [InitData::new(bytemuck::cast_slice(&[0u16; 4]), 4)];
            self.terrain.lf_zero = self.dev.create_texture_2d(&td, Some(&sd)).and_then(|tx| self.dev.create_shader_resource_view(&tx, None)).ok();
        }

        self.terrain.lf_zero.clone()
    }

    /// The index buffer of a patch of `cx` x `cz` cells joining every `k`-th vertex (`k` divides both): blocks of k x k cells, two triangles
    /// each, but the blocks along the patch's edges keep every edge vertex (a fan from the block's centre), so patches at any two strides
    /// meet without cracks.
    fn terrain_grid_ib(&mut self, cx: i32, cz: i32, k: i32) -> (Option<ID3D11Buffer>, u32) {
        if let Some(ib) = self.terrain.grid_ib.get(&(cx, cz, k)) {
            return ib.clone();
        }

        let row = PATCH_CELLS + 1;
        let v = |x: i32, z: i32| (z * row + x) as u16;
        let mut idx = Vec::<u16>::with_capacity((cx * cz * 6) as usize);
        let mut z0 = 0;
        while z0 < cz {
            let mut x0 = 0;
            while x0 < cx {
                let (x1, z1) = (x0 + k, z0 + k);
                let (a, b, c, d) = (v(x0, z0), v(x1, z0), v(x0, z1), v(x1, z1));
                let (left, right, bottom, top) = (x0 == 0, x1 == cx, z0 == 0, z1 == cz);
                if k == 1 || !(left || right || bottom || top) {
                    // x right, z up (looking down +y): clockwise = front (the surface faces +y).
                    idx.extend([a, c, b, b, c, d]);
                } else {
                    // The perimeter clockwise (up the left side, along the top, down the right, back along the bottom), every vertex on a patch
                    // edge, the corners only elsewhere; a fan from the centre (clockwise too).
                    let mut ring = vec![];
                    let side = |ring: &mut Vec<u16>, fine: bool, from: (i32, i32), dir: (i32, i32)| {
                        let n = if fine { k } else { 1 };
                        let st = k / n;
                        for i in 0..n {
                            ring.push(v(from.0 + dir.0 * i * st, from.1 + dir.1 * i * st));
                        }
                    };

                    side(&mut ring, left, (x0, z0), (0, 1));
                    side(&mut ring, top, (x0, z1), (1, 0));
                    side(&mut ring, right, (x1, z1), (0, -1));
                    side(&mut ring, bottom, (x1, z0), (-1, 0));
                    let centre = v(x0 + k / 2, z0 + k / 2);
                    for i in 0..ring.len() {
                        idx.extend([centre, ring[i], ring[(i + 1) % ring.len()]]);
                    }
                }

                x0 = x1;
            }

            z0 += k;
        }

        let ib = immutable_buffer(&self.dev, &idx.iter().flat_map(|i| i.to_le_bytes()).collect::<Vec<_>>(), D3D11_BIND_INDEX_BUFFER);
        let out = (ib, idx.len() as u32);
        self.terrain.grid_ib.insert((cx, cz, k), out.clone());
        out
    }

    /// The GPU side of a terrain material, made now (the open tile's; a map context's are made on the loading threads, see `terrain_ready`).
    fn build_terrain_material(&mut self, m: &TerrainMaterial) {
        if let Some(have) = self.terrain.mat.get_mut(&m.id) {
            // A paint stroke only changes the blend and index maps: the rectangle it changed when the maps here are the ones it changed
            // (written in place), else the maps made again.
            if have.blend_id != m.blend_id
                && let (Some(b), Some(i)) = (&m.blend.rgba, &m.index.rgba)
            {
                let patched = m.blend_dirty.filter(|&(from, _)| from == have.blend_id).zip(have.patchable.as_ref()).is_some_and(|((_, rect), p)| {
                    let mut p = p.lock().unwrap_or_else(|e| e.into_inner());
                    if !p.0.fits(m.blend.w, m.blend.h) || !p.1.fits(m.index.w, m.index.h) {
                        return false;
                    }

                    p.0.patch(&self.ctx, b, rect);
                    p.1.patch(&self.ctx, i, rect);
                    true
                });

                if patched {
                    have.blend_id = m.blend_id;
                } else if let Some((pb, pi)) = crate::texture::PatchableRgba::new(&self.dev, m.blend.w, m.blend.h, b).zip(crate::texture::PatchableRgba::new(&self.dev, m.index.w, m.index.h, i)) {
                    (have.blend, have.index) = (pb.texture.clone(), pi.texture.clone());
                    have.patchable = Some(std::sync::Arc::new(std::sync::Mutex::new((pb, pi))));
                    have.blend_id = m.blend_id;
                }
            }

            // A height stroke only changes the normal map: the rectangle a dab changed when the map here is the one it changed, else the
            // map made again.
            if have.normal_id != m.normal_id
                && let Some(rgba) = &m.normal.rgba
            {
                let patched = m.normal_dirty.filter(|&(from, _)| from == have.normal_id).zip(have.patchable_normal.as_ref()).is_some_and(|((_, rect), p)| {
                    let mut p = p.lock().unwrap_or_else(|e| e.into_inner());
                    p.fits(m.normal.w, m.normal.h) && {
                        p.patch(&self.ctx, rgba, rect);
                        true
                    }
                });

                if patched {
                    have.normal_id = m.normal_id;
                } else if let Some(p) = crate::texture::PatchableRgba::new(&self.dev, m.normal.w, m.normal.h, rgba) {
                    have.normal = p.texture.clone();
                    have.patchable_normal = Some(std::sync::Arc::new(std::sync::Mutex::new(p)));
                    have.normal_id = m.normal_id;
                }
            }

            return;
        }

        if self.terrain.mat_pending.contains(&m.id) {
            return;
        }

        wce_prof!("terrain.build_material");
        let arrays = &self.terrain.arrays;
        let built = material_gpu(&self.dev, self.files.as_ref(), m, &|k| arrays.get(k).cloned());
        self.take_material(built);
    }

    /// A material made (here or on a loading thread) into the pass's maps.
    fn take_material(&mut self, built: MaterialBuilt) {
        for e in built.errors {
            self.error(e);
        }

        for (k, t) in built.arrays {
            self.terrain.arrays.entry(k).or_insert(t);
        }

        let mut gpu = built.gpu;
        gpu.used = self.terrain.material_epoch;
        self.terrain.mat_pending.remove(&built.id);
        self.terrain.mat.insert(built.id, gpu);
    }

    /// Takes in what the loading threads finished.
    fn poll_terrain_jobs(&mut self) {
        let done = std::mem::take(&mut *self.terrain.jobs.lock().unwrap_or_else(|p| p.into_inner()));
        for m in done.materials {
            self.take_material(m);
        }

        let frame = self.clock.frame;
        for (key, g) in done.hf {
            self.terrain.hf_pending.remove(&key);
            self.terrain.hf_made.insert(key, frame);
            self.terrain.hf.insert(key, g);
        }
    }

    /// Whether a terrain's GPU side (its patches, its material's maps) is ready; what is not is made on the loading threads, and the
    /// renderer takes it in at a later frame. A map context puts a streamed chunk on the scene once its terrains are, so making them never
    /// stalls a frame and the chunk it replaces stays drawn meanwhile.
    pub fn terrain_ready(&mut self, t: &Terrain) -> bool {
        self.poll_terrain_jobs();
        let key = hf_key(t);
        let hf = self.terrain.hf.contains_key(&key);
        if !hf && t.nx >= 2 && t.nz >= 2 && self.terrain.hf_pending.insert(key) {
            let (dev, t, jobs) = (crate::model_cache::SharedDevice(self.dev.clone()), t.clone(), self.terrain.jobs.clone());
            io_pool().post(move || {
                let g = fill_hf(dev.device(), HfGeo { nx: t.nx, nz: t.nz, step: t.step, ox: t.origin_x, oz: t.origin_z, ..Default::default() }, &t);
                jobs.lock().unwrap_or_else(|p| p.into_inner()).hf.push((key, g));
            }, 0.0);
        }

        let Some(m) = &t.material else { return hf };
        let ready = self.terrain.mat.contains_key(&m.id);
        if !ready && self.terrain.mat_pending.insert(m.id) {
            let have: Vec<((String, bool), Texture)> = [(m.diffuse.join("|"), true), (m.normal_tex.join("|"), false)].into_iter()
                .filter_map(|k| self.terrain.arrays.get(&k).cloned().map(|t| (k, t))).collect();
            let (dev, files, m, jobs) = (crate::model_cache::SharedDevice(self.dev.clone()), self.files.clone(), m.clone(), self.terrain.jobs.clone());
            io_pool().post(move || {
                let built = material_gpu(dev.device(), files.as_ref(), &m, &|k| have.iter().find(|(h, _)| h == k).map(|(_, t)| t.clone()));
                jobs.lock().unwrap_or_else(|p| p.into_inner()).materials.push(built);
            }, 0.0);
        }

        hf && ready
    }

    /// Which terrains the pass takes over this frame: every visible terrain has a material (a mixed scene keeps the legacy ground), not the
    /// environment cube.
    pub(crate) fn prepare_terrain_materials(&mut self, scene: &RenderScene) -> bool {
        self.poll_terrain_jobs();
        self.terrain.entities.clear();

        // A shader that didn't load: the legacy ground stays.
        if self.env.active || self.terrain.failed {
            return false;
        }

        let mut total = 0;
        for e in KindIndex::entities(&self.kinds.terrain, scene) {
            let Payload::Terrain(t) = &e.payload else { continue };
            if !scene.drawable(e) {
                continue;
            }

            total += 1;
            if t.material.is_none() || t.nx < 2 || t.nz < 2 || t.heights.is_empty() || !is_ground_world(&e.world) {
                // Rome 2 has no legacy ground to fall back on, nor has a map context's terrain (not `legacy`): a terrain without a material
                // (still loading, or none made) is left out. Falling back for it turned the whole ground off: the context's has no
                // legacy patches.
                if self.game == super::game::RenderGame::Rome2 || !t.legacy {
                    if !t.legacy && self.terrain.unusable_logged.insert(e.id) {
                        let (m, ok) = (&e.world.m, t.material.is_some());
                        wce_foundation::log::warn(format!(
                            "terrain: a map context terrain at ({:.0}, {:.0}) is left out: {}", m[0][3], m[2][3],
                            if !ok { "no material" } else if t.heights.is_empty() || t.nx < 2 || t.nz < 2 { "no heights" } else { "not a ground transform" }
                        ));
                    }

                    total -= 1;
                    continue;
                }

                return false;
            }

            self.terrain.entities.push(e.id);
        }

        // The geometry, placements and LF maps of terrains that are gone go (only when the scene's terrains changed).
        let key = (std::ptr::from_ref(scene) as usize, scene.structure_revision(), scene.terrain_revision());
        if key != self.terrain.prune_key {
            self.terrain.prune_key = key;
            let mut lfs = std::collections::BTreeSet::new();
            let alive: std::collections::BTreeSet<EntityId> = KindIndex::entities(&self.kinds.terrain, scene).into_iter().filter_map(|e| match &e.payload {
                Payload::Terrain(t) => {
                    lfs.extend(t.lf.as_ref().map(|lf| lf.id));
                    Some(e.id)
                }

                _ => None,
            }).collect();

            self.terrain.placed.retain(|id, _| alive.contains(id));
            // The buffers the entities hold, and the ones they held (a new buffer is built from its entity's old one).
            let mut keys: std::collections::HashSet<usize> = KindIndex::entities(&self.kinds.terrain, scene).into_iter().filter_map(|e| match &e.payload {
                Payload::Terrain(t) => Some(hf_key(t)),
                _ => None,
            }).collect();

            keys.extend(self.terrain.placed.values().map(|p| p.key));

            // And the ones the loading threads made lately (their chunks wait for the rest of their terrains).
            let frame = self.clock.frame;
            self.terrain.hf_made.retain(|_, at| *at + 600 > frame);
            keys.extend(self.terrain.hf_made.keys());

            self.terrain.hf.retain(|k, _| keys.contains(k));
            self.terrain.lf.retain(|id, _| lfs.contains(id));
        }

        total > 0 && self.terrain.entities.len() == total
    }

    pub(crate) fn pass_terrain_material(&mut self, scene: &RenderScene) {
        self.terrain.r2_resolved = false;
        if self.terrain.entities.is_empty() {
            return;
        }

        wce_prof!("render.terrain");
        self.make_terrain_states();
        // The targets.
        if self.terrain.viewz.tex.is_none() || self.terrain.rt_w != self.gbuffer.w || self.terrain.rt_h != self.gbuffer.h {
            self.terrain.viewz = make_target(&self.dev, self.gbuffer.w, self.gbuffer.h, DXGI_FORMAT_R32_FLOAT);
            for i in 0..3 {
                self.terrain.acc[i] = make_target(&self.dev, self.gbuffer.w, self.gbuffer.h, DXGI_FORMAT_R16G16B16A16_FLOAT);
            }

            self.terrain.rt_w = self.gbuffer.w;
            self.terrain.rt_h = self.gbuffer.h;
        }

        if self.game == super::game::RenderGame::Rome2 {
            let ents = self.terrain_surface_entities(scene);
            let key = (std::ptr::from_ref(scene) as usize, scene.structure_revision(), scene.terrain_revision());
            if key != self.terrain.materials_key {
                self.terrain.materials_key = key;
                self.update_terrain_materials(&ents);
            }

            if !ents.is_empty() {
                null_ps_srvs(&self.ctx, 16);
                self.draw_terrain_rome2(&ents);
            }

            return;
        }

        let shaders = [GEOM_VS, GEOM_PS, DEPTH_PS, BLEND_VS, BLEND_PS, RESOLVE_VS, RESOLVE_PS].map(|name| self.shader(name));
        let [Some(geom_vs), Some(geom_ps), Some(depth_ps), Some(blend_vs), Some(blend_ps), Some(res_vs), Some(res_ps)] = shaders else {
            self.terrain.failed = true;
            return;
        };

        null_ps_srvs(&self.ctx, 16);
        // The clip z / w of the blend boxes' depth map is the G-buffer's own depth channel (the terrain-blend meshes wrote theirs in the geometry pass; the
        // surface adds its own below), not cleared here.
        if let Some(rtv) = &self.terrain.viewz.rtv {
            self.ctx.clear_render_target_view(rtv, [1.0, 0.0, 0.0, 1.0]);
        }

        for a in self.terrain.acc.iter().filter_map(|a| a.rtv.as_ref()) {
            self.ctx.clear_render_target_view(a, [0.0; 4]);
        }

        // ---- 1. The surface (per terrain entity: geometry, then the pixel shader's two outputs).
        let ents = self.terrain_surface_entities(scene);

        // The materials (and texture arrays) the scene's terrains use stay; the others go. Only when the terrains changed.
        let key = (std::ptr::from_ref(scene) as usize, scene.structure_revision(), scene.terrain_revision());
        if key != self.terrain.materials_key {
            self.terrain.materials_key = key;
            self.update_terrain_materials(&ents);
        }

        if ents.is_empty() {
            return;
        }

        self.draw_terrain_material(&ents, scene.snow(), [geom_vs, geom_ps, depth_ps, blend_vs, blend_ps, res_vs, res_ps]);
    }

    /// Brings the GPU materials and texture arrays up to date with the scene's terrains (dropping those no terrain uses).
    fn update_terrain_materials(&mut self, ents: &[&wce_scene::render_scene::Entity]) {
        wce_prof!("terrain.update_materials");
        let ents = ents.to_vec();
        self.terrain.material_epoch += 1;
        let epoch = self.terrain.material_epoch;
        for e in &ents {
            if let Payload::Terrain(Terrain { material: Some(material), .. }) = &e.payload {
                self.build_terrain_material(material);
                if let Some(g) = self.terrain.mat.get_mut(&material.id) {
                    g.used = epoch;
                }
            }
        }

        self.terrain.mat.retain(|_, g| g.used + MATERIAL_KEEP > epoch);
        for e in &ents {
            if let Payload::Terrain(Terrain { custom: Some(m), .. }) = &e.payload {
                self.build_custom(m, epoch);
            }
        }

        self.terrain.custom.retain(|_, g| g.used + MATERIAL_KEEP > epoch);

        let arrays: std::collections::BTreeSet<(String, bool)> = ents.iter().filter_map(|e| match &e.payload {
            Payload::Terrain(Terrain { material: Some(m), .. }) => Some([(m.diffuse.join("|"), true), (m.normal_tex.join("|"), false)]),
            _ => None,
        }).flatten().collect();
        self.terrain.arrays.retain(|k, _| arrays.contains(k));
    }

    /// The material terrain's passes (surface, blend boxes, resolve) over its entities.
    fn draw_terrain_material(&mut self, ents: &[&wce_scene::render_scene::Entity], snow: Option<&wce_scene::render_scene::SnowRegions>, shaders: [std::rc::Rc<GameShader>; 7]) {
        let ents = ents.to_vec();
        let [geom_vs, geom_ps, depth_ps, blend_vs, blend_ps, res_vs, res_ps] = shaders;
        let Some(geom_il) = self.layout(&geom_vs, "terrain_geom", &geom_layout()) else { return };
        self.ctx.set_rasterizer_state(self.states.rs_none.as_ref());
        self.ctx.set_blend_state(self.states.bs_opaque.as_ref());
        self.ctx.set_input_layout(Some(&geom_il));
        self.ctx.set_topology(D3D11_PRIMITIVE_TOPOLOGY_TRIANGLELIST);

        let (zw_rtv, viewz_rtv) = (self.gbuffer.channels[3].rtv.clone(), self.terrain.viewz.rtv.clone());
        let (ds_depth, ds_equal) = (self.terrain.ds_depth.clone(), self.terrain.ds_equal.clone());
        // A custom-mesh tile's mesh instead of its patches (`terrain_custom`).
        let custom_vs = self.shader(super::terrain_custom::CUSTOM_GEOM_VS);
        let how = custom_vs.as_deref().map_or(CustomDraw::None, CustomDraw::Depth);
        self.draw_terrain_surface(&ents, &geom_vs, &geom_ps, &zw_rtv, &ds_depth, TERRAIN_STENCIL, how);
        self.draw_terrain_surface(&ents, &geom_vs, &depth_ps, &viewz_rtv, &ds_equal, 0, how);
        self.ctx.set_input_layout(Some(&geom_il));

        // The terrain-blend meshes' view-space depth (`geometry_30`): the resolve places them with it.
        let (terrain_draws, draws) = (std::mem::take(&mut self.draw_lists.terrain_draws), std::mem::take(&mut self.draw_lists.draws));
        let (vis_terrain, vis) = (std::mem::take(&mut self.draw_lists.vis_terrain), std::mem::take(&mut self.draw_lists.vis));
        let firsts = self.draw_lists.gbuffer_firsts;
        let d = blend_mesh::BlendDraw {
            dsv: self.gbuffer.dsv.clone(),
            vs: blend_mesh::BLEND_GEOM_VS, ps: blend_mesh::BLEND_GEOM_PS, targets: vec![viewz_rtv.clone()], depth_stencil: self.terrain.ds_lequal.clone(), stencil_ref: 0,
            blend: self.states.bs_opaque.clone(),
        };

        self.draw_blend_meshes([&terrain_draws, &draws], [&vis_terrain, &vis], firsts, &d);
        self.draw_lists.terrain_draws = terrain_draws;
        self.draw_lists.draws = draws;
        self.draw_lists.vis_terrain = vis_terrain;
        self.draw_lists.vis = vis;

        // With MSAA the terrain's depth and mark are expanded into the MS depth now: the snowy boxes below mark it too.
        if self.gbuffer.ms.is_some() {
            self.terrain_depth_to_ms();
        }

        // ---- 2. The blend boxes.
        let el = [
            element(c"POSITION", 0, DXGI_FORMAT_R32G32B32_FLOAT, 0, 0, false),
            element(c"TEXCOORD", 0, DXGI_FORMAT_R32G32B32A32_FLOAT, 1, 0, true),
            element(c"TEXCOORD", 1, DXGI_FORMAT_R32G32B32A32_FLOAT, 1, 16, true),
            element(c"TEXCOORD", 2, DXGI_FORMAT_R32G32B32A32_FLOAT, 1, 32, true),
            element(c"TEXCOORD", 3, DXGI_FORMAT_R32G32B32A32_FLOAT, 1, 48, true),
        ];

        let Some(il) = self.layout(&blend_vs, "terrain_box", &el) else { return };
        let acc = self.terrain.acc.iter().map(|a| a.rtv.clone()).collect::<Vec<_>>();
        self.ctx.set_render_targets(&acc, self.gbuffer.dsv.as_ref());
        self.ctx.set_depth_stencil_state(self.terrain.ds_stencil.as_ref(), 2);
        self.ctx.set_blend_state(self.terrain.bs_add.as_ref());
        self.ctx.set_rasterizer_state(self.terrain.rs_front_cull.as_ref());
        self.ctx.set_input_layout(Some(&il));

        // A campaign tile's resolve: the resolve's one campaign switch is global (it skips the weather's snow on the ground).
        let campaign = ents.iter().any(|e| matches!(&e.payload, Payload::Terrain(t) if t.material.as_ref().is_some_and(|m| m.campaign)));

        // The boxes in view by material and LF map (a context's placements of one tile share their material): one bind, the boxes as instances.
        let mut by_material: Vec<MaterialBoxes> = vec![];
        let mut slot: HashMap<(u64, u64, bool), usize> = HashMap::new();
        let (u, view_proj) = (self.camera.units, self.camera.main_view_proj);
        for e in &ents {
            let Payload::Terrain(t) = &e.payload else { continue };
            let (Some(m), Some((_, b, rot))) = (t.material.clone(), self.terrain.placed.get(&e.id).and_then(|p| p.blend_box)) else { continue };
            if !box_in_frustum(&view_proj, [b[0], b[1], b[2]], [b[3], b[4], b[5]]) {
                continue;
            }

            // One instance: the layer mapping indices (T0), the box corners (T1 min, T2 max, game units), the rotation index of the material's
            // maps and the layer base 0 (T3).
            let o = self.camera.origin;
            let inst = [0.0, 1.0, 2.0, 3.0, (b[0] - o.x) * u, (b[1] - o.y) * u, (b[2] - o.z) * u, 0.0, (b[3] - o.x) * u, (b[4] - o.y) * u, (b[5] - o.z) * u, 0.0, rot, 0.0, 0.0, 0.0];
            // In snow regions: whether the tile's cell (its box's centre) has snow.
            let snowy = snow.is_some_and(|s| s.at(0.5 * (b[0] + b[3]), 0.5 * (b[2] + b[5])));
            let key = (m.id, t.lf.as_ref().map_or(0, |lf| lf.id), snowy);
            match slot.get(&key) {
                Some(&k) => by_material[k].2.extend(inst),
                None => {
                    slot.insert(key, by_material.len());
                    by_material.push((m, t.lf.clone(), inst.to_vec(), snowy));
                }
            }
        }

        // The constants every material shares, set once (they were rebuilt per material: a map context's 192 tiles spent 11 ms a frame
        // building and comparing the 32 KB mapping array); per material only its four layers' scales change below.
        let cbs = &mut self.cbs;

        // ps_height_map_cb: the values the live game shows (dump of a battle) where the tool has no other source; the far-distance blend
        // and the map-wide normal map are switched off (g_far_tile_distance / g_far_blend_range very large): the tile's own maps are the near
        // field, and the editor has one tile.
        cbs.set_f("ps_height_map_cb", "g_mid_distance_details", &[20.0, 550.0, 3.0, 0.9]);
        cbs.set_f("ps_height_map_cb", "g_near_distance_details", &[200.0, 4.0, 0.6, 0.0]);
        cbs.set_f("ps_height_map_cb", "g_mid_distance_normal_strength", &[2.0]);
        cbs.set_f("ps_height_map_cb", "g_mid_distance_slope", &[0.96, 3.0]);
        cbs.set_f("ps_height_map_cb", "g_channel_offsets", &[0.0; 4]);
        cbs.set_f("ps_height_map_cb", "g_normal_scales", &[1.0, 1.0, 1.0]);
        cbs.set_f("ps_height_map_cb", "g_blend_gain", &[1.0]);
        cbs.set_f("ps_height_map_cb", "g_far_tile_distance", &[1.0e9]);
        cbs.set_f("ps_height_map_cb", "g_far_blend_range", &[1.0e9, 2.0e9]);
        cbs.set_f("ps_height_map_cb", "g_far_world_write_uv_offset", &[1.0, 1.0, 0.0, 0.0]);
        cbs.set_f("ps_height_map_cb", "g_far_world_read_uv_offset", &[0.0]);
        cbs.set_f("ps_height_map_cb", "g_blend_contrast", &[100.0]);
        cbs.set_f("ps_height_map_cb", "g_lq_infield_blend_range", &[0.0]);
        cbs.set_i("ps_height_map_cb", "g_is_campaign", i32::from(campaign));

        // The (disabled) map-wide normal map's uv.
        cbs.set_f("vs_height_map_cb", "g_world_dimensions", &[1.0e6, 1.0e6]);

        // ps_texture_array_cb: one layer per texture group, its four variations are the slots 4 * layer + 0..3; g_array_section_sizes.z = 0
        // keeps the index at the tile's own. The shader reads `cb3[mapping + 1]` of the cbuffer, whose row 0 is g_light_params_texture_width:
        // that is array element [mapping] (= the layer here): (metres per repeat, outfield scale, mid-distance strength). Element [layer + 1]
        // left layer 0 with a scale of 0 (uv = world / 0: one flat texel).
        cbs.set_f("ps_texture_array_cb", "g_light_params_texture_width", &[16.0]);
        cbs.set_f("ps_texture_array_cb", "g_array_section_sizes", &[1.0, 1.0, 0.0, 0.0]);
        cbs.set_f("ps_texture_array_cb", "g_remapped_season", &[0.0]);
        cbs.set_f("ps_texture_array_cb", "g_climate_change_index", &[0.0]);
        cbs.set_f("ps_texture_array_cb", "g_layer_exempt_groups", &[1.0e6; 4]);
        cbs.set_f("ps_texture_array_cb", "g_sea_texture_index", &[0.0, 0.0]);

        // g_array_mapping[i] = i (a float per 16-byte element).
        let mut map = vec![0.0f32; 2048 * 4];
        for i in 0..2048 {
            map[i * 4] = i as f32;
        }

        cbs.set_f("ps_mapping_array", "g_array_mapping", &map);
        cbs.set_f("ps_texture_array_cb", "g_blend_pixel_scale_array", &[0.0f32; 400 * 4]);
        // What every material shares, bound once: the shaders, the depth map and the samplers as the blend shader declares them (fn
        // 0x116fd320; filter / address are D3D11 enums): the texture-quality class (vtable 0x11c8bab8, the filter from the texture filtering
        // setting, fn 0x1177f410: anisotropic 2..16 or trilinear) wraps, except s_alpha_map (clamp); s_depth_map and s_light_params are
        // point / clamp; s_normal_array is trilinear / wrap.
        self.upload();
        self.bind(&blend_vs);
        self.bind(&blend_ps);
        null_ps_srvs(&self.ctx, 16);
        let ctx = &self.ctx;
        bind_srv(ctx, &blend_ps, "s_depth_map", self.gbuffer.channels[3].srv.as_ref());
        ps_samplers(ctx, &blend_ps, &self.states.s_wrap_aniso);
        bind_sampler(ctx, &blend_ps, "s_alpha_map", self.states.s_clamp_aniso.as_ref());
        bind_sampler(ctx, &blend_ps, "s_depth_map", self.states.s_clamp_point.as_ref());
        bind_sampler(ctx, &blend_ps, "s_light_params", self.states.s_clamp_point.as_ref());
        bind_sampler(ctx, &blend_ps, "s_normal_array", self.states.s_wrap_linear.as_ref());

        // Every material's boxes in the instance buffer at once (a map context blends hundreds of materials: mapping the buffer per
        // material cost 8 us each), each drawn from its first instance; more than it holds, a buffer's worth at a time.
        let Some(inst_vb) = self.terrain.inst_vb.clone() else { return };
        let capacity = INSTANCE_BYTES / 4 / 16;
        let total = by_material.iter().map(|(_, _, b, _)| b.len() / 16).sum::<usize>();
        let mut box_first = vec![None; by_material.len()];
        if total <= capacity {
            let all = by_material.iter().flat_map(|(_, _, b, _)| b.iter().copied()).collect::<Vec<f32>>();
            if ctx.map(&inst_vb, 0, D3D11_MAP_WRITE_DISCARD, 0).is_ok_and(|mut ms| ms.write(0, &all)) {
                let mut at = 0u32;
                for (k, (_, _, b, _)) in by_material.iter().enumerate() {
                    box_first[k] = Some(at);
                    at += (b.len() / 16) as u32;
                }
            }
        }

        ctx.set_vertex_buffers(0, &[self.terrain.box_vb.clone(), Some(inst_vb.clone())], &[12, 64], &[0, 0]);
        ctx.set_index_buffer(self.terrain.box_ib.as_ref(), DXGI_FORMAT_R16_UINT, 0);

        // With MSAA a snowy tile's boxes draw again into the MS depth-stencil alone, so that their mark (bit 0) reaches its samples
        // (the snowy resolve tests it there); the same shader decides which pixels (it discards outside the tile and below the alpha).
        let ms_dsv = self.gbuffer.ms.as_ref().and_then(|m| m.depth.dsv.clone());
        let s1_dsv = self.gbuffer.dsv.clone();
        let draw_boxes = |ctx: &ID3D11DeviceContext, count: u32, first: u32, mark_ms: bool| {
            ctx.draw_indexed_instanced(36, count, 0, 0, first);
            if let (true, Some(dsv)) = (mark_ms, &ms_dsv) {
                ctx.set_render_targets(&[], Some(dsv));
                ctx.draw_indexed_instanced(36, count, 0, 0, first);
                ctx.set_render_targets(&acc, s1_dsv.as_ref());
            }
        };
        for (k, (m, lf, boxes, snow)) in by_material.into_iter().enumerate() {
            let g = self.terrain.mat.get(&m.id).cloned().unwrap_or_default();
            if snow {
                self.ctx.set_depth_stencil_state(self.terrain.ds_stencil_snow.as_ref(), 3);
            } else {
                self.ctx.set_depth_stencil_state(self.terrain.ds_stencil.as_ref(), 2);
            }

            // The LF map's normals, at uv = (x, -z) / g_world_dimensions (wrapping); a flat normal without one.
            let lf_normal = lf.as_ref().and_then(|lf| self.terrain.lf.get(&lf.id)).and_then(|l| l.normal.srv.clone()).or_else(|| g.lf_normal.srv.clone());
            let world = lf.as_ref().filter(|lf| lf.normal.w > 0).map_or([1.0e6, 1.0e6], |lf| [lf.normal_world[0] * u, lf.normal_world[1] * u]);
            self.cbs.set_f("vs_height_map_cb", "g_world_dimensions", &world);
            let mut arr = [0.0f32; 16];
            for l in 0..4 {
                arr[l * 4] = super::tiling(m.pixel_scale[l], super::ORIGIN_REACH * u);
                arr[l * 4 + 1] = m.outfield_scale[l];
                arr[l * 4 + 2] = m.mid_strength[l];
            }

            // Elements 0 .. 3 (the rest stays zero).
            self.cbs.set_f("ps_texture_array_cb", "g_blend_pixel_scale_array", &arr);
            self.upload();
            let ctx = &self.ctx;
            bind_srv(ctx, &blend_ps, "s_alpha_map", g.alpha.srv.as_ref());
            bind_srv(ctx, &blend_ps, "s_blend_map", g.blend.srv.as_ref());
            bind_srv(ctx, &blend_ps, "s_index_map", g.index.srv.as_ref());
            bind_srv(ctx, &blend_ps, "s_normal_map", g.normal.srv.as_ref());
            bind_srv(ctx, &blend_ps, "s_lf_normal_map", lf_normal.as_ref());
            bind_srv(ctx, &blend_ps, "s_light_params", g.light_params.as_ref());
            bind_srv(ctx, &blend_ps, "t_diffuse_array", g.diffuse.srv.as_ref());
            bind_srv(ctx, &blend_ps, "t_normal_array", g.normals.srv.as_ref());
            if let Some(first) = box_first[k] {
                draw_boxes(ctx, (boxes.len() / 16) as u32, first, snow);
                self.draw_stats.draw_calls += 1;
                continue;
            }

            for chunk in boxes.chunks(capacity * 16) {
                if !ctx.map(&inst_vb, 0, D3D11_MAP_WRITE_DISCARD, 0).is_ok_and(|mut ms| ms.write(0, chunk)) {
                    continue;
                }

                draw_boxes(ctx, (chunk.len() / 16) as u32, 0, snow);
                self.draw_stats.draw_calls += 1;
            }
        }

        // ---- 2b. The terrain-blend meshes' own material (`custom_material`): added to the accumulation with the weight 10 * vertex alpha.
        {
            let (terrain_draws, draws) = (std::mem::take(&mut self.draw_lists.terrain_draws), std::mem::take(&mut self.draw_lists.draws));
            let (vis_terrain, vis) = (std::mem::take(&mut self.draw_lists.vis_terrain), std::mem::take(&mut self.draw_lists.vis));
            let d = blend_mesh::BlendDraw {
                dsv: self.gbuffer.dsv.clone(),
                vs: blend_mesh::BLEND_VS, ps: blend_mesh::BLEND_MATERIAL_PS, targets: self.terrain.acc.iter().map(|a| a.rtv.clone()).collect(),
                depth_stencil: self.terrain.ds_equal.clone(), stencil_ref: 0, blend: self.terrain.bs_add.clone(),
            };

            self.draw_blend_meshes([&terrain_draws, &draws], [&vis_terrain, &vis], firsts, &d);
            self.draw_lists.terrain_draws = terrain_draws;
            self.draw_lists.draws = draws;
            self.draw_lists.vis_terrain = vis_terrain;
            self.draw_lists.vis = vis;
        }

        // ---- 2c. The custom meshes' own material (`custom_geometry`).
        self.draw_custom_material(&ents);

        // ---- 3. The resolve: the terrain pixels into the G-buffer.
        let Payload::Terrain(first) = &ents[0].payload else { return };
        let Some(m) = first.material.clone() else { return };
        let g = self.terrain.mat.get(&m.id).cloned().unwrap_or_default();
        let el = [element(c"POSITION", 0, DXGI_FORMAT_R32G32B32A32_FLOAT, 0, 0, false), element(c"TEXCOORD", 0, DXGI_FORMAT_R32G32_FLOAT, 0, 16, false)];
        let Some(il) = self.layout(&res_vs, "terrain_resolve", &el) else { return };
        let rtvs = self.gbuffer.scene_rtvs();
        self.ctx.set_render_targets(&rtvs, self.gbuffer.scene_dsv().as_ref());
        self.ctx.set_depth_stencil_state(self.terrain.ds_resolve.as_ref(), 2);
        self.ctx.set_blend_state(self.states.bs_opaque.as_ref());
        self.ctx.set_rasterizer_state(self.states.rs_none.as_ref());
        self.ctx.set_input_layout(Some(&il));

        let cbs = &mut self.cbs;
        cbs.set_f("vs_height_map_cb", "g_world_dimensions", &[1.0e6, 1.0e6]);
        cbs.set_f("vs_height_map_cb", "g_far_terrain_scale", &[1.0]);
        cbs.set_f("vs_height_map_cb", "g_layer1_texture_index", &[0.0]);
        cbs.set_f("vs_height_map_cb", "g_layer2_texture_index", &[0.0]);
        cbs.set_f("ps_height_map_cb", "g_far_blend_range", &[1.0e9, 2.0e9]);
        cbs.set_f("shroud_PS", "g_shroud_enabled", &[0.0]);
        self.upload();
        self.bind(&res_vs);
        self.bind(&res_ps);
        null_ps_srvs(&self.ctx, 16);
        let dc = if m.detail_colour.is_empty() { None } else { self.textures.get(&m.detail_colour, false) };
        let dn = if m.detail_normal.is_empty() { None } else { self.textures.get(&m.detail_normal, false) };
        let snow = self.snow_normals_srv();
        let ctx = &self.ctx;
        bind_srv(ctx, &res_ps, "s_albedo_map", self.terrain.acc[0].srv.as_ref());
        bind_srv(ctx, &res_ps, "s_terrain_normal_map", self.terrain.acc[1].srv.as_ref());
        bind_srv(ctx, &res_ps, "s_terrain_gloss_map", self.terrain.acc[2].srv.as_ref());
        bind_srv(ctx, &res_ps, "s_depth_map", self.terrain.viewz.srv.as_ref());
        bind_srv(ctx, &res_ps, "s_far_diffuse_map", g.black.srv.as_ref());
        bind_srv(ctx, &res_ps, "s_far_normal_map", g.lf_normal.srv.as_ref());
        bind_srv(ctx, &res_ps, "s_mask_map", g.black.srv.as_ref());
        bind_srv(ctx, &res_ps, "s_shroud", self.res.white_srv.as_ref());
        bind_srv(ctx, &res_ps, "s_snow_normals", snow.as_ref());
        bind_srv(ctx, &res_ps, "t_diffuse_array", g.diffuse.srv.as_ref());
        let dc = dc.and_then(|t| t.read().ok().and_then(|t| t.srv.clone())).or_else(|| self.res.white_srv.clone());
        let dn = dn.and_then(|t| t.read().ok().and_then(|t| t.srv.clone())).or_else(|| g.lf_normal.srv.clone());
        bind_srv(ctx, &res_ps, "s_mid_distance_detail_map_colour", dc.as_ref());
        bind_srv(ctx, &res_ps, "s_mid_distance_detail_map_normal", dn.as_ref());
        // The samplers as the terrain shader declares them (fn 0x117000c0): the accumulation maps point / clamp, the detail, far, mask maps
        // and the diffuse array the texture-quality class (anisotropic / wrap); s_snow_normals trilinear / wrap (fn 0x116f7b40). s_shroud
        // is not declared there (clamp linear here, an all-white map).
        ps_samplers(ctx, &res_ps, &self.states.s_wrap_aniso);
        for n in ["s_albedo_map", "s_terrain_normal_map", "s_terrain_gloss_map", "s_depth_map"] {
            bind_sampler(ctx, &res_ps, n, self.states.s_clamp_point.as_ref());
        }

        bind_sampler(ctx, &res_ps, "s_snow_normals", self.states.s_wrap_linear.as_ref());
        bind_sampler(ctx, &res_ps, "s_shroud", self.states.s_clamp_linear.as_ref());
        ctx.set_vertex_buffers(0, std::slice::from_ref(&self.terrain.resolve_vb), &[24], &[0]);
        ctx.set_topology(D3D11_PRIMITIVE_TOPOLOGY_TRIANGLELIST);

        // With snow regions: the pixels of the tiles outside them as the weather snows, then those inside at their intensity.
        if let Some(region) = self.snow_region {
            let weather = self.snow_weather;
            self.set_snowing(weather);
            self.ctx.draw(6, 0);
            self.set_snowing(region);
            self.ctx.set_depth_stencil_state(self.terrain.ds_resolve_snow.as_ref(), 3);
            self.draw_stats.draw_calls += 1;
        }

        self.ctx.draw(6, 0);
        self.draw_stats.draw_calls += 1;
        null_ps_srvs(&self.ctx, 16);
        self.ctx.set_rasterizer_state(self.states.rs_back.as_ref());
        self.ctx.set_blend_state(self.states.bs_opaque.as_ref());
    }

    /// With MSAA (docs/plans/msaa.md): the terrain drew its depth and its stencil mark into the one-sample depth; the game's
    /// `ps_antiresolve_with_fixed_stencil` (resolve_depth_buffer.hlsl) writes that depth into every sample of the MS depth where the mark
    /// is set (it discards elsewhere), where it is nearer than what the meshes left, with the terrain's mark as the stencil reference.
    fn terrain_depth_to_ms(&mut self) {
        let Some(ps) = self.shader(ANTIRESOLVE_DEPTH_PS) else { return };
        let (depth, stencil) = (self.gbuffer.depth.depth_srv.clone(), self.gbuffer.depth.stencil_srv.clone());
        let Some(ms_dsv) = self.gbuffer.ms.as_ref().and_then(|m| m.depth.dsv.clone()) else { return };
        self.ctx.set_render_targets(&[], Some(&ms_dsv));
        self.ctx.set_depth_stencil_state(self.terrain.ds_depth.as_ref(), TERRAIN_STENCIL);
        self.ctx.set_rasterizer_state(self.states.rs_none.as_ref());
        self.ctx.set_viewports(&[viewport(0.0, 0.0, self.gbuffer.w as f32, self.gbuffer.h as f32)]);
        self.bind(&ps);
        bind_srv(&self.ctx, &ps, "t_depthbuffer", depth.as_ref());
        bind_srv(&self.ctx, &ps, "t_stencilbuffer", stencil.as_ref());
        self.upload();
        self.fullscreen(&ps);
        null_ps_srvs(&self.ctx, 4);
    }

    /// Draws the visible patches of every terrain entity, grouped by size, with a pixel shader into one target.
    pub(crate) fn draw_terrain_surface(
        &mut self, ents: &[&wce_scene::render_scene::Entity], vs: &GameShader, ps: &GameShader, rtv: &Option<ID3D11RenderTargetView>, ds: &Option<ID3D11DepthStencilState>,
        stencil_ref: u32, how: CustomDraw,
    ) {
        self.ctx.set_render_targets(std::slice::from_ref(rtv), self.gbuffer.dsv.as_ref());
        self.ctx.set_depth_stencil_state(ds.as_ref(), stencil_ref);
        let (cull, clip) = (self.camera.main_view_proj, self.camera.cb_view_proj);
        let triangles = self.draw_terrain_patches_with(ents, vs, ps, &cull, &clip, how);
        self.draw_stats.triangles += triangles;
    }

    /// The terrain entities the material pass draws, with their geometry built and placed.
    fn terrain_surface_entities<'s>(&mut self, scene: &'s RenderScene) -> Vec<&'s wce_scene::render_scene::Entity> {
        wce_prof!("terrain.surface_entities");
        let mut ents = vec![];
        for id in self.terrain.entities.clone() {
            let Some(e) = scene.find(id) else { continue };
            let Payload::Terrain(t) = &e.payload else { continue };
            self.build_hf(id, t);
            if let Some(m) = &t.custom
                && !self.terrain.custom.contains_key(&m.id)
            {
                self.build_custom(m, self.terrain.material_epoch);
            }

            if let Some(lf) = &t.lf {
                self.build_lf(lf);
            }

            self.place_terrain(id, &e.world, t);
            if self.terrain.placed.get(&id).is_some_and(|p| p.key == hf_key(t)) {
                ents.push(e);
            }
        }

        ents
    }

    /// The terrain into the bound shadow cascade, as the game casts it: the patches with rigidterrain's `vs30_shadow_new` (geom_new's
    /// placement plus a 9 cm lift past 20 m from the camera) and `ps30_shadow` (clip z / w). `view_proj` (the cascade's, scene units) culls the
    /// patches, `relative` (the same of positions relative to the render origin) draws them. Returns the triangles drawn.
    pub(crate) fn draw_terrain_shadow(&mut self, scene: &RenderScene, view_proj: &Float4x4, relative: &Float4x4) -> u64 {
        // Rome 2's shadow vertex shader is its surface one (the LF window through lf_block_cb, two inputs it never reads).
        let rome2 = self.game == super::game::RenderGame::Rome2;
        let (Some(vs), Some(ps)) = (self.shader(if rome2 { super::terrain_rome2::R2_SHADOW_VS } else { SHADOW_NEW_VS }), self.shader(SHADOW_TERRAIN_PS)) else {
            return 0;
        };

        let ents = self.terrain_surface_entities(scene);
        let il = if rome2 { self.layout(&vs, "terrain_shadow_rome2", &super::terrain_rome2::geom_layout()) } else { self.layout(&vs, "terrain_shadow", &geom_layout()) };
        let Some(il) = il else { return 0 };
        if ents.is_empty() {
            return 0;
        }

        self.ctx.set_input_layout(Some(&il));
        let clip = game_clip(relative, self.camera.units);
        let triangles = self.draw_terrain_patches(&ents, &vs, &ps, view_proj, &clip);

        // The cascade's matrix back for the objects after.
        self.cbs.set_m("camera_VS_PS", "view_projection", clip.data());
        self.upload();
        triangles
    }

    /// Draws the patches of every terrain entity inside `cull` (scene units), grouped by size and stride, into the bound targets; `clip` is the
    /// view-projection the shaders get (game units). Each LF map's terrains are drawn in its frame (see `lf_draw`: the camera constants move
    /// with it and are put back after); the placements of one patch go out as instances of one draw. Returns the triangles drawn.
    fn draw_terrain_patches(&mut self, ents: &[&wce_scene::render_scene::Entity], vs: &GameShader, ps: &GameShader, cull: &Float4x4, clip: &Float4x4) -> u64 {
        self.draw_terrain_patches_with(ents, vs, ps, cull, clip, CustomDraw::None)
    }

    /// [`Self::draw_terrain_patches`], and the custom meshes of each LF group (`how`): a terrain with a custom mesh has no patches drawn (the
    /// game's tile has none: its mesh is all its geometry), and the material pass draws the meshes alone.
    pub(crate) fn draw_terrain_patches_with(
        &mut self, ents: &[&wce_scene::render_scene::Entity], vs: &GameShader, ps: &GameShader, cull: &Float4x4, clip: &Float4x4, how: CustomDraw,
    ) -> u64 {
        let by_id: HashMap<EntityId, &wce_scene::render_scene::Entity> = ents.iter().map(|e| (e.id, *e)).collect();
        let material_only = matches!(how, CustomDraw::Material);
        let geom_il = match how {
            CustomDraw::Depth(_) => self.layout(vs, "terrain_geom", &geom_layout()),
            _ => None,
        };

        // Groups by LF map, in the entities' order.
        let mut groups: Vec<(Option<Arc<LfMap>>, Vec<EntityId>)> = vec![];
        for e in ents {
            let Payload::Terrain(t) = &e.payload else { continue };
            let id = t.lf.as_ref().map_or(0, |lf| lf.id);
            match groups.iter_mut().find(|(lf, _)| lf.as_ref().map_or(0, |lf| lf.id) == id) {
                Some((_, v)) => v.push(e.id),
                None => groups.push((t.lf.clone(), vec![e.id])),
            }
        }

        let (eye, units, origin) = (self.terrain.lod_eye, self.camera.units, self.camera.origin);
        let (view, eye_cb) = (self.camera.cb_view, self.camera.cb_eye);
        let mut triangles = 0;
        for (lf, ids) in groups {
            // vs_height_map_cb: the LF window (unorm 0..1 times the map's scale, less its offset, in game units), its size and the world's,
            // the offset that puts it where it is; the high-frequency height comes from the vertices (the instance rows are in game units).
            let d = match &lf {
                Some(lf) => self.lf_draw(lf, eye),
                // No LF: the frame of the render origin.
                None => LfDraw { srv: self.lf_zero(), n: 2.0, width: 1.0e6, scale: 0.0, offset: 0.0, uv_offset: 0.0, shift: [origin.x, origin.z] },
            };

            let [sx, sz] = d.shift;
            let cbs = &mut self.cbs;
            cbs.set_f("vs_height_map_cb", "g_height_map_lf_scale", &[d.scale * units]);
            cbs.set_f("vs_height_map_cb", "g_height_map_hf_scale", &[1.0]);
            cbs.set_f("vs_height_map_cb", "g_height_map_vertical_offset", &[d.offset * units]);
            cbs.set_f("vs_height_map_cb", "g_height_map_dimension", &[d.n, d.n, 1.0 / d.n, 1.0 / d.n]);
            cbs.set_f("vs_height_map_cb", "g_world_dimensions", &[d.width * units, d.width * units]);
            cbs.set_f("vs_height_map_cb", "g_lf_uv_offset", &[d.uv_offset]);
            // Rome 2's shader places the window through lf_block_cb's rectangles: uv = (x, z) / size, v = 1 - v, then (u + offset,
            // (1 - v) - offset): Attila's (x / size + offset, 1 - (z / size + offset)). (A mirrored v went unseen on a flat LF.)
            cbs.set_f("lf_block_cb", "g_height_map_dimension", &[d.n, d.n, 1.0 / d.n, 1.0 / d.n]);
            cbs.set_f("lf_block_cb", "g_ws_lf_aabb", &[0.0, 0.0, d.width * units, d.width * units]);
            cbs.set_f("lf_block_cb", "g_lf_aabb", &[d.uv_offset, -d.uv_offset, 1.0, 1.0]);
            cbs.set_f("lf_block_cb", "g_normal_lf_aabb", &[0.0, 0.0, 1.0, 1.0]);

            // The camera in the group's frame: world' = world - shift; the camera constants are relative to the render origin.
            let (rx, rz) = (sx - origin.x, sz - origin.z);
            let mut to_world = Float4x4::IDENTITY;
            (to_world.m[0][3], to_world.m[2][3]) = (rx * units, rz * units);
            cbs.set_m("camera_VS_PS", "view_projection", (*clip * to_world).data());
            cbs.set_m("camera_VS_PS", "view", (view * to_world).data());
            cbs.set_f("camera_VS_PS", "camera_position", &[eye_cb.x - rx * units, eye_cb.y, eye_cb.z - rz * units]);
            self.upload();
            self.bind(vs);
            self.bind(ps);
            bind_srv(&self.ctx, vs, "t_low_res_height_map", d.srv.as_ref());
            bind_srv(&self.ctx, vs, "s_low_res_height_map", d.srv.as_ref());
            // Rome 2's shader filters the LF itself (four samples, its own weights): point samples, else every texel edge is a step.
            bind_sampler(&self.ctx, vs, "s_low_res_height_map", self.states.s_clamp_point.as_ref());

            // The batches: (buffer, patch, stride) -> instance rows.
            let mut batches: Vec<(usize, usize, i32, i32, i32, Vec<f32>)> = vec![];
            let mut at: HashMap<(usize, usize, i32), usize> = HashMap::new();
            for id in &ids {
                let custom = by_id.get(id).is_some_and(|e| matches!(&e.payload, Payload::Terrain(t) if t.custom.is_some()));
                if material_only || custom {
                    continue;
                }

                let Some(p) = self.terrain.placed.get(id) else { continue };
                for pp in &p.patches {
                    if !box_in_frustum(cull, pp.bmin, pp.bmax) {
                        continue;
                    }

                    // The stride by distance: one more halving of the vertices per doubling past LOD_STEPS grid steps (2 m steps: every 2nd
                    // vertex from 512 m, every 4th from 1 km ...), a power of two that divides the patch.
                    let k = if p.lod {
                        let (lo, hi) = (pp.bmin, pp.bmax);
                        let (dx, dy, dz) = (eye.x.clamp(lo[0], hi[0]) - eye.x, eye.y.clamp(lo[1], hi[1]) - eye.y, eye.z.clamp(lo[2], hi[2]) - eye.z);
                        let d = (dx * dx + dy * dy + dz * dz).sqrt() / (LOD_STEPS * p.step);
                        let mut k = 1;
                        while k < PATCH_CELLS && d >= (2 * k) as f32 && pp.cx % (2 * k) == 0 && pp.cz % (2 * k) == 0 {
                            k *= 2;
                        }

                        k
                    } else {
                        1
                    };

                    let slot = *at.entry((p.key, pp.patch, k)).or_insert_with(|| {
                        batches.push((p.key, pp.patch, pp.cx, pp.cz, k, vec![]));
                        batches.len() - 1
                    });

                    // Into the group's frame.
                    let mut row = pp.row;
                    (row[3], row[7], row[11]) = (row[3] - sx * units, row[7] - origin.y * units, row[11] - sz * units);
                    batches[slot].5.extend(row);
                }
            }

            // Every batch's rows in the instance buffer at once, each drawn from its first; more than it holds, a buffer's worth at a time.
            let Some(inst_vb) = self.terrain.inst_vb.clone() else { continue };
            let capacity = INSTANCE_BYTES / 48;
            let total = batches.iter().map(|b| b.5.len() / 12).sum::<usize>();
            let mut first = vec![None; batches.len()];
            if total <= capacity {
                let all = batches.iter().flat_map(|b| b.5.iter().copied()).collect::<Vec<f32>>();
                if self.ctx.map(&inst_vb, 0, D3D11_MAP_WRITE_DISCARD, 0).is_ok_and(|mut ms| ms.write(0, &all)) {
                    let mut n = 0u32;
                    for (k, b) in batches.iter().enumerate() {
                        first[k] = Some(n);
                        n += (b.5.len() / 12) as u32;
                    }
                }
            }

            for (k, (key, patch, cx, cz, stride, rows)) in batches.into_iter().enumerate() {
                let Some(vb) = self.terrain.hf.get(&key).and_then(|g| g.patches.get(patch)).and_then(|p| p.vb.clone()) else { continue };
                let (ib, index_count) = self.terrain_grid_ib(cx, cz, stride);
                self.ctx.set_vertex_buffers(0, &[Some(vb), Some(inst_vb.clone())], &[12, 48], &[0, 0]);
                self.ctx.set_index_buffer(ib.as_ref(), DXGI_FORMAT_R16_UINT, 0);
                let count = (rows.len() / 12) as u32;
                if let Some(first) = first[k] {
                    self.ctx.draw_indexed_instanced(index_count, count, 0, 0, first);
                    self.draw_stats.draw_calls += 1;
                } else {
                    for chunk in rows.chunks(capacity * 12) {
                        if !self.ctx.map(&inst_vb, 0, D3D11_MAP_WRITE_DISCARD, 0).is_ok_and(|mut ms| ms.write(0, chunk)) {
                            continue;
                        }

                        self.ctx.draw_indexed_instanced(index_count, (chunk.len() / 12) as u32, 0, 0, 0);
                        self.draw_stats.draw_calls += 1;
                    }
                }

                triangles += u64::from(index_count / 3) * u64::from(count);
            }

            if !matches!(how, CustomDraw::None) {
                triangles += self.draw_custom_meshes(&by_id, &ids, how, cull, [sx, sz]);
                if let Some(il) = &geom_il {
                    self.ctx.set_input_layout(Some(il));
                    self.bind(vs);
                }
            }
        }

        // The camera as it was.
        let cbs = &mut self.cbs;
        cbs.set_m("camera_VS_PS", "view_projection", clip.data());
        cbs.set_m("camera_VS_PS", "view", view.data());
        cbs.set_f("camera_VS_PS", "camera_position", &[eye_cb.x, eye_cb.y, eye_cb.z]);
        self.upload();
        triangles
    }
}
