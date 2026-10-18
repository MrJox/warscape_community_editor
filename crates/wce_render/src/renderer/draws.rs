//! The scene as culling items: the draw sets of mesh entities and terrain patches, rebuilt only when the scene's structure changes, and the
//! ring buffer the visible instances of every view are written into.

use std::collections::HashMap;

use std::rc::Rc;
use std::sync::Arc;

use wce_d3d11::*;
use wce_foundation::math::{Float3, Float4x4, length};
use wce_foundation::wce_prof;
use wce_scene::render_scene::{EntityId, Payload, RenderScene};

use super::{Renderer, dynamic_buffer};
use crate::draw_set::{DrawSet, Frustum, View as DrawView, Visible};
use crate::model_cache::{GpuMesh, GpuModel, RigidInstance, State, TEX_SLOTS, gpu_texture_name};
use crate::shader_lib::GameShader;

/// The world matrix and bounds of a terrain entity (the selection outline).
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct EntityBounds {
    pub world: Float4x4,
    pub bmin: Float3,
    pub bmax: Float3,
}

/// An item index of an entity without a single item.
pub(crate) const NO_ITEM: u32 = u32::MAX;

/// The per-view instance data: one dynamic vertex buffer used as a ring, one Map per view.
#[derive(Clone, Debug)]
pub(crate) struct InstanceRing {
    pub buffer: Option<ID3D11Buffer>,
    pub capacity: usize,
    pub cursor: usize,

    /// The next Map discards (the buffer is new).
    pub fresh: bool,
}

impl Default for InstanceRing {
    fn default() -> Self {
        Self { buffer: None, capacity: 0, cursor: 0, fresh: true }
    }
}

#[derive(Default)]
pub(crate) struct SceneDraws {
    /// The mesh entities as culling items, rebuilt when the scene changes or a model finishes loading; and the terrain patches, in a set of
    /// their own (a brush stroke rebuilds only these, a few hundred items).
    pub draws: DrawSet,
    pub terrain_draws: DrawSet,
    pub terrain_bounds: HashMap<EntityId, EntityBounds>,

    /// What the sets hold: entities and LOD 0 meshes, and the entities whose models are still loading.
    pub entities: usize,
    pub meshes: usize,
    pub waiting: usize,
    pub terrain_entities: usize,
    pub terrain_meshes: usize,

    /// What they were built from: the scene (its address), its structure, terrain and model-cache revisions, and the moved-entity log.
    pub scene_id: usize,
    pub revision: u64,
    pub terrain_revision: u64,
    pub model_revision: u64,
    pub moved_cursor: u64,

    /// Entity id -> item (single-item entities), NO_ITEM otherwise.
    pub item_of_entity: Vec<u32>,

    /// The view being drawn (camera, cascade, environment face), per set.
    pub vis: Visible,
    pub vis_terrain: Visible,
    pub ring: InstanceRing,

    /// The first instance of each set in the ring as the camera's G-buffer pass wrote them (the terrain pass draws the same instances).
    pub gbuffer_firsts: [u32; 2],
}

impl Renderer {

    /// Draws the visible batches of both sets with the bound shaders (the G-buffer and the shadow cascades share this).
    pub(crate) fn draw_batches(&mut self, sets: [&DrawSet; 2], viss: [&Visible; 2], firsts: [u32; 2], mut each: impl FnMut(&mut Self, &GpuMesh) -> bool) -> (u64, u64) {
        let (mut instances, mut triangles) = (0u64, 0u64);
        for k in 0..2 {
            let (set, vis) = (sets[k], viss[k]);
            for &b in &vis.active {
                let m = set.batch_mesh(b);
                if !each(self, m) {
                    continue;
                }

                self.ctx.set_vertex_buffers(0, std::slice::from_ref(&m.vb), &[32], &[0]);
                self.ctx.set_index_buffer(m.ib.as_ref(), DXGI_FORMAT_R16_UINT, 0);
                self.ctx.draw_indexed_instanced(m.index_count, vis.count[b as usize], 0, 0, firsts[k] + vis.start[b as usize]);

                self.draw_stats.draw_calls += 1;
                let count = u64::from(vis.count[b as usize]);
                instances += count;
                triangles += u64::from(m.index_count / 3) * count;
            }
        }

        (instances, triangles)
    }
}

impl Renderer {

    /// The texture slots of a pixel shader's material textures.
    pub(crate) fn slots_of(&mut self, ps: &Rc<GameShader>) -> [Option<u32>; TEX_SLOTS] {
        *self.ps_slots.entry(Rc::as_ptr(ps) as usize).or_insert_with(|| std::array::from_fn(|i| ps.textures.get(gpu_texture_name(i)).copied()))
    }

    /// Writes the instances of both sets into the ring with one Map (growing or wrapping the ring must not lose what the first set wrote);
    /// returns the first instance of each set.
    pub(crate) fn upload_instances(&mut self, sets: [&DrawSet; 2], viss: [&Visible; 2]) -> [u32; 2] {
        wce_prof!("render.upload_instances");
        let (n0, n1) = (viss[0].instances(), viss[1].instances());
        let n = n0 + n1;
        if n == 0 {
            return [0, 0];
        }

        if n > self.draw_lists.ring.capacity {
            self.draw_lists.ring.capacity = (n * 2).max(1 << 16);
            self.draw_lists.ring.buffer = dynamic_buffer(&self.dev, self.draw_lists.ring.capacity * size_of::<RigidInstance>(), D3D11_BIND_VERTEX_BUFFER);
            if self.draw_lists.ring.buffer.is_none() {
                self.error("cannot create the instance buffer".to_owned());
                self.draw_lists.ring.capacity = 0;
                return [0, 0];
            }

            self.draw_lists.ring.cursor = 0;
            self.draw_lists.ring.fresh = true;
        }

        let mut discard = self.draw_lists.ring.fresh;
        if self.draw_lists.ring.cursor + n > self.draw_lists.ring.capacity {
            self.draw_lists.ring.cursor = 0;
            discard = true;
        }

        let Some(ring) = self.draw_lists.ring.buffer.clone() else { return [0, 0] };
        let Ok(mut ms) = self.ctx.map(&ring, 0, if discard { D3D11_MAP_WRITE_DISCARD } else { D3D11_MAP_WRITE_NO_OVERWRITE }, 0) else { return [0, 0] };
        let size = size_of::<RigidInstance>();
        let Some(Ok(dst)) = ms.get_mut(self.draw_lists.ring.cursor * size..(self.draw_lists.ring.cursor + n) * size).map(bytemuck::try_cast_slice_mut::<u8, RigidInstance>) else { return [0, 0] };
        let (first, second) = dst.split_at_mut(n0);
        sets[0].gather(viss[0], first);
        sets[1].gather(viss[1], second);
        drop(ms);

        self.draw_lists.ring.fresh = false;
        let firsts = [self.draw_lists.ring.cursor as u32, (self.draw_lists.ring.cursor + n0) as u32];
        self.draw_lists.ring.cursor += n;
        firsts
    }

    pub(crate) fn camera_view(&self, lod_scale_factor: f32) -> DrawView {
        DrawView {
            frustum: Frustum::from_view_projection(&self.camera.view_proj),
            eye: self.camera.eye,
            min_ratio: self.settings.min_pixels * self.camera.px_ratio,
            lod_scale: self.settings.lod_scale * lod_scale_factor,
            ..Default::default()
        }
    }

    /// The terrain patches of visible terrain entities (their own draw set: a brush stroke only rebuilds this one).
    fn build_terrain_draws(&mut self, scene: &RenderScene) {
        wce_prof!("render.build_terrain_draws");
        self.draw_lists.terrain_revision = scene.terrain_revision();
        self.draw_lists.terrain_draws.clear();
        self.draw_lists.terrain_bounds.clear();
        self.draw_lists.terrain_entities = 0;
        self.draw_lists.terrain_meshes = 0;
        self.models.prune_terrain(scene);
        for e in scene.entities() {
            let Payload::Terrain(t) = &e.payload else { continue };
            if !scene.drawable(e) {
                continue;
            }

            let Some(tg) = self.models.terrain(e.id, t) else { continue };
            let mut eb = EntityBounds { world: e.world, bmin: Float3::new(1e30, 1e30, 1e30), bmax: Float3::new(-1e30, -1e30, -1e30) };
            for p in tg.patches.iter().filter(|p| !p.meshes.is_empty()) {
                self.draw_lists.terrain_draws.add(p, &e.world, None, t.casts_shadow, t.lod, e.id);
                self.draw_lists.terrain_meshes += 1;
                eb.bmin = Float3::new(eb.bmin.x.min(p.bmin.x), eb.bmin.y.min(p.bmin.y), eb.bmin.z.min(p.bmin.z));
                eb.bmax = Float3::new(eb.bmax.x.max(p.bmax.x), eb.bmax.y.max(p.bmax.y), eb.bmax.z.max(p.bmax.z));
            }

            self.draw_lists.terrain_bounds.insert(e.id, eb);
            self.draw_lists.terrain_entities += 1;
        }
    }

    /// The mesh entities of visible layers, as culling items. Models still loading (live runs) are requested nearest first and picked up
    /// when they finish; fixed-step runs wait for every model so that frame N is the same every run.
    pub(crate) fn build_draws(&mut self, scene: &RenderScene, eye: Float3) {
        let scene_id = scene as *const RenderScene as usize;
        let same = self.draw_lists.scene_id == scene_id && self.draw_lists.revision == scene.structure_revision();
        if same && (self.draw_lists.waiting == 0 || self.models.revision() == self.draw_lists.model_revision) {
            // Nothing structural changed: patch the entities that only moved (a dragged building) instead of rebuilding 100 000 items.
            let mut rebuild_terrain = self.draw_lists.terrain_revision != scene.terrain_revision();
            if let Some(moved) = scene.moved_since(&mut self.draw_lists.moved_cursor) {
                wce_prof!("render.patch_moved");
                for id in moved {
                    let Some(e) = scene.find(id) else { continue };
                    if matches!(e.payload, Payload::Terrain(_)) {
                        rebuild_terrain = true;
                        continue;
                    }

                    if let Some(&item) = self.draw_lists.item_of_entity.get(id as usize).filter(|&&item| item != NO_ITEM) {
                        self.draw_lists.draws.set_world(item, &e.world);
                    }
                }
            }

            if rebuild_terrain {
                self.build_terrain_draws(scene);
            }

            return;
        }

        wce_prof!("render.build_draws");
        self.draw_lists.scene_id = scene_id;
        self.draw_lists.revision = scene.structure_revision();
        self.draw_lists.model_revision = self.models.revision();

        // Everything is rebuilt from the current state: forget what was logged.
        let _ = scene.moved_since(&mut self.draw_lists.moved_cursor);
        self.draw_lists.draws.clear();
        self.draw_lists.item_of_entity = vec![NO_ITEM; scene.entities().last().map_or(0, |e| e.id as usize + 1)];
        self.draw_lists.entities = 0;
        self.draw_lists.meshes = 0;
        self.draw_lists.waiting = 0;
        self.build_terrain_draws(scene);

        // Request every model first: the loads run in parallel, then wait once.
        if self.settings.deterministic {
            for e in scene.entities() {
                if let Payload::MeshInstance(mi) = &e.payload
                    && scene.drawable(e)
                {
                    self.models.try_get(&mi.model, 0.0);
                }
            }

            self.models.wait_all();
        }

        self.draw_lists.draws.reserve(scene.entities().len());

        // One cache lookup (and one lock) per distinct model per build.
        let mut resolved = HashMap::<&str, Option<Arc<GpuModel>>>::new();
        for e in scene.entities() {
            let Payload::MeshInstance(mi) = &e.payload else { continue };
            if !scene.drawable(e) {
                continue;
            }

            let m = resolved.entry(&mi.model).or_insert_with(|| {
                let d = Float3::new(e.world.m[0][3] - eye.x, e.world.m[1][3] - eye.y, e.world.m[2][3] - eye.z);
                self.models.try_get(&mi.model, length(d))
            }).clone();

            let Some(m) = m else {
                if self.models.state(&mi.model) != State::Failed {
                    self.draw_lists.waiting += 1;
                }

                continue;
            };

            let params: [f32; 16] = std::array::from_fn(|i| [mi.p4, mi.p5, mi.p6, mi.p7][i / 4][i % 4]);
            let item = self.draw_lists.draws.add(&m, &e.world, Some(&params), mi.casts_shadow, mi.lod, e.id);
            if let Some(slot) = self.draw_lists.item_of_entity.get_mut(e.id as usize) {
                *slot = item;
            }

            self.draw_lists.entities += 1;
            self.draw_lists.meshes += m.meshes.len();
        }

        self.draw_stats.items = self.draw_lists.draws.items() + self.draw_lists.terrain_draws.items();
        self.draw_stats.batches = self.draw_lists.draws.batches() + self.draw_lists.terrain_draws.batches();
        self.draw_stats.models = self.draw_lists.draws.models() + self.draw_lists.terrain_draws.models();
        self.draw_stats.waiting_models = self.draw_lists.waiting;
    }
}
