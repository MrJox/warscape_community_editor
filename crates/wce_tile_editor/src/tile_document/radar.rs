//! What the radar generator (`crate::radar`, docs/plans/radar.md) draws of the tile, in metres: its heights, its water (the sea under
//! 0 m, stage B's rivers and lakes), its roads (the procedural roads' lines, the path and road decals, the blend map's thin layers), its
//! trees, its buildings (their hard outlines, else their collision hulls from above), its walls and its streets and squares (the siege AI
//! the tile exports), its cliffs (the automatic no-go areas).

use rayon::prelude::*;
use wce_siege::NodeKind;
use wce_foundation::math::{Double3, Float3};
use wce_tile::blend_field::BlendField;
use wce_tile::world_transform::world_transform;

use super::Tile;
use crate::radar::{Building, Field, P, RadarInput, Wall, blend_roads};
use crate::terrain_bake::WaterDepth;

/// The heights are sampled every this many metres.
const HEIGHT_CELL_M: f64 = 2.0;

/// The water planes and rivers every this many metres (a narrow stream barely over the ground still shows).
const WATER_CELL_M: f64 = 1.0;

/// The share of the buildings with a red roof.
const RED_ROOFS: f64 = 0.3;

impl Tile {
    /// What the radar draws of the tile as it is in memory: `water` is stage B's water depth when the tool has it (the rivers and lakes).
    pub fn radar_input(&mut self, water: Option<&WaterDepth>) -> RadarInput {
        // The image is the tile (as the hand-made radars: the border is left out); what is sampled covers the border too.
        let bordered = self.minimap_area();
        let [x0, z0, w_m, h_m] = bordered;
        let mut input = RadarInput { area: [0.0, 0.0, self.project.extent_x(), self.project.extent_z()], ..RadarInput::default() };

        // The heights.
        if self.project.has_height() {
            let (w, h) = (((w_m / HEIGHT_CELL_M).ceil() as usize).max(2), ((h_m / HEIGHT_CELL_M).ceil() as usize).max(2));
            let (cx, cz) = (w_m / w as f64, h_m / h as f64);
            let project = &self.project;
            let v: Vec<f32> = (0..w * h).into_par_iter().map(|k| project.height_at(x0 + ((k % w) as f64 + 0.5) * cx, z0 + ((k / w) as f64 + 0.5) * cz) as f32).collect();
            input.sea = v.iter().any(|&h| h < wce_siege::Sea::WATER as f32);
            input.height = Field { x0, z0, w_m, h_m, w, h, v };
        }

        // Rivers and lakes: stage B's depth over the tile without its border.
        if let Some(d) = water.filter(|d| d.w > 1 && d.h > 1) {
            let v = d.depth.iter().map(|&x| if x.is_finite() { x.max(0.0) } else { 0.0 }).collect();
            let (ex, ez) = (self.project.extent_x(), self.project.extent_z());
            // One sample per vertex: the cells' centres are half a step in.
            let (sx, sz) = (ex / (d.w - 1) as f64, ez / (d.h - 1) as f64);
            input.water = Some(Field { x0: -sx * 0.5, z0: -sz * 0.5, w_m: ex + sx, h_m: ez + sz, w: d.w, h: d.h, v });
        }

        // The water planes and the rivers as the tile places them (stage B leaves out a plane barely over the ground): their surface over
        // the ground, with stage B's depth, every `WATER_CELL_M`.
        let mut wm = self.water_map();
        wm.sea = false;
        // The water planes from their models' bounds as the files have them: the viewport's need not have loaded them (a hidden layer,
        // far from the camera), and a plane it has not is missing from the water map.
        wm.planes = self.radar_water_planes();
        input.water_planes = wm.planes.iter().map(|(c, _)| c.to_vec()).collect();
        if !wm.planes.is_empty() || !wm.rivers.is_empty() || input.water.is_some() {
            let (w, h) = (((w_m / WATER_CELL_M).ceil() as usize).max(2), ((h_m / WATER_CELL_M).ceil() as usize).max(2));
            let (cx, cz) = (w_m / w as f64, h_m / h as f64);
            let (project, staged, unit) = (&self.project, input.water.as_ref(), self.unit());
            // The planes' and the rivers' boxes: the water map is asked only inside one (it walks every plane and river segment).
            let mut boxes: Vec<[f64; 4]> = wm.planes.iter().map(|(c, _)| {
                c.iter().fold([f64::MAX, f64::MAX, f64::MIN, f64::MIN], |b, p| [b[0].min(p[0]), b[1].min(p[1]), b[2].max(p[0]), b[3].max(p[1])])
            }).collect();
            boxes.extend(wm.rivers.iter().map(|r| {
                r.iter().fold([f64::MAX, f64::MAX, f64::MIN, f64::MIN], |b, p| [b[0].min(p[0] - p[3]), b[1].min(p[1] - p[3]), b[2].max(p[0] + p[3]), b[3].max(p[1] + p[3])])
            }));
            let v: Vec<f32> = (0..w * h).into_par_iter().map(|k| {
                let (x, z) = (x0 + ((k % w) as f64 + 0.5) * cx, z0 + ((k / w) as f64 + 0.5) * cz);
                let inside = boxes.iter().any(|b| x >= b[0] && x <= b[2] && z >= b[1] && z <= b[3]);
                // The water map's heights are the scene's (metres over the unit).
                let own = if inside { wm.level(x, z).map_or(0.0, |l| (l * unit - project.height_at(x, z)).max(0.0)) as f32 } else { 0.0 };
                own.max(staged.map_or(0.0, |f| f.sample(x, z)))
            }).collect();
            input.water = Some(Field { x0, z0, w_m, h_m, w, h, v });
        }

        // The roads of the blend map (the map in memory, else its file), over the bordered tile.
        let read;
        let blend = match self.project.blend.as_ref() {
            Some(b) => Some(b),
            None => {
                read = BlendField::read(&self.project.dir.join("blend_map.tif")).ok();
                read.as_ref()
            }
        };
        if let Some(b) = blend.filter(|_| self.project.bordered()) {
            input.roads = Some(blend_roads(b.weights(), b.width() as usize, b.height() as usize, bordered));
        }

        // The trees: the shown list's and the static ones.
        let trees = self.tree_list();
        let ez = self.project.extent_z() as f32;
        input.trees = trees.iter().filter(|t| is_tree(&t.model)).map(|t| [f64::from(t.x), f64::from(ez - t.z)]).collect();
        if std::env::var("RADAR_TREE_MODELS").is_ok() {
            let mut m = std::collections::BTreeMap::<&str, (usize, f32)>::new();
            for t in trees.iter() {
                let e = m.entry(t.model.as_str()).or_default();
                e.0 += 1;
                e.1 += t.scale;
            }
            let mut v: Vec<_> = m.into_iter().collect();
            v.sort_by_key(|x| std::cmp::Reverse(x.1.0));
            for (k, (n, s)) in v.iter().take(40) {
                println!("  {n:6} {k} (scale {:.2})", s / *n as f32);
            }
        }
        let sc = self.tscene();
        for e in sc.find_all("VEGETATION_INSTANCE") {
            if self.shown(e) && is_tree(&sc.doc().get(e, "key").unwrap_or_default()) {
                let p = self.world_position(e);
                input.trees.push([f64::from(p.x), f64::from(p.z)]);
            }
        }

        input.road_lines = self.radar_road_lines();

        // The settlement as the siege AI sees it (scene units).
        let u = self.unit();
        let to_m = |pts: &[P]| -> Vec<P> { pts.iter().map(|p| [p[0] / u, p[1] / u]).collect() };
        let siege = self.siege_input_with(false);
        input.buildings = self.radar_buildings();
        input.walls = siege.walls.iter().map(|w| {
            let footprint: Vec<Vec<P>> = w.footprint.iter().map(|r| to_m(r)).collect();
            let (lo, hi) = bounds(&footprint);
            let palisade = siege.palisade.iter().any(|p| {
                let q = [p.at[0] / u, p.at[1] / u];
                q[0] >= lo[0] - 0.5 && q[0] <= hi[0] + 0.5 && q[1] >= lo[1] - 0.5 && q[1] <= hi[1] + 0.5
            });
            Wall { footprint, line: [[w.ends[0][0] / u, w.ends[0][1] / u], [w.ends[1][0] / u, w.ends[1][1] / u]], gate: w.gate, palisade }
        }).collect();

        let layout = self.current_siege_layout();
        for e in &layout.edges {
            if e.kind != wce_siege::EdgeKind::Street || e.line.len() < 2 {
                continue;
            }
            let line = to_m(&e.line);
            let width = match e.sides.as_slice() {
                [a, b, ..] if !a.is_empty() && !b.is_empty() => side_gap(&to_m(a), &to_m(b)),
                _ => 6.0,
            };
            input.streets.push((line, (width * 0.6).clamp(4.0, 9.0)));
        }
        input.plazas = layout.nodes.iter().filter(|n| n.kind == NodeKind::Area && n.boundary.len() > 2).map(|n| to_m(&n.boundary)).collect();

        if let Some(nogos) = &self.auto_nogos {
            input.cliffs = nogos.iter().map(|o| o.iter().map(|p| [f64::from(p[0]), f64::from(p[1])]).collect()).collect();
        }

        input
    }

}

impl Tile {
    /// The buildings as the radar draws them: the hard collision outlines of every exported building that is not a fort piece, those of a
    /// house's size (fences, barrels and posts left out), in metres.
    fn radar_buildings(&self) -> Vec<Building> {
        let sc = self.tscene();
        let u = self.unit();
        let mut out = vec![];
        for e in sc.find_all("BUILDING_INSTANCE") {
            let key = sc.doc().get(e, "key").unwrap_or_default();
            if !super::siege::exported(sc, e) {
                continue;
            }
            let lower = key.to_lowercase();
            if ["barricade", "rock", "stone", "cliff"].iter().any(|k| lower.contains(k)) {
                continue;
            }
            let Some(l) = self.logic_of(&key) else { continue };
            if l.fort || l.gate {
                continue;
            }
            let w = world_transform(sc, e);
            let place = |p: &P| -> P {
                let q = w.position + w.rotation * Double3::new(p[0], 0.0, p[1]);
                [q.x / u, q.z / u]
            };
            let red = {
                let h = (w.position.x * 7.31).floor() as i64 * 73_856_093 ^ (w.position.z * 3.17).floor() as i64 * 19_349_663;
                (h.rem_euclid(1000) as f64) < RED_ROOFS * 1000.0
            };

            // Its hard outlines of a house's size, else its hull from above when that is of a house's size.
            let rings: Vec<Vec<P>> = l.hard.iter().map(|r| r.iter().map(place).collect::<Vec<P>>()).filter(|r| big_enough(std::slice::from_ref(r))).collect();
            let polys = if rings.is_empty() {
                let tris: Vec<Vec<P>> = l.hull.iter().map(|t| t.iter().map(place).collect()).collect();
                if big_enough(&tris) { tris } else { vec![] }
            } else {
                rings
            };
            if !polys.is_empty() {
                out.push(Building { polys, red });
            }
        }
        out
    }
}

impl Tile {
    /// The water planes (`SIMPLE_MESH_INSTANCE`s of a `water_plane` model) as `Tile::water_map` has them (corners in metres, height in the
    /// scene's units), their models' bounds from the loaded model, else from the model file's mesh headers.
    fn radar_water_planes(&self) -> Vec<([[f64; 2]; 4], f64)> {
        let sc = self.tscene();
        let mut read: std::collections::HashMap<String, Option<(Float3, Float3)>> = std::collections::HashMap::new();
        sc.find_all("SIMPLE_MESH_INSTANCE").into_iter().filter_map(|e| {
            let key = sc.doc().get(e, "key")?;
            if !key.to_lowercase().contains("water_plane") {
                return None;
            }
            let (lo, hi) = match self.model_of(e).filter(|m| m.ok && m.resolved) {
                Some(m) => (m.bmin, m.bmax),
                None => (*read.entry(key.clone()).or_insert_with(|| {
                    let path = key.replace('\\', "/").to_lowercase();
                    self.files.as_deref().and_then(|f| f.read(&path).ok()).and_then(|b| rmv2_bounds(&b))
                }))?,
            };
            let w = self.world_matrix(e);
            let corner = |x: f32, z: f32| {
                let p = w.transform_point(Float3::new(x, 0.0, z));
                [f64::from(p.x), f64::from(p.z)]
            };
            let y = f64::from(w.transform_point(Float3::new(0.0, 0.0, 0.0)).y);
            Some(([corner(lo.x, lo.z), corner(hi.x, lo.z), corner(hi.x, hi.z), corner(lo.x, hi.z)], y))
        }).collect()
    }

    /// How many of the tile's models have their bounds (they come in while the renderer loads them): the water planes' footprints are
    /// their models', so a radar drawn before they came is drawn again.
    pub fn radar_models_resolved(&self) -> usize {
        self.models.values().filter(|m| m.resolved).count()
    }

    /// The roads as lines: the procedural roads' centre lines, and every shown road decal (a path, a road, its borders) as a line along
    /// its x, in metres.
    fn radar_road_lines(&self) -> Vec<Vec<P>> {
        let sc = self.tscene();
        let u = self.unit();
        let mut out: Vec<Vec<P>> = (0..sc.doc().elements().len()).filter(|&e| self.is_road(e) && self.shown(e))
            .map(|e| self.road_world_curve(e).iter().map(|p| [f64::from(p.x), f64::from(p.z)]).collect()).collect();
        for e in sc.find_all("ProjectedDecalInstance") {
            if !self.shown(e) || !super::siege::exported(sc, e) {
                continue;
            }
            let key = sc.doc().get(e, "key").unwrap_or_default().to_lowercase();
            let name = key.rsplit(['\\', '/']).next().unwrap_or_default();
            if !ROAD_DECALS.iter().any(|k| name.starts_with(k) || name.contains(&format!("_{k}"))) {
                continue;
            }
            let w = world_transform(sc, e);
            let len = sc.scale(e).map_or(10.0, |s| s.x.abs());
            let d = w.rotation * Double3::new(len, 0.0, 0.0);
            let (c, d) = ([w.position.x / u, w.position.z / u], [d.x / u, d.z / u]);
            out.push(vec![[c[0] - d[0] * 0.5, c[1] - d[1] * 0.5], [c[0] + d[0] * 0.5, c[1] + d[1] * 0.5]]);
        }
        out
    }
}

/// A rigid model's bounds (model space) from its first LOD's mesh headers: each holds its box at 0x18 (min x y z, max x y z).
fn rmv2_bounds(b: &[u8]) -> Option<(Float3, Float3)> {
    let u32_at = |o: usize| b.get(o..o + 4).map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]) as usize);
    let f32_at = |o: usize| b.get(o..o + 4).map(|s| f32::from_le_bytes([s[0], s[1], s[2], s[3]]));
    if b.get(..4) != Some(b"RMV2") || u32_at(8)? == 0 {
        return None;
    }
    let (count_at, offset_at) = if u32_at(4)? < 6 { (0x10c, 0x118) } else { (0x8c, 0x98) };
    let (count, mut at) = (u32_at(count_at)?, u32_at(offset_at)?);
    let (mut lo, mut hi) = (Float3::new(f32::MAX, f32::MAX, f32::MAX), Float3::new(f32::MIN, f32::MIN, f32::MIN));
    for _ in 0..count {
        let v: Vec<f32> = (0..6).map(|k| f32_at(at + 0x18 + 4 * k)).collect::<Option<_>>()?;
        lo = Float3::new(lo.x.min(v[0]), lo.y.min(v[1]), lo.z.min(v[2]));
        hi = Float3::new(hi.x.max(v[3]), hi.y.max(v[4]), hi.z.max(v[5]));
        at += u32_at(at + 4)?.max(1);
    }
    (count > 0 && hi.x >= lo.x && hi.z >= lo.z).then_some((lo, hi))
}

/// Whether a vegetation model or key is a tree (the lists hold shrubs, flowers and stones too: `trees/shrubs/...`,
/// `trees/stones_atlantic/...`, `shrubs.shrub_rosec`).
fn is_tree(model: &str) -> bool {
    let m = model.to_lowercase();
    !NOT_TREES.iter().any(|k| m.contains(k))
}

const NOT_TREES: [&str; 11] = ["shrub", "stone", "rock", "flower", "grass", "bush", "reed", "fern", "calamus", "bulrush", "drya_"];

/// The decals that are roads (their names' starts, or after an underscore: `global_terrain_path_01`).
const ROAD_DECALS: [&str; 2] = ["path_", "road_"];

/// Whether a footprint is a house's size: at least `MIN_SIDE_M` across its box both ways and `MIN_AREA_M2` in its box.
fn big_enough(rings: &[Vec<P>]) -> bool {
    let (lo, hi) = bounds(rings);
    let (a, b) = (hi[0] - lo[0], hi[1] - lo[1]);
    a.min(b) >= MIN_SIDE_M && a * b >= MIN_AREA_M2
}

/// The least a building is across, and its least area, to be drawn.
const MIN_SIDE_M: f64 = 2.5;
const MIN_AREA_M2: f64 = 10.0;

fn bounds(rings: &[Vec<P>]) -> (P, P) {
    rings.iter().flatten().fold(([f64::MAX; 2], [f64::MIN; 2]), |(lo, hi), p| ([lo[0].min(p[0]), lo[1].min(p[1])], [hi[0].max(p[0]), hi[1].max(p[1])]))
}

/// How far apart two sides of a street are: the median of each of `a`'s points' distance to `b`.
fn side_gap(a: &[P], b: &[P]) -> f64 {
    let mut d: Vec<f64> = a.iter().map(|p| {
        b.windows(2).map(|s| seg_dist(*p, s[0], s[1])).fold(f64::MAX, f64::min)
    }).filter(|d| d.is_finite() && *d < 1e6).collect();
    if d.is_empty() {
        return 6.0;
    }
    d.sort_by(f64::total_cmp);
    d[d.len() / 2]
}

fn seg_dist(p: P, a: P, b: P) -> f64 {
    let d = [b[0] - a[0], b[1] - a[1]];
    let l2 = d[0] * d[0] + d[1] * d[1];
    let t = if l2 > 0.0 { (((p[0] - a[0]) * d[0] + (p[1] - a[1]) * d[1]) / l2).clamp(0.0, 1.0) } else { 0.0 };
    ((p[0] - a[0] - t * d[0]).powi(2) + (p[1] - a[1] - t * d[1]).powi(2)).sqrt()
}
