//! The platform check (the Tile panel's Problems, *Check platforms*): the tile's bridges and fort walls as the game's nav graph would link
//! them, from the rules traced in `docs/re/platforms_and_bridges.md`, without launching the game.
//!
//! **Walls** (`check_walls`): the walkways of the fort's buildings (fort walls, towers, gates: the categories with "fort", "wall", "gate" or
//! "tower" in their name) and of the procedural walls join where two of their platforms have an edge with the same two corners, run the
//! other way (the joint rule below). Two buildings whose platforms come close along an edge (facing each other within [`WALL_NEAR`] across,
//! [`WALL_RISE`] in height, along at least [`WALL_OVERLAP`]) but join nowhere are a break in the wall walk: units on the walls stop there.
//!
//! A bridge is the platforms of the buildings whose `battlefield_buildings` category is `bridge`, and of the procedural roads' generated
//! bridges, joined to each other. In the game:
//! - a platform edge links to the **ground** when it runs between two consecutive corners of a plain collision outline (named with neither
//!   "outline" nor "gate"; corners within [`CORNER`] in x / z) and the ground at both corners is within the platform's snap tolerance of it
//!   (1.0 for bridges and walls, 0.1 otherwise);
//! - two platforms **join** along an edge with the same two corners, run the other way (within [`CORNER`]; heights within [`JOINT_HEIGHT`]);
//! - a ground platform (`flag_2`, or any platform of a `ground_platform` building; green in the tech overlay) is no platform at all: units
//!   walk the ground there, and the ground there is the platform (a kit bridge's ramps end on such platforms, well above the terrain: the
//!   vanilla river crossings' `bridge_wood_complete` is ~2.3 game units over it). This is an inference from the kit's working bridges
//!   (user, 2026-11-11), not traced.
//!
//! Each bridge's ways off it are its ground links and its joints to other buildings' platforms (a gatehouse, a wall walk). Grouped by
//! where they are (closer than a third of the bridge's length, at least [`MIN_GAP`]), a crossable bridge has at least two groups: one
//! group is a bridge that reaches one bank only, none one that units cannot get onto.
//!
//! ASSUMPTIONS: away from ground platforms, the ground is the editor's heights (the game's are BOB's final heights: the same away from the tile's edges); a platform's
//! corners are taken at their file heights (the game takes them on the plane through the first with the file's normal: the same for the
//! kit's and the generated pieces, `bridge_check`'s E3); a ground edge along an outline edge exists unless something else cuts it.

use std::collections::{HashMap, VecDeque};

use wce_foundation::math::Float3;

use super::Tile;

/// The most two corners may differ in x / z to be the same (squared 2.0e-4 in the game).
const CORNER: f32 = 0.01414;

/// The most a joint's two edges may differ in height.
const JOINT_HEIGHT: f32 = 0.1;

/// The ground snap tolerances: bridges and walls, everything else.
const SNAP_BRIDGE: f32 = 1.0;
const SNAP_OTHER: f32 = 0.1;

/// Wall walks: how far across (metres) and in height two facing platform edges may be to be meant to join, and how long they must face each
/// other.
const WALL_NEAR: f32 = 0.75;
const WALL_RISE: f32 = 1.0;
const WALL_OVERLAP: f32 = 0.5;

/// Facing edges closer than this (and within [`JOINT_HEIGHT`] in height) are not reported: the vanilla tiles' walls, which work in the game,
/// have a few 1 to 2 cm off (western_city_rome/medium: 2), within what the saved positions round to.
const WALL_FINE: f32 = 0.03;

/// Ways off a bridge closer than this (metres) are on the same side whatever the bridge's length.
const MIN_GAP: f32 = 4.0;

/// A platform sloped more than this (degrees) is a ramp: its edges at its lowest height are a foot, which should reach the ground.
const RAMP_DEGREES: f32 = 15.0;

/// What a segment of the check's overlay shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BridgeEdge {
    /// A platform edge the ground links to.
    Ground,

    /// A joint between two of the bridge's platforms.
    Joint,

    /// A joint to another building's platform: a way off the bridge.
    Exit,

    /// A ramp's foot the ground does not link to.
    LooseFoot,

    /// Two wall walks' platforms joined.
    WallJoint,

    /// Two wall walks' platforms that face each other but do not join: a break in the wall walk.
    WallGap,
}

/// A bridge that units cannot cross, or a break in a wall walk.
#[derive(Clone, Debug)]
pub struct BridgeProblem {
    /// The elements its pieces belong to (buildings, roads).
    pub elems: Vec<usize>,
    pub message: String,

    /// Its middle (metres).
    pub at: Float3,
}

/// What the check found.
#[derive(Clone, Debug, Default)]
pub struct BridgeReport {
    /// The bridges (platforms joined together) and the bad ones; the wall walks' buildings and their joints; the problems of both.
    pub bridges: usize,
    pub walls: usize,
    pub wall_joints: usize,
    pub problems: Vec<BridgeProblem>,

    /// What to draw: the bridges' ground links, joints, exits and loose feet.
    pub edges: Vec<(Float3, Float3, BridgeEdge)>,

    /// Bridge buildings without a logic file (not checked).
    pub unread: Vec<String>,

    /// Each bridge in a line: its pieces' keys, its platforms, its ways off and the sides they are on.
    pub summary: Vec<String>,
}

/// A platform of the check.
struct Platform {
    /// Its element (a building, a wall, a road) and its key.
    elem: usize,
    key: String,
    corners: Vec<Float3>,
    bridge: bool,
    snap: f32,
}

/// A cell of the corner grids.
fn cell(p: Float3) -> (i64, i64) {
    ((p.x / (CORNER * 2.0)).floor() as i64, (p.z / (CORNER * 2.0)).floor() as i64)
}

fn near(a: Float3, b: Float3) -> bool {
    (a.x - b.x).powi(2) + (a.z - b.z).powi(2) <= CORNER * CORNER
}

/// The keys of a point's cell and its neighbours.
fn around(p: Float3) -> impl Iterator<Item = (i64, i64)> {
    let (x, z) = cell(p);
    (-1..=1).flat_map(move |dx| (-1..=1).map(move |dz| (x + dx, z + dz)))
}

/// A platform's slope (degrees): of the plane through its corners (Newell's normal).
fn slope(r: &[Float3]) -> f32 {
    let mut n = Float3::new(0.0, 0.0, 0.0);
    for i in 0..r.len() {
        let (a, b) = (r[i], r[(i + 1) % r.len()]);
        n.x += (a.y - b.y) * (a.z + b.z);
        n.y += (a.z - b.z) * (a.x + b.x);
        n.z += (a.x - b.x) * (a.y + b.y);
    }

    let len = (n.x * n.x + n.y * n.y + n.z * n.z).sqrt();
    if len <= 0.0 { 0.0 } else { (n.y.abs() / len).clamp(-1.0, 1.0).acos().to_degrees() }
}

fn edges(r: &[Float3]) -> impl Iterator<Item = (usize, Float3, Float3)> + '_ {
    (0..r.len()).map(move |i| (i, r[i], r[(i + 1) % r.len()]))
}

/// The surfaces gathered for the check.
#[derive(Default)]
struct Gathered {
    platforms: Vec<Platform>,
    outlines: Vec<[Float3; 2]>,

    /// The ground platforms (`flag_2`, or a `ground_platform` building's): the ground units walk there.
    grounds: Vec<Vec<Float3>>,
    unread: Vec<String>,
}

impl Gathered {
    /// A building's surfaces (`bridge`: one of the bridges; `snap`: its ground snap tolerance; `all_ground`: every platform of it is ground).
    fn add(&mut self, t: &mut Tile, e: usize, bridge: bool, snap: f32, all_ground: bool) {
        let surfaces = t.walk_surfaces(e);
        if surfaces.is_empty() && bridge {
            self.unread.push(t.tscene().doc().get(e, "key").unwrap_or_default());
        }

        for s in surfaces {
            for o in &s.outlines {
                self.outlines.extend(edges(o).map(|(_, a, b)| [a, b]));
            }

            for (corners, ground) in s.platforms {
                if corners.len() < 3 {
                    continue;
                }

                if ground || all_ground {
                    self.grounds.push(corners);
                } else {
                    self.platforms.push(Platform { elem: e, key: s.key.clone(), corners, bridge, snap });
                }
            }
        }
    }
}

/// The height of the highest ground platform at a point (on it, or on its edge within [`CORNER`]), on the plane through its corners.
fn ground_height(grounds: &[Vec<Float3>], p: Float3) -> Option<f32> {
    grounds.iter().filter(|r| {
        let ring = r.iter().map(|q| [q.x, q.z]).collect::<Vec<_>>();
        inside([p.x, p.z], &ring) || (0..ring.len()).any(|i| seg_dist([p.x, p.z], ring[i], ring[(i + 1) % ring.len()]) <= CORNER)
    }).map(|r| plane_height(r, p.x, p.z)).reduce(f32::max)
}

/// The height at (x, z) of the plane through a ring's corners (Newell's normal through their mean).
fn plane_height(r: &[Float3], x: f32, z: f32) -> f32 {
    let mut n = Float3::new(0.0, 0.0, 0.0);
    for i in 0..r.len() {
        let (a, b) = (r[i], r[(i + 1) % r.len()]);
        n.x += (a.y - b.y) * (a.z + b.z);
        n.y += (a.z - b.z) * (a.x + b.x);
        n.z += (a.x - b.x) * (a.y + b.y);
    }

    let k = 1.0 / r.len() as f32;
    let m = r.iter().fold(Float3::new(0.0, 0.0, 0.0), |s, &q| s + q) * k;
    if n.y.abs() < 1e-6 { m.y } else { m.y - (n.x * (x - m.x) + n.z * (z - m.z)) / n.y }
}

fn inside(p: [f32; 2], ring: &[[f32; 2]]) -> bool {
    let mut c = false;
    let mut j = ring.len() - 1;
    for i in 0..ring.len() {
        let (a, b) = (ring[i], ring[j]);
        if (a[1] > p[1]) != (b[1] > p[1]) && p[0] < (b[0] - a[0]) * (p[1] - a[1]) / (b[1] - a[1]) + a[0] {
            c = !c;
        }

        j = i;
    }

    c
}

fn seg_dist(p: [f32; 2], a: [f32; 2], b: [f32; 2]) -> f32 {
    let (dx, dz) = (b[0] - a[0], b[1] - a[1]);
    let len = dx * dx + dz * dz;
    let t = if len > 0.0 { (((p[0] - a[0]) * dx + (p[1] - a[1]) * dz) / len).clamp(0.0, 1.0) } else { 0.0 };
    (p[0] - a[0] - t * dx).hypot(p[1] - a[1] - t * dz)
}

impl Tile {
    /// The tile's bridges checked against the game's linking rules (the module's doc). Reads the logic files of the bridges and of what is
    /// around them.
    pub fn check_bridges(&mut self) -> BridgeReport {
        let mut report = BridgeReport::default();

        // The bridges: buildings of the category, and the roads' generated bridges.
        let buildings = self.instances.iter().copied().filter(|&e| self.tscene().type_of(e) == "BUILDING_INSTANCE").collect::<Vec<_>>();
        let category = |t: &Self, e: usize| t.tscene().doc().get(e, "key").and_then(|k| t.building_category(&k.to_lowercase())).unwrap_or_default();
        let bridges = buildings.iter().copied().filter(|&e| category(self, e) == "bridge").collect::<Vec<_>>();
        let roads = self.roads_with_buildings();
        let mut g = Gathered::default();
        for &e in bridges.iter().chain(&roads) {
            g.add(self, e, true, SNAP_BRIDGE, false);
        }

        report.unread = std::mem::take(&mut g.unread);
        if g.platforms.is_empty() {
            return report;
        }

        // What is around them: the other buildings and the walls near enough to join them, or to cut the ground beside them.
        let (mut lo, mut hi) = ([f32::MAX; 2], [f32::MIN; 2]);
        for c in g.platforms.iter().flat_map(|p| &p.corners) {
            (lo, hi) = ([lo[0].min(c.x), lo[1].min(c.z)], [hi[0].max(c.x), hi[1].max(c.z)]);
        }

        let reach = 64.0;
        let close = |p: Float3| p.x >= lo[0] - reach && p.x <= hi[0] + reach && p.z >= lo[1] - reach && p.z <= hi[1] + reach;
        let others = buildings.iter().copied().filter(|e| !bridges.contains(e)).filter(|&e| close(self.world_position(e))).collect::<Vec<_>>();
        for e in others {
            let c = category(self, e);
            let snap = if matches!(c.as_str(), "fort_wall" | "unbreachable_fort_wall") { SNAP_BRIDGE } else { SNAP_OTHER };
            g.add(self, e, false, snap, c == "ground_platform");
        }

        let walls = self.wall_preview.shown_walls();
        let walls = walls.into_iter().filter(|&e| self.generated_buildings(e).iter().any(|(w, _)| close(w.transform_point(Float3::new(0.0, 0.0, 0.0))))).collect::<Vec<_>>();
        for e in walls {
            g.add(self, e, false, SNAP_BRIDGE, false);
        }

        let Gathered { platforms, outlines, grounds, .. } = g;

        // The outlines' edges and the platforms' edges by their first corner's cell.
        let mut outline_grid: HashMap<(i64, i64), Vec<usize>> = HashMap::new();
        for (i, [a, b]) in outlines.iter().enumerate() {
            outline_grid.entry(cell(*a)).or_default().push(i);
            outline_grid.entry(cell(*b)).or_default().push(i);
        }

        let mut edge_grid: HashMap<(i64, i64), Vec<(usize, usize)>> = HashMap::new();
        for (k, p) in platforms.iter().enumerate() {
            for (i, a, _) in edges(&p.corners) {
                edge_grid.entry(cell(a)).or_default().push((k, i));
            }
        }

        let on_outline = |a: Float3, b: Float3| {
            around(a).filter_map(|c| outline_grid.get(&c)).flatten().any(|&i| {
                let [p, q] = outlines[i];
                (near(p, a) && near(q, b)) || (near(p, b) && near(q, a))
            })
        };

        // Each platform's ground links and joints.
        // The ground: a ground platform where one is (units walk it as the ground), else the terrain.
        let ground_at = |t: &Self, p: Float3| ground_height(&grounds, p).unwrap_or_else(|| t.terrain_height(p.x, p.z));
        let mut joints: Vec<Vec<(usize, usize)>> = vec![vec![]; platforms.len()];
        let mut ground: Vec<Vec<usize>> = vec![vec![]; platforms.len()];
        for (k, p) in platforms.iter().enumerate() {
            for (i, a, b) in edges(&p.corners) {
                let links = on_outline(a, b) && (ground_at(self, a) - a.y).abs() <= p.snap && (ground_at(self, b) - b.y).abs() <= p.snap;
                if links {
                    ground[k].push(i);
                }

                // The other platforms with the edge b -> a.
                for &(m, j) in around(b).filter_map(|c| edge_grid.get(&c)).flatten() {
                    if m == k {
                        continue;
                    }

                    let q = &platforms[m].corners;
                    let (c, d) = (q[j], q[(j + 1) % q.len()]);
                    if near(c, b) && near(d, a) && (c.y - b.y).abs() <= JOINT_HEIGHT && (d.y - a.y).abs() <= JOINT_HEIGHT && !joints[k].iter().any(|&(o, _)| o == m) {
                        joints[k].push((m, i));
                    }
                }
            }
        }

        // The bridges: their platforms joined together, from each bridge platform; another building's platform is a way off.
        let mut seen = vec![false; platforms.len()];
        for start in 0..platforms.len() {
            if !platforms[start].bridge || seen[start] {
                continue;
            }

            let (mut members, mut exits) = (vec![], vec![]);
            let mut queue = VecDeque::from([start]);
            seen[start] = true;
            while let Some(k) = queue.pop_front() {
                members.push(k);
                let c = &platforms[k].corners;
                for &(m, i) in &joints[k] {
                    let (a, b) = (c[i], c[(i + 1) % c.len()]);
                    if platforms[m].bridge {
                        report.edges.push((a, b, BridgeEdge::Joint));
                        if !seen[m] {
                            seen[m] = true;
                            queue.push_back(m);
                        }
                    } else {
                        report.edges.push((a, b, BridgeEdge::Exit));
                        exits.push((a + b) * 0.5);
                    }
                }
            }

            // Its ways off: the ground links and the exits; its loose feet.
            for &k in &members {
                let c = &platforms[k].corners;
                for &i in &ground[k] {
                    let (a, b) = (c[i], c[(i + 1) % c.len()]);
                    report.edges.push((a, b, BridgeEdge::Ground));
                    exits.push((a + b) * 0.5);
                }

                if slope(c) > RAMP_DEGREES {
                    let low = c.iter().map(|q| q.y).fold(f32::MAX, f32::min);
                    for (i, a, b) in edges(c) {
                        if (a.y - low).abs() < 0.05 && (b.y - low).abs() < 0.05 && !ground[k].contains(&i) {
                            report.edges.push((a, b, BridgeEdge::LooseFoot));
                        }
                    }
                }
            }

            report.bridges += 1;
            let groups = sides(&members.iter().flat_map(|&k| platforms[k].corners.iter().copied()).collect::<Vec<_>>(), &exits);
            let mut names = members.iter().map(|&k| platforms[k].key.clone()).collect::<Vec<_>>();
            names.sort();
            names.dedup();
            report.summary.push(format!("{}: {} platform(s), {} way(s) off on {groups} side(s)", names.join(" + "), members.len(), exits.len()));
            if groups >= 2 {
                continue;
            }

            let mut elems = members.iter().map(|&k| platforms[k].elem).collect::<Vec<_>>();
            elems.sort_unstable();
            elems.dedup();
            let mut keys = members.iter().map(|&k| platforms[k].key.clone()).collect::<Vec<_>>();
            keys.sort();
            keys.dedup();
            let all = members.iter().flat_map(|&k| platforms[k].corners.iter().copied()).collect::<Vec<_>>();
            let at = all.iter().fold(Float3::new(0.0, 0.0, 0.0), |s, &p| s + p) * (1.0 / all.len().max(1) as f32);
            let what = if groups == 0 { "units cannot get onto it: no platform edge links to the ground or to another building" } else { "it reaches the ground on one side only: units can get on but not across" };
            let loose = members.iter().any(|&k| slope(&platforms[k].corners) > RAMP_DEGREES && ground[k].is_empty());
            let hint = if loose { " (a ramp's foot does not run along its collision outline's edge, or the ground there, a green ground platform where there is one, is more than 1 m off)" } else { "" };
            report.problems.push(BridgeProblem { elems, message: format!("bridge {}: {what}{hint}", keys.join(", ")), at });
        }

        report
    }
}

impl Tile {
    /// The tile's bridges and wall walks checked (`check_bridges`, `check_walls`), in one report.
    pub fn check_platforms(&mut self) -> BridgeReport {
        let mut report = self.check_bridges();
        let walls = self.check_walls();
        report.walls = walls.walls;
        report.wall_joints = walls.wall_joints;
        report.problems.extend(walls.problems);
        report.edges.extend(walls.edges);
        report.unread.extend(walls.unread);
        report.summary.extend(walls.summary);
        report
    }

    /// The fort's wall walks checked: the pairs of buildings whose platforms face each other but join nowhere (the module's doc).
    pub fn check_walls(&mut self) -> BridgeReport {
        let mut report = BridgeReport::default();
        let fort = |c: &str| c.contains("fort") || c.contains("wall") || c.contains("gate") || c.contains("tower");
        let buildings = self.instances.iter().copied().filter(|&e| self.tscene().type_of(e) == "BUILDING_INSTANCE").collect::<Vec<_>>();
        let mut g = Gathered::default();
        for e in buildings {
            let c = self.tscene().doc().get(e, "key").and_then(|k| self.building_category(&k.to_lowercase())).unwrap_or_default();
            if fort(&c) {
                g.add(self, e, false, SNAP_BRIDGE, false);
                report.walls += 1;
            }
        }

        for e in self.wall_preview.shown_walls() {
            g.add(self, e, false, SNAP_BRIDGE, false);
            report.walls += 1;
        }

        let platforms = g.platforms;
        report.unread = g.unread;
        let grounds = g.grounds.len();

        // The platforms' edges, by the cells of a coarse grid (WALL_NEAR) their corners are in.
        let coarse = |p: Float3| ((p.x / WALL_NEAR).floor() as i64, (p.z / WALL_NEAR).floor() as i64);
        let mut grid: HashMap<(i64, i64), Vec<(usize, usize)>> = HashMap::new();
        for (k, p) in platforms.iter().enumerate() {
            for (i, a, b) in edges(&p.corners) {
                let (ca, cb) = (coarse(a), coarse(b));
                for c in [ca, cb] {
                    grid.entry(c).or_default().push((k, i));
                }
            }
        }

        // Per pair of elements: their joints, and their closest facing edges that do not join.
        let mut joined: HashMap<(usize, usize), usize> = HashMap::new();
        let mut gaps: HashMap<(usize, usize), (f32, f32, Float3, Float3, Float3, Float3)> = HashMap::new();
        for (k, p) in platforms.iter().enumerate() {
            for (_, a, b) in edges(&p.corners) {
                let (ca, cb) = (coarse(a), coarse(b));
                let mut seen = vec![];
                for (dx, dz) in (-1..=1).flat_map(|x| (-1..=1).map(move |z| (x, z))) {
                    for c in [(ca.0 + dx, ca.1 + dz), (cb.0 + dx, cb.1 + dz)] {
                        for &(m, j) in grid.get(&c).into_iter().flatten() {
                            if m <= k || platforms[m].elem == p.elem || seen.contains(&(m, j)) {
                                continue;
                            }

                            seen.push((m, j));
                            let q = &platforms[m].corners;
                            let (c0, d0) = (q[j], q[(j + 1) % q.len()]);
                            let pair = (p.elem.min(platforms[m].elem), p.elem.max(platforms[m].elem));
                            if near(c0, b) && near(d0, a) && (c0.y - b.y).abs() <= JOINT_HEIGHT && (d0.y - a.y).abs() <= JOINT_HEIGHT {
                                *joined.entry(pair).or_default() += 1;
                                report.edges.push((a, b, BridgeEdge::WallJoint));
                                continue;
                            }

                            if let Some((across, rise)) = facing(a, b, c0, d0).filter(|&(x, h)| x >= WALL_FINE || h > JOINT_HEIGHT) {
                                let worst = gaps.get(&pair).map_or(f32::MAX, |g| g.0);
                                if across < worst {
                                    gaps.insert(pair, (across, rise, a, b, c0, d0));
                                }
                            }
                        }
                    }
                }
            }
        }

        report.wall_joints = joined.values().sum();
        let key = |e: usize| self.tscene().doc().get(e, "key").unwrap_or_else(|| "procedural wall".to_owned());
        let mut found = gaps.into_iter().filter(|(pair, _)| !joined.contains_key(pair)).collect::<Vec<_>>();
        found.sort_by(|a, b| a.0.cmp(&b.0));
        for ((ea, eb), (across, rise, a, b, c, d)) in found {
            report.edges.push((a, b, BridgeEdge::WallGap));
            report.edges.push((c, d, BridgeEdge::WallGap));
            let at = (a + b + c + d) * 0.25;
            report.problems.push(BridgeProblem {
                elems: vec![ea, eb],
                message: format!(
                    "walls {} and {}: their walkways meet but do not join ({across:.2} m apart, {rise:.2} m in height; the game joins them within {CORNER} m and {JOINT_HEIGHT} m, corner for corner)",
                    key(ea),
                    key(eb)
                ),
                at,
            });
        }

        report.summary.push(format!("wall walks: {} building(s), {} platform(s) ({grounds} ground), {} joint(s), {} break(s)", report.walls, platforms.len(), report.wall_joints, report.problems.len()));
        report
    }
}

/// Two platform edges that face each other: anti-parallel within 25 degrees, along each other for at least [`WALL_OVERLAP`], at most
/// [`WALL_NEAR`] apart across and [`WALL_RISE`] in height. How far apart they are (the larger of the two ends' distances to the other edge)
/// and their height difference there.
fn facing(a: Float3, b: Float3, c: Float3, d: Float3) -> Option<(f32, f32)> {
    let (u, v) = ([b.x - a.x, b.z - a.z], [d.x - c.x, d.z - c.z]);
    let (lu, lv) = (u[0].hypot(u[1]), v[0].hypot(v[1]));
    if lu < 1e-3 || lv < 1e-3 {
        return None;
    }

    let (u, v) = ([u[0] / lu, u[1] / lu], [v[0] / lv, v[1] / lv]);
    if u[0] * v[0] + u[1] * v[1] > -(25.0f32.to_radians().cos()) {
        return None;
    }

    // The overlap along a -> b of c .. d's projection.
    let along = |p: Float3| (p.x - a.x) * u[0] + (p.z - a.z) * u[1];
    let (t0, t1) = (along(c).min(along(d)).max(0.0), along(c).max(along(d)).min(lu));
    if t1 - t0 < WALL_OVERLAP {
        return None;
    }

    // Across: the overlap's ends of the other edge to this one's line; height: on each edge at the overlap's middle.
    let across = |p: Float3| ((p.x - a.x) * -u[1] + (p.z - a.z) * u[0]).abs();
    let at = |t: f32| Float3::new(a.x + u[0] * t, 0.0, a.z + u[1] * t);
    let on_cd = |p: Float3| {
        let t = ((p.x - c.x) * v[0] + (p.z - c.z) * v[1]).clamp(0.0, lv) / lv;
        c + (d - c) * t
    };
    let dist = across(on_cd(at(t0))).max(across(on_cd(at(t1))));
    let mid = (t0 + t1) * 0.5;
    let h_ab = a.y + (b.y - a.y) * (mid / lu);
    let rise = (on_cd(at(mid)).y - h_ab).abs();
    (dist <= WALL_NEAR && rise <= WALL_RISE).then_some((dist, rise))
}

/// How many sides a bridge's ways off it are on: grouped when closer than a third of its length (its corners' longest extent along their
/// main direction), at least `MIN_GAP`.
fn sides(corners: &[Float3], exits: &[Float3]) -> usize {
    if exits.is_empty() {
        return 0;
    }

    // The main direction of the corners on the ground.
    let n = corners.len().max(1) as f32;
    let (mx, mz) = (corners.iter().map(|p| p.x).sum::<f32>() / n, corners.iter().map(|p| p.z).sum::<f32>() / n);
    let (mut sxx, mut sxz, mut szz) = (0.0, 0.0, 0.0);
    for p in corners {
        let (dx, dz) = (p.x - mx, p.z - mz);
        (sxx, sxz, szz) = (sxx + dx * dx, sxz + dx * dz, szz + dz * dz);
    }

    let angle = 0.5 * (2.0 * sxz).atan2(sxx - szz);
    let (c, s) = (angle.cos(), angle.sin());
    let along = |p: &Float3| (p.x - mx) * c + (p.z - mz) * s;
    let (lo, hi) = corners.iter().map(along).fold((f32::MAX, f32::MIN), |(a, b), v| (a.min(v), b.max(v)));
    let gap = ((hi - lo) / 3.0).max(MIN_GAP);

    // Single linkage.
    let mut group = (0..exits.len()).collect::<Vec<_>>();
    fn root(g: &mut [usize], i: usize) -> usize {
        let mut r = i;
        while g[r] != r {
            r = g[r];
        }

        g[i] = r;
        r
    }

    for i in 0..exits.len() {
        for j in i + 1..exits.len() {
            let (a, b) = (exits[i], exits[j]);
            if (a.x - b.x).hypot(a.z - b.z) < gap {
                let (ri, rj) = (root(&mut group, i), root(&mut group, j));
                group[ri] = rj;
            }
        }
    }

    (0..exits.len()).filter(|&i| root(&mut group, i) == i).count()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ways_off_a_bridge_count_by_side() {
        let deck = [Float3::new(0.0, 1.0, -2.0), Float3::new(30.0, 1.0, -2.0), Float3::new(30.0, 1.0, 2.0), Float3::new(0.0, 1.0, 2.0)];
        assert_eq!(sides(&deck, &[]), 0);
        assert_eq!(sides(&deck, &[Float3::new(0.0, 0.0, 0.0), Float3::new(1.0, 0.0, 1.0)]), 1, "both at one end");
        assert_eq!(sides(&deck, &[Float3::new(0.0, 0.0, 0.0), Float3::new(30.0, 0.0, 0.0)]), 2, "one at each end");
    }

    #[test]
    fn facing_edges_of_wall_walks() {
        let a = Float3::new(0.0, 5.0, 0.0);
        let b = Float3::new(0.0, 5.0, 4.0);
        // The same edge run the other way, 0.3 m across: facing (it would join if it were within 0.014).
        let f = facing(a, b, Float3::new(0.3, 5.0, 4.0), Float3::new(0.3, 5.0, 0.0)).expect("facing");
        assert!((f.0 - 0.3).abs() < 1e-4 && f.1 < 1e-4, "{f:?}");
        // Run the same way: not facing; too far; too high.
        assert!(facing(a, b, Float3::new(0.3, 5.0, 0.0), Float3::new(0.3, 5.0, 4.0)).is_none());
        assert!(facing(a, b, Float3::new(2.0, 5.0, 4.0), Float3::new(2.0, 5.0, 0.0)).is_none());
        assert!(facing(a, b, Float3::new(0.3, 7.0, 4.0), Float3::new(0.3, 7.0, 0.0)).is_none());
    }

    #[test]
    fn a_ramp_is_sloped_and_a_deck_is_not() {
        let deck = [Float3::new(0.0, 1.0, 0.0), Float3::new(10.0, 1.0, 0.0), Float3::new(10.0, 1.0, 2.0), Float3::new(0.0, 1.0, 2.0)];
        let ramp = [Float3::new(-5.0, 0.0, -2.0), Float3::new(-5.0, 1.0, 0.0), Float3::new(5.0, 1.0, 0.0), Float3::new(5.0, 0.0, -2.0)];
        assert!(slope(&deck) < 1.0);
        assert!(slope(&ramp) > RAMP_DEGREES, "{}", slope(&ramp));
    }
}
