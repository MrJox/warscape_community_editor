//! The scene's moving ropes (`Payload::Ropes`: a simulated sail's ropes, `cloth_sim`): drawn into the G-buffer after the units, through
//! `rope.hlsl`'s world technique (vs30_main: the rope's vertices in game units about the render origin, no instance; ps30_main_gbuffer, the
//! rigid technique's pixel shader without the instance's burn). Their strips are made again every frame on the CPU, one dynamic vertex and
//! index buffer for all of them. Not in the shadows (the rope depth techniques aren't wired) nor the id pass.

use wce_d3d11::*;
use wce_foundation::math::Float3;
use wce_scene::render_scene::{Payload, RenderScene};

use super::{KindIndex, Renderer, dynamic_buffer, element};
use crate::model_cache::{ROPE_STRIDE, rope_geometry};
use crate::shader_lib::bind_srv;

pub(crate) const ROPE_WORLD_VS: &str = "dx11_sm_3_0_rope.hlsl_vs_vs30_main";
pub(crate) const ROPE_WORLD_PS: &str = "dx11_sm_3_0_rope.hlsl_ps_ps30_main_gbuffer";

/// The frame's ropes on the GPU: the buffers (grown when needed) and per entity its indices and textures.
#[derive(Default)]
pub(crate) struct RopeDraws {
    vb: Option<ID3D11Buffer>,
    vb_bytes: usize,
    ib: Option<ID3D11Buffer>,
    ib_bytes: usize,
    draws: Vec<(u32, u32, String, String)>,
}

/// The world technique's vertex: [`ROPE_STRIDE`] bytes, as the rigid one's (POSITION0, NORMAL0, TEXCOORD0..2), no instance.
fn rope_world_layout() -> Vec<InputElement> {
    vec![
        element(c"POSITION", 0, DXGI_FORMAT_R32G32B32_FLOAT, 0, 0, false),
        element(c"NORMAL", 0, DXGI_FORMAT_R32G32B32_FLOAT, 0, 12, false),
        element(c"TEXCOORD", 0, DXGI_FORMAT_R32G32_FLOAT, 0, 24, false),
        element(c"TEXCOORD", 1, DXGI_FORMAT_R32G32_FLOAT, 0, 32, false),
        element(c"TEXCOORD", 2, DXGI_FORMAT_R32G32_FLOAT, 0, 40, false),
    ]
}

impl Renderer {
    /// The ropes' strips of this frame, in game units about the render origin (after the camera is set).
    pub(crate) fn prepare_ropes(&mut self, scene: &RenderScene) {
        self.ropes.draws.clear();
        if self.kinds.ropes.is_empty() {
            return;
        }

        let (u, o) = (self.camera.units, self.camera.origin);
        let (mut vertices, mut indices): (Vec<u8>, Vec<u32>) = (vec![], vec![]);
        for e in KindIndex::entities(&self.kinds.ropes, scene) {
            let Payload::Ropes(r) = &e.payload else { continue };
            if !scene.drawable(e) {
                continue;
            }

            let first = indices.len() as u32;
            for line in &r.lines {
                let nodes: Vec<[f32; 3]> = line.nodes.iter().map(|p| {
                    let w = e.world.transform_point(Float3::new(p[0], p[1], p[2]));
                    [(w.x - o.x) * u, (w.y - o.y) * u, (w.z - o.z) * u]
                }).collect();
                let Some((v, i, ..)) = rope_geometry(&nodes, line.radius) else { continue };
                let base = (vertices.len() / ROPE_STRIDE as usize) as u32;
                vertices.extend_from_slice(&v);
                indices.extend(i.iter().map(|&k| base + u32::from(k)));
            }

            let count = indices.len() as u32 - first;
            if count > 0 {
                self.ropes.draws.push((first, count, r.diffuse.clone(), r.normal.clone()));
            }
        }

        if self.ropes.draws.is_empty() {
            return;
        }

        let index_bytes: Vec<u8> = indices.iter().flat_map(|i| i.to_le_bytes()).collect();
        for (bytes, buffer, size, bind) in [(&vertices, &mut self.ropes.vb, &mut self.ropes.vb_bytes, D3D11_BIND_VERTEX_BUFFER), (&index_bytes, &mut self.ropes.ib, &mut self.ropes.ib_bytes, D3D11_BIND_INDEX_BUFFER)] {
            if bytes.len() > *size || buffer.is_none() {
                *size = bytes.len().next_power_of_two().max(1 << 14);
                *buffer = dynamic_buffer(&self.dev, *size, bind);
            }

            let written = buffer.as_ref().is_some_and(|b| self.ctx.map(b, 0, D3D11_MAP_WRITE_DISCARD, 0).is_ok_and(|mut m| m.write(0, bytes)));
            if !written {
                self.ropes.draws.clear();
                return;
            }
        }
    }

    /// Draws the frame's ropes into the bound G-buffer.
    pub(crate) fn draw_ropes_gbuffer(&mut self) {
        if self.ropes.draws.is_empty() {
            return;
        }

        let (Some(vs), Some(ps)) = (self.shader(ROPE_WORLD_VS), self.shader(ROPE_WORLD_PS)) else { return };
        let Some(layout) = self.layout(&vs, "rope_world", &rope_world_layout()) else { return };
        self.upload();
        self.ctx.set_input_layout(Some(&layout));
        self.bind(&vs);
        self.bind(&ps);
        self.ctx.set_rasterizer_state(self.states.rs_none.as_ref());
        self.ctx.set_depth_stencil_state(self.states.ds_gbuffer.as_ref(), 1);
        self.ctx.set_vertex_buffers(0, std::slice::from_ref(&self.ropes.vb), &[ROPE_STRIDE], &[0]);
        self.ctx.set_index_buffer(self.ropes.ib.as_ref(), DXGI_FORMAT_R32_UINT, 0);
        super::ps_samplers(&self.ctx, &ps, &self.states.s_wrap_aniso);
        bind_srv(&self.ctx, &ps, "s_fire_map", self.res.black_srv.as_ref());
        let draws = std::mem::take(&mut self.ropes.draws);
        for (first, count, diffuse, normal) in &draws {
            let srv = |path: &str, srgb: bool| self.textures.get(path, srgb).and_then(|t| t.read().ok().and_then(|t| t.srv.clone()));
            let (d, n) = (srv(diffuse, true), srv(normal, false));
            bind_srv(&self.ctx, &ps, "s_diffuse_map", d.as_ref().or(self.res.white_srv.as_ref()));
            bind_srv(&self.ctx, &ps, "s_normal_map", n.as_ref().or(self.res.black_srv.as_ref()));
            self.ctx.draw_indexed(*count, *first, 0);
            self.draw_stats.draw_calls += 1;
        }

        self.ropes.draws = draws;
        self.ctx.set_rasterizer_state(self.states.rs_back.as_ref());
    }
}
