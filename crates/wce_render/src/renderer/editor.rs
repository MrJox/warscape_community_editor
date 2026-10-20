//! The editor-side passes of the renderer: the overlay pass (scene overlays, grid, selection outlines) and entity picking. These are not game
//! shaders: a minimal vertex-colour shader and an id shader, compiled from the source below.

use wce_foundation::math::{Float3, Float4x4, look_at_lh, perspective_lh};
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
struct PickOut { float4 pos : SV_Position; nointerpolation uint id : TEXCOORD0; };
PickOut pick_vs(PickIn i) {
    float4 p = float4(i.pos.xyz, 1);
    PickOut o;
    o.pos = mul(vp, float4(dot(i.w0, p), dot(i.w1, p), dot(i.w2, p), 1));
    o.id = i.id;
    return o;
}
uint pick_ps(PickOut i) : SV_Target { return i.id; }
"#;

/// The size of the frame constants: a matrix and the id, padded.
const FRAME_CB: usize = 80;

/// The size of an overlay vertex.
const OVERLAY_VERTEX: usize = 16;

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
    pick_id_tex: Option<ID3D11Texture2D>,
    pick_staging: Option<ID3D11Texture2D>,
    pick_id_rtv: Option<ID3D11RenderTargetView>,
    pick_dsv: Option<ID3D11DepthStencilView>,
    pick_w: u32,
    pick_h: u32,
    pick_inst_vb: Option<ID3D11Buffer>,
    pick_id_vb: Option<ID3D11Buffer>,
    pick_inst_capacity: usize,
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

        e.ready = true;
        if pick_failed {
            self.errors.push("pick input layout failed".to_owned());
        }

        Ok(())
    }

    pub fn view_projection(cam: &Camera, w: u32, h: u32) -> Float4x4 {
        perspective_lh(cam.fov_y, w as f32 / h as f32, cam.znear, cam.zfar) * look_at_lh(cam.eye, cam.target, Float3::new(0.0, 1.0, 0.0))
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
        // selection outline are added per frame.
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
                if !scene.drawable(e) {
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
            let mut sel = Overlay::default();
            let yellow = overlay_colour(1.0, 0.85, 0.2, 1.0);
            for &id in &self.settings.selection {
                let Some(e) = scene.find(id) else { continue };
                if !scene.drawable(e) {
                    continue;
                }

                match &e.payload {
                    Payload::Terrain(_) => {
                        if let Some(b) = self.draw_lists.terrain_bounds.get(&id) {
                            sel.box_wire_transformed(&b.world, b.bmin, b.bmax, yellow);
                        }
                    }

                    Payload::MeshInstance(_) => {
                        if let Some(&item) = self.draw_lists.item_of_entity.get(id as usize).filter(|&&item| item != NO_ITEM)
                            && let Some((w, lo, hi)) = self.draw_lists.draws.item_box(item)
                        {
                            sel.box_wire_transformed(&w, lo, hi, yellow);
                        }
                    }

                    Payload::Overlay(o) => {
                        let (mut lo, mut hi) = (Float3::new(1e30, 1e30, 1e30), Float3::new(-1e30, -1e30, -1e30));
                        for v in o.lines.iter().chain(&o.triangles) {
                            lo = Float3::new(lo.x.min(v.p.x), lo.y.min(v.p.y), lo.z.min(v.p.z));
                            hi = Float3::new(hi.x.max(v.p.x), hi.y.max(v.p.y), hi.z.max(v.p.z));
                        }

                        if lo.x <= hi.x {
                            sel.box_wire_transformed(&e.world, lo, hi, yellow);
                        }
                    }

                    // The light's reach.
                    Payload::PointLight(l) => {
                        let c = Float3::new(e.world.m[0][3], e.world.m[1][3], e.world.m[2][3]);
                        let r = Float3::new(l.radius, l.radius, l.radius);
                        sel.box_wire(c - r, c + r, yellow);
                    }

                    _ => {}
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
            if !o.pickable || !scene.pickable(e) {
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
        if self.editor.pick_w != w || self.editor.pick_h != h {
            let e = &mut self.editor;
            let desc = |w: u32, h: u32, fmt: DXGI_FORMAT, usage: D3D11_USAGE, bind: u32, cpu: u32| D3D11_TEXTURE2D_DESC {
                Width: w, Height: h, MipLevels: 1, ArraySize: 1, Format: fmt, SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 }, Usage: usage, BindFlags: bind,
                CPUAccessFlags: cpu, MiscFlags: 0,
            };

            e.pick_id_tex = self.dev.create_texture_2d(&desc(w, h, DXGI_FORMAT_R32_UINT, D3D11_USAGE_DEFAULT, D3D11_BIND_RENDER_TARGET as u32, 0), None).ok();
            e.pick_id_rtv = e.pick_id_tex.as_ref().and_then(|tex| self.dev.create_render_target_view(tex, None).ok());
            e.pick_dsv = depth_target(&self.dev, w, h, DXGI_FORMAT_D32_FLOAT);
            e.pick_staging = self.dev.create_texture_2d(&desc(1, 1, DXGI_FORMAT_R32_UINT, D3D11_USAGE_STAGING, 0, D3D11_CPU_ACCESS_READ as u32), None).ok();

            e.pick_w = w;
            e.pick_h = h;
        }

        let (Some(rtv), Some(dsv), Some(staging), Some(id_tex)) = (self.editor.pick_id_rtv.clone(), self.editor.pick_dsv.clone(), self.editor.pick_staging.clone(), self.editor.pick_id_tex.clone()) else { return 0 };
        let e = &self.editor;
        // The unsigned integer target is cleared with zero bits.
        self.ctx.clear_render_target_view(&rtv, [0.0; 4]);
        self.ctx.clear_depth_stencil_view(&dsv, D3D11_CLEAR_DEPTH, 1.0, 0);
        self.ctx.set_render_targets(&[Some(rtv)], Some(&dsv));
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
            min_ratio: self.settings.min_pixels * 2.0 * (params.camera.fov_y * 0.5).tan() / h as f32,
            ..Default::default()
        };

        let (draws, terrain_draws) = (std::mem::take(&mut self.draw_lists.draws), std::mem::take(&mut self.draw_lists.terrain_draws));
        for k in 0..2 {
            let set = if k == 0 { &draws } else { &terrain_draws };
            let mut vis = Visible::default();
            set.cull(&view, &mut vis);

            // Locked layers can't be picked (the flag can change without the scene's revision moving).
            set.retain(&mut vis, |id| scene.find(id).is_some_and(|e| scene.pickable(e)));
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
            set.gather(&vis, &mut instances, 1.0);
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
        let pick_box = D3D11_BOX { left: x as u32, top: y as u32, front: 0, right: x as u32 + 1, bottom: y as u32 + 1, back: 1 };
        self.ctx.set_render_targets(&[], None);
        self.ctx.copy_subresource_region(&staging, 0, [0; 3], &id_tex, 0, Some(&pick_box));
        self.ctx.map(&staging, 0, D3D11_MAP_READ, 0).ok().and_then(|ms| ms.read::<u32>()).unwrap_or(0)
    }
}
