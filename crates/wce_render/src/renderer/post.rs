//! After lighting: SSAO, the tone mapping chain (with its auto exposure), hdr_to_screen, the lens flare and the debug display; and the
//! depth-range readback the shadow cascades are fitted to.

use std::rc::Rc;

use wce_foundation::math::Float3;
use wce_scene::camera::Camera;
use wce_scene::environment::{EnvironmentParams, build_scurve};

use super::*;
use super::targets::{SizedTarget, make_target};
use crate::render_target::RenderTarget;
use crate::shader_lib::{bind_sampler, bind_srv};

// The lens flare (LENSE_FLARE_SHADER, fn 0x116de600): four sun sprites and a vignette, drawn after hdr_to_screen.
const FLARE_VS: [&str; 4] = ["dx11_sm_3_0_lenseflare.hlsl_vs_vs30_flare_0", "dx11_sm_3_0_lenseflare.hlsl_vs_vs30_flare_1", "dx11_sm_3_0_lenseflare.hlsl_vs_vs30_flare_2", "dx11_sm_3_0_lenseflare.hlsl_vs_vs30_flare_3"];
const FLARE_PS: [&str; 4] = ["dx11_sm_3_0_lenseflare.hlsl_ps_ps30_flare_0", "dx11_sm_3_0_lenseflare.hlsl_ps_ps30_flare_1", "dx11_sm_3_0_lenseflare.hlsl_ps_ps30_flare_2", "dx11_sm_3_0_lenseflare.hlsl_ps_ps30_flare_3"];
const FLARE_MAIN_VS: &str = "dx11_sm_3_0_lenseflare.hlsl_vs_vs30_main";
const VIGNETTE_PS: &str = "dx11_sm_3_0_lenseflare.hlsl_ps_ps30_vignette";

/// The auto exposure of the tone mapping chain: the black / white points (two 1x1 targets, previous / current, plus the CPU readback for
/// the constant tone mapper) and the S-curve texture.
#[derive(Clone, Debug)]
pub(crate) struct ToneMap {
    pub points: [SizedTarget; 2],
    pub staging: [Option<ID3D11Texture2D>; READBACK_RING],
    pub cur: usize,
    pub luma_black: f32,
    pub luma_white: f32,
    pub scurve_tex: Option<ID3D11Texture2D>,
    pub scurve_srv: Option<ID3D11ShaderResourceView>,
    pub scurve_last: [u16; 256 * 4],

    /// The last points read back, and how many reads there were.
    pub debug_points: [f32; 4],
    pub debug_maps: i32,
}

impl ToneMap {
    pub const BLACK: f32 = 0.001;
    pub const WHITE: f32 = 9.0;

    pub fn new(dev: &ID3D11Device) -> Self {
        let staging = D3D11_TEXTURE2D_DESC {
            Width: 1, Height: 1, MipLevels: 1, ArraySize: 1, Format: DXGI_FORMAT_R32G32B32A32_FLOAT, SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
            Usage: D3D11_USAGE_STAGING, BindFlags: 0, CPUAccessFlags: D3D11_CPU_ACCESS_READ as u32, MiscFlags: 0,
        };

        let sd = D3D11_TEXTURE2D_DESC {
            Width: 256, Height: 1, MipLevels: 1, ArraySize: 1, Format: DXGI_FORMAT_R16G16B16A16_FLOAT, SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
            Usage: D3D11_USAGE_DEFAULT, BindFlags: D3D11_BIND_SHADER_RESOURCE as u32, CPUAccessFlags: 0, MiscFlags: 0,
        };

        let scurve_tex = dev.create_texture_2d(&sd, None).ok();
        Self {
            points: std::array::from_fn(|_| SizedTarget { t: make_target(dev, 1, 1, DXGI_FORMAT_R32G32B32A32_FLOAT), w: 1, h: 1 }),
            staging: std::array::from_fn(|_| dev.create_texture_2d(&staging, None).ok()),
            cur: 0,
            luma_black: Self::BLACK,
            luma_white: Self::WHITE,
            scurve_srv: scurve_tex.as_ref().and_then(|tex| dev.create_shader_resource_view(tex, None).ok()),
            scurve_tex,
            scurve_last: [0; 256 * 4],
            debug_points: [0.0; 4],
            debug_maps: 0,
        }
    }
}

/// The Depth Buffer Analysis Shadow Map View equivalent: the min / max view depth of the visible geometry, read back from the G-buffer's
/// depth channel through two alternating staging textures.
#[derive(Clone, Debug)]
pub(crate) struct DepthRange {
    pub staging: [Option<ID3D11Texture2D>; 2],
    pub w: u32,
    pub h: u32,
    pub frame: u64,
    pub valid: bool,
    pub min: f32,
    pub max: f32,
}

impl Default for DepthRange {
    fn default() -> Self {
        Self { staging: Default::default(), w: 0, h: 0, frame: 0, valid: false, min: 1.0, max: 781.0 }
    }
}

impl Renderer {

    /// Unbinds the outputs (D3D11 drops an input whose resource is a bound render target), binds a shader and gives every sampler slot one state.
    fn bind_post(&self, s: &GameShader, smp: &Option<ID3D11SamplerState>) {
        self.ctx.set_render_targets(&[], None);
        self.bind(s);
        ps_samplers(&self.ctx, s, smp);
    }

    pub(crate) fn pass_tonemap(&mut self, env: &EnvironmentParams, dt: f32) {
        let rome2 = self.game == super::game::RenderGame::Rome2;
        // Rome 2: the statistics in log Y (its own shaders for steps 1 and 2), the S-curve from the single tones biases.
        let names = if rome2 {
            [TM_R2_DOWN, TM_R2_STATS, TM_R2_STATS_DOWN, TM_POINTS, TM_HIGH_PASS, TM_BLUR_X, TM_BLUR_Y, TM_TONE_MAP]
        } else {
            [TM_DOWN_LUMA, TM_LUMA_STATS, TM_STATS_DOWN, TM_POINTS, TM_HIGH_PASS, TM_BLUR_X, TM_BLUR_Y, TM_TONE_MAP]
        };
        let shaders = names.map(|name| self.shader(name));
        let [Some(down), Some(stats), Some(stats_down), Some(points), Some(high), Some(bx), Some(by), Some(tm)] = shaders else { return };
        let log_y = if rome2 { self.shader(TM_R2_LOG_Y) } else { None };
        if rome2 && log_y.is_none() {
            return;
        }

        self.ctx.set_depth_stencil_state(self.states.ds_off.as_ref(), 0);
        self.ctx.set_rasterizer_state(self.states.rs_none.as_ref());

        // 1. A quarter-size copy with the luma in .w (Rome 2: the colour only).
        self.bind_post(&down, &self.states.s_clamp_linear.clone());
        bind_srv(&self.ctx, &down, "g_hdr_rgb_texture_sampler", self.gbuffer.hdr.srv.as_ref());
        let quarter = self.post.quarter.clone();
        self.fullscreen_to(&quarter.t.rtv, quarter.w, quarter.h, &down);

        // Rome 2: the quarter image as log Y, x, y (into the second bloom target, free until the bloom).
        let mut src = self.post.quarter.t.srv.clone();
        if let Some(log_y) = &log_y {
            let target = self.post.bloom_b.clone();
            self.bind_post(log_y, &self.states.s_clamp_point.clone());
            bind_srv(&self.ctx, log_y, "g_hdr_rgb_texture_sampler", src.as_ref());
            self.fullscreen_to(&target.t.rtv, target.w, target.h, log_y);
            src = target.t.srv;
        }

        // 2. The luma statistics pyramid down to 1x1 (min, max, average), clamped by the absolute points.
        let (first_in, next_in) = if rome2 { ("g_hdr_logYxy_texture_sampler", "g_logY_values_texture_sampler") } else { ("g_luma_values_texture_sampler", "g_luma_stats_texture_sampler") };
        for i in 0..self.post.stats.len() {
            let s = if i == 0 { &stats } else { &stats_down };
            self.bind_post(s, &self.states.s_clamp_point.clone());
            bind_srv(&self.ctx, s, if i == 0 { first_in } else { next_in }, src.as_ref());
            let target = self.post.stats[i].clone();
            self.fullscreen_to(&target.t.rtv, target.w, target.h, s);
            src = target.t.srv;
        }

        // 3. The black / white points with temporal adaptation (prev -> cur); the first frame snaps.
        let (prev, cur) = (self.tonemap.cur, 1 - self.tonemap.cur);
        self.cbs.set_f("auto_exposure_buffer", "g_auto_exposure_dt", &[if self.clock.frame == 0 { 1000.0 } else { dt }]);
        self.bind_post(&points, &self.states.s_clamp_point.clone());
        bind_srv(&self.ctx, &points, "g_prev_black_and_white_points_sampler", self.tonemap.points[prev].t.srv.as_ref());
        bind_srv(&self.ctx, &points, if rome2 { "g_logY_values_texture_sampler" } else { "g_luma_stats_texture_sampler" }, self.post.stats.last().and_then(|s| s.t.srv.as_ref()));
        let point_rtv = self.tonemap.points[cur].t.rtv.clone();
        self.fullscreen_to(&point_rtv, 1, 1, &points);
        self.tonemap.cur = cur;

        // The constant tone mapper takes the points from a cbuffer: the previous frames' result is read back through a ring of staging copies;
        // the oldest one (READBACK_RING - 1 frames back) is finished on the GPU.
        if let (Some(staging), Some(tex)) = (&self.tonemap.staging[(self.clock.frame % READBACK_RING as u64) as usize], &self.tonemap.points[cur].t.tex) {
            self.ctx.copy_resource(staging, tex);
        }

        if self.clock.frame + 1 >= READBACK_RING as u64
            && let Some(staging) = self.tonemap.staging[((self.clock.frame + 1) % READBACK_RING as u64) as usize].clone()
            && let Some(p) = map_read(&self.ctx, &staging, 0, true).and_then(|ms| ms.read::<[f32; 4]>())
        {
            self.tonemap.debug_points = p;
            self.tonemap.debug_maps += 1;
            if p[1] > p[0] {
                self.tonemap.luma_black = p[0];
                self.tonemap.luma_white = p[1];
            }
        }

        self.clock.frame += 1;

        // 4. Bloom: a high pass on the quarter image, a separable 11-tap blur.
        let (bloom_a, bloom_b) = (self.post.bloom_a.clone(), self.post.bloom_b.clone());
        self.bind_post(&high, &self.states.s_clamp_point.clone());
        bind_srv(&self.ctx, &high, "g_black_and_white_points_sampler", self.tonemap.points[cur].t.srv.as_ref());
        bind_srv(&self.ctx, &high, "g_hdr_rgb_texture_sampler", self.post.quarter.t.srv.as_ref());
        self.fullscreen_to(&bloom_a.t.rtv, bloom_a.w, bloom_a.h, &high);
        self.bind_post(&bx, &self.states.s_clamp_linear.clone());
        bind_srv(&self.ctx, &bx, "g_hdr_rgb_texture_sampler", bloom_a.t.srv.as_ref());
        self.fullscreen_to(&bloom_b.t.rtv, bloom_b.w, bloom_b.h, &bx);
        self.bind_post(&by, &self.states.s_clamp_linear.clone());
        bind_srv(&self.ctx, &by, "g_hdr_rgb_texture_sampler", bloom_b.t.srv.as_ref());
        self.fullscreen_to(&bloom_a.t.rtv, bloom_a.w, bloom_a.h, &by);

        // 5. The tone map, with bloom and the S-curve (Rome 2: of the single tones biases, one curve for the three channels).
        let curve = match env.tones_bias_single.filter(|_| rome2) {
            Some([low, high]) => build_scurve(&EnvironmentParams { low_tones_bias: [low; 3], high_tones_bias: [high; 3], ..env.clone() }),
            None => build_scurve(env),
        };
        if curve != self.tonemap.scurve_last {
            if let Some(tex) = &self.tonemap.scurve_tex {
                let _ = self.ctx.update_subresource(tex, 0, None, bytemuck::cast_slice(&curve), 256 * 8, 0);
            }

            self.tonemap.scurve_last = curve;
        }

        self.bind_post(&tm, &self.states.s_clamp_linear.clone());
        bind_sampler(&self.ctx, &tm, "g_hdr_rgb_texture_sampler", self.states.s_clamp_point.as_ref());
        bind_sampler(&self.ctx, &tm, "g_black_and_white_points_sampler", self.states.s_clamp_point.as_ref());
        bind_srv(&self.ctx, &tm, "g_black_and_white_points_sampler", self.tonemap.points[cur].t.srv.as_ref());
        bind_srv(&self.ctx, &tm, "g_hdr_rgb_texture_sampler", self.gbuffer.hdr.srv.as_ref());
        bind_srv(&self.ctx, &tm, "g_hdr_rgb_bloom_texture_sampler", bloom_a.t.srv.as_ref());
        bind_srv(&self.ctx, &tm, "g_scurve_texture_sampler", self.tonemap.scurve_srv.as_ref());
        let ldr = self.post.ldr.clone();
        self.fullscreen_to(&ldr.t.rtv, ldr.w, ldr.h, &tm);
    }

    pub(crate) fn pass_to_screen(&mut self, env: &EnvironmentParams) {
        let mut distortion = if self.settings.distortion { self.particles.distortion().cloned() } else { None };
        let dof = env.use_dof && self.settings.depth_of_field;
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
                    e[0] = (x - n / 2) as f32 / self.gbuffer.w as f32;
                    e[1] = (y - n / 2) as f32 / self.gbuffer.h as f32;
                    e[2] = (weight(x, y) / sum) as f32;
                    e[3] = 1.0;
                }
            }

            self.cbs.set_f("hdr_to_screen_PS", name, &d);
        }

        // Battle: no fog of war.
        self.cbs.set_f("shared_fog_of_war_PS", "g_fog_of_war_blend", &[0.0, 0.0]);
        self.ctx.set_render_targets(&[], None);
        self.bind(&ps);
        ps_samplers(&self.ctx, &ps, &self.states.s_clamp_point);
        bind_srv(&self.ctx, &ps, "frame_sampler", self.post.ldr.t.srv.as_ref());
        bind_srv(&self.ctx, &ps, "gbuffer_channel_4_sampler", self.gbuffer.channels[3].srv.as_ref());
        bind_srv(&self.ctx, &ps, "s_fog_of_war_mask", self.res.black_srv.as_ref());

        // Thrones of Britannia's adds 0.1 x the god rays' red (gfx_god_rays; docs/re/thrones_renderer.md), which the editor doesn't draw:
        // black, as the game with them off (the slot is empty after `fullscreen_to` too; this keeps it so whatever the passes before).
        bind_srv(&self.ctx, &ps, "god_rays_sampler", self.res.black_srv.as_ref());
        if let Some(distortion) = &distortion {
            bind_srv(&self.ctx, &ps, "distortion_sampler", Some(distortion));
            bind_sampler(&self.ctx, &ps, "distortion_sampler", self.states.s_clamp_linear.as_ref());
            bind_sampler(&self.ctx, &ps, "frame_sampler", self.states.s_clamp_linear.as_ref());
        }

        let (rtv, w, h) = (self.post.final_.rtv.clone(), self.gbuffer.w, self.gbuffer.h);
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
        self.cbs.set_f("lense_flare_VS", "sun_occlusion", &[self.sun.visibility]);
        self.cbs.set_f("lense_flare_VS", "sun_position", &[sun.x, sun.y, sun.z]);
        self.cbs.set_f("lense_flare_VS", "sun_flare", &[flare]);
        self.cbs.set_f("lense_flare_VS", "pos_transform", &[1.0, 1.0, 0.0, 0.0]);
        self.cbs.set_f("lense_flare_PS", "time", &[time_s]);
        self.cbs.set_f("lense_flare_PS", "half_texel_offset", &[0.5 / self.gbuffer.w as f32, 0.5 / self.gbuffer.h as f32]);
        self.cbs.set_f("lense_flare_PS", "vignette_strength", &[1.0]);
        self.cbs.set_f("lense_flare_PS", "sun_flare_ps", &[flare]);
        self.upload();
        let el = [
            element(c"POSITION", 0, DXGI_FORMAT_R32G32B32A32_FLOAT, 0, 0, false),
            element(c"COLOR", 0, DXGI_FORMAT_R32G32B32A32_FLOAT, 0, 16, false),
            element(c"TEXCOORD", 0, DXGI_FORMAT_R32G32_FLOAT, 0, 32, false),
        ];

        self.ctx.set_render_targets(std::slice::from_ref(&self.post.final_.rtv), None);
        self.ctx.set_viewports(&[viewport(0.0, 0.0, self.gbuffer.w as f32, self.gbuffer.h as f32)]);
        self.ctx.set_depth_stencil_state(self.states.ds_off.as_ref(), 0);
        self.ctx.set_rasterizer_state(self.states.rs_none.as_ref());
        self.ctx.set_vertex_buffers(0, std::slice::from_ref(&self.res.flare_vb), &[40], &[0]);
        self.ctx.set_topology(D3D11_PRIMITIVE_TOPOLOGY_TRIANGLELIST);

        if flare > 0.0 && self.sun.visibility > 0.0 {
            let textures = ["commontextures/green_dot_flare.dds", "commontextures/streaks_flare.dds", "commontextures/green_dot_flare.dds", "commontextures/rays_flare.dds"];
            let samplers = ["lense_flare_1_sampler", "lense_flare_2_sampler", "lense_flare_1_sampler", "lense_flare_3_sampler"];
            self.ctx.set_blend_state(self.states.bs_add_one.as_ref());
            for (i, (vs, ps)) in flares.into_iter().enumerate() {
                let (Some(vs), Some(ps)) = (vs, ps) else { continue };
                let Some(layout) = self.layout(&vs, "flare", &el) else { continue };
                self.ctx.set_input_layout(Some(&layout));
                self.bind(&vs);
                self.bind(&ps);

                // lense_flare_1..3_sampler have the engine's sRGB-read flag (sampler +0x3c, fn 0x116de600).
                let srv = self.sky_texture(textures[i], true);
                bind_srv(&self.ctx, &ps, samplers[i], srv.as_ref());
                bind_sampler(&self.ctx, &ps, samplers[i], self.states.s_clamp_linear.as_ref());
                self.ctx.draw(6, 0);
            }
        }

        // The vignette (blend object +0x71f: SRC_ALPHA / INV_SRC_ALPHA, black with alpha = saturate(2 r - 0.9)^1.7).
        let Some(layout) = self.layout(&main_vs, "flare", &el) else { return };
        self.ctx.set_blend_state(self.states.bs_alpha.as_ref());
        self.ctx.set_input_layout(Some(&layout));

        self.bind(&main_vs);
        self.bind(&vignette);
        self.ctx.draw(6, 0);
        self.ctx.set_blend_state(self.states.bs_opaque.as_ref());

        null_ps_srvs(&self.ctx, 4);
    }

    /// The Depth Buffer Analysis Shadow Map View equivalent: the min / max view depth of the visible geometry from the G-buffer depth channel
    /// (z / w), read back two frames later. Sky pixels (z / w = 0 after the clear) are skipped.
    pub(crate) fn analyse_depth(&mut self, cam: &Camera) {
        if self.depth_range.staging[0].is_none() || self.depth_range.w != self.gbuffer.w || self.depth_range.h != self.gbuffer.h {
            let sd = D3D11_TEXTURE2D_DESC {
                Width: self.gbuffer.w, Height: self.gbuffer.h, MipLevels: 1, ArraySize: 1, Format: DXGI_FORMAT_R32_FLOAT, SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
                Usage: D3D11_USAGE_STAGING, BindFlags: 0, CPUAccessFlags: D3D11_CPU_ACCESS_READ as u32, MiscFlags: 0,
            };

            for t in &mut self.depth_range.staging {
                *t = self.dev.create_texture_2d(&sd, None).ok();
            }

            self.depth_range.w = self.gbuffer.w;
            self.depth_range.h = self.gbuffer.h;
            self.depth_range.frame = 0;
        }

        if let (Some(staging), Some(depth)) = (&self.depth_range.staging[(self.depth_range.frame & 1) as usize], &self.gbuffer.channels[3].tex) {
            self.ctx.copy_resource(staging, depth);
        }

        let first = self.depth_range.frame == 0;
        self.depth_range.frame += 1;
        if first {
            return;
        }

        let Some(src) = self.depth_range.staging[(self.depth_range.frame & 1) as usize].clone() else { return };
        let Some(ms) = map_read(&self.ctx, &src, 0, self.settings.deterministic) else { return };
        let (mut lo, mut hi) = (3.4e38f32, 0.0f32);
        for y in (0..self.gbuffer.h as usize).step_by(2) {
            let Some(row) = ms.row(y, self.gbuffer.w as usize * 4) else { break };
            for texel in row.as_chunks::<4>().0.iter().step_by(2) {
                let d = f32::from_ne_bytes(*texel);
                if d.partial_cmp(&0.0) != Some(std::cmp::Ordering::Greater) || d >= 1.0 {
                    continue;
                }

                let z = cam.linear_depth(d);
                lo = lo.min(z);
                hi = hi.max(z);
            }
        }

        drop(ms);
        self.depth_range.valid = hi > 0.0;
        if self.depth_range.valid {
            self.depth_range.min = lo;
            self.depth_range.max = hi;
        }
    }

    pub(crate) fn pass_ssao(&mut self, cam: &Camera) {
        let res = self.shader(SSAO_PS);
        let bx = self.shader(SSAO_BLUR_X_PS);
        let by = self.shader(SSAO_BLUR_Y_PS);
        // Not for an orthographic view: the HBAO resolve rebuilds view positions with the perspective's focal length.
        let (Some(res), Some(bx), Some(by)) = (res, bx, by).into_iter_if(self.settings.ssao && !cam.is_ortho()) else {
            if let Some(rtv) = &self.post.ao.rtv {
                self.ctx.clear_render_target_view(rtv, [1.0; 4]);
            }

            self.post.ao_result = self.post.ao.srv.clone();
            return;
        };

        // ssao_resolve_PS / ssao_blur_PS as in the memory dump (HBAO: 5 directions x 5 steps).
        let cbs = &mut self.cbs;
        cbs.set_f("ssao_resolve_PS", "g_R", &[1.0]);
        cbs.set_f("ssao_resolve_PS", "g_ExternalAngleBias", &[0.0]);
        cbs.set_f("ssao_resolve_PS", "g_Attenuation", &[0.78]);
        cbs.set_f("ssao_resolve_PS", "g_ExternalContrast", &[1.0]);
        cbs.set_f("ssao_resolve_PS", "g_AOResolution", &[self.gbuffer.w as f32, self.gbuffer.h as f32]);
        cbs.set_f("ssao_resolve_PS", "g_VerticalFieldOfView", &[cam.fov_y]);
        cbs.set_f("ssao_resolve_PS", "g_Strength", &[3.0]);
        cbs.set_f("ssao_resolve_PS", "g_MinStrength", &[0.17]);
        cbs.set_f("ssao_resolve_PS", "g_JitterResolution", &[256.0]);
        cbs.set_f("ssao_resolve_PS", "g_NumDir", &[5.0]);
        cbs.set_f("ssao_resolve_PS", "g_NumSteps", &[5.0]);
        cbs.set_f("ssao_blur_PS", "g_BlurRadius", &[4.0]);
        cbs.set_f("ssao_blur_PS", "g_BlurSharpness", &[3.0]);

        // Dump: half the screen size.
        cbs.set_f("ssao_blur_PS", "g_BlurResolution", &[self.gbuffer.w as f32 * 0.5, self.gbuffer.h as f32 * 0.5]);
        self.ctx.set_depth_stencil_state(self.states.ds_off.as_ref(), 0);
        self.ctx.set_rasterizer_state(self.states.rs_none.as_ref());

        let (w, h) = (self.gbuffer.w, self.gbuffer.h);
        let (ao, ao_tmp, ao_blur) = (self.post.ao.clone(), self.post.ao_tmp.clone(), self.post.ao_blur.clone());
        self.ssao_run(&res, None, &ao, w, h);
        self.ssao_run(&bx, ao.srv.clone(), &ao_tmp, w / 2, h / 2);
        self.ssao_run(&by, ao_tmp.srv.clone(), &ao_blur, w / 2, h / 2);
        self.post.ao_result = ao_blur.srv;
    }

    fn ssao_run(&mut self, ps: &Rc<GameShader>, src: Option<ID3D11ShaderResourceView>, dst: &Target, tw: u32, th: u32) {
        self.ctx.set_render_targets(&[], None);
        self.bind(ps);
        ps_samplers(&self.ctx, ps, &self.states.s_clamp_point);
        bind_sampler(&self.ctx, ps, "jitter_texture_sampler", self.states.s_wrap_point.as_ref());
        bind_sampler(&self.ctx, ps, "resolve_target_sampler", self.states.s_clamp_linear.as_ref());

        // blur_y's name for its input.
        bind_sampler(&self.ctx, ps, "blur_x_target_sampler", self.states.s_clamp_linear.as_ref());
        bind_srv(&self.ctx, ps, "gbuffer_channel_4_sampler", self.gbuffer.channels[3].srv.as_ref());
        bind_srv(&self.ctx, ps, "jitter_texture_sampler", self.res.jitter_srv.as_ref());
        if let Some(src) = &src {
            bind_srv(&self.ctx, ps, "resolve_target_sampler", Some(src));
            bind_srv(&self.ctx, ps, "blur_x_target_sampler", Some(src));
        }

        self.ctx.set_render_targets(std::slice::from_ref(&dst.rtv), None);
        self.ctx.set_viewports(&[viewport(0.0, 0.0, tw as f32, th as f32)]);

        self.upload();
        self.ctx.set_input_layout(None);
        self.ctx.set_topology(D3D11_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
        self.ctx.vs_set_shader(self.res.fs_vs2.as_ref());

        self.bind(ps);
        self.ctx.draw(3, 0);
        null_ps_srvs(&self.ctx, 4);
    }

    /// The final image (or a debug view) into the caller's target, with alpha 1.
    pub(crate) fn pass_display(&mut self, target: &RenderTarget) {
        let (mode, src): (i32, _) = match self.settings.view {
            View::Final => (3, self.post.final_.srv.clone()),
            View::HdrRaw => (1, self.gbuffer.hdr.srv.clone()),
            View::Albedo => (2, self.gbuffer.channels[1].srv.clone()),
            View::Normal => (3, self.gbuffer.channels[0].srv.clone()),
            View::Specular => (3, self.gbuffer.channels[2].srv.clone()),
            View::Depth => (4, self.gbuffer.channels[3].srv.clone()),
            View::SkyCube => (5, self.gbuffer.hdr.srv.clone()),
            View::Bloom => (1, self.post.bloom_a.t.srv.clone()),
            View::Quarter => (1, self.post.quarter.t.srv.clone()),
            View::ToneMapped => (1, self.post.ldr.t.srv.clone()),
            View::Ao => (3, self.post.ao_result.clone().or_else(|| self.post.ao.srv.clone())),
        };

        let mut dc = [0u8; 16];
        dc[0..4].copy_from_slice(&mode.to_le_bytes());
        dc[4..8].copy_from_slice(&1.0f32.to_le_bytes());

        // The SkyCube view shows the environment cube.
        let cube = if self.env.valid { self.env.cube.srv.clone() } else { self.sky.cube.srv.clone() };
        self.ctx.set_render_targets(&[target.rtv().cloned()], None);
        self.ctx.set_depth_stencil_state(self.states.ds_off.as_ref(), 0);
        if let Some(cb) = &self.res.display_cb {
            let _ = self.ctx.update_subresource(cb, 0, None, &dc, 0, 0);
        }

        self.ctx.set_input_layout(None);
        self.ctx.set_topology(D3D11_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
        self.ctx.vs_set_shader(self.res.fs_vs.as_ref());
        self.ctx.ps_set_shader(self.res.display_ps.as_ref());
        self.ctx.ps_set_constant_buffers(0, std::slice::from_ref(&self.res.display_cb));
        self.ctx.ps_set_shader_resources(0, &[src, cube]);
        self.ctx.ps_set_samplers(0, std::slice::from_ref(&self.states.s_clamp_linear));
        self.ctx.draw(3, 0);
    }
}
