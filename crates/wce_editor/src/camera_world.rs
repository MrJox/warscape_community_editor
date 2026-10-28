//! What the viewport's cameras stand on and bump into ([`CameraWorld`]): the terrain (the coordinates' ground probe) and the models'
//! bounding boxes.
//!
//! TODO: collide with the buildings' real collision meshes (their cs2 hard collision, `docs/re/buildings.md`) instead of their models'
//! bounding boxes: a box shuts a courtyard or the space under an arch.

use std::collections::HashMap;

use wce_editor_core::camera_controller::CameraWorld;
use wce_foundation::math::{Float3, Float4x4, inverse, length};
use wce_render::model_cache::{ModelCache, State};
use wce_scene::render_scene::{Payload, RenderScene};

use crate::coords::GroundProbe;

/// Bucket size in metres.
const BUCKET: f32 = 64.0;

/// A model's box in the world: its matrix, the matrix's inverse and its local corners.
#[derive(Clone, Copy, Debug)]
struct Obstacle {
    world: Float4x4,
    to_local: Float4x4,
    bmin: Float3,
    bmax: Float3,
}

/// The models' boxes of a scene, in buckets over the ground, read again when the scene changes (or while models are still loading).
#[derive(Default)]
pub struct ObstacleProbe {
    key: (usize, u64),
    missing: bool,
    frames: u32,
    boxes: Vec<Obstacle>,
    buckets: HashMap<(i32, i32), Vec<u32>>,
}

impl ObstacleProbe {
    fn refresh(&mut self, scene: &RenderScene, models: &ModelCache) {
        let key = (std::ptr::from_ref(scene) as usize, scene.revision());
        self.frames += 1;
        if key == self.key && !(self.missing && self.frames >= 60) {
            return;
        }

        (self.key, self.missing, self.frames) = (key, false, 0);
        self.boxes.clear();
        self.buckets.clear();
        for e in scene.entities() {
            let Payload::MeshInstance(mi) = &e.payload else { continue };
            if !scene.drawable(e) {
                continue;
            }

            let (model, state) = models.peek(&mi.model);
            let Some(m) = model else {
                self.missing |= state != State::Failed;
                continue;
            };

            // Smaller than the eye: nothing to bump into.
            if length(m.bmax - m.bmin) < 0.5 {
                continue;
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
            self.boxes.push(Obstacle { world: e.world, to_local: inverse(&e.world), bmin: m.bmin, bmax: m.bmax });
            for i in (lo[0] / BUCKET).floor() as i32..=(hi[0] / BUCKET).floor() as i32 {
                for j in (lo[1] / BUCKET).floor() as i32..=(hi[1] / BUCKET).floor() as i32 {
                    self.buckets.entry((i, j)).or_default().push(n);
                }
            }
        }
    }

    /// A sphere moved out of the boxes: to the box's nearest point plus the radius when its centre is outside, out through the nearest face
    /// when it is inside. Twice, for corners between boxes.
    pub fn push_out(&mut self, scene: &RenderScene, models: &ModelCache, centre: Float3, radius: f32) -> Float3 {
        self.refresh(scene, models);
        let mut p = centre;
        for _ in 0..2 {
            let Some(ids) = self.buckets.get(&((p.x / BUCKET).floor() as i32, (p.z / BUCKET).floor() as i32)) else { return p };
            for &i in ids {
                let o = &self.boxes[i as usize];
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
        self.ground.ground_at(self.scene, x, z)
    }

    fn ray_ground(&mut self, origin: Float3, dir: Float3) -> Option<Float3> {
        self.ground.hit(self.scene, origin, dir, self.far)
    }

    fn push_out(&mut self, centre: Float3, radius: f32) -> Float3 {
        match self.models {
            Some(m) => self.obstacles.push_out(self.scene, m, centre, radius),
            None => centre,
        }
    }
}
