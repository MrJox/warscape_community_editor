//! The projected decal pass: the scene's decal entities drawn with the game's rigidprojecteddecal shaders (vs30_main, ps30_diffuse), after the
//! G-buffer and terrain passes and before the lights.
//!
//! What the shaders do (from their code): the vertex shader draws a box, the instance's 3x4 world matrix (POSITION1..3) times the unit box
//! (POSITION0, -0.5 .. 0.5). The pixel shader rebuilds each pixel's world position from the depth channel (gbuffer_channel_4, z / w, through
//! inv_view_projection), maps it into the box (the matrix rows normalised, scaled by POSITION4 / POSITION4^2: the instance's size), discards
//! it outside, and projects the decal's textures down the box's y axis: uv = (x + 0.5, 0.5 - z). POSITION5 = (tiled, opacity, snow): tiled
//! decals repeat at local * size * g_texture_ratio * 0.1587 with the wrap samplers. It writes the G-buffer normal (channel 1, the decal's normal
//! map in the box's frame), sqrt(albedo) (channel 2) and the snow parameters (channel 3), each with alpha = diffuse alpha * opacity * how much
//! the surface faces the box's y axis (from the G-buffer normal, which it reads: a copy, since channel 1 is written). The blend is the source
//! alpha over the G-buffer's colour channels; the alpha channels (gloss, mask, material id) are kept.
//!
//! The boxes are drawn where they are behind the scene (depth test greater or equal, no write), both sides (the camera may be inside one). The parallax variant (ps30_parallax, the `_parallax.dds` height in its normal map's alpha) is not used: its normal map is
//! read as a plain one.

use std::collections::BTreeMap;

use wce_foundation::math::Float4x4;
use wce_foundation::wce_prof;
use wce_scene::render_scene::{Payload, RenderScene};

use super::*;
use crate::draw_set::Frustum;
use crate::shader_lib::{bind_sampler, bind_srv};

const DECAL_VS: &str = "dx11_sm_3_0_rigidprojecteddecal.hlsl_vs_vs30_main";
const DECAL_PS: &str = "dx11_sm_3_0_rigidprojecteddecal.hlsl_ps_ps30_diffuse";

/// Floats of one instance: POSITION1..3 (world rows), POSITION4 (size), POSITION5 (tiled, opacity, snow), POSITION6 (unused by the shaders).
const INSTANCE_FLOATS: usize = 24;

/// The decal pass's state.
#[derive(Default)]
pub(crate) struct DecalGpu {
    box_vb: Option<ID3D11Buffer>,
    box_ib: Option<ID3D11Buffer>,
    inst_vb: Option<ID3D11Buffer>,
    inst_capacity: usize,
    rs: Option<ID3D11RasterizerState>,
    ds: Option<ID3D11DepthStencilState>,
    bs: Option<ID3D11BlendState>,

    /// The G-buffer normal as it was before the decals (the shader reads it while channel 1 is written).
    normal_copy: Target,
    copy_w: u32,
    copy_h: u32,

    /// Per decal model: its diffuse and normal map paths (the normal map may be missing).
    textures: BTreeMap<String, (String, Option<String>)>,

    /// The drawable decals of the scene, sorted by model, rebuilt when the scene changes (its address and revision): a frame only culls them.
    scene_id: usize,
    revision: u64,
    models: Vec<String>,
    items: Vec<DecalItem>,
}

/// A decal as the pass draws it.
struct DecalItem {
    model: u32,
    inst: [f32; INSTANCE_FLOATS],

    /// Bounding sphere: centre and radius.
    sphere: [f32; 4],
}

/// The size of each of a world matrix's axes (the length of its columns).
fn axis_sizes(m: &Float4x4) -> [f32; 3] {
    std::array::from_fn(|c| (m.m[0][c] * m.m[0][c] + m.m[1][c] * m.m[1][c] + m.m[2][c] * m.m[2][c]).sqrt())
}

impl Renderer {

    fn make_decal_resources(&mut self) {
        if self.decals.box_vb.is_some() {
            return;
        }

        let dev = self.dev.clone();
        let d = &mut self.decals;
        let mut verts = vec![];
        for k in 0..8 {
            verts.extend([if k & 1 != 0 { 0.5f32 } else { -0.5 }, if k & 2 != 0 { 0.5 } else { -0.5 }, if k & 4 != 0 { 0.5 } else { -0.5 }]);
        }

        // Two triangles per face, both sides drawn: a far face passes the depth test where the scene is inside or in front of the box, a near
        // face only where the scene is in front of the box, which the shader discards (outside the box).
        let idx: [u16; 36] = [0, 2, 1, 1, 2, 3, 4, 5, 6, 5, 7, 6, 0, 1, 4, 1, 5, 4, 2, 6, 3, 3, 6, 7, 0, 4, 2, 2, 4, 6, 1, 3, 5, 3, 7, 5];
        d.box_vb = immutable_buffer(&dev, &f32_bytes(&verts), D3D11_BIND_VERTEX_BUFFER);
        d.box_ib = immutable_buffer(&dev, &idx.iter().flat_map(|i| i.to_le_bytes()).collect::<Vec<_>>(), D3D11_BIND_INDEX_BUFFER);
        let rd = D3D11_RASTERIZER_DESC { FillMode: D3D11_FILL_SOLID, CullMode: D3D11_CULL_NONE, DepthClipEnable: false.into(), ..Default::default() };
        d.rs = dev.create_rasterizer_state(&rd).ok();
        d.ds = dev.create_depth_stencil_state(&D3D11_DEPTH_STENCIL_DESC {
            DepthEnable: true.into(), DepthWriteMask: D3D11_DEPTH_WRITE_MASK_ZERO, DepthFunc: D3D11_COMPARISON_GREATER_EQUAL, ..Default::default()
        }).ok();

        let rgb = (D3D11_COLOR_WRITE_ENABLE_RED | D3D11_COLOR_WRITE_ENABLE_GREEN | D3D11_COLOR_WRITE_ENABLE_BLUE) as u8;
        d.bs = crate::particles::blend_state(&dev, D3D11_RENDER_TARGET_BLEND_DESC {
            BlendEnable: true.into(), SrcBlend: D3D11_BLEND_SRC_ALPHA, DestBlend: D3D11_BLEND_INV_SRC_ALPHA, BlendOp: D3D11_BLEND_OP_ADD,
            SrcBlendAlpha: D3D11_BLEND_ZERO, DestBlendAlpha: D3D11_BLEND_ONE, BlendOpAlpha: D3D11_BLEND_OP_ADD, RenderTargetWriteMask: rgb,
        });
    }

    /// The diffuse and normal map of a decal model (`<model>_diffuse.dds`, `_normal.dds` or else `_parallax.dds`).
    fn decal_textures(&mut self, model: &str) -> (String, Option<String>) {
        if let Some(t) = self.decals.textures.get(model) {
            return t.clone();
        }

        let base = model.strip_suffix(".rigid_model_v2").unwrap_or(model);
        let normal = [format!("{base}_normal.dds"), format!("{base}_parallax.dds")].into_iter().find(|p| self.files.exists(p));
        let t = (format!("{base}_diffuse.dds"), normal);
        self.decals.textures.insert(model.to_owned(), t.clone());
        t
    }

    /// The scene's drawable decals, sorted by model (see [`DecalGpu::items`]).
    fn collect_decals(&mut self, scene: &RenderScene) {
        let id = std::ptr::from_ref(scene) as usize;
        let d = &mut self.decals;
        if d.scene_id == id && d.revision == scene.revision() && !d.items.is_empty() {
            return;
        }

        d.scene_id = id;
        d.revision = scene.revision();
        d.items.clear();
        let mut index = BTreeMap::<&str, u32>::new();
        let mut models = vec![];
        for e in scene.entities() {
            let Payload::Decal(decal) = &e.payload else { continue };
            if !scene.drawable(e) {
                continue;
            }

            let model = *index.entry(decal.model.as_str()).or_insert_with(|| {
                models.push(decal.model.clone());
                (models.len() - 1) as u32
            });

            let w = &e.world;
            let size = axis_sizes(w);
            let mut inst = [0.0f32; INSTANCE_FLOATS];
            for row in 0..3 {
                inst[row * 4..row * 4 + 4].copy_from_slice(&w.m[row]);
            }

            inst[12..15].copy_from_slice(&size);
            inst[16..19].copy_from_slice(&[if decal.tiled { 1.0 } else { 0.0 }, decal.opacity, 0.0]);
            let r = 0.5 * (size[0] * size[0] + size[1] * size[1] + size[2] * size[2]).sqrt();
            d.items.push(DecalItem { model, inst, sphere: [w.m[0][3], w.m[1][3], w.m[2][3], r] });
        }

        d.items.sort_by_key(|i| i.model);
        d.models = models;
    }

    pub(crate) fn pass_decals(&mut self, scene: &RenderScene) {
        wce_prof!("render.decals");
        self.collect_decals(scene);

        // The visible decals, in runs of one model.
        let frustum = Frustum::from_view_projection(&self.camera.view_proj);
        let mut all = vec![];
        let mut runs: Vec<(u32, u32)> = vec![];
        for item in &self.decals.items {
            if frustum.outside(&item.sphere[..3], item.sphere[3]) {
                continue;
            }

            all.extend_from_slice(&item.inst);
            match runs.last_mut() {
                Some(run) if run.0 == item.model => run.1 += 1,
                _ => runs.push((item.model, 1)),
            }
        }

        let total = all.len() / INSTANCE_FLOATS;
        if total == 0 {
            return;
        }

        let (Some(vs), Some(ps)) = (self.shader(DECAL_VS), self.shader(DECAL_PS)) else { return };
        self.make_decal_resources();
        let el: Vec<InputElement> = std::iter::once(element(c"POSITION", 0, DXGI_FORMAT_R32G32B32_FLOAT, 0, 0, false))
            .chain((1..=6).map(|i| element(c"POSITION", i, DXGI_FORMAT_R32G32B32A32_FLOAT, 1, (i - 1) * 16, true)))
            .collect();
        let Some(layout) = self.layout(&vs, "projected_decal", &el) else { return };

        // One buffer for every visible decal, the models' runs one after the other.
        if total > self.decals.inst_capacity {
            self.decals.inst_capacity = (total * 2).max(4096);
            self.decals.inst_vb = dynamic_buffer(&self.dev, self.decals.inst_capacity * INSTANCE_FLOATS * 4, D3D11_BIND_VERTEX_BUFFER);
        }

        let Some(inst_vb) = self.decals.inst_vb.clone() else { return };
        if !self.ctx.map(&inst_vb, 0, D3D11_MAP_WRITE_DISCARD, 0).is_ok_and(|mut ms| ms.write(0, &all)) {
            return;
        }

        // The G-buffer normal before the decals.
        let (w, h) = (self.gbuffer.w, self.gbuffer.h);
        if self.decals.normal_copy.tex.is_none() || self.decals.copy_w != w || self.decals.copy_h != h {
            self.decals.normal_copy = make_target(&self.dev, w, h, DXGI_FORMAT_R8G8B8A8_UNORM);
            (self.decals.copy_w, self.decals.copy_h) = (w, h);
        }

        if let (Some(dst), Some(src)) = (&self.decals.normal_copy.tex, &self.gbuffer.channels[0].tex) {
            self.ctx.copy_resource(dst, src);
        }

        let cbs = &mut self.cbs;
        cbs.set_f("projected_decal_PS", "g_viewport_offset_scale", &[0.0, 0.0, 1.0, 1.0]);
        cbs.set_f("projected_decal_PS", "g_texture_ratio", &[1.0, 1.0]);
        cbs.set_f("projected_decal_PS", "g_is_campaign", &[0.0]);
        cbs.set_f("shroud_PS", "g_shroud_enabled", &[0.0]);
        self.upload();

        let rtvs = self.gbuffer.channels[..3].iter().map(|t| t.rtv.clone()).collect::<Vec<_>>();
        self.ctx.set_render_targets(&rtvs, self.gbuffer.dsv.as_ref());
        self.ctx.set_depth_stencil_state(self.decals.ds.as_ref(), 0);
        self.ctx.set_rasterizer_state(self.decals.rs.as_ref());
        self.ctx.set_blend_state(self.decals.bs.as_ref());
        self.ctx.set_input_layout(Some(&layout));
        self.ctx.set_topology(D3D11_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
        self.ctx.set_vertex_buffers(0, &[self.decals.box_vb.clone(), Some(inst_vb)], &[12, (INSTANCE_FLOATS * 4) as u32], &[0, 0]);
        self.ctx.set_index_buffer(self.decals.box_ib.as_ref(), DXGI_FORMAT_R16_UINT, 0);
        self.bind(&vs);
        self.bind(&ps);
        null_ps_srvs(&self.ctx, 16);
        {
            let ctx = &self.ctx;
            bind_srv(ctx, &ps, "gbuffer_channel_4_sampler", self.gbuffer.channels[3].srv.as_ref());
            bind_srv(ctx, &ps, "s_gbuffer_normal", self.decals.normal_copy.srv.as_ref());
            bind_srv(ctx, &ps, "s_shroud", self.res.white_srv.as_ref());
            bind_srv(ctx, &ps, "s_snow_normals", self.res.black_srv.as_ref());
            ps_samplers(ctx, &ps, &self.states.s_wrap_aniso);
            for n in ["gbuffer_channel_4_sampler", "s_gbuffer_normal"] {
                bind_sampler(ctx, &ps, n, self.states.s_clamp_point.as_ref());
            }

            for n in ["s_diffuse_map_clamp", "s_normal", "s_shroud"] {
                bind_sampler(ctx, &ps, n, self.states.s_clamp_linear.as_ref());
            }
        }

        let mut first = 0u32;
        for (model, count) in runs {
            let model = self.decals.models[model as usize].clone();
            let (diffuse, normal) = self.decal_textures(&model);
            let srv = |t: Option<crate::model_cache::SharedTexture>| t.and_then(|t| t.read().ok().and_then(|t| t.srv.clone()));
            let diffuse = srv(self.textures.get(&diffuse, true));
            let normal = srv(normal.and_then(|n| self.textures.get(&n, false))).or_else(|| srv(Some(self.textures.flat_normal())));
            if diffuse.is_some() {
                let ctx = &self.ctx;
                bind_srv(ctx, &ps, "s_diffuse_map_clamp", diffuse.as_ref());
                bind_srv(ctx, &ps, "s_diffuse_map_wrap", diffuse.as_ref());
                bind_srv(ctx, &ps, "s_normal", normal.as_ref());
                bind_srv(ctx, &ps, "s_normal_wrap", normal.as_ref());
                ctx.draw_indexed_instanced(36, count, 0, 0, first);
                self.draw_stats.draw_calls += 1;
            }

            first += count;
        }

        null_ps_srvs(&self.ctx, 16);
        self.ctx.set_blend_state(self.states.bs_opaque.as_ref());
        self.ctx.set_rasterizer_state(self.states.rs_back.as_ref());
    }
}
