//! The water passes: river ribbons and sea / lake planes of the scene's water entities, drawn with the game's own shaders: rigidriver
//! (vs30_main_battle_river, ps30_main_battle_river), waterplane (vs30_water_battle, ps30_battle_ultra_water_quality) and waterrefraction
//! (ps30_main_3_channel).
//!
//! The frame order (the game's, as far as the shaders show it): the lit scene is copied half-size by waterrefraction into s_terrain_refraction,
//! colour in rgb and the refraction weight (from the ground's world height) in alpha; the sea plane is drawn over the lit scene and then covered
//! by the fog pass (its shader has no fog); the river is drawn after the fog (its shader fogs itself). Both pixel shaders write four targets:
//! the G-buffer channels 1 .. 3 (normal, albedo, specular) and the final lit colour into the HDR target, opaque, with the scene's depth test.

use std::collections::BTreeMap;

use wce_foundation::wce_prof;
use wce_scene::render_scene::{EntityId, Payload, RenderScene};

use super::*;
use crate::shader_lib::{bind_sampler, bind_srv};

const RIVER_VS: &str = "dx11_sm_3_0_rigidriver.hlsl_vs_vs30_main_battle_river";
const RIVER_PS: &str = "dx11_sm_3_0_rigidriver.hlsl_ps_ps30_main_battle_river";

/// The pixel shaders with shadows, the variant the game's lighting uses (`DIR_LIGHT_PS`: hardware gather, shader model 5): the same inputs
/// as the plain ones (so the same vertex shaders), plus `shadowmap_PS`, `g_tShadowBuffer_SM50` and its comparison sampler. The water
/// receives the shadows of what stands over it, as the game's does with shadows on.
const RIVER_PS_SHADOWS: &str = "dx11_enable_shadows_shadows_hardware_gather_5_0_sm_5_0_rigidriver.hlsl_ps_ps30_main_battle_river";
const SEA_PS_SHADOWS: &str = "dx11_enable_shadows_shadows_hardware_gather_5_0_sm_5_0_waterplane.hlsl_ps_ps30_battle_ultra_water_quality";
const SEA_VS: &str = "dx11_sm_3_0_waterplane.hlsl_vs_vs30_water_battle";

/// The water planes' pixel shader at a gfx_water_quality level (the engine's techniques `water_low` .. `water_ultra`, fn 0x116ec510;
/// Rome 2's extreme draws ultra). Only ultra has the variants with shadows.
pub(crate) fn sea_ps(quality: i32) -> String {
    format!("dx11_sm_3_0_waterplane.hlsl_ps_ps30_battle_{}_water_quality", water_level(quality))
}

/// The water quality level's name in the shaders' entry points.
pub(crate) fn water_level(quality: i32) -> &'static str {
    ["low", "medium", "high", "ultra"][quality.clamp(0, 3) as usize]
}
const REFRACTION_PS: &str = "dx11_sm_3_0_waterrefraction.hlsl_ps_ps30_main_3_channel";
/// With MSAA: the MS light buffer's (`t_light_accumulation_buffer_ms`; its technique keeps the ps30 name).
const REFRACTION_MS_PS: &str = "dx11_sm_4_1_waterrefraction.hlsl_ps_ps30_main_3_channel_ms";

/// water_refraction_PS.g_refraction_depth (and directional_light_PS's, which writes the same weight into the light buffer's alpha): the weight of
/// the refraction is saturate(-ground_y * this) below the water level. 0.75 is the value the engine's constructors of both cbuffers store
/// (fn 0x116d45c0 and 0x117037e0: 0x3f400000); a runtime override has not been looked for.
const REFRACTION_DEPTH: f32 = 0.75;

/// The GPU copy of a water mesh.
#[derive(Clone, Debug)]
pub(crate) struct WaterMesh {
    vb: Option<ID3D11Buffer>,
    ib: Option<ID3D11Buffer>,
    index_count: u32,
    version: u64,
}

/// The water pass's state.
#[derive(Default)]
pub(crate) struct WaterGpu {
    meshes: BTreeMap<EntityId, WaterMesh>,

    /// The half-resolution scene colour (rgb) + the refraction weight (a), s_terrain_refraction.
    pub(crate) refr: Target,
    refr_w: u32,
    refr_h: u32,
    river_inst_vb: Option<ID3D11Buffer>,
    sea_inst_vb: Option<ID3D11Buffer>,

    /// The game units per scene unit the instance rows hold.
    inst_units: f32,
    inst_origin: wce_foundation::math::Float3,
    pub(crate) ds: Option<ID3D11DepthStencilState>,

    /// The sea planes' ocean pass (renderer/ocean.rs).
    pub(crate) ocean: super::ocean::OceanGpu,
}

/// The size of a water vertex.
const WATER_VERTEX: u32 = 32;

impl Renderer {

    fn water_refraction(&mut self) {
        let (tw, th) = ((self.gbuffer.w / 2).max(1), (self.gbuffer.h / 2).max(1));
        if self.water.refr.tex.is_none() || self.water.refr_w != tw || self.water.refr_h != th {
            self.water.refr = make_target(&self.dev, tw, th, DXGI_FORMAT_R16G16B16A16_FLOAT);
            self.water.refr_w = tw;
            self.water.refr_h = th;
        }

        let Some(ps) = self.shader(if self.gbuffer.ms.is_some() { REFRACTION_MS_PS } else { REFRACTION_PS }) else { return };
        self.cbs.set_f("water_refraction_PS", "g_refraction_depth", &[REFRACTION_DEPTH]);
        null_ps_srvs(&self.ctx, 16);
        self.ctx.set_render_targets(std::slice::from_ref(&self.water.refr.rtv), None);
        self.ctx.set_viewports(&[viewport(0.0, 0.0, tw as f32, th as f32)]);
        self.ctx.set_blend_state(self.states.bs_opaque.as_ref());
        self.ctx.set_depth_stencil_state(self.states.ds_off.as_ref(), 0);
        self.ctx.set_rasterizer_state(self.states.rs_none.as_ref());

        ps_samplers(&self.ctx, &ps, &self.states.s_clamp_linear);
        bind_sampler(&self.ctx, &ps, "gbuffer_channel_4_sampler", self.states.s_clamp_point.as_ref());
        bind_srv(&self.ctx, &ps, "gbuffer_channel_4_sampler", self.gbuffer.depth_srv().as_ref());
        bind_srv(&self.ctx, &ps, "s_light_accumulation_buffer", self.gbuffer.hdr.srv.as_ref());
        if let Some(ms) = &self.gbuffer.ms {
            bind_srv(&self.ctx, &ps, "t_light_accumulation_buffer_ms", ms.hdr.srv.as_ref());
        }

        self.upload();
        self.fullscreen(&ps);
        null_ps_srvs(&self.ctx, 16);
        self.ctx.set_viewports(&[viewport(0.0, 0.0, self.gbuffer.w as f32, self.gbuffer.h as f32)]);
    }

    pub(crate) fn pass_water(&mut self, scene: &RenderScene, rivers: bool) {
        // The GPU copies of the meshes: rebuilt when an entity's version moves on, dropped with the entity.
        let mut keep = BTreeMap::new();
        let mut any = false;
        let water = KindIndex::entities(&self.kinds.water, scene);
        for e in water.iter().copied() {
            let Payload::Water(w) = &e.payload else { continue };
            if w.verts.is_empty() || w.indices.is_empty() {
                continue;
            }

            let mesh = match self.water.meshes.get(&e.id).filter(|m| m.version == w.version && m.vb.is_some()) {
                Some(mesh) => mesh.clone(),
                None => {
                    let verts = w.verts.iter().flat_map(|v| [v.x, v.y, v.z, v.across, v.u, v.v, v.flow, v.fade]).collect::<Vec<_>>();
                    let indices = w.indices.iter().flat_map(|i| i.to_le_bytes()).collect::<Vec<_>>();
                    let (vb, ib) = (immutable_buffer(&self.dev, &f32_bytes(&verts), D3D11_BIND_VERTEX_BUFFER), immutable_buffer(&self.dev, &indices, D3D11_BIND_INDEX_BUFFER));
                    if vb.is_none() || ib.is_none() {
                        continue;
                    }

                    WaterMesh { vb, ib, index_count: w.indices.len() as u32, version: w.version }
                }
            };

            keep.insert(e.id, mesh);
            if w.river == rivers && scene.drawable(e) {
                any = true;
            }
        }

        self.water.meshes = keep;

        // The water meshes of models (water planes, fountains) the camera's G-buffer pass found visible: drawn with the sea plane.
        let model_water = !rivers && [(&self.draw_lists.terrain_draws, &self.draw_lists.vis_terrain), (&self.draw_lists.draws, &self.draw_lists.vis)]
            .iter()
            .any(|(set, vis)| vis.active.iter().any(|&b| set.batch_mesh(b).kind == crate::model_cache::MeshKind::Water));
        if !any && !model_water {
            return;
        }

        wce_prof!("render.water");
        let vs = self.shader(if rivers { RIVER_VS } else { SEA_VS });
        let quality = if self.game == super::game::RenderGame::Rome2 { super::rome2_water(self.settings.water_quality, false) } else { self.settings.water_quality };
        let sea_ps = sea_ps(quality);
        let ultra = quality >= 3;
        let shadows = if self.game == super::game::RenderGame::Rome2 { self.rome2_shadows_on() } else { self.settings.shadows };
        let ps = match (rivers, shadows) {
            (true, true) => self.shader(RIVER_PS_SHADOWS).or_else(|| self.shader(RIVER_PS)),
            (true, false) => self.shader(RIVER_PS),
            (false, true) if ultra => self.shader(SEA_PS_SHADOWS).or_else(|| self.shader(&sea_ps)),
            (false, _) => self.shader(&sea_ps),
        };
        let (Some(vs), Some(ps)) = (vs, ps) else { return };
        if self.water.ds.is_none() {
            // Geometry is stencil 1: the fog pass covers the sea plane.
            let replace = D3D11_DEPTH_STENCILOP_DESC { StencilFailOp: D3D11_STENCIL_OP_KEEP, StencilDepthFailOp: D3D11_STENCIL_OP_KEEP, StencilPassOp: D3D11_STENCIL_OP_REPLACE, StencilFunc: D3D11_COMPARISON_ALWAYS };
            let dd = D3D11_DEPTH_STENCIL_DESC {
                DepthEnable: true.into(), DepthWriteMask: D3D11_DEPTH_WRITE_MASK_ALL, DepthFunc: D3D11_COMPARISON_LESS_EQUAL, StencilEnable: true.into(),
                StencilReadMask: 0xFF, StencilWriteMask: 0xFF, FrontFace: replace, BackFace: replace,
            };

            self.water.ds = self.dev.create_depth_stencil_state(&dd).ok();
        }

        // The instance data of the vertex shaders (one instance each): the rows of the 3x4 world matrix, the scene -> game units scale (the
        // vertices are in world space).
        let (u, o) = (self.camera.units, self.camera.origin);
        if self.water.river_inst_vb.is_none() || self.water.inst_units != u || self.water.inst_origin != o {
            (self.water.inst_units, self.water.inst_origin) = (u, o);

            // P2 .. P4 rows (moved to the render origin), P5 (x >= 1 flips the uv).
            let river = [u, 0.0, 0.0, -o.x * u, 0.0, u, 0.0, -o.y * u, 0.0, 0.0, u, -o.z * u, 0.0, 0.0, 0.0, 0.0];

            // P1 .. P3 rows, P4 .. P6 unused.
            let mut sea = [0.0f32; 24];
            sea[..12].copy_from_slice(&river[..12]);
            self.water.river_inst_vb = immutable_buffer(&self.dev, &f32_bytes(&river), D3D11_BIND_VERTEX_BUFFER);
            self.water.sea_inst_vb = immutable_buffer(&self.dev, &f32_bytes(&sea), D3D11_BIND_VERTEX_BUFFER);
        }

        self.water_refraction();
        if !rivers {
            self.water_reflection_ssr();
        }

        // The sea planes: the game's ocean shaders (renderer/ocean.rs); rivers and the models' water planes below.
        if !rivers {
            self.draw_ocean(scene, &water);
            if !model_water {
                null_ps_srvs(&self.ctx, 16);
                self.ctx.set_render_targets(&[self.gbuffer.scene_hdr_rtv()], self.gbuffer.scene_dsv().as_ref());
                return;
            }
        }
        let nt = self.textures.get(if rivers { "battleterrain/river/river_normal.dds" } else { "sea/water_detail_normal.dds" }, false);
        let f4 = DXGI_FORMAT_R32G32B32A32_FLOAT;
        let layout = if rivers {
            let mut el = vec![element(c"POSITION", 0, f4, 0, 0, false), element(c"POSITION", 1, f4, 0, 16, false)];
            for i in 0..4 {
                el.push(element(c"POSITION", 2 + i, f4, 1, i * 16, true));
            }

            self.layout(&vs, "river", &el)
        } else {
            let mut el = vec![element(c"POSITION", 0, DXGI_FORMAT_R32G32B32_FLOAT, 0, 0, false), element(c"TEXCOORD", 0, DXGI_FORMAT_R32G32_FLOAT, 0, 16, false)];
            for i in 0..6 {
                el.push(element(c"POSITION", 1 + i, f4, 1, i * 16, true));
            }

            self.layout(&vs, "sea", &el)
        };

        let Some(layout) = layout else { return };

        // The constants the river's vertex shader reads for the terrain height: the game puts the ribbon on the low-res height map; here the mesh
        // carries its own world height (rebuilt from the tile's terrain), so the map's contribution is switched off (scale 0, a black map) and
        // the dimensions are kept finite.
        let cbs = &mut self.cbs;
        cbs.set_f("vs_height_map_cb", "g_height_map_lf_scale", &[0.0]);
        cbs.set_f("vs_height_map_cb", "g_height_map_hf_scale", &[1.0]);
        cbs.set_f("vs_height_map_cb", "g_height_map_vertical_offset", &[0.0]);
        cbs.set_f("vs_height_map_cb", "g_height_map_dimension", &[256.0, 256.0, 1.0 / 256.0, 1.0 / 256.0]);
        cbs.set_f("vs_height_map_cb", "g_world_dimensions", &[1024.0, 1024.0]);
        cbs.set_f("vs_height_map_cb", "g_lf_uv_offset", &[0.0]);
        cbs.set_f("river_CB", "g_world_size", &[1.0, 1.0]);
        cbs.set_f("river_CB", "g_flow_rate", &[0.0]);
        cbs.set_f("river_CB", "g_vertical_offset", &[0.0]);
        self.upload();

        null_ps_srvs(&self.ctx, 16);
        // Rome 2's river and water plane shaders are forward passes: the lit colour only.
        let rtvs = if self.game == super::game::RenderGame::Rome2 {
            vec![self.gbuffer.scene_hdr_rtv()]
        } else {
            self.gbuffer.scene_rtvs()[..3].iter().cloned().chain([self.gbuffer.scene_hdr_rtv()]).collect()
        };
        self.ctx.set_render_targets(&rtvs, self.gbuffer.scene_dsv().as_ref());
        // Rivers are stencil 2 (the fog pass, stencil 1, leaves them: their shader fogs itself); the water planes 1.
        self.ctx.set_depth_stencil_state(self.water.ds.as_ref(), if rivers { 2 } else { 1 });
        self.ctx.set_blend_state(self.states.bs_opaque.as_ref());
        self.ctx.set_rasterizer_state(self.states.rs_none.as_ref());
        self.ctx.set_input_layout(Some(&layout));
        self.ctx.set_topology(D3D11_PRIMITIVE_TOPOLOGY_TRIANGLELIST);

        self.bind(&vs);
        self.bind(&ps);
        let ctx = &self.ctx;
        bind_srv(ctx, &vs, "t_low_res_height_map", self.res.black_srv.as_ref());
        bind_srv(ctx, &vs, "s_low_res_height_map", self.res.black_srv.as_ref());
        ps_samplers(ctx, &ps, &self.states.s_clamp_linear);
        bind_sampler(ctx, &ps, "s_normal", self.states.s_wrap_linear.as_ref());
        bind_sampler(ctx, &ps, "s_environment", self.states.s_wrap_linear.as_ref());
        bind_sampler(ctx, &ps, "s_terrain_refraction", self.states.s_clamp_point.as_ref());
        bind_sampler(ctx, &ps, "gbuffer_channel_4_sampler", self.states.s_clamp_point.as_ref());
        bind_sampler(ctx, &ps, "s_sky", self.states.s_clamp_linear.as_ref());
        let normal = nt.and_then(|t| t.read().ok().and_then(|t| t.srv.clone())).or_else(|| self.res.black_srv.clone());
        bind_srv(ctx, &ps, "s_normal", normal.as_ref());
        bind_srv(ctx, &ps, "s_environment", if self.env.valid { self.env.cube.srv.as_ref() } else { self.sky.cube.srv.as_ref() });
        bind_srv(ctx, &ps, "s_sky", self.sky.cube.srv.as_ref());
        bind_srv(ctx, &ps, "s_terrain_refraction", self.water.refr.srv.as_ref());
        bind_srv(ctx, &ps, "gbuffer_channel_4_sampler", self.gbuffer.depth_srv().as_ref());
        bind_srv(ctx, &ps, "g_tShadowBuffer_SM50", self.shadow_map.srv.as_ref());
        self.bind_baked_shadow(&ps);
        bind_sampler(ctx, &ps, "sShadowBuffer_SM50", self.states.s_shadow_cmp.as_ref());
        // Rome 2's planar reflection (the mirrored sky, as the sea's).
        let reflection = self.water_reflection_srv();
        bind_srv(ctx, &ps, "s_terrain_reflection", reflection.as_ref());
        let inst = if rivers { self.water.river_inst_vb.clone() } else { self.water.sea_inst_vb.clone() };
        let inst_stride = if rivers { 64u32 } else { 96 };
        for e in water.iter().copied() {
            let Payload::Water(w) = &e.payload else { continue };
            let Some(mesh) = self.water.meshes.get(&e.id) else { continue };
            if !rivers || !w.river || !scene.drawable(e) {
                continue;
            }

            ctx.set_vertex_buffers(0, &[mesh.vb.clone(), inst.clone()], &[WATER_VERTEX, inst_stride], &[0, 0]);
            ctx.set_index_buffer(mesh.ib.as_ref(), DXGI_FORMAT_R32_UINT, 0);
            ctx.draw_indexed_instanced(mesh.index_count, 1, 0, 0, 0);

            self.draw_stats.draw_calls += 1;
        }

        // The models' water meshes: the same shaders, the vertices in model space (f16 position) and the instance's world rows from the
        // G-buffer pass's instances (POSITION1..3; the shader reads nothing else of them).
        if model_water {
            // The shader declares the sea quad's inputs too (TEXCOORD0, POSITION4..6), unread: the uv and the instance's other rows.
            let mut el = vec![element(c"POSITION", 0, DXGI_FORMAT_R16G16B16A16_FLOAT, 0, 0, false), element(c"TEXCOORD", 0, DXGI_FORMAT_R16G16_FLOAT, 0, 8, false)];
            for i in 0..6 {
                el.push(element(c"POSITION", 1 + i, f4, 1, i * 16, true));
            }

            if let Some(model_layout) = self.layout(&vs, "sea_model", &el) {
                self.ctx.set_input_layout(Some(&model_layout));
                self.ctx.set_vertex_buffers(1, std::slice::from_ref(&self.draw_lists.ring.buffer), &[(size_of::<crate::model_cache::RigidInstance>() as u32)], &[0]);
                let (terrain_draws, draws) = (std::mem::take(&mut self.draw_lists.terrain_draws), std::mem::take(&mut self.draw_lists.draws));
                let (vis_terrain, vis) = (std::mem::take(&mut self.draw_lists.vis_terrain), std::mem::take(&mut self.draw_lists.vis));
                let firsts = self.draw_lists.gbuffer_firsts;
                let (_, triangles) = self.draw_batches([&terrain_draws, &draws], [&vis_terrain, &vis], firsts, |_, m| m.kind == crate::model_cache::MeshKind::Water);
                self.draw_stats.triangles += triangles;
                self.draw_lists.terrain_draws = terrain_draws;
                self.draw_lists.draws = draws;
                self.draw_lists.vis_terrain = vis_terrain;
                self.draw_lists.vis = vis;
            }
        }

        null_ps_srvs(&self.ctx, 16);
        self.ctx.set_render_targets(&[self.gbuffer.scene_hdr_rtv()], self.gbuffer.scene_dsv().as_ref());
    }
}
