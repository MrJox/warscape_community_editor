//! What the renderer draws, laid out for the CPU cache and for the GPU's instancing: every mesh or terrain-patch entity is one *item* in
//! parallel arrays (bounding sphere, world matrix, model, flags), and each distinct mesh at each LOD is a *batch*. A view (the camera, a
//! shadow cascade, an environment cube face) culls the items in parallel (frustum, screen size, LOD by distance) and sorts the survivors
//! by batch; the renderer then writes one instance buffer per view and issues one instanced draw per visible batch.

use std::collections::HashMap;
use std::sync::Arc;

use rayon::prelude::*;
use wce_foundation::math::{Float3, Float4x4, length};
use wce_foundation::wce_prof;
use wce_scene::render_scene::{DEFAULT_INSTANCE_PARAMS, EntityId};

use crate::model_cache::{GpuMesh, GpuModel, MeshKind, RigidInstance};

/// A plane; inside is dot(n, p) + d >= 0.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Plane {
    pub n: [f32; 3],
    pub d: f32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Frustum {
    pub p: [Plane; 6],
}

impl Frustum {

    /// The planes of a D3D clip volume (x, y in -w..w, z in 0..w) from a view-projection matrix in this code base's convention
    /// (clip = M * p).
    pub fn from_view_projection(vp: &Float4x4) -> Self {
        // s * rows[k] + rows[r], or just rows[r] without k.
        let row = |r: usize, s: f32, k: Option<usize>| {
            let v: [f32; 4] = std::array::from_fn(|c| vp.m[r][c] + k.map_or(0.0, |k| s * vp.m[k][c]));
            Plane { n: [v[0], v[1], v[2]], d: v[3] }
        };

        let mut f = Self {
            p: [
                row(3, 1.0, Some(0)),   // left: w + x >= 0
                row(3, -1.0, Some(0)),  // right: w - x >= 0
                row(3, 1.0, Some(1)),   // bottom: w + y >= 0
                row(3, -1.0, Some(1)),  // top: w - y >= 0
                row(2, 0.0, None),      // near: z >= 0 (D3D)
                row(3, -1.0, Some(2)),  // far: w - z >= 0
            ],
        };

        for pl in &mut f.p {
            let l = (pl.n[0] * pl.n[0] + pl.n[1] * pl.n[1] + pl.n[2] * pl.n[2]).sqrt();
            if l > 0.0 {
                pl.n[0] /= l;
                pl.n[1] /= l;
                pl.n[2] /= l;
                pl.d /= l;
            }
        }

        f
    }

    /// If a sphere is completely outside.
    pub fn outside(&self, c: &[f32], r: f32) -> bool {
        self.p.iter().any(|pl| pl.n[0] * c[0] + pl.n[1] * c[1] + pl.n[2] * c[2] + pl.d < -r)
    }
}

/// The first three rows of a world matrix.
pub type Mat34 = [[f32; 4]; 3];

const CASTS_SHADOW: u8 = 1;
const LOD: u8 = 2;
const PARAMS: u8 = 4;

/// An item whose entity went: culling skips it, `add` fills its slot again.
const DEAD: u8 = 8;

/// Cull chunk size.
const GRAIN: usize = 2048;

struct ModelEntry {
    centre: Float3,
    bmin: Float3,
    bmax: Float3,
    radius: f32,

    /// Per LOD; +inf for the last.
    lod_distance: Vec<f32>,

    /// lod_first[l]..lod_first[l + 1] index lod_batch.
    lod_first: Vec<u32>,

    /// Batch ids.
    lod_batch: Vec<u32>,

    /// The engine's model type (`GpuModel::material`) for the quality options' LOD switching.
    material: u16,
}

/// A batch: a mesh of a model at one LOD (0 is the model's own meshes).
#[derive(Clone, Debug)]
struct Batch {
    model: Arc<GpuModel>,
    lod: usize,
    mesh: usize,

    /// Its instances are burning: the G-buffer pass draws them with their material's fire shader.
    burning: bool,

    /// Its instances are faded (opacity below 1): drawn by the blended forward pass, not the G-buffer's.
    faded: bool,
}

/// One view's request.
#[derive(Clone, Copy, Debug)]
pub struct View {
    pub frustum: Frustum,
    pub eye: Float3,

    /// Only items that cast shadows.
    pub shadow: bool,

    /// Multiplies the LOD distances (below 1: coarser sooner).
    pub lod_scale: f32,

    /// Added to the LOD index (shadow maps need less detail).
    pub coarser: i32,

    /// LOD items with radius / distance below this are skipped (too small to see).
    pub min_ratio: f32,

    /// LOD items with a smaller radius are skipped (shadow texels).
    pub min_radius: f32,

    /// Items farther than this (their sphere's nearest point) are skipped; 0: no limit.
    pub max_distance: f32,

    /// The quality options' levels that switch LODs off.
    pub quality: LodQuality,
}

/// The quality options' levels that switch a model's first LODs off in battle (docs/re/graphics_options.md: Attila's fn 0x1180c370 and
/// fn 0x11841280, Thrones of Britannia's the same): gfx_building_quality, gfx_unit_quality and gfx_tree_quality. `None`: no LOD is off
/// (the highest levels, and Rome 2, whose pass is not found).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LodQuality(pub Option<[i32; 3]>);

impl LodQuality {
    /// The LODs switched off for a model of this type with `n` LODs: a range of LOD indices (the last LOD always stays).
    pub fn off(&self, material: u16, n: usize) -> std::ops::Range<usize> {
        let Some([building, unit, tree]) = self.0 else { return 0..0 };
        let last = n.saturating_sub(1);

        // The level a model's type takes (fn 0x11841280, battle) and the pass's numbers for it: (first kept when spared, end of the run,
        // start of the run) as uVar6, uVar5, uVar3 of fn 0x1180c370.
        let (spare, end, start) = match material {
            0x47 | 0x48 => (0, match building { 0 => 2, 1 => 1, _ => 0 }, 0),
            0x49 | 0x4a | 0x4b | 0x6c => (0, usize::from(unit == 0), 0),
            0x5b | 0x5c | 0x6e | 0x6f => (0, usize::from(tree == 0), 0),
            0x6b if building == 0 => (0, 1, 0),
            0x6b => (1, 2, 1),
            _ => return 0..0,
        };

        let first = if spare < last { start } else { last };
        first..end.min(last).max(first)
    }
}

impl Default for View {

    /// As the C++ default: LOD distances unscaled.
    fn default() -> Self {
        Self {
            frustum: Frustum::default(), eye: Float3::default(), shadow: false, lod_scale: 1.0, coarser: 0, min_ratio: 0.0, min_radius: 0.0, max_distance: 0.0,
            quality: LodQuality::default(),
        }
    }
}

/// What a view sees, grouped by batch.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Visible {
    /// Per batch: the first instance and the number of instances.
    pub start: Vec<u32>,
    pub count: Vec<u32>,

    /// The batches with instances, ascending.
    pub active: Vec<u32>,

    /// The item of every instance, grouped by batch.
    pub order: Vec<u32>,

    /// The batch of every instance.
    pub batch_of: Vec<u32>,
}

impl Visible {
    pub fn instances(&self) -> usize {
        self.order.len()
    }
}

/// The items and batches of a scene.
#[derive(Default)]
pub struct DrawSet {
    // Items, as a structure of arrays.
    sphere: Vec<f32>,
    model: Vec<u32>,
    flags: Vec<u8>,
    world: Vec<Mat34>,
    params_index: Vec<u32>,
    params: Vec<f32>,
    entity: Vec<EntityId>,

    /// The dead items' slots.
    free: Vec<u32>,

    // Models and batches.
    models: Vec<ModelEntry>,
    /// (Model address, burning, faded) -> model index; the batches keep the models alive, so an address stays unique. The burning and the
    /// faded instances of a model have batches of their own.
    model_of: HashMap<(usize, bool, bool), u32>,
    batch: Vec<Batch>,
}

/// Whether instance parameters (POSITION4..7) fade the instance: POSITION4.w is round(opacity * 1000) plus a fraction, as the engine
/// packs it; below 1000 the engine files the piece for the blended forward shaders (render flag 0x2000, docs/re/buildings.md).
pub fn faded(params: &[f32; 16]) -> bool {
    params[3].floor() < 1000.0
}

/// The largest squared length of the rotation-scale columns of a world matrix.
fn max_scale2(world: &Float4x4) -> f32 {
    (0..3).fold(0.0f32, |s2, col| s2.max(world.m[0][col] * world.m[0][col] + world.m[1][col] * world.m[1][col] + world.m[2][col] * world.m[2][col]))
}

impl DrawSet {

    pub fn clear(&mut self) {
        *self = Self::default();
    }

    pub fn reserve(&mut self, n: usize) {
        self.sphere.reserve(n * 4);
        self.model.reserve(n);
        self.flags.reserve(n);
        self.world.reserve(n);
        self.params_index.reserve(n);
        self.entity.reserve(n);
    }

    /// The live items.
    pub fn items(&self) -> usize {
        self.model.len() - self.free.len()
    }

    /// The items' slots, the dead ones too.
    fn slots(&self) -> usize {
        self.model.len()
    }

    /// The dead items (their slots wait for new ones; `compact` drops them).
    pub fn dead(&self) -> usize {
        self.free.len()
    }

    /// Kills an item (its entity went): a removal costs its own items, not a pass over the set (a map context streams chunks of 10 000 in
    /// and out of a set of 150 000).
    pub fn kill(&mut self, item: u32) {
        let i = item as usize;
        if i < self.flags.len() && self.flags[i] & DEAD == 0 {
            self.flags[i] = DEAD;
            self.entity[i] = 0;
            self.free.push(item);
        }
    }

    /// Drops the dead items; returns each old item's new index (`u32::MAX` for the dropped ones).
    pub fn compact(&mut self) -> Vec<u32> {
        self.remove_items(|_| false)
    }

    pub fn batches(&self) -> usize {
        self.batch.len()
    }

    pub fn models(&self) -> usize {
        self.models.len()
    }

    pub fn entity(&self, item: u32) -> EntityId {
        self.entity[item as usize]
    }

    pub fn batch_mesh(&self, batch: u32) -> &GpuMesh {
        let b = &self.batch[batch as usize];
        if b.lod == 0 { &b.model.meshes[b.mesh] } else { &b.model.lods[b.lod - 1].meshes[b.mesh] }
    }

    /// Whether a batch's instances are burning.
    pub fn batch_burning(&self, batch: u32) -> bool {
        self.batch[batch as usize].burning
    }

    /// Whether a batch's instances are faded (opacity below 1, [`faded`]).
    pub fn batch_faded(&self, batch: u32) -> bool {
        self.batch[batch as usize].faded
    }

    fn model_index(&mut self, m: &Arc<GpuModel>, burning: bool, faded: bool) -> u32 {
        if let Some(&index) = self.model_of.get(&(Arc::as_ptr(m) as usize, burning, faded)) {
            return index;
        }

        let mut me = ModelEntry {
            centre: (m.bmin + m.bmax) * 0.5,
            bmin: m.bmin,
            bmax: m.bmax,
            radius: length(m.bmax - m.bmin) * 0.5,
            lod_distance: vec![],
            lod_first: vec![],
            lod_batch: vec![],
            material: m.material,
        };

        let lods = std::iter::once((&m.meshes, m.distance)).chain(m.lods.iter().map(|l| (&l.meshes, l.distance)));
        for (lod, (meshes, distance)) in lods.enumerate() {
            me.lod_first.push(me.lod_batch.len() as u32);
            for mesh in 0..meshes.len() {
                me.lod_batch.push(self.batch.len() as u32);
                self.batch.push(Batch { model: m.clone(), lod, mesh, burning, faded });
            }

            me.lod_distance.push(if distance > 0.0 { distance } else { f32::INFINITY });
        }

        me.lod_first.push(me.lod_batch.len() as u32);

        // The last LOD is used from there on.
        if let Some(last) = me.lod_distance.last_mut() {
            *last = f32::INFINITY;
        }

        let index = self.models.len() as u32;
        self.models.push(me);
        self.model_of.insert((Arc::as_ptr(m) as usize, burning, faded), index);
        index
    }

    /// Adds an item; `params` are POSITION4..7, `None` for the engine's defaults. Returns the item's index.
    pub fn add(&mut self, model: &Arc<GpuModel>, world: &Float4x4, params: Option<&[f32; 16]>, casts_shadow: bool, lod: bool, entity: EntityId) -> u32 {
        self.add_burning(model, world, params, casts_shadow, lod, false, entity)
    }

    /// `add`, burning or not (the fire shaders).
    #[allow(clippy::too_many_arguments)]
    pub fn add_burning(&mut self, model: &Arc<GpuModel>, world: &Float4x4, params: Option<&[f32; 16]>, casts_shadow: bool, lod: bool, burning: bool, entity: EntityId) -> u32 {
        let mi = self.model_index(model, burning, params.is_some_and(faded));
        let me = &self.models[mi as usize];
        let c = world.transform_point(me.centre);
        let sphere = [c.x, c.y, c.z, me.radius * max_scale2(world).sqrt()];
        let mut f = if casts_shadow { CASTS_SHADOW } else { 0 } | if lod { LOD } else { 0 };
        let params_index = match params.filter(|p| **p != DEFAULT_INSTANCE_PARAMS) {
            Some(p) => {
                f |= PARAMS;
                self.params.extend_from_slice(p);
                (self.params.len() / 16 - 1) as u32
            }

            None => 0,
        };

        // A dead item's slot, else a new one.
        let w = [world.m[0], world.m[1], world.m[2]];
        if let Some(item) = self.free.pop() {
            let i = item as usize;
            self.sphere[i * 4..i * 4 + 4].copy_from_slice(&sphere);
            (self.model[i], self.flags[i], self.world[i], self.params_index[i], self.entity[i]) = (mi, f, w, params_index, entity);
            return item;
        }

        let item = self.slots() as u32;
        self.sphere.extend(sphere);
        self.model.push(mi);
        self.world.push(w);
        self.params_index.push(params_index);
        self.flags.push(f);
        self.entity.push(entity);
        item
    }

    /// Drops the items of the entities `gone` names (the others keep their order); returns each old item's new index, `u32::MAX` for the
    /// dropped ones. The models and batches stay.
    pub fn remove_items(&mut self, gone: impl Fn(EntityId) -> bool) -> Vec<u32> {
        let n = self.slots();
        let mut remap = vec![u32::MAX; n];
        let mut to = 0usize;
        for (from, slot) in remap.iter_mut().enumerate() {
            if self.flags[from] & DEAD != 0 || gone(self.entity[from]) {
                continue;
            }

            if to != from {
                self.sphere.copy_within(from * 4..from * 4 + 4, to * 4);
                self.model[to] = self.model[from];
                self.flags[to] = self.flags[from];
                self.world[to] = self.world[from];
                self.params_index[to] = self.params_index[from];
                self.entity[to] = self.entity[from];
            }

            *slot = to as u32;
            to += 1;
        }

        self.sphere.truncate(to * 4);
        self.model.truncate(to);
        self.flags.truncate(to);
        self.world.truncate(to);
        self.params_index.truncate(to);
        self.entity.truncate(to);

        // The parameters of the items that stay, packed again.
        let mut params = Vec::with_capacity(self.params.len());
        for i in 0..to {
            if self.flags[i] & PARAMS != 0 {
                let at = self.params_index[i] as usize * 16;
                self.params_index[i] = (params.len() / 16) as u32;
                params.extend_from_slice(&self.params[at..at + 16]);
            }
        }

        self.params = params;
        self.free.clear();
        remap
    }

    pub fn set_world(&mut self, item: u32, world: &Float4x4) {
        let me = &self.models[self.model[item as usize] as usize];
        let c = world.transform_point(me.centre);
        let at = item as usize * 4;
        self.sphere[at..at + 4].copy_from_slice(&[c.x, c.y, c.z, me.radius * max_scale2(world).sqrt()]);
        self.world[item as usize] = [world.m[0], world.m[1], world.m[2]];
    }

    /// The world matrix and model-space bounds of an item (the selection outline).
    pub fn item_box(&self, item: u32) -> Option<(Float4x4, Float3, Float3)> {
        let w = self.world.get(item as usize)?;
        let me = &self.models[self.model[item as usize] as usize];
        let mut world = Float4x4::IDENTITY;
        world.m[..3].copy_from_slice(w);
        Some((world, me.bmin, me.bmax))
    }

    /// The LOD an item uses seen from an eye (tests, statistics).
    pub fn lod_of(&self, item: u32, eye: Float3, lod_scale: f32) -> usize {
        let me = &self.models[self.model[item as usize] as usize];
        let s = &self.sphere[item as usize * 4..item as usize * 4 + 4];
        let (dx, dy, dz) = (s[0] - eye.x, s[1] - eye.y, s[2] - eye.z);
        let item_scale = if me.radius > 0.0 { s[3] / me.radius } else { 1.0 };
        let dist = (dx * dx + dy * dy + dz * dz).sqrt() / lod_scale / item_scale.max(1e-3);
        let last = me.lod_distance.len() - 1;
        let mut lod = 0;
        while lod < last && dist > me.lod_distance[lod] {
            lod += 1;
        }

        lod
    }

    /// Culls the items for a view and groups the survivors by batch. The same culled set always gives the same draw order.
    pub fn cull(&self, v: &View, out: &mut Visible) {
        self.cull_split(v, out, None);
    }

    /// As [`DrawSet::cull`], with the leaves (TreeLeaf batches) of the items whose centre is within a range of the eye (squared distances
    /// in the view's units, from `.1.0` to `.1.1`) grouped into `near.0` instead: the order-independent transparency's (gfx_alpha_blend).
    pub fn cull_split(&self, v: &View, out: &mut Visible, mut near: Option<(&mut Visible, (f32, f32))>) {
        wce_prof!("drawset.cull");
        let range = near.as_ref().map(|n| n.1);
        let (n, nb) = (self.slots(), self.batches());
        let inv_scale = 1.0 / v.lod_scale.max(1e-3);
        let chunk_pairs = (0..n.div_ceil(GRAIN)).into_par_iter().map(|chunk| {
            let mut pairs = vec![];
            for i in chunk * GRAIN..((chunk + 1) * GRAIN).min(n) {
                let f = self.flags[i];
                if f & DEAD != 0 || (v.shadow && f & CASTS_SHADOW == 0) {
                    continue;
                }

                let s = &self.sphere[i * 4..i * 4 + 4];
                if v.frustum.outside(s, s[3]) {
                    continue;
                }

                if v.max_distance > 0.0 {
                    let (dx, dy, dz) = (s[0] - v.eye.x, s[1] - v.eye.y, s[2] - v.eye.z);
                    let reach = v.max_distance + s[3];
                    if dx * dx + dy * dy + dz * dz > reach * reach {
                        continue;
                    }
                }

                let me = &self.models[self.model[i] as usize];
                let mut lod = 0usize;
                if f & LOD != 0 {
                    let (dx, dy, dz) = (s[0] - v.eye.x, s[1] - v.eye.y, s[2] - v.eye.z);
                    let dist = (dx * dx + dy * dy + dz * dz).sqrt();
                    if s[3] < dist * v.min_ratio || s[3] < v.min_radius {
                        continue;
                    }

                    // LOD distances are the model's own units: a model drawn at another scale (tiles: 1 / unit) switches at the scaled
                    // distances.
                    let item_scale = if me.radius > 0.0 { s[3] / me.radius } else { 1.0 };
                    let d = dist * inv_scale / item_scale.max(1e-3);
                    let last = me.lod_distance.len() - 1;

                    // A LOD the quality options switched off is passed over (its distance 0, not enabled: the engine's selection).
                    let off = v.quality.off(me.material, last + 1);
                    while lod < last && (off.contains(&lod) || d > me.lod_distance[lod]) {
                        lod += 1;
                    }

                    lod = (lod as i32 + v.coarser).min(last as i32) as usize;
                }

                // Bit 63 files the pair under `near`.
                let close = range.is_some_and(|(lo, hi)| {
                    let (dx, dy, dz) = (s[0] - v.eye.x, s[1] - v.eye.y, s[2] - v.eye.z);
                    let d = dx * dx + dy * dy + dz * dz;
                    lo <= d && d <= hi
                });

                for k in me.lod_first[lod]..me.lod_first[lod + 1] {
                    let b = me.lod_batch[k as usize];
                    let to_near = close && self.batch_mesh(b).kind == MeshKind::TreeLeaf;
                    pairs.push((u64::from(to_near) << 63) | (u64::from(b) << 32) | i as u64);
                }
            }

            pairs
        }).collect::<Vec<_>>();

        Self::group(&chunk_pairs, nb, out, false);
        if let Some((near, _)) = near.as_mut() {
            Self::group(&chunk_pairs, nb, near, true);
        }
    }

    /// Groups the culled (batch, item) pairs filed under `near` (bit 63) or not by batch: a counting sort, chunks in order.
    fn group(chunk_pairs: &[Vec<u64>], nb: usize, out: &mut Visible, near: bool) {
        let mine = |p: &&u64| (**p >> 63 == 1) == near;
        let batch = |p: u64| ((p >> 32) & 0x7fff_ffff) as usize;
        out.count = vec![0; nb];
        for p in chunk_pairs.iter().flatten().filter(mine) {
            out.count[batch(*p)] += 1;
        }

        out.start = vec![0; nb];
        out.active.clear();
        let mut at = 0;
        for b in 0..nb {
            out.start[b] = at;
            at += out.count[b];
            if out.count[b] != 0 {
                out.active.push(b as u32);
            }
        }

        let mut cursor = out.start.clone();
        out.order = vec![0; at as usize];
        out.batch_of = vec![0; at as usize];
        for p in chunk_pairs.iter().flatten().filter(mine) {
            let b = batch(*p);
            let pos = cursor[b] as usize;
            cursor[b] += 1;
            out.order[pos] = *p as u32;
            out.batch_of[pos] = b as u32;
        }
    }

    /// Fills one record per instance: the item's world matrix (with the mesh offset applied) and its pixel-shader parameters.
    ///
    /// `dst` is usually a mapped GPU buffer (write-combined memory): each record is built locally and stored once, never read back. `units`
    /// scales the world matrices (the game's units per scene unit; 1 for the scene's own).
    /// `origin` is subtracted from the translations (the render origin in the shaders' units, after the scale).
    pub fn gather(&self, vis: &Visible, dst: &mut [RigidInstance], units: f32, origin: [f32; 3]) {
        wce_prof!("drawset.gather");
        dst[..vis.order.len()].par_iter_mut().enumerate().for_each(|(k, r)| {
            let item = vis.order[k] as usize;
            let w = &self.world[item];
            let mut world = *w;
            let mesh = self.batch_mesh(vis.batch_of[k]);

            // translation += rotation * offset
            if mesh.has_offset {
                for (out, w) in world.iter_mut().zip(w) {
                    out[3] += w[0] * mesh.offset.x + w[1] * mesh.offset.y + w[2] * mesh.offset.z;
                }
            }

            if units != 1.0 {
                world = world.map(|row| row.map(|v| v * units));
            }

            for (row, o) in world.iter_mut().zip(origin) {
                row[3] -= o;
            }

            let params = if self.flags[item] & PARAMS != 0 {
                let at = self.params_index[item] as usize * 16;
                std::array::from_fn(|i| self.params[at + i])
            } else {
                DEFAULT_INSTANCE_PARAMS
            };

            let p = |i: usize| std::array::from_fn(|k| params[i * 4 + k]);
            let (mut p5, mut p6, mut p7) = (p(1), p(2), p(3));

            // A tree (rigidbranch / rigidleaf v3) blends two matrices per vertex: the world one (tilted to the terrain) and an untilted one
            // (TEXCOORD3..5, read from POSITION5..7's place), so that only the roots follow the slope (docs/re/vegetation.md). The entity
            // gives the untilted rows in p5..p7 (scene units, like the world); all zero: the world's.
            if matches!(mesh.kind, crate::model_cache::MeshKind::TreeBranch | crate::model_cache::MeshKind::TreeLeaf) {
                let given = [p5, p6, p7];
                let mut second = if given.iter().flatten().all(|&v| v == 0.0) { *w } else { given };
                if mesh.has_offset {
                    let rows = second;
                    for (out, r) in second.iter_mut().zip(&rows) {
                        out[3] += r[0] * mesh.offset.x + r[1] * mesh.offset.y + r[2] * mesh.offset.z;
                    }
                }

                if units != 1.0 {
                    second = second.map(|row| row.map(|v| v * units));
                }

                for (row, o) in second.iter_mut().zip(origin) {
                    row[3] -= o;
                }

                [p5, p6, p7] = second;
            }

            // A tree billboard (rigidtreebillboard vs30_main) reads POSITION5 as (position, 1 - dissolve) and POSITION6 as (facing x, tint
            // seed, facing z, size): from the world matrix (its z axis faces, its y axis is the size) and the seed of POSITION4.
            if mesh.kind == crate::model_cache::MeshKind::TreeBillboard {
                let size = (world[0][1] * world[0][1] + world[1][1] * world[1][1] + world[2][1] * world[2][1]).sqrt();
                p5 = [world[0][3], world[1][3], world[2][3], 1.0];
                p6 = [world[0][2], params[0], world[2][2], size];
            }

            *r = RigidInstance { world, p4: p(0), p5, p6, p7 };
        });
    }

    /// The entity of every instance (the id pass).
    pub fn gather_ids(&self, vis: &Visible) -> Vec<u32> {
        vis.order.iter().map(|&item| self.entity[item as usize]).collect()
    }

    /// Drops the instances whose entity `keep` rejects (the id pass skips locked layers), keeping the grouping by batch.
    pub fn retain(&self, vis: &mut Visible, keep: impl Fn(EntityId) -> bool) {
        let mut out = 0;
        for &b in &vis.active {
            let (first, n) = (vis.start[b as usize] as usize, vis.count[b as usize] as usize);
            let mut kept = 0;
            vis.start[b as usize] = out as u32;
            for i in first..first + n {
                if keep(self.entity[vis.order[i] as usize]) {
                    vis.order[out] = vis.order[i];
                    vis.batch_of[out] = vis.batch_of[i];
                    out += 1;
                    kept += 1;
                }
            }

            vis.count[b as usize] = kept;
        }

        vis.order.truncate(out);
        vis.batch_of.truncate(out);
        let count = &vis.count;
        vis.active.retain(|&b| count[b as usize] != 0);
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn removing_items_keeps_the_others_and_their_parameters() {
        let mut set = DrawSet::default();
        let model = Arc::new(GpuModel::default());
        let at = |x: f32| {
            let mut w = Float4x4::IDENTITY;
            w.m[0][3] = x;
            w
        };

        let params = |v: f32| std::array::from_fn::<f32, 16, _>(|i| if i == 0 { v } else { DEFAULT_INSTANCE_PARAMS[i] });
        for (k, entity) in [10u32, 11, 12, 13].into_iter().enumerate() {
            set.add(&model, &at(k as f32), (k % 2 == 1).then(|| params(k as f32)).as_ref(), true, false, entity);
        }

        let remap = set.remove_items(|e| e == 10 || e == 12);
        assert_eq!(remap, [u32::MAX, 0, u32::MAX, 1]);
        assert_eq!((set.items(), set.entity(0), set.entity(1)), (2, 11, 13));
        assert_eq!((set.world[0][0][3], set.world[1][0][3]), (1.0, 3.0));

        // Both kept items had parameters: packed again in order.
        assert_eq!(set.params.len(), 32);
        assert_eq!((set.params[set.params_index[0] as usize * 16], set.params[set.params_index[1] as usize * 16]), (1.0, 3.0));
    }

    #[test]
    fn killed_items_are_skipped_reused_and_compacted() {
        let mut set = DrawSet::default();
        let model = Arc::new(GpuModel::default());
        for entity in [1u32, 2, 3] {
            set.add(&model, &Float4x4::IDENTITY, None, true, false, entity);
        }

        set.kill(1);
        set.kill(1);
        assert_eq!((set.items(), set.dead(), set.slots()), (2, 1, 3));

        // The view culls nothing but the dead item (everything sits at the origin, inside a frustum of everything).
        let mut vis = Visible::default();
        set.cull(&View { frustum: Frustum::default(), ..Default::default() }, &mut vis);
        assert!(!vis.order.contains(&1));

        // A new item takes the dead slot; compacting drops what is dead and keeps the order.
        assert_eq!(set.add(&model, &Float4x4::IDENTITY, None, true, false, 4), 1);
        set.kill(0);
        let remap = set.compact();
        assert_eq!(remap, [u32::MAX, 0, 1]);
        assert_eq!((set.items(), set.dead(), set.entity(0), set.entity(1)), (2, 0, 4, 3));
    }
}
