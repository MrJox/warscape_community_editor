//! A road's way over a tile (docs/plans/river_bridges.md, step 5): a search over a grid of the tile from one place to another, its cost the
//! length, the grade (steep ground gone round), the turns, the water (crossed straight and as few times as it can) and a seeded noise (a
//! semi-random way); then the way simplified into a road's few points.

use std::cmp::Ordering;
use std::collections::BinaryHeap;

use crate::river_tile_gen::{corner, fbm, nearest_on_line};

/// The search's settings.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RouteSettings {
    /// The grid's cell (metres).
    pub cell: f64,

    /// The grade a road takes without trouble (rise over run); over it a step costs [`STEEP`] times its length.
    pub max_grade: f64,

    /// How much the grade (as a share of `max_grade`, squared) adds to a metre.
    pub grade_weight: f64,

    /// What a turn of 45 degrees costs (metres; two of them in one go four times that).
    pub turn: f64,

    /// How much the noise (0 .. 1) adds to a metre: the way's wandering.
    pub noise_weight: f64,

    /// What a metre in the water costs, and going into it (metres): the river crossed once, the short way.
    pub water_metre: f64,
    pub water_entry: f64,

    /// How close to the tile's edges the way may come (metres), away from its ends.
    pub margin: f64,

    /// How far a simplified way strays from the search's (metres).
    pub tolerance: f64,

    /// Near the edges (within `edge_reach` metres, away from its ends) the way costs up to `edge_weight` more a metre: it does not run
    /// along them (the relief fades flat there).
    pub edge_reach: f64,
    pub edge_weight: f64,
}

impl Default for RouteSettings {
    fn default() -> Self {
        Self {
            cell: 4.0,
            max_grade: 0.22,
            grade_weight: 4.0,
            turn: 6.0,
            noise_weight: 1.2,
            water_metre: 6.0,
            water_entry: 400.0,
            margin: 10.0,
            tolerance: 2.5,
            edge_reach: 48.0,
            edge_weight: 2.0,
        }
    }
}

/// A step's cost over the grade the road takes without trouble, per metre.
const STEEP: f64 = 60.0;

/// The eight ways out of a cell, in turn order.
const DIRS: [(i64, i64); 8] = [(1, 0), (1, 1), (0, 1), (-1, 1), (-1, 0), (-1, -1), (0, -1), (1, -1)];

/// The salt of the way's noise.
const ROUTE_SALT: u64 = 0x0a0d_5eed_0000_0003;

#[derive(PartialEq)]
struct Open {
    f: f64,
    state: usize,
}

impl Eq for Open {}

impl Ord for Open {
    fn cmp(&self, other: &Self) -> Ordering {
        other.f.total_cmp(&self.f)
    }
}

impl PartialOrd for Open {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// The way from `from` (going `inward` there, a unit direction) to `to` over a tile of `extent` metres: `height` the ground at a place,
/// `water` whether a place is under a river's water. The way's points (x, z), from `from` to `to`, before simplifying: a point per grid cell
/// crossed. None when no way gets there.
#[must_use]
pub fn route(
    from: [f64; 2],
    inward: [f64; 2],
    to: [f64; 2],
    extent: (f64, f64),
    height: &dyn Fn(f64, f64) -> f64,
    water: &dyn Fn(f64, f64) -> bool,
    seed: u64,
    s: &RouteSettings,
) -> Option<Vec<[f64; 2]>> {
    let (nx, nz) = ((extent.0 / s.cell).floor() as i64 + 1, (extent.1 / s.cell).floor() as i64 + 1);
    let cell_of = |p: [f64; 2]| {
        ((p[0] / s.cell).round().clamp(0.0, (nx - 1) as f64) as i64, (p[1] / s.cell).round().clamp(0.0, (nz - 1) as f64) as i64)
    };
    let at = |i: i64, j: i64| [i as f64 * s.cell, j as f64 * s.cell];
    let (start, goal) = (cell_of(from), cell_of(to));
    let n = (nx * nz) as usize;

    // Each cell's ground, water and noise, once.
    let mut ground = vec![0.0; n];
    let mut wet = vec![false; n];
    let mut noise = vec![0.0; n];
    for j in 0..nz {
        for i in 0..nx {
            let k = (j * nx + i) as usize;
            let p = at(i, j);
            ground[k] = height(p[0], p[1]);
            wet[k] = water(p[0], p[1]);
            noise[k] = fbm(seed ^ ROUTE_SALT, p[0], p[1], 48.0) * 0.5 + 0.5;
        }
    }

    // Near the ends the way may run along the edges (it starts on one); elsewhere not.
    let near_end = |p: [f64; 2]| (p[0] - from[0]).hypot(p[1] - from[1]) < 24.0 || (p[0] - to[0]).hypot(p[1] - to[1]) < 24.0;
    let inside = |p: [f64; 2]| p[0] >= s.margin && p[1] >= s.margin && p[0] <= extent.0 - s.margin && p[1] <= extent.1 - s.margin;

    // A state is a cell and the way the road came into it.
    let state = |i: i64, j: i64, d: usize| ((j * nx + i) as usize) * 8 + d;
    let first_dir = (0..8)
        .max_by(|&a, &b| {
            let dot = |d: usize| {
                let (dx, dz) = DIRS[d];
                (dx as f64 * inward[0] + dz as f64 * inward[1]) / (dx as f64).hypot(dz as f64)
            };
            dot(a).total_cmp(&dot(b))
        })
        .unwrap_or(0);
    let mut cost = vec![f64::INFINITY; n * 8];
    let mut came = vec![usize::MAX; n * 8];
    let mut heap = BinaryHeap::new();
    let h = |i: i64, j: i64| ((i - goal.0) as f64).hypot((j - goal.1) as f64) * s.cell;
    let s0 = state(start.0, start.1, first_dir);
    cost[s0] = 0.0;
    heap.push(Open { f: h(start.0, start.1), state: s0 });
    let mut reached = None;
    while let Some(Open { f, state: st }) = heap.pop() {
        let (k, d) = (st / 8, st % 8);
        let (i, j) = ((k as i64) % nx, (k as i64) / nx);
        if f > cost[st] + h(i, j) + 1e-9 {
            continue;
        }

        if (i, j) == goal {
            reached = Some(st);
            break;
        }

        for (nd, &(dx, dz)) in DIRS.iter().enumerate() {
            // At most 90 degrees in one go.
            let turn = (nd as i64 - d as i64).rem_euclid(8).min((d as i64 - nd as i64).rem_euclid(8));
            if turn > 2 {
                continue;
            }

            let (ni, nj) = (i + dx, j + dz);
            if ni < 0 || nj < 0 || ni >= nx || nj >= nz {
                continue;
            }

            let p = at(ni, nj);
            if !inside(p) && !near_end(p) {
                continue;
            }

            let nk = (nj * nx + ni) as usize;
            let len = (dx as f64).hypot(dz as f64) * s.cell;
            let grade = (ground[nk] - ground[k]).abs() / len;
            let edge = p[0].min(p[1]).min(extent.0 - p[0]).min(extent.1 - p[1]);
            let by_edge = if near_end(p) { 0.0 } else { (1.0 - edge / s.edge_reach).max(0.0) * s.edge_weight };
            let mut c = len * (1.0 + s.grade_weight * (grade / s.max_grade).powi(2) + s.noise_weight * noise[nk] + by_edge);
            if grade > s.max_grade {
                c += len * STEEP;
            }

            c += s.turn * (turn * turn) as f64;
            if wet[nk] {
                c += len * s.water_metre;
                if !wet[k] {
                    c += s.water_entry;
                }

                // No turning in the water: the crossing straight.
                if turn > 0 {
                    c += s.water_entry;
                }
            }

            let ns = state(ni, nj, nd);
            let total = cost[st] + c;
            if total < cost[ns] {
                cost[ns] = total;
                came[ns] = st;
                heap.push(Open { f: total + h(ni, nj), state: ns });
            }
        }
    }

    // Back from the goal; the ends exactly where asked.
    let mut st = reached?;
    let mut cells = vec![];
    loop {
        let k = st / 8;
        cells.push(at((k as i64) % nx, (k as i64) / nx));
        if came[st] == usize::MAX {
            break;
        }

        st = came[st];
    }

    cells.reverse();
    if let Some(first) = cells.first_mut() {
        *first = from;
    }

    if let Some(last) = cells.last_mut() {
        *last = to;
    }

    Some(cells)
}

/// A way's few points: the search's, simplified (Douglas-Peucker within `tolerance`), each stretch in or out of the water simplified on its
/// own (the points where the way goes into the water and comes out kept: the crossing stays straight and where it was).
#[must_use]
pub fn simplify(way: &[[f64; 2]], water: &dyn Fn(f64, f64) -> bool, tolerance: f64) -> Vec<[f64; 2]> {
    if way.len() < 3 {
        return way.to_vec();
    }

    // The points where the water changes.
    let mut keep = vec![0];
    for k in 1..way.len() {
        if water(way[k][0], way[k][1]) != water(way[k - 1][0], way[k - 1][1]) {
            keep.push(if water(way[k][0], way[k][1]) { k - 1 } else { k });
        }
    }

    keep.push(way.len() - 1);
    keep.dedup();
    let mut out = vec![way[0]];
    for w in keep.windows(2) {
        let part = douglas_peucker(&way[w[0]..=w[1]], tolerance);
        out.extend_from_slice(&part[1..]);
    }

    out
}

fn douglas_peucker(points: &[[f64; 2]], tolerance: f64) -> Vec<[f64; 2]> {
    if points.len() < 3 {
        return points.to_vec();
    }

    let (a, b) = (points[0], points[points.len() - 1]);
    let (far, d) = points[1..points.len() - 1]
        .iter()
        .enumerate()
        .map(|(k, p)| (k + 1, nearest_on_line(*p, &[a, b]).0))
        .max_by(|x, y| x.1.total_cmp(&y.1))
        .unwrap_or((0, 0.0));
    if d <= tolerance {
        return vec![a, b];
    }

    let mut left = douglas_peucker(&points[..=far], tolerance);
    left.pop();
    left.extend(douglas_peucker(&points[far..], tolerance));
    left
}

/// Where a road from one link ends (docs/plans/river_bridges.md: from it over the river, ending somewhere on the far side): a place by the
/// seed on the far side of the river's line from `from`, `off` metres or more past its water (`water_half` from its line), half the tile or
/// more from `from`, `margin` in from the edges. None when there is no such place.
#[must_use]
pub fn far_side_end(
    from: [f64; 2],
    river: &[[f64; 2]],
    water_half: f64,
    off: f64,
    extent: (f64, f64),
    margin: f64,
    seed: u64,
) -> Option<[f64; 2]> {
    let side = nearest_on_line(from, river).2;
    let reach = extent.0.min(extent.1) * 0.5;
    let step = 8.0;
    let mut places = vec![];
    let mut z = margin;
    while z <= extent.1 - margin {
        let mut x = margin;
        while x <= extent.0 - margin {
            let (d, _, sd) = nearest_on_line([x, z], river);
            if sd != side && d >= water_half + off && (x - from[0]).hypot(z - from[1]) >= reach {
                places.push([x, z]);
            }

            x += step;
        }

        z += step;
    }

    if places.is_empty() {
        return None;
    }

    let pick = ((corner(seed ^ ROUTE_SALT, 7, 11) * 0.5 + 0.5) * places.len() as f64) as usize;
    Some(places[pick.min(places.len() - 1)])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::river_tile_gen::distance_to_line;

    /// A river across the middle of a flat tile, along x, 20 m either side of z 128.
    fn river() -> Vec<[f64; 2]> {
        vec![[0.0, 128.0], [256.0, 128.0]]
    }

    fn in_river(x: f64, z: f64) -> bool {
        let _ = x;
        (z - 128.0).abs() < 20.0
    }

    /// The times a way crosses the river (from one side of its middle to the other).
    fn crossings(way: &[[f64; 2]]) -> usize {
        way.windows(2).filter(|w| (w[0][1] - 128.0).signum() != (w[1][1] - 128.0).signum()).count()
    }

    #[test]
    fn a_way_over_a_river_crosses_it_once_and_square() {
        let flat = |_: f64, _: f64| 0.0;
        for seed in 0..6 {
            let way = route([60.0, 0.0], [0.0, 1.0], [200.0, 256.0], (256.0, 256.0), &flat, &in_river, seed, &RouteSettings::default())
                .expect("a way");
            assert_eq!(way[0], [60.0, 0.0]);
            assert_eq!(way[way.len() - 1], [200.0, 256.0]);
            assert_eq!(crossings(&way), 1, "seed {seed}: once");
            // Square: in the water it goes along z only.
            let wet = way.iter().filter(|p| in_river(p[0], p[1])).collect::<Vec<_>>();
            assert!(wet.iter().all(|p| (p[0] - wet[0][0]).abs() < 1e-6), "seed {seed}: straight across {wet:?}");

            let few = simplify(&way, &in_river, 2.5);
            assert!(few.len() < way.len() / 3, "seed {seed}: simplified {} of {}", few.len(), way.len());
            assert_eq!((few[0], few[few.len() - 1]), (way[0], way[way.len() - 1]));
            assert_eq!(crossings(&few), 1);
        }
    }

    #[test]
    fn a_way_goes_round_steep_ground() {
        // A steep mound in the way: 30 m high, 40 m across, on the straight line between the ends.
        let mound = |x: f64, z: f64| {
            let d = (x - 128.0).hypot(z - 60.0);
            30.0 * (1.0 - (d / 40.0).min(1.0))
        };
        let dry = |_: f64, _: f64| false;
        let way =
            route([128.0, 0.0], [0.0, 1.0], [128.0, 140.0], (256.0, 256.0), &mound, &dry, 1, &RouteSettings::default()).expect("a way");
        let worst = way.iter().map(|p| mound(p[0], p[1])).fold(0.0, f64::max);
        assert!(worst < 3.0, "round the mound, not over it: {worst}");
    }

    #[test]
    fn a_road_from_one_link_ends_on_the_far_side() {
        let line = river();
        for seed in 0..8 {
            let end = far_side_end([60.0, 0.0], &line, 20.0, 30.0, (256.0, 256.0), 24.0, seed).expect("a place");
            assert!(end[1] > 128.0 + 20.0 + 30.0 - 1e-6, "seed {seed}: past the river: {end:?}");
            assert!(distance_to_line(end, &line) >= 50.0);
        }

        let a = far_side_end([60.0, 0.0], &line, 20.0, 30.0, (256.0, 256.0), 24.0, 1);
        let b = far_side_end([60.0, 0.0], &line, 20.0, 30.0, (256.0, 256.0), 24.0, 2);
        assert_ne!(a, b, "by the seed");
    }
}
