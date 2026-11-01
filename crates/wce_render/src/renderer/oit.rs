//! Order-independent transparency (gfx_alpha_blend; Attila's and Thrones of Britannia's OIT views, docs/re/graphics_options.md): the
//! near leaves and grass, left out of the G-buffer, drawn lit with the forward OIT shaders into per-pixel fragment lists, then sorted
//! and blended over the light buffer.

use std::rc::Rc;

use wce_foundation::wce_prof;
use wce_scene::render_scene::RenderScene;

use super::*;
use crate::draw_set::Visible;
use crate::model_cache::MeshKind;
use crate::shader_lib::{GameShader, bind_sampler, bind_srv};

impl Renderer {
    /// The forward-lit shaders' lighting inputs beyond the constant buffers the light passes filled: the environment cube, the sky, the
    /// cascaded shadow map with its comparison sampler, the snow normals.
    pub(crate) fn bind_forward_inputs(&mut self, ps: &Rc<GameShader>) {
        let env = if self.env.valid { self.env.cube.srv.clone() } else { self.sky.cube.srv.clone() };
        let snow = self.snow_normals_srv();
        let ctx = &self.ctx;
        bind_srv(ctx, ps, "s_environment", env.as_ref());
        bind_srv(ctx, ps, "s_sky", self.sky.cube.srv.as_ref());
        bind_srv(ctx, ps, "g_tShadowBuffer_SM50", self.shadow_map.srv.as_ref());
        bind_srv(ctx, ps, "s_snow_normals", snow.as_ref());
        bind_sampler(ctx, ps, "s_environment", self.states.s_clamp_linear.as_ref());
        bind_sampler(ctx, ps, "s_sky", self.states.s_clamp_linear.as_ref());
        bind_sampler(ctx, ps, "sShadowBuffer_SM50", self.states.s_shadow_cmp.as_ref());
    }

    /// The order-independent transparency, after the lighting and fog, before the blended forward meshes (the game's OIT_PARENT, fn
    /// 0x11738a00, is filed before the DEFERRED_ALPHA views):
    /// - OIT_STORE_ALPHA_FRAGMENTS (fn 0x116e2910): the heads cleared to 0xffffffff (fn 0x11729000); the near leaves (rigidleaf_v3
    ///   `ps30_forward_lit_OIT`) and grass (rigidgrass `ps30_forward_lit_OIT_grass`) lit, each fragment appended to its pixel's list
    ///   (depth-tested against the scene without writing; the shaders force the early depth test).
    /// - OIT_SORT_ALPHA_FRAGMENTS_RESOLVE_COLOUR (fn 0x116e25f0): `ps50_sort_alpha_fragments_resolve_colour` sorts each pixel's fragments
    ///   by depth (64 at most) and composites them front to back, blended ONE / INV_SRC_ALPHA over the light buffer.
    ///
    /// The game's first-pass / post-first-pass depth-stencil states (fns 0x116e2470, 0x116e2580: stencil ALWAYS / REPLACE, then EQUAL;
    /// `oit_multi_pass` off) mark the pixels; the editor keeps its stencil (its bits mean other things) and lets the resolve discard the
    /// empty pixels, as the shader does.
    pub(crate) fn pass_oit(&mut self, scene: &RenderScene) {
        let Some(range) = self.oit_range() else { return };
        let (Some(leaf_vs), Some(leaf_ps), Some(grass_ps), Some(resolve)) =
            (self.shader(TREE_LEAF_VS), self.shader(OIT_LEAF_PS), self.shader(OIT_GRASS_PS), self.shader(OIT_RESOLVE_PS))
        else {
            return;
        };

        let oit = self.post.oit.clone();
        let (Some(head_uav), Some(fragments_uav)) = (oit.head_uav.clone(), oit.fragments_uav.clone()) else { return };
        wce_prof!("render.oit");

        // 1. The fragments: the lists emptied, the fragment counter reset by the binding.
        self.ctx.clear_unordered_access_view_uint(&head_uav, [u32::MAX; 4]);
        self.ctx.set_render_targets_and_uavs(&[], self.gbuffer.scene_dsv().as_ref(), &[Some(head_uav), Some(fragments_uav)], &[0, 0]);
        self.ctx.set_depth_stencil_state(self.states.ds_sky.as_ref(), 0);
        self.ctx.set_rasterizer_state(self.states.rs_none.as_ref());
        null_ps_srvs(&self.ctx, 16);

        // The leaves the G-buffer pass left out (two-sided, as there).
        let draws = std::mem::take(&mut self.draw_lists.draws);
        let vis = std::mem::take(&mut self.draw_lists.vis_oit);
        if vis.instances() > 0
            && let Some(layout) = self.layout(&leaf_vs, "tree", &tree_layout(self.game))
        {
            let empty = Visible::default();
            let firsts = self.upload_instances([&draws, &draws], [&vis, &empty]);
            self.ctx.set_input_layout(Some(&layout));
            self.ctx.set_topology(D3D11_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
            self.ctx.set_vertex_buffers(1, std::slice::from_ref(&self.draw_lists.ring.buffer), &[(size_of::<RigidInstance>() as u32)], &[0]);
            let slots = self.slots_of(&leaf_ps);
            let mut bound = false;
            let (_, triangles) = self.draw_batches_flags([&draws, &draws], [&vis, &empty], firsts, |r, m, _, faded| {
                if faded || m.kind != MeshKind::TreeLeaf {
                    return false;
                }

                if !bound {
                    r.bind(&leaf_vs);
                    for &slot in leaf_vs.textures.values() {
                        r.ctx.vs_set_shader_resources(slot, std::slice::from_ref(&r.res.instance_ids_srv));
                    }

                    r.bind(&leaf_ps);
                    bind_srv(&r.ctx, &leaf_ps, "s_fire_map", r.res.black_srv.as_ref());
                    bind_srv(&r.ctx, &leaf_ps, "s_diffuse_burn_map", r.res.black_srv.as_ref());
                    bind_srv(&r.ctx, &leaf_ps, "s_dissolve_map", r.res.white_srv.as_ref());
                    r.upload();
                    ps_samplers(&r.ctx, &leaf_ps, &r.states.s_wrap_aniso);
                    r.bind_forward_inputs(&leaf_ps);
                    bound = true;
                }

                r.bind_mesh_material(&leaf_ps, &slots, m);
                true
            });

            self.draw_stats.triangles += triangles;
        }

        self.draw_lists.draws = draws;
        self.draw_lists.vis_oit = vis;

        // The near grass.
        self.ctx.set_rasterizer_state(self.states.rs_none.as_ref());
        self.draw_oit_grass(scene, &grass_ps, range);
        null_ps_srvs(&self.ctx, 16);

        // 2. The resolve over the light buffer (the UAVs unbound first: the resolve reads them).
        self.ctx.set_render_targets_and_uavs(&[], None, &[None, None], &[u32::MAX, u32::MAX]);
        self.ctx.set_depth_stencil_state(self.states.ds_off.as_ref(), 0);
        self.ctx.set_blend_state(self.states.bs_premultiplied.as_ref());
        let rtv = self.gbuffer.scene_hdr_rtv();
        self.ctx.set_render_targets(std::slice::from_ref(&rtv), None);
        self.bind(&resolve);
        bind_srv(&self.ctx, &resolve, "g_head_fragment_index_SRV", oit.head_srv.as_ref());
        bind_srv(&self.ctx, &resolve, "g_fragment_linked_list_SRV", oit.fragments_srv.as_ref());
        let (w, h) = (self.gbuffer.w, self.gbuffer.h);
        self.fullscreen_to(&rtv, w, h, &resolve);
        self.ctx.set_blend_state(self.states.bs_opaque.as_ref());
        self.ctx.set_rasterizer_state(self.states.rs_back.as_ref());
    }
}
