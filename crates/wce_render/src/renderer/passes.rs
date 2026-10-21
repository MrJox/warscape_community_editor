//! The geometry, shadow and lighting passes.

use wce_foundation::math::{Float3, Float4x4, cross, inverse, look_at_lh, normalize};
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

/// The atlas quadrants of the four cascades.
pub(crate) const SHADOW_BOUNDS: [[f32; 4]; 4] = [[0.0, 0.0, 0.5, 0.5], [0.5, 0.0, 1.0, 0.5], [0.0, 0.5, 0.5, 1.0], [0.5, 0.5, 1.0, 1.0]];

impl Renderer {

    pub(crate) fn pass_gbuffer(&mut self) {
        let rtvs = self.gbuffer.rtvs();
        for rtv in rtvs.iter().flatten() {
            self.ctx.clear_render_target_view(rtv, [0.0; 4]);
        }

        if let Some(dsv) = &self.gbuffer.dsv {
            self.ctx.clear_depth_stencil_view(dsv, D3D11_CLEAR_DEPTH | D3D11_CLEAR_STENCIL, 1.0, 0);
        }

        self.ctx.set_render_targets(&rtvs, self.gbuffer.dsv.as_ref());
        self.ctx.set_depth_stencil_state(self.states.ds_gbuffer.as_ref(), 1);
        self.ctx.set_rasterizer_state(self.states.rs_back.as_ref());

        if self.draw_lists.draws.items() == 0 && self.draw_lists.terrain_draws.items() == 0 {
            return;
        }

        // The rigid vertex shaders share one layout, the tree ones another.
        let Some(vs) = self.shader(RIGID_VS) else { return };
        let Some(layout) = self.layout(&vs, "rigid_std", &rigid_layout()) else { return };
        let tree_layout = self.shader(TREE_BRANCH_VS).and_then(|tvs| self.layout(&tvs, "tree", &tree_layout()));
        let billboard_layout = self.shader(TREE_BILLBOARD_VS).and_then(|bvs| self.layout(&bvs, "tree_billboard", &billboard_layout()));
        self.cbs.set_f("tree_billboard_constants", "g_tint_colours", &[1.0; 12]);
        self.ctx.set_input_layout(Some(&layout));

        // White: no tint (the engine's values per climate have not been looked for).
        self.cbs.set_f("vegetation_tint_colours", "g_tint_colours", &[1.0; 12]);
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
        draws.cull(&view, &mut vis);

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
        let shaders = GBUFFER_PASSES.map(|(_, _, vs, ps, _)| self.shader(vs).zip(self.shader(ps)));
        let pass_of = |m: &GpuMesh| {
            if m.kind == MeshKind::TerrainBlend && blend_in_terrain {
                return None;
            }

            let opaque = GBUFFER_PASSES.iter().position(|p| p.0 == m.kind && !p.1)?;
            let alpha = GBUFFER_PASSES.iter().position(|p| p.0 == m.kind && p.1).filter(|&k| m.alpha_mode == 1 && shaders[k].is_some());
            alpha.or(Some(opaque).filter(|&k| shaders[k].is_some()))
        };

        for (pass, pair) in shaders.iter().enumerate() {
            let Some((vs, ps)) = pair.clone() else { continue };
            if !self.env.active {
                self.gpu_timer.mark(GBUFFER_PASSES[pass].4);
            }

            let pass_layout = match GBUFFER_PASSES[pass].0 {
                MeshKind::TreeBranch | MeshKind::TreeLeaf => tree_layout.as_ref(),
                MeshKind::TreeBillboard => billboard_layout.as_ref(),
                _ => Some(&layout),
            };

            let Some(pass_layout) = pass_layout else { continue };
            self.ctx.set_input_layout(Some(pass_layout));

            let slots = self.slots_of(&ps);
            let mut bound = false;
            let (instances, triangles) = self.draw_batches([&terrain_draws, &draws], [&vis_terrain, &vis], firsts, |r, m| {
                if pass_of(m) != Some(pass) {
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

                    // Trees: no burn, nothing dissolved (the dissolve amount is in the instance).
                    bind_srv(&r.ctx, &ps, "s_diffuse_burn_map", r.res.black_srv.as_ref());
                    bind_srv(&r.ctx, &ps, "s_dissolve_map", r.res.white_srv.as_ref());
                    r.upload();
                    ps_samplers(&r.ctx, &ps, &r.states.s_wrap_aniso);
                    bound = true;
                }

                if matches!(m.kind, MeshKind::TiledDirtmap | MeshKind::Dirtmap) {
                    r.cbs.set_f("rigid_config_PS", "g_uv2_tile_interval", &m.uv2_tile);
                    r.upload();
                }

                // rigidwithdirtmesh names its dirt map and mask differently.
                if m.kind == MeshKind::Dirtmap {
                    let srv = |t: usize| m.tex[t].read().ok().and_then(|tex| tex.srv.clone());
                    bind_srv(&r.ctx, &ps, "s_decal_dirt_map", srv(crate::model_cache::TEX_DETAIL).as_ref());
                    bind_srv(&r.ctx, &ps, "s_decal_dirt_mask", srv(crate::model_cache::TEX_ALPHA_MASK).as_ref());
                }

                // Trees draw their season's diffuse (as grass does).
                let seasonal = if m.kind.is_tree() { r.vegetation_diffuse(m) } else { None };

                // The billboard's textures have names of their own.
                if m.kind == MeshKind::TreeBillboard {
                    let srv = |t: usize| m.tex[t].read().ok().and_then(|tex| tex.srv.clone());
                    bind_srv(&r.ctx, &ps, "s_diffuse_map_tree", seasonal.clone().or_else(|| srv(crate::model_cache::TEX_DIFFUSE)).as_ref());
                    bind_srv(&r.ctx, &ps, "s_combined_normal_and_displacement", srv(crate::model_cache::TEX_NORMAL).as_ref());
                }

                for (t, slot) in slots.iter().enumerate().take(TEX_SLOTS) {
                    let Some(slot) = slot else { continue };
                    let srv = if t == crate::model_cache::TEX_DIFFUSE && seasonal.is_some() {
                        seasonal.clone()
                    } else {
                        m.tex[t].read().ok().and_then(|tex| tex.srv.clone())
                    };
                    r.ctx.ps_set_shader_resources(*slot, &[srv]);
                }

                true
            });

            self.draw_stats.triangles += triangles;
            if !self.env.active {
                self.draw_stats.camera_triangles += triangles;
                self.draw_stats.camera_instances += instances;
            }
        }

        // The terrain-blend meshes' geometry pass (`custom_geometry`): depth, the G-buffer depth channel and stencil 2, the mark of the terrain pixels (the blend boxes and
        // the resolve only touch those), so that the terrain textures are projected onto the meshes.
        if blend_in_terrain {
            let d = blend_mesh::BlendDraw {
                vs: blend_mesh::BLEND_VS, ps: blend_mesh::BLEND_GBUFFER_PS, targets: self.gbuffer.rtvs(), depth_stencil: self.states.ds_gbuffer.clone(), stencil_ref: 2,
                blend: self.states.bs_opaque.clone(),
            };

            self.draw_blend_meshes([&terrain_draws, &draws], [&vis_terrain, &vis], firsts, &d);
        }

        self.draw_lists.terrain_draws = terrain_draws;
        self.draw_lists.draws = draws;
        self.draw_lists.vis_terrain = vis_terrain;
        self.draw_lists.vis = vis;
    }

    pub(crate) fn pass_directional_light(&mut self) {
        let Some(ps) = self.shader(DIR_LIGHT_PS) else { return };
        self.ctx.set_render_targets(std::slice::from_ref(&self.gbuffer.hdr.rtv), self.gbuffer.dsv.as_ref());
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
        self.fullscreen(&ps);
    }

    /// The battle cascade splits (ShadowMapView, fns 0x11794320 / 0x11794430 / 0x11784c00, verified on all three dumps):
    /// - far = camera near + sum(row) with the quality row (30, 90, 210, 450) -> 781 (fn 0x11751fc0, table dll+0x1c88aa0);
    /// - range = the depth-buffer analysis (min, max visible view depth, max clamped to far), max *= shadow_split_enlargement_safe_zone (1.05);
    ///   start = min, scale = (max - min) / (far - near);
    /// - s0 = start, s_i = start + scale * (cumulative row)_i; cascade i covers [s_i - 0.1 (s_i - s_(i-1)), s_(i+1)] (shadow_split_blend_fraction
    ///   0.1); g_fFadeRange = (0.1, (s4 - (far - 0.2 (s4 - s3))) / (0.2 (s4 - s3)) * 0.2) (final_shadow_split_blend_fraction 0.2).
    pub(crate) fn pass_shadows(&mut self, scene: &RenderScene, cam: &Camera, aspect: f32, light_dir: Float3, _time_s: f32) {
        let row = SHADOW_ROWS[self.settings.shadow_quality.clamp(0, 3) as usize];
        let n = cam.znear.max(1.0);
        let far_q = n + row[0] + row[1] + row[2] + row[3];
        let (mut start, mut scale) = (n, 1.0f32);
        if self.depth_range.valid {
            let (lo, hi) = (self.depth_range.min, self.depth_range.max.min(far_q) * 1.05);
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
        let cam_view = look_at_lh(cam.eye, cam.target, Float3::new(0.0, 1.0, 0.0));
        let inv_view = inverse(&cam_view);
        let tan_y = (cam.fov_y * 0.5).tan();
        let tan_x = tan_y * aspect;
        let view_dir = normalize(cam.target - cam.eye);
        let side = normalize(cross(Float3::new(0.0, 1.0, 0.0), view_dir));

        let mut matrices = [0.0f32; 64];
        let mut light_vp = [Float4x4::IDENTITY; 4];
        let mut texel_c = [0.0f32; 4];
        for c in 0..4 {
            // The frustum slice's corners (fn 0x11698df0), fitted as the game does (shadow_fit.rs).
            let pts: [Float3; 8] = std::array::from_fn(|k| {
                let d = if k & 4 != 0 { splits[c + 1] } else { slice_near[c] };
                let v = Float3::new(if k & 1 != 0 { 1.0 } else { -1.0 } * tan_x * d, if k & 2 != 0 { 1.0 } else { -1.0 } * tan_y * d, d);
                inv_view.transform_point(v)
            });

            let fit = shadow_fit::fit(&pts, light_dir, view_dir, side, slice_near[c]);
            light_vp[c] = fit.view_proj;
            texel_c[c] = fit.width / (SHADOW_SIZE / 2) as f32;

            // World -> atlas uv (y down) + depth: the lighting shader's lookup.
            let (bx, by) = (SHADOW_BOUNDS[c][0], SHADOW_BOUNDS[c][1]);
            let mut to_uv = Float4x4::IDENTITY;
            to_uv.m[0][0] = 0.25;
            to_uv.m[0][3] = bx + 0.25;
            to_uv.m[1][1] = -0.25;
            to_uv.m[1][3] = by + 0.25;
            matrices[c * 16..c * 16 + 16].copy_from_slice(rel_clip(&(to_uv * light_vp[c]), self.camera.origin, self.camera.units).data());
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
            self.ctx.set_rasterizer_state(self.states.rs_shadow.as_ref());

            let tree_shaders = TREE_SHADOW_PASSES.map(|(kind, tvs, tps)| (kind, self.shader(tvs).zip(self.shader(tps))));
            let tree_layout = self.shader(TREE_BRANCH_VS).and_then(|tvs| self.layout(&tvs, "tree", &tree_layout()));
            let billboard_layout = self.shader(TREE_BILLBOARD_VS).and_then(|bvs| self.layout(&bvs, "tree_billboard", &billboard_layout()));
            let (terrain_draws, draws) = (std::mem::take(&mut self.draw_lists.terrain_draws), std::mem::take(&mut self.draw_lists.draws));
            let (mut vis_terrain, mut vis) = (std::mem::take(&mut self.draw_lists.vis_terrain), std::mem::take(&mut self.draw_lists.vis));
            let terrain_material = self.terrain.active && !self.env.active;
            for c in 0..4 {
                self.ctx.set_input_layout(Some(&layout));
                let half = SHADOW_SIZE as f32 / 2.0;
                self.ctx.set_viewports(&[viewport(SHADOW_BOUNDS[c][0] * SHADOW_SIZE as f32, SHADOW_BOUNDS[c][1] * SHADOW_SIZE as f32, half, half)]);
                self.cbs.set_m("camera_VS_PS", "view_projection", rel_clip(&light_vp[c], self.camera.origin, self.camera.units).data());
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
                    self.ctx.set_rasterizer_state(self.states.rs_shadow_terrain.as_ref());
                    self.draw_stats.shadow_triangles += self.draw_terrain_shadow(scene, &light_vp[c]);
                    self.ctx.set_rasterizer_state(self.states.rs_shadow.as_ref());
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
            self.draw_grass_shadows(scene, &light_vp, cam.eye);
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
        self.ctx.set_render_targets(std::slice::from_ref(&self.gbuffer.hdr.rtv), self.gbuffer.dsv.as_ref());
        self.ctx.set_depth_stencil_state(self.states.ds_light.as_ref(), 1);
        self.ctx.set_blend_state(self.states.bs_alpha.as_ref());

        self.upload();
        self.fullscreen(&ps);
        self.ctx.set_blend_state(self.states.bs_opaque.as_ref());
        null_ps_srvs(&self.ctx, 8);
    }

}
