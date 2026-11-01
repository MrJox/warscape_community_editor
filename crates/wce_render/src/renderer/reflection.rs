//! Rome 2's planar water reflection: the texture its sea (`ocean.hlsl` `reflection_texture_sampler`), water planes and rivers
//! (`s_terrain_reflection`) read. The ocean's pixel shader takes the sky's reflection from it alone (times its Fresnel term and 0.65; the
//! sky cube only fogs it), sampled at the pixel's screen position turned upside down, `(x, 1 - y)` plus the normal's distortion: the view
//! of a camera at the eye mirrored in the water's plane, looking at the mirrored target with +y up, whose picture is the mirror image
//! upside down.
//!
//! As the game (a RenderDoc capture of a battle at Carthage, 2026-11-01, `rome2_carthage.rdc`): the mirrored camera (its projection
//! oblique at the water) draws the sky dome and the cloud planes into a half-size target, before the G-buffer; the land and buildings come
//! later, from the screen-space reflection (`helpers.hlsl` `scree_space_reflections_ps`, after the water's refraction copy, before the
//! sea): for each pixel the scene point under it (the depth), the water's height there, the view ray reflected off it and marched through
//! the depth buffer; the scene copy at the hit, faded towards the screen's edges, with the march's length and with the camera's height
//! over the water (none above 250 units), blended over the sky by its alpha. Not done here: the game's fog pass over the mirrored sky
//! (fog_VS_PS with the mirrored camera, alpha blended) and the oblique near plane (no geometry is drawn mirrored). The water's plane is the
//! sea's level.

use wce_scene::camera::Camera;
use wce_scene::environment::EnvironmentParams;

use super::targets::GBuffer;
use super::*;
use crate::shader_lib::{bind_sampler, bind_srv};

/// The reflection's size: a fraction of the screen's (the game's: half).
const REFLECTION_DIV: u32 = 2;

const SSR_PS: &str = "dx11_sm_3_0_helpers.hlsl_ps_scree_space_reflections_ps";

/// screen_space_refl_params as the capture has them: the water height map's 2048-unit square around the battle's centre, its heights
/// between 0 and 1, the quality factor of the highest water quality (the march takes 200 * it steps).
const SSR_PLAYABLE_AREA: f32 = 2048.0;
const SSR_QUALITY: f32 = 0.8;

/// The reflection's targets (the sky and clouds draw into a G-buffer's light buffer, swapped with the main view's while they render).
#[derive(Clone, Debug, Default)]
pub(crate) struct WaterReflection {
    pub gbuffer: GBuffer,

    /// The texture holds this frame's reflection.
    pub valid: bool,

    /// The screen-space reflection's water height map: one texel, water (alpha 1) at height 0 everywhere. The game's is 256 x 256 over
    /// the playable area, alpha marking the water; its only effect is to skip the pixels with no water under them, whose reflection the sea
    /// never reads.
    pub water_height: Option<ID3D11ShaderResourceView>,
}

impl Renderer {
    /// The reflection texture for this frame's water (black when there is none).
    pub(crate) fn water_reflection_srv(&self) -> Option<ID3D11ShaderResourceView> {
        if self.reflection.valid { self.reflection.gbuffer.hdr.srv.clone() } else { self.res.black_srv.clone() }
    }

    /// Draws the reflection of the sky in the sea's plane as `cam` sees it (Rome 2 only); the caller sets its own camera again after.
    pub(crate) fn render_water_reflection(&mut self, env: &EnvironmentParams, cam: &Camera, width: u32, height: u32, time_s: f32) {
        self.reflection.valid = false;
        if self.game != game::RenderGame::Rome2 {
            return;
        }

        let (w, h) = ((width / REFLECTION_DIV).max(8), (height / REFLECTION_DIV).max(8));
        if self.reflection.gbuffer.w != w || self.reflection.gbuffer.h != h {
            self.reflection.gbuffer = GBuffer::new(&self.dev, w, h);
        }

        wce_prof!("render.water_reflection");
        let plane = self.camera.origin.y;
        let mirror = |p: Float3| Float3::new(p.x, 2.0 * plane - p.y, p.z);
        let mirrored = Camera { eye: mirror(cam.eye), target: mirror(cam.target), up: Float3::new(0.0, 1.0, 0.0), ..*cam };
        let view = mirrored.view_from(self.camera.origin);
        let proj = mirrored.projection(width as f32 / height as f32);

        std::mem::swap(&mut self.gbuffer, &mut self.reflection.gbuffer);
        let save_dt = std::mem::take(&mut self.cloud_layer.dt);
        self.set_camera(&view, &proj, mirrored.eye, w, h, time_s);
        self.upload();
        self.ctx.set_viewports(&[viewport(0.0, 0.0, w as f32, h as f32)]);
        self.ctx.set_blend_state(self.states.bs_opaque.as_ref());
        if let Some(dsv) = &self.gbuffer.dsv {
            self.ctx.clear_depth_stencil_view(dsv, D3D11_CLEAR_DEPTH | D3D11_CLEAR_STENCIL, 1.0, 0);
        }

        if let Some(rtv) = &self.gbuffer.hdr.rtv {
            self.ctx.clear_render_target_view(rtv, [0.0; 4]);
        }

        self.pass_sky_forward();
        self.pass_clouds(env);
        self.ctx.set_render_targets(&[], None);
        self.cloud_layer.dt = save_dt;
        std::mem::swap(&mut self.gbuffer, &mut self.reflection.gbuffer);
        self.reflection.valid = true;
    }

    /// The screen-space reflection of the scene over the mirrored sky (Rome 2): after the refraction copy (its input, the scene before the
    /// water) and before the sea, with the view's camera.
    pub(crate) fn water_reflection_ssr(&mut self) {
        if !self.reflection.valid || self.game != game::RenderGame::Rome2 {
            return;
        }

        let Some(ps) = self.shader(SSR_PS) else { return };
        wce_prof!("render.water_reflection_ssr");
        if self.reflection.water_height.is_none() {
            self.reflection.water_height = solid_srv(&self.dev, DXGI_FORMAT_R8G8B8A8_UNORM, &[0, 0, 0, 255]);
        }

        let (vp, eye) = (self.camera.cb_view_proj, self.camera.cb_eye);
        let cbs = &mut self.cbs;
        cbs.set_m("screen_space_refl_params", "view_projection_matrix", vp.data());
        cbs.set_m("screen_space_refl_params", "inv_view_projection_matrix", inverse(&vp).data());
        cbs.set_f("screen_space_refl_params", "camera_position_and_reflection_height", &[eye.x, eye.y, eye.z, 0.0]);
        cbs.set_f("screen_space_refl_params", "min_water_height", &[0.0]);
        cbs.set_f("screen_space_refl_params", "max_water_height", &[1.0]);
        cbs.set_f("screen_space_refl_params", "tiles_per_dim", &[256.0]);
        cbs.set_f("screen_space_refl_params", "playable_area_size", &[SSR_PLAYABLE_AREA]);
        cbs.set_f("screen_space_refl_params", "quality_factor", &[SSR_QUALITY]);

        let (w, h) = (self.reflection.gbuffer.w, self.reflection.gbuffer.h);
        null_ps_srvs(&self.ctx, 16);
        self.ctx.set_render_targets(std::slice::from_ref(&self.reflection.gbuffer.hdr.rtv), None);
        self.ctx.set_viewports(&[viewport(0.0, 0.0, w as f32, h as f32)]);
        self.ctx.set_depth_stencil_state(self.states.ds_off.as_ref(), 0);
        self.ctx.set_rasterizer_state(self.states.rs_none.as_ref());
        self.ctx.set_blend_state(self.states.bs_alpha.as_ref());
        self.upload();
        bind_srv(&self.ctx, &ps, "scene_tex", self.water.refr.srv.as_ref());
        bind_srv(&self.ctx, &ps, "depth_tex", self.gbuffer.depth_srv().as_ref());
        bind_srv(&self.ctx, &ps, "water_height_tex", self.reflection.water_height.as_ref());
        bind_sampler(&self.ctx, &ps, "scene_tex", self.states.s_clamp_linear.as_ref());
        bind_sampler(&self.ctx, &ps, "depth_tex", self.states.s_clamp_point.as_ref());
        bind_sampler(&self.ctx, &ps, "water_height_tex", self.states.s_clamp_point.as_ref());
        self.fullscreen(&ps);
        null_ps_srvs(&self.ctx, 16);
        self.ctx.set_blend_state(self.states.bs_opaque.as_ref());
        self.ctx.set_viewports(&[viewport(0.0, 0.0, self.gbuffer.w as f32, self.gbuffer.h as f32)]);
    }
}
