//! Rome 2's planar water reflection: the texture its sea (`ocean.hlsl` `reflection_texture_sampler`), water planes and rivers
//! (`s_terrain_reflection`) read. The ocean's pixel shader takes the sky's reflection from it alone (times its Fresnel term and 0.65; the
//! sky cube only fogs it), sampled at the pixel's screen position turned upside down, `(x, 1 - y)` plus the normal's distortion: the view
//! of a camera at the eye mirrored in the water's plane, looking at the mirrored target with +y up, whose picture is the mirror image
//! upside down.
//!
//! ASSUMPTIONS (no capture of Rome 2's frame): the game renders the mirrored scene itself (it has `_reflection` techniques of the
//! directional light and the terrain); this pass draws the sky dome and the cloud planes only, at half the screen's size, so the sea
//! reflects the sky and not the land or buildings. The water's plane is the sea's level.

use wce_scene::camera::Camera;
use wce_scene::environment::EnvironmentParams;

use super::targets::GBuffer;
use super::*;

/// The reflection's size: a fraction of the screen's.
const REFLECTION_DIV: u32 = 2;

/// The reflection's targets (the sky and clouds draw into a G-buffer's light buffer, swapped with the main view's while they render).
#[derive(Clone, Debug, Default)]
pub(crate) struct WaterReflection {
    pub gbuffer: GBuffer,

    /// The texture holds this frame's reflection.
    pub valid: bool,
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
}
