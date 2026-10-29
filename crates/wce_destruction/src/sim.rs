//! A collapse simulated (rapier3d, deterministic): each fragment a convex hull held in place until its release time, then dynamic with
//! its initial velocity and spin; the ground (y = 0), the standing part and the earlier collapses' debris fixed. Sampled at the
//! collapse's frame rate until everything rests, or the longest duration.

use rapier3d::prelude::*;

use crate::geom::V3;

/// A fragment: its bone's origin (the building's frame), its hull's points around it, what it is made of, and how it starts.
#[derive(Clone, Debug)]
pub struct Body {
    pub centre: V3,
    pub hull: Vec<V3>,
    pub density: f32,
    pub friction: f32,
    pub restitution: f32,
    pub release: f32,
    pub linvel: V3,
    pub angvel: V3,
}

/// A fixed triangle mesh.
#[derive(Clone, Debug, Default)]
pub struct Fixed {
    pub vertices: Vec<V3>,
    pub triangles: Vec<[u32; 3]>,
}

/// A fragment hitting something hard: when, where, how fast.
#[derive(Clone, Copy, Debug)]
pub struct Impact {
    pub time: f32,
    pub body: usize,
    pub at: V3,
    pub speed: f32,
}

/// The simulated collapse: per frame, per fragment its translation (of its origin) and rotation (x y z w).
#[derive(Clone, Debug, Default)]
pub struct Track {
    pub frames: Vec<Vec<([f32; 3], [f32; 4])>>,
    pub impacts: Vec<Impact>,
}

impl Track {
    pub fn last(&self) -> Option<&Vec<([f32; 3], [f32; 4])>> {
        self.frames.last()
    }

    pub fn duration(&self, fps: f32) -> f32 {
        self.frames.len().saturating_sub(1) as f32 / fps
    }
}

fn g(v: &V3) -> Vector {
    Vector::new(v.x, v.y, v.z)
}

/// Simulates the bodies among the fixed meshes; frames at `fps` for at most `max` seconds (at least `min`).
pub fn simulate(bodies: &[Body], fixed: &[Fixed], fps: f32, min: f32, max: f32) -> Track {
    let mut world = PhysicsWorld::new();
    world.integration_parameters.dt = 1.0 / 60.0;
    world.colliders.insert(ColliderBuilder::cuboid(1000.0, 50.0, 1000.0).translation(Vector::new(0.0, -50.0, 0.0)).friction(0.9).build());
    for f in fixed {
        if f.triangles.is_empty() {
            continue;
        }

        if let Ok(c) = ColliderBuilder::trimesh(f.vertices.iter().map(g).collect(), f.triangles.clone()) {
            world.colliders.insert(c.friction(0.8).build());
        }
    }

    let mut handles = Vec::with_capacity(bodies.len());
    for b in bodies {
        // A hull a little smaller than the fragment: neighbours touching at the start do not push each other apart.
        let pts = b.hull.iter().map(|p| g(&(p * 0.92))).collect::<Vec<_>>();
        let collider = ColliderBuilder::convex_hull(&pts).unwrap_or_else(|| ColliderBuilder::ball(0.2));
        let body = RigidBodyBuilder::fixed().translation(g(&b.centre)).ccd_enabled(true).linear_damping(0.15).angular_damping(0.8);
        let h = world.bodies.insert(body);
        world.colliders.insert_with_parent(collider.density(b.density).friction(b.friction).restitution(b.restitution).build(), h, &mut world.bodies);
        handles.push(h);
    }

    let step = world.integration_parameters.dt;
    let frame_every = 1.0 / fps.max(1.0);
    let mut track = Track::default();
    let mut released = vec![false; bodies.len()];
    let mut last_speed = vec![0.0f32; bodies.len()];
    let mut last_impact = vec![f32::MIN; bodies.len()];
    let mut quiet_since = 0.0f32;
    let mut t = 0.0f32;
    let mut next_frame = 0.0f32;
    let last_release = bodies.iter().map(|b| b.release).fold(0.0, f32::max);
    loop {
        if t + 1e-6 >= next_frame {
            track.frames.push(handles.iter().map(|h| {
                let p = world.bodies[*h].position();
                let (tr, q) = (p.translation, p.rotation);
                ([tr.x, tr.y, tr.z], [q.x, q.y, q.z, q.w])
            }).collect());
            next_frame += frame_every;
            let done = t >= min && t > last_release && quiet_since >= 0.5;
            if done || t >= max {
                break;
            }
        }

        for (i, b) in bodies.iter().enumerate() {
            if !released[i] && t >= b.release {
                released[i] = true;
                let rb = &mut world.bodies[handles[i]];
                rb.set_body_type(RigidBodyType::Dynamic, true);
                rb.set_linvel(g(&b.linvel), true);
                rb.set_angvel(g(&b.angvel), true);
            }
        }

        world.step();
        t += step;

        let mut moving = false;
        for (i, h) in handles.iter().enumerate() {
            if !released[i] {
                moving = true;
                continue;
            }

            let rb = &world.bodies[*h];
            let v = rb.linvel();
            let speed = (v.x * v.x + v.y * v.y + v.z * v.z).sqrt();
            if speed > 0.15 || rb.angvel().length() > 0.3 {
                moving = true;
            }

            // A sudden loss of speed: it hit something.
            if last_speed[i] - speed > 2.0 && t - last_impact[i] > 0.4 {
                let p = rb.translation();
                track.impacts.push(Impact { time: t, body: i, at: V3::new(p.x, p.y, p.z), speed: last_speed[i] });
                last_impact[i] = t;
            }

            last_speed[i] = speed;
        }

        quiet_since = if moving { 0.0 } else { quiet_since + step };
    }

    track
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_block_falls_and_rests_on_the_ground() {
        let hull = [-0.5f32, 0.5].iter().flat_map(|&x| [-0.5f32, 0.5].iter().flat_map(move |&y| [-0.5f32, 0.5].map(move |z| V3::new(x, y, z)))).collect::<Vec<_>>();
        let b = Body { centre: V3::new(0.0, 5.0, 0.0), hull, density: 2.3, friction: 0.9, restitution: 0.05, release: 0.2, linvel: V3::new(1.0, 0.0, 0.0), angvel: V3::zeros() };
        let a = simulate(std::slice::from_ref(&b), &[], 20.0, 1.0, 8.0);
        let b2 = simulate(&[b], &[], 20.0, 1.0, 8.0);
        assert_eq!(a.frames, b2.frames, "deterministic");
        let first = a.frames[0][0].0;
        let last = a.last().unwrap()[0].0;
        assert!((first[1] - 5.0).abs() < 1e-5);
        assert!(last[1] < 0.6 && last[1] > 0.3, "rests on the ground: {last:?}");
        assert!(last[0] > 0.5, "moved along its push: {last:?}");
        assert!(!a.impacts.is_empty());
        assert!(a.duration(20.0) < 8.0);
    }
}
