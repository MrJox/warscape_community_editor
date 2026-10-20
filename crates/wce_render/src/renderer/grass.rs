//! The grass pass: the scene's grass entities (one per grass model, its patches as instances) drawn with the game's rigidgrass shaders
//! (vs30_main_grass, ps30_gbuffer_grass) into the G-buffer, after the terrain and before the decals (they paint over it, as in the game).
//! docs/re/vegetation.md has the RE.
//!
//! What the shaders do (from their code): the vertex shader places the patch with the instance's 3x4 world rows (TEXCOORD1..3, with the
//! mesh's pivot folded into the translation), sways the vertices whose colour alpha is set (the blade tips) with the wind, and sinks them
//! towards the ground with the camera distance: at 100 m (`g_inv_lod_distance`) the tips are at the roots. The pixel shader alpha-tests the
//! diffuse against `commontextures/dissolve.dds` (blades seen edge-on fade out), darkens and details the colour with the terrain's
//! mid / near distance detail maps (`ps_height_map_cb`), and writes the G-buffer normal, sqrt(albedo), a white specular and z / w; its alpha
//! fades the patch into the ground with distance where the scene behind it is the ground (`s_depth_map`, the terrain's view depth). The
//! blend is the source alpha over the colour channels (the game's grass gbuffer state). With the constructor's `g_far_addition` (-1) the
//! far-diffuse tint is off, and with no burn (TEXCOORD4 = 0) the fire and burn maps don't count.
//!
//! Only the patches within [`GRASS_DISTANCE`] of the camera are drawn, found through a grid of [`CELL`]-metre cells.

use std::collections::HashMap;
use std::sync::Arc;

use wce_foundation::math::{Float3, Float4x4};
use wce_foundation::wce_prof;
use wce_scene::render_scene::{Payload, RenderScene};

use super::*;
use crate::draw_set::Frustum;
use crate::shader_lib::{bind_sampler, bind_srv};

const GRASS_VS: &str = "dx11_sm_3_0_rigidgrass.hlsl_vs_vs30_main_grass";
const GRASS_PS: &str = "dx11_sm_3_0_rigidgrass.hlsl_ps_ps30_gbuffer_grass";
const GRASS_SHADOW_VS: &str = "dx11_sm_3_0_rigidgrass.hlsl_vs_vs30_shadow_grass";
const GRASS_SHADOW_PS: &str = "dx11_sm_3_0_rigidgrass.hlsl_ps_ps30_shadow_grass";
const DISSOLVE_MAP: &str = "commontextures/dissolve.dds";

/// How far the grass is drawn: where the vertex shader has sunk it into the ground (1 / g_inv_lod_distance).
pub(crate) const GRASS_DISTANCE: f32 = 100.0;

/// The grid the patches are found through.
const CELL: f32 = 32.0;

/// A patch's bounding radius (the models are about 10 m across).
const PATCH_RADIUS: f32 = 6.0;

/// Floats of one instance: TEXCOORD1..3 (world rows), TEXCOORD4 (burn, unused).
const INSTANCE_FLOATS: usize = 16;

#[derive(Default)]
pub(crate) struct GrassGpu {
    inst_vb: Option<ID3D11Buffer>,
    inst_capacity: usize,
    bs: Option<ID3D11BlendState>,

    /// The scene's grass, rebuilt when the scene changes (its address and structure revision: the patches are in the payload, an entity
    /// moving does not move them).
    scene_id: usize,
    revision: u64,
    sets: Vec<GrassSet>,

    /// The scene's vegetation season (see `RenderScene::vegetation_season`), and which texture variants exist.
    pub(crate) season: String,
    variants: HashMap<String, bool>,
}

/// One grass entity: its model, patches and their grid.
struct GrassSet {
    model: String,
    instances: Arc<Vec<[f32; 12]>>,
    cells: HashMap<(i32, i32), Vec<u32>>,
}

fn cell_of(x: f32, z: f32) -> (i32, i32) {
    ((x / CELL).floor() as i32, (z / CELL).floor() as i32)
}

impl Renderer {

    fn collect_grass(&mut self, scene: &RenderScene) {
        let id = std::ptr::from_ref(scene) as usize;
        let g = &mut self.grass;
        if g.scene_id == id && g.revision == scene.structure_revision() {
            return;
        }

        g.scene_id = id;
        g.revision = scene.structure_revision();
        g.sets.clear();
        for e in scene.entities() {
            let Payload::Grass(grass) = &e.payload else { continue };
            if !scene.drawable(e) || grass.instances.is_empty() {
                continue;
            }

            let mut cells: HashMap<(i32, i32), Vec<u32>> = HashMap::new();
            for (k, m) in grass.instances.iter().enumerate() {
                cells.entry(cell_of(m[3], m[11])).or_default().push(k as u32);
            }

            g.sets.push(GrassSet { model: grass.model.clone(), instances: grass.instances.clone(), cells });
        }
    }

    /// A vegetation mesh's diffuse: its season's variant (the game's texture resolver FUN_1186c880, with seasons:
    /// `<prefix>_<season>_diffuse<rest>` where that file exists), else its own.
    pub(crate) fn vegetation_diffuse(&mut self, mesh: &crate::model_cache::GpuMesh) -> Option<ID3D11ShaderResourceView> {
        let srv = |t: Option<crate::model_cache::SharedTexture>| t.and_then(|t| t.read().ok().and_then(|t| t.srv.clone()));
        let season = self.grass.season.clone();
        let seasonal = (!season.is_empty()).then(|| self.diffuse_variant(mesh, &season)).flatten();
        seasonal.and_then(|p| srv(self.textures.get(&p, true))).or_else(|| srv(Some(mesh.tex[crate::model_cache::TEX_DIFFUSE].clone())))
    }

    /// The resolver's `<prefix>_<tag>_diffuse<rest>` of the mesh's diffuse, if that file exists (looked up once).
    fn diffuse_variant(&mut self, mesh: &crate::model_cache::GpuMesh, tag: &str) -> Option<String> {
        let at = mesh.diffuse_path.find("_diffuse")?;
        let path = format!("{}_{tag}{}", &mesh.diffuse_path[..at], &mesh.diffuse_path[at..]);
        if let Some(found) = self.grass.variants.get(&path) {
            return found.then_some(path);
        }

        let found = self.files.exists(&path);
        self.grass.variants.insert(path.clone(), found);
        found.then_some(path)
    }

    /// The burnt look the shader blends in by the instance's burn (TEXCOORD4): the resolver's `_br` variant, black without one.
    fn grass_burn(&mut self, mesh: &crate::model_cache::GpuMesh) -> Option<ID3D11ShaderResourceView> {
        self.diffuse_variant(mesh, "br")
            .and_then(|p| self.textures.get(&p, true))
            .and_then(|t| t.read().ok().and_then(|t| t.srv.clone()))
            .or_else(|| self.res.black_srv.clone())
    }

    fn grass_layout(&mut self, vs: &GameShader) -> Option<ID3D11InputLayout> {
        let b4 = DXGI_FORMAT_B8G8R8A8_UNORM;
        let mut el = vec![
            element(c"POSITION", 0, DXGI_FORMAT_R16G16B16A16_FLOAT, 0, 0, false),
            element(c"TEXCOORD", 0, DXGI_FORMAT_R32G32_FLOAT, 0, 8, false),
            element(c"NORMAL", 0, b4, 0, 16, false),
            element(c"NORMAL", 1, b4, 0, 20, false),
            element(c"COLOR", 0, b4, 0, 24, false),
        ];
        for i in 1..=4 {
            el.push(element(c"TEXCOORD", i, DXGI_FORMAT_R32G32B32A32_FLOAT, 1, (i - 1) * 16, true));
        }

        self.layout(vs, "grass", &el)
    }

    /// The patches within [`GRASS_DISTANCE`] (game units) of `eye` (and in the frustum, if any): their instances per model mesh (the pivot
    /// goes into the translation) in game units, in runs of (model, mesh, count).
    fn gather_grass(&self, eye: Float3, frustum: Option<&Frustum>) -> (Vec<f32>, Vec<(Arc<crate::model_cache::GpuModel>, usize, u32)>) {
        let units = self.camera.units;
        let reach = GRASS_DISTANCE / units + PATCH_RADIUS;
        let (c0, c1) = (cell_of(eye.x - reach, eye.z - reach), cell_of(eye.x + reach, eye.z + reach));
        let mut all: Vec<f32> = vec![];
        let mut runs = vec![];
        for set in &self.grass.sets {
            let Some(model) = self.models.model(&set.model) else { continue };
            let mut near: Vec<u32> = vec![];
            for cx in c0.0..=c1.0 {
                for cz in c0.1..=c1.1 {
                    let Some(list) = set.cells.get(&(cx, cz)) else { continue };
                    for &k in list {
                        let m = &set.instances[k as usize];
                        let (dx, dy, dz) = (m[3] - eye.x, m[7] - eye.y, m[11] - eye.z);
                        if dx * dx + dy * dy + dz * dz <= reach * reach && !frustum.is_some_and(|f| f.outside(&[m[3], m[7], m[11]], PATCH_RADIUS)) {
                            near.push(k);
                        }
                    }
                }
            }

            if near.is_empty() {
                continue;
            }

            for (mi, mesh) in model.meshes.iter().enumerate() {
                if mesh.kind != MeshKind::Grass {
                    continue;
                }

                let o = mesh.offset;
                for &k in &near {
                    let m = &set.instances[k as usize];
                    for r in 0..3 {
                        let t = m[r * 4 + 3] + m[r * 4] * o.x + m[r * 4 + 1] * o.y + m[r * 4 + 2] * o.z;
                        all.extend_from_slice(&[m[r * 4] * units, m[r * 4 + 1] * units, m[r * 4 + 2] * units, t * units]);
                    }

                    all.extend_from_slice(&[0.0; 4]);
                }

                runs.push((model.clone(), mi, near.len() as u32));
            }
        }

        (all, runs)
    }

    fn upload_grass(&mut self, all: &[f32]) -> Option<ID3D11Buffer> {
        let total = all.len() / INSTANCE_FLOATS;
        if total == 0 {
            return None;
        }

        if total > self.grass.inst_capacity {
            self.grass.inst_capacity = (total * 2).max(16384);
            self.grass.inst_vb = dynamic_buffer(&self.dev, self.grass.inst_capacity * INSTANCE_FLOATS * 4, D3D11_BIND_VERTEX_BUFFER);
        }

        let inst_vb = self.grass.inst_vb.clone()?;
        self.ctx.map(&inst_vb, 0, D3D11_MAP_WRITE_DISCARD, 0).is_ok_and(|mut ms| ms.write(0, all)).then_some(inst_vb)
    }

    /// The grass casters (vs30_shadow_grass / ps30_shadow_grass: the same sway and sinking, alpha-tested on the diffuse times how much the
    /// blade faces the light, against the dissolve map) into the shadow cascades: the patches near the camera, every cascade. The game draws
    /// them there (FUN_118900c0: the shadow pass picks the grass shader's `shadowmap` technique, as the leaves').
    pub(crate) fn draw_grass_shadows(&mut self, scene: &RenderScene, light_vp: &[Float4x4; 4], eye: Float3) {
        self.collect_grass(scene);
        if self.grass.sets.is_empty() {
            return;
        }

        let (Some(vs), Some(ps)) = (self.shader(GRASS_SHADOW_VS), self.shader(GRASS_SHADOW_PS)) else { return };
        let Some(layout) = self.grass_layout(&vs) else { return };
        let (all, runs) = self.gather_grass(eye, None);
        let Some(inst_vb) = self.upload_grass(&all) else { return };
        let dissolve = self.textures.get(DISSOLVE_MAP, false).and_then(|t| t.read().ok().and_then(|t| t.srv.clone())).or_else(|| self.res.white_srv.clone());
        self.ctx.set_input_layout(Some(&layout));
        self.ctx.set_rasterizer_state(self.states.rs_none.as_ref());
        self.bind(&vs);
        self.bind(&ps);
        ps_samplers(&self.ctx, &ps, &self.states.s_wrap_linear);
        bind_srv(&self.ctx, &ps, "s_dissolve_map", dissolve.as_ref());
        for (c, vp) in light_vp.iter().enumerate() {
            let half = SHADOW_SIZE as f32 / 2.0;
            self.ctx.set_viewports(&[viewport(super::passes::SHADOW_BOUNDS[c][0] * SHADOW_SIZE as f32, super::passes::SHADOW_BOUNDS[c][1] * SHADOW_SIZE as f32, half, half)]);
            self.cbs.set_m("camera_VS_PS", "view_projection", game_clip(vp, self.camera.units).data());
            self.upload();
            let mut first = 0u32;
            for (model, mi, count) in &runs {
                let mesh = &model.meshes[*mi];
                let diffuse = self.vegetation_diffuse(mesh);
                bind_srv(&self.ctx, &ps, "s_diffuse_map", diffuse.as_ref());
                self.ctx.set_vertex_buffers(0, &[mesh.vb.clone(), Some(inst_vb.clone())], &[mesh.stride, (INSTANCE_FLOATS * 4) as u32], &[0, 0]);
                self.ctx.set_index_buffer(mesh.ib.as_ref(), DXGI_FORMAT_R16_UINT, 0);
                self.ctx.draw_indexed_instanced(mesh.index_count, *count, 0, 0, first);
                self.draw_stats.draw_calls += 1;
                self.draw_stats.shadow_instances += u64::from(*count);
                self.draw_stats.shadow_triangles += u64::from(mesh.index_count / 3) * u64::from(*count);
                first += count;
            }
        }

        self.ctx.set_rasterizer_state(self.states.rs_shadow.as_ref());
    }

    pub(crate) fn pass_grass(&mut self, scene: &RenderScene) {
        wce_prof!("render.grass");
        self.collect_grass(scene);
        if self.grass.sets.is_empty() {
            return;
        }

        let (Some(vs), Some(ps)) = (self.shader(GRASS_VS), self.shader(GRASS_PS)) else { return };
        let Some(layout) = self.grass_layout(&vs) else { return };
        let frustum = Frustum::from_view_projection(&self.camera.view_proj);
        let (all, runs) = self.gather_grass(self.camera.eye, Some(&frustum));
        let Some(inst_vb) = self.upload_grass(&all) else { return };

        if self.grass.bs.is_none() {
            let blend = D3D11_RENDER_TARGET_BLEND_DESC {
                BlendEnable: true.into(), SrcBlend: D3D11_BLEND_SRC_ALPHA, DestBlend: D3D11_BLEND_INV_SRC_ALPHA, BlendOp: D3D11_BLEND_OP_ADD,
                SrcBlendAlpha: D3D11_BLEND_ZERO, DestBlendAlpha: D3D11_BLEND_INV_SRC_ALPHA, BlendOpAlpha: D3D11_BLEND_OP_ADD,
                RenderTargetWriteMask: D3D11_COLOR_WRITE_ENABLE_ALL as u8,
            };
            let opaque = D3D11_RENDER_TARGET_BLEND_DESC { RenderTargetWriteMask: D3D11_COLOR_WRITE_ENABLE_ALL as u8, ..Default::default() };
            let mut desc = D3D11_BLEND_DESC { IndependentBlendEnable: true.into(), ..Default::default() };
            desc.RenderTarget[..3].fill(blend);
            desc.RenderTarget[3..].fill(opaque);
            self.grass.bs = self.dev.create_blend_state(&desc).ok();
        }

        // grass_PS as the grass draw (FUN_118858a0) sets it: g_alpha = the grass_alpha_mul / grass_alpha_add tweakers (2.5, 0: the roots blend
        // into the ground), g_normal_scale_bias (1, 0) for a normal map without the flag, g_inv_lod_distance 1 / a distance setting (the shaders
        // hard-code 100 m); the rest as the shader object's constructor leaves them. ps_height_map_cb as the terrain pass left it (or its
        // values without one).
        let cbs = &mut self.cbs;
        cbs.set_f("grass_PS", "g_inv_lod_distance", &[1.0 / GRASS_DISTANCE]);
        cbs.set_f("grass_PS", "g_alpha", &[2.5, 0.0]);
        cbs.set_f("grass_PS", "g_normal_scale_bias", &[1.0, 0.0]);
        cbs.set_f("grass_PS", "g_far_addition", &[-1.0]);
        cbs.set_f("grass_PS", "g_far_scale", &[1.0, 1.0]);
        cbs.set_f("grass_PS", "g_clip_bias", &[0.0]);
        if !self.terrain.active {
            cbs.set_f("ps_height_map_cb", "g_mid_distance_details", &[20.0, 550.0, 3.0, 0.9]);
            cbs.set_f("ps_height_map_cb", "g_near_distance_details", &[200.0, 4.0, 0.6, 0.0]);
        }

        self.upload();

        // The terrain's detail maps.
        let material = scene.entities().iter().find_map(|e| match &e.payload {
            Payload::Terrain(t) => t.material.clone(),
            _ => None,
        });
        let srv = |t: Option<crate::model_cache::SharedTexture>| t.and_then(|t| t.read().ok().and_then(|t| t.srv.clone()));
        let tex = |path: Option<&String>| path.filter(|p| !p.is_empty()).and_then(|p| self.textures.get(p, false));
        let detail_colour = srv(tex(material.as_ref().map(|m| &m.detail_colour))).or_else(|| self.res.white_srv.clone());
        let detail_normal = srv(tex(material.as_ref().map(|m| &m.detail_normal))).or_else(|| srv(Some(self.textures.flat_normal())));
        let dissolve = srv(self.textures.get(DISSOLVE_MAP, false)).or_else(|| self.res.white_srv.clone());
        // The engine binds the global snow normals (FUN_118ce490); the shader reads them only with snow (weather_shared).
        let snow = self.snow_normals_srv();
        let depth = if self.terrain.active { self.terrain.viewz.srv.clone() } else { self.res.white_srv.clone() };

        let rtvs = self.gbuffer.channels[..4].iter().map(|t| t.rtv.clone()).collect::<Vec<_>>();
        self.ctx.set_render_targets(&rtvs, self.gbuffer.dsv.as_ref());
        self.ctx.set_depth_stencil_state(self.states.ds_gbuffer.as_ref(), 1);
        self.ctx.set_rasterizer_state(self.states.rs_none.as_ref());
        self.ctx.set_blend_state(self.grass.bs.as_ref());
        self.ctx.set_input_layout(Some(&layout));
        self.ctx.set_topology(D3D11_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
        self.bind(&vs);
        self.bind(&ps);
        null_ps_srvs(&self.ctx, 16);
        {
            let ctx = &self.ctx;
            ps_samplers(ctx, &ps, &self.states.s_wrap_aniso);
            bind_srv(ctx, &ps, "burn_sampler", self.res.black_srv.as_ref());
            bind_srv(ctx, &ps, "s_fire_map", self.res.black_srv.as_ref());
            bind_srv(ctx, &ps, "s_dissolve_map", dissolve.as_ref());
            bind_srv(ctx, &ps, "s_depth_map", depth.as_ref());
            bind_srv(ctx, &ps, "s_far_diffuse_map", self.res.black_srv.as_ref());
            bind_srv(ctx, &ps, "s_mid_distance_detail_map_colour", detail_colour.as_ref());
            bind_srv(ctx, &ps, "s_mid_distance_detail_map_normal", detail_normal.as_ref());
            bind_srv(ctx, &ps, "s_near_distance_detail_map_colour", self.res.white_srv.as_ref());
            bind_srv(ctx, &ps, "s_snow_normals", snow.as_ref());
            bind_sampler(ctx, &ps, "s_depth_map", self.states.s_clamp_point.as_ref());
            bind_sampler(ctx, &ps, "s_far_diffuse_map", self.states.s_clamp_linear.as_ref());
        }

        let mut first = 0u32;
        for (model, mi, count) in runs {
            let mesh = &model.meshes[mi];
            let diffuse = self.vegetation_diffuse(mesh);
            bind_srv(&self.ctx, &ps, "s_diffuse_map", diffuse.as_ref());
            let burn = self.grass_burn(mesh);
            bind_srv(&self.ctx, &ps, "burn_sampler", burn.as_ref());
            self.ctx.set_vertex_buffers(0, &[mesh.vb.clone(), Some(inst_vb.clone())], &[mesh.stride, (INSTANCE_FLOATS * 4) as u32], &[0, 0]);
            self.ctx.set_index_buffer(mesh.ib.as_ref(), DXGI_FORMAT_R16_UINT, 0);
            self.ctx.draw_indexed_instanced(mesh.index_count, count, 0, 0, first);
            self.draw_stats.draw_calls += 1;
            self.draw_stats.instances += u64::from(count);
            self.draw_stats.triangles += u64::from(mesh.index_count / 3) * u64::from(count);
            first += count;
        }

        null_ps_srvs(&self.ctx, 16);
        self.ctx.set_blend_state(self.states.bs_opaque.as_ref());
        self.ctx.set_rasterizer_state(self.states.rs_back.as_ref());
    }
}
