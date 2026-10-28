//! The editor-side passes of the renderer: the selection outline, the overlay pass (scene overlays, grid) and entity picking. These are not game
//! shaders: a minimal vertex-colour shader, an id shader and the outline's mask and edge shaders, compiled from the source below.
//!
//! The selection outline: the selected meshes, terrain and overlays (and the overlays that only stand for selected items, `selection_mask`)
//! are drawn into a mask at the target's size, every one in red and, against the scene's depth, the parts in front in green. A full-screen
//! pass then colours the pixels outside the mask within `selection_outline_px` of it: solid where a visible part is near, faint where only
//! hidden parts are.

use wce_foundation::math::{Float3, Float4x4};
use wce_foundation::wce_prof;
use wce_scene::camera::Camera;
use wce_scene::render_scene::{EntityId, Overlay, OverlayVertex, Payload, RenderScene, overlay_colour};

use super::*;
use crate::draw_set::{Frustum, View as DrawView, Visible};
use crate::render_target::RenderTarget;

const EDITOR_HLSL: &str = r#"
cbuffer Frame : register(b0) { row_major float4x4 vp; uint id; };

struct OvlIn  { float3 pos : POSITION; float4 col : COLOR; };
struct OvlOut { float4 pos : SV_Position; float4 col : COLOR; };
OvlOut ovl_vs(OvlIn i) { OvlOut o; o.pos = mul(vp, float4(i.pos, 1)); o.col = i.col; return o; }
float4 ovl_ps(OvlOut i) : SV_Target { return i.col; }

// Same vertex and instance layout as the game's rigid shaders: half4 position, rows of the 3x4 world matrix in POSITION1..3.
struct PickIn { float4 pos : POSITION0; float4 w0 : POSITION1; float4 w1 : POSITION2; float4 w2 : POSITION3; uint id : TEXCOORD0; };
struct PickOut { float4 pos : SV_Position; nointerpolation uint id : TEXCOORD0; float3 world : TEXCOORD1; };
PickOut pick_vs(PickIn i) {
    float4 p = float4(i.pos.xyz, 1);
    PickOut o;
    o.world = float3(dot(i.w0, p), dot(i.w1, p), dot(i.w2, p));
    o.pos = mul(vp, float4(o.world, 1));
    o.id = i.id;
    return o;
}

// The entity id, and the world position of the surface (w 1: something is there) for the placing on surfaces.
struct PickTargets { uint id : SV_Target0; float4 world : SV_Target1; };
PickTargets pick_ps(PickOut i) { PickTargets o; o.id = i.id; o.world = float4(i.world, 1); return o; }

// The selection mask: the blend state's write mask picks the channel (red: the whole selection, green: its parts in front of the scene).
float4 mask_ps() : SV_Target { return float4(1, 1, 0, 0); }

// The tint over chosen meshes (an editor view filter): one translucent colour.
cbuffer Tint : register(b1) { float4 tint_colour; };
float4 tint_ps() : SV_Target { return tint_colour; }

// The outline over the target: a full-screen triangle; a pixel outside the mask within `radius` of it.
cbuffer Outline : register(b1) { float4 colour; float radius; float hidden_alpha; float2 outline_pad; };
Texture2D<float2> mask : register(t0);
float4 outline_vs(uint v : SV_VertexID) : SV_Position {
    float2 uv = float2((v << 1) & 2, v & 2);
    return float4(uv * float2(2, -2) + float2(-1, 1), 0, 1);
}
float4 outline_ps(float4 pos : SV_Position) : SV_Target {
    int2 p = int2(pos.xy);
    if (mask.Load(int3(p, 0)).r > 0) discard;
    int r = (int)ceil(radius);
    float near_any = 0, near_visible = 0;
    for (int y = -r; y <= r; y++) {
        for (int x = -r; x <= r; x++) {
            if (x * x + y * y > radius * radius + 0.5) continue;
            float2 m = mask.Load(int3(p + int2(x, y), 0));
            near_any = max(near_any, m.r);
            near_visible = max(near_visible, m.g);
        }
    }
    if (near_any == 0) discard;
    return float4(colour.rgb, colour.a * (near_visible > 0 ? 1 : hidden_alpha));
}
"#;

/// The outline's constants: the colour, the radius in pixels and the alpha of the hidden parts' outline, padded.
const OUTLINE_CB: usize = 32;

/// How strong the outline of the selection's hidden parts is.
const HIDDEN_ALPHA: f32 = 0.35;

/// The size of the frame constants: a matrix and the id, padded.
const FRAME_CB: usize = 80;

/// The size of an overlay vertex.
const OVERLAY_VERTEX: usize = 16;

/// A surface under a pixel (`Renderer::pick_surface`): its world point, its normal, and the entity it belongs to (0: none known).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SurfaceHit {
    pub point: Float3,
    pub normal: Float3,
    pub entity: EntityId,
}

/// The targets of an id pass: the entity ids, the world positions of the surfaces (w 1 where one is), the depth, and staging copies to read
/// them back (`staging` pixels wide and high).
#[derive(Default)]
pub(crate) struct PickTargets {
    id_tex: Option<ID3D11Texture2D>,
    id_rtv: Option<ID3D11RenderTargetView>,
    id_staging: Option<ID3D11Texture2D>,
    pos_tex: Option<ID3D11Texture2D>,
    pos_rtv: Option<ID3D11RenderTargetView>,
    pos_staging: Option<ID3D11Texture2D>,
    dsv: Option<ID3D11DepthStencilView>,
    w: u32,
    h: u32,
}

impl PickTargets {
    /// Made again when the size changes; `id_staging` / `pos_staging`: the sizes of the read-back copies.
    fn ensure(&mut self, dev: &ID3D11Device, w: u32, h: u32, id_staging: (u32, u32), pos_staging: (u32, u32)) {
        if self.w == w && self.h == h && self.id_tex.is_some() {
            return;
        }

        let desc = |w: u32, h: u32, fmt: DXGI_FORMAT, usage: D3D11_USAGE, bind: u32, cpu: u32| D3D11_TEXTURE2D_DESC {
            Width: w, Height: h, MipLevels: 1, ArraySize: 1, Format: fmt, SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 }, Usage: usage, BindFlags: bind,
            CPUAccessFlags: cpu, MiscFlags: 0,
        };

        self.id_tex = dev.create_texture_2d(&desc(w, h, DXGI_FORMAT_R32_UINT, D3D11_USAGE_DEFAULT, D3D11_BIND_RENDER_TARGET as u32, 0), None).ok();
        self.id_rtv = self.id_tex.as_ref().and_then(|tex| dev.create_render_target_view(tex, None).ok());
        self.dsv = depth_target(dev, w, h, DXGI_FORMAT_D32_FLOAT);
        self.id_staging = dev.create_texture_2d(&desc(id_staging.0, id_staging.1, DXGI_FORMAT_R32_UINT, D3D11_USAGE_STAGING, 0, D3D11_CPU_ACCESS_READ as u32), None).ok();
        self.pos_tex = dev.create_texture_2d(&desc(w, h, DXGI_FORMAT_R32G32B32A32_FLOAT, D3D11_USAGE_DEFAULT, D3D11_BIND_RENDER_TARGET as u32, 0), None).ok();
        self.pos_rtv = self.pos_tex.as_ref().and_then(|tex| dev.create_render_target_view(tex, None).ok());
        self.pos_staging = dev.create_texture_2d(&desc(pos_staging.0, pos_staging.1, DXGI_FORMAT_R32G32B32A32_FLOAT, D3D11_USAGE_STAGING, 0, D3D11_CPU_ACCESS_READ as u32), None).ok();
        self.w = w;
        self.h = h;
    }
}

/// The topmost surfaces over a square of the ground, seen straight down (`Renderer::surfaces_below`): per pixel its world point (w 1 where
/// there is one) and entity, row 0 at the square's +z edge.
#[derive(Clone, Debug, Default)]
pub struct SurfaceGrid {
    pub center: Float3,
    pub size: f32,
    pub res: u32,
    pub points: Vec<[f32; 4]>,
    pub ids: Vec<EntityId>,
}

impl SurfaceGrid {
    fn at(&self, i: i64, j: i64) -> Option<(Float3, EntityId)> {
        let n = i64::from(self.res);
        if i < 0 || j < 0 || i >= n || j >= n {
            return None;
        }

        let k = (j * n + i) as usize;
        let p = self.points.get(k)?;
        (p[3] > 0.5).then(|| (Float3::new(p[0], p[1], p[2]), self.ids.get(k).copied().unwrap_or(0)))
    }

    /// The topmost surface at a ground point: its world point, its normal (from the pixels around it, up-facing; straight up where they
    /// are missing) and its entity. `None` outside the square or where nothing is.
    pub fn hit(&self, x: f32, z: f32) -> Option<SurfaceHit> {
        let res = self.res as f32;
        let i = (((x - self.center.x) / self.size + 0.5) * res).floor() as i64;
        let j = ((0.5 - (z - self.center.z) / self.size) * res).floor() as i64;
        let (point, entity) = self.at(i, j)?;

        // Neighbours on the same thing only (an edge between a roof and the ground is no slope).
        let same = |a: i64, b: i64| self.at(a, b).filter(|(_, id)| *id == entity).map(|(p, _)| p);
        let side = |a: Option<Float3>, b: Option<Float3>| match (a, b) {
            (Some(a), Some(b)) => Some(b - a),
            (Some(a), None) => Some(point - a),
            (None, Some(b)) => Some(b - point),
            (None, None) => None,
        };

        let up = Float3::new(0.0, 1.0, 0.0);
        let normal = match (side(same(i - 1, j), same(i + 1, j)), side(same(i, j + 1), same(i, j - 1))) {
            (Some(dx), Some(dz)) => {
                let n = wce_foundation::math::cross(dz, dx);
                let len = (n.x * n.x + n.y * n.y + n.z * n.z).sqrt();
                if len > 1e-9 && n.y.abs() > 1e-6 {
                    let s = if n.y < 0.0 { -1.0 } else { 1.0 } / len;
                    Float3::new(n.x * s, n.y * s, n.z * s)
                } else {
                    up
                }
            }

            _ => up,
        };

        Some(SurfaceHit { point, normal, entity })
    }
}

/// The editor passes' GPU objects, created on first use.
#[derive(Default)]
pub(crate) struct EditorGpu {
    ready: bool,
    ovl_vs: Option<ID3D11VertexShader>,
    pick_vs: Option<ID3D11VertexShader>,
    ovl_ps: Option<ID3D11PixelShader>,
    pick_ps: Option<ID3D11PixelShader>,
    ovl_il: Option<ID3D11InputLayout>,
    pick_il: Option<ID3D11InputLayout>,
    ovl_cb: Option<ID3D11Buffer>,
    ovl_vb: Option<ID3D11Buffer>,
    pick_cb: Option<ID3D11Buffer>,
    ovl_vb_capacity: usize,

    /// The scene's overlays transformed to world space, per batch (triangles / lines with the depth test, then without): rebuilt only when the
    /// scene changed.
    ovl_cache: [Vec<OverlayVertex>; 4],
    ovl_cache_scene: usize,
    ovl_cache_rev: u64,
    ovl_blend: Option<ID3D11BlendState>,
    ovl_ds_test: Option<ID3D11DepthStencilState>,
    ovl_ds_off: Option<ID3D11DepthStencilState>,
    pick_ds: Option<ID3D11DepthStencilState>,
    ovl_rs: Option<ID3D11RasterizerState>,
    /// The id pass's targets: the view's (`pick`, `pick_surface`) and the straight-down one (`surfaces_below`), each kept at its size.
    pick_view: PickTargets,
    pick_top: PickTargets,
    pick_inst_vb: Option<ID3D11Buffer>,
    pick_id_vb: Option<ID3D11Buffer>,
    pick_inst_capacity: usize,

    // The tint over chosen meshes.
    tint_ps: Option<ID3D11PixelShader>,
    tint_cb: Option<ID3D11Buffer>,
    tint_inst_vb: Option<ID3D11Buffer>,
    tint_inst_capacity: usize,

    // The selection outline.
    mask_ps: Option<ID3D11PixelShader>,
    outline_vs: Option<ID3D11VertexShader>,
    outline_ps: Option<ID3D11PixelShader>,
    outline_cb: Option<ID3D11Buffer>,
    mask_bs_all: Option<ID3D11BlendState>,
    mask_bs_visible: Option<ID3D11BlendState>,

    /// The visible parts' draw, pulled towards the camera so that it does not fight the scene's own depth.
    mask_rs_biased: Option<ID3D11RasterizerState>,
    mask_tex: Option<ID3D11Texture2D>,
    mask_rtv: Option<ID3D11RenderTargetView>,
    mask_srv: Option<ID3D11ShaderResourceView>,
    mask_w: u32,
    mask_h: u32,
    mask_inst_vb: Option<ID3D11Buffer>,
    mask_inst_capacity: usize,
    mask_vb: Option<ID3D11Buffer>,
    mask_vb_capacity: usize,
}

fn overlay_bytes(v: &[OverlayVertex]) -> Vec<u8> {
    v.iter().flat_map(|v| [v.p.x.to_le_bytes(), v.p.y.to_le_bytes(), v.p.z.to_le_bytes(), v.rgba.to_le_bytes()]).flatten().collect()
}

fn frame_cb(vp: &Float4x4) -> [u8; FRAME_CB] {
    let mut cb = [0u8; FRAME_CB];
    cb[..64].copy_from_slice(&f32_bytes(vp.data()));
    cb
}

/// Writes data into a dynamic buffer; false when it can't be mapped or the data doesn't fit.
fn write_dynamic<T: bytemuck::NoUninit>(ctx: &ID3D11DeviceContext, buffer: &ID3D11Buffer, data: &[T]) -> bool {
    ctx.map(buffer, 0, D3D11_MAP_WRITE_DISCARD, 0).is_ok_and(|mut ms| ms.write(0, data))
}

impl Renderer {

    fn create_editor_resources(&mut self) -> Result<(), RenderError> {
        if self.editor.ready {
            return Ok(());
        }

        let dev = self.dev.clone();
        let e = &mut self.editor;
        let vs = compile(EDITOR_HLSL, "editor_passes", "ovl_vs", "vs_5_0")?;
        let ps = compile(EDITOR_HLSL, "editor_passes", "ovl_ps", "ps_5_0")?;
        let pvs = compile(EDITOR_HLSL, "editor_passes", "pick_vs", "vs_5_0")?;
        let pps = compile(EDITOR_HLSL, "editor_passes", "pick_ps", "ps_5_0")?;
        let mps = compile(EDITOR_HLSL, "editor_passes", "mask_ps", "ps_5_0")?;
        let tps = compile(EDITOR_HLSL, "editor_passes", "tint_ps", "ps_5_0")?;
        let ovs = compile(EDITOR_HLSL, "editor_passes", "outline_vs", "vs_5_0")?;
        let ops = compile(EDITOR_HLSL, "editor_passes", "outline_ps", "ps_5_0")?;
        let oel = [element(c"POSITION", 0, DXGI_FORMAT_R32G32B32_FLOAT, 0, 0, false), element(c"COLOR", 0, DXGI_FORMAT_R8G8B8A8_UNORM, 0, 12, false)];
        let mut pel = vec![element(c"POSITION", 0, DXGI_FORMAT_R16G16B16A16_FLOAT, 0, 0, false)];
        for i in 1..=3 {
            pel.push(element(c"POSITION", i, DXGI_FORMAT_R32G32B32A32_FLOAT, 1, (i - 1) * 16, true));
        }

        // The entity id of every instance.
        pel.push(element(c"TEXCOORD", 0, DXGI_FORMAT_R32_UINT, 2, 0, true));
        e.ovl_vs = dev.create_vertex_shader(&vs).ok();
        e.ovl_il = dev.create_input_layout(&oel, &vs).ok();
        e.ovl_ps = dev.create_pixel_shader(&ps).ok();
        e.pick_vs = dev.create_vertex_shader(&pvs).ok();
        e.pick_il = dev.create_input_layout(&pel, &pvs).ok();
        let pick_failed = e.pick_il.is_none();
        e.pick_ps = dev.create_pixel_shader(&pps).ok();
        e.mask_ps = dev.create_pixel_shader(&mps).ok();
        e.tint_ps = dev.create_pixel_shader(&tps).ok();
        e.tint_cb = dynamic_buffer(&dev, 16, D3D11_BIND_CONSTANT_BUFFER);
        e.outline_vs = dev.create_vertex_shader(&ovs).ok();
        e.outline_ps = dev.create_pixel_shader(&ops).ok();
        e.outline_cb = dynamic_buffer(&dev, OUTLINE_CB, D3D11_BIND_CONSTANT_BUFFER);
        let channel = |mask: u8| crate::particles::blend_state(&dev, D3D11_RENDER_TARGET_BLEND_DESC {
            BlendEnable: false.into(), SrcBlend: D3D11_BLEND_ONE, DestBlend: D3D11_BLEND_ZERO, BlendOp: D3D11_BLEND_OP_ADD, SrcBlendAlpha: D3D11_BLEND_ONE,
            DestBlendAlpha: D3D11_BLEND_ZERO, BlendOpAlpha: D3D11_BLEND_OP_ADD, RenderTargetWriteMask: mask,
        });
        e.mask_bs_all = channel(D3D11_COLOR_WRITE_ENABLE_RED as u8);
        e.mask_bs_visible = channel(D3D11_COLOR_WRITE_ENABLE_GREEN as u8);

        e.ovl_cb = dynamic_buffer(&dev, FRAME_CB, D3D11_BIND_CONSTANT_BUFFER);
        e.pick_cb = dynamic_buffer(&dev, FRAME_CB, D3D11_BIND_CONSTANT_BUFFER);
        e.ovl_blend = crate::particles::blend_state(&dev, D3D11_RENDER_TARGET_BLEND_DESC {
            BlendEnable: true.into(), SrcBlend: D3D11_BLEND_SRC_ALPHA, DestBlend: D3D11_BLEND_INV_SRC_ALPHA, BlendOp: D3D11_BLEND_OP_ADD,
            SrcBlendAlpha: D3D11_BLEND_ZERO, DestBlendAlpha: D3D11_BLEND_ONE, BlendOpAlpha: D3D11_BLEND_OP_ADD, RenderTargetWriteMask: D3D11_COLOR_WRITE_ENABLE_ALL as u8,
        });

        let mut dd = D3D11_DEPTH_STENCIL_DESC { DepthEnable: true.into(), DepthWriteMask: D3D11_DEPTH_WRITE_MASK_ZERO, DepthFunc: D3D11_COMPARISON_LESS_EQUAL, ..Default::default() };
        let rd = D3D11_RASTERIZER_DESC { FillMode: D3D11_FILL_SOLID, CullMode: D3D11_CULL_NONE, DepthClipEnable: true.into(), ..Default::default() };
        e.ovl_ds_test = dev.create_depth_stencil_state(&dd).ok();
        dd.DepthEnable = false.into();
        e.ovl_ds_off = dev.create_depth_stencil_state(&dd).ok();
        dd.DepthEnable = true.into();
        dd.DepthWriteMask = D3D11_DEPTH_WRITE_MASK_ALL;
        dd.DepthFunc = D3D11_COMPARISON_LESS;
        e.pick_ds = dev.create_depth_stencil_state(&dd).ok();
        e.ovl_rs = dev.create_rasterizer_state(&rd).ok();
        e.mask_rs_biased = dev.create_rasterizer_state(&D3D11_RASTERIZER_DESC { DepthBias: -2000, SlopeScaledDepthBias: -2.0, ..rd }).ok();

        e.ready = true;
        if pick_failed {
            self.errors.push("pick input layout failed".to_owned());
        }

        Ok(())
    }

    pub fn view_projection(cam: &Camera, w: u32, h: u32) -> Float4x4 {
        cam.view_projection(w as f32 / h as f32)
    }

    /// A world point to a pixel of a w x h view; `None` when the point is behind the camera.
    pub fn project(cam: &Camera, w: u32, h: u32, p: Float3) -> Option<(f32, f32)> {
        let m = Self::view_projection(cam, w, h);
        let cw = m.m[3][0] * p.x + m.m[3][1] * p.y + m.m[3][2] * p.z + m.m[3][3];
        if cw <= 1e-6 {
            return None;
        }

        let cx = (m.m[0][0] * p.x + m.m[0][1] * p.y + m.m[0][2] * p.z + m.m[0][3]) / cw;
        let cy = (m.m[1][0] * p.x + m.m[1][1] * p.y + m.m[1][2] * p.z + m.m[1][3]) / cw;
        Some(((cx * 0.5 + 0.5) * w as f32, (0.5 - cy * 0.5) * h as f32))
    }

    pub(crate) fn pass_overlay(&mut self, scene: &RenderScene, target: &RenderTarget) {
        if let Err(error) = self.create_editor_resources() {
            self.error(error.0);
            return;
        }

        // Four batches: lines / triangles, with and without the depth test. The scene's overlays are cached between frames; the grid and the
        // selected lights' reach are added per frame.
        let scene_id = scene as *const RenderScene as usize;
        if self.editor.ovl_cache_scene != scene_id || self.editor.ovl_cache_rev != scene.revision() {
            wce_prof!("render.overlay_rebuild");
            self.editor.ovl_cache_scene = scene_id;
            self.editor.ovl_cache_rev = scene.revision();
            for v in &mut self.editor.ovl_cache {
                v.clear();
            }

            for e in scene.entities() {
                let Payload::Overlay(o) = &e.payload else { continue };
                if !scene.drawable(e) || o.selection_mask {
                    continue;
                }

                // Triangles 0 / 2 and lines 1 / 3, with and without the depth test.
                let (tris, lines) = if o.depth_test { (0, 1) } else { (2, 3) };
                self.editor.ovl_cache[lines].extend(o.lines.iter().map(|v| OverlayVertex { p: e.world.transform_point(v.p), rgba: v.rgba }));
                self.editor.ovl_cache[tris].extend(o.triangles.iter().map(|v| OverlayVertex { p: e.world.transform_point(v.p), rgba: v.rgba }));
            }
        }

        // This frame's extras.
        let (mut lines_d, mut lines_n) = (vec![], vec![]);
        if self.settings.show_grid {
            let n = (self.settings.grid_half_extent / self.settings.grid_step) as i32;
            let g = self.settings.grid_half_extent;
            for i in -n..=n {
                let t = i as f32 * self.settings.grid_step;
                let c = if i == 0 { overlay_colour(0.9, 0.3, 0.3, 0.8) } else { overlay_colour(0.7, 0.7, 0.7, 0.25) };
                let c2 = if i == 0 { overlay_colour(0.3, 0.4, 0.95, 0.8) } else { overlay_colour(0.7, 0.7, 0.7, 0.25) };
                lines_d.extend([
                    OverlayVertex { p: Float3::new(-g, 0.0, t), rgba: c },
                    OverlayVertex { p: Float3::new(g, 0.0, t), rgba: c },
                    OverlayVertex { p: Float3::new(t, 0.0, -g), rgba: c2 },
                    OverlayVertex { p: Float3::new(t, 0.0, g), rgba: c2 },
                ]);
            }
        }

        if !self.settings.selection.is_empty() {
            // The outline (`pass_selection_outline`) shows the rest of the selection.
            let mut sel = Overlay::default();
            let [r, g, b, a] = self.settings.selection_colour;
            let yellow = overlay_colour(r, g, b, a);
            for &id in &self.settings.selection {
                // The light's reach: a circle of its radius in each axis plane.
                if let Some(e) = scene.find(id).filter(|e| scene.drawable(e))
                    && let Payload::PointLight(l) = &e.payload
                {
                    let c = Float3::new(e.world.m[0][3], e.world.m[1][3], e.world.m[2][3]);
                    let r = l.radius;
                    let at = |a: f32, plane: usize| {
                        let (u, v) = (r * a.cos(), r * a.sin());
                        match plane {
                            0 => Float3::new(c.x + u, c.y, c.z + v),
                            1 => Float3::new(c.x + u, c.y + v, c.z),
                            _ => Float3::new(c.x, c.y + u, c.z + v),
                        }
                    };

                    const SEGMENTS: usize = 64;
                    for plane in 0..3 {
                        for k in 0..SEGMENTS {
                            let (a0, a1) = (k as f32 / SEGMENTS as f32 * std::f32::consts::TAU, (k + 1) as f32 / SEGMENTS as f32 * std::f32::consts::TAU);
                            sel.line(at(a0, plane), at(a1, plane), yellow);
                        }
                    }
                }
            }

            lines_n.extend(sel.lines);
        }

        let extra: [&[OverlayVertex]; 4] = [&[], &lines_d, &[], &lines_n];
        let total = (0..4).map(|b| self.editor.ovl_cache[b].len() + extra[b].len()).sum::<usize>();
        if total == 0 {
            return;
        }

        if self.editor.ovl_vb_capacity < total {
            self.editor.ovl_vb_capacity = (total * 2).max(4096);
            self.editor.ovl_vb = dynamic_buffer(&self.dev, self.editor.ovl_vb_capacity * OVERLAY_VERTEX, D3D11_BIND_VERTEX_BUFFER);
        }

        let (mut first, mut count) = ([0u32; 4], [0u32; 4]);
        let mut bytes = Vec::with_capacity(total * OVERLAY_VERTEX);
        for b in 0..4 {
            first[b] = (bytes.len() / OVERLAY_VERTEX) as u32;
            bytes.extend(overlay_bytes(&self.editor.ovl_cache[b]));
            bytes.extend(overlay_bytes(extra[b]));
            count[b] = (bytes.len() / OVERLAY_VERTEX) as u32 - first[b];
        }

        let (Some(vb), Some(cb)) = (self.editor.ovl_vb.clone(), self.editor.ovl_cb.clone()) else { return };
        if !write_dynamic(&self.ctx, &vb, &bytes) {
            return;
        }

        write_dynamic(&self.ctx, &cb, &frame_cb(&self.camera.main_view_proj));
        let e = &self.editor;
        let topology = [D3D11_PRIMITIVE_TOPOLOGY_TRIANGLELIST, D3D11_PRIMITIVE_TOPOLOGY_LINELIST, D3D11_PRIMITIVE_TOPOLOGY_TRIANGLELIST, D3D11_PRIMITIVE_TOPOLOGY_LINELIST];
        self.ctx.set_render_targets(&[target.rtv().cloned()], self.gbuffer.dsv.as_ref());
        self.ctx.set_viewports(&[viewport(0.0, 0.0, target.width() as f32, target.height() as f32)]);
        self.ctx.set_blend_state(e.ovl_blend.as_ref());
        self.ctx.set_rasterizer_state(e.ovl_rs.as_ref());
        self.ctx.set_input_layout(e.ovl_il.as_ref());
        self.ctx.set_vertex_buffers(0, &[Some(vb)], &[OVERLAY_VERTEX as u32], &[0]);
        self.ctx.vs_set_shader(e.ovl_vs.as_ref());
        self.ctx.vs_set_constant_buffers(0, &[Some(cb)]);
        self.ctx.ps_set_shader(e.ovl_ps.as_ref());
        for b in 0..4 {
            if count[b] == 0 {
                continue;
            }

            self.ctx.set_depth_stencil_state(if b < 2 { e.ovl_ds_test.as_ref() } else { e.ovl_ds_off.as_ref() }, 0);
            self.ctx.set_topology(topology[b]);
            self.ctx.draw(count[b], first[b]);
        }

        self.ctx.set_blend_state(self.states.bs_opaque.as_ref());
        self.ctx.set_render_targets(&[], None);
    }

    /// The selection outline (see the module): the mask of the selected meshes, terrain and overlays, then its edge over the target.
    /// The tint (an editor view filter, `settings.tinted`): those mesh entities drawn again over the target in `settings.tint_colour`,
    /// against the scene's depth (pulled towards the camera like the selection mask's visible parts), so only their visible surfaces take
    /// it. The game's shaders drew them first; this only lays a translucent colour on top.
    pub(crate) fn pass_tint(&mut self, scene: &RenderScene, target: &RenderTarget) {
        if self.settings.tinted.is_empty() || self.settings.tint_colour[3] <= 0.0 {
            return;
        }

        if let Err(error) = self.create_editor_resources() {
            self.error(error.0);
            return;
        }

        let tinted = self.settings.tinted.iter().copied().filter(|&id| scene.find(id).is_some_and(|e| scene.drawable(e) && matches!(e.payload, Payload::MeshInstance(_))))
            .collect::<std::collections::HashSet<_>>();
        if tinted.is_empty() {
            return;
        }

        let (Some(cb), Some(tcb)) = (self.editor.pick_cb.clone(), self.editor.tint_cb.clone()) else { return };
        let vp_m = self.camera.main_view_proj;
        let view = DrawView { frustum: Frustum::from_view_projection(&vp_m), eye: self.camera.eye, lod_scale: self.settings.lod_scale, ..Default::default() };
        let draws = std::mem::take(&mut self.draw_lists.draws);
        let mut vis = Visible::default();
        draws.cull(&view, &mut vis);
        draws.retain(&mut vis, |id| tinted.contains(&id));
        let n = vis.instances();
        if n == 0 {
            self.draw_lists.draws = draws;
            return;
        }

        let mut instances = vec![RigidInstance::default(); n];
        draws.gather(&vis, &mut instances, 1.0, [0.0; 3]);
        if self.editor.tint_inst_capacity < n {
            self.editor.tint_inst_capacity = (n * 2).max(1024);
            self.editor.tint_inst_vb = dynamic_buffer(&self.dev, self.editor.tint_inst_capacity * size_of::<RigidInstance>(), D3D11_BIND_VERTEX_BUFFER);
        }

        let Some(vb) = self.editor.tint_inst_vb.clone().filter(|vb| write_dynamic(&self.ctx, vb, &instances)) else {
            self.draw_lists.draws = draws;
            return;
        };

        write_dynamic(&self.ctx, &cb, &frame_cb(&vp_m));
        write_dynamic(&self.ctx, &tcb, &self.settings.tint_colour);
        let e = &self.editor;
        let (w, h) = (target.width(), target.height());
        self.ctx.set_render_targets(&[target.rtv().cloned()], self.gbuffer.dsv.as_ref());
        self.ctx.set_viewports(&[viewport(0.0, 0.0, w as f32, h as f32)]);
        self.ctx.set_blend_state(e.ovl_blend.as_ref());
        self.ctx.set_depth_stencil_state(e.ovl_ds_test.as_ref(), 0);
        self.ctx.set_rasterizer_state(e.mask_rs_biased.as_ref());
        self.ctx.set_input_layout(e.pick_il.as_ref());
        self.ctx.vs_set_shader(e.pick_vs.as_ref());
        self.ctx.vs_set_constant_buffers(0, std::slice::from_ref(&e.pick_cb));
        self.ctx.ps_set_shader(e.tint_ps.as_ref());
        self.ctx.ps_set_constant_buffers(1, &[Some(tcb)]);
        self.ctx.set_topology(D3D11_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
        let stride = size_of::<RigidInstance>() as u32;
        self.ctx.set_vertex_buffers(1, &[Some(vb.clone()), Some(vb)], &[stride, stride], &[0, 0]);
        for &b in &vis.active {
            let m = draws.batch_mesh(b);
            let offset = if m.stride == crate::model_cache::TREE_STRIDE { 8 } else { 0 };
            self.ctx.set_vertex_buffers(0, std::slice::from_ref(&m.vb), &[m.stride], &[offset]);
            self.ctx.set_index_buffer(m.ib.as_ref(), DXGI_FORMAT_R16_UINT, 0);
            self.ctx.draw_indexed_instanced(m.index_count, vis.count[b as usize], 0, 0, vis.start[b as usize]);
        }

        self.draw_lists.draws = draws;
        self.ctx.set_blend_state(self.states.bs_opaque.as_ref());
        self.ctx.set_render_targets(&[], None);
    }

    pub(crate) fn pass_selection_outline(&mut self, scene: &RenderScene, target: &RenderTarget) {
        if let Err(error) = self.create_editor_resources() {
            self.error(error.0);
            return;
        }

        // What goes into the mask: the selected entities (meshes and terrain through their draw sets, overlays as triangles and lines) and the
        // overlays that stand for selected items.
        let selected = self.settings.selection.iter().copied().filter(|&id| scene.find(id).is_some_and(|e| scene.drawable(e))).collect::<std::collections::HashSet<_>>();
        let (mut tris, mut lines) = (vec![], vec![]);
        for e in scene.entities() {
            let Payload::Overlay(o) = &e.payload else { continue };
            if (o.selection_mask || selected.contains(&e.id)) && scene.drawable(e) {
                tris.extend(o.triangles.iter().map(|v| OverlayVertex { p: e.world.transform_point(v.p), rgba: v.rgba }));
                lines.extend(o.lines.iter().map(|v| OverlayVertex { p: e.world.transform_point(v.p), rgba: v.rgba }));
            }
        }

        let meshes = selected.iter().any(|&id| scene.find(id).is_some_and(|e| matches!(e.payload, Payload::MeshInstance(_) | Payload::Terrain(_))));
        if !meshes && tris.is_empty() && lines.is_empty() {
            return;
        }

        let (w, h) = (target.width(), target.height());
        if self.editor.mask_w != w || self.editor.mask_h != h {
            let desc = D3D11_TEXTURE2D_DESC {
                Width: w, Height: h, MipLevels: 1, ArraySize: 1, Format: DXGI_FORMAT_R8G8_UNORM, SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
                Usage: D3D11_USAGE_DEFAULT, BindFlags: (D3D11_BIND_RENDER_TARGET | D3D11_BIND_SHADER_RESOURCE) as u32, CPUAccessFlags: 0, MiscFlags: 0,
            };

            let e = &mut self.editor;
            e.mask_tex = self.dev.create_texture_2d(&desc, None).ok();
            e.mask_rtv = e.mask_tex.as_ref().and_then(|tex| self.dev.create_render_target_view(tex, None).ok());
            e.mask_srv = e.mask_tex.as_ref().and_then(|tex| self.dev.create_shader_resource_view(tex, None).ok());
            (e.mask_w, e.mask_h) = (w, h);
        }

        let (Some(rtv), Some(srv), Some(cb), Some(ocb)) = (self.editor.mask_rtv.clone(), self.editor.mask_srv.clone(), self.editor.pick_cb.clone(), self.editor.outline_cb.clone()) else { return };

        // The selected items of both draw sets, culled and gathered once for the two draws.
        let vp_m = self.camera.main_view_proj;
        let view = DrawView { frustum: Frustum::from_view_projection(&vp_m), eye: self.camera.eye, lod_scale: self.settings.lod_scale, ..Default::default() };
        let (draws, terrain_draws) = (std::mem::take(&mut self.draw_lists.draws), std::mem::take(&mut self.draw_lists.terrain_draws));
        // Per set: its visible list and where its instances start in `instances`.
        let mut sets: Vec<(&crate::draw_set::DrawSet, Visible, u32)> = vec![];
        let mut instances: Vec<RigidInstance> = vec![];
        if meshes {
            for set in [&draws, &terrain_draws] {
                let mut vis = Visible::default();
                set.cull(&view, &mut vis);
                set.retain(&mut vis, |id| selected.contains(&id));
                let n = vis.instances();
                if n != 0 {
                    let first = instances.len();
                    instances.resize(first + n, RigidInstance::default());
                    set.gather(&vis, &mut instances[first..], 1.0, [0.0; 3]);
                    sets.push((set, vis, first as u32));
                }
            }
        }

        // Uploads: the gathered instances, the overlays' triangles then lines.
        let n_tris = tris.len() as u32;
        let n_lines = lines.len() as u32;
        let ovl_bytes = [overlay_bytes(&tris), overlay_bytes(&lines)].concat();
        if self.editor.mask_inst_capacity < instances.len() {
            self.editor.mask_inst_capacity = (instances.len() * 2).max(1024);
            self.editor.mask_inst_vb = dynamic_buffer(&self.dev, self.editor.mask_inst_capacity * size_of::<RigidInstance>(), D3D11_BIND_VERTEX_BUFFER);
        }

        if self.editor.mask_vb_capacity < ovl_bytes.len() {
            self.editor.mask_vb_capacity = (ovl_bytes.len() * 2).max(4096 * OVERLAY_VERTEX);
            self.editor.mask_vb = dynamic_buffer(&self.dev, self.editor.mask_vb_capacity, D3D11_BIND_VERTEX_BUFFER);
        }

        let inst_vb = self.editor.mask_inst_vb.clone().filter(|vb| instances.is_empty() || write_dynamic(&self.ctx, vb, &instances));
        let ovl_vb = self.editor.mask_vb.clone().filter(|vb| ovl_bytes.is_empty() || write_dynamic(&self.ctx, vb, &ovl_bytes));
        write_dynamic(&self.ctx, &cb, &frame_cb(&vp_m));

        // The mask: everything in red without the depth test, then the parts in front of the scene in green.
        self.ctx.clear_render_target_view(&rtv, [0.0; 4]);
        self.ctx.set_render_targets(&[Some(rtv)], self.gbuffer.dsv.as_ref());
        self.ctx.set_viewports(&[viewport(0.0, 0.0, w as f32, h as f32)]);
        let e = &self.editor;
        for visible in [false, true] {
            self.ctx.set_blend_state(if visible { e.mask_bs_visible.as_ref() } else { e.mask_bs_all.as_ref() });
            self.ctx.set_depth_stencil_state(if visible { e.ovl_ds_test.as_ref() } else { e.ovl_ds_off.as_ref() }, 0);
            self.ctx.set_rasterizer_state(if visible { e.mask_rs_biased.as_ref() } else { e.ovl_rs.as_ref() });
            self.ctx.ps_set_shader(e.mask_ps.as_ref());
            if let Some(vb) = inst_vb.clone().filter(|_| !sets.is_empty()) {
                self.ctx.set_input_layout(e.pick_il.as_ref());
                self.ctx.vs_set_shader(e.pick_vs.as_ref());
                self.ctx.vs_set_constant_buffers(0, std::slice::from_ref(&e.pick_cb));
                self.ctx.set_topology(D3D11_PRIMITIVE_TOPOLOGY_TRIANGLELIST);

                // The layout's id stream (slot 2) is not read by the mask: the instances stand in for it.
                let stride = size_of::<RigidInstance>() as u32;
                self.ctx.set_vertex_buffers(1, &[Some(vb.clone()), Some(vb)], &[stride, stride], &[0, 0]);
                for (set, vis, first) in &sets {
                    for &b in &vis.active {
                        let m = set.batch_mesh(b);
                        let offset = if m.stride == crate::model_cache::TREE_STRIDE { 8 } else { 0 };
                        self.ctx.set_vertex_buffers(0, std::slice::from_ref(&m.vb), &[m.stride], &[offset]);
                        self.ctx.set_index_buffer(m.ib.as_ref(), DXGI_FORMAT_R16_UINT, 0);
                        self.ctx.draw_indexed_instanced(m.index_count, vis.count[b as usize], 0, 0, first + vis.start[b as usize]);
                    }
                }
            }

            if let Some(vb) = ovl_vb.clone().filter(|_| n_tris + n_lines != 0) {
                self.ctx.set_input_layout(e.ovl_il.as_ref());
                self.ctx.vs_set_shader(e.ovl_vs.as_ref());
                self.ctx.vs_set_constant_buffers(0, std::slice::from_ref(&e.pick_cb));
                self.ctx.set_vertex_buffers(0, &[Some(vb)], &[OVERLAY_VERTEX as u32], &[0]);
                if n_tris != 0 {
                    self.ctx.set_topology(D3D11_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
                    self.ctx.draw(n_tris, 0);
                }

                if n_lines != 0 {
                    self.ctx.set_topology(D3D11_PRIMITIVE_TOPOLOGY_LINELIST);
                    self.ctx.draw(n_lines, n_tris);
                }
            }
        }

        drop(sets);
        self.draw_lists.draws = draws;
        self.draw_lists.terrain_draws = terrain_draws;

        // The edge over the target.
        let mut consts = [0f32; OUTLINE_CB / 4];
        consts[..4].copy_from_slice(&self.settings.selection_colour);
        consts[4] = self.settings.selection_outline_px.max(1.0);
        consts[5] = HIDDEN_ALPHA;
        write_dynamic(&self.ctx, &ocb, &consts);
        let e = &self.editor;
        self.ctx.set_render_targets(&[target.rtv().cloned()], None);
        self.ctx.set_blend_state(e.ovl_blend.as_ref());
        self.ctx.set_depth_stencil_state(e.ovl_ds_off.as_ref(), 0);
        self.ctx.set_rasterizer_state(e.ovl_rs.as_ref());
        self.ctx.set_input_layout(None);
        self.ctx.set_topology(D3D11_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
        self.ctx.vs_set_shader(e.outline_vs.as_ref());
        self.ctx.ps_set_shader(e.outline_ps.as_ref());
        self.ctx.ps_set_constant_buffers(1, &[Some(ocb)]);
        self.ctx.ps_set_shader_resources(0, &[Some(srv)]);
        self.ctx.draw(3, 0);
        self.ctx.ps_set_shader_resources(0, &[None]);
        self.ctx.set_blend_state(self.states.bs_opaque.as_ref());
        self.ctx.set_render_targets(&[], None);
    }

    /// The entity under pixel (x, y) of `target` as `params` sees the scene; 0 for none. Mesh and terrain entities are found with an id pass of
    /// their own (the game's G-buffer id target is fed by an engine-private lookup table); overlay entities are picked in screen space (within 6
    /// px of a line, inside a triangle) and win over meshes. Layers marked locked are skipped.
    pub fn pick(&mut self, scene: &RenderScene, params: &ViewParams, target: &RenderTarget, x: i32, y: i32) -> EntityId {
        let (w, h) = (target.width(), target.height());
        if x < 0 || y < 0 || x >= w as i32 || y >= h as i32 {
            return 0;
        }

        if let Err(error) = self.create_editor_resources() {
            self.error(error.0);
            return 0;
        }

        self.build_draws(scene, params.camera.eye);
        let vp_m = Self::view_projection(&params.camera, w, h);

        // 1. Overlay entities in screen space: the nearest line within 6 px, or a triangle that contains the pixel.
        let mut overlay_hit = 0;
        let mut best = 6.0f32;
        let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
        for e in scene.entities() {
            let Payload::Overlay(o) = &e.payload else { continue };
            if !o.pickable || !scene.pickable(e) || self.settings.game_view {
                continue;
            }

            let screen = |v: &OverlayVertex| Self::project(&params.camera, w, h, e.world.transform_point(v.p));
            for pair in o.lines.as_chunks::<2>().0 {
                let (Some((ax, ay)), Some((bx, by))) = (screen(&pair[0]), screen(&pair[1])) else { continue };
                let (dx, dy) = (bx - ax, by - ay);
                let len2 = dx * dx + dy * dy;
                let t = if len2 > 0.0 { ((fx - ax) * dx + (fy - ay) * dy) / len2 } else { 0.0 }.clamp(0.0, 1.0);
                let d = (fx - (ax + t * dx)).hypot(fy - (ay + t * dy));
                if d < best {
                    best = d;
                    overlay_hit = e.id;
                }
            }

            for tri in o.triangles.as_chunks::<3>().0 {
                let (Some(p0), Some(p1), Some(p2)) = (screen(&tri[0]), screen(&tri[1]), screen(&tri[2])) else { continue };
                let (px, py) = ([p0.0, p1.0, p2.0], [p0.1, p1.1, p2.1]);
                let edge = |a: usize, b: usize| (fx - px[a]) * (py[b] - py[a]) - (fy - py[a]) * (px[b] - px[a]);
                let (e0, e1, e2) = (edge(0, 1), edge(1, 2), edge(2, 0));
                if (e0 >= 0.0 && e1 >= 0.0 && e2 >= 0.0) || (e0 <= 0.0 && e1 <= 0.0 && e2 <= 0.0) {
                    best = 0.0;
                    overlay_hit = e.id;
                }
            }
        }

        if overlay_hit != 0 {
            return overlay_hit;
        }

        // 2. Meshes and terrain: an id pass at the target's size, read back at one pixel.
        if !self.id_pass(scene, params, &vp_m, (w, h), &|_| false, false) {
            return 0;
        }

        let (Some(staging), Some(id_tex)) = (self.editor.pick_view.id_staging.clone(), self.editor.pick_view.id_tex.clone()) else { return 0 };
        let pick_box = D3D11_BOX { left: x as u32, top: y as u32, front: 0, right: x as u32 + 1, bottom: y as u32 + 1, back: 1 };
        self.ctx.copy_subresource_region(&staging, 0, [0; 3], &id_tex, 0, Some(&pick_box));
        self.ctx.map(&staging, 0, D3D11_MAP_READ, 0).ok().and_then(|ms| ms.read::<u32>()).unwrap_or(0)
    }

    /// The id pass of the meshes and the terrain into the pick targets (ids, world positions), at the target's size; `skip`: entities left out.
    /// `top`: the straight-down targets (`surfaces_below`, every LOD drawn as the camera sees it, nothing too small to draw left out), else
    /// the view's. False when the targets can't be made.
    fn id_pass(&mut self, scene: &RenderScene, params: &ViewParams, vp_m: &Float4x4, (w, h): (u32, u32), skip: &dyn Fn(EntityId) -> bool, top: bool) -> bool {
        let vp_m = *vp_m;
        let dev = self.dev.clone();
        if top {
            self.editor.pick_top.ensure(&dev, w, h, (w, h), (w, h));
        } else {
            self.editor.pick_view.ensure(&dev, w, h, (1, 1), (3, 3));
        }

        let t = if top { &self.editor.pick_top } else { &self.editor.pick_view };
        let (Some(rtv), Some(pos_rtv), Some(dsv)) = (t.id_rtv.clone(), t.pos_rtv.clone(), t.dsv.clone()) else { return false };
        let e = &self.editor;
        // The unsigned integer target is cleared with zero bits; the positions with w 0 (nothing there).
        self.ctx.clear_render_target_view(&rtv, [0.0; 4]);
        self.ctx.clear_render_target_view(&pos_rtv, [0.0; 4]);
        self.ctx.clear_depth_stencil_view(&dsv, D3D11_CLEAR_DEPTH, 1.0, 0);
        self.ctx.set_render_targets(&[Some(rtv), Some(pos_rtv)], Some(&dsv));
        self.ctx.set_viewports(&[viewport(0.0, 0.0, w as f32, h as f32)]);
        self.ctx.set_depth_stencil_state(e.pick_ds.as_ref(), 0);
        self.ctx.set_blend_state(self.states.bs_opaque.as_ref());
        self.ctx.set_rasterizer_state(e.ovl_rs.as_ref());
        self.ctx.set_input_layout(e.pick_il.as_ref());
        self.ctx.set_topology(D3D11_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
        self.ctx.vs_set_shader(e.pick_vs.as_ref());
        self.ctx.vs_set_constant_buffers(0, std::slice::from_ref(&e.pick_cb));

        // The pixel shader writes `id`.
        self.ctx.ps_set_constant_buffers(0, std::slice::from_ref(&e.pick_cb));
        self.ctx.ps_set_shader(e.pick_ps.as_ref());

        let view = DrawView {
            frustum: Frustum::from_view_projection(&vp_m),
            eye: params.camera.eye,
            lod_scale: self.settings.lod_scale,
            min_ratio: if top || params.camera.is_ortho() { 0.0 } else { self.settings.min_pixels * 2.0 * (params.camera.fov_y * 0.5).tan() / h as f32 },
            ..Default::default()
        };

        let (draws, terrain_draws) = (std::mem::take(&mut self.draw_lists.draws), std::mem::take(&mut self.draw_lists.terrain_draws));
        for k in 0..2 {
            let set = if k == 0 { &draws } else { &terrain_draws };
            let mut vis = Visible::default();
            set.cull(&view, &mut vis);

            // Locked layers can't be picked (the flag can change without the scene's revision moving); nor what the caller skips.
            set.retain(&mut vis, |id| !skip(id) && scene.find(id).is_some_and(|e| scene.pickable(e)));
            let n = vis.instances();
            if n == 0 {
                continue;
            }

            if self.editor.pick_inst_capacity < n {
                self.editor.pick_inst_capacity = (n * 2).max(4096);
                self.editor.pick_inst_vb = dynamic_buffer(&self.dev, self.editor.pick_inst_capacity * size_of::<RigidInstance>(), D3D11_BIND_VERTEX_BUFFER);
                self.editor.pick_id_vb = dynamic_buffer(&self.dev, self.editor.pick_inst_capacity * 4, D3D11_BIND_VERTEX_BUFFER);
            }

            let (Some(inst_vb), Some(id_vb), Some(cb)) = (self.editor.pick_inst_vb.clone(), self.editor.pick_id_vb.clone(), self.editor.pick_cb.clone()) else { continue };
            let mut instances = vec![RigidInstance::default(); n];
            set.gather(&vis, &mut instances, 1.0, [0.0; 3]);
            if !write_dynamic(&self.ctx, &inst_vb, &instances) || !write_dynamic(&self.ctx, &id_vb, &set.gather_ids(&vis)) {
                continue;
            }

            write_dynamic(&self.ctx, &cb, &frame_cb(&vp_m));
            self.ctx.set_vertex_buffers(1, &[Some(inst_vb), Some(id_vb)], &[size_of::<RigidInstance>() as u32, 4], &[0, 0]);
            for &b in &vis.active {
                let m = set.batch_mesh(b);
                // A tree's LOD position (bytes 8..16) is the one drawn at LOD 0 (its weights are 0, 1): picked as POSITION0.
                let offset = if m.stride == crate::model_cache::TREE_STRIDE { 8 } else { 0 };
                self.ctx.set_vertex_buffers(0, std::slice::from_ref(&m.vb), &[m.stride], &[offset]);
                self.ctx.set_index_buffer(m.ib.as_ref(), DXGI_FORMAT_R16_UINT, 0);
                self.ctx.draw_indexed_instanced(m.index_count, vis.count[b as usize], 0, 0, vis.start[b as usize]);
            }
        }

        self.draw_lists.draws = draws;
        self.draw_lists.terrain_draws = terrain_draws;
        self.ctx.set_render_targets(&[], None);
        true
    }

    /// The surface under pixel (x, y): the nearest mesh or terrain (not the overlays, not the locked layers, not what `skip` says), its world
    /// point, its normal (from the points of the pixels around it, towards the camera; straight up where they are missing) and its entity.
    /// `None` where nothing is.
    pub fn pick_surface(&mut self, scene: &RenderScene, params: &ViewParams, target: &RenderTarget, x: i32, y: i32, skip: &dyn Fn(EntityId) -> bool) -> Option<SurfaceHit> {
        let (w, h) = (target.width(), target.height());
        if x < 1 || y < 1 || x + 1 >= w as i32 || y + 1 >= h as i32 {
            return None;
        }

        if let Err(error) = self.create_editor_resources() {
            self.error(error.0);
            return None;
        }

        self.build_draws(scene, params.camera.eye);
        let vp_m = Self::view_projection(&params.camera, w, h);
        if !self.id_pass(scene, params, &vp_m, (w, h), skip, false) {
            return None;
        }

        let t = &self.editor.pick_view;
        let (Some(staging), Some(id_staging), Some(pos_tex), Some(id_tex)) = (t.pos_staging.clone(), t.id_staging.clone(), t.pos_tex.clone(), t.id_tex.clone()) else {
            return None;
        };

        let around = D3D11_BOX { left: x as u32 - 1, top: y as u32 - 1, front: 0, right: x as u32 + 2, bottom: y as u32 + 2, back: 1 };
        let one = D3D11_BOX { left: x as u32, top: y as u32, front: 0, right: x as u32 + 1, bottom: y as u32 + 1, back: 1 };
        self.ctx.copy_subresource_region(&staging, 0, [0; 3], &pos_tex, 0, Some(&around));
        self.ctx.copy_subresource_region(&id_staging, 0, [0; 3], &id_tex, 0, Some(&one));
        let entity = self.ctx.map(&id_staging, 0, D3D11_MAP_READ, 0).ok().and_then(|ms| ms.read::<u32>()).unwrap_or(0);
        let ms = self.ctx.map(&staging, 0, D3D11_MAP_READ, 0).ok()?;
        let at = |i: usize, j: usize| -> Option<Float3> {
            let row = ms.row(j, 48)?;
            let f = |k: usize| f32::from_le_bytes([row[i * 16 + k * 4], row[i * 16 + k * 4 + 1], row[i * 16 + k * 4 + 2], row[i * 16 + k * 4 + 3]]);
            (f(3) > 0.5).then(|| Float3::new(f(0), f(1), f(2)))
        };

        let point = at(1, 1)?;
        let side = |a: Option<Float3>, b: Option<Float3>| match (a, b) {
            (Some(a), Some(b)) => Some(b - a),
            (Some(a), None) => Some(point - a),
            (None, Some(b)) => Some(b - point),
            (None, None) => None,
        };

        let up = Float3::new(0.0, 1.0, 0.0);
        let normal = match (side(at(0, 1), at(2, 1)), side(at(1, 0), at(1, 2))) {
            (Some(dx), Some(dy)) => {
                let n = wce_foundation::math::cross(dx, dy);
                let len = (n.x * n.x + n.y * n.y + n.z * n.z).sqrt();
                if len > 1e-9 {
                    let n = Float3::new(n.x / len, n.y / len, n.z / len);
                    let to_eye = params.camera.eye - point;
                    if n.x * to_eye.x + n.y * to_eye.y + n.z * to_eye.z < 0.0 { Float3::new(-n.x, -n.y, -n.z) } else { n }
                } else {
                    up
                }
            }

            _ => up,
        };

        Some(SurfaceHit { point, normal, entity })
    }

    /// The topmost surfaces (meshes and terrain; not the overlays, the locked layers, what `skip` says) over a `size` metres square around
    /// `center`, seen straight down at `res` x `res` pixels, the models at the LODs the camera of `params` sees. For putting things on
    /// whatever is below a ground point, wherever the camera is. `None` when the pass can't be drawn.
    pub fn surfaces_below(&mut self, scene: &RenderScene, params: &ViewParams, center: Float3, size: f32, res: u32, skip: &dyn Fn(EntityId) -> bool) -> Option<SurfaceGrid> {
        if let Err(error) = self.create_editor_resources() {
            self.error(error.0);
            return None;
        }

        // From well above the ground, looking down with +z up on the image.
        const ABOVE: f32 = 1000.0;
        let eye = Float3::new(center.x, center.y + ABOVE, center.z);
        let view = wce_foundation::math::look_at_lh(eye, center, Float3::new(0.0, 0.0, 1.0));
        let vp_m = wce_foundation::math::ortho_lh(size, size, 1.0, ABOVE * 2.0) * view;
        self.build_draws(scene, params.camera.eye);
        if !self.id_pass(scene, params, &vp_m, (res, res), skip, true) {
            return None;
        }

        let t = &self.editor.pick_top;
        let (Some(pos_staging), Some(id_staging), Some(pos_tex), Some(id_tex)) = (t.pos_staging.clone(), t.id_staging.clone(), t.pos_tex.clone(), t.id_tex.clone()) else {
            return None;
        };

        self.ctx.copy_resource(&pos_staging, &pos_tex);
        self.ctx.copy_resource(&id_staging, &id_tex);
        let n = res as usize;
        let mut grid = SurfaceGrid { center, size, res, points: Vec::with_capacity(n * n), ids: Vec::with_capacity(n * n) };
        {
            let ms = self.ctx.map(&id_staging, 0, D3D11_MAP_READ, 0).ok()?;
            for j in 0..n {
                let row = ms.row(j, n * 4)?;
                grid.ids.extend(row.as_chunks::<4>().0.iter().map(|b| u32::from_le_bytes(*b)));
            }
        }

        let ms = self.ctx.map(&pos_staging, 0, D3D11_MAP_READ, 0).ok()?;
        for j in 0..n {
            let row = ms.row(j, n * 16)?;
            grid.points.extend(row.as_chunks::<16>().0.iter().map(|b| {
                let f = |k: usize| f32::from_le_bytes([b[k * 4], b[k * 4 + 1], b[k * 4 + 2], b[k * 4 + 3]]);
                [f(0), f(1), f(2), f(3)]
            }));
        }

        Some(grid)
    }
}
