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

use std::collections::BTreeMap;

use wce_foundation::math::{Float3, Float4x4};
use wce_foundation::wce_prof;
use wce_scene::render_scene::{EntityId, Payload, RenderScene, Terrain, TerrainImage, TerrainMaterial};

use super::*;
use crate::shader_lib::{bind_sampler, bind_srv};
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

const PATCH_CELLS: i32 = 64;

/// A patch at this many grid steps from the eye starts to skip vertices (see draw_terrain_patches).
const LOD_STEPS: f32 = 128.0;

/// The instance buffer's size.
const INSTANCE_BYTES: usize = 64 * 4096;

/// A patch of the terrain grid for culling.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Patch {
    bmin: [f32; 3],
    bmax: [f32; 3],
    pi: i32,
    pj: i32,
    cx: i32,
    cz: i32,
}

/// The height texture and patches of one terrain entity.
#[derive(Clone, Debug)]
pub(crate) struct TerrainGeo {
    version: u64,
    tex: i32,
    step: f32,
    ox: f32,
    oz: f32,

    /// The entity's transform: world z = kz * z + tz, world x / y shifted by tx / ty (refreshed every frame).
    kz: f32,
    tx: f32,
    ty: f32,
    tz: f32,

    /// The height texture's sample spacing and the world x (= z) of its sample 0 (before the entity's translation).
    tex_step: f32,
    uv0: f32,

    /// The shared map it reads (0: its own).
    shared: u64,

    /// Coarser index buffers with distance.
    lod: bool,
    height_srv: Option<ID3D11ShaderResourceView>,
    patches: Vec<Patch>,
}

impl Default for TerrainGeo {
    fn default() -> Self {
        Self {
            version: u64::MAX, tex: 0, step: 1.0, ox: 0.0, oz: 0.0, kz: 1.0, tx: 0.0, ty: 0.0, tz: 0.0, tex_step: 1.0, uv0: 0.0, shared: 0, lod: false, height_srv: None,
            patches: vec![],
        }
    }
}

/// The GPU side of a terrain material.
#[derive(Clone, Debug, Default)]
pub(crate) struct TerrainMatGpu {
    blend: Texture,
    index: Texture,
    normal: Texture,
    alpha: Texture,
    lf_normal: Texture,
    black: Texture,
    diffuse: Texture,
    normals: Texture,
    normal_id: u64,
    light_params: Option<ID3D11ShaderResourceView>,
}

/// The terrain pass's state.
#[derive(Default)]
pub(crate) struct TerrainGpuState {
    pub active: bool,

    /// The frame camera's eye, for the patches' level of detail (the shadow pass runs before the camera is set).
    pub lod_eye: Float3,
    pub failed: bool,
    geo: BTreeMap<EntityId, TerrainGeo>,
    mat: BTreeMap<u64, TerrainMatGpu>,

    /// The shared height maps by id: (texture side, view).
    shared: BTreeMap<u64, (i32, Option<ID3D11ShaderResourceView>)>,

    /// The climates' texture arrays by (paths, sRGB): the materials of a scene's tiles mostly use the same ones.
    arrays: BTreeMap<(String, bool), Texture>,
    /// Index buffers of a patch of cx x cz cells at a stride, and their index counts.
    grid_ib: BTreeMap<(i32, i32, i32), (Option<ID3D11Buffer>, u32)>,
    entities: Vec<EntityId>,
    pub(crate) viewz: Target,
    acc: [Target; 3],
    rt_w: u32,
    rt_h: u32,
    grid_vb: Option<ID3D11Buffer>,
    box_vb: Option<ID3D11Buffer>,
    box_ib: Option<ID3D11Buffer>,
    resolve_vb: Option<ID3D11Buffer>,
    inst_vb: Option<ID3D11Buffer>,
    ds_depth: Option<ID3D11DepthStencilState>,
    ds_equal: Option<ID3D11DepthStencilState>,
    ds_lequal: Option<ID3D11DepthStencilState>,
    ds_stencil: Option<ID3D11DepthStencilState>,
    ds_resolve: Option<ID3D11DepthStencilState>,
    bs_add: Option<ID3D11BlendState>,
    rs_front_cull: Option<ID3D11RasterizerState>,
}

/// The frustum test of an axis-aligned box against a view-projection matrix (D3D clip volume: -w <= x, y <= w, 0 <= z <= w).
fn box_in_frustum(vp: &Float4x4, bmin: [f32; 3], bmax: [f32; 3]) -> bool {
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

/// The entity transforms the tile tool uses: a translation, and a scale of z (the height map's rows are not always as far apart as its columns).
/// No rotation, no scale and no horizontal move (a terrain reading a shared height map: the map is in world coordinates).
fn is_identity_placement(w: &Float4x4) -> bool {
    is_simple_world(w) && (w.m[2][2] - 1.0).abs() < 1e-6 && w.m[0][3].abs() < 1e-6 && w.m[2][3].abs() < 1e-6
}

fn is_simple_world(w: &Float4x4) -> bool {
    for r in 0..3 {
        for c in 0..3 {
            if r == 2 && c == 2 {
                continue;
            }

            if (w.m[r][c] - if r == c { 1.0 } else { 0.0 }).abs() > 1e-5 {
                return false;
            }
        }
    }

    w.m[2][2] > 1e-4
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

        // Only the terrain pixels (stencil 2) ...
        let keep = D3D11_DEPTH_STENCILOP_DESC { StencilPassOp: D3D11_STENCIL_OP_KEEP, StencilFunc: D3D11_COMPARISON_EQUAL, ..replace };
        let mut s = D3D11_DEPTH_STENCIL_DESC { DepthEnable: false.into(), StencilEnable: true.into(), StencilReadMask: 0xFF, StencilWriteMask: 0xFF, FrontFace: keep, BackFace: keep, ..Default::default() };
        t.ds_stencil = depth_stencil(&dev, &s);

        // ... and they become geometry (stencil 1) for the lights.
        s.FrontFace.StencilPassOp = D3D11_STENCIL_OP_DECR;
        s.BackFace = s.FrontFace;
        t.ds_resolve = depth_stencil(&dev, &s);

        // The tiles accumulate (premultiplied by their alpha).
        t.bs_add = crate::particles::blend_state(&dev, D3D11_RENDER_TARGET_BLEND_DESC {
            BlendEnable: true.into(), SrcBlend: D3D11_BLEND_ONE, DestBlend: D3D11_BLEND_ONE, BlendOp: D3D11_BLEND_OP_ADD, SrcBlendAlpha: D3D11_BLEND_ONE,
            DestBlendAlpha: D3D11_BLEND_ONE, BlendOpAlpha: D3D11_BLEND_OP_ADD, RenderTargetWriteMask: D3D11_COLOR_WRITE_ENABLE_ALL as u8,
        });

        // The blend boxes: their back faces cover the pixels once, also from inside the box.
        let rd = D3D11_RASTERIZER_DESC { FillMode: D3D11_FILL_SOLID, CullMode: D3D11_CULL_FRONT, DepthClipEnable: true.into(), ..Default::default() };
        t.rs_front_cull = dev.create_rasterizer_state(&rd).ok();

        // The 65 x 65 vertex grid of a patch (x, 0, z in cells); the height comes from the texture.
        let mut grid = Vec::with_capacity(((PATCH_CELLS + 1) * (PATCH_CELLS + 1) * 3) as usize);
        for z in 0..=PATCH_CELLS {
            for x in 0..=PATCH_CELLS {
                grid.extend([x as f32, 0.0, z as f32]);
            }
        }

        t.grid_vb = immutable_buffer(&dev, &f32_bytes(&grid), D3D11_BIND_VERTEX_BUFFER);

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

    /// The patches of the heights, the height texture the game's vertex shader samples, and the patch table for culling. The shader reads the map
    /// with its own addressing: uv = world xz / g_world_dimensions + g_lf_uv_offset, the second axis flipped (row r = size - k), no clamp below
    /// sample 1: so the texture is square, padded by one sample at the low side, and `origin - step` is the world position of its sample 0.
    fn build_terrain_geo(&mut self, id: EntityId, t: &Terrain) {
        let shared_id = t.shared.as_ref().map_or(0, |sh| sh.id);
        if self.terrain.geo.get(&id).is_some_and(|g| g.version == t.version && g.shared == shared_id && g.height_srv.is_some()) {
            return;
        }

        let mut g = TerrainGeo { version: t.version, step: t.step, ox: t.origin_x, oz: t.origin_z, shared: shared_id, lod: t.lod, ..Default::default() };
        if let Some(sh) = &t.shared {
            // A shared map: one texture for every terrain that reads it.
            if !self.terrain.shared.contains_key(&sh.id) {
                let srv = self.height_texture(sh.n + 2, |i, j| sh.height_at(i, j));
                self.terrain.shared.insert(sh.id, (sh.n + 2, srv));
            }

            let (side, srv) = self.terrain.shared[&sh.id].clone();
            (g.tex, g.tex_step, g.uv0, g.height_srv) = (side, sh.step, sh.origin - sh.step, srv);
        } else {
            let s = t.nx.max(t.nz) + 2;
            let srv = self.height_texture(s, |i, j| t.height_at(i.clamp(0, t.nx - 1), j.clamp(0, t.nz - 1)));
            (g.tex, g.tex_step, g.uv0, g.height_srv) = (s, t.step, t.origin_x - t.step, srv);
        }

        if g.height_srv.is_none() {
            self.terrain.geo.insert(id, g);
            return;
        }

        let (cx, cz) = (t.nx - 1, t.nz - 1);
        let mut pj = 0;
        while pj * PATCH_CELLS < cz {
            let mut pi = 0;
            while pi * PATCH_CELLS < cx {
                let mut p = Patch { pi, pj, cx: PATCH_CELLS.min(cx - pi * PATCH_CELLS), cz: PATCH_CELLS.min(cz - pj * PATCH_CELLS), ..Default::default() };
                let (mut lo, mut hi) = (1e30f32, -1e30f32);
                for j in pj * PATCH_CELLS..=pj * PATCH_CELLS + p.cz {
                    for i in pi * PATCH_CELLS..=pi * PATCH_CELLS + p.cx {
                        lo = lo.min(t.height_at(i, j));
                        hi = hi.max(t.height_at(i, j));
                    }
                }

                p.bmin[0] = g.ox + (pi * PATCH_CELLS) as f32 * g.step;
                p.bmax[0] = p.bmin[0] + p.cx as f32 * g.step;
                p.bmin[2] = g.oz + (pj * PATCH_CELLS) as f32 * g.step;
                p.bmax[2] = p.bmin[2] + p.cz as f32 * g.step;

                // The shader moves the surface by up to 9 cm.
                p.bmin[1] = lo - 0.2;
                p.bmax[1] = hi + 0.2;
                g.patches.push(p);
                pi += 1;
            }

            pj += 1;
        }

        self.terrain.geo.insert(id, g);
    }

    /// A square height texture of side `s` in the shader's layout: sample (c, r) is grid vertex (c - 1, s - r - 1).
    fn height_texture(&self, s: i32, h: impl Fn(i32, i32) -> f32) -> Option<ID3D11ShaderResourceView> {
        let mut tex = vec![0.0f32; (s * s) as usize];
        for r in 0..s {
            for c in 0..s {
                tex[(r * s + c) as usize] = h(c - 1, s - r - 1);
            }
        }

        let td = D3D11_TEXTURE2D_DESC {
            Width: s as u32, Height: s as u32, MipLevels: 1, ArraySize: 1, Format: DXGI_FORMAT_R32_FLOAT, SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
            Usage: D3D11_USAGE_IMMUTABLE, BindFlags: D3D11_BIND_SHADER_RESOURCE as u32, CPUAccessFlags: 0, MiscFlags: 0,
        };

        let sd = [InitData::new(bytemuck::cast_slice(&tex), (s * 4) as u32)];
        self.dev.create_texture_2d(&td, Some(&sd)).and_then(|tx| self.dev.create_shader_resource_view(&tx, None)).ok()
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

    /// The GPU side of a terrain material: the tile's maps (mipmapped), the climate's texture arrays, the light parameter texture.
    fn build_terrain_material(&mut self, m: &TerrainMaterial) {
        if let Some(have) = self.terrain.mat.get_mut(&m.id) {
            // A height stroke only changes the normal map.
            if have.normal_id != m.normal_id
                && let Some(rgba) = &m.normal.rgba
            {
                let t = make_rgba_texture(&self.dev, m.normal.w, m.normal.h, rgba, false);
                if t.srv.is_some() {
                    have.normal = t;
                    have.normal_id = m.normal_id;
                }
            }

            return;
        }

        let dev = self.dev.clone();
        let mut errors = vec![];
        let mut img = |im: &TerrainImage, what: &str| {
            let t = im.rgba.as_ref().map(|rgba| make_rgba_texture(&dev, im.w, im.h, rgba, false)).unwrap_or_default();
            if t.srv.is_none() {
                errors.push(format!("terrain material: {what} map did not upload"));
            }

            t
        };

        let mut g = TerrainMatGpu {
            normal_id: m.normal_id,
            blend: img(&m.blend, "blend"),
            index: img(&m.index, "index"),
            normal: img(&m.normal, "normal"),

            // Alpha 1 over the whole rectangle: the editor shows one tile; the feathered edge is for neighbours.
            alpha: make_solid_texture(&dev, 0xFFFF_FFFF),

            // A flat normal (the far-distance normal map is switched off with g_far_tile_distance).
            lf_normal: make_solid_texture(&dev, 0x7F00_7FFF),
            black: make_solid_texture(&dev, 0),
            ..Default::default()
        };

        for e in errors {
            self.error(e);
        }

        // The diffuse / normal texture arrays: slot = 4 * group + variation.
        let load_array = |r: &mut Self, paths: &[String; 16], srgb: bool| {
            let mut bytes = vec![None; 16];
            for (i, path) in paths.iter().enumerate() {
                if path.is_empty() || !r.files.exists(path) {
                    continue;
                }

                match r.files.read(path) {
                    Ok(data) => bytes[i] = Some(data),
                    Err(error) => r.error(format!("terrain material: {path}: {error}")),
                }
            }

            let slices = bytes.iter().map(|b| b.as_deref()).collect::<Vec<_>>();
            let (t, bad) = load_dds_array(&r.dev, &slices, srgb, !srgb);
            if bad > 0 {
                r.error(format!("terrain material: {bad} texture(s) of the climate differ in size / format from the others and are drawn neutral"));
            }

            t
        };

        let array = |r: &mut Self, paths: &[String; 16], srgb: bool| {
            let key = (paths.join("|"), srgb);
            if let Some(t) = r.terrain.arrays.get(&key) {
                return t.clone();
            }

            let t = load_array(r, paths, srgb);
            r.terrain.arrays.insert(key, t.clone());
            t
        };

        g.diffuse = array(self, &m.diffuse, true);
        g.normals = array(self, &m.normal_tex, false);
        if g.diffuse.srv.is_none() || g.normals.srv.is_none() {
            self.error("terrain material: the texture arrays did not build".to_owned());
        }

        // s_light_params: 16 texels (smoothness, reflectivity) per slot.
        let lp: Vec<f32> = (0..16).flat_map(|i| [m.smoothness[i], m.reflectivity[i]]).collect();
        let td = D3D11_TEXTURE2D_DESC {
            Width: 16, Height: 1, MipLevels: 1, ArraySize: 1, Format: DXGI_FORMAT_R32G32_FLOAT, SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
            Usage: D3D11_USAGE_IMMUTABLE, BindFlags: D3D11_BIND_SHADER_RESOURCE as u32, CPUAccessFlags: 0, MiscFlags: 0,
        };

        let sd = [InitData::new(bytemuck::cast_slice(&lp), 16 * 8)];
        if let Ok(tx) = self.dev.create_texture_2d(&td, Some(&sd)) {
            g.light_params = self.dev.create_shader_resource_view(&tx, None).ok();
        }

        self.terrain.mat.insert(m.id, g);
    }

    /// Which terrains the pass takes over this frame: every visible terrain has a material (a mixed scene keeps the legacy ground), not the
    /// environment cube.
    pub(crate) fn prepare_terrain_materials(&mut self, scene: &RenderScene) -> bool {
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
            if t.material.is_none() || t.nx < 2 || t.nz < 2 || t.heights.is_empty() || !is_simple_world(&e.world) {
                return false;
            }

            // A shared map starts on the diagonal by construction; it needs the identity.
            if t.shared.is_some() {
                if !is_identity_placement(&e.world) {
                    return false;
                }

                self.terrain.entities.push(e.id);
                continue;
            }

            // The shader has one uv offset for both axes (see build_terrain_geo): the padded origin over the padded size must be the same in x and z.
            let kz = e.world.m[2][2];
            let s = (t.nx.max(t.nz) + 2) as f32;
            let offx = (t.origin_x + e.world.m[0][3] - t.step) / (s * t.step);
            let offz = (kz * t.origin_z + e.world.m[2][3] - kz * t.step) / (s * t.step * kz);
            if (offx - offz).abs() > 1e-5 {
                return false;
            }

            self.terrain.entities.push(e.id);
        }

        // The shared maps no terrain reads any more go.
        let used: std::collections::BTreeSet<u64> = self.terrain.entities.iter().filter_map(|id| scene.find(*id)).filter_map(|e| match &e.payload {
            Payload::Terrain(t) => t.shared.as_ref().map(|sh| sh.id),
            _ => None,
        }).collect();
        self.terrain.shared.retain(|id, _| used.contains(id));
        total > 0 && self.terrain.entities.len() == total
    }

    pub(crate) fn pass_terrain_material(&mut self, scene: &RenderScene) {
        if self.terrain.entities.is_empty() {
            return;
        }

        wce_prof!("render.terrain");
        self.make_terrain_states();
        let shaders = [GEOM_VS, GEOM_PS, DEPTH_PS, BLEND_VS, BLEND_PS, RESOLVE_VS, RESOLVE_PS].map(|name| self.shader(name));
        let [Some(geom_vs), Some(geom_ps), Some(depth_ps), Some(blend_vs), Some(blend_ps), Some(res_vs), Some(res_ps)] = shaders else {
            self.terrain.failed = true;
            return;
        };

        // The targets.
        if self.terrain.viewz.tex.is_none() || self.terrain.rt_w != self.gbuffer.w || self.terrain.rt_h != self.gbuffer.h {
            self.terrain.viewz = make_target(&self.dev, self.gbuffer.w, self.gbuffer.h, DXGI_FORMAT_R32_FLOAT);
            for i in 0..3 {
                self.terrain.acc[i] = make_target(&self.dev, self.gbuffer.w, self.gbuffer.h, DXGI_FORMAT_R16G16B16A16_FLOAT);
            }

            self.terrain.rt_w = self.gbuffer.w;
            self.terrain.rt_h = self.gbuffer.h;
        }

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

        // The materials (and texture arrays) the scene's terrains use stay; the others go.
        let used: std::collections::BTreeSet<u64> = ents.iter().filter_map(|e| match &e.payload {
            Payload::Terrain(Terrain { material: Some(m), .. }) => Some(m.id),
            _ => None,
        }).collect();
        self.terrain.mat.retain(|id, _| used.contains(id));
        for e in &ents {
            if let Payload::Terrain(Terrain { material: Some(material), .. }) = &e.payload {
                self.build_terrain_material(material);
            }
        }

        let arrays: std::collections::BTreeSet<(String, bool)> = ents.iter().filter_map(|e| match &e.payload {
            Payload::Terrain(Terrain { material: Some(m), .. }) => Some([(m.diffuse.join("|"), true), (m.normal_tex.join("|"), false)]),
            _ => None,
        }).flatten().collect();
        self.terrain.arrays.retain(|k, _| arrays.contains(k));

        if ents.is_empty() {
            return;
        }

        let Some(geom_il) = self.layout(&geom_vs, "terrain_geom", &geom_layout()) else { return };
        self.ctx.set_rasterizer_state(self.states.rs_none.as_ref());
        self.ctx.set_blend_state(self.states.bs_opaque.as_ref());
        self.ctx.set_input_layout(Some(&geom_il));
        self.ctx.set_topology(D3D11_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
        self.ctx.set_vertex_buffers(0, std::slice::from_ref(&self.terrain.grid_vb), &[12], &[0]);

        let (zw_rtv, viewz_rtv) = (self.gbuffer.channels[3].rtv.clone(), self.terrain.viewz.rtv.clone());
        let (ds_depth, ds_equal) = (self.terrain.ds_depth.clone(), self.terrain.ds_equal.clone());
        self.draw_terrain_surface(&ents, &geom_vs, &geom_ps, &zw_rtv, &ds_depth, 2);
        self.draw_terrain_surface(&ents, &geom_vs, &depth_ps, &viewz_rtv, &ds_equal, 0);

        // The terrain-blend meshes' view-space depth (`geometry_30`): the resolve places them with it.
        let (terrain_draws, draws) = (std::mem::take(&mut self.draw_lists.terrain_draws), std::mem::take(&mut self.draw_lists.draws));
        let (vis_terrain, vis) = (std::mem::take(&mut self.draw_lists.vis_terrain), std::mem::take(&mut self.draw_lists.vis));
        let firsts = self.draw_lists.gbuffer_firsts;
        let d = blend_mesh::BlendDraw {
            vs: blend_mesh::BLEND_GEOM_VS, ps: blend_mesh::BLEND_GEOM_PS, targets: vec![viewz_rtv.clone()], depth_stencil: self.terrain.ds_lequal.clone(), stencil_ref: 0,
            blend: self.states.bs_opaque.clone(),
        };

        self.draw_blend_meshes([&terrain_draws, &draws], [&vis_terrain, &vis], firsts, &d);
        self.draw_lists.terrain_draws = terrain_draws;
        self.draw_lists.draws = draws;
        self.draw_lists.vis_terrain = vis_terrain;
        self.draw_lists.vis = vis;

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

        // The boxes by material (a context's placements of one tile share theirs): one bind, the boxes as instances.
        let mut by_material: Vec<(std::sync::Arc<TerrainMaterial>, Vec<f32>)> = vec![];
        for e in &ents {
            let Payload::Terrain(t) = &e.payload else { continue };
            let Some(m) = t.material.clone() else { continue };

            // One instance: the layer mapping indices (T0), the box corners (T1 min, T2 max, game units), rotation index 0 and the layer base 0 (T3).
            let u = f64::from(self.camera.units);
            let inst = [0.0, 1.0, 2.0, 3.0, (m.x0 * u) as f32, (m.y0 * u) as f32, (m.z0 * u) as f32, 0.0, (m.x1 * u) as f32, (m.y1 * u) as f32, (m.z1 * u) as f32, 0.0, 0.0, 0.0, 0.0, 0.0];
            match by_material.iter_mut().find(|(have, _)| have.id == m.id) {
                Some((_, v)) => v.extend(inst),
                None => by_material.push((m, inst.to_vec())),
            }
        }

        for (m, boxes) in by_material {
            let g = self.terrain.mat.get(&m.id).cloned().unwrap_or_default();
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
            cbs.set_i("ps_height_map_cb", "g_is_campaign", 0);

            // The (disabled) map-wide normal map's uv.
            cbs.set_f("vs_height_map_cb", "g_world_dimensions", &[1.0e6, 1.0e6]);

            // ps_texture_array_cb: one layer per texture group, its four variations are the slots 4 * layer + 0..3; g_array_section_sizes.z = 0
            // keeps the index at the tile's own. The shader reads `cb3[mapping + 1]` of the cbuffer, whose row 0 is g_light_params_texture_width:
            // that is array element [mapping] (= the layer here): (metres per repeat, outfield scale, mid-distance strength). Element [layer + 1]
            // left layer 0 with a scale of 0 (uv = world / 0: one flat texel).
            let mut arr = vec![0.0f32; 400 * 4];
            for l in 0..4 {
                arr[l * 4] = m.pixel_scale[l];
                arr[l * 4 + 1] = m.outfield_scale[l];
                arr[l * 4 + 2] = m.mid_strength[l];
            }

            cbs.set_f("ps_texture_array_cb", "g_blend_pixel_scale_array", &arr);
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
            self.upload();
            self.bind(&blend_vs);
            self.bind(&blend_ps);
            null_ps_srvs(&self.ctx, 16);
            let ctx = &self.ctx;
            bind_srv(ctx, &blend_ps, "s_depth_map", self.gbuffer.channels[3].srv.as_ref());
            bind_srv(ctx, &blend_ps, "s_alpha_map", g.alpha.srv.as_ref());
            bind_srv(ctx, &blend_ps, "s_blend_map", g.blend.srv.as_ref());
            bind_srv(ctx, &blend_ps, "s_index_map", g.index.srv.as_ref());
            bind_srv(ctx, &blend_ps, "s_normal_map", g.normal.srv.as_ref());
            bind_srv(ctx, &blend_ps, "s_lf_normal_map", g.lf_normal.srv.as_ref());
            bind_srv(ctx, &blend_ps, "s_light_params", g.light_params.as_ref());
            bind_srv(ctx, &blend_ps, "t_diffuse_array", g.diffuse.srv.as_ref());
            bind_srv(ctx, &blend_ps, "t_normal_array", g.normals.srv.as_ref());
            // The samplers as the blend shader declares them (fn 0x116fd320; filter / address are D3D11 enums): the texture-quality class
            // (vtable 0x11c8bab8, the filter from the texture filtering setting, fn 0x1177f410: anisotropic 2..16 or trilinear) wraps,
            // except s_alpha_map (clamp); s_depth_map and s_light_params are point / clamp; s_normal_array is trilinear / wrap.
            ps_samplers(ctx, &blend_ps, &self.states.s_wrap_aniso);
            bind_sampler(ctx, &blend_ps, "s_alpha_map", self.states.s_clamp_aniso.as_ref());
            bind_sampler(ctx, &blend_ps, "s_depth_map", self.states.s_clamp_point.as_ref());
            bind_sampler(ctx, &blend_ps, "s_light_params", self.states.s_clamp_point.as_ref());
            bind_sampler(ctx, &blend_ps, "s_normal_array", self.states.s_wrap_linear.as_ref());

            let Some(inst_vb) = self.terrain.inst_vb.clone() else { continue };
            for chunk in boxes.chunks(INSTANCE_BYTES / 4 / 16 * 16) {
                if !ctx.map(&inst_vb, 0, D3D11_MAP_WRITE_DISCARD, 0).is_ok_and(|mut ms| ms.write(0, chunk)) {
                    continue;
                }

                ctx.set_vertex_buffers(0, &[self.terrain.box_vb.clone(), Some(inst_vb.clone())], &[12, 64], &[0, 0]);
                ctx.set_index_buffer(self.terrain.box_ib.as_ref(), DXGI_FORMAT_R16_UINT, 0);
                ctx.draw_indexed_instanced(36, (chunk.len() / 16) as u32, 0, 0, 0);
                self.draw_stats.draw_calls += 1;
            }
        }

        // ---- 2b. The terrain-blend meshes' own material (`custom_material`): added to the accumulation with the weight 10 * vertex alpha.
        {
            let (terrain_draws, draws) = (std::mem::take(&mut self.draw_lists.terrain_draws), std::mem::take(&mut self.draw_lists.draws));
            let (vis_terrain, vis) = (std::mem::take(&mut self.draw_lists.vis_terrain), std::mem::take(&mut self.draw_lists.vis));
            let d = blend_mesh::BlendDraw {
                vs: blend_mesh::BLEND_VS, ps: blend_mesh::BLEND_MATERIAL_PS, targets: self.terrain.acc.iter().map(|a| a.rtv.clone()).collect(),
                depth_stencil: self.terrain.ds_equal.clone(), stencil_ref: 0, blend: self.terrain.bs_add.clone(),
            };

            self.draw_blend_meshes([&terrain_draws, &draws], [&vis_terrain, &vis], firsts, &d);
            self.draw_lists.terrain_draws = terrain_draws;
            self.draw_lists.draws = draws;
            self.draw_lists.vis_terrain = vis_terrain;
            self.draw_lists.vis = vis;
        }

        // ---- 3. The resolve: the terrain pixels into the G-buffer.
        let Payload::Terrain(first) = &ents[0].payload else { return };
        let Some(m) = first.material.clone() else { return };
        let g = self.terrain.mat.get(&m.id).cloned().unwrap_or_default();
        let el = [element(c"POSITION", 0, DXGI_FORMAT_R32G32B32A32_FLOAT, 0, 0, false), element(c"TEXCOORD", 0, DXGI_FORMAT_R32G32_FLOAT, 0, 16, false)];
        let Some(il) = self.layout(&res_vs, "terrain_resolve", &el) else { return };
        let rtvs = self.gbuffer.channels.iter().map(|t| t.rtv.clone()).collect::<Vec<_>>();
        self.ctx.set_render_targets(&rtvs, self.gbuffer.dsv.as_ref());
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
        ctx.draw(6, 0);

        self.draw_stats.draw_calls += 1;
        null_ps_srvs(&self.ctx, 16);
        self.ctx.set_rasterizer_state(self.states.rs_back.as_ref());
        self.ctx.set_blend_state(self.states.bs_opaque.as_ref());
    }

    /// Draws the visible patches of every terrain entity, grouped by size, with a pixel shader into one target.
    fn draw_terrain_surface(&mut self, ents: &[&wce_scene::render_scene::Entity], vs: &GameShader, ps: &GameShader, rtv: &Option<ID3D11RenderTargetView>, ds: &Option<ID3D11DepthStencilState>, stencil_ref: u32) {
        self.ctx.set_render_targets(std::slice::from_ref(rtv), self.gbuffer.dsv.as_ref());
        self.ctx.set_depth_stencil_state(ds.as_ref(), stencil_ref);
        let view_proj = self.camera.main_view_proj;
        let triangles = self.draw_terrain_patches(ents, vs, ps, &view_proj);
        self.draw_stats.triangles += triangles;
    }

    /// The terrain entities the material pass draws, with their geometry built and placed.
    fn terrain_surface_entities<'s>(&mut self, scene: &'s RenderScene) -> Vec<&'s wce_scene::render_scene::Entity> {
        let mut ents = vec![];
        for id in self.terrain.entities.clone() {
            let Some(e) = scene.find(id) else { continue };
            let Payload::Terrain(t) = &e.payload else { continue };
            self.build_terrain_geo(id, t);
            let Some(g) = self.terrain.geo.get_mut(&id) else { continue };
            if g.height_srv.is_none() {
                continue;
            }

            g.kz = e.world.m[2][2];
            g.tx = e.world.m[0][3];
            g.ty = e.world.m[1][3];
            g.tz = e.world.m[2][3];
            ents.push(e);
        }

        ents
    }

    /// The terrain into the bound shadow cascade, as the game casts it: the height-map grid with rigidterrain's `vs30_shadow_new` (geom_new's
    /// placement plus a 9 cm lift past 20 m from the camera) and `ps30_shadow` (clip z / w). `view_proj` (the cascade's) culls the patches.
    /// Returns the triangles drawn.
    pub(crate) fn draw_terrain_shadow(&mut self, scene: &RenderScene, view_proj: &Float4x4) -> u64 {
        let (Some(vs), Some(ps)) = (self.shader(SHADOW_NEW_VS), self.shader(SHADOW_TERRAIN_PS)) else {
            return 0;
        };

        let ents = self.terrain_surface_entities(scene);
        let Some(il) = self.layout(&vs, "terrain_shadow", &geom_layout()) else { return 0 };
        if ents.is_empty() {
            return 0;
        }

        self.ctx.set_input_layout(Some(&il));
        self.ctx.set_vertex_buffers(0, std::slice::from_ref(&self.terrain.grid_vb), &[12], &[0]);
        self.draw_terrain_patches(&ents, &vs, &ps, view_proj)
    }

    /// Draws the patches of every terrain entity inside `view_proj`, grouped by size and stride, into the bound targets. The terrains that read
    /// one shared height map share their constants and go out in the same batches. Returns the triangles drawn.
    fn draw_terrain_patches(&mut self, ents: &[&wce_scene::render_scene::Entity], vs: &GameShader, ps: &GameShader, view_proj: &Float4x4) -> u64 {
        // Groups: a shared map's terrains together, the others one by one (in the entities' order).
        let mut groups: Vec<(u64, Vec<TerrainGeo>)> = vec![];
        for (n, e) in ents.iter().enumerate() {
            let Some(g) = self.terrain.geo.get(&e.id).cloned() else { continue };
            let key = if g.shared != 0 { g.shared } else { u64::MAX - n as u64 };
            match groups.iter_mut().find(|(k, _)| *k == key) {
                Some((_, v)) => v.push(g),
                None => groups.push((key, vec![g])),
            }
        }

        let eye = self.terrain.lod_eye;
        let mut triangles = 0;
        for (_, geos) in groups {
            let g0 = &geos[0];

            // vs_height_map_cb: the height texture (scene heights: scaled to game units, no offset), its addressing, the world dimensions (game
            // units); river_CB has nothing to do with it.
            let units = self.camera.units;
            let world_x = g0.tex as f32 * g0.tex_step;
            let world_z = world_x * g0.kz;
            let cbs = &mut self.cbs;
            cbs.set_f("vs_height_map_cb", "g_height_map_lf_scale", &[units]);
            cbs.set_f("vs_height_map_cb", "g_height_map_hf_scale", &[units]);
            cbs.set_f("vs_height_map_cb", "g_height_map_vertical_offset", &[0.0]);
            cbs.set_f("vs_height_map_cb", "g_height_map_dimension", &[g0.tex as f32, g0.tex as f32, 1.0 / g0.tex as f32, 1.0 / g0.tex as f32]);
            cbs.set_f("vs_height_map_cb", "g_world_dimensions", &[world_x * units, world_z * units]);
            cbs.set_f("vs_height_map_cb", "g_lf_uv_offset", &[-(g0.uv0 + g0.tx) / world_x]);
            self.upload();
            self.bind(vs);
            self.bind(ps);
            bind_srv(&self.ctx, vs, "t_low_res_height_map", g0.height_srv.as_ref());

            // The batches: (cx, cz, stride) -> instance rows.
            let mut batches: Vec<(i32, i32, Vec<f32>, u32, i32)> = vec![];
            for g in &geos {
                for p in &g.patches {
                    let wmin = [p.bmin[0] + g.tx, p.bmin[1] + g.ty, g.kz * p.bmin[2] + g.tz];
                    let wmax = [p.bmax[0] + g.tx, p.bmax[1] + g.ty, g.kz * p.bmax[2] + g.tz];
                    if !box_in_frustum(view_proj, wmin, wmax) {
                        continue;
                    }

                    // The stride by distance: one more halving of the vertices per doubling past LOD_STEPS grid steps (2 m steps: every 2nd
                    // vertex from 512 m, every 4th from 1 km ...), a power of two that divides the patch.
                    let k = if g.lod {
                        let (lo, hi) = ([wmin[0].min(wmax[0]), wmin[1], wmin[2].min(wmax[2])], [wmin[0].max(wmax[0]), wmax[1], wmin[2].max(wmax[2])]);
                        let (dx, dy, dz) = (eye.x.clamp(lo[0], hi[0]) - eye.x, eye.y.clamp(lo[1], hi[1]) - eye.y, eye.z.clamp(lo[2], hi[2]) - eye.z);
                        let d = (dx * dx + dy * dy + dz * dz).sqrt() / (LOD_STEPS * g.step);
                        let mut k = 1;
                        while k < PATCH_CELLS && d >= (2 * k) as f32 && p.cx % (2 * k) == 0 && p.cz % (2 * k) == 0 {
                            k *= 2;
                        }

                        k
                    } else {
                        1
                    };

                    let at = match batches.iter().position(|b| b.0 == p.cx && b.1 == p.cz && b.4 == k) {
                        Some(at) => at,
                        None => {
                            batches.push((p.cx, p.cz, vec![], 0, k));
                            batches.len() - 1
                        }
                    };

                    let tx = g.ox + g.tx + (p.pi * PATCH_CELLS) as f32 * g.step;
                    let tz = g.kz * (g.oz + (p.pj * PATCH_CELLS) as f32 * g.step) + g.tz;

                    // World x = gx * step + tx, the height texture's y (+ ty), world z = gz * step * kz + tz; in game units.
                    batches[at].2.extend([g.step, 0.0, 0.0, tx, 0.0, 0.0, 0.0, g.ty, 0.0, 0.0, g.step * g.kz, tz].map(|v| v * units));
                    batches[at].3 += 1;
                }
            }

            for (cx, cz, rows, _, k) in batches {
                let (ib, index_count) = self.terrain_grid_ib(cx, cz, k);
                let Some(inst_vb) = self.terrain.inst_vb.clone() else { continue };

                // As many instances as the buffer holds per draw.
                for chunk in rows.chunks(INSTANCE_BYTES / 4 / 12 * 12) {
                    if !self.ctx.map(&inst_vb, 0, D3D11_MAP_WRITE_DISCARD, 0).is_ok_and(|mut ms| ms.write(0, chunk)) {
                        continue;
                    }

                    let count = (chunk.len() / 12) as u32;
                    self.ctx.set_vertex_buffers(1, &[Some(inst_vb.clone())], &[48], &[0]);
                    self.ctx.set_index_buffer(ib.as_ref(), DXGI_FORMAT_R16_UINT, 0);
                    self.ctx.draw_indexed_instanced(index_count, count, 0, 0, 0);
                    self.draw_stats.draw_calls += 1;
                    triangles += u64::from(index_count / 3) * u64::from(count);
                }
            }
        }

        triangles
    }
}
