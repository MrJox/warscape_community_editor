//! What the viewport's cameras stand on and bump into ([`CameraWorld`]): the terrain and the sea (the coordinates' ground probe) and the
//! models' bounding boxes.
//!
//! TODO: collide with the buildings' real collision meshes (their cs2 hard collision, `docs/re/buildings.md`) instead of their models'
//! bounding boxes: a box shuts a courtyard or the space under an arch.

use std::collections::HashMap;

use wce_editor_core::camera_controller::CameraWorld;
use wce_foundation::math::{Float3, Float4x4, inverse, length};
use wce_render::model_cache::{ModelCache, State};
use wce_scene::render_scene::{Entity, EntityId, Payload, RenderScene, StructureChange};

use crate::coords::GroundProbe;

/// Bucket size in metres.
const BUCKET: f32 = 64.0;

/// A box over more buckets than this is kept in a list of its own (its bucket range) instead of in each of its buckets: a campaign map's
/// props (`fallen_leaves_medium`, 6 500 units across) put 1.6 M entries in the buckets, 140 ms each time the streamed map changed.
const LARGE_BUCKETS: i64 = 64;

/// A model's box in the world: its matrix, the matrix's inverse and its local corners; dead once its entity went.
#[derive(Clone, Copy, Debug)]
struct Obstacle {
    world: Float4x4,
    to_local: Float4x4,
    bmin: Float3,
    bmax: Float3,
    id: EntityId,
    alive: bool,
}

/// The models' boxes of a scene, in buckets over the ground. They follow the scene's structure log: the boxes of removed entities die, the
/// added entities' come (a streamed map adds and drops chunks most frames: reading every box again each time was 3 to 5 ms, 140 ms on a
/// campaign map before its large props left the buckets). Read again when the log says anything else, when an obstacle moves, or once
/// many boxes are dead. Not when anything else moves: a preview follows the mouse, and reading every box again for it cost a frame's worth
/// of time at every step of the mouse.
#[derive(Default)]
pub struct ObstacleProbe {
    /// The scene the boxes are of, its structure log's position and the first entity id not looked at yet.
    uid: usize,
    cursor: u64,
    next: EntityId,
    moved: u64,

    /// Frames since the entities whose models were loading were looked at again, and those entities.
    frames: u32,
    pending: Vec<EntityId>,

    /// Each entity's box.
    ids: HashMap<EntityId, u32>,
    boxes: Vec<Obstacle>,
    dead: usize,
    buckets: HashMap<(i32, i32), Vec<u32>>,

    /// The boxes over more than `LARGE_BUCKETS` buckets, in order: the box and its bucket range (x and z, inclusive).
    large: Vec<(u32, [i32; 4])>,

    /// The boxes a point is tested against (reused).
    near: Vec<u32>,
}

impl ObstacleProbe {
    fn refresh(&mut self, scene: &RenderScene, models: &ModelCache) {
        let moved = scene.moved_since(&mut self.moved);
        let obstacle_moved = moved.is_none_or(|ids| ids.iter().any(|id| self.ids.contains_key(id)));
        self.frames += 1;

        // What changed since: removals and payloads follow, anything else reads every box again.
        let mut cursor = self.cursor;
        let log = scene.structure_since(&mut cursor).filter(|_| self.uid == scene.uid() && !obstacle_moved);
        let follow = log.filter(|l| l.iter().all(|c| !matches!(c, StructureChange::Other)));
        let Some(log) = follow.filter(|_| self.dead <= 4096 || self.dead * 2 <= self.boxes.len()) else {
            wce_foundation::wce_prof!("camera.obstacles_refresh");
            self.uid = scene.uid();
            (self.frames, self.dead) = (0, 0);
            self.pending.clear();
            self.ids.clear();
            self.boxes.clear();
            self.buckets.clear();
            self.large.clear();
            for e in scene.entities() {
                self.add(scene, models, e);
            }

            self.next = scene.next_id();
            let _ = scene.structure_since(&mut self.cursor);
            return;
        };

        self.cursor = cursor;
        let retry = !self.pending.is_empty() && self.frames >= 60;
        if log.is_empty() && self.next == scene.next_id() && !retry {
            return;
        }

        wce_foundation::wce_prof!("camera.obstacles_follow");
        for c in &log {
            match c {
                StructureChange::Removed(r) => {
                    if r.end - r.start > self.ids.len() as EntityId {
                        let gone: Vec<EntityId> = self.ids.keys().copied().filter(|id| r.contains(id)).collect();
                        gone.into_iter().for_each(|id| self.kill(id));
                    } else {
                        r.clone().for_each(|id| self.kill(id));
                    }

                    self.pending.retain(|id| !r.contains(id));
                }

                // Another payload (a mesh or not): its box as it is now.
                StructureChange::Payload(id) => {
                    self.kill(*id);
                    self.pending.retain(|p| p != id);
                    if *id < self.next
                        && let Some(e) = scene.find(*id)
                    {
                        self.add(scene, models, e);
                    }
                }

                StructureChange::Other => unreachable!("filtered above"),
            }
        }

        // The entities added since (their ids follow; the scene keeps them in id order), and the ones whose models were loading.
        let all = scene.entities();
        let first = all.partition_point(|e| e.id < self.next);
        for e in &all[first..] {
            self.add(scene, models, e);
        }

        self.next = scene.next_id();
        if retry {
            self.frames = 0;
            for id in std::mem::take(&mut self.pending) {
                if let Some(e) = scene.find(id) {
                    self.add(scene, models, e);
                }
            }
        }
    }

    /// The box of an entity that is a drawn mesh (noted to look at again while its model is loading).
    fn add(&mut self, scene: &RenderScene, models: &ModelCache, e: &Entity) {
        let Payload::MeshInstance(mi) = &e.payload else { return };
        if !scene.drawable(e) || e.transient {
            return;
        }

        let (model, state) = models.peek(&mi.model);
        let Some(m) = model else {
            if state != State::Failed {
                self.pending.push(e.id);
            }

            return;
        };

        // Smaller than the eye, or decals alone (painted on what is under them; their box is a cube about them: a campaign map's
        // fallen_leaves patches made walls 4 600 units tall over its forests): nothing to bump into.
        if length(m.bmax - m.bmin) < 0.5 || m.meshes.is_empty() {
            return;
        }

        // A campaign's tree clumps: the camera flies over the forests, as the game's (and a box a clump, tens of thousands of them coming
        // and going with each rebuild of the forests ahead, had every box read again every second or so while the camera turned: 60 - 90
        // ms a time).
        if m.meshes.iter().all(|g| matches!(g.kind, wce_render::model_cache::MeshKind::CampaignVegetation)) {
            return;
        }

        let (mut lo, mut hi) = ([f32::MAX; 2], [f32::MIN; 2]);
        for k in 0..8 {
            let c = Float3::new(
                if k & 1 != 0 { m.bmax.x } else { m.bmin.x },
                if k & 2 != 0 { m.bmax.y } else { m.bmin.y },
                if k & 4 != 0 { m.bmax.z } else { m.bmin.z },
            );
            let p = e.world.transform_point(c);
            lo = [lo[0].min(p.x), lo[1].min(p.z)];
            hi = [hi[0].max(p.x), hi[1].max(p.z)];
        }

        let n = self.boxes.len() as u32;
        self.ids.insert(e.id, n);
        self.boxes.push(Obstacle { world: e.world, to_local: inverse(&e.world), bmin: m.bmin, bmax: m.bmax, id: e.id, alive: true });
        let range = [
            (lo[0] / BUCKET).floor() as i32,
            (hi[0] / BUCKET).floor() as i32,
            (lo[1] / BUCKET).floor() as i32,
            (hi[1] / BUCKET).floor() as i32,
        ];
        if (i64::from(range[1]) - i64::from(range[0]) + 1) * (i64::from(range[3]) - i64::from(range[2]) + 1) > LARGE_BUCKETS {
            self.large.push((n, range));
            return;
        }

        for i in range[0]..=range[1] {
            for j in range[2]..=range[3] {
                self.buckets.entry((i, j)).or_default().push(n);
            }
        }
    }

    /// An entity's box dies (its bucket entries stay until the boxes are read again).
    fn kill(&mut self, id: EntityId) {
        if let Some(n) = self.ids.remove(&id) {
            self.boxes[n as usize].alive = false;
            self.dead += 1;
        }
    }

    /// A sphere moved out of the boxes: to the box's nearest point plus the radius when its centre is outside, out through the nearest face
    /// when it is inside. Twice, for corners between boxes.
    pub fn push_out(&mut self, scene: &RenderScene, models: &ModelCache, centre: Float3, radius: f32) -> Float3 {
        self.refresh(scene, models);
        let mut p = centre;
        let mut near = std::mem::take(&mut self.near);
        for _ in 0..2 {
            // The bucket's boxes and the large ones over it.
            let cell = ((p.x / BUCKET).floor() as i32, (p.z / BUCKET).floor() as i32);
            near.clear();
            near.extend(self.buckets.get(&cell).into_iter().flatten().copied());
            near.extend(
                self.large.iter().filter(|(_, r)| (r[0]..=r[1]).contains(&cell.0) && (r[2]..=r[3]).contains(&cell.1)).map(|(n, _)| *n),
            );
            if near.is_empty() {
                break;
            }

            // In the scene's order (entity ids), as a reading afresh has them: the pushes add up in turn, and a box that came late (its
            // model was loading) must not take another's turn.
            near.sort_unstable_by_key(|&i| self.boxes[i as usize].id);

            for &i in &near {
                let o = &self.boxes[i as usize];
                if !o.alive {
                    continue;
                }

                let l = o.to_local.transform_point(p);
                let clamp = Float3::new(l.x.clamp(o.bmin.x, o.bmax.x), l.y.clamp(o.bmin.y, o.bmax.y), l.z.clamp(o.bmin.z, o.bmax.z));
                let inside = clamp == l;
                let near = o.world.transform_point(clamp);
                let d = p - near;
                let dl = length(d);
                if !inside {
                    if dl < radius && dl > 1e-6 {
                        p = near + d * (radius / dl);
                    }

                    continue;
                }

                // Inside: out through the nearest face (measured in the world: the matrix may scale).
                let mut best: Option<(f32, Float3)> = None;
                for (axis, lo, hi) in [(0, o.bmin.x, o.bmax.x), (1, o.bmin.y, o.bmax.y), (2, o.bmin.z, o.bmax.z)] {
                    for face in [lo, hi] {
                        let mut q = l;
                        match axis {
                            0 => q.x = face,
                            1 => q.y = face,
                            _ => q.z = face,
                        }

                        let w = o.world.transform_point(q);
                        let dist = length(w - p);
                        if best.is_none_or(|(b, _)| dist < b) {
                            best = Some((dist, w));
                        }
                    }
                }

                if let Some((dist, w)) = best {
                    p = if dist > 1e-6 { w + (w - p) * (radius / dist) } else { w };
                }
            }
        }

        self.near = near;
        p
    }
}

/// The viewport's world for one camera update.
pub struct ViewportWorld<'a> {
    pub scene: &'a RenderScene,
    pub ground: &'a mut GroundProbe,
    pub obstacles: &'a mut ObstacleProbe,
    pub models: Option<&'a ModelCache>,
    pub far: f32,
}

impl CameraWorld for ViewportWorld<'_> {
    fn ground(&mut self, x: f32, z: f32) -> Option<f32> {
        self.ground.surface_at(self.scene, x, z)
    }

    fn ray_ground(&mut self, origin: Float3, dir: Float3) -> Option<Float3> {
        self.ground.surface_hit(self.scene, origin, dir, self.far)
    }

    fn push_out(&mut self, centre: Float3, radius: f32) -> Float3 {
        match self.models {
            Some(m) => self.obstacles.push_out(self.scene, m, centre, radius),
            None => centre,
        }
    }

    fn game_units(&self) -> f32 {
        self.scene.game_units
    }

    fn clearance_scale(&self) -> f32 {
        if self.scene.campaign { 1.0 / self.scene.game_units.max(1e-9) } else { 1.0 }
    }
}
