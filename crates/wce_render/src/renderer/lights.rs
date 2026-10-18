//! The point light pass: the scene's point lights drawn with the game's pointlightsmoothfalloff / pointlightlinearfalloff shaders (vs30_main,
//! ps30_slm_highdef), after the directional light, added into the light buffer.
//!
//! What the shaders do (from their code): the vertex shader places a unit volume (POSITION0) at the light, POSITION0 * radius + centre with
//! POSITION2 = (centre, radius), and passes POSITION1 (the colour) on. The pixel shader rebuilds the pixel's world position from the depth
//! channel through inv_view_projection (the screen uv through point_light_PS.g_viewport_offset_scale), and lights the G-buffer the way the
//! directional light does (diffuse and the specular lobe from the gloss channel, times the SSAO), with the attenuation saturate(1 - d / r),
//! squared for the smooth falloff; with g_hdr_on its diffuse term is multiplied by 0.004. The colour and radius are the tile's (BOB writes
//! them unchanged to point_light_marker.markers).
//!
//! The colour (verified, code): the light submission (fn 0x118d9680) passes colour * (colour_scale + colour_min) for LAT_NONE, colour *
//! (colour_scale * |sin(t * speed_x + phase)| + colour_min) for LAT_RADIUS_SIN and the same with |sin(t * speed_x + phase) * sin(t *
//! speed_y + phase)| for LAT_RADIUS_SIN_SIN (the "radius" animations modulate the colour), and the radius, to the add-light method
//! (fn 0x118dc6c0), which multiplies the colour by fn 0x11753690: MAX_HDR_SUN_COLOUR_SCALE (25000) * 0.2 = 5000 with HDR on, else 1; the
//! draw (fn 0x118da9a0) copies it into POSITION1 = (colour, 1) and POSITION2 = (centre, radius). No gamma conversion anywhere. Here t is
//! the frame clock's seconds; the phase is the game's (see the tile document's `light_phase`).
//!
//! The pipeline (the shader object's constructor, fn 0x116e3bc0): blend ONE / ONE add on colour and alpha with the write mask rgb (the alpha
//! of the light buffer keeps the directional light's refraction weight), cull mode 2 (front faces culled: the far side of the volume is
//! drawn, so each pixel is lit once and the camera may be inside it). Here the volume is a cube around the sphere, without the depth test
//! (the shader's attenuation ends at the radius), only where the stencil marks geometry, as the directional light.

use wce_foundation::wce_prof;
use wce_scene::environment::SUN_HDR_SCALE;
use wce_scene::render_scene::{Payload, RenderScene};

use super::*;
use crate::draw_set::Frustum;
use crate::shader_lib::{bind_sampler, bind_srv};

const SMOOTH_VS: &str = "dx11_sm_3_0_pointlightsmoothfalloff.hlsl_vs_vs30_main";
const SMOOTH_PS: &str = "dx11_sm_3_0_pointlightsmoothfalloff.hlsl_ps_ps30_slm_highdef";
const LINEAR_VS: &str = "dx11_sm_3_0_pointlightlinearfalloff.hlsl_vs_vs30_main";
const LINEAR_PS: &str = "dx11_sm_3_0_pointlightlinearfalloff.hlsl_ps_ps30_slm_highdef";

/// Floats of one instance: POSITION1 (colour), POSITION2 (centre, radius).
const INSTANCE_FLOATS: usize = 8;

#[derive(Default)]
pub(crate) struct LightGpu {
    cube_vb: Option<ID3D11Buffer>,
    cube_ib: Option<ID3D11Buffer>,
    inst_vb: Option<ID3D11Buffer>,
    inst_capacity: usize,
    rs: Option<ID3D11RasterizerState>,
    bs: Option<ID3D11BlendState>,
}

impl Renderer {

    fn make_light_resources(&mut self) {
        if self.lights.cube_vb.is_some() {
            return;
        }

        let dev = self.dev.clone();
        let l = &mut self.lights;
        let mut verts = vec![];
        for k in 0..8 {
            verts.extend([if k & 1 != 0 { 1.0f32 } else { -1.0 }, if k & 2 != 0 { 1.0 } else { -1.0 }, if k & 4 != 0 { 1.0 } else { -1.0 }]);
        }

        // Every face wound clockwise seen from outside (D3D's front faces): (b - a) x (c - a) points out.
        let idx: [u16; 36] = [0, 2, 3, 0, 3, 1, 4, 5, 7, 4, 7, 6, 0, 4, 6, 0, 6, 2, 1, 3, 7, 1, 7, 5, 0, 1, 5, 0, 5, 4, 2, 6, 7, 2, 7, 3];
        l.cube_vb = immutable_buffer(&dev, &f32_bytes(&verts), D3D11_BIND_VERTEX_BUFFER);
        l.cube_ib = immutable_buffer(&dev, &idx.iter().flat_map(|i| i.to_le_bytes()).collect::<Vec<_>>(), D3D11_BIND_INDEX_BUFFER);
        let rd = D3D11_RASTERIZER_DESC { FillMode: D3D11_FILL_SOLID, CullMode: D3D11_CULL_FRONT, DepthClipEnable: false.into(), ..Default::default() };
        l.rs = dev.create_rasterizer_state(&rd).ok();
        let rgb = (D3D11_COLOR_WRITE_ENABLE_RED | D3D11_COLOR_WRITE_ENABLE_GREEN | D3D11_COLOR_WRITE_ENABLE_BLUE) as u8;
        l.bs = crate::particles::blend_state(&dev, D3D11_RENDER_TARGET_BLEND_DESC {
            BlendEnable: true.into(), SrcBlend: D3D11_BLEND_ONE, DestBlend: D3D11_BLEND_ONE, BlendOp: D3D11_BLEND_OP_ADD,
            SrcBlendAlpha: D3D11_BLEND_ONE, DestBlendAlpha: D3D11_BLEND_ONE, BlendOpAlpha: D3D11_BLEND_OP_ADD, RenderTargetWriteMask: rgb,
        });
    }

    pub(crate) fn pass_point_lights(&mut self, scene: &RenderScene) {
        wce_prof!("render.point_lights");

        // The visible lights, the smooth ones first.
        let frustum = Frustum::from_view_projection(&self.camera.view_proj);
        let time = self.clock.last_time.max(0.0);
        let (mut smooth, mut linear) = (vec![], vec![]);
        for e in KindIndex::entities(&self.kinds.lights, scene) {
            let Payload::PointLight(l) = &e.payload else { continue };
            let c = [e.world.m[0][3], e.world.m[1][3], e.world.m[2][3]];
            if l.radius <= 0.0 || !scene.drawable(e) || frustum.outside(&c, l.radius) {
                continue;
            }

            let out = if l.smooth { &mut smooth } else { &mut linear };
            let wave = |speed: f32| (time * speed + l.phase).sin().abs();
            let level = match l.animation {
                1 => l.colour_scale * wave(l.speed[0]) + l.colour_min,
                2 => l.colour_scale * (wave(l.speed[0]) * wave(l.speed[1])) + l.colour_min,
                _ => l.colour_scale + l.colour_min,
            };

            let k = |v: f32| v * level * SUN_HDR_SCALE;
            out.extend([k(l.colour[0]), k(l.colour[1]), k(l.colour[2]), 1.0, c[0], c[1], c[2], l.radius]);
        }

        // The placed effects' lights (fire VFX): their colour already carries its level.
        for l in self.particles.effect_lights(self.camera.eye) {
            let c = [l.pos.x, l.pos.y, l.pos.z];
            if l.radius <= 0.0 || frustum.outside(&c, l.radius) {
                continue;
            }

            let out = if l.smooth { &mut smooth } else { &mut linear };
            out.extend([l.colour[0] * SUN_HDR_SCALE, l.colour[1] * SUN_HDR_SCALE, l.colour[2] * SUN_HDR_SCALE, 1.0, c[0], c[1], c[2], l.radius]);
        }

        let (n_smooth, n_linear) = ((smooth.len() / INSTANCE_FLOATS) as u32, (linear.len() / INSTANCE_FLOATS) as u32);
        if n_smooth + n_linear == 0 {
            return;
        }

        self.make_light_resources();
        let total = (n_smooth + n_linear) as usize;
        if total > self.lights.inst_capacity {
            self.lights.inst_capacity = (total * 2).max(1024);
            self.lights.inst_vb = dynamic_buffer(&self.dev, self.lights.inst_capacity * INSTANCE_FLOATS * 4, D3D11_BIND_VERTEX_BUFFER);
        }

        smooth.extend_from_slice(&linear);
        let Some(inst_vb) = self.lights.inst_vb.clone() else { return };
        if !self.ctx.map(&inst_vb, 0, D3D11_MAP_WRITE_DISCARD, 0).is_ok_and(|mut ms| ms.write(0, &smooth)) {
            return;
        }

        self.cbs.set_f("point_light_PS", "g_viewport_offset_scale", &[0.0, 0.0, 1.0, 1.0]);
        self.upload();
        self.ctx.set_render_targets(std::slice::from_ref(&self.gbuffer.hdr.rtv), self.gbuffer.dsv.as_ref());
        self.ctx.set_depth_stencil_state(self.states.ds_light.as_ref(), 1);
        self.ctx.set_rasterizer_state(self.lights.rs.as_ref());
        self.ctx.set_blend_state(self.lights.bs.as_ref());
        self.ctx.set_topology(D3D11_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
        self.ctx.set_vertex_buffers(0, &[self.lights.cube_vb.clone(), Some(inst_vb)], &[12, (INSTANCE_FLOATS * 4) as u32], &[0, 0]);
        self.ctx.set_index_buffer(self.lights.cube_ib.as_ref(), DXGI_FORMAT_R16_UINT, 0);
        let el = [
            element(c"POSITION", 0, DXGI_FORMAT_R32G32B32_FLOAT, 0, 0, false),
            element(c"POSITION", 1, DXGI_FORMAT_R32G32B32A32_FLOAT, 1, 0, true),
            element(c"POSITION", 2, DXGI_FORMAT_R32G32B32A32_FLOAT, 1, 16, true),
        ];

        for (vs, ps, count, first) in [(SMOOTH_VS, SMOOTH_PS, n_smooth, 0), (LINEAR_VS, LINEAR_PS, n_linear, n_smooth)] {
            if count == 0 {
                continue;
            }

            let (Some(vs), Some(ps)) = (self.shader(vs), self.shader(ps)) else { continue };
            let Some(layout) = self.layout(&vs, "point_light", &el) else { continue };
            self.ctx.set_input_layout(Some(&layout));
            self.bind(&vs);
            self.bind(&ps);
            let ctx = &self.ctx;
            null_ps_srvs(ctx, 16);
            for i in 0..4 {
                bind_srv(ctx, &ps, &format!("gbuffer_channel_{}_sampler", i + 1), self.gbuffer.channels[i].srv.as_ref());
            }

            bind_srv(ctx, &ps, "s_ssao", self.post.ao_result.as_ref().or(self.res.white_srv.as_ref()));
            ps_samplers(ctx, &ps, &self.states.s_clamp_point);
            bind_sampler(ctx, &ps, "s_ssao", self.states.s_clamp_linear.as_ref());
            ctx.draw_indexed_instanced(36, count, 0, 0, first);
            self.draw_stats.draw_calls += 1;
        }

        null_ps_srvs(&self.ctx, 16);
        self.ctx.set_blend_state(self.states.bs_opaque.as_ref());
        self.ctx.set_rasterizer_state(self.states.rs_back.as_ref());
    }
}
