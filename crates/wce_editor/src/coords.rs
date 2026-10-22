//! The coordinates readout in a corner of the viewport: the camera's and the cursor's positions in scene units and game units, and, when
//! a tool places the scene in a map (the map context), their map cells and the cursor's height above the sea. The cursor's point is the
//! ground under the mouse: a march along its ray over the scene's terrains (every drawable one, the map context's too).

use std::collections::HashMap;

use wce_foundation::math::{Float3, normalize};
use wce_scene::camera::Camera;
use wce_scene::render_scene::{EntityId, Payload, RenderScene};

use crate::gui;

/// Where a tool's scene sits in a map: frame x, z -> map cell (`cell = m * (x, z, 1)`), the sea's height (scene units) and the metres
/// in a scene unit.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MapFrame {
    pub to_map: [[f64; 3]; 2],
    pub sea_y: f32,
    pub metres: f64,
}

impl MapFrame {
    pub fn cell(&self, x: f32, z: f32) -> (f64, f64) {
        let (x, z, m) = (f64::from(x), f64::from(z), &self.to_map);
        (m[0][0] * x + m[0][1] * z + m[0][2], m[1][0] * x + m[1][1] * z + m[1][2])
    }
}

/// The side of the grid the terrains are bucketed on (scene units).
const BUCKET: f32 = 256.0;

/// The scene's terrains by the grid squares their bounds touch, made again when the scene's structure or terrains change.
#[derive(Default)]
pub struct GroundProbe {
    key: (usize, u64, u64),
    buckets: HashMap<(i32, i32), Vec<EntityId>>,

    /// The last cursor ray (eye, direction) and what it hit.
    last: Option<([f32; 6], Option<Float3>)>,
}

impl GroundProbe {
    fn refresh(&mut self, scene: &RenderScene) {
        let key = (std::ptr::from_ref(scene) as usize, scene.structure_revision(), scene.terrain_revision());
        if key == self.key {
            return;
        }

        self.key = key;
        self.last = None;
        self.buckets.clear();
        for e in scene.entities() {
            let Payload::Terrain(t) = &e.payload else { continue };
            if !scene.drawable(e) || t.nx < 2 || t.nz < 2 {
                continue;
            }

            // The grid's corners in the world (the matrix turns and scales xz, the heights do not move them).
            let (x1, z1) = (t.origin_x + (t.nx - 1) as f32 * t.step, t.origin_z + (t.nz - 1) as f32 * t.step);
            let m = &e.world.m;
            let (mut lo, mut hi) = ([f32::MAX; 2], [f32::MIN; 2]);
            for (lx, lz) in [(t.origin_x, t.origin_z), (x1, t.origin_z), (t.origin_x, z1), (x1, z1)] {
                let (x, z) = (m[0][0] * lx + m[0][2] * lz + m[0][3], m[2][0] * lx + m[2][2] * lz + m[2][3]);
                lo = [lo[0].min(x), lo[1].min(z)];
                hi = [hi[0].max(x), hi[1].max(z)];
            }

            for i in (lo[0] / BUCKET).floor() as i32..=(hi[0] / BUCKET).floor() as i32 {
                for j in (lo[1] / BUCKET).floor() as i32..=(hi[1] / BUCKET).floor() as i32 {
                    self.buckets.entry((i, j)).or_default().push(e.id);
                }
            }
        }
    }

    /// The highest ground of the terrains at a point.
    fn ground(&self, scene: &RenderScene, x: f32, z: f32) -> Option<f32> {
        let ids = self.buckets.get(&((x / BUCKET).floor() as i32, (z / BUCKET).floor() as i32))?;
        ids.iter()
            .filter_map(|&id| scene.find(id))
            .filter_map(|e| match &e.payload {
                Payload::Terrain(t) => t.ground_at(&e.world, x, z),
                _ => None,
            })
            .fold(None, |m: Option<f32>, h| Some(m.map_or(h, |m| m.max(h))))
    }

    /// Where a ray meets the ground first (out to `far`): steps that grow with the distance, then halvings of the step that crossed.
    pub fn hit(&mut self, scene: &RenderScene, eye: Float3, dir: Float3, far: f32) -> Option<Float3> {
        self.refresh(scene);
        let dir = normalize(dir);
        let ray = [eye.x, eye.y, eye.z, dir.x, dir.y, dir.z];
        if let Some((r, hit)) = self.last
            && r == ray
        {
            return hit;
        }

        let at = |t: f32| eye + dir * t;
        let below = |t: f32| {
            let p = at(t);
            self.ground(scene, p.x, p.z).is_some_and(|g| p.y <= g)
        };

        let (mut t0, mut t) = (0.0f32, 0.5f32);
        let mut hit = None;
        while t < far {
            if below(t) {
                let mut t1 = t;
                for _ in 0..24 {
                    let mid = 0.5 * (t0 + t1);
                    if below(mid) { t1 = mid } else { t0 = mid }
                }

                let p = at(t1);
                hit = Some(Float3::new(p.x, self.ground(scene, p.x, p.z).unwrap_or(p.y), p.z));
                break;
            }

            t0 = t;
            t += (t * 0.004).max(0.5);
        }

        self.last = Some((ray, hit));
        hit
    }
}

/// The readout's lines: the camera, its target (orbit) or nothing (fly), the cursor's ground point.
pub fn lines(cam: &Camera, orbit: bool, cursor: Option<Float3>, game_units: f32, map: Option<&MapFrame>) -> Vec<String> {
    let point = |name: &str, p: Float3, height: bool| {
        let g = p * game_units;
        let mut s = format!("{name:<7}{:>10.1} {:>8.1} {:>10.1}   game {:>10.1} {:>8.1} {:>10.1}", p.x, p.y, p.z, g.x, g.y, g.z);
        if let Some(m) = map {
            let (cx, cy) = m.cell(p.x, p.z);
            s += &format!("   cell {cx:.2}, {cy:.2}");
            if height {
                s += &format!("   {:.2} m above the sea", f64::from(p.y - m.sea_y) * m.metres);
            }
        }

        s
    };

    let mut out = vec![point("camera", cam.eye, false)];
    if orbit {
        out.push(point("target", cam.target, false));
    }

    out.push(cursor.map_or_else(|| "cursor  -".to_owned(), |c| point("cursor", c, true)));
    out
}

/// The readout in the image's bottom-left corner.
pub fn draw(origin: gui::Vec2, size: gui::Vec2, lines: &[String]) {
    if lines.is_empty() || size.y < 80.0 {
        return;
    }

    let (lh, pad) = (gui::text_line_height(), 6.0);
    let w = lines.iter().map(|l| gui::calc_text_size(l).x).fold(0.0, f32::max);
    let h = lh * lines.len() as f32;
    let (x0, y0) = (origin.x + 8.0, origin.y + size.y - 8.0 - h - 2.0 * pad);
    let dl = gui::window_draw_list();
    dl.push_clip_rect(origin, gui::v2(origin.x + size.x, origin.y + size.y));
    dl.rect_filled(gui::v2(x0, y0), gui::v2(x0 + w + 2.0 * pad, y0 + h + 2.0 * pad), gui::col32(0, 0, 0, 150), 4.0);
    for (k, l) in lines.iter().enumerate() {
        dl.text(gui::v2(x0 + pad, y0 + pad + k as f32 * lh), gui::col32(235, 235, 235, 255), l);
    }

    dl.pop_clip_rect();
}
