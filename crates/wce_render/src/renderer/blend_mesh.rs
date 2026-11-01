//! Terrain-blend meshes (RS_TERRAIN_BLEND_V5: the gaul_ / attila_ rocks): geometry that blends with the terrain material by its vertex colour alpha.
//!
//! The game's `RIGID_TERRAIN_BLEND_SHADER` (`rigidterrainblend.hlsl`) has these entries: `vs30_geom` + `ps30_geom` (view-space depth, the same number the terrain's
//! `ps_depth` writes), `vs30_main` + `ps30_gbuffer` (the G-buffer pass of the geometry), `vs30_main` + `ps_terrain_material` (the mesh's own material, added to the
//! terrain's accumulation targets with the weight `10 * vertex alpha`) and `vs30_shadow` + `ps30_shadow`. In the terrain pass the blend boxes project the terrain
//! textures onto every pixel of the terrain stencil, rocks included (they mark the stencil like the surface does), so where the vertex alpha is 0 the rock shows the
//! terrain, and where it is large the rock's own diffuse wins. The resolve then writes the sum into the G-buffer.

use super::*;
use crate::draw_set::{DrawSet, Visible};
use crate::model_cache::TEX_SLOTS;
use crate::shader_lib::bind_srv;

pub(crate) const BLEND_VS: &str = "dx11_sm_3_0_rigidterrainblend.hlsl_vs_vs30_main";
pub(crate) const BLEND_GEOM_VS: &str = "dx11_sm_3_0_rigidterrainblend.hlsl_vs_vs30_geom";
pub(crate) const BLEND_GBUFFER_PS: &str = "dx11_sm_3_0_rigidterrainblend.hlsl_ps_ps30_gbuffer";
pub(crate) const BLEND_GEOM_PS: &str = "dx11_sm_3_0_rigidterrainblend.hlsl_ps_ps30_geom";
pub(crate) const BLEND_MATERIAL_PS: &str = "dx11_sm_3_0_rigidterrainblend.hlsl_ps_ps_terrain_material";

/// One draw of the terrain-blend meshes: the shaders, what they draw into and how.
pub(crate) struct BlendDraw<'a> {
    /// The depth-stencil: the scene's (MS with MSAA) for the G-buffer pass, the one-sample one for the terrain's own targets.
    pub dsv: Option<ID3D11DepthStencilView>,
    pub vs: &'a str,
    pub ps: &'a str,
    pub targets: Vec<Option<ID3D11RenderTargetView>>,
    pub depth_stencil: Option<ID3D11DepthStencilState>,
    pub stencil_ref: u32,
    pub blend: Option<ID3D11BlendState>,
}

impl Renderer {

    /// Draws the terrain-blend meshes among the visible batches of `sets` (the instances are already in the ring: `firsts`).
    pub(crate) fn draw_blend_meshes(&mut self, sets: [&DrawSet; 2], viss: [&Visible; 2], firsts: [u32; 2], d: &BlendDraw) {
        if !sets.iter().zip(viss).any(|(s, v)| v.active.iter().any(|&b| s.batch_mesh(b).kind == MeshKind::TerrainBlend)) {
            return;
        }

        let (Some(vs), Some(ps)) = (self.shader(d.vs), self.shader(d.ps)) else { return };
        let Some(layout) = self.layout(&vs, "rigid_std", &rigid_layout()) else { return };
        self.ctx.set_render_targets(&d.targets, d.dsv.as_ref());
        self.ctx.set_depth_stencil_state(d.depth_stencil.as_ref(), d.stencil_ref);
        self.ctx.set_blend_state(d.blend.as_ref());
        self.ctx.set_rasterizer_state(self.states.rs_back.as_ref());
        self.ctx.set_input_layout(Some(&layout));
        self.ctx.set_topology(D3D11_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
        self.ctx.set_vertex_buffers(1, std::slice::from_ref(&self.draw_lists.ring.buffer), &[(size_of::<RigidInstance>() as u32)], &[0]);
        self.bind(&vs);
        for &slot in vs.textures.values() {
            self.ctx.vs_set_shader_resources(slot, std::slice::from_ref(&self.res.instance_ids_srv));
        }

        self.bind(&ps);
        let snow = self.snow_normals_srv();
        bind_srv(&self.ctx, &ps, "s_snow_normals", snow.as_ref());
        ps_samplers(&self.ctx, &ps, &self.states.s_wrap_aniso);
        let slots = self.slots_of(&ps);
        let (instances, triangles) = self.draw_batches(sets, viss, firsts, |r, m| {
            if m.kind != MeshKind::TerrainBlend {
                return false;
            }

            for (t, slot) in slots.iter().enumerate().take(TEX_SLOTS) {
                let Some(slot) = slot else { continue };
                let srv = m.tex[t].read().ok().and_then(|tex| tex.srv.clone());
                r.ctx.ps_set_shader_resources(*slot, &[srv]);
            }

            true
        });

        self.draw_stats.triangles += triangles;
        self.draw_stats.camera_triangles += triangles;
        self.draw_stats.camera_instances += instances;
        null_ps_srvs(&self.ctx, 16);
    }
}
