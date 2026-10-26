//! Rome 2's terrain colour pipeline (docs/re/rome2_renderer.md, "Terrain"): its rigidterrain.hlsl has no texture arrays. The tiles' maps
//! are accumulated first, then textured per texture group in screen space:
//!
//! 1. surface: vs30_geom + ps30_geom, the terrain grid displaced by the LF height texture (its uv through `lf_block_cb`): z / w into the
//!    G-buffer's depth channel (+ hardware depth, stencil 2);
//! 2. per material (a tile's maps and its four texture groups): its boxes through vs30_blend + ps30_blend, which accumulate the tile's blend
//!    weights, index and normal times its alpha into three targets; then ps30_terrain_composite, full screen, textures the accumulated weights
//!    with the material's four groups (one diffuse and one normal texture each: `s_diffuse_r` .. `s_normal_a`) and adds albedo, normal and
//!    alpha into the terrain targets;
//! 3. resolve: vs30_terrain_to_gbuffer + ps30_terrain_to_gbuffer_ss, full screen: the terrain targets (+ the climate's mid-distance detail
//!    normal, the noise) become the G-buffer channels of the terrain pixels (stencil 2 -> 1).
//!
//! Open: the shaders never read the index map (ps30_blend writes it to a target the composite does not sample), so which of a group's four
//! variations the game binds is not known; the first one is used.

use wce_scene::render_scene::{Payload, Terrain};

use super::*;
use super::terrain::{INSTANCE_BYTES, box_in_frustum};
use crate::shader_lib::{bind_sampler, bind_srv};

pub(crate) const R2_GEOM_VS: &str = "dx11_sm_3_0_rigidterrain.hlsl_vs_vs30_geom";
pub(crate) const R2_GEOM_PS: &str = "dx11_sm_3_0_rigidterrain.hlsl_ps_ps30_geom";
pub(crate) const R2_SHADOW_VS: &str = "dx11_sm_3_0_rigidterrain.hlsl_vs_vs30_shadow";
const R2_BLEND_VS: &str = "dx11_sm_3_0_rigidterrain.hlsl_vs_vs30_blend";
const R2_BLEND_PS: &str = "dx11_sm_3_0_rigidterrain.hlsl_ps_ps30_blend";
const R2_COMPOSITE_VS: &str = "dx11_sm_3_0_rigidterrain.hlsl_vs_vs30_terrain_composite";
const R2_COMPOSITE_PS: &str = "dx11_sm_3_0_rigidterrain.hlsl_ps_ps30_terrain_composite";
const R2_RESOLVE_VS: &str = "dx11_sm_3_0_rigidterrain.hlsl_vs_vs30_terrain_to_gbuffer";
const R2_RESOLVE_PS: &str = "dx11_sm_3_0_rigidterrain.hlsl_ps_ps30_terrain_to_gbuffer_ss";

/// The terrain noise of the resolve (`s_noise_map`).
const NOISE_MAP: &str = "terrain/textures/common/terrain_noise.dds";

/// Floats of a blend box instance: P1 the tile's rectangle in the box (x0 z0 x1 z1), P2 the weights of the LF minimum and maximum added to
/// the height (unused: 0), P3 .. P5 the box's world rows, P6 the rotation index of the tile's maps; padded to 20.
const BOX_FLOATS: usize = 20;

/// The grid vertex (slot 0) and the instance's three world rows (slot 1). The shader also declares NORMAL0 and POSITION4, which it never
/// reads: they point at the instance too.
pub(crate) fn geom_layout() -> [InputElement; 6] {
    [
        element(c"POSITION", 0, DXGI_FORMAT_R32G32B32_FLOAT, 0, 0, false),
        element(c"NORMAL", 0, DXGI_FORMAT_R32G32B32A32_FLOAT, 1, 0, true),
        element(c"POSITION", 1, DXGI_FORMAT_R32G32B32A32_FLOAT, 1, 0, true),
        element(c"POSITION", 2, DXGI_FORMAT_R32G32B32A32_FLOAT, 1, 16, true),
        element(c"POSITION", 3, DXGI_FORMAT_R32G32B32A32_FLOAT, 1, 32, true),
        element(c"POSITION", 4, DXGI_FORMAT_R32_FLOAT, 1, 0, true),
    ]
}

/// The box vertex (slot 0) and the instance (slot 1, [`BOX_FLOATS`]).
fn box_layout() -> [InputElement; 7] {
    [
        element(c"POSITION", 0, DXGI_FORMAT_R32G32B32_FLOAT, 0, 0, false),
        element(c"POSITION", 1, DXGI_FORMAT_R32G32B32A32_FLOAT, 1, 0, true),
        element(c"POSITION", 2, DXGI_FORMAT_R32G32_FLOAT, 1, 16, true),
        element(c"POSITION", 3, DXGI_FORMAT_R32G32B32A32_FLOAT, 1, 24, true),
        element(c"POSITION", 4, DXGI_FORMAT_R32G32B32A32_FLOAT, 1, 40, true),
        element(c"POSITION", 5, DXGI_FORMAT_R32G32B32A32_FLOAT, 1, 56, true),
        element(c"POSITION", 6, DXGI_FORMAT_R32_FLOAT, 1, 72, true),
    ]
}

impl Renderer {

    /// Rome 2's terrain passes over the terrain entities (see the module).
    pub(crate) fn draw_terrain_rome2(&mut self, ents: &[&wce_scene::render_scene::Entity]) {
        let shaders = [R2_GEOM_VS, R2_GEOM_PS, R2_BLEND_VS, R2_BLEND_PS, R2_COMPOSITE_VS, R2_COMPOSITE_PS, R2_RESOLVE_VS, R2_RESOLVE_PS].map(|name| self.shader(name));
        let [Some(geom_vs), Some(geom_ps), Some(blend_vs), Some(blend_ps), Some(comp_vs), Some(comp_ps), Some(res_vs), Some(res_ps)] = shaders else {
            self.terrain.failed = true;
            return;
        };

        // The composite's targets: albedo + alpha, normal, gloss (Rome 2's terrain to G-buffer inputs).
        if self.terrain.r2_comp[0].tex.is_none() || self.terrain.r2_w != self.gbuffer.w || self.terrain.r2_h != self.gbuffer.h {
            for i in 0..3 {
                self.terrain.r2_comp[i] = make_target(&self.dev, self.gbuffer.w, self.gbuffer.h, DXGI_FORMAT_R16G16B16A16_FLOAT);
            }

            self.terrain.r2_w = self.gbuffer.w;
            self.terrain.r2_h = self.gbuffer.h;
        }

        for t in self.terrain.r2_comp.iter().filter_map(|t| t.rtv.as_ref()) {
            self.ctx.clear_render_target_view(t, [0.0; 4]);
        }

        // ---- 1. The surface.
        let Some(geom_il) = self.layout(&geom_vs, "terrain_geom_rome2", &geom_layout()) else { return };
        self.ctx.set_rasterizer_state(self.states.rs_none.as_ref());
        self.ctx.set_blend_state(self.states.bs_opaque.as_ref());
        self.ctx.set_input_layout(Some(&geom_il));
        self.ctx.set_topology(D3D11_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
        let zw_rtv = self.gbuffer.channels[3].rtv.clone();
        let ds_depth = self.terrain.ds_depth.clone();
        self.draw_terrain_surface(ents, &geom_vs, &geom_ps, &zw_rtv, &ds_depth, 2);

        // ---- 2. Per material: its boxes accumulated, then textured into the terrain targets.
        let Some(box_il) = self.layout(&blend_vs, "terrain_box_rome2", &box_layout()) else { return };
        let comp_el = [element(c"POSITION", 0, DXGI_FORMAT_R32G32B32A32_FLOAT, 0, 0, false), element(c"TEXCOORD", 0, DXGI_FORMAT_R32G32_FLOAT, 0, 16, false)];
        let Some(comp_il) = self.layout(&comp_vs, "terrain_composite_rome2", &comp_el) else { return };

        let (u, view_proj, o) = (self.camera.units, self.camera.main_view_proj, self.camera.origin);
        let mut by_material: Vec<(Arc<wce_scene::render_scene::TerrainMaterial>, Option<Arc<wce_scene::render_scene::LfMap>>, Vec<f32>)> = vec![];
        for e in ents {
            let Payload::Terrain(t) = &e.payload else { continue };
            let (Some(m), Some((_, b, rot))) = (t.material.clone(), self.terrain.placed.get(&e.id).and_then(|p| p.blend_box)) else { continue };
            if !box_in_frustum(&view_proj, [b[0], b[1], b[2]], [b[3], b[4], b[5]]) {
                continue;
            }

            // The unit box onto the tile's box (game units, from the render origin).
            let (x0, y0, z0) = ((b[0] - o.x) * u, (b[1] - o.y) * u, (b[2] - o.z) * u);
            let (sx, sy, sz) = ((b[3] - b[0]) * u, (b[4] - b[1]) * u, (b[5] - b[2]) * u);
            let inst = [0.0, 0.0, 1.0, 1.0, 0.0, 0.0, sx, 0.0, 0.0, x0, 0.0, sy, 0.0, y0, 0.0, 0.0, sz, z0, rot, 0.0];
            match by_material.iter_mut().find(|(mm, lf, _)| mm.id == m.id && lf.as_ref().map(|l| l.id) == t.lf.as_ref().map(|l| l.id)) {
                Some((_, _, v)) => v.extend(inst),
                None => by_material.push((m, t.lf.clone(), inst.to_vec())),
            }
        }

        let ctx = self.ctx.clone();
        let cbs = &mut self.cbs;
        cbs.set_f("vs_height_map_cb", "g_blend_lf_min_max", &[0.0, 0.0]);
        cbs.set_f("ps_height_map_cb", "g_normal_scales", &[1.0, 1.0, 1.0]);
        cbs.set_f("ps_height_map_cb", "g_far_tile_distance", &[1.0e9]);
        cbs.set_f("lf_block_cb", "g_lf_aabb", &[0.0, -1.0, 1.0, 1.0]);
        cbs.set_f("lf_block_cb", "g_normal_lf_aabb", &[0.0, 0.0, 1.0, 1.0]);
        let Some(inst_vb) = self.terrain.inst_vb.clone() else { return };
        for (m, lf, boxes) in by_material {
            let g = self.terrain.mat.get(&m.id).cloned().unwrap_or_default();

            // The accumulation of this material's tiles.
            for a in self.terrain.acc.iter().filter_map(|a| a.rtv.as_ref()) {
                ctx.clear_render_target_view(a, [0.0; 4]);
            }

            // The LF map's normals at uv = (x, -z) / its world size (as Attila's: lf_block_cb's rectangles do that: (x, z) / size, v = 1 - v,
            // then v - 1).
            let lf_normal = lf.as_ref().and_then(|lf| self.terrain.lf.get(&lf.id).and_then(|l| l.normal.srv.clone())).or_else(|| g.lf_normal.srv.clone());
            let world = lf.as_ref().filter(|lf| lf.normal.w > 0).map_or(1.0e6, |lf| lf.normal_world[0] * u);
            self.cbs.set_f("lf_block_cb", "g_ws_lf_aabb", &[0.0, 0.0, world, world]);
            let scale = m.pixel_scale.iter().copied().filter(|s| *s > 0.0).sum::<f32>() / m.pixel_scale.iter().filter(|s| **s > 0.0).count().max(1) as f32;
            self.cbs.set_f("ps_height_map_cb", "g_blend_pixel_scale", &[super::tiling(scale, super::ORIGIN_REACH * u)]);
            self.upload();

            let acc = self.terrain.acc.iter().map(|a| a.rtv.clone()).collect::<Vec<_>>();
            ctx.set_render_targets(&acc, self.gbuffer.dsv.as_ref());
            ctx.set_depth_stencil_state(self.terrain.ds_stencil.as_ref(), 2);
            ctx.set_blend_state(self.terrain.bs_add.as_ref());
            ctx.set_rasterizer_state(self.terrain.rs_front_cull.as_ref());
            ctx.set_input_layout(Some(&box_il));
            self.bind(&blend_vs);
            self.bind(&blend_ps);
            null_ps_srvs(&ctx, 16);
            ps_samplers(&ctx, &blend_ps, &self.states.s_wrap_aniso);
            bind_sampler(&ctx, &blend_ps, "s_alpha_map", self.states.s_clamp_aniso.as_ref());
            bind_sampler(&ctx, &blend_ps, "s_depth_map", self.states.s_clamp_point.as_ref());
            bind_srv(&ctx, &blend_ps, "s_depth_map", self.gbuffer.channels[3].srv.as_ref());
            bind_srv(&ctx, &blend_ps, "s_alpha_map", g.alpha.srv.as_ref());
            bind_srv(&ctx, &blend_ps, "s_blend_map", g.blend.srv.as_ref());
            bind_srv(&ctx, &blend_ps, "s_index_map", g.index.srv.as_ref());
            bind_srv(&ctx, &blend_ps, "s_normal_map", g.normal.srv.as_ref());
            bind_srv(&ctx, &blend_ps, "s_lf_normal_map", lf_normal.as_ref());
            ctx.set_vertex_buffers(0, &[self.terrain.box_vb.clone(), Some(inst_vb.clone())], &[12, (BOX_FLOATS * 4) as u32], &[0, 0]);
            ctx.set_index_buffer(self.terrain.box_ib.as_ref(), DXGI_FORMAT_R16_UINT, 0);
            for chunk in boxes.chunks(INSTANCE_BYTES / 4 / BOX_FLOATS * BOX_FLOATS) {
                if ctx.map(&inst_vb, 0, D3D11_MAP_WRITE_DISCARD, 0).is_ok_and(|mut ms| ms.write(0, chunk)) {
                    ctx.draw_indexed_instanced(36, (chunk.len() / BOX_FLOATS) as u32, 0, 0, 0);
                    self.draw_stats.draw_calls += 1;
                }
            }

            // The composite: the material's four groups (their first variation) over the accumulated weights, added into the terrain targets.
            let tex = |s: &mut Self, path: &str, srgb: bool| -> Option<ID3D11ShaderResourceView> {
                if path.is_empty() {
                    return None;
                }

                s.textures.get(path, srgb).and_then(|t| t.read().ok().and_then(|t| t.srv.clone()))
            };

            let diffuse: [Option<ID3D11ShaderResourceView>; 4] = std::array::from_fn(|c| tex(self, &m.diffuse[4 * c], true));
            let normals: [Option<ID3D11ShaderResourceView>; 4] = std::array::from_fn(|c| tex(self, &m.normal_tex[4 * c], false));
            let comp = self.terrain.r2_comp.iter().map(|t| t.rtv.clone()).collect::<Vec<_>>();
            ctx.set_render_targets(&comp, None);
            ctx.set_depth_stencil_state(self.states.ds_off.as_ref(), 0);
            ctx.set_blend_state(self.terrain.bs_add.as_ref());
            ctx.set_rasterizer_state(self.states.rs_none.as_ref());
            ctx.set_input_layout(Some(&comp_il));
            self.bind(&comp_vs);
            self.bind(&comp_ps);
            null_ps_srvs(&ctx, 16);
            ps_samplers(&ctx, &comp_ps, &self.states.s_wrap_aniso);
            for n in ["s_depth_map", "s_blend_map", "s_normal_map"] {
                bind_sampler(&ctx, &comp_ps, n, self.states.s_clamp_point.as_ref());
            }

            bind_srv(&ctx, &comp_ps, "s_depth_map", self.gbuffer.channels[3].srv.as_ref());
            bind_srv(&ctx, &comp_ps, "s_blend_map", self.terrain.acc[0].srv.as_ref());
            bind_srv(&ctx, &comp_ps, "s_normal_map", self.terrain.acc[2].srv.as_ref());
            let flat = g.lf_normal.srv.clone();
            for (c, name) in ["r", "g", "b", "a"].iter().enumerate() {
                bind_srv(&ctx, &comp_ps, &format!("s_diffuse_{name}"), diffuse[c].as_ref().or(self.res.white_srv.as_ref()));
                bind_srv(&ctx, &comp_ps, &format!("s_normal_{name}"), normals[c].as_ref().or(flat.as_ref()));
            }

            ctx.set_vertex_buffers(0, std::slice::from_ref(&self.terrain.resolve_vb), &[24], &[0]);
            ctx.set_topology(D3D11_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
            ctx.draw(6, 0);
            self.draw_stats.draw_calls += 1;
            null_ps_srvs(&ctx, 16);
        }

        // ---- 3. The resolve: the terrain pixels into the G-buffer.
        let Some(m) = ents.iter().find_map(|e| match &e.payload {
            Payload::Terrain(Terrain { material: Some(m), .. }) => Some(m.clone()),
            _ => None,
        }) else {
            return;
        };

        let el = [element(c"POSITION", 0, DXGI_FORMAT_R32G32B32A32_FLOAT, 0, 0, false), element(c"TEXCOORD", 0, DXGI_FORMAT_R32G32_FLOAT, 0, 16, false)];
        let Some(il) = self.layout(&res_vs, "terrain_resolve_rome2", &el) else { return };
        let rtvs = self.gbuffer.channels.iter().map(|t| t.rtv.clone()).collect::<Vec<_>>();
        ctx.set_render_targets(&rtvs, self.gbuffer.dsv.as_ref());
        ctx.set_depth_stencil_state(self.terrain.ds_resolve.as_ref(), 2);
        ctx.set_blend_state(self.states.bs_opaque.as_ref());
        ctx.set_rasterizer_state(self.states.rs_none.as_ref());
        ctx.set_input_layout(Some(&il));

        // ps_height_map_cb as Rome 2's battle database's render parameters say (mid-distance detail: far 256 m, repeat 400 m, strength 0.6;
        // its normal strength 1; the slope 0.96); the noise off (scale 0, offset 1).
        let cbs = &mut self.cbs;
        cbs.set_f("ps_height_map_cb", "g_mid_distance_details", &[256.0 * u, 400.0 * u, 3.0, 0.6]);
        cbs.set_f("ps_height_map_cb", "g_mid_distance_normal_strength", &[1.0]);
        cbs.set_f("ps_height_map_cb", "g_mid_distance_slope", &[0.96, 3.0]);
        cbs.set_f("ps_height_map_cb", "g_noise_params", &[0.0, 1.0, 1.0, 0.0]);
        cbs.set_f("shroud_PS", "g_shroud_enabled", &[0.0]);
        self.upload();
        self.bind(&res_vs);
        self.bind(&res_ps);
        null_ps_srvs(&ctx, 16);
        let detail = if m.detail_normal.is_empty() { None } else { self.textures.get(&m.detail_normal, false) };
        let detail = detail.and_then(|t| t.read().ok().and_then(|t| t.srv.clone())).or_else(|| self.terrain.mat.get(&m.id).and_then(|g| g.lf_normal.srv.clone()));
        let noise = self.textures.get(NOISE_MAP, false).and_then(|t| t.read().ok().and_then(|t| t.srv.clone())).or_else(|| self.res.white_srv.clone());
        ps_samplers(&ctx, &res_ps, &self.states.s_wrap_aniso);
        for n in ["s_albedo_map", "s_terrain_normal_map", "s_terrain_gloss_map", "s_depth_map"] {
            bind_sampler(&ctx, &res_ps, n, self.states.s_clamp_point.as_ref());
        }

        bind_sampler(&ctx, &res_ps, "s_shroud", self.states.s_clamp_linear.as_ref());
        bind_srv(&ctx, &res_ps, "s_albedo_map", self.terrain.r2_comp[0].srv.as_ref());
        bind_srv(&ctx, &res_ps, "s_terrain_normal_map", self.terrain.r2_comp[1].srv.as_ref());
        bind_srv(&ctx, &res_ps, "s_terrain_gloss_map", self.terrain.r2_comp[2].srv.as_ref());
        bind_srv(&ctx, &res_ps, "s_depth_map", self.gbuffer.channels[3].srv.as_ref());
        bind_srv(&ctx, &res_ps, "s_mid_distance_detail_map", detail.as_ref());
        bind_srv(&ctx, &res_ps, "s_noise_map", noise.as_ref());
        bind_srv(&ctx, &res_ps, "s_shroud", self.res.white_srv.as_ref());
        ctx.set_vertex_buffers(0, std::slice::from_ref(&self.terrain.resolve_vb), &[24], &[0]);
        ctx.set_topology(D3D11_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
        ctx.draw(6, 0);
        self.draw_stats.draw_calls += 1;
        null_ps_srvs(&ctx, 16);
        ctx.set_rasterizer_state(self.states.rs_back.as_ref());
        ctx.set_blend_state(self.states.bs_opaque.as_ref());
    }
}

