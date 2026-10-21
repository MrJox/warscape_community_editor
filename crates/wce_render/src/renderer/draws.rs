//! The scene as culling items: the draw sets of mesh entities and terrain patches, rebuilt only when the scene's structure changes, and the
//! ring buffer the visible instances of every view are written into.

use std::collections::HashMap;

use std::rc::Rc;
use std::sync::Arc;

use wce_d3d11::*;
use wce_foundation::math::{Float3, Float4x4, length};
use wce_foundation::wce_prof;
use wce_scene::render_scene::{EntityId, Payload, RenderScene, StructureChange};

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

    /// The scene's structure log as far as the sets follow it, and the first entity id they have not seen (a map context streams chunks
    /// in and out: their entities are added and removed without rebuilding the 100 000 items).
    pub structure_cursor: u64,
    pub next_entity: EntityId,

    /// The mesh entities whose models were still loading (`waiting` of them): added when their models come.
    pub waiting_ids: Vec<EntityId>,

    /// The terrain entities drawn as legacy patches (a change that adds or removes none leaves the terrain set as it is).
    pub legacy_terrain: std::collections::HashSet<EntityId>,

    /// Entity id -> item (single-item entities), NO_ITEM otherwise.
    pub item_of_entity: Vec<u32>,

    /// The view being drawn (camera, cascade, environment face), per set.
    pub vis: Visible,
    pub vis_terrain: Visible,
    pub ring: InstanceRing,

    /// The first instance of each set in the ring as the camera's G-buffer pass wrote them (the terrain pass draws the same instances).
    pub gbuffer_firsts: [u32; 2],

    /// The effects' debris (particles::EffectRigid): per model, items of `draws` added for them and moved every frame (the unused ones
    /// out of sight). Emptied with the set.
    pub rigid_pool: HashMap<String, Vec<u32>>,
}

impl Renderer {

    /// Places the placed effects' debris rigids (particles::effect_rigids, a frame behind the simulation) as items of the draw set: the
    /// pool of a model grows as needed, its items are moved each frame, the unused ones out of sight. Drawn as the scene's rigid models.
    pub(crate) fn place_effect_rigids(&mut self) {
        let u = self.camera.units;
        // The particles live relative to the render origin; the draw set is absolute.
        let o = self.camera.origin;
        let rigids = self.particles.effect_rigids((self.camera.eye - o) * u);
        if rigids.is_empty() && self.draw_lists.rigid_pool.values().all(Vec::is_empty) {
            return;
        }

        // Game units -> scene units.
        let mut to_scene = Float4x4::IDENTITY;
        for k in 0..3 {
            to_scene.m[k][k] = 1.0 / u;
        }

        let mut used = HashMap::<String, usize>::new();
        for r in rigids {
            let Some(m) = self.models.try_get(&r.model, 0.0) else { continue };
            let mut world = to_scene * r.world;
            (world.m[0][3], world.m[1][3], world.m[2][3]) = (world.m[0][3] + o.x, world.m[1][3] + o.y, world.m[2][3] + o.z);
            let n = used.entry(r.model.clone()).or_default();
            let pool = self.draw_lists.rigid_pool.entry(r.model).or_default();
            match pool.get(*n) {
                Some(&item) => self.draw_lists.draws.set_world(item, &world),
                None => pool.push(self.draw_lists.draws.add(&m, &world, None, true, true, 0)),
            }

            *n += 1;
        }

        let mut away = Float4x4 { m: [[0.0; 4]; 4] };
        (away.m[1][3], away.m[3][3]) = (-1e9, 1.0);
        for (model, pool) in &self.draw_lists.rigid_pool {
            for &item in &pool[used.get(model).copied().unwrap_or(0).min(pool.len())..] {
                self.draw_lists.draws.set_world(item, &away);
            }
        }
    }

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

                self.ctx.set_vertex_buffers(0, std::slice::from_ref(&m.vb), &[m.stride], &[0]);
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
        let (units, o) = (self.camera.units, self.camera.origin);
        let origin = [o.x * units, o.y * units, o.z * units];
        sets[0].gather(viss[0], first, units, origin);
        sets[1].gather(viss[1], second, units, origin);
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

        // A visible list names batches of the set it was culled from: a pass that culls nothing this frame (the G-buffer pass with no
        // items) must not leave the old one to the passes that read it after (the sea's model water).
        self.draw_lists.vis_terrain = Visible::default();
        self.draw_lists.terrain_bounds.clear();
        self.draw_lists.terrain_entities = 0;
        self.draw_lists.terrain_meshes = 0;
        self.draw_lists.legacy_terrain.clear();
        self.models.prune_terrain(scene);
        for e in scene.entities() {
            let Payload::Terrain(t) = &e.payload else { continue };
            if !scene.drawable(e) || !t.legacy {
                continue;
            }

            self.draw_lists.legacy_terrain.insert(e.id);
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

        if self.draw_lists.scene_id == scene_id && self.update_draws(scene, eye) {
            return;
        }

        wce_prof!("render.build_draws");
        self.draw_lists.scene_id = scene_id;
        self.draw_lists.revision = scene.structure_revision();
        self.draw_lists.model_revision = self.models.revision();

        // Everything is rebuilt from the current state: forget what was logged.
        let _ = scene.moved_since(&mut self.draw_lists.moved_cursor);
        let _ = scene.structure_since(&mut self.draw_lists.structure_cursor);
        self.draw_lists.next_entity = scene.next_id();
        self.draw_lists.draws.clear();
        self.draw_lists.rigid_pool.clear();
        self.draw_lists.vis = Visible::default();
        self.draw_lists.item_of_entity = vec![NO_ITEM; scene.entities().last().map_or(0, |e| e.id as usize + 1)];
        self.draw_lists.entities = 0;
        self.draw_lists.meshes = 0;
        self.draw_lists.waiting = 0;
        self.draw_lists.waiting_ids.clear();
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

        self.add_draws(scene, scene.entities(), eye);
        self.draw_stats.items = self.draw_lists.draws.items() + self.draw_lists.terrain_draws.items();
        self.draw_stats.batches = self.draw_lists.draws.batches() + self.draw_lists.terrain_draws.batches();
        self.draw_stats.models = self.draw_lists.draws.models() + self.draw_lists.terrain_draws.models();
        self.draw_stats.waiting_models = self.draw_lists.waiting;
    }

    /// Follows the scene's structure log since the last build: the items of removed entities go, the added entities' come (a streamed map
    /// context's chunks). False when the log asks for a rebuild (anything but removals and payloads that are no mesh instances).
    fn update_draws(&mut self, scene: &RenderScene, eye: Float3) -> bool {
        let mut cursor = self.draw_lists.structure_cursor;
        let Some(log) = scene.structure_since(&mut cursor) else { return false };
        let item = |l: &SceneDraws, id: EntityId| l.item_of_entity.get(id as usize).is_some_and(|&i| i != NO_ITEM);
        let mut gone: Vec<std::ops::Range<EntityId>> = vec![];
        for c in &log {
            match c {
                StructureChange::Removed(r) => gone.push(r.clone()),
                StructureChange::Payload(id) if !item(&self.draw_lists, *id) && !scene.find(*id).is_some_and(|e| matches!(e.payload, Payload::MeshInstance(_))) => {}
                _ => return false,
            }
        }

        wce_prof!("render.update_draws");
        self.draw_lists.structure_cursor = cursor;
        self.draw_lists.revision = scene.structure_revision();
        if !gone.is_empty() {
            gone.sort_by_key(|r| r.start);
            let inside = |id: EntityId| {
                let at = gone.partition_point(|r| r.start <= id);
                at > 0 && gone[at - 1].contains(&id)
            };

            // The removed entities' items die (each through its entity: the cost of the chunk, not of the set).
            self.draw_lists.waiting_ids.retain(|&id| !inside(id));
            for r in &gone {
                let (lo, hi) = (r.start as usize, (r.end as usize).min(self.draw_lists.item_of_entity.len()));
                for id in lo..hi.max(lo) {
                    let slot = &mut self.draw_lists.item_of_entity[id];
                    if *slot != NO_ITEM {
                        self.draw_lists.draws.kill(*slot);
                        *slot = NO_ITEM;
                    }
                }
            }

            // Packed again once a quarter of the slots is dead.
            let draws = &mut self.draw_lists.draws;
            if draws.dead() > (draws.items() / 3).max(4096) {
                wce_prof!("render.compact_draws");
                let remap = draws.compact();
                let at = |i: u32| if i == NO_ITEM { NO_ITEM } else { remap.get(i as usize).copied().unwrap_or(NO_ITEM) };
                for slot in &mut self.draw_lists.item_of_entity {
                    *slot = at(*slot);
                }

                for pool in self.draw_lists.rigid_pool.values_mut() {
                    pool.retain_mut(|i| {
                        *i = at(*i);
                        *i != NO_ITEM
                    });
                }
            }

            self.draw_lists.vis = Visible::default();
        }

        // The entities added since (their ids follow; the scene keeps them in id order).
        let all = scene.entities();
        let first = all.partition_point(|e| e.id < self.draw_lists.next_entity);
        self.draw_lists.next_entity = scene.next_id();
        let n = all.last().map_or(0, |e| e.id as usize + 1);
        if self.draw_lists.item_of_entity.len() < n {
            self.draw_lists.item_of_entity.resize(n, NO_ITEM);
        }

        // The entities whose models have come since (the others wait on), then the new ones.
        let models_revision = self.models.revision();
        if models_revision != self.draw_lists.model_revision && !self.draw_lists.waiting_ids.is_empty() {
            let ids = std::mem::take(&mut self.draw_lists.waiting_ids);
            let back: Vec<wce_scene::render_scene::Entity> = ids.iter().filter_map(|&id| scene.find(id)).cloned().collect();
            self.add_draws(scene, &back, eye);
        }

        self.draw_lists.model_revision = models_revision;
        self.add_draws(scene, &all[first..], eye);
        self.draw_lists.waiting = self.draw_lists.waiting_ids.len();
        self.draw_stats.waiting_models = self.draw_lists.waiting;
        // The legacy terrain set again only when one of its terrains came or went (a map context's are not legacy).
        let legacy_gone = self.draw_lists.legacy_terrain.iter().any(|&id| gone.iter().any(|r| r.contains(&id)));
        let legacy_new = all[first..].iter().any(|e| matches!(&e.payload, Payload::Terrain(t) if t.legacy));
        if legacy_gone || legacy_new {
            self.build_terrain_draws(scene);
        }

        // What moved meanwhile (the items added above are where their entities are already).
        if let Some(moved) = scene.moved_since(&mut self.draw_lists.moved_cursor) {
            for id in moved {
                let Some(e) = scene.find(id) else { continue };
                if let Some(&item) = self.draw_lists.item_of_entity.get(id as usize).filter(|&&item| item != NO_ITEM) {
                    self.draw_lists.draws.set_world(item, &e.world);
                }
            }
        }
        self.draw_stats.items = self.draw_lists.draws.items() + self.draw_lists.terrain_draws.items();
        self.draw_stats.batches = self.draw_lists.draws.batches() + self.draw_lists.terrain_draws.batches();
        self.draw_stats.models = self.draw_lists.draws.models() + self.draw_lists.terrain_draws.models();
        true
    }

    /// The mesh instances among `entities` as items (one cache lookup and one lock per distinct model).
    fn add_draws(&mut self, scene: &RenderScene, entities: &[wce_scene::render_scene::Entity], eye: Float3) {
        let mut resolved = HashMap::<&str, Option<Arc<GpuModel>>>::new();
        for e in entities {
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
                    self.draw_lists.waiting_ids.push(e.id);
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
    }
}
