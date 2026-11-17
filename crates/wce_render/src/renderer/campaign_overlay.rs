//! A campaign map's overlay (docs/plans/campaign_environment.md, Phase 4; RenderDoc, camp_attila 2026-11-17): the rivers drawn flat
//! (world y 0) into an RGBA16F target with a camera of their own, which the lighting and the sea then read:
//! - the target: 2048 x 1620 at a 1920 x 1080 screen (ASSUMPTION: 1.5 x the screen, at most 2048), cleared to (0, 0, 0, 1); colour
//!   SrcAlpha / InvSrcAlpha, alpha Zero / InvSrcAlpha: rgb the overlay's colour over black, a the transmittance (1: nothing drawn); no
//!   depth;
//! - the camera: the view's rotation, field of view and aspect (the screen's, not the target's), its eye moved back by d (near + d, far
//!   - d). The game's d: 1.012 at an eye 9.29 up, 1.690 at 4.85 (not derived; ASSUMPTION here: the eye's height over the sea times the
//!   cotangent of the view's lowest ray, as far back as the ground the view's bottom edge meets, which fits the closer capture);
//!   ASSUMPTION: back horizontally along the view (both captures look north: world -z and the view's back are the same);
//! - the rivers: `rigidriver.hlsl` vs30 / ps30 `main_river` (the shadowed pixel shader with shadows on), the tiles' river meshes,
//!   their vertices (x, y, z, flow sign) (u, v, end fade, across 0 .. 1); the pixel shader lights and fogs the water, its alpha the
//!   soft edges times the end fade; `s_lf_colour` the map's sea colour, `s_normal` `battleterrain/river/river_normal.dds`;
//! - read by the directional light (`lit * a + rgb * k` where the G-buffer marks terrain, above the sea) and the campaign sea
//!   (`sea * a + rgb`), both through `g_overlay_view_projection`.
//!
//! Not drawn yet: the campaign borders (`campaignborder.hlsl`, the second part of the game's pass).

use std::collections::BTreeMap;

use wce_foundation::math::{Float3, Float4x4, translation};
use wce_foundation::wce_prof;
use wce_scene::camera::Camera;
use wce_scene::render_scene::{EntityId, Payload, RenderScene};

use super::*;
use crate::shader_lib::{bind_sampler, bind_srv};

const OVERLAY_VS: &str = "dx11_sm_3_0_rigidriver.hlsl_vs_vs30_main_river";
const OVERLAY_PS: &str = "dx11_sm_3_0_rigidriver.hlsl_ps_ps30_main_river";
const OVERLAY_PS_SHADOWS: &str = "dx11_enable_shadows_shadows_hardware_gather_5_0_sm_5_0_rigidriver.hlsl_ps_ps30_main_river";
const RIVER_NORMAL: &str = "battleterrain/river/river_normal.dds";
const OVERLAY_MAX: u32 = 2048;

/// The overlay's target, its blend state, the rivers' vertex buffers in the campaign layout, and this frame's camera.
#[derive(Default)]
pub(crate) struct OverlayGpu {
    tex: Option<ID3D11Texture2D>,
    rtv: Option<ID3D11RenderTargetView>,
    pub srv: Option<ID3D11ShaderResourceView>,
    size: (u32, u32),
    bs: Option<ID3D11BlendState>,
    meshes: BTreeMap<EntityId, (u64, Option<ID3D11Buffer>, Option<ID3D11Buffer>, u32)>,

    /// `g_overlay_view_projection` for world positions relative to the render origin (game units); None: no overlay this frame.
    pub view_proj: Option<Float4x4>,
}

impl Renderer {
    /// The overlay of a campaign map with a sea colour map (its frame): its camera from the view's, the rivers into it.
    pub(crate) fn pass_campaign_overlay(&mut self, scene: &RenderScene, cam: &Camera, aspect: f32, screen: (u32, u32), time_s: f32) {
        self.overlay.view_proj = None;
        let Some(sea) = scene.campaign_sea.clone().filter(|_| scene.campaign) else { return };
        wce_prof!("render.overlay");
        let (u, o) = (self.camera.units, self.camera.origin);

        // The camera: back by d, horizontally along the view.
        let fwd = cam.target - cam.eye;
        let flat = Float3::new(fwd.x, 0.0, fwd.z);
        let len = (flat.x * flat.x + flat.z * flat.z).sqrt();
        let pitch = (-fwd.y).atan2(len);
        let low = pitch + cam.fov_y * 0.5;
        let height = (cam.eye.y - o.y).max(0.0);
        let d = if low > 1.0e-3 && low < std::f32::consts::FRAC_PI_2 { (height / low.tan()).min(cam.zfar * 0.25) } else { 0.0 };
        let back = if len > 1.0e-6 { Float3::new(-flat.x / len, 0.0, -flat.z / len) * d } else { Float3::default() };
        let ov = Camera {
            eye: cam.eye + back,
            target: cam.target + back,
            znear: cam.znear + d,
            zfar: (cam.zfar - d).max(cam.znear + d + 1.0),
            ..*cam
        };
        let view = ov.view_from(o);
        let proj = ov.projection(aspect);
        let ovp = game_proj(&proj, u) * game_view(&view, u);
        self.overlay.view_proj = Some(ovp);

        // The target, 1.5 x the screen at most 2048 a side.
        let size = (((screen.0 as f32 * 1.5) as u32).min(OVERLAY_MAX).max(8), ((screen.1 as f32 * 1.5) as u32).min(OVERLAY_MAX).max(8));
        if self.overlay.size != size || self.overlay.tex.is_none() {
            let target = super::targets::make_target(&self.dev, size.0, size.1, DXGI_FORMAT_R16G16B16A16_FLOAT);
            (self.overlay.tex, self.overlay.rtv, self.overlay.srv) = (target.tex, target.rtv, target.srv);
            self.overlay.size = size;
        }

        if self.overlay.bs.is_none() {
            let mut d = D3D11_BLEND_DESC::default();
            d.RenderTarget[0] = D3D11_RENDER_TARGET_BLEND_DESC {
                BlendEnable: true.into(),
                SrcBlend: D3D11_BLEND_SRC_ALPHA,
                DestBlend: D3D11_BLEND_INV_SRC_ALPHA,
                BlendOp: D3D11_BLEND_OP_ADD,
                SrcBlendAlpha: D3D11_BLEND_ZERO,
                DestBlendAlpha: D3D11_BLEND_INV_SRC_ALPHA,
                BlendOpAlpha: D3D11_BLEND_OP_ADD,
                RenderTargetWriteMask: D3D11_COLOR_WRITE_ENABLE_ALL as u8,
            };
            self.overlay.bs = self.dev.create_blend_state(&d).ok();
        }

        if let Some(rtv) = &self.overlay.rtv {
            self.ctx.clear_render_target_view(rtv, [0.0, 0.0, 0.0, 1.0]);
        }

        let rivers: Vec<_> = KindIndex::entities(&self.kinds.water, scene)
            .into_iter()
            .filter(|e| scene.drawable(e) && matches!(&e.payload, Payload::Water(w) if w.river && !w.verts.is_empty()))
            .collect();
        let shadows = self.settings.shadows;
        let vs = self.shader(OVERLAY_VS);
        let ps = if shadows { self.shader(OVERLAY_PS_SHADOWS).or_else(|| self.shader(OVERLAY_PS)) } else { self.shader(OVERLAY_PS) };
        let (Some(vs), Some(ps)) = (vs, ps) else { return };
        let f4 = DXGI_FORMAT_R32G32B32A32_FLOAT;
        let mut el = vec![element(c"POSITION", 0, f4, 0, 0, false), element(c"POSITION", 1, f4, 0, 16, false)];
        for i in 0..4 {
            el.push(element(c"POSITION", 2 + i, f4, 1, i * 16, true));
        }

        let Some(layout) = self.layout(&vs, "campaign_river", &el) else { return };

        // The rivers' meshes in the campaign layout (made again when an entity's version moves on).
        let mut keep = BTreeMap::new();
        for e in &rivers {
            let Payload::Water(w) = &e.payload else { continue };
            let mesh = match self.overlay.meshes.get(&e.id).filter(|m| m.0 == w.version) {
                Some(m) => m.clone(),
                None => {
                    let half = w.verts.iter().map(|v| v.across.abs()).fold(0.0f32, f32::max).max(1.0e-3);
                    let verts: Vec<f32> = w
                        .verts
                        .iter()
                        .flat_map(|v| {
                            let sign = if v.flow < 0.0 { -1.0 } else { 1.0 };
                            [v.x, v.y, v.z, sign, v.u, v.v, v.fade, 0.5 + 0.5 * v.across / half]
                        })
                        .collect();
                    let indices: Vec<u8> = w.indices.iter().flat_map(|i| i.to_le_bytes()).collect();
                    (
                        w.version,
                        immutable_buffer(&self.dev, bytemuck::cast_slice(&verts), D3D11_BIND_VERTEX_BUFFER),
                        immutable_buffer(&self.dev, &indices, D3D11_BIND_INDEX_BUFFER),
                        w.indices.len() as u32,
                    )
                }
            };

            keep.insert(e.id, mesh);
        }

        self.overlay.meshes = keep;
        if rivers.is_empty() {
            return;
        }

        // The rivers in the map corner's frame (the pixel shader reads the colour map at world xz / the map's size).
        let corner = Float3::new(sea.corner[0], o.y, sea.corner[1]);
        let shift = corner - o;
        let inst: [f32; 16] = [u, 0.0, 0.0, -corner.x * u, 0.0, u, 0.0, 0.0, 0.0, 0.0, u, -corner.z * u, 0.0, 0.0, 0.0, 0.0];
        let inst_vb = immutable_buffer(&self.dev, bytemuck::cast_slice(&inst), D3D11_BIND_VERTEX_BUFFER);
        self.set_camera(&view, &proj, ov.eye, size.0, size.1, time_s);
        self.shift_camera_constants(shift);
        let cbs = &mut self.cbs;
        cbs.set_f("river_CB", "g_world_size", &[sea.size[0] * u, sea.size[1] * u]);
        cbs.set_f("river_CB", "g_flow_rate", &[0.02]);
        cbs.set_f("river_CB", "g_vertical_offset", &[0.0]);
        self.upload();

        let srv = |t: Option<crate::model_cache::SharedTexture>| t.and_then(|t| t.read().ok().and_then(|t| t.srv.clone()));
        let normal = srv(self.textures.get(RIVER_NORMAL, false)).or_else(|| self.res.black_srv.clone());
        let colour = srv(self.textures.get(&sea.colour, false)).or_else(|| self.res.white_srv.clone());
        let (env, sky) = (self.env_srv(), self.sky_srv());
        let ctx = &self.ctx;
        ctx.set_render_targets(&[self.overlay.rtv.clone()], None);
        ctx.set_viewports(&[viewport(0.0, 0.0, size.0 as f32, size.1 as f32)]);
        ctx.set_depth_stencil_state(self.states.ds_off.as_ref(), 0);
        ctx.set_blend_state(self.overlay.bs.as_ref());
        ctx.set_rasterizer_state(self.states.rs_none.as_ref());
        ctx.set_input_layout(Some(&layout));
        ctx.set_topology(D3D11_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
        self.bind(&vs);
        self.bind(&ps);
        let ctx = &self.ctx;
        ps_samplers(ctx, &ps, &self.states.s_clamp_linear);
        bind_sampler(ctx, &ps, "s_normal", self.states.s_wrap_linear.as_ref());
        bind_srv(ctx, &ps, "s_normal", normal.as_ref());
        bind_srv(ctx, &ps, "s_lf_colour", colour.as_ref());
        bind_srv(ctx, &ps, "s_environment", env.as_ref());
        bind_srv(ctx, &ps, "s_sky", sky.as_ref());
        bind_srv(ctx, &ps, "g_tShadowBuffer_SM50", self.shadow_map.srv.as_ref());
        bind_sampler(ctx, &ps, "sShadowBuffer_SM50", self.states.s_shadow_cmp.as_ref());
        for e in &rivers {
            let Some((_, vb, ib, count)) = self.overlay.meshes.get(&e.id) else { continue };
            ctx.set_vertex_buffers(0, &[vb.clone(), inst_vb.clone()], &[32, 64], &[0, 0]);
            ctx.set_index_buffer(ib.as_ref(), DXGI_FORMAT_R32_UINT, 0);
            ctx.draw_indexed_instanced(*count, 1, 0, 0, 0);
            self.draw_stats.draw_calls += 1;
        }

        null_ps_srvs(&self.ctx, 16);
        self.ctx.set_render_targets(&[], None);
        self.ctx.set_blend_state(self.states.bs_opaque.as_ref());

        // The main camera back.
        if let Some((view, proj, eye, w, h, time)) = self.camera.last {
            let main_vp = self.camera.main_view_proj;
            self.set_camera(&view, &proj, eye, w, h, time);
            self.camera.main_view_proj = main_vp;
        }

        self.ctx.set_viewports(&[viewport(0.0, 0.0, screen.0 as f32, screen.1 as f32)]);
        self.upload();
    }

    /// `g_overlay_view_projection` for world positions in a frame moved by `shift` from the render origin (scene units), and the
    /// overlay's texture; None: no overlay.
    pub(crate) fn overlay_for(&self, shift: Float3) -> Option<(Float4x4, Option<ID3D11ShaderResourceView>)> {
        let vp = self.overlay.view_proj?;
        Some((vp * translation(shift * self.camera.units), self.overlay.srv.clone()))
    }
}
