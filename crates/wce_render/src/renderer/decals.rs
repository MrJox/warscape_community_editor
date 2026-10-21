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
//! The boxes are drawn where they are behind the scene (depth test greater or equal, no write), both sides (the camera may be inside one).
//!
//! Parallax (ps30_parallax): the same shader plus three steps of parallax mapping. The `_parallax.dds` maps hold the normal in .w / .y like the
//! `_normal.dds` ones and a height in .z; the uv moves by (height - 0.5) / 2 along the view direction in the box's frame, times POSITION4.w,
//! the instance's `parallax_scale`. POSITION6 (an offset and a turn about y inside the box, which only the parallax shader reads) is 0.
//! ASSUMPTION: the engine draws a decal with ps30_parallax when its model has a `_parallax.dds` map (the models are all `ProjectedDecal`, with
//! no texture list; the technique choice was not traced). Without it those decals read the height as nothing and lose their relief.

use std::collections::BTreeMap;

use wce_foundation::math::{Float3, Float4x4};
use wce_foundation::wce_prof;
use wce_scene::render_scene::{EntityId, Payload, RenderScene, StructureChange};

use crate::model_cache::State;

use super::*;
use crate::draw_set::Frustum;
use crate::shader_lib::{bind_sampler, bind_srv};

const DECAL_VS: &str = "dx11_sm_3_0_rigidprojecteddecal.hlsl_vs_vs30_main";
const DECAL_PS: &str = "dx11_sm_3_0_rigidprojecteddecal.hlsl_ps_ps30_diffuse";
const DECAL_PARALLAX_PS: &str = "dx11_sm_3_0_rigidprojecteddecal.hlsl_ps_ps30_parallax";

/// Floats of one instance: POSITION1..3 (world rows), POSITION4 (size, parallax scale), POSITION5 (tiled, opacity, snow), POSITION6 (offset
/// and turn inside the box, parallax shader only: 0).
/// A decal is drawn while its radius covers at least this many pixels.
const DECAL_MIN_PIXELS: f32 = 3.0;

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

    /// Per decal model: its textures.
    textures: BTreeMap<String, DecalTextures>,

    /// The drawable decals of the scene, sorted by model, rebuilt when the scene changes (its address and revision): a frame only culls them.
    /// Entities that moved without carrying decals (a building's collapsing fragments) don't rebuild them: the move log is followed from
    /// `moved_cursor`, between two structure revisions.
    scene_id: usize,
    revision: u64,
    structure: u64,
    moved_cursor: u64,

    /// The model cache's revision when they were collected (a building's decal boxes come with its model).
    models_revision: u64,

    /// The game units per scene unit and the render origin the instances were made with.
    units: f32,
    origin: Float3,
    models: Vec<String>,
    items: Vec<DecalItem>,

    /// Collected at all (a scene may have none); the structure log as far as they follow it and the first entity id not seen (a map
    /// context's chunks come and go without collecting 100 000 entities again); the mesh entities whose models had not loaded (their
    /// decal boxes come with the model).
    built: bool,
    structure_cursor: u64,
    next_entity: EntityId,
    pending: Vec<EntityId>,
}

/// The textures of a decal model.
#[derive(Clone)]
struct DecalTextures {
    diffuse: String,

    /// `_normal.dds`, or else `_parallax.dds` (may be missing).
    normal: Option<String>,

    /// The normal map is a `_parallax.dds` one: drawn with ps30_parallax.
    parallax: bool,
}

/// A decal as the pass draws it.
struct DecalItem {
    entity: EntityId,
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
    fn decal_textures(&mut self, model: &str) -> DecalTextures {
        if let Some(t) = self.decals.textures.get(model) {
            return t.clone();
        }

        let base = model.strip_suffix(".rigid_model_v2").unwrap_or(model);
        let normal = [format!("{base}_normal.dds"), format!("{base}_parallax.dds")].into_iter().find(|p| self.files.exists(p));
        let parallax = normal.as_deref().is_some_and(|n| n.ends_with("_parallax.dds"));
        let t = DecalTextures { diffuse: format!("{base}_diffuse.dds"), normal, parallax };
        self.decals.textures.insert(model.to_owned(), t.clone());
        t
    }

    /// The decal items of entities (decals, and the decal boxes of loaded models; the meshes whose models have not loaded go to
    /// `pending`).
    fn decal_items<'a>(&mut self, scene: &RenderScene, entities: impl Iterator<Item = &'a wce_scene::render_scene::Entity>) {
        let (units, origin) = (self.camera.units, self.camera.origin);
        let d = &mut self.decals;
        let mut index: BTreeMap<String, u32> = d.models.iter().enumerate().map(|(i, m)| (m.clone(), i as u32)).collect();
        let mut push = |d: &mut DecalGpu, entity: EntityId, name: &str, w: &Float4x4, tiled: bool, opacity: f32, parallax: f32| {
            let model = *index.entry(name.to_owned()).or_insert_with(|| {
                d.models.push(name.to_owned());
                (d.models.len() - 1) as u32
            });

            // The shaders get the box in game units; culling keeps the scene's.
            let g = rel_world(w, origin, units);
            let size = axis_sizes(&g);
            let mut inst = [0.0f32; INSTANCE_FLOATS];
            for row in 0..3 {
                inst[row * 4..row * 4 + 4].copy_from_slice(&g.m[row]);
            }

            inst[12..16].copy_from_slice(&[size[0], size[1], size[2], parallax]);
            // POSITION5 = (tiled, opacity, snow): the decal takes snow while it snows (ASSUMPTION, as the buildings).
            inst[16..19].copy_from_slice(&[if tiled { 1.0 } else { 0.0 }, opacity, 1.0]);
            let r = 0.5 * (size[0] * size[0] + size[1] * size[1] + size[2] * size[2]).sqrt() / units;
            d.items.push(DecalItem { entity, model, inst, sphere: [w.m[0][3], w.m[1][3], w.m[2][3], r] });
        };

        for e in entities {
            if !scene.drawable(e) {
                continue;
            }

            match &e.payload {
                Payload::Decal(decal) => push(d, e.id, &decal.model, &e.world, decal.tiled, decal.opacity, decal.parallax),

                // The decal meshes of a model (rubble around a ruin), once the model has loaded: its world matrix times the decal's box,
                // stretched once over it, opaque.
                Payload::MeshInstance(mi) => match self.models.peek(&mi.model) {
                    (Some(gm), _) => {
                        for decal in &gm.decals {
                            push(d, e.id, &decal.texture, &(e.world * decal.matrix), false, 1.0, 0.0);
                        }
                    }

                    (None, State::Failed) => {}
                    (None, _) => d.pending.push(e.id),
                },

                _ => {}
            }
        }
    }

    /// Follows the scene since the last collection: removed entities' decals go, added ones' come, meshes whose models loaded since bring
    /// theirs. False when it has to start over (anything structural but removals and payloads of other kinds, a decal carrier moved).
    fn update_decals(&mut self, scene: &RenderScene) -> bool {
        let d = &mut self.decals;
        let models = &self.models;
        let carries = |id: EntityId| scene.find(id).is_some_and(|e| match &e.payload {
            Payload::Decal(_) => true,
            Payload::MeshInstance(mi) => models.peek(&mi.model).0.is_some_and(|m| !m.decals.is_empty()),
            _ => false,
        });

        let mut cursor = d.structure_cursor;
        let Some(log) = scene.structure_since(&mut cursor) else { return false };
        let mut gone: Vec<std::ops::Range<EntityId>> = vec![];
        for c in &log {
            match c {
                StructureChange::Removed(r) => gone.push(r.clone()),
                StructureChange::Payload(id) if !d.items.iter().any(|i| i.entity == *id) && !carries(*id) && !d.pending.contains(id) => {}
                _ => return false,
            }
        }

        if scene.moved_since(&mut d.moved_cursor).is_none_or(|moved| moved.into_iter().any(carries)) {
            return false;
        }

        d.structure_cursor = cursor;
        if !gone.is_empty() {
            gone.sort_by_key(|r| r.start);
            let inside = |id: EntityId| {
                let at = gone.partition_point(|r| r.start <= id);
                at > 0 && gone[at - 1].contains(&id)
            };

            d.items.retain(|i| !inside(i.entity));
            d.pending.retain(|&id| !inside(id));
        }

        // The pending meshes whose models have come, then the entities added since.
        let ready: Vec<EntityId> = d.pending.iter().copied().filter(|&id| scene.find(id).is_some_and(|e| match &e.payload {
            Payload::MeshInstance(mi) => matches!(models.peek(&mi.model).1, State::Ready | State::Failed),
            _ => true,
        })).collect();

        d.pending.retain(|id| !ready.contains(id));
        let all = scene.entities();
        let first = all.partition_point(|e| e.id < d.next_entity);
        d.next_entity = scene.next_id();
        let before = self.decals.items.len();
        self.decal_items(scene, ready.iter().filter_map(|&id| scene.find(id)).chain(all[first..].iter()));
        if self.decals.items.len() != before {
            self.decals.items.sort_by_key(|i| i.model);
        }

        true
    }

    /// The scene's drawable decals, sorted by model (see [`DecalGpu::items`]).
    fn collect_decals(&mut self, scene: &RenderScene) {
        let id = std::ptr::from_ref(scene) as usize;
        let models_revision = self.models.revision();
        let units = self.camera.units;
        // A new render origin moves the instances only (their translations from the absolute positions, as rel_world makes them).
        let o = self.camera.origin;
        let d = &mut self.decals;
        if d.scene_id == id && d.units == units && d.built && d.origin != o {
            wce_prof!("render.decals_origin");
            for item in &mut d.items {
                (item.inst[3], item.inst[7], item.inst[11]) = ((item.sphere[0] - o.x) * units, (item.sphere[1] - o.y) * units, (item.sphere[2] - o.z) * units);
            }

            d.origin = o;
        }

        let d = &self.decals;
        let same = d.scene_id == id && d.units == units && d.origin == o && d.built;
        if same && d.revision == scene.revision() && d.models_revision == models_revision {
            return;
        }

        if same && self.update_decals(scene) {
            let d = &mut self.decals;
            (d.revision, d.structure, d.models_revision) = (scene.revision(), scene.structure_revision(), models_revision);
            return;
        }

        wce_prof!("render.collect_decals");
        let d = &mut self.decals;
        d.units = units;
        d.origin = self.camera.origin;
        d.scene_id = id;
        d.revision = scene.revision();
        d.structure = scene.structure_revision();
        let _ = scene.moved_since(&mut d.moved_cursor);
        let _ = scene.structure_since(&mut d.structure_cursor);
        d.next_entity = scene.next_id();
        d.models_revision = models_revision;
        d.items.clear();
        d.models.clear();
        d.pending.clear();
        d.built = true;
        self.decal_items(scene, scene.entities().iter());
        self.decals.items.sort_by_key(|i| i.model);
    }

    pub(crate) fn pass_decals(&mut self, scene: &RenderScene) {
        wce_prof!("render.decals");
        self.collect_decals(scene);

        // The visible decals, in runs of one model.
        let frustum = Frustum::from_view_projection(&self.camera.view_proj);

        // Decals smaller than a few pixels on screen are skipped (ASSUMPTION: the game's own decal distance was not traced; a map context
        // around a city has some 180,000 of them).
        let (eye, min_ratio) = (self.camera.eye, DECAL_MIN_PIXELS * self.camera.px_ratio);
        let mut all = vec![];
        let mut runs: Vec<(u32, u32)> = vec![];
        for item in &self.decals.items {
            if frustum.outside(&item.sphere[..3], item.sphere[3]) {
                continue;
            }

            let (dx, dy, dz) = (item.sphere[0] - eye.x, item.sphere[1] - eye.y, item.sphere[2] - eye.z);
            let r = item.sphere[3] / min_ratio;
            if dx * dx + dy * dy + dz * dz > r * r {
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

        let (Some(vs), Some(plain_ps)) = (self.shader(DECAL_VS), self.shader(DECAL_PS)) else { return };
        let parallax_ps = self.shader(DECAL_PARALLAX_PS);
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
        let snow = self.snow_normals_srv();

        // The two pixel shaders declare the same inputs in other slots: the fixed ones are bound again when the shader changes.
        let mut bound: Option<bool> = None;
        let mut first = 0u32;
        for (model, count) in runs {
            let model = self.decals.models[model as usize].clone();
            let tex = self.decal_textures(&model);
            let parallax = tex.parallax && parallax_ps.is_some();
            let ps = if parallax { parallax_ps.clone().unwrap_or_else(|| plain_ps.clone()) } else { plain_ps.clone() };
            if bound != Some(parallax) {
                bound = Some(parallax);
                self.bind(&ps);
                null_ps_srvs(&self.ctx, 16);
                let ctx = &self.ctx;
                bind_srv(ctx, &ps, "gbuffer_channel_4_sampler", self.gbuffer.channels[3].srv.as_ref());
                bind_srv(ctx, &ps, "s_gbuffer_normal", self.decals.normal_copy.srv.as_ref());
                bind_srv(ctx, &ps, "s_shroud", self.res.white_srv.as_ref());
                bind_srv(ctx, &ps, "s_snow_normals", snow.as_ref());
                ps_samplers(ctx, &ps, &self.states.s_wrap_aniso);
                for n in ["gbuffer_channel_4_sampler", "s_gbuffer_normal"] {
                    bind_sampler(ctx, &ps, n, self.states.s_clamp_point.as_ref());
                }

                for n in ["s_diffuse_map_clamp", "s_normal", "s_shroud"] {
                    bind_sampler(ctx, &ps, n, self.states.s_clamp_linear.as_ref());
                }
            }

            let srv = |t: Option<crate::model_cache::SharedTexture>| t.and_then(|t| t.read().ok().and_then(|t| t.srv.clone()));
            let diffuse = srv(self.textures.get(&tex.diffuse, true));
            let normal = srv(tex.normal.and_then(|n| self.textures.get(&n, false))).or_else(|| srv(Some(self.textures.flat_normal())));
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
