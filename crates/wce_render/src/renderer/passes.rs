//! The geometry, shadow and lighting passes.

use wce_foundation::math::{Float3, Float4x4, cross, inverse, normalize, translation};
use wce_foundation::wce_prof;
use wce_scene::camera::Camera;

use super::*;

/// How far the environment cube's scene reaches (scene units).
const ENV_CUBE_RANGE: f32 = 2000.0;
use crate::draw_set::{Frustum, View as DrawView, Visible};
use crate::model_cache::{GpuMesh, MeshKind, TEX_SLOTS};
use crate::shader_lib::{bind_sampler, bind_srv};

/// Battle rows of the split table (dll+0x1c88aa0), indexed by gfx_shadow_quality.
const SHADOW_ROWS: [[f32; 4]; 4] = [[60.0, 180.0, 180.0, 180.0], [30.0, 90.0, 210.0, 210.0], [30.0, 90.0, 210.0, 450.0], [30.0, 90.0, 210.0, 450.0]];

/// Rome 2's battle cascade lengths (game units) at shadow qualities 4 and 5, the four cascade ones (table dll+0x118ff698; qualities 1-3 have
/// two or three cascades: 30, 120 and 15, 40, 120).
const SHADOW_ROW_ROME2: [f32; 4] = [15.0, 35.0, 90.0, 200.0];

/// The atlas quadrants of the four cascades.
pub(crate) const SHADOW_BOUNDS: [[f32; 4]; 4] = [[0.0, 0.0, 0.5, 0.5], [0.5, 0.0, 1.0, 0.5], [0.0, 0.5, 0.5, 1.0], [0.5, 0.5, 1.0, 1.0]];

impl Renderer {

    pub(crate) fn pass_gbuffer(&mut self) {
        // Channel 4 (z / w) starts at the far plane: the game's is its depth buffer, cleared to 1 (RenderDoc, coast.rdc). Cleared to 0, a
        // pixel with nothing behind the sea read as the near plane, and the fog (which reads it) left those pixels clear: dark slivers of
        // sea along the horizon where the far ground had gaps.
        for (k, rtv) in self.gbuffer.rtvs().iter().enumerate() {
            if let Some(rtv) = rtv {
                self.ctx.clear_render_target_view(rtv, if k == 3 { [1.0, 0.0, 0.0, 0.0] } else { [0.0; 4] });
            }
        }

        let rtvs = self.gbuffer.scene_rtvs();
        if let Some(ms) = &self.gbuffer.ms {
            for rtv in ms.channels.iter().filter_map(|t| t.rtv.as_ref()) {
                self.ctx.clear_render_target_view(rtv, [0.0; 4]);
            }
        }

        for dsv in [self.gbuffer.dsv.clone(), self.gbuffer.ms.as_ref().and_then(|m| m.depth.dsv.clone())].iter().flatten() {
            self.ctx.clear_depth_stencil_view(dsv, D3D11_CLEAR_DEPTH | D3D11_CLEAR_STENCIL, 1.0, 0);
        }

        self.ctx.set_render_targets(&rtvs, self.gbuffer.scene_dsv().as_ref());
        self.ctx.set_depth_stencil_state(self.states.ds_gbuffer.as_ref(), 1);
        self.ctx.set_rasterizer_state(self.states.rs_back.as_ref());

        // The near leaves are this view's (none if it draws nothing): the OIT pass must never see another view's.
        self.draw_lists.vis_oit = Visible::default();
        if self.draw_lists.draws.items() == 0 && self.draw_lists.terrain_draws.items() == 0 {
            return;
        }

        // The rigid vertex shaders share one layout, the tree ones another.
        let Some(vs) = self.shader(RIGID_VS) else { return };
        let Some(layout) = self.layout(&vs, "rigid_std", &rigid_layout()) else { return };
        let tree_layout = self.shader(TREE_BRANCH_VS).and_then(|tvs| self.layout(&tvs, "tree", &tree_layout(self.game)));
        let billboard_layout = self.shader(TREE_BILLBOARD_VS).and_then(|bvs| self.layout(&bvs, "tree_billboard", &billboard_layout()));
        self.ctx.set_input_layout(Some(&layout));
        self.ctx.set_topology(D3D11_PRIMITIVE_TOPOLOGY_TRIANGLELIST);

        // The ground first, as the entity order had it (equal depths: the first one drawn wins).
        let mut view = self.camera_view(if self.env.active { 0.5 } else { 1.0 });

        // The environment cube is taken from one point for reflections and the ambient: what is kilometres away adds nothing to it
        // (ASSUMPTION: the game's EnvMapGen range was not traced; 2 km).
        if self.env.active {
            view.max_distance = ENV_CUBE_RANGE;
        }

        let (terrain_draws, draws) = (std::mem::take(&mut self.draw_lists.terrain_draws), std::mem::take(&mut self.draw_lists.draws));
        let (mut vis_terrain, mut vis) = (std::mem::take(&mut self.draw_lists.vis_terrain), std::mem::take(&mut self.draw_lists.vis));
        terrain_draws.cull(&view, &mut vis_terrain);

        // gfx_alpha_blend: the near leaves are the order-independent transparency's (`pass_oit`).
        let mut vis_oit = Visible::default();
        match self.oit_range() {
            Some(range) => draws.cull_split(&view, &mut vis, Some((&mut vis_oit, range))),
            None => draws.cull(&view, &mut vis),
        }

        self.draw_lists.vis_oit = vis_oit;

        // The ground is drawn by the terrain pass (game shaders), not as flat rigid patches.
        if self.terrain.active && !self.env.active {
            vis_terrain = Visible::default();
        }

        let firsts = self.upload_instances([&terrain_draws, &draws], [&vis_terrain, &vis]);

        // The terrain-blend meshes belong to the terrain pass when it draws the ground (below); without it they are ordinary meshes.
        let blend_in_terrain = self.terrain.active && !self.env.active;
        for (k, v) in [&vis_terrain, &vis].into_iter().enumerate() {
            self.draw_stats.views += 1;
            self.draw_stats.instances += v.instances() as u64;
            if k == 0 && !self.env.active {
                self.draw_stats.terrain_patches = v.instances() as u64;
            }
        }

        self.ctx.set_vertex_buffers(1, std::slice::from_ref(&self.draw_lists.ring.buffer), &[(size_of::<RigidInstance>() as u32)], &[0]);

        wce_prof!("render.gbuffer.draw");

        // One sub-pass per shader pair (GBUFFER_PASSES). An alpha-tested mesh whose material has no alpha-test shader is drawn opaque; a mesh
        // whose material has no shaders at all is not drawn. With the game's terrain pass, TerrainBlend meshes leave the table: their geometry
        // pass below marks them as terrain pixels, and the terrain pass projects the terrain's material onto them (blend_mesh.rs).
        self.draw_lists.gbuffer_firsts = firsts;
        let game = self.game;
        let shaders = GBUFFER_PASSES.map(|(kind, _, fire, vs, ps, _)| (!fire || game.has_fire_shaders(kind)).then(|| self.shader(vs).zip(self.shader(ps))).flatten());
        let pass_in = |m: &GpuMesh, fire: bool| {
            let opaque = GBUFFER_PASSES.iter().position(|p| p.0 == m.kind && !p.1 && p.2 == fire)?;
            let alpha = GBUFFER_PASSES.iter().position(|p| p.0 == m.kind && p.1 && p.2 == fire).filter(|&k| m.alpha_mode == 1 && shaders[k].is_some());
            alpha.or(Some(opaque).filter(|&k| shaders[k].is_some()))
        };

        // A burning batch takes its material's fire shader (the plain one when the material has none).
        let pass_of = |m: &GpuMesh, burning: bool| {
            if m.kind == MeshKind::TerrainBlend && blend_in_terrain {
                return None;
            }

            burning.then(|| pass_in(m, true)).flatten().or_else(|| pass_in(m, false))
        };

        // gfx_tesselation (fn 0x1188fc40: technique 0x42 for an opaque mesh with a displacement map, in the G-buffer pass): those meshes are
        // drawn in a second round, through rigidcommon's displacement vertex shader, hull and domain shaders, with their material's pixel shader.
        let tess_stages = if self.settings.tessellation && game != super::game::RenderGame::Rome2 {
            match (self.shader(TESS_VS), self.shader(TESS_HS), self.shader(TESS_DS)) {
                (Some(v), Some(h), Some(d)) => Some((v, h, d)),
                _ => None,
            }
        } else {
            None
        };
        let tessellated = |m: &GpuMesh, pass: usize| {
            tess_stages.is_some() && m.displacement.is_some() && !GBUFFER_PASSES[pass].1 && !GBUFFER_PASSES[pass].2 && !m.kind.is_tree()
        };

        for tess in [false, true] {
            if tess && tess_stages.is_none() {
                continue;
            }

            for (pass, pair) in shaders.iter().enumerate() {
                let Some((plain_vs, ps)) = pair.clone() else { continue };
                let vs = match (&tess_stages, tess) {
                    (Some((v, _, _)), true) => v.clone(),
                    _ => plain_vs,
                };
                if !self.env.active && !tess {
                    self.gpu_timer.mark(GBUFFER_PASSES[pass].5);
                }

                let pass_layout = match GBUFFER_PASSES[pass].0 {
                    MeshKind::TreeBranch | MeshKind::TreeLeaf => tree_layout.as_ref(),
                    MeshKind::TreeBillboard => billboard_layout.as_ref(),
                    _ => Some(&layout),
                };

                let Some(pass_layout) = pass_layout else { continue };
                self.ctx.set_input_layout(Some(pass_layout));

                // The game draws the trees' branches and leaves two-sided (RenderDoc captures of Rome: CULL_NONE).
                let two_sided = matches!(GBUFFER_PASSES[pass].0, MeshKind::TreeBranch | MeshKind::TreeLeaf);
                self.ctx.set_rasterizer_state(if two_sided { self.states.rs_none.as_ref() } else { self.states.rs_back.as_ref() });

                let slots = self.slots_of(&ps);
                let mut bound = false;
                let fire = GBUFFER_PASSES[pass].2;
                // Faded instances are the blended forward pass's (`pass_faded`).
                let (instances, triangles) = self.draw_batches_flags([&terrain_draws, &draws], [&vis_terrain, &vis], firsts, |r, m, burning, faded| {
                    if faded || pass_of(m, burning) != Some(pass) || tessellated(m, pass) != tess {
                        return false;
                    }

                    // With snow regions a tree batch snows by its model's mark (MODEL_SNOW_SUFFIX), the rest at the regions' intensity.
                    if let Some(region) = r.snow_region {
                        r.set_snowing(if m.kind.is_tree() && !m.snow { r.snow_weather } else { region });
                    }

                    if !bound {
                        r.bind(&vs);
                        for &slot in vs.textures.values() {
                            r.ctx.vs_set_shader_resources(slot, std::slice::from_ref(&r.res.instance_ids_srv));
                        }

                        r.bind(&ps);
                        // Linear, as the game's s_fire_map samplers (docs/re/samplers.tsv): its channels are masks, not a colour.
                        let fire_map = if fire { r.textures.get(FIRE_MAP, false).and_then(|t| t.read().ok().and_then(|t| t.srv.clone())) } else { None };
                        bind_srv(&r.ctx, &ps, "s_fire_map", fire_map.or_else(|| r.res.black_srv.clone()).as_ref());
                        let snow = r.snow_normals_srv();
                        bind_srv(&r.ctx, &ps, "s_snow_normals", snow.as_ref());

                        // Trees: no burn, nothing dissolved (the dissolve amount is in the instance).
                        bind_srv(&r.ctx, &ps, "s_diffuse_burn_map", r.res.black_srv.as_ref());
                        bind_srv(&r.ctx, &ps, "s_dissolve_map", r.res.white_srv.as_ref());
                        if let (true, Some((_, hs, ds))) = (tess, &tess_stages) {
                            r.cbs.set_f("rigid_config_PS", "g_tesselation_factor", &[TESS_FACTOR]);
                            r.cbs.set_f("rigid_config_PS", "g_tesselation_displacement_scale", &[TESS_DISPLACEMENT_SCALE]);
                            r.bind(hs);
                            r.bind(ds);
                            bind_sampler(&r.ctx, ds, "s_displacement_map", r.states.s_wrap_linear.as_ref());
                            r.ctx.set_topology(D3D11_PRIMITIVE_TOPOLOGY_3_CONTROL_POINT_PATCHLIST);
                        }

                        r.upload();
                        ps_samplers(&r.ctx, &ps, &r.states.s_wrap_aniso);
                        bound = true;
                    }

                    if let (true, Some((_, _, ds)), Some(map)) = (tess, &tess_stages, &m.displacement) {
                        let srv = map.read().ok().and_then(|t| t.srv.clone());
                        bind_srv(&r.ctx, ds, "t_tesselation_displacement_map", srv.as_ref());
                    }

                    r.bind_mesh_material(&ps, &slots, m);
                    true
                });

                self.draw_stats.triangles += triangles;
                if !self.env.active {
                    self.draw_stats.camera_triangles += triangles;
                    self.draw_stats.camera_instances += instances;
                }
            }
        }

        // The tessellated round leaves its stages bound.
        if tess_stages.is_some() {
            self.ctx.hs_set_shader(None);
            self.ctx.ds_set_shader(None);
            self.ctx.set_topology(D3D11_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
        }

        // The terrain-blend meshes' geometry pass (`custom_geometry`): depth, the G-buffer depth channel and stencil 2, the mark of the terrain pixels (the blend boxes and
        // the resolve only touch those), so that the terrain textures are projected onto the meshes.
        if blend_in_terrain {
            let d = blend_mesh::BlendDraw {
                vs: blend_mesh::BLEND_VS, ps: blend_mesh::BLEND_GBUFFER_PS, targets: self.gbuffer.scene_rtvs(), dsv: self.gbuffer.scene_dsv(), depth_stencil: self.states.ds_gbuffer.clone(), stencil_ref: TERRAIN_STENCIL,
                blend: self.states.bs_opaque.clone(),
            };

            self.draw_blend_meshes([&terrain_draws, &draws], [&vis_terrain, &vis], firsts, &d);
        }

        self.draw_lists.terrain_draws = terrain_draws;
        self.draw_lists.draws = draws;
        self.draw_lists.vis_terrain = vis_terrain;
        self.draw_lists.vis = vis;
    }

    /// Binds a mesh's material for a rigid or tree pixel shader: its textures in the shader's slots, the dirt maps' names, the tiled dirt's
    /// repeats, a tree's tint and season.
    pub(crate) fn bind_mesh_material(&mut self, ps: &Rc<GameShader>, slots: &[Option<u32>; TEX_SLOTS], m: &GpuMesh) {
        if matches!(m.kind, MeshKind::TiledDirtmap | MeshKind::Dirtmap) {
            self.cbs.set_f("rigid_config_PS", "g_uv2_tile_interval", &m.uv2_tile);
            self.upload();
        }

        // A tree mesh's material tints it.
        if m.kind.is_tree() {
            let cb = if m.kind == MeshKind::TreeBillboard { "tree_billboard_constants" } else { "vegetation_tint_colours" };
            self.cbs.set_f(cb, "g_tint_colours", &m.tint);
            self.upload();
        }

        // rigidwithdirtmesh names its dirt map and mask differently.
        if m.kind == MeshKind::Dirtmap {
            let srv = |t: usize| m.tex[t].read().ok().and_then(|tex| tex.srv.clone());
            bind_srv(&self.ctx, ps, "s_decal_dirt_map", srv(crate::model_cache::TEX_DETAIL).as_ref());
            bind_srv(&self.ctx, ps, "s_decal_dirt_mask", srv(crate::model_cache::TEX_ALPHA_MASK).as_ref());
        }

        // Trees draw their season's diffuse (as grass does).
        let seasonal = if m.kind.is_tree() { self.vegetation_diffuse(m) } else { None };

        // The billboard's textures have names of their own.
        if m.kind == MeshKind::TreeBillboard {
            let srv = |t: usize| m.tex[t].read().ok().and_then(|tex| tex.srv.clone());
            bind_srv(&self.ctx, ps, "s_diffuse_map_tree", seasonal.clone().or_else(|| srv(crate::model_cache::TEX_DIFFUSE)).as_ref());
            bind_srv(&self.ctx, ps, "s_combined_normal_and_displacement", srv(crate::model_cache::TEX_NORMAL).as_ref());
        }

        for (t, slot) in slots.iter().enumerate().take(TEX_SLOTS) {
            let Some(slot) = slot else { continue };
            let srv = if t == crate::model_cache::TEX_DIFFUSE && seasonal.is_some() {
                seasonal.clone()
            } else {
                m.tex[t].read().ok().and_then(|tex| tex.srv.clone())
            };
            self.ctx.ps_set_shader_resources(*slot, &[srv]);
        }
    }

    /// The faded instances (opacity below 1), which the G-buffer pass leaves out: drawn after the lighting with their material's blended
    /// forward shader over the light buffer, depth-tested without writing depth, from the G-buffer pass's instances. A material without
    /// one (trees, terrain blend) is not drawn faded.
    pub(crate) fn pass_faded(&mut self) {
        if self.env.active || !self.draw_lists.vis.active.iter().any(|&b| self.draw_lists.draws.batch_faded(b)) {
            return;
        }

        let Some(vs) = self.shader(RIGID_VS) else { return };
        let Some(layout) = self.layout(&vs, "rigid_std", &rigid_layout()) else { return };
        wce_prof!("render.faded");
        self.ctx.set_render_targets(&[self.gbuffer.scene_hdr_rtv()], self.gbuffer.scene_dsv().as_ref());
        self.ctx.set_depth_stencil_state(self.states.ds_sky.as_ref(), 0);
        self.ctx.set_rasterizer_state(self.states.rs_back.as_ref());
        self.ctx.set_blend_state(self.states.bs_alpha.as_ref());
        self.ctx.set_input_layout(Some(&layout));
        self.ctx.set_topology(D3D11_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
        self.ctx.set_vertex_buffers(1, std::slice::from_ref(&self.draw_lists.ring.buffer), &[(size_of::<RigidInstance>() as u32)], &[0]);

        let (terrain_draws, draws) = (std::mem::take(&mut self.draw_lists.terrain_draws), std::mem::take(&mut self.draw_lists.draws));
        let (vis_terrain, vis) = (std::mem::take(&mut self.draw_lists.vis_terrain), std::mem::take(&mut self.draw_lists.vis));
        let firsts = self.draw_lists.gbuffer_firsts;
        for (kind, name) in FADED_PASSES {
            let Some(ps) = self.shader(name) else { continue };
            let slots = self.slots_of(&ps);
            let mut bound = false;
            let (_, triangles) = self.draw_batches_flags([&terrain_draws, &draws], [&vis_terrain, &vis], firsts, |r, m, _, faded| {
                if !faded || m.kind != kind {
                    return false;
                }

                if !bound {
                    r.bind(&vs);
                    for &slot in vs.textures.values() {
                        r.ctx.vs_set_shader_resources(slot, std::slice::from_ref(&r.res.instance_ids_srv));
                    }

                    r.bind(&ps);
                    bind_srv(&r.ctx, &ps, "s_fire_map", r.res.black_srv.as_ref());
                    let snow = r.snow_normals_srv();
                    bind_srv(&r.ctx, &ps, "s_snow_normals", snow.as_ref());
                    bind_srv(&r.ctx, &ps, "s_dissolve_map", r.res.white_srv.as_ref());
                    bind_srv(&r.ctx, &ps, "s_environment", if r.env.valid { r.env.cube.srv.as_ref() } else { r.sky.cube.srv.as_ref() });
                    bind_srv(&r.ctx, &ps, "s_sky", r.sky.cube.srv.as_ref());
                    r.upload();
                    ps_samplers(&r.ctx, &ps, &r.states.s_wrap_aniso);
                    bind_sampler(&r.ctx, &ps, "s_environment", r.states.s_wrap_linear.as_ref());
                    bind_sampler(&r.ctx, &ps, "s_sky", r.states.s_clamp_linear.as_ref());
                    bound = true;
                }

                r.bind_mesh_material(&ps, &slots, m);
                true
            });

            self.draw_stats.triangles += triangles;
        }

        self.draw_lists.terrain_draws = terrain_draws;
        self.draw_lists.draws = draws;
        self.draw_lists.vis_terrain = vis_terrain;
        self.draw_lists.vis = vis;
        self.ctx.set_blend_state(self.states.bs_opaque.as_ref());
        null_ps_srvs(&self.ctx, 16);
    }

    pub(crate) fn pass_directional_light(&mut self) {
        let Some(ps) = self.shader(DIR_LIGHT_PS) else { return };
        self.ctx.set_render_targets(&[self.gbuffer.scene_hdr_rtv()], self.gbuffer.scene_dsv().as_ref());
        self.ctx.set_depth_stencil_state(self.states.ds_light.as_ref(), 1);
        self.ctx.set_rasterizer_state(self.states.rs_none.as_ref());

        self.cbs.set_f("shroud_PS", "g_shroud_enabled", &[0.0]);
        self.cbs.set_m("directional_light_PS", "g_overlay_view_projection", Float4x4::IDENTITY.data());

        // The engine's default (see the water pass); the shader writes the refraction weight into the light buffer's alpha.
        self.cbs.set_f("directional_light_PS", "g_refraction_depth", &[0.75]);
        self.upload();
        for i in 0..4 {
            bind_srv(&self.ctx, &ps, &format!("gbuffer_channel_{}_sampler", i + 1), self.gbuffer.channels[i].srv.as_ref());
        }

        bind_srv(&self.ctx, &ps, "s_ssao", self.post.ao_result.as_ref().or(self.res.white_srv.as_ref()));
        bind_srv(&self.ctx, &ps, "s_environment", if self.env.active || !self.env.valid { self.sky.cube.srv.as_ref() } else { self.env.cube.srv.as_ref() });
        bind_srv(&self.ctx, &ps, "s_overlay", self.res.overlay_none_srv.as_ref());
        bind_srv(&self.ctx, &ps, "s_shroud", self.res.white_srv.as_ref());
        bind_srv(&self.ctx, &ps, "g_tShadowBuffer_SM50", self.shadow_map.srv.as_ref());
        ps_samplers(&self.ctx, &ps, &self.states.s_clamp_point);
        bind_sampler(&self.ctx, &ps, "s_environment", self.states.s_clamp_linear.as_ref());
        bind_sampler(&self.ctx, &ps, "s_ssao", self.states.s_clamp_linear.as_ref());
        bind_sampler(&self.ctx, &ps, "sShadowBuffer_SM50", self.states.s_shadow_cmp.as_ref());

        // gfx_cloud_shadows (Thrones of Britannia; fn 0x117c3680): the clouds' shade over the sun, in battle scale 0.0005 and every pixel
        // (w 1); off: zero, the shade never applies.
        let shade = self.settings.cloud_shadows && self.game != super::game::RenderGame::Rome2 && !self.env.active && ps.textures.contains_key("cloud_shade_sampler");
        let [ox, oy] = self.cloud_shade;
        self.cbs.set_f("directional_light_PS", "g_cloud_shade_params", &if shade { [ox, oy, CLOUD_SHADE_BATTLE_SCALE, 1.0] } else { [0.0; 4] });
        if shade {
            let srv = self.sky_texture(CLOUD_SHADE_TEXTURE, false);
            bind_srv(&self.ctx, &ps, "cloud_shade_sampler", srv.as_ref());
            bind_sampler(&self.ctx, &ps, "cloud_shade_sampler", self.states.s_wrap_linear.as_ref());
        }

        self.upload();
        self.fullscreen(&ps);
    }

    /// The environment's specular, as the game adds it after the lights (two RenderDoc captures of Rome, docs/re/renderer.md): the
    /// directional light leaves it out (`g_apply_environment_specular` 0). environment_specular_light writes each pixel's reflection (a
    /// screen-space march through the depth, hits taken from the light buffer; else the environment cube, below the horizon the map's ground
    /// colour lit by the sun) and its blur radius and weight; reflections_blur blurs it in x, then in y, multiplied by the specular colour and
    /// the weight, added to the light buffer. The parameters are the capture's.
    /// ASSUMPTIONS: the light buffer as the march's source (the low-settings capture bound a placeholder there); the ground colour is a
    /// constant (`ground_colour_srv`); every pixel passes the stencil test (the game's geometry has low bits 1, the sky 0, whose specular
    /// colour is 0 anyway).
    pub(crate) fn pass_environment_specular(&mut self) {
        if self.env.active || self.game == super::game::RenderGame::Rome2 || self.reflections_off() {
            return;
        }

        // gfx_screen_space_reflections 1: half resolution (fn 0x11903ec0's table: 0.5), the half resolution entries.
        let half = self.settings.reflections == 1 && self.game != super::game::RenderGame::Rome2;
        let div = if half { 2 } else { 1 };
        let (w, h) = (self.gbuffer.w, self.gbuffer.h);
        if self.post.reflection_div != div {
            self.post.resize_reflection(&self.dev, w, h, div);
        }

        let (rw, rh) = (w.div_ceil(div), h.div_ceil(div));
        let (Some(es), Some(bx), Some(by)) = (
            self.shader(if half { ENV_SPECULAR_HALF_PS } else { ENV_SPECULAR_PS }), self.shader(REFLECTION_BLUR_X_PS),
            self.shader(if half { REFLECTION_BLUR_Y_HALF_PS } else { REFLECTION_BLUR_Y_PS }),
        ) else { return };
        wce_prof!("render.environment_specular");

        // The camera's screen constants stay the full screen's: the reflection and the x blur divide their pixel's position by
        // g_buffers_scale (fn 0x118e0b90 writes the table's 0.5 / 1 to both), the y blur draws the full screen.
        let scale = if half { 0.5 } else { 1.0 };
        if half {
            self.ctx.set_viewports(&[viewport(0.0, 0.0, rw as f32, rh as f32)]);
        }
        let f = |v: f32| v.to_le_bytes();
        let i = |v: i32| v.to_le_bytes();
        self.ctx.set_depth_stencil_state(self.states.ds_off.as_ref(), 0);
        self.ctx.set_rasterizer_state(self.states.rs_none.as_ref());
        self.ctx.set_blend_state(self.states.bs_opaque.as_ref());
        if let Some(rtv) = &self.post.stencil.rtv {
            self.ctx.clear_render_target_view(rtv, [1.0, 1.0, 0.0, 0.0]);
        }

        // 1. The reflection and its blur parameters.
        let p: [(usize, Vec<u8>); 11] = [
            (0, i(0).to_vec()), (4, f(1.0).to_vec()), (8, f(20.0).to_vec()), (16, [f(0.4), f(1.0)].concat()), (24, [f(0.9), f(1.0)].concat()),
            (32, f(0.1).to_vec()), (36, [f(0.996), f(0.99)].concat()), (48, [f(2.0), f(5.0)].concat()), (56, f(0.06).to_vec()),
            (64, [f(0.5), f(0.5), f(28672.0), f(-28672.0)].concat()), (80, f(scale).to_vec()),
        ];
        for (at, bytes) in &p {
            self.cbs.set_at("parameters", *at, bytes);
        }

        self.upload();
        self.ctx.set_render_targets(&[self.post.reflection.rtv.clone(), self.post.reflection_params.rtv.clone()], None);
        for k in 0..4 {
            bind_srv(&self.ctx, &es, &format!("gbuffer_channel_{}_sampler", k + 1), self.gbuffer.channels[k].srv.as_ref());
        }

        bind_srv(&self.ctx, &es, "s_environment", if self.env.valid { self.env.cube.srv.as_ref() } else { self.sky.cube.srv.as_ref() });
        bind_srv(&self.ctx, &es, "s_terrain", self.res.ground_colour_srv.as_ref());
        bind_srv(&self.ctx, &es, "s_light_accumulation_buffer", self.gbuffer.hdr.srv.as_ref());
        bind_srv(&self.ctx, &es, "s_ssao", self.post.ao_result.as_ref().or(self.res.white_srv.as_ref()));
        bind_srv(&self.ctx, &es, "t_depth_stencil_int", self.post.stencil.srv.as_ref());
        bind_srv(&self.ctx, &es, "t_jitter", self.res.jitter_srv.as_ref());
        ps_samplers(&self.ctx, &es, &self.states.s_clamp_point);
        for name in ["s_environment", "s_terrain", "s_ssao", "s_light_accumulation_buffer"] {
            bind_sampler(&self.ctx, &es, name, self.states.s_clamp_linear.as_ref());
        }

        self.fullscreen(&es);
        null_ps_srvs(&self.ctx, 16);

        // 2. The x blur.
        let blur = |cbs: &mut crate::cbuffers::ConstantBuffers| {
            for (at, bytes) in [(0, i(0).to_vec()), (4, f(0.98).to_vec()), (8, f(0.5).to_vec()), (12, f(scale).to_vec())] {
                cbs.set_at("parameters", at, &bytes);
            }
        };

        blur(&mut self.cbs);
        self.upload();
        self.ctx.set_render_targets(std::slice::from_ref(&self.post.reflection_blur.rtv), None);
        bind_srv(&self.ctx, &bx, "gbuffer_channel_1_sampler", self.gbuffer.channels[0].srv.as_ref());
        bind_srv(&self.ctx, &bx, "s_reflection_buffer", self.post.reflection.srv.as_ref());
        bind_srv(&self.ctx, &bx, "s_blur_parameters_buffer", self.post.reflection_params.srv.as_ref());
        ps_samplers(&self.ctx, &bx, &self.states.s_clamp_point);
        bind_sampler(&self.ctx, &bx, "s_reflection_buffer", self.states.s_clamp_linear.as_ref());
        self.fullscreen(&bx);
        null_ps_srvs(&self.ctx, 16);

        // 3. The y blur, added to the light buffer (ONE / ONE, as the game).
        if half {
            self.ctx.set_viewports(&[viewport(0.0, 0.0, w as f32, h as f32)]);
        }

        blur(&mut self.cbs);
        self.upload();
        self.ctx.set_render_targets(&[self.gbuffer.scene_hdr_rtv()], None);
        self.ctx.set_blend_state(self.states.bs_add_one.as_ref());
        // The half resolution one also reads channels 2 (its .w weighs a Fresnel term) and 4 (the depth, for the view's direction).
        for k in 0..4 {
            bind_srv(&self.ctx, &by, &format!("gbuffer_channel_{}_sampler", k + 1), self.gbuffer.channels[k].srv.as_ref());
        }
        bind_srv(&self.ctx, &by, "s_reflection_buffer", self.post.reflection_blur.srv.as_ref());
        bind_srv(&self.ctx, &by, "s_blur_parameters_buffer", self.post.reflection_params.srv.as_ref());
        ps_samplers(&self.ctx, &by, &self.states.s_clamp_point);
        bind_sampler(&self.ctx, &by, "s_reflection_buffer", self.states.s_clamp_linear.as_ref());
        self.fullscreen(&by);
        null_ps_srvs(&self.ctx, 16);
        self.ctx.set_blend_state(self.states.bs_opaque.as_ref());
        self.ctx.set_render_targets(&[self.gbuffer.scene_hdr_rtv()], self.gbuffer.scene_dsv().as_ref());
    }

    /// gfx_screen_space_reflections at 0: the environment specular pass is off (fn 0x118c2530: on above 0); 1 draws it at half
    /// resolution (the table at 0x11cca8fc: 0, 0.5, 1).
    /// gfx_alpha_blend above 0 (Attila, ToB; the camera's view, with the OIT shaders and buffers): the squared distances (scene units) of
    /// the leaves and grass the order-independent transparency draws (`OIT_RANGE`).
    pub(crate) fn oit_range(&mut self) -> Option<(f32, f32)> {
        if self.settings.alpha_blend < 1 || self.game == super::game::RenderGame::Rome2 || self.env.active {
            return None;
        }

        let (w, h) = (self.gbuffer.w, self.gbuffer.h);
        if self.post.oit.w != w || self.post.oit.h != h {
            self.post.oit = super::targets::OitBuffers::new(&self.dev, w, h, OIT_FRAGMENTS_PER_PIXEL);
        }

        let ready = self.post.oit.ready() && [OIT_LEAF_PS, OIT_GRASS_PS, OIT_RESOLVE_PS].iter().all(|n| self.shader(n).is_some());
        let (lo, hi) = (OIT_RANGE.0 / self.camera.units, OIT_RANGE.1 / self.camera.units);
        ready.then_some((lo * lo, hi * hi))
    }

    pub(crate) fn reflections_off(&self) -> bool {
        self.game != super::game::RenderGame::Rome2 && self.settings.reflections == 0
    }

    /// The battle cascade splits (ShadowMapView, fns 0x11794320 / 0x11794430 / 0x11784c00, verified on all three dumps; in game units, the coast dump 2026-10-31):
    /// - far = camera near + sum(row) with the quality row (30, 90, 210, 450) -> 781 (fn 0x11751fc0, table dll+0x1c88aa0);
    /// - range = the depth-buffer analysis (min, max visible view depth, max clamped to far), max *= shadow_split_enlargement_safe_zone (1.05);
    ///   start = min, scale = (max - min) / (far - near);
    /// - s0 = start, s_i = start + scale * (cumulative row)_i; cascade i covers [s_i - 0.1 (s_i - s_(i-1)), s_(i+1)] (shadow_split_blend_fraction
    ///   0.1); g_fFadeRange = (0.1, (s4 - (far - 0.2 (s4 - s3))) / (0.2 (s4 - s3)) * 0.2) (final_shadow_split_blend_fraction 0.2).
    pub(crate) fn pass_shadows(&mut self, scene: &RenderScene, cam: &Camera, aspect: f32, light_dir: Float3, _time_s: f32) {
        // In game units, as the rows (the depth analysis is in scene units); the slices go back to scene units below. Mixed, every
        // cascade came out twice the game's length (coast dump: 9.9, 41, 134, 352, 819 against the editor's 9.8, 73, 261, 700, 1640).
        let u = self.camera.units;
        let row = SHADOW_ROWS[self.settings.shadow_quality.clamp(0, 3) as usize];
        let n = cam.znear.max(1.0);
        let far_q = n + row[0] + row[1] + row[2] + row[3];
        let (mut start, mut scale) = (n, 1.0f32);
        if self.depth_range.valid {
            let (lo, hi) = (self.depth_range.min * u, (self.depth_range.max * u).min(far_q) * 1.05);
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
        for s in splits.iter_mut().chain(slice_near.iter_mut()) {
            *s /= u;
        }

        // Rome 2 (fn 0x114765e0): no depth analysis nor overlap; from the camera's near, its battle row of lengths (game units) of the
        // four cascade qualities. Its light pass reads x, y unprojected: an ortho fit (shadow_fit::fit_rome2).
        let rome2 = self.game == super::game::RenderGame::Rome2;
        if rome2 {
            splits[0] = n;
            for i in 1..=4 {
                splits[i] = splits[i - 1] + SHADOW_ROW_ROME2[i - 1] / u;
            }

            slice_near.copy_from_slice(&splits[..4]);
        }

        // The slices are fitted around the camera relative to the render origin: fitted absolute, 10^5 units from the scene's origin, the
        // light's matrices lost their precision once moved back to the origin and the shadows (the trees' most) flickered as the camera moved.
        let o = self.camera.origin;
        let cam_view = cam.view_from(o);
        let inv_view = inverse(&cam_view);
        let view_dir = normalize(cam.target - cam.eye);
        let side = normalize(cross(cam.up, view_dir));

        let mut matrices = [0.0f32; 64];
        // The cascades for the shaders (relative positions) and for culling (absolute).
        let mut light_rel = [Float4x4::IDENTITY; 4];
        let mut light_vp = [Float4x4::IDENTITY; 4];
        let mut texel_c = [0.0f32; 4];
        for c in 0..4 {
            // The frustum slice's corners (fn 0x11698df0), fitted as the game does (shadow_fit.rs).
            let pts: [Float3; 8] = std::array::from_fn(|k| {
                let d = if k & 4 != 0 { splits[c + 1] } else { slice_near[c] };
                let (hx, hy) = cam.half_extents(d, aspect);
                let v = Float3::new(if k & 1 != 0 { 1.0 } else { -1.0 } * hx, if k & 2 != 0 { 1.0 } else { -1.0 } * hy, d);
                inv_view.transform_point(v)
            });

            let tile = (SHADOW_SIZE / 2) as f32;
            if rome2 {
                // Fitted in game units (the texel snap is the game's); `light_rel` takes scene units.
                let fit = shadow_fit::fit_rome2(&pts.map(|p| p * u), light_dir, tile);
                let g = fit.view_proj;
                let mut rel = g;
                for r in &mut rel.m {
                    for v in r.iter_mut().take(3) {
                        *v *= u;
                    }
                }

                light_rel[c] = rel;
                light_vp[c] = rel * translation(-o);
                texel_c[c] = fit.width / u / tile;

                // The atlas (fn 0x1141f840): x scale + offset to the tile's centre (and half a texel), y flipped.
                let size = SHADOW_SIZE as f32;
                let (ox, oy) = ((SHADOW_BOUNDS[c][0] * size + 0.5) / size + 0.25, (SHADOW_BOUNDS[c][1] * size + 0.5) / size + 0.25);
                let mut m = g;
                m.m[0] = std::array::from_fn(|k| 0.25 * g.m[0][k] + ox * g.m[3][k]);
                m.m[1] = std::array::from_fn(|k| -0.25 * g.m[1][k] + oy * g.m[3][k]);
                matrices[c * 16..c * 16 + 16].copy_from_slice(m.data());
                continue;
            }

            // Fitted in game units (the pull-back and the warp's near distance are the game's numbers), then taking scene units.
            let mut fit = shadow_fit::fit(&pts.map(|p| p * u), light_dir, view_dir, side, slice_near[c] * u);
            for r in &mut fit.view_proj.m {
                for v in r.iter_mut().take(3) {
                    *v *= u;
                }
            }

            fit.width /= u;
            light_rel[c] = fit.view_proj;
            light_vp[c] = fit.view_proj * translation(-o);
            texel_c[c] = fit.width / tile;

            // World -> atlas uv (y down) + depth: the lighting shader's lookup.
            let (bx, by) = (SHADOW_BOUNDS[c][0], SHADOW_BOUNDS[c][1]);
            let mut to_uv = Float4x4::IDENTITY;
            to_uv.m[0][0] = 0.25;
            to_uv.m[0][3] = bx + 0.25;
            to_uv.m[1][1] = -0.25;
            to_uv.m[1][3] = by + 0.25;
            matrices[c * 16..c * 16 + 16].copy_from_slice(game_clip(&(to_uv * light_rel[c]), self.camera.units).data());
        }

        // Render the casters into the four quadrants.
        let vs = self.shader(SHADOW_VS);
        let ps = self.shader(SHADOW_PS);
        self.ctx.set_render_targets(&[], None);
        if let Some(dsv) = &self.shadow_map.dsv {
            self.ctx.clear_depth_stencil_view(dsv, D3D11_CLEAR_DEPTH, 1.0, 0);
        }

        if let (Some(vs), Some(ps)) = (vs, ps)
            && (self.draw_lists.draws.items() > 0 || self.draw_lists.terrain_draws.items() > 0)
            && self.settings.shadows
            && let Some(layout) = self.layout(&vs, "rigid_std", &rigid_layout())
        {
            self.ctx.set_input_layout(Some(&layout));
            self.ctx.set_topology(D3D11_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
            self.ctx.set_render_targets(&[], self.shadow_map.dsv.as_ref());
            self.ctx.set_depth_stencil_state(self.states.ds_sky.as_ref(), 0);
            self.ctx.set_depth_stencil_state(self.states.ds_gbuffer.as_ref(), 0);
            let (rs_shadow, rs_shadow_terrain) = if rome2 {
                (self.states.rs_shadow_rome2.clone(), self.states.rs_shadow_terrain_rome2.clone())
            } else {
                (self.states.rs_shadow.clone(), self.states.rs_shadow_terrain.clone())
            };

            self.ctx.set_rasterizer_state(rs_shadow.as_ref());

            let tree_shaders = TREE_SHADOW_PASSES.map(|(kind, tvs, tps)| (kind, self.shader(tvs).zip(self.shader(tps))));
            let tree_layout = self.shader(TREE_BRANCH_VS).and_then(|tvs| self.layout(&tvs, "tree", &tree_layout(self.game)));
            let billboard_layout = self.shader(TREE_BILLBOARD_VS).and_then(|bvs| self.layout(&bvs, "tree_billboard", &billboard_layout()));
            let (terrain_draws, draws) = (std::mem::take(&mut self.draw_lists.terrain_draws), std::mem::take(&mut self.draw_lists.draws));
            let (mut vis_terrain, mut vis) = (std::mem::take(&mut self.draw_lists.vis_terrain), std::mem::take(&mut self.draw_lists.vis));
            let terrain_material = self.terrain.active && !self.env.active;
            for c in 0..4 {
                self.ctx.set_input_layout(Some(&layout));
                let half = SHADOW_SIZE as f32 / 2.0;
                self.ctx.set_viewports(&[viewport(SHADOW_BOUNDS[c][0] * SHADOW_SIZE as f32, SHADOW_BOUNDS[c][1] * SHADOW_SIZE as f32, half, half)]);
                self.cbs.set_m("camera_VS_PS", "view_projection", game_clip(&light_rel[c], self.camera.units).data());
                self.upload();
                self.bind(&vs);
                self.bind(&ps);
                for &slot in vs.textures.values() {
                    self.ctx.vs_set_shader_resources(slot, std::slice::from_ref(&self.res.instance_ids_srv));
                }

                let view = DrawView {
                    frustum: Frustum::from_view_projection(&light_vp[c]),
                    eye: cam.eye,
                    shadow: true,

                    // Farther cascades need less detail; a caster smaller than a couple of shadow texels leaves no shadow.
                    coarser: c as i32,
                    lod_scale: self.settings.lod_scale,
                    min_radius: texel_c[c] * self.settings.shadow_min_texels,
                    quality: self.lod_quality(),
                    ..Default::default()
                };

                terrain_draws.cull(&view, &mut vis_terrain);
                draws.cull(&view, &mut vis);

                // The material terrain casts with the game's terrain shadow shaders below, not as legacy patches.
                if terrain_material {
                    vis_terrain = Visible::default();
                }

                let firsts = self.upload_instances([&terrain_draws, &draws], [&vis_terrain, &vis]);
                for v in [&vis_terrain, &vis] {
                    self.draw_stats.views += 1;
                    self.draw_stats.instances += v.instances() as u64;
                }

                self.ctx.set_vertex_buffers(1, std::slice::from_ref(&self.draw_lists.ring.buffer), &[(size_of::<RigidInstance>() as u32)], &[0]);
                let (instances, triangles) = self.draw_batches([&terrain_draws, &draws], [&vis_terrain, &vis], firsts, |_, m| !m.kind.is_tree() && m.kind != MeshKind::Water);
                self.draw_stats.shadow_instances += instances;
                self.draw_stats.shadow_triangles += triangles;
                if terrain_material {
                    self.ctx.set_rasterizer_state(rs_shadow_terrain.as_ref());
                    self.draw_stats.shadow_triangles += self.draw_terrain_shadow(scene, &light_vp[c], &light_rel[c]);
                    self.ctx.set_rasterizer_state(rs_shadow.as_ref());
                    self.ctx.set_vertex_buffers(1, std::slice::from_ref(&self.draw_lists.ring.buffer), &[(size_of::<RigidInstance>() as u32)], &[0]);
                }

                // The trees, with their own vertex format and shaders, alpha-tested on their diffuse map.
                for (kind, pair) in &tree_shaders {
                    let Some((tvs, tps)) = pair else { continue };
                    let kind_layout = if *kind == MeshKind::TreeBillboard { &billboard_layout } else { &tree_layout };
                    let Some(kind_layout) = kind_layout else { continue };
                    self.ctx.set_input_layout(Some(kind_layout));
                    self.bind(tvs);
                    self.bind(tps);
                    bind_srv(&self.ctx, tps, "s_dissolve_map", self.res.white_srv.as_ref());
                    ps_samplers(&self.ctx, tps, &self.states.s_wrap_linear);
                    let (instances, triangles) = self.draw_batches([&terrain_draws, &draws], [&vis_terrain, &vis], firsts, |r, m| {
                        if m.kind != *kind {
                            return false;
                        }

                        let srv = r.vegetation_diffuse(m);
                        bind_srv(&r.ctx, tps, "s_diffuse_map", srv.as_ref());
                        bind_srv(&r.ctx, tps, "s_diffuse_map_tree", srv.as_ref());
                        true
                    });

                    self.draw_stats.shadow_instances += instances;
                    self.draw_stats.shadow_triangles += triangles;
                }
            }

            self.draw_lists.terrain_draws = terrain_draws;
            self.draw_lists.draws = draws;
            self.draw_lists.vis_terrain = vis_terrain;
            self.draw_lists.vis = vis;
        }

        if self.settings.shadows {
            self.ctx.set_render_targets(&[], self.shadow_map.dsv.as_ref());
            self.ctx.set_depth_stencil_state(self.states.ds_gbuffer.as_ref(), 0);
            self.ctx.set_topology(D3D11_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
            self.draw_grass_shadows(scene, &light_rel, cam.eye);
        }

        self.ctx.set_render_targets(&[], None);

        // shadowmap_PS as the game fills it (the layout and constants from the memory dump).
        let size = SHADOW_SIZE as f32;
        self.cbs.set_f("shadowmap_PS", "g_vHardShadowBufferSize", &[size, size, 1.0 / size, 1.0 / size]);
        self.cbs.set_f("shadowmap_PS", "g_amHardSplit", &matrices);

        // float[5], a 16-byte stride.
        let mut dist = [0.0f32; 17];
        for (i, split) in splits.iter().enumerate() {
            dist[i * 4] = *split * self.camera.units;
        }

        self.cbs.set_f("shadowmap_PS", "g_split_distances", &dist);
        self.cbs.set_f("shadowmap_PS", "g_split_uv_texture_bounds", SHADOW_BOUNDS.as_flattened());
        self.cbs.set_f("shadowmap_PS", "g_fFadeRange", &[0.1, fade_y]);
        self.cbs.set_f("shadowmap_PS", "g_sample_bias", &[0.0]);
        self.cbs.set_f("shadowmap_PS", "g_iSplitCount", &[if self.settings.shadows { 4.0 } else { 0.0 }]);
        self.cbs.set_f("shadowmap_PS", "g_shadow_light_direction", &[light_dir.x, light_dir.y, light_dir.z]);

        // Rome 2's light pass picks the cascade by the number of split ends the pixel's view depth (game units) is past, shades nothing
        // beyond the shadow range, blends a cascade into the next over g_fFadeRange.x and fades out over .y before the last split; its baked
        // shadows are unused (their distance out of reach). The game's values (fn 0x1147aa90): range `shadow_farz` 360, fade (3, half the
        // third split), sample bias 0.1 (battle). docs/re/rome2_renderer.md.
        if rome2 {
            let end = |i: usize| splits[i] * u;
            self.cbs.set_f("shadowmap_PS", "g_vSplitDepth", &[end(1), end(2), end(3), end(4)]);
            self.cbs.set_f("shadowmap_PS", "g_vShadowRange", &[360.0, 0.0, 0.0, 0.0]);
            self.cbs.set_f("shadowmap_PS", "g_fFadeRange", &[3.0, end(3) * 0.5]);
            self.cbs.set_f("shadowmap_PS", "g_sample_bias", &[0.1]);
            self.cbs.set_f("shadowmap_PS", "g_vBakedShadowmapParams", &[0.0, 1.0e9, 0.0, 1.0]);
        }
    }

    pub(crate) fn pass_fog(&mut self) {
        let Some(ps) = self.shader(FOG_PS) else { return };
        if !self.settings.fog {
            return;
        }

        self.ctx.set_render_targets(&[], None);
        self.bind(&ps);
        bind_srv(&self.ctx, &ps, "gbuffer_channel_4_sampler", self.gbuffer.channels[3].srv.as_ref());
        bind_srv(&self.ctx, &ps, "s_sky", self.sky.cube.srv.as_ref());
        ps_samplers(&self.ctx, &ps, &self.states.s_clamp_point);
        bind_sampler(&self.ctx, &ps, "s_sky", self.states.s_clamp_linear.as_ref());

        // Fog over geometry only (stencil 1): sky pixels keep the sky.
        self.ctx.set_render_targets(&[self.gbuffer.scene_hdr_rtv()], self.gbuffer.scene_dsv().as_ref());
        self.ctx.set_depth_stencil_state(self.states.ds_light.as_ref(), 1);
        self.ctx.set_blend_state(self.states.bs_alpha.as_ref());

        self.upload();
        self.fullscreen(&ps);
        self.ctx.set_blend_state(self.states.bs_opaque.as_ref());
        null_ps_srvs(&self.ctx, 8);
    }

}
