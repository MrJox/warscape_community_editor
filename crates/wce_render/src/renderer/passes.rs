//! The renderer's passes.

use std::f32::consts::PI;
use std::rc::Rc;

use wce_foundation::math::{Float3, Float4x4, inverse, length, look_at_lh, ortho_lh, perspective_lh, translation};
use wce_foundation::wce_prof;
use wce_scene::camera::Camera;
use wce_scene::clouds::cloud_texture;
use wce_scene::environment::{EnvironmentParams, build_scurve, colour_to_linear};

use super::*;
use crate::draw_set::{DrawSet, Frustum, View as DrawView, Visible};
use crate::model_cache::{half_to_float, TEX_SLOTS};
use crate::render_target::RenderTarget;
use crate::shader_lib::{bind_sampler, bind_srv};
use crate::texture::load_dds;

/// Battle rows of the split table (dll+0x1c88aa0), indexed by gfx_shadow_quality.
const SHADOW_ROWS: [[f32; 4]; 4] = [[60.0, 180.0, 180.0, 180.0], [30.0, 90.0, 210.0, 210.0], [30.0, 90.0, 210.0, 450.0], [30.0, 90.0, 210.0, 450.0]];

/// The atlas quadrants of the four cascades.
const SHADOW_BOUNDS: [[f32; 4]; 4] = [[0.0, 0.0, 0.5, 0.5], [0.5, 0.0, 1.0, 0.5], [0.0, 0.5, 0.5, 1.0], [0.5, 0.5, 1.0, 1.0]];

// The lens flare (LENSE_FLARE_SHADER, fn 0x116de600): four sun sprites and a vignette, drawn after hdr_to_screen.
const FLARE_VS: [&str; 4] = ["dx11_sm_3_0_lenseflare.hlsl_vs_vs30_flare_0", "dx11_sm_3_0_lenseflare.hlsl_vs_vs30_flare_1", "dx11_sm_3_0_lenseflare.hlsl_vs_vs30_flare_2", "dx11_sm_3_0_lenseflare.hlsl_vs_vs30_flare_3"];
const FLARE_PS: [&str; 4] = ["dx11_sm_3_0_lenseflare.hlsl_ps_ps30_flare_0", "dx11_sm_3_0_lenseflare.hlsl_ps_ps30_flare_1", "dx11_sm_3_0_lenseflare.hlsl_ps_ps30_flare_2", "dx11_sm_3_0_lenseflare.hlsl_ps_ps30_flare_3"];
const FLARE_MAIN_VS: &str = "dx11_sm_3_0_lenseflare.hlsl_vs_vs30_main";
const VIGNETTE_PS: &str = "dx11_sm_3_0_lenseflare.hlsl_ps_ps30_vignette";

impl Renderer {

    pub(crate) fn pass_sky_cube(&mut self, _env: &EnvironmentParams, time_s: f32) {
        let vs = self.shader(SKY_VS);
        let ps = self.shader(SKY_GEN_PS);
        let (Some(vs), Some(ps)) = (vs, ps) else { return };
        let proj = perspective_lh(PI / 2.0, 1.0, 0.1, 100.0);
        let Some(layout) = self.layout(&vs, "pos3", &sky_layout()) else { return };
        // SAFETY: drawing calls with objects of this device.
        unsafe {
            self.ctx.RSSetViewports(Some(&[viewport(0.0, 0.0, SKY_CUBE as f32, SKY_CUBE as f32)]));
            self.ctx.RSSetState(self.rs_none.as_ref());
            self.ctx.OMSetDepthStencilState(self.ds_off.as_ref(), 0);
            self.ctx.OMSetBlendState(self.bs_opaque.as_ref(), None, 0xFFFF_FFFF);
            self.ctx.IASetInputLayout(&layout);
            self.ctx.IASetVertexBuffers(0, 1, Some(&self.cube_vb), Some(&12), Some(&0));
            self.ctx.IASetIndexBuffer(self.cube_ib.as_ref(), DXGI_FORMAT_R16_UINT, 0);
            self.ctx.IASetPrimitiveTopology(D3D11_PRIMITIVE_TOPOLOGY(D3D11_PRIMITIVE_TOPOLOGY_TRIANGLELIST));
        }

        for f in 0..6 {
            self.set_camera(&face_view(f), &proj, Float3::default(), SKY_CUBE, SKY_CUBE, time_s);
            self.upload();
            // SAFETY: drawing calls with objects of this device.
            unsafe { self.ctx.OMSetRenderTargets(Some(&[self.sky_face_rtv[f].clone()]), None) };
            self.bind(&vs);
            self.bind(&ps);
            // SAFETY: a drawing call.
            unsafe { self.ctx.DrawIndexed(36, 0, 0) };
        }

        // SAFETY: state calls.
        unsafe {
            self.ctx.OMSetRenderTargets(None, None);
            if let Some(srv) = &self.sky_cube_srv {
                self.ctx.GenerateMips(srv);
            }
        }
    }

    /// EnvMapGen (fn 0x118e00e0): six 90-degree views from (0, ground + 50, 0), far plane 972800, through the normal scene passes; then every
    /// face's lowest mip is read back (readback_ambient). While it renders, the lighting uses the XML ambient (no computed cube exists yet in
    /// the engine at that point, fn 0x178a300) and no shadows (measured: binding the main-camera shadow map changes the faces by < 1%).
    pub(crate) fn render_env_cube(&mut self, env: &EnvironmentParams, time_s: f32) {
        let n = SKY_CUBE;
        std::mem::swap(&mut self.gb, &mut self.env_gb);
        std::mem::swap(&mut self.hdr, &mut self.env_hdr);
        std::mem::swap(&mut self.dsv, &mut self.env_dsv);
        let (save_w, save_h) = (self.w, self.h);
        let save_ao = self.ao_result.clone();
        let save_dt = self.cloud_dt;
        self.w = n;
        self.h = n;
        self.ao_result = self.white_srv.clone();
        self.cloud_dt = 0.0;
        self.env_pass = true;
        self.cbs.set_f("shadowmap_PS", "g_iSplitCount", &[0.0]);
        let proj = perspective_lh(PI / 2.0, 1.0, 1.0, 972_800.0);

        // The test map's ground is at 0.
        let eye = Float3::new(0.0, 50.0, 0.0);
        for f in 0..6 {
            let view = face_view(f) * translation(-eye);
            self.set_camera(&view, &proj, eye, n, n, time_s);
            self.upload();
            // SAFETY: state calls.
            unsafe {
                self.ctx.RSSetViewports(Some(&[viewport(0.0, 0.0, n as f32, n as f32)]));
                self.ctx.OMSetBlendState(self.bs_opaque.as_ref(), None, 0xFFFF_FFFF);
            }

            // No sun disc: with it the +x face of the dry file is 10x the game's value; without it the side faces match the dump within 10-25%.
            self.pass_gbuffer();
            // SAFETY: a clear of a view of this device.
            unsafe {
                if let Some(rtv) = &self.hdr.rtv {
                    self.ctx.ClearRenderTargetView(rtv, &[0.0; 4]);
                }
            }

            self.pass_sky_forward();

            // Only the top (+y) face receives clouds: the game's side faces equal the cloud-free scene within 10-15%, while clouds on every face put
            // them 100% off. Mechanism inferred: fn 0x1192aa40 sets per-cloud visibility flags from a frustum during EnvMapGen (fn 0x1192b640),
            // which here leaves only the zenith clouds.
            if f == 2 {
                self.pass_clouds(env);
            }

            self.pass_directional_light();
            self.pass_fog();
            // SAFETY: a copy between textures of this device.
            unsafe {
                self.ctx.OMSetRenderTargets(None, None);
                if let (Some(cube), Some(hdr)) = (&self.env_cube, &self.hdr.tex) {
                    self.ctx.CopySubresourceRegion(cube, f as u32 * 9, 0, 0, 0, hdr, 0, None);
                }
            }
        }

        // SAFETY: a state call.
        unsafe {
            if let Some(srv) = &self.env_cube_srv {
                self.ctx.GenerateMips(srv);
            }
        }

        self.env_valid = true;
        self.cbs.set_f("shadowmap_PS", "g_iSplitCount", &[if self.shadows { 4.0 } else { 0.0 }]);
        self.env_pass = false;
        self.cloud_dt = save_dt;
        self.ao_result = save_ao;
        self.w = save_w;
        self.h = save_h;
        std::mem::swap(&mut self.gb, &mut self.env_gb);
        std::mem::swap(&mut self.hdr, &mut self.env_hdr);
        std::mem::swap(&mut self.dsv, &mut self.env_dsv);
        null_ps_srvs(&self.ctx, 16);
        self.readback_ambient();
    }

    /// The face averages of the environment cube: copies each face's 1x1 mip into this frame's staging texture and reads the previous one.
    fn readback_ambient(&mut self) {
        // 256 -> 1.
        let mips = 9;
        let (Some(dst), Some(src), Some(cube)) = (self.amb_staging[(self.amb_frame & 1) as usize].clone(), self.amb_staging[((self.amb_frame + 1) & 1) as usize].clone(), self.env_cube.clone()) else { return };
        // SAFETY: copies between textures of this device; mapped subresources are read within their size and unmapped.
        unsafe {
            for f in 0..6 {
                self.ctx.CopySubresourceRegion(&dst, f, 0, 0, 0, &cube, (mips - 1) + f * mips, None);
            }

            let first = self.amb_frame == 0;
            self.amb_frame += 1;
            if first {
                return;
            }

            for f in 0..6u32 {
                let Some(ms) = map_read(&self.ctx, &src, f, self.deterministic) else { return };
                let h = std::slice::from_raw_parts(ms.pData.cast::<u16>(), 3);
                for (face, &half) in self.amb_faces[f as usize].iter_mut().zip(h) {
                    *face = half_to_float(half);
                }

                self.ctx.Unmap(&src, f);
            }
        }

        self.amb_valid = true;
    }

    /// Verified (EnvMapGen fn 0x118e00e0, ambient apply fn 0x1178a300, dry dump): with use_custom_ambient_cube_colours = false the cube is the
    /// per-face average (lowest mip) of the environment cube rendered from 50 m above the origin, the bottom face replaced by 0.35 * luma(top) *
    /// (0.71, 0.69, 0.66) (fn 0x118f0980, 0.35 = engine variable), times ambient_scale; lr = (+x, -x), tb = (+y, bottom), fb = (+z, -z).
    pub(crate) fn apply_engine_ambient(&mut self, env: &EnvironmentParams) {
        if env.use_custom_ambient || !self.amb_valid {
            return;
        }

        let mut faces = self.amb_faces;
        let top = faces[2];
        let luma = 0.2126 * top[0] + 0.7152 * top[1] + 0.0722 * top[2];
        let tint = [0.71f32, 0.69, 0.66];
        for c in 0..3 {
            faces[3][c] = 0.35 * tint[c] * luma;
        }

        let s = env.ambient_scale;
        for (var, a, b) in [("ambient_cube_lr", 0, 1), ("ambient_cube_tb", 2, 3), ("ambient_cube_fb", 4, 5)] {
            self.cbs.set_f("lighting_VS_PS", var, &[faces[a][0] * s, faces[a][1] * s, faces[a][2] * s, 0.0, faces[b][0] * s, faces[b][1] * s, faces[b][2] * s]);
        }
    }

    /// Draws the visible batches of both sets with the bound shaders (the G-buffer and the shadow cascades share this).
    fn draw_batches(&mut self, sets: [&DrawSet; 2], viss: [&Visible; 2], firsts: [u32; 2], mut each: impl FnMut(&mut Self, &crate::model_cache::GpuMesh) -> bool) -> (u64, u64) {
        let (mut instances, mut triangles) = (0u64, 0u64);
        for k in 0..2 {
            let (set, vis) = (sets[k], viss[k]);
            for &b in &vis.active {
                let m = set.batch_mesh(b);
                if !each(self, m) {
                    continue;
                }

                // SAFETY: drawing calls with buffers of this device.
                unsafe {
                    self.ctx.IASetVertexBuffers(0, 1, Some(&m.vb), Some(&32), Some(&0));
                    self.ctx.IASetIndexBuffer(m.ib.as_ref(), DXGI_FORMAT_R16_UINT, 0);
                    self.ctx.DrawIndexedInstanced(m.index_count, vis.count[b as usize], 0, 0, firsts[k] + vis.start[b as usize]);
                }

                self.draw_stats.draw_calls += 1;
                let count = u64::from(vis.count[b as usize]);
                instances += count;
                triangles += u64::from(m.index_count / 3) * count;
            }
        }

        (instances, triangles)
    }

    pub(crate) fn pass_gbuffer(&mut self) {
        let rtvs = self.gb.iter().map(|t| t.rtv.clone()).collect::<Vec<_>>();
        // SAFETY: clears and state calls with objects of this device.
        unsafe {
            for rtv in rtvs.iter().flatten() {
                self.ctx.ClearRenderTargetView(rtv, &[0.0; 4]);
            }

            if let Some(dsv) = &self.dsv {
                self.ctx.ClearDepthStencilView(dsv, (D3D11_CLEAR_DEPTH | D3D11_CLEAR_STENCIL) as u32, 1.0, 0);
            }

            self.ctx.OMSetRenderTargets(Some(&rtvs), self.dsv.as_ref());
            self.ctx.OMSetDepthStencilState(self.ds_gbuffer.as_ref(), 1);
            self.ctx.RSSetState(self.rs_back.as_ref());
        }

        if self.draws.items() == 0 && self.terrain_draws.items() == 0 {
            return;
        }

        let vs = self.shader(RIGID_VS);
        let ps_opaque = self.shader(RIGID_GBUFFER_PS);
        let ps_alpha = self.shader(RIGID_GBUFFER_ALPHA_PS);
        let (Some(vs), Some(ps_opaque)) = (vs, ps_opaque) else { return };
        let Some(layout) = self.layout(&vs, "rigid_std", &rigid_layout()) else { return };
        // SAFETY: state calls with objects of this device.
        unsafe {
            self.ctx.IASetInputLayout(&layout);
            self.ctx.IASetPrimitiveTopology(D3D11_PRIMITIVE_TOPOLOGY(D3D11_PRIMITIVE_TOPOLOGY_TRIANGLELIST));
        }

        self.bind(&vs);
        for &slot in vs.textures.values() {
            // SAFETY: a binding call.
            unsafe { self.ctx.VSSetShaderResources(slot, Some(std::slice::from_ref(&self.instance_ids_srv))) };
        }

        // The ground first, as the entity order had it (equal depths: the first one drawn wins).
        let view = self.camera_view(if self.env_pass { 0.5 } else { 1.0 });
        let (terrain_draws, draws) = (std::mem::take(&mut self.terrain_draws), std::mem::take(&mut self.draws));
        let (mut vis_terrain, mut vis) = (std::mem::take(&mut self.vis_terrain), std::mem::take(&mut self.vis));
        terrain_draws.cull(&view, &mut vis_terrain);
        draws.cull(&view, &mut vis);

        // The ground is drawn by the terrain pass (game shaders), not as flat rigid patches.
        if self.terrain.active && !self.env_pass {
            vis_terrain = Visible::default();
        }

        let firsts = self.upload_instances([&terrain_draws, &draws], [&vis_terrain, &vis]);
        for (k, v) in [&vis_terrain, &vis].into_iter().enumerate() {
            self.draw_stats.views += 1;
            self.draw_stats.instances += v.instances() as u64;
            if k == 0 && !self.env_pass {
                self.draw_stats.terrain_patches = v.instances() as u64;
            }
        }

        // SAFETY: a binding call.
        unsafe { self.ctx.IASetVertexBuffers(1, 1, Some(&self.instance_ring), Some(&(size_of::<RigidInstance>() as u32)), Some(&0)) };

        wce_prof!("render.gbuffer.draw");

        // Opaque meshes, then alpha-tested ones.
        let has_alpha = ps_alpha.is_some();
        for pass in 0..2 {
            let Some(ps) = (if pass == 0 { Some(ps_opaque.clone()) } else { ps_alpha.clone() }) else { continue };
            if !self.env_pass {
                self.gpu_timer.mark(if pass == 0 { "gbuffer opaque" } else { "gbuffer alpha-tested" });
            }

            let slots = self.slots_of(&ps);
            let mut bound = false;
            let (instances, triangles) = self.draw_batches([&terrain_draws, &draws], [&vis_terrain, &vis], firsts, |r, m| {
                if i32::from(m.alpha_mode == 1 && has_alpha) != pass {
                    return false;
                }

                if !bound {
                    r.bind(&ps);
                    bind_srv(&r.ctx, &ps, "s_fire_map", r.black_srv.as_ref());
                    bind_srv(&r.ctx, &ps, "s_snow_normals", r.black_srv.as_ref());
                    ps_samplers(&r.ctx, &ps, &r.s_wrap_aniso);
                    bound = true;
                }

                for (t, slot) in slots.iter().enumerate().take(TEX_SLOTS) {
                    let Some(slot) = slot else { continue };
                    let srv = m.tex[t].read().ok().and_then(|tex| tex.srv.clone());
                    // SAFETY: a binding call.
                    unsafe { r.ctx.PSSetShaderResources(*slot, Some(&[srv])) };
                }

                true
            });

            self.draw_stats.triangles += triangles;
            if !self.env_pass {
                self.draw_stats.camera_triangles += triangles;
                self.draw_stats.camera_instances += instances;
            }
        }

        self.terrain_draws = terrain_draws;
        self.draws = draws;
        self.vis_terrain = vis_terrain;
        self.vis = vis;
    }

    pub(crate) fn pass_sky_forward(&mut self) {
        let vs = self.shader(SKY_VS);
        let ps = self.shader(SKY_PS);
        let (Some(vs), Some(ps)) = (vs, ps) else { return };
        let Some(layout) = self.layout(&vs, "pos3", &sky_layout()) else { return };
        // SAFETY: drawing calls with objects of this device.
        unsafe {
            self.ctx.OMSetRenderTargets(Some(std::slice::from_ref(&self.hdr.rtv)), self.dsv.as_ref());
            self.ctx.OMSetDepthStencilState(self.ds_sky.as_ref(), 0);
            self.ctx.RSSetState(self.rs_none.as_ref());
            self.ctx.IASetInputLayout(&layout);
            self.ctx.IASetVertexBuffers(0, 1, Some(&self.cube_vb), Some(&12), Some(&0));
            self.ctx.IASetIndexBuffer(self.cube_ib.as_ref(), DXGI_FORMAT_R16_UINT, 0);
            self.ctx.IASetPrimitiveTopology(D3D11_PRIMITIVE_TOPOLOGY(D3D11_PRIMITIVE_TOPOLOGY_TRIANGLELIST));
        }

        self.bind(&vs);
        self.bind(&ps);
        bind_srv(&self.ctx, &ps, "s_sky", self.sky_cube_srv.as_ref());
        bind_sampler(&self.ctx, &ps, "s_sky", self.s_clamp_linear.as_ref());
        // SAFETY: a drawing call.
        unsafe { self.ctx.DrawIndexed(36, 0, 0) };
    }

    pub(crate) fn pass_directional_light(&mut self) {
        let Some(ps) = self.shader(DIR_LIGHT_PS) else { return };
        // SAFETY: state calls.
        unsafe {
            self.ctx.OMSetRenderTargets(Some(std::slice::from_ref(&self.hdr.rtv)), self.dsv.as_ref());
            self.ctx.OMSetDepthStencilState(self.ds_light.as_ref(), 1);
            self.ctx.RSSetState(self.rs_none.as_ref());
        }

        self.cbs.set_f("shroud_PS", "g_shroud_enabled", &[0.0]);
        self.cbs.set_m("directional_light_PS", "g_overlay_view_projection", Float4x4::IDENTITY.data());

        // The engine's default (see the water pass); the shader writes the refraction weight into the light buffer's alpha.
        self.cbs.set_f("directional_light_PS", "g_refraction_depth", &[0.75]);
        self.upload();
        for i in 0..4 {
            bind_srv(&self.ctx, &ps, &format!("gbuffer_channel_{}_sampler", i + 1), self.gb[i].srv.as_ref());
        }

        bind_srv(&self.ctx, &ps, "s_ssao", self.ao_result.as_ref().or(self.white_srv.as_ref()));
        bind_srv(&self.ctx, &ps, "s_environment", if self.env_pass || !self.env_valid { self.sky_cube_srv.as_ref() } else { self.env_cube_srv.as_ref() });
        bind_srv(&self.ctx, &ps, "s_overlay", self.overlay_none_srv.as_ref());
        bind_srv(&self.ctx, &ps, "s_shroud", self.white_srv.as_ref());
        bind_srv(&self.ctx, &ps, "g_tShadowBuffer_SM50", self.shadow_atlas.srv.as_ref());
        ps_samplers(&self.ctx, &ps, &self.s_clamp_point);
        bind_sampler(&self.ctx, &ps, "s_environment", self.s_clamp_linear.as_ref());
        bind_sampler(&self.ctx, &ps, "s_ssao", self.s_clamp_linear.as_ref());
        bind_sampler(&self.ctx, &ps, "sShadowBuffer_SM50", self.s_shadow_cmp.as_ref());
        self.fullscreen(&ps);
    }

    /// Unbinds the outputs (D3D11 drops an input whose resource is a bound render target), binds a shader and gives every sampler slot one state.
    fn bind_post(&self, s: &GameShader, smp: &Option<ID3D11SamplerState>) {
        // SAFETY: an unbinding call.
        unsafe { self.ctx.OMSetRenderTargets(None, None) };
        self.bind(s);
        ps_samplers(&self.ctx, s, smp);
    }

    pub(crate) fn pass_tonemap(&mut self, env: &EnvironmentParams, dt: f32) {
        let names = [TM_DOWN_LUMA, TM_LUMA_STATS, TM_STATS_DOWN, TM_POINTS, TM_HIGH_PASS, TM_BLUR_X, TM_BLUR_Y, TM_TONE_MAP];
        let shaders = names.map(|name| self.shader(name));
        let [Some(down), Some(stats), Some(stats_down), Some(points), Some(high), Some(bx), Some(by), Some(tm)] = shaders else { return };
        // SAFETY: state calls.
        unsafe {
            self.ctx.OMSetDepthStencilState(self.ds_off.as_ref(), 0);
            self.ctx.RSSetState(self.rs_none.as_ref());
        }

        // 1. A quarter-size copy with the luma in .w.
        self.bind_post(&down, &self.s_clamp_linear.clone());
        bind_srv(&self.ctx, &down, "g_hdr_rgb_texture_sampler", self.hdr.srv.as_ref());
        let quarter = self.quarter.clone();
        self.fullscreen_to(&quarter.t.rtv, quarter.w, quarter.h, &down);

        // 2. The luma statistics pyramid down to 1x1 (min, max, average), clamped by the absolute points.
        let mut src = self.quarter.t.srv.clone();
        for i in 0..self.stats.len() {
            let s = if i == 0 { &stats } else { &stats_down };
            self.bind_post(s, &self.s_clamp_point.clone());
            bind_srv(&self.ctx, s, if i == 0 { "g_luma_values_texture_sampler" } else { "g_luma_stats_texture_sampler" }, src.as_ref());
            let target = self.stats[i].clone();
            self.fullscreen_to(&target.t.rtv, target.w, target.h, s);
            src = target.t.srv;
        }

        // 3. The black / white points with temporal adaptation (prev -> cur); the first frame snaps.
        let (prev, cur) = (self.points_cur, 1 - self.points_cur);
        self.cbs.set_f("auto_exposure_buffer", "g_auto_exposure_dt", &[if self.frame == 0 { 1000.0 } else { dt }]);
        self.bind_post(&points, &self.s_clamp_point.clone());
        bind_srv(&self.ctx, &points, "g_prev_black_and_white_points_sampler", self.points[prev].t.srv.as_ref());
        bind_srv(&self.ctx, &points, "g_luma_stats_texture_sampler", self.stats.last().and_then(|s| s.t.srv.as_ref()));
        let point_rtv = self.points[cur].t.rtv.clone();
        self.fullscreen_to(&point_rtv, 1, 1, &points);
        self.points_cur = cur;

        // The constant tone mapper takes the points from a cbuffer: the previous frames' result is read back through a ring of staging copies;
        // the oldest one (READBACK_RING - 1 frames back) is finished on the GPU.
        // SAFETY: copies between textures of this device; the mapped texel is read within its size and unmapped.
        unsafe {
            if let (Some(staging), Some(tex)) = (&self.points_staging[(self.frame % READBACK_RING as u64) as usize], &self.points[cur].t.tex) {
                self.ctx.CopyResource(staging, tex);
            }

            if self.frame + 1 >= READBACK_RING as u64
                && let Some(staging) = self.points_staging[((self.frame + 1) % READBACK_RING as u64) as usize].clone()
                && let Some(ms) = map_read(&self.ctx, &staging, 0, true)
            {
                let p = std::slice::from_raw_parts(ms.pData.cast::<f32>(), 4);
                self.debug_points.copy_from_slice(p);
                self.debug_maps += 1;
                if p[1] > p[0] {
                    self.luma_black = p[0];
                    self.luma_white = p[1];
                }

                self.ctx.Unmap(&staging, 0);
            }
        }

        self.frame += 1;

        // 4. Bloom: a high pass on the quarter image, a separable 11-tap blur.
        let (bloom_a, bloom_b) = (self.bloom_a.clone(), self.bloom_b.clone());
        self.bind_post(&high, &self.s_clamp_point.clone());
        bind_srv(&self.ctx, &high, "g_black_and_white_points_sampler", self.points[cur].t.srv.as_ref());
        bind_srv(&self.ctx, &high, "g_hdr_rgb_texture_sampler", self.quarter.t.srv.as_ref());
        self.fullscreen_to(&bloom_a.t.rtv, bloom_a.w, bloom_a.h, &high);
        self.bind_post(&bx, &self.s_clamp_linear.clone());
        bind_srv(&self.ctx, &bx, "g_hdr_rgb_texture_sampler", bloom_a.t.srv.as_ref());
        self.fullscreen_to(&bloom_b.t.rtv, bloom_b.w, bloom_b.h, &bx);
        self.bind_post(&by, &self.s_clamp_linear.clone());
        bind_srv(&self.ctx, &by, "g_hdr_rgb_texture_sampler", bloom_b.t.srv.as_ref());
        self.fullscreen_to(&bloom_a.t.rtv, bloom_a.w, bloom_a.h, &by);

        // 5. The tone map, with bloom and the S-curve.
        let curve = build_scurve(env);
        if curve != self.scurve_last {
            // SAFETY: the data covers the 256 texels of the texture.
            unsafe {
                if let Some(tex) = &self.scurve_tex {
                    self.ctx.UpdateSubresource(tex, 0, None, curve.as_ptr().cast(), 256 * 8, 0);
                }
            }

            self.scurve_last = curve;
        }

        self.bind_post(&tm, &self.s_clamp_linear.clone());
        bind_sampler(&self.ctx, &tm, "g_hdr_rgb_texture_sampler", self.s_clamp_point.as_ref());
        bind_sampler(&self.ctx, &tm, "g_black_and_white_points_sampler", self.s_clamp_point.as_ref());
        bind_srv(&self.ctx, &tm, "g_black_and_white_points_sampler", self.points[cur].t.srv.as_ref());
        bind_srv(&self.ctx, &tm, "g_hdr_rgb_texture_sampler", self.hdr.srv.as_ref());
        bind_srv(&self.ctx, &tm, "g_hdr_rgb_bloom_texture_sampler", bloom_a.t.srv.as_ref());
        bind_srv(&self.ctx, &tm, "g_scurve_texture_sampler", self.scurve_srv.as_ref());
        let ldr = self.ldr.clone();
        self.fullscreen_to(&ldr.t.rtv, ldr.w, ldr.h, &tm);
    }

    pub(crate) fn pass_to_screen(&mut self, env: &EnvironmentParams) {
        let mut distortion = if self.distortion { self.particles.distortion().cloned() } else { None };
        let dof = env.use_dof && self.depth_of_field;
        let mut ps = None;
        if distortion.is_some() {
            ps = self.shader(if dof { TO_SCREEN_DOF_DISTORT_PS } else { TO_SCREEN_DISTORT_PS });
        }

        if ps.is_none() {
            ps = self.shader(if dof { TO_SCREEN_DOF_PS } else { TO_SCREEN_PS });
            distortion = None;
        }

        if ps.is_none() {
            ps = self.shader(TO_SCREEN_PS);
        }

        let Some(ps) = ps else { return };

        // The DOF constants (fn 0x172aae0): with use_DOF, the environment's DOF block: focus_distance = DOF_focus's first value, focal_length =
        // the second, blur_kernel_scale = DOF_blur_size; otherwise the battle camera's (dump: 0, 1000, 1).
        let (focus, focal, kernel_scale) = if dof { (env.dof_focus[0], env.dof_focus[1], env.dof_blur_size) } else { (0.0, 1000.0, 1.0) };
        self.cbs.set_f("hdr_to_screen_PS", "focus_distance", &[focus]);
        self.cbs.set_f("hdr_to_screen_PS", "focal_length", &[focal]);
        self.cbs.set_f("hdr_to_screen_PS", "blur_kernel_scale", &[kernel_scale]);

        // The blur kernels (dump): offsets in texels (x / width, y / height), Gaussian weights with sigma^2 = 4 normalised over the kernel, w = 1.
        for (name, n) in [("sample_data_9x9", 9i32), ("sample_data_5x5", 5), ("sample_data_3x3", 3)] {
            let weight = |x: i32, y: i32| (-f64::from((x - n / 2) * (x - n / 2) + (y - n / 2) * (y - n / 2)) / 8.0).exp();
            let mut sum = 0.0f64;
            for y in 0..n {
                for x in 0..n {
                    sum += weight(x, y);
                }
            }

            let mut d = vec![0.0f32; (n * n * 4) as usize];
            for y in 0..n {
                for x in 0..n {
                    let e = &mut d[((y * n + x) * 4) as usize..((y * n + x) * 4 + 4) as usize];
                    e[0] = (x - n / 2) as f32 / self.w as f32;
                    e[1] = (y - n / 2) as f32 / self.h as f32;
                    e[2] = (weight(x, y) / sum) as f32;
                    e[3] = 1.0;
                }
            }

            self.cbs.set_f("hdr_to_screen_PS", name, &d);
        }

        // Battle: no fog of war.
        self.cbs.set_f("shared_fog_of_war_PS", "g_fog_of_war_blend", &[0.0, 0.0]);
        // SAFETY: an unbinding call.
        unsafe { self.ctx.OMSetRenderTargets(None, None) };
        self.bind(&ps);
        ps_samplers(&self.ctx, &ps, &self.s_clamp_point);
        bind_srv(&self.ctx, &ps, "frame_sampler", self.ldr.t.srv.as_ref());
        bind_srv(&self.ctx, &ps, "gbuffer_channel_4_sampler", self.gb[3].srv.as_ref());
        bind_srv(&self.ctx, &ps, "s_fog_of_war_mask", self.black_srv.as_ref());
        if let Some(distortion) = &distortion {
            bind_srv(&self.ctx, &ps, "distortion_sampler", Some(distortion));
            bind_sampler(&self.ctx, &ps, "distortion_sampler", self.s_clamp_linear.as_ref());
            bind_sampler(&self.ctx, &ps, "frame_sampler", self.s_clamp_linear.as_ref());
        }

        let (rtv, w, h) = (self.final_.rtv.clone(), self.w, self.h);
        self.fullscreen_to(&rtv, w, h, &ps);
    }

    /// The lens flare and vignette (LENSE_FLARE_SHADER), drawn on the final image after hdr_to_screen (fn 0x1173ec10). The constants as fn
    /// 0x1172b1b0 sets them: sun_position = camera + to_sun * 7600 (both dumps), sun_flare = SUN_MESH sun_flare, time = the battle clock,
    /// half_texel_offset = 0.5 / size, vignette_strength = 1, pos_transform = (1, 1, 0, 0). Textures (fn 0x116dd4e0): lense_flare_1 =
    /// green_dot_flare, 2 = streaks_flare, 3 = rays_flare (commontextures).
    pub(crate) fn pass_lens_flare(&mut self, env: &EnvironmentParams, eye: Float3, time_s: f32) {
        let main_vs = self.shader(FLARE_MAIN_VS);
        let vignette = self.shader(VIGNETTE_PS);
        let mut flares = vec![];
        for i in 0..4 {
            let vs = self.shader(FLARE_VS[i]);
            let ps = self.shader(FLARE_PS[i]);
            flares.push((vs, ps));
        }

        let (Some(main_vs), Some(vignette)) = (main_vs, vignette) else { return };
        let sun = eye + env.to_sun() * SUN_DISTANCE;

        // The SUN_MESH loader clamps 0..1.
        let flare = if env.has_sky { env.sun_flare.clamp(0.0, 1.0) } else { 0.0 };
        self.cbs.set_f("lense_flare_VS", "sun_occlusion", &[self.sun_visibility]);
        self.cbs.set_f("lense_flare_VS", "sun_position", &[sun.x, sun.y, sun.z]);
        self.cbs.set_f("lense_flare_VS", "sun_flare", &[flare]);
        self.cbs.set_f("lense_flare_VS", "pos_transform", &[1.0, 1.0, 0.0, 0.0]);
        self.cbs.set_f("lense_flare_PS", "time", &[time_s]);
        self.cbs.set_f("lense_flare_PS", "half_texel_offset", &[0.5 / self.w as f32, 0.5 / self.h as f32]);
        self.cbs.set_f("lense_flare_PS", "vignette_strength", &[1.0]);
        self.cbs.set_f("lense_flare_PS", "sun_flare_ps", &[flare]);
        self.upload();
        let el = [
            element(c"POSITION", 0, DXGI_FORMAT_R32G32B32A32_FLOAT, 0, 0, false),
            element(c"COLOR", 0, DXGI_FORMAT_R32G32B32A32_FLOAT, 0, 16, false),
            element(c"TEXCOORD", 0, DXGI_FORMAT_R32G32_FLOAT, 0, 32, false),
        ];

        // SAFETY: state calls with objects of this device.
        unsafe {
            self.ctx.OMSetRenderTargets(Some(std::slice::from_ref(&self.final_.rtv)), None);
            self.ctx.RSSetViewports(Some(&[viewport(0.0, 0.0, self.w as f32, self.h as f32)]));
            self.ctx.OMSetDepthStencilState(self.ds_off.as_ref(), 0);
            self.ctx.RSSetState(self.rs_none.as_ref());
            self.ctx.IASetVertexBuffers(0, 1, Some(&self.flare_vb), Some(&40), Some(&0));
            self.ctx.IASetPrimitiveTopology(D3D11_PRIMITIVE_TOPOLOGY(D3D11_PRIMITIVE_TOPOLOGY_TRIANGLELIST));
        }

        if flare > 0.0 && self.sun_visibility > 0.0 {
            let textures = ["commontextures/green_dot_flare.dds", "commontextures/streaks_flare.dds", "commontextures/green_dot_flare.dds", "commontextures/rays_flare.dds"];
            let samplers = ["lense_flare_1_sampler", "lense_flare_2_sampler", "lense_flare_1_sampler", "lense_flare_3_sampler"];
            // SAFETY: a state call.
            unsafe { self.ctx.OMSetBlendState(self.bs_add_one.as_ref(), None, 0xFFFF_FFFF) };
            for (i, (vs, ps)) in flares.into_iter().enumerate() {
                let (Some(vs), Some(ps)) = (vs, ps) else { continue };
                let Some(layout) = self.layout(&vs, "flare", &el) else { continue };
                // SAFETY: a state call.
                unsafe { self.ctx.IASetInputLayout(&layout) };
                self.bind(&vs);
                self.bind(&ps);

                // lense_flare_1..3_sampler have the engine's sRGB-read flag (sampler +0x3c, fn 0x116de600).
                let srv = self.sky_texture(textures[i], true);
                bind_srv(&self.ctx, &ps, samplers[i], srv.as_ref());
                bind_sampler(&self.ctx, &ps, samplers[i], self.s_clamp_linear.as_ref());
                // SAFETY: a drawing call.
                unsafe { self.ctx.Draw(6, 0) };
            }
        }

        // The vignette (blend object +0x71f: SRC_ALPHA / INV_SRC_ALPHA, black with alpha = saturate(2 r - 0.9)^1.7).
        let Some(layout) = self.layout(&main_vs, "flare", &el) else { return };
        // SAFETY: state and drawing calls with objects of this device.
        unsafe {
            self.ctx.OMSetBlendState(self.bs_alpha.as_ref(), None, 0xFFFF_FFFF);
            self.ctx.IASetInputLayout(&layout);
        }

        self.bind(&main_vs);
        self.bind(&vignette);
        // SAFETY: drawing and state calls.
        unsafe {
            self.ctx.Draw(6, 0);
            self.ctx.OMSetBlendState(self.bs_opaque.as_ref(), None, 0xFFFF_FFFF);
        }

        null_ps_srvs(&self.ctx, 4);
    }

    /// The Depth Buffer Analysis Shadow Map View equivalent: the min / max view depth of the visible geometry from the G-buffer depth channel
    /// (z / w), read back two frames later. Sky pixels (z / w = 0 after the clear) are skipped.
    pub(crate) fn analyse_depth(&mut self, cam: &Camera) {
        if self.depth_staging[0].is_none() || self.depth_staging_w != self.w || self.depth_staging_h != self.h {
            for t in &mut self.depth_staging {
                *t = None;
                let sd = D3D11_TEXTURE2D_DESC {
                    Width: self.w, Height: self.h, MipLevels: 1, ArraySize: 1, Format: DXGI_FORMAT_R32_FLOAT, SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
                    Usage: D3D11_USAGE_STAGING, BindFlags: 0, CPUAccessFlags: D3D11_CPU_ACCESS_READ as u32, MiscFlags: 0,
                };

                // SAFETY: a plain creation call.
                let _ = unsafe { self.dev.CreateTexture2D(&sd, None, Some(t)) };
            }

            self.depth_staging_w = self.w;
            self.depth_staging_h = self.h;
            self.depth_frame = 0;
        }

        // SAFETY: a copy between textures of this device; the mapped rows are read within their pitch and unmapped.
        unsafe {
            if let (Some(staging), Some(depth)) = (&self.depth_staging[(self.depth_frame & 1) as usize], &self.gb[3].tex) {
                self.ctx.CopyResource(staging, depth);
            }

            let first = self.depth_frame == 0;
            self.depth_frame += 1;
            if first {
                return;
            }

            let Some(src) = self.depth_staging[(self.depth_frame & 1) as usize].clone() else { return };
            let Some(ms) = map_read(&self.ctx, &src, 0, self.deterministic) else { return };
            let (zn, zf) = (cam.znear, cam.zfar);
            let (mut lo, mut hi) = (3.4e38f32, 0.0f32);
            for y in (0..self.h as usize).step_by(2) {
                let row = std::slice::from_raw_parts(ms.pData.cast::<u8>().add(y * ms.RowPitch as usize).cast::<f32>(), self.w as usize);
                for x in (0..self.w as usize).step_by(2) {
                    let d = row[x];
                    if d.partial_cmp(&0.0) != Some(std::cmp::Ordering::Greater) || d >= 1.0 {
                        continue;
                    }

                    let z = zn * zf / (zf - d * (zf - zn));
                    lo = lo.min(z);
                    hi = hi.max(z);
                }
            }

            self.ctx.Unmap(&src, 0);
            self.depth_range_valid = hi > 0.0;
            if self.depth_range_valid {
                self.depth_min = lo;
                self.depth_max = hi;
            }
        }
    }

    /// The battle cascade splits (ShadowMapView, fns 0x11794320 / 0x11794430 / 0x11784c00, verified on all three dumps):
    /// - far = camera near + sum(row) with the quality row (30, 90, 210, 450) -> 781 (fn 0x11751fc0, table dll+0x1c88aa0);
    /// - range = the depth-buffer analysis (min, max visible view depth, max clamped to far), max *= shadow_split_enlargement_safe_zone (1.05);
    ///   start = min, scale = (max - min) / (far - near);
    /// - s0 = start, s_i = start + scale * (cumulative row)_i; cascade i covers [s_i - 0.1 (s_i - s_(i-1)), s_(i+1)] (shadow_split_blend_fraction
    ///   0.1); g_fFadeRange = (0.1, (s4 - (far - 0.2 (s4 - s3))) / (0.2 (s4 - s3)) * 0.2) (final_shadow_split_blend_fraction 0.2).
    pub(crate) fn pass_shadows(&mut self, cam: &Camera, aspect: f32, light_dir: Float3, _time_s: f32) {
        let row = SHADOW_ROWS[self.shadow_quality.clamp(0, 3) as usize];
        let n = cam.znear.max(1.0);
        let far_q = n + row[0] + row[1] + row[2] + row[3];
        let (mut start, mut scale) = (n, 1.0f32);
        if self.depth_range_valid {
            let (lo, hi) = (self.depth_min, self.depth_max.min(far_q) * 1.05);
            if lo < hi {
                start = lo;
                scale = (hi - lo).max(1.0) / (far_q - n);
            }
        }

        let mut splits = [0.0f32; 5];
        splits[0] = start;
        for i in 1..=4 {
            splits[i] = splits[i - 1] + row[i - 1] * scale;
        }

        let mut slice_near = [0.0f32; 4];
        slice_near[0] = splits[0];
        for i in 1..4 {
            slice_near[i] = splits[i] - 0.1 * (splits[i] - splits[i - 1]);
        }

        let fade_w = (splits[4] - splits[3]) * 0.2;
        let fade_y = (splits[4] - (far_q - fade_w)).max(1e-4) / fade_w * 0.2;
        let cam_view = look_at_lh(cam.eye, cam.target, Float3::new(0.0, 1.0, 0.0));
        let inv_view = inverse(&cam_view);
        let tan_y = (cam.fov_y * 0.5).tan();
        let tan_x = tan_y * aspect;
        let up = if light_dir.y.abs() > 0.99 { Float3::new(0.0, 0.0, 1.0) } else { Float3::new(0.0, 1.0, 0.0) };

        let mut matrices = [0.0f32; 64];
        let mut light_vp = [Float4x4::IDENTITY; 4];
        let mut texel_c = [0.0f32; 4];
        for c in 0..4 {
            // The bounding sphere of the frustum slice (a stable size while the camera rotates).
            let pts: [Float3; 8] = std::array::from_fn(|k| {
                let d = if k & 4 != 0 { splits[c + 1] } else { slice_near[c] };
                let v = Float3::new(if k & 1 != 0 { 1.0 } else { -1.0 } * tan_x * d, if k & 2 != 0 { 1.0 } else { -1.0 } * tan_y * d, d);
                inv_view.transform_point(v)
            });

            let mut centre = Float3::default();
            for p in &pts {
                centre = centre + *p * 0.125;
            }

            let radius = pts.iter().fold(0.0f32, |r, p| r.max(length(*p - centre))).ceil();

            // Snap the centre to shadow texels in light space to avoid shimmering.
            let lview = look_at_lh(centre - light_dir * 2000.0, centre, up);
            let texel = 2.0 * radius / (SHADOW_SIZE / 2) as f32;
            texel_c[c] = texel;
            let lc = lview.transform_point(centre);
            let sx = (lc.x / texel).floor() * texel - lc.x;
            let sy = (lc.y / texel).floor() * texel - lc.y;
            light_vp[c] = ortho_lh(2.0 * radius, 2.0 * radius, 0.0, 4000.0) * translation(Float3::new(sx, sy, 0.0)) * lview;

            // World -> atlas uv (y down) + depth: the lighting shader's lookup.
            let (bx, by) = (SHADOW_BOUNDS[c][0], SHADOW_BOUNDS[c][1]);
            let mut to_uv = Float4x4::IDENTITY;
            to_uv.m[0][0] = 0.25;
            to_uv.m[0][3] = bx + 0.25;
            to_uv.m[1][1] = -0.25;
            to_uv.m[1][3] = by + 0.25;
            matrices[c * 16..c * 16 + 16].copy_from_slice((to_uv * light_vp[c]).data());
        }

        // Render the casters into the four quadrants.
        let vs = self.shader(SHADOW_VS);
        let ps = self.shader(SHADOW_PS);
        // SAFETY: clears of views of this device.
        unsafe {
            self.ctx.OMSetRenderTargets(None, None);
            if let Some(rtv) = &self.shadow_atlas.rtv {
                self.ctx.ClearRenderTargetView(rtv, &[1.0; 4]);
            }

            if let Some(dsv) = &self.shadow_dsv {
                self.ctx.ClearDepthStencilView(dsv, D3D11_CLEAR_DEPTH as u32, 1.0, 0);
            }
        }

        if let (Some(vs), Some(ps)) = (vs, ps)
            && (self.draws.items() > 0 || self.terrain_draws.items() > 0)
            && self.shadows
            && let Some(layout) = self.layout(&vs, "rigid_std", &rigid_layout())
        {
            // SAFETY: state calls with objects of this device.
            unsafe {
                self.ctx.IASetInputLayout(&layout);
                self.ctx.IASetPrimitiveTopology(D3D11_PRIMITIVE_TOPOLOGY(D3D11_PRIMITIVE_TOPOLOGY_TRIANGLELIST));
                self.ctx.OMSetRenderTargets(Some(std::slice::from_ref(&self.shadow_atlas.rtv)), self.shadow_dsv.as_ref());
                self.ctx.OMSetDepthStencilState(self.ds_sky.as_ref(), 0);
                self.ctx.OMSetDepthStencilState(self.ds_gbuffer.as_ref(), 0);
                self.ctx.RSSetState(self.rs_shadow.as_ref());
            }

            let (terrain_draws, draws) = (std::mem::take(&mut self.terrain_draws), std::mem::take(&mut self.draws));
            let (mut vis_terrain, mut vis) = (std::mem::take(&mut self.vis_terrain), std::mem::take(&mut self.vis));
            for c in 0..4 {
                let half = SHADOW_SIZE as f32 / 2.0;
                // SAFETY: a state call.
                unsafe { self.ctx.RSSetViewports(Some(&[viewport(SHADOW_BOUNDS[c][0] * SHADOW_SIZE as f32, SHADOW_BOUNDS[c][1] * SHADOW_SIZE as f32, half, half)])) };
                self.cbs.set_m("camera_VS_PS", "view_projection", light_vp[c].data());
                self.upload();
                self.bind(&vs);
                self.bind(&ps);
                for &slot in vs.textures.values() {
                    // SAFETY: a binding call.
                    unsafe { self.ctx.VSSetShaderResources(slot, Some(std::slice::from_ref(&self.instance_ids_srv))) };
                }

                let view = DrawView {
                    frustum: Frustum::from_view_projection(&light_vp[c]),
                    eye: cam.eye,
                    shadow: true,

                    // Farther cascades need less detail; a caster smaller than a couple of shadow texels leaves no shadow.
                    coarser: c as i32,
                    lod_scale: self.lod_scale,
                    min_radius: texel_c[c] * self.shadow_min_texels,
                    ..Default::default()
                };

                terrain_draws.cull(&view, &mut vis_terrain);
                draws.cull(&view, &mut vis);
                let firsts = self.upload_instances([&terrain_draws, &draws], [&vis_terrain, &vis]);
                for v in [&vis_terrain, &vis] {
                    self.draw_stats.views += 1;
                    self.draw_stats.instances += v.instances() as u64;
                }

                // SAFETY: a binding call.
                unsafe { self.ctx.IASetVertexBuffers(1, 1, Some(&self.instance_ring), Some(&(size_of::<RigidInstance>() as u32)), Some(&0)) };
                let (instances, triangles) = self.draw_batches([&terrain_draws, &draws], [&vis_terrain, &vis], firsts, |_, _| true);
                self.draw_stats.shadow_instances += instances;
                self.draw_stats.shadow_triangles += triangles;
            }

            self.terrain_draws = terrain_draws;
            self.draws = draws;
            self.vis_terrain = vis_terrain;
            self.vis = vis;
        }

        // SAFETY: an unbinding call.
        unsafe { self.ctx.OMSetRenderTargets(None, None) };

        // shadowmap_PS as the game fills it (the layout and constants from the memory dump).
        let size = SHADOW_SIZE as f32;
        self.cbs.set_f("shadowmap_PS", "g_vHardShadowBufferSize", &[size, size, 1.0 / size, 1.0 / size]);
        self.cbs.set_f("shadowmap_PS", "g_amHardSplit", &matrices);

        // float[5], a 16-byte stride.
        let mut dist = [0.0f32; 17];
        for (i, split) in splits.iter().enumerate() {
            dist[i * 4] = *split;
        }

        self.cbs.set_f("shadowmap_PS", "g_split_distances", &dist);
        self.cbs.set_f("shadowmap_PS", "g_split_uv_texture_bounds", SHADOW_BOUNDS.as_flattened());
        self.cbs.set_f("shadowmap_PS", "g_fFadeRange", &[0.1, fade_y]);
        self.cbs.set_f("shadowmap_PS", "g_sample_bias", &[0.0]);
        self.cbs.set_f("shadowmap_PS", "g_iSplitCount", &[if self.shadows { 4.0 } else { 0.0 }]);
        self.cbs.set_f("shadowmap_PS", "g_shadow_light_direction", &[light_dir.x, light_dir.y, light_dir.z]);
    }

    /// The sun disc, and its occlusion (fn 0x1927460, getter fn 0x118f50c0): visible = the occlusion-query sample count of the sun draw, total =
    /// int(kx / w * W / 2) * int(ky / w * H / 2) for the view-space corner (k, k, z) of the sun quad, k = sun_scale * 2000 (1 when w <= 0);
    /// sun_occlusion = clamp(min(visible, total) / total, 0, 1).
    pub(crate) fn pass_sun(&mut self, env: &EnvironmentParams, view_m: &Float4x4, proj: &Float4x4) {
        let vs = self.shader(SUN_VS);
        let ps = self.shader(SUN_PS);
        if !self.env_pass {
            let old = self.sun_query[((self.sun_query_frame + 1) % 4) as usize].clone();
            let mut visible = 0u64;
            let query_ready = |r: &Self, q: &ID3D11Query, visible: &mut u64| {
                // SAFETY: the output is a local of the query's data size.
                unsafe {
                    let out = Some((visible as *mut u64).cast());
                    if !r.deterministic {
                        return r.ctx.GetData(q, out, 8, D3D11_ASYNC_GETDATA_DONOTFLUSH as u32).0 == 0;
                    }

                    loop {
                        let hr = r.ctx.GetData(q, out, 8, 0);
                        if hr.0 != 1 {
                            return hr.0 == 0;
                        }
                    }
                }
            };

            if self.sun_query_frame >= 3
                && let Some(old) = old
                && query_ready(self, &old, &mut visible)
            {
                let c = env.to_sun() * SUN_DISTANCE;
                let z = view_m.m[2][0] * c.x + view_m.m[2][1] * c.y + view_m.m[2][2] * c.z;
                let k = env.sun_scale * SUN_MESH_SIZE;
                let w = proj.m[3][0] * k + proj.m[3][1] * k + proj.m[3][2] * z + proj.m[3][3];
                let mut total = 1u64;
                if w > 0.0 {
                    let cx = proj.m[0][0] * k + proj.m[0][1] * k + proj.m[0][2] * z + proj.m[0][3];
                    let cy = proj.m[1][0] * k + proj.m[1][1] * k + proj.m[1][2] * z + proj.m[1][3];
                    let px = (cx / w * self.w as f32 * 0.5) as i64;
                    let py = (cy / w * self.h as f32 * 0.5) as i64;
                    total = (px * py).max(1) as u64;
                } else {
                    visible = 0;
                }

                self.sun_visibility = ((visible.min(total) as f64 / total as f64) as f32).clamp(0.0, 1.0);
            }

            self.sun_query_frame += 1;
        }

        let (Some(vs), Some(ps)) = (vs, ps).into_iter_if(env.has_sky && env.sun_scale > 0.0) else {
            if !self.env_pass {
                self.sun_visibility = 0.0;
            }

            return;
        };

        let tex = format!("rigidmodels/skybox/{}", env.sun_texture);
        if tex != self.sun_texture_path {
            self.sun_texture_path = tex.clone();
            self.sun_srv = None;
            match self.files.read(&tex).and_then(|bytes| load_dds(&self.dev, &bytes, true).map_err(|error| error.to_string())) {
                Ok(texture) => self.sun_srv = texture.srv,
                Err(error) => self.error(format!("sun texture: {error}")),
            }
        }

        if self.sun_srv.is_none() {
            return;
        }

        // g_world_view: the camera rotation, translation = rotation * (to_sun * distance) (camera-relative).
        let mut wv = *view_m;
        let s = env.to_sun() * SUN_DISTANCE;
        for r in 0..3 {
            wv.m[r][3] = view_m.m[r][0] * s.x + view_m.m[r][1] * s.y + view_m.m[r][2] * s.z;
        }

        self.cbs.set_m("sun_VS", "g_world_view", wv.data());
        self.cbs.set_m("sun_VS", "g_projection", proj.data());
        self.cbs.set_f("sun_VS", "g_sun_scale", &[env.sun_scale]);
        self.upload();
        let el = [element(c"POSITION", 0, DXGI_FORMAT_R32G32_FLOAT, 0, 0, false), element(c"TEXCOORD", 0, DXGI_FORMAT_R32G32_FLOAT, 0, 8, false)];
        let Some(layout) = self.layout(&vs, "sun", &el) else { return };
        // SAFETY: state calls with objects of this device.
        unsafe {
            self.ctx.IASetInputLayout(&layout);
            self.ctx.IASetVertexBuffers(0, 1, Some(&self.sun_vb), Some(&16), Some(&0));
            self.ctx.IASetPrimitiveTopology(D3D11_PRIMITIVE_TOPOLOGY(D3D11_PRIMITIVE_TOPOLOGY_TRIANGLELIST));
            self.ctx.OMSetRenderTargets(Some(std::slice::from_ref(&self.hdr.rtv)), self.dsv.as_ref());
            self.ctx.OMSetDepthStencilState(self.ds_sky.as_ref(), 0);

            // The SUN_SHADER blend (fn 0x116fcf80 area, blend object +0x23b): SRC_ALPHA / INV_SRC_ALPHA on colour and alpha.
            self.ctx.OMSetBlendState(self.bs_sun.as_ref(), None, 0xFFFF_FFFF);
            self.ctx.RSSetState(self.rs_none.as_ref());
        }

        self.bind(&vs);
        self.bind(&ps);
        bind_srv(&self.ctx, &ps, "s_diffuse", self.sun_srv.as_ref());

        // SUN_SHADER s_diffuse.
        ps_samplers(&self.ctx, &ps, &self.s_wrap_linear);
        // SAFETY: drawing calls; the sun object's occlusion query wraps its own draw.
        unsafe {
            if self.env_pass {
                self.ctx.Draw(6, 0);
            } else if let Some(q) = self.sun_query[(self.sun_query_frame % 4) as usize].clone() {
                self.ctx.Begin(&q);
                self.ctx.Draw(6, 0);
                self.ctx.End(&q);
            } else {
                self.ctx.Draw(6, 0);
            }

            self.ctx.OMSetBlendState(self.bs_opaque.as_ref(), None, 0xFFFF_FFFF);
        }
    }

    /// A sky texture (cloud maps, the lens flare sprites), cached; `None` (reported once) when it can't be loaded.
    pub(crate) fn sky_texture(&mut self, path: &str, srgb: bool) -> Option<ID3D11ShaderResourceView> {
        let key = format!("{path}{}", if srgb { "|srgb" } else { "|unorm" });
        if let Some(srv) = self.sky_textures.get(&key) {
            return srv.clone();
        }

        // s_diffuse_cloud_maps_0/1: the sRGB-read flag is set (sampler +0x3c).
        let srv = match self.files.read(path).and_then(|bytes| load_dds(&self.dev, &bytes, srgb).map_err(|error| error.to_string())) {
            Ok(texture) => texture.srv,
            Err(error) => {
                self.error(format!("{path}: {error}"));
                None
            }
        };

        self.sky_textures.insert(key, srv.clone());
        srv
    }

    pub(crate) fn pass_clouds(&mut self, env: &EnvironmentParams) {
        let vs = self.shader(CLOUD_VS);
        let ps = self.shader(CLOUD_PS);
        let (Some(vs), Some(ps)) = (vs, ps) else { return };
        if !self.clouds {
            return;
        }

        self.cloud_gpu.resize_with(env.cloud_planes.len(), Default::default);

        // The far plane first.
        let mut order = vec![];
        for (i, plane) in env.cloud_planes.iter().enumerate() {
            if plane.far_plane {
                order.insert(0, i);
            } else {
                order.push(i);
            }
        }

        let el = [
            element(c"POSITION", 0, DXGI_FORMAT_R32G32_FLOAT, 0, 0, false),
            element(c"TEXCOORD", 0, DXGI_FORMAT_R32G32_FLOAT, 0, 8, false),
            element(c"TEXCOORD", 1, DXGI_FORMAT_R32G32B32A32_FLOAT, 1, 0, true),
            element(c"TEXCOORD", 2, DXGI_FORMAT_R32G32B32A32_FLOAT, 1, 16, true),
            element(c"TEXCOORD", 3, DXGI_FORMAT_R32G32B32A32_FLOAT, 1, 32, true),
            element(c"TEXCOORD", 4, DXGI_FORMAT_R32G32B32A32_FLOAT, 1, 48, true),
            element(c"TEXCOORD", 5, DXGI_FORMAT_R32G32_FLOAT, 1, 64, true),
        ];

        let Some(layout) = self.layout(&vs, "clouds", &el) else { return };
        // SAFETY: state calls with objects of this device.
        unsafe {
            self.ctx.IASetInputLayout(&layout);
            self.ctx.IASetPrimitiveTopology(D3D11_PRIMITIVE_TOPOLOGY(D3D11_PRIMITIVE_TOPOLOGY_TRIANGLELIST));
            self.ctx.OMSetRenderTargets(Some(std::slice::from_ref(&self.hdr.rtv)), self.dsv.as_ref());
            self.ctx.OMSetDepthStencilState(self.ds_sky.as_ref(), 0);
            self.ctx.OMSetBlendState(self.bs_alpha.as_ref(), None, 0xFFFF_FFFF);
            self.ctx.RSSetState(self.rs_none.as_ref());
        }

        let to_sun = env.to_sun();
        for i in order {
            let p = &env.cloud_planes[i];

            // Regenerated when anything that affects spawning changes (the numbers as C's %f writes them, as the C++ editor compared them).
            let mut sig = format!("{}|{}|{:.6}|{:.6}|{:.6}|{}", self.cloud_seed, p.grid_size, p.density, p.height_variation, p.rotation, i32::from(p.far_plane));
            for c in &p.clouds {
                sig += &format!("|{}{:.6}", i32::from(c.this_cloud_only), c.probability);
                for e in &c.elements {
                    sig += &format!(";{}{:.6}{:.6}{:.6}{:.6}{}{}{}", i32::from(e.enabled), e.scale, e.offset[0], e.offset[1], e.offset[2], e.texture_index, e.texture_row, e.texture_column);
                }
            }

            let mut dirty = false;
            let g = &mut self.cloud_gpu[i];
            if sig != g.signature {
                g.signature = sig;
                g.field.reset(p, self.cloud_seed.wrapping_mul(7919).wrapping_add(i as u32));
                dirty = true;
            }

            if p.velocity * 0.1 > 0.001 && self.cloud_dt > 0.0 {
                g.field.update(p, self.cloud_dt);
                dirty = true;
            }

            if dirty {
                let inst = g.field.instances();
                g.count = inst.len() as u32;
                if g.count > g.capacity || g.instances.is_none() {
                    g.capacity = (g.count + 256).max(1024);
                    g.instances = dynamic_buffer(&self.dev, g.capacity as usize * 72, D3D11_BIND_VERTEX_BUFFER);
                }

                let bytes = inst.iter().flat_map(|c| c.t1.iter().chain(&c.t2).chain(&c.t3).chain(&c.t4).chain(&c.t5).flat_map(|f| f.to_le_bytes())).collect::<Vec<_>>();
                if g.count > 0
                    && let Some(buffer) = &g.instances
                {
                    let mut ms = D3D11_MAPPED_SUBRESOURCE::default();
                    // SAFETY: the buffer is written within its capacity while mapped.
                    unsafe {
                        if self.ctx.Map(buffer, 0, D3D11_MAP_WRITE_DISCARD, 0, Some(&mut ms)).is_ok() {
                            std::ptr::copy_nonoverlapping(bytes.as_ptr(), ms.pData.cast::<u8>(), bytes.len());
                            self.ctx.Unmap(buffer, 0);
                        }
                    }
                }
            }

            let (count, instances) = (self.cloud_gpu[i].count, self.cloud_gpu[i].instances.clone());
            if count == 0 || instances.is_none() {
                continue;
            }

            // cloud_plane_VS / cloud_plane_PS as the game fills them (memory dump): the colours raw except the shadow colour (pow 2.2).
            // Verified (fn 0x191e430): g_horizon_alpha_start / end = the file's value * 0.1 + 0.9.
            let alignment = self.wind.alignment();
            let cbs = &mut self.cbs;

            // fn 0x191e430 (unused by the shader).
            cbs.set_f("cloud_plane_VS", "g_wind_alignment", &[alignment]);
            cbs.set_f("cloud_plane_VS", "g_cloud_shadow_gradient", &[p.shadow_gradient]);
            cbs.set_f("cloud_plane_VS", "g_height_scale", &[p.height_scale]);
            cbs.set_f("cloud_plane_PS", "g_sun_direction", &[to_sun.x, to_sun.y, to_sun.z]);
            cbs.set_f("cloud_plane_PS", "g_sun_transmitted_colour", &p.transmitted);
            cbs.set_f("cloud_plane_PS", "g_sun_reflected_colour", &p.reflected);
            cbs.set_f("cloud_plane_PS", "g_transmit_falloff", &[p.transmit_falloff]);
            cbs.set_f("cloud_plane_PS", "g_flatten_factor", &[p.flatten]);
            let sh = colour_to_linear(Float3::new(p.shadow[0], p.shadow[1], p.shadow[2]));
            cbs.set_f("cloud_plane_PS", "g_cloud_shadow_colour", &[sh.x, sh.y, sh.z]);
            cbs.set_f("cloud_plane_PS", "g_cloud_shadow_falloff", &[p.shadow_falloff]);
            cbs.set_f("cloud_plane_PS", "g_horizon_alpha_start", &[p.horizon_alpha_start * 0.1 + 0.9]);
            cbs.set_f("cloud_plane_PS", "g_horizon_alpha_end", &[p.horizon_alpha_end * 0.1 + 0.9]);
            self.upload();
            self.bind(&vs);
            self.bind(&ps);
            let (map0, map1) = (self.sky_texture(&cloud_texture(p, 0), true), self.sky_texture(&cloud_texture(p, 1), true));
            bind_srv(&self.ctx, &ps, "s_diffuse_cloud_maps_0", map0.as_ref());
            bind_srv(&self.ctx, &ps, "s_diffuse_cloud_maps_1", map1.as_ref());

            // The cloud maps: linear wrap.
            ps_samplers(&self.ctx, &ps, &self.s_wrap_linear);
            // SAFETY: drawing calls with buffers of this device.
            unsafe {
                self.ctx.IASetVertexBuffers(0, 2, Some([self.cloud_quad_vb.clone(), instances].as_ptr()), Some([16u32, 72].as_ptr()), Some([0u32, 0].as_ptr()));
                self.ctx.DrawInstanced(6, count, 0, 0);
            }
        }

        // SAFETY: a state call.
        unsafe { self.ctx.OMSetBlendState(self.bs_opaque.as_ref(), None, 0xFFFF_FFFF) };
    }

    pub(crate) fn pass_ssao(&mut self, cam: &Camera) {
        let res = self.shader(SSAO_PS);
        let bx = self.shader(SSAO_BLUR_X_PS);
        let by = self.shader(SSAO_BLUR_Y_PS);
        let (Some(res), Some(bx), Some(by)) = (res, bx, by).into_iter_if(self.ssao) else {
            // SAFETY: a clear of a view of this device.
            unsafe {
                if let Some(rtv) = &self.ao.rtv {
                    self.ctx.ClearRenderTargetView(rtv, &[1.0; 4]);
                }
            }

            self.ao_result = self.ao.srv.clone();
            return;
        };

        // ssao_resolve_PS / ssao_blur_PS as in the memory dump (HBAO: 5 directions x 5 steps).
        let cbs = &mut self.cbs;
        cbs.set_f("ssao_resolve_PS", "g_R", &[1.0]);
        cbs.set_f("ssao_resolve_PS", "g_ExternalAngleBias", &[0.0]);
        cbs.set_f("ssao_resolve_PS", "g_Attenuation", &[0.78]);
        cbs.set_f("ssao_resolve_PS", "g_ExternalContrast", &[1.0]);
        cbs.set_f("ssao_resolve_PS", "g_AOResolution", &[self.w as f32, self.h as f32]);
        cbs.set_f("ssao_resolve_PS", "g_VerticalFieldOfView", &[cam.fov_y]);
        cbs.set_f("ssao_resolve_PS", "g_Strength", &[3.0]);
        cbs.set_f("ssao_resolve_PS", "g_MinStrength", &[0.17]);
        cbs.set_f("ssao_resolve_PS", "g_JitterResolution", &[256.0]);
        cbs.set_f("ssao_resolve_PS", "g_NumDir", &[5.0]);
        cbs.set_f("ssao_resolve_PS", "g_NumSteps", &[5.0]);
        cbs.set_f("ssao_blur_PS", "g_BlurRadius", &[4.0]);
        cbs.set_f("ssao_blur_PS", "g_BlurSharpness", &[3.0]);

        // Dump: half the screen size.
        cbs.set_f("ssao_blur_PS", "g_BlurResolution", &[self.w as f32 * 0.5, self.h as f32 * 0.5]);
        // SAFETY: state calls.
        unsafe {
            self.ctx.OMSetDepthStencilState(self.ds_off.as_ref(), 0);
            self.ctx.RSSetState(self.rs_none.as_ref());
        }

        let (w, h) = (self.w, self.h);
        let (ao, ao_tmp, ao_blur) = (self.ao.clone(), self.ao_tmp.clone(), self.ao_blur.clone());
        self.ssao_run(&res, None, &ao, w, h);
        self.ssao_run(&bx, ao.srv.clone(), &ao_tmp, w / 2, h / 2);
        self.ssao_run(&by, ao_tmp.srv.clone(), &ao_blur, w / 2, h / 2);
        self.ao_result = ao_blur.srv;
    }

    fn ssao_run(&mut self, ps: &Rc<GameShader>, src: Option<ID3D11ShaderResourceView>, dst: &Target, tw: u32, th: u32) {
        // SAFETY: an unbinding call.
        unsafe { self.ctx.OMSetRenderTargets(None, None) };
        self.bind(ps);
        ps_samplers(&self.ctx, ps, &self.s_clamp_point);
        bind_sampler(&self.ctx, ps, "jitter_texture_sampler", self.s_wrap_point.as_ref());
        bind_sampler(&self.ctx, ps, "resolve_target_sampler", self.s_clamp_linear.as_ref());

        // blur_y's name for its input.
        bind_sampler(&self.ctx, ps, "blur_x_target_sampler", self.s_clamp_linear.as_ref());
        bind_srv(&self.ctx, ps, "gbuffer_channel_4_sampler", self.gb[3].srv.as_ref());
        bind_srv(&self.ctx, ps, "jitter_texture_sampler", self.jitter_srv.as_ref());
        if let Some(src) = &src {
            bind_srv(&self.ctx, ps, "resolve_target_sampler", Some(src));
            bind_srv(&self.ctx, ps, "blur_x_target_sampler", Some(src));
        }

        // SAFETY: state calls with objects of this device.
        unsafe {
            self.ctx.OMSetRenderTargets(Some(std::slice::from_ref(&dst.rtv)), None);
            self.ctx.RSSetViewports(Some(&[viewport(0.0, 0.0, tw as f32, th as f32)]));
        }

        self.upload();
        // SAFETY: state calls.
        unsafe {
            self.ctx.IASetInputLayout(None);
            self.ctx.IASetPrimitiveTopology(D3D11_PRIMITIVE_TOPOLOGY(D3D11_PRIMITIVE_TOPOLOGY_TRIANGLELIST));
            self.ctx.VSSetShader(self.fs_vs2.as_ref(), None);
        }

        self.bind(ps);
        // SAFETY: a drawing call.
        unsafe { self.ctx.Draw(3, 0) };
        null_ps_srvs(&self.ctx, 4);
    }

    pub(crate) fn pass_fog(&mut self) {
        let Some(ps) = self.shader(FOG_PS) else { return };
        if !self.fog {
            return;
        }

        // SAFETY: an unbinding call.
        unsafe { self.ctx.OMSetRenderTargets(None, None) };
        self.bind(&ps);
        bind_srv(&self.ctx, &ps, "gbuffer_channel_4_sampler", self.gb[3].srv.as_ref());
        bind_srv(&self.ctx, &ps, "s_sky", self.sky_cube_srv.as_ref());
        ps_samplers(&self.ctx, &ps, &self.s_clamp_point);
        bind_sampler(&self.ctx, &ps, "s_sky", self.s_clamp_linear.as_ref());

        // Fog over geometry only (stencil 1): sky pixels keep the sky.
        // SAFETY: state calls with objects of this device.
        unsafe {
            self.ctx.OMSetRenderTargets(Some(std::slice::from_ref(&self.hdr.rtv)), self.dsv.as_ref());
            self.ctx.OMSetDepthStencilState(self.ds_light.as_ref(), 1);
            self.ctx.OMSetBlendState(self.bs_alpha.as_ref(), None, 0xFFFF_FFFF);
        }

        self.upload();
        self.fullscreen(&ps);
        // SAFETY: a state call.
        unsafe { self.ctx.OMSetBlendState(self.bs_opaque.as_ref(), None, 0xFFFF_FFFF) };
        null_ps_srvs(&self.ctx, 8);
    }

    /// The final image (or a debug view) into the caller's target, with alpha 1.
    pub(crate) fn pass_display(&mut self, target: &RenderTarget) {
        let (mode, src): (i32, _) = match self.view {
            View::Final => (3, self.final_.srv.clone()),
            View::HdrRaw => (1, self.hdr.srv.clone()),
            View::Albedo => (2, self.gb[1].srv.clone()),
            View::Normal => (3, self.gb[0].srv.clone()),
            View::Specular => (3, self.gb[2].srv.clone()),
            View::Depth => (4, self.gb[3].srv.clone()),
            View::SkyCube => (5, self.hdr.srv.clone()),
            View::Bloom => (1, self.bloom_a.t.srv.clone()),
            View::Quarter => (1, self.quarter.t.srv.clone()),
            View::ToneMapped => (1, self.ldr.t.srv.clone()),
            View::Ao => (3, self.ao_result.clone().or_else(|| self.ao.srv.clone())),
        };

        let mut dc = [0u8; 16];
        dc[0..4].copy_from_slice(&mode.to_le_bytes());
        dc[4..8].copy_from_slice(&1.0f32.to_le_bytes());

        // The SkyCube view shows the environment cube.
        let cube = if self.env_valid { self.env_cube_srv.clone() } else { self.sky_cube_srv.clone() };
        // SAFETY: state and drawing calls with objects of this device; the constant data is 16 bytes on the stack.
        unsafe {
            self.ctx.OMSetRenderTargets(Some(&[target.rtv().cloned()]), None);
            self.ctx.OMSetDepthStencilState(self.ds_off.as_ref(), 0);
            if let Some(cb) = &self.display_cb {
                self.ctx.UpdateSubresource(cb, 0, None, dc.as_ptr().cast(), 0, 0);
            }

            self.ctx.IASetInputLayout(None);
            self.ctx.IASetPrimitiveTopology(D3D11_PRIMITIVE_TOPOLOGY(D3D11_PRIMITIVE_TOPOLOGY_TRIANGLELIST));
            self.ctx.VSSetShader(self.fs_vs.as_ref(), None);
            self.ctx.PSSetShader(self.display_ps.as_ref(), None);
            self.ctx.PSSetConstantBuffers(0, Some(std::slice::from_ref(&self.display_cb)));
            self.ctx.PSSetShaderResources(0, Some(&[src, cube]));
            self.ctx.PSSetSamplers(0, Some(std::slice::from_ref(&self.s_clamp_linear)));
            self.ctx.Draw(3, 0);
        }
    }
}

/// A tuple of options that is only taken when a condition holds.
trait IntoIterIf: Sized {
    fn into_iter_if(self, condition: bool) -> Self;
}

impl<A, B> IntoIterIf for (Option<A>, Option<B>) {
    fn into_iter_if(self, condition: bool) -> Self {
        if condition { self } else { (None, None) }
    }
}

impl<A, B, C> IntoIterIf for (Option<A>, Option<B>, Option<C>) {
    fn into_iter_if(self, condition: bool) -> Self {
        if condition { self } else { (None, None, None) }
    }
}
