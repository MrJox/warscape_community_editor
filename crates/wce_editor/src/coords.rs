//! The coordinates readout in a corner of the viewport: the camera's and the cursor's positions in scene units and game units, and, when
//! a tool places the scene in a map (the map context), their map cells and the cursor's height above the sea. The cursor's point is the
//! ground under the mouse: a march along its ray over the scene's terrains (every drawable one, the map context's too).

use std::collections::HashMap;

use wce_foundation::math::{Float3, length, normalize};
use wce_scene::camera::Camera;
use wce_scene::render_scene::{EntityId, Payload, RenderScene, StructureChange, Water};

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

/// A terrain over more buckets than this goes in the coarse grid (`COARSE` buckets a side) instead: a map view's far LF ground and blocks
/// span thousands of buckets, and filling them took 6 ms each time the streamed map changed.
const LARGE_BUCKETS: i64 = 64;
const COARSE: f32 = BUCKET * 16.0;

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
            y: s.y.max(v.y),
            lo: [s.lo[0].min(v.x), s.lo[1].min(v.z)],
            hi: [s.hi[0].max(v.x), s.hi[1].max(v.z)],
        }))
    }

    fn covers(&self, x: f32, z: f32) -> bool {
        (self.lo[0]..=self.hi[0]).contains(&x) && (self.lo[1]..=self.hi[1]).contains(&z)
    }
}

/// The scene's terrains by the grid squares their bounds touch and its sea planes. They follow the scene's structure log (a streamed map
/// adds and drops chunks most frames: reading every terrain again each time was 3 to 6 ms): the entries of removed terrains stay until
/// they are many (a lookup skips them, ids are never reused), the added entities' come. Read again when the log says anything else or a
/// terrain's payload changes (`terrain_revision`: a brush stroke).
#[derive(Default)]
pub struct GroundProbe {
    /// The scene, its terrain revision, its structure log's position and the first entity id not looked at yet.
    key: (usize, u64),
    cursor: u64,
    next: EntityId,

    /// The bucket entries each terrain has, and how many belong to terrains gone.
    entries: HashMap<EntityId, usize>,
    live: usize,
    dead: usize,
    buckets: HashMap<(i32, i32), Vec<EntityId>>,
    coarse: HashMap<(i32, i32), Vec<EntityId>>,
    seas: Vec<(EntityId, SeaPlane)>,

    /// The last cursor ray (eye, direction) and what it hit.
    last: Option<([f32; 6], Option<Float3>)>,
}

impl GroundProbe {
    fn refresh(&mut self, scene: &RenderScene) {
        let key = (scene.uid(), scene.terrain_revision());
        let mut cursor = self.cursor;
        let log = scene.structure_since(&mut cursor).filter(|_| key == self.key);
        let follow = log.filter(|l| l.iter().all(|c| !matches!(c, StructureChange::Other)));
        let Some(log) = follow.filter(|_| self.dead <= 65_536 || self.dead <= self.live) else {
            wce_foundation::wce_prof!("coords.ground_refresh");
            self.key = key;
            self.last = None;
            self.entries.clear();
            (self.live, self.dead) = (0, 0);
            self.buckets.clear();
            self.coarse.clear();
            self.seas.clear();
            for e in scene.entities() {
                self.add(scene, e);
            }

            self.next = scene.next_id();
            let _ = scene.structure_since(&mut self.cursor);
            return;
        };

        self.cursor = cursor;
        if log.is_empty() && self.next == scene.next_id() {
            return;
        }

        wce_foundation::wce_prof!("coords.ground_follow");
        self.last = None;
        for c in &log {
            match c {
                StructureChange::Removed(r) => {
                    if r.end - r.start > self.entries.len() as EntityId {
                        let gone: Vec<EntityId> = self.entries.keys().copied().filter(|id| r.contains(id)).collect();
                        gone.into_iter().for_each(|id| self.kill(id));
                    } else {
                        r.clone().for_each(|id| self.kill(id));
                    }

                    self.seas.retain(|(id, _)| !r.contains(id));
                }

                // Another payload: its entries as it is now.
                StructureChange::Payload(id) => {
                    self.kill(*id);
                    self.seas.retain(|(s, _)| s != id);
                    if *id < self.next
                        && let Some(e) = scene.find(*id)
                    {
                        self.add(scene, e);
                    }
                }

                StructureChange::Other => unreachable!("filtered above"),
            }
        }

        // The entities added since (their ids follow; the scene keeps them in id order).
        let all = scene.entities();
        let first = all.partition_point(|e| e.id < self.next);
        for e in &all[first..] {
            self.add(scene, e);
        }

        self.next = scene.next_id();
    }

    /// A drawn terrain's bucket entries, or a sea's plane.
    fn add(&mut self, scene: &RenderScene, e: &wce_scene::render_scene::Entity) {
        if !scene.drawable(e) {
            return;
        }

        let t = match &e.payload {
            Payload::Terrain(t) if t.nx >= 2 && t.nz >= 2 => t,
            Payload::Water(w) if !w.river => {
                self.seas.extend(SeaPlane::of(w).map(|s| (e.id, s)));
                return;
            }

            _ => return,
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

        let (i0, i1, j0, j1) = ((lo[0] / BUCKET).floor(), (hi[0] / BUCKET).floor(), (lo[1] / BUCKET).floor(), (hi[1] / BUCKET).floor());
        let (size, grid) = match ((i1 - i0 + 1.0) as i64) * ((j1 - j0 + 1.0) as i64) > LARGE_BUCKETS {
            true => (COARSE, &mut self.coarse),
            false => (BUCKET, &mut self.buckets),
        };

        let mut n = 0;
        for i in (lo[0] / size).floor() as i32..=(hi[0] / size).floor() as i32 {
            for j in (lo[1] / size).floor() as i32..=(hi[1] / size).floor() as i32 {
                grid.entry((i, j)).or_default().push(e.id);
                n += 1;
            }
        }

        *self.entries.entry(e.id).or_default() += n;
        self.live += n;
    }

    /// A terrain's entries belong to one gone (a lookup no longer finds it).
    fn kill(&mut self, id: EntityId) {
        if let Some(n) = self.entries.remove(&id) {
            self.live -= n;
            self.dead += n;
        }
    }

    /// The highest ground of the terrains at a point.
    fn ground(&self, scene: &RenderScene, x: f32, z: f32) -> Option<f32> {
        let fine = self.buckets.get(&((x / BUCKET).floor() as i32, (z / BUCKET).floor() as i32));
        let coarse = self.coarse.get(&((x / COARSE).floor() as i32, (z / COARSE).floor() as i32));
        fine.into_iter()
            .chain(coarse)
            .flatten()
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
        let sea = self.seas.iter().map(|(_, s)| s).filter(|s| s.covers(x, z)).map(|s| s.y).reduce(f32::max);
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
            .map(|(_, s)| s)
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
pub fn lines(
    cam: &Camera,
    orbit: bool,
    cursor: Option<Float3>,
    game_units: f32,
    map: Option<&MapFrame>,
    battle: Option<&BattleFrame>,
) -> Vec<String> {
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
        let water = Water {
            river,
            verts: vec![v(-10.0, -10.0), v(10.0, -10.0), v(-10.0, 10.0), v(10.0, 10.0)],
            indices: vec![0, 2, 1, 1, 2, 3],
            ..Default::default()
        };
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

    /// A flat terrain of `cells` x `cells` at a height, its corner at (x, z).
    fn terrain(cells: i32, step: f32, height: f32) -> Payload {
        let n = cells + 1;
        Payload::Terrain(wce_scene::render_scene::Terrain {
            nx: n,
            nz: n,
            origin_x: 0.0,
            origin_z: 0.0,
            step,
            heights: std::sync::Arc::new(vec![height; (n * n) as usize]),
            ..Default::default()
        })
    }

    fn at(x: f32, z: f32) -> Float4x4 {
        let mut m = Float4x4::IDENTITY;
        (m.m[0][3], m.m[2][3]) = (x, z);
        m
    }

    /// A probe that follows a streamed scene (chunks added and removed in ranges, payloads replaced, seas) answers as one made afresh.
    #[test]
    fn a_followed_probe_answers_as_a_fresh_one() {
        let mut seed = 0x2545_F491_4F6C_DD1Du64;
        let mut rand = move |n: u32| {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            (seed % u64::from(n)) as u32
        };

        let mut scene = RenderScene::default();
        let layer = scene.add_layer(1, "map");
        let mut chunks: Vec<std::ops::Range<EntityId>> = vec![];
        let mut probe = GroundProbe::default();
        for step in 0..120 {
            match rand(5) {
                // A chunk: small terrains, a large one (the coarse grid's), now and then a sea.
                0 | 1 => {
                    let first = scene.next_id();
                    for _ in 0..1 + rand(6) {
                        let (x, z) = (rand(8000) as f32 - 4000.0, rand(8000) as f32 - 4000.0);
                        let p = if rand(4) == 0 { terrain(64, 64.0, rand(50) as f32) } else { terrain(8, 32.0, rand(50) as f32) };
                        scene.add(layer, at(x, z), p, SourceRef { doc: 1, item: u64::MAX });
                    }

                    if rand(6) == 0 {
                        let y = rand(30) as f32;
                        let v = |x: f32, z: f32| WaterVertex { x, y, z, ..Default::default() };
                        let (x, z) = (rand(6000) as f32 - 3000.0, rand(6000) as f32 - 3000.0);
                        let verts = vec![v(x, z), v(x + 900.0, z), v(x, z + 900.0), v(x + 900.0, z + 900.0)];
                        let water = Water { verts, indices: vec![0, 2, 1, 1, 2, 3], ..Default::default() };
                        scene.add(layer, Float4x4::IDENTITY, Payload::Water(water), SourceRef { doc: 1, item: u64::MAX });
                    }

                    chunks.push(first..scene.next_id());
                }

                // Chunks dropped.
                2 if !chunks.is_empty() => {
                    let gone = chunks.remove(rand(chunks.len() as u32) as usize);
                    let more = if chunks.is_empty() || rand(2) == 0 { vec![] } else { vec![chunks.remove(0)] };
                    scene.remove_ranges(&[vec![gone], more].concat());
                }

                // A payload replaced by another kind (a terrain becomes an overlay, or the other way).
                3 => {
                    if let Some(e) = scene.entities().get(rand(scene.entities().len().max(1) as u32) as usize) {
                        let id = e.id;
                        let p = match e.payload {
                            Payload::Terrain(_) => Payload::Overlay(Default::default()),
                            _ => terrain(8, 32.0, rand(50) as f32),
                        };
                        scene.update_payload(id, p);
                    }
                }

                _ => {}
            }

            // Asked between changes, as the viewport asks every frame.
            let fresh = &mut GroundProbe::default();
            for _ in 0..200 {
                let (x, z) = (rand(12000) as f32 - 6000.0, rand(12000) as f32 - 6000.0);
                assert_eq!(probe.surface_at(&scene, x, z), fresh.surface_at(&scene, x, z), "step {step} at {x}, {z}");
                assert_eq!(probe.ground_at(&scene, x, z), fresh.ground_at(&scene, x, z), "step {step} at {x}, {z}");
            }

            let (eye, dir) = (Float3::new(rand(4000) as f32 - 2000.0, 200.0, 0.0), Float3::new(0.3, -0.2, 0.5));
            assert_eq!(probe.surface_hit(&scene, eye, dir, 5000.0), fresh.surface_hit(&scene, eye, dir, 5000.0), "step {step}");
        }

        // Only a probe that followed the removals keeps their entries (a reading afresh starts without).
        assert!(probe.dead > 0, "the probe followed the log");
    }

    #[test]
    fn rivers_are_not_ground() {
        let scene = scene_with(true);
        assert_eq!(GroundProbe::default().surface_at(&scene, 0.0, 0.0), None);
    }
}
