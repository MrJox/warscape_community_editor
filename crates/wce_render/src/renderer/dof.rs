//! Rome 2's depth of field (gfx_depth_of_field 2; a RenderDoc capture of a battle at Carthage, 2026-11-01): after the tone mapper,
//! `depth_of_field.hlsl` `pixel_dof_coc` copies the tone-mapped image into an RGBA16F target with the circle of confusion in its alpha,
//! `0.6 * g_coc_blur_kernel_scale * (1 - exp(-(z - min(g_focus_distance, 500))^2 / (2 * g_focal_length^2)))` of the view depth z; then
//! `depth_of_field_cs.hlsl` blurs it by that alpha, `cs_dof_horizontal_blur` into a second target, `cs_dof_vertical_blur` back (groups
//! of 32 x 2 threads covering 128 pixels along the blur: (w / 128, h / 2) and (w / 2, h / 128) groups); `hdr_to_screen` (without its own
//! depth of field) reads the result. The battle camera's constants: focus 0, focal length 1000, kernel scale 1 (the distance blurs), or
//! the environment's DOF block when it has one.

use super::*;
use crate::shader_lib::{bind_sampler, bind_srv};

const COC_PS: &str = "dx11_sm_3_0_depth_of_field.hlsl_ps_pixel_dof_coc";
const BLUR_H_CS: &str = "dx11_default_depth_of_field_cs.hlsl_cs_cs_dof_horizontal_blur";
const BLUR_V_CS: &str = "dx11_default_depth_of_field_cs.hlsl_cs_cs_dof_vertical_blur";

/// A target the compute blurs write.
#[derive(Clone, Debug, Default)]
pub(crate) struct UavTarget {
    pub t: Target,
    pub uav: Option<ID3D11UnorderedAccessView>,
    pub w: u32,
    pub h: u32,
}

fn uav_target(dev: &ID3D11Device, w: u32, h: u32) -> UavTarget {
    let td = D3D11_TEXTURE2D_DESC {
        Width: w, Height: h, MipLevels: 1, ArraySize: 1, Format: DXGI_FORMAT_R16G16B16A16_FLOAT, SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
        Usage: D3D11_USAGE_DEFAULT, BindFlags: (D3D11_BIND_RENDER_TARGET | D3D11_BIND_SHADER_RESOURCE | D3D11_BIND_UNORDERED_ACCESS) as u32, CPUAccessFlags: 0, MiscFlags: 0,
    };

    let Ok(tex) = dev.create_texture_2d(&td, None) else { return UavTarget::default() };
    let res: ID3D11Resource = tex.clone().into();
    UavTarget {
        t: Target { rtv: dev.create_render_target_view(&tex, None).ok(), srv: dev.create_shader_resource_view(&tex, None).ok(), tex: Some(tex) },
        uav: dev.create_unordered_access_view(&res, None).ok(),
        w,
        h,
    }
}

#[derive(Clone, Debug, Default)]
pub(crate) struct DofTargets {
    pub a: UavTarget,
    pub b: UavTarget,
}

impl Renderer {
    /// The depth-of-field image of the tone-mapped one (Rome 2), `None` when the shaders or targets are missing.
    pub(crate) fn pass_dof_rome2(&mut self, env: &EnvironmentParams) -> Option<ID3D11ShaderResourceView> {
        let (coc, bh, bv) = (self.shader(COC_PS)?, self.shader(BLUR_H_CS)?, self.shader(BLUR_V_CS)?);
        wce_prof!("render.dof");
        let (w, h) = (self.post.ldr.w, self.post.ldr.h);
        if self.dof.a.w != w || self.dof.a.h != h {
            self.dof = DofTargets { a: uav_target(&self.dev, w, h), b: uav_target(&self.dev, w, h) };
        }

        let (focus, focal, kernel) = if env.use_dof { (env.dof_focus[0], env.dof_focus[1], env.dof_blur_size) } else { (0.0, 1000.0, 1.0) };
        self.cbs.set_f("depth_of_field_coc_PS", "g_focus_distance", &[focus]);
        self.cbs.set_f("depth_of_field_coc_PS", "g_focal_length", &[focal]);
        self.cbs.set_f("depth_of_field_coc_PS", "g_coc_blur_kernel_scale", &[kernel]);

        // 1. The circle of confusion.
        let (a, b) = (self.dof.a.clone(), self.dof.b.clone());
        self.ctx.set_depth_stencil_state(self.states.ds_off.as_ref(), 0);
        self.ctx.set_blend_state(self.states.bs_opaque.as_ref());
        self.ctx.set_rasterizer_state(self.states.rs_none.as_ref());
        self.ctx.set_render_targets(std::slice::from_ref(&a.t.rtv), None);
        self.bind(&coc);
        bind_srv(&self.ctx, &coc, "input_sampler", self.post.ldr.t.srv.as_ref());
        bind_srv(&self.ctx, &coc, "gbuffer_channel_4_sampler", self.gbuffer.depth_srv().as_ref());
        bind_srv(&self.ctx, &coc, "distortion_sampler", self.res.black_srv.as_ref());
        bind_sampler(&self.ctx, &coc, "input_sampler", self.states.s_clamp_point.as_ref());
        bind_sampler(&self.ctx, &coc, "gbuffer_channel_4_sampler", self.states.s_clamp_point.as_ref());
        bind_sampler(&self.ctx, &coc, "distortion_sampler", self.states.s_wrap_linear.as_ref());
        self.fullscreen_to(&a.t.rtv, w, h, &coc);
        self.ctx.set_render_targets(&[], None);
        null_ps_srvs(&self.ctx, 8);

        // 2. The blurs, a -> b -> a.
        for (cs, src, dst, groups) in [(&bh, &a, &b, (w.div_ceil(128), h.div_ceil(2))), (&bv, &b, &a, (w.div_ceil(2), h.div_ceil(128)))] {
            self.upload();
            self.bind(cs);
            bind_srv(&self.ctx, cs, "input_texture_SM5", src.t.srv.as_ref());
            if let Some(&slot) = cs.uavs.get("output_texture") {
                self.ctx.cs_set_unordered_access_views(slot, std::slice::from_ref(&dst.uav));
            }

            self.ctx.dispatch(groups.0, groups.1, 1);
            self.ctx.cs_set_unordered_access_views(0, &[None]);
            self.ctx.cs_set_shader_resources(0, &[None]);
        }

        self.ctx.cs_set_shader(None);
        a.t.srv.clone()
    }
}
