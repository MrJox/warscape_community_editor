//! The coordinates readout in a corner of the viewport: the camera's and the cursor's positions in scene units and game units, and, when
//! a tool places the scene in a map (the map context), their map cells and the cursor's height above the sea. The cursor's point is the
//! ground under the mouse: a march along its ray over the scene's terrains (every drawable one, the map context's too).

use std::collections::HashMap;

use wce_foundation::math::{Float3, length, normalize};
use wce_scene::camera::Camera;
use wce_scene::render_scene::{EntityId, Payload, RenderScene, Water};

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

/// Where the game's battle puts its origin in the scene (scene units): game = (point - origin) * game units. A tile's battle is centred
/// on the tile; its height zero is the map's sea level at a map placement, the tile's lowest height for a tile alone (the cameras of
/// in-game captures against the editor's, docs/re/game_command_line.md).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BattleFrame {
    pub origin: Float3,

    /// The battle's frame turned in the scene (degrees; `RenderScene::world_yaw`).
    pub yaw: f32,
}

impl BattleFrame {
    /// A scene point in the battle's frame (scene units, not scaled to game units).
    pub fn local(&self, p: Float3) -> Float3 {
        let d = p - self.origin;
        let (s, c) = self.yaw.to_radians().sin_cos();
        Float3::new(c * d.x - s * d.z, d.y, s * d.x + c * d.z)
    }
}

/// The side of the grid the terrains are bucketed on (scene units).
const BUCKET: f32 = 256.0;

/// A sea plane: its height and its rectangle.
#[derive(Clone, Copy, Debug)]
struct SeaPlane {
    y: f32,
    lo: [f32; 2],
    hi: [f32; 2],
}

impl SeaPlane {
    fn of(w: &Water) -> Option<Self> {
        let first = w.verts.first()?;
        let start = Self { y: first.y, lo: [first.x, first.z], hi: [first.x, first.z] };
        Some(w.verts.iter().fold(start, |s, v| Self {
            y: s.y.max(v.y), lo: [s.lo[0].min(v.x), s.lo[1].min(v.z)], hi: [s.hi[0].max(v.x), s.hi[1].max(v.z)],
        }))
    }

    fn covers(&self, x: f32, z: f32) -> bool {
        (self.lo[0]..=self.hi[0]).contains(&x) && (self.lo[1]..=self.hi[1]).contains(&z)
    }
}

/// The scene's terrains by the grid squares their bounds touch and its sea planes, made again when the scene's structure or terrains change.
#[derive(Default)]
pub struct GroundProbe {
    key: (usize, u64, u64),
    buckets: HashMap<(i32, i32), Vec<EntityId>>,
    seas: Vec<SeaPlane>,

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
        self.seas.clear();
        for e in scene.entities() {
            if !scene.drawable(e) {
                continue;
            }

            let t = match &e.payload {
                Payload::Terrain(t) if t.nx >= 2 && t.nz >= 2 => t,
                Payload::Water(w) if !w.river => {
                    self.seas.extend(SeaPlane::of(w));
                    continue;
                }

                _ => continue,
            };

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

    /// The highest ground of the terrains at a point (metres).
    pub fn ground_at(&mut self, scene: &RenderScene, x: f32, z: f32) -> Option<f32> {
        self.refresh(scene);
        self.ground(scene, x, z)
    }

    /// What the cameras stand on at a point: the ground, or the sea where it is over it (or off the terrains).
    pub fn surface_at(&mut self, scene: &RenderScene, x: f32, z: f32) -> Option<f32> {
        self.refresh(scene);
        let sea = self.seas.iter().filter(|s| s.covers(x, z)).map(|s| s.y).reduce(f32::max);
        match (self.ground(scene, x, z), sea) {
            (Some(g), Some(s)) => Some(g.max(s)),
            (g, s) => g.or(s),
        }
    }

    /// Where a ray meets the ground or the sea first (out to `far`). From under a sea, its surface is not in the way.
    pub fn surface_hit(&mut self, scene: &RenderScene, eye: Float3, dir: Float3, far: f32) -> Option<Float3> {
        let ground = self.hit(scene, eye, dir, far);
        let dir = normalize(dir);
        if dir.y >= 0.0 {
            return ground;
        }

        let sea = self
            .seas
            .iter()
            .filter(|s| eye.y > s.y)
            .filter_map(|s| {
                let t = (s.y - eye.y) / dir.y;
                let p = eye + dir * t;
                (t < far && s.covers(p.x, p.z)).then_some((t, p))
            })
            .min_by(|a, b| a.0.total_cmp(&b.0));

        match (ground, sea) {
            (Some(g), Some((t, p))) => Some(if length(g - eye) <= t { g } else { p }),
            (g, s) => g.or(s.map(|(_, p)| p)),
        }
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
pub fn lines(cam: &Camera, orbit: bool, cursor: Option<Float3>, game_units: f32, map: Option<&MapFrame>, battle: Option<&BattleFrame>) -> Vec<String> {
    let point = |name: &str, p: Float3, height: bool| {
        let g = battle.map_or(p, |b| b.local(p)) * game_units;
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

#[cfg(test)]
mod tests {
    use super::*;
    use wce_foundation::math::Float4x4;
    use wce_scene::render_scene::{SourceRef, WaterVertex};

    fn scene_with(river: bool) -> RenderScene {
        let v = |x: f32, z: f32| WaterVertex { x, y: 3.0, z, ..Default::default() };
        let water = Water { river, verts: vec![v(-10.0, -10.0), v(10.0, -10.0), v(-10.0, 10.0), v(10.0, 10.0)], indices: vec![0, 2, 1, 1, 2, 3], ..Default::default() };
        let mut scene = RenderScene::default();
        let layer = scene.add_layer(1, "test");
        scene.add(layer, Float4x4::IDENTITY, Payload::Water(water), SourceRef { doc: 1, item: 1 });
        scene
    }

    #[test]
    fn the_sea_is_ground_for_the_cameras_only() {
        let scene = scene_with(false);
        let mut probe = GroundProbe::default();
        assert_eq!(probe.surface_at(&scene, 0.0, 0.0), Some(3.0), "the sea holds the camera");
        assert_eq!(probe.surface_at(&scene, 20.0, 0.0), None, "nothing past its edge");
        assert_eq!(probe.ground_at(&scene, 0.0, 0.0), None, "the cursor's ground stays the terrain");

        let down = Float3::new(0.0, -1.0, 0.0);
        let hit = probe.surface_hit(&scene, Float3::new(1.0, 8.0, 2.0), down, 1000.0);
        assert_eq!(hit, Some(Float3::new(1.0, 3.0, 2.0)), "a ray stops on the sea");
        assert_eq!(probe.surface_hit(&scene, Float3::new(1.0, 1.0, 2.0), down, 1000.0), None, "not from under it");
    }

    #[test]
    fn rivers_are_not_ground() {
        let scene = scene_with(true);
        assert_eq!(GroundProbe::default().surface_at(&scene, 0.0, 0.0), None);
    }
}
