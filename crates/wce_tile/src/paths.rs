//! Paths between things (docs/plans/auto_detail_2.md, H): the way a path would go over the ground between two points, found by A* over
//! a grid of the heights. A step costs its length, more the steeper it climbs (a soft limit), none past the hard limit; water and
//! buildings bar the way, existing roads are cheap to walk along. The cells found are simplified to a few points, for a procedural road
//! (`roads`) to be made on.

use std::cmp::Ordering;
use std::collections::BinaryHeap;

/// How a path is found.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PathSettings {
    /// Degrees: steeper costs more (a step at this slope costs twice its length), and past `max_slope` it is barred.
    pub soft_slope: f32,
    pub max_slope: f32,

    /// What a step along a road costs against one off it (0 .. 1).
    pub road_cost: f32,

    /// Metres: how far from the straight line between the ends the search may go, at the least (it may go as far as the ends are
    /// apart).
    pub margin: f32,

    /// Metres the found line may stray from the cells it went through when it is simplified.
    pub tolerance: f32,
}

impl Default for PathSettings {
    fn default() -> Self {
        Self { soft_slope: 8.0, max_slope: 22.0, road_cost: 0.4, margin: 60.0, tolerance: 1.5 }
    }
}

/// The ground a path is found on: `w` × `h` cells `cell` metres across from (`x0`, `z0`), their heights (metres), whether each is barred
/// (water, a building) and whether it is on a road.
pub struct PathGrid {
    pub x0: f32,
    pub z0: f32,
    pub cell: f32,
    pub w: usize,
    pub h: usize,
    pub heights: Vec<f32>,
    pub barred: Vec<bool>,
    pub road: Vec<bool>,
}

impl PathGrid {
    fn index(&self, x: f32, z: f32) -> Option<usize> {
        let (i, j) = (((x - self.x0) / self.cell).round(), ((z - self.z0) / self.cell).round());
        (i >= 0.0 && j >= 0.0 && (i as usize) < self.w && (j as usize) < self.h).then(|| j as usize * self.w + i as usize)
    }

    fn at(&self, i: usize) -> (f32, f32) {
        (self.x0 + (i % self.w) as f32 * self.cell, self.z0 + (i / self.w) as f32 * self.cell)
    }
}

#[derive(Clone, Copy, PartialEq)]
struct Open {
    f: f32,
    i: usize,
}

impl Eq for Open {}

impl Ord for Open {
    fn cmp(&self, other: &Self) -> Ordering {
        other.f.total_cmp(&self.f).then_with(|| other.i.cmp(&self.i))
    }
}

impl PartialOrd for Open {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// The way from `from` to `to` (metres, x and z) over the grid: its points from one end to the other, simplified. The ends themselves
/// may be barred (a building's door, a point in a ford): the cells around them are let through. None when there is no way.
#[must_use]
pub fn find(g: &PathGrid, from: (f32, f32), to: (f32, f32), s: &PathSettings) -> Option<Vec<(f32, f32)>> {
    let (a, b) = (g.index(from.0, from.1)?, g.index(to.0, to.1)?);
    let n = g.w * g.h;
    if g.heights.len() != n || g.barred.len() != n || g.road.len() != n {
        return None;
    }

    // The search's bounds: around the straight line, by the margin or the ends' distance.
    let span = (to.0 - from.0).hypot(to.1 - from.1);
    let reach = s.margin.max(span * 0.75);
    let near_line = |i: usize| {
        let (x, z) = g.at(i);
        let (dx, dz) = (to.0 - from.0, to.1 - from.1);
        let len2 = dx * dx + dz * dz;
        let t = if len2 > 0.0 { (((x - from.0) * dx + (z - from.1) * dz) / len2).clamp(0.0, 1.0) } else { 0.0 };
        (x - from.0 - t * dx).hypot(z - from.1 - t * dz) <= reach
    };

    // The ends let through: a few metres round each.
    let free_radius = 4.0f32;
    let free = |i: usize| {
        let (x, z) = g.at(i);
        (x - from.0).hypot(z - from.1) <= free_radius || (x - to.0).hypot(z - to.1) <= free_radius
    };

    let (soft, hard) = (s.soft_slope.max(0.1).to_radians().tan(), s.max_slope.clamp(0.1, 89.0).to_radians().tan());
    let cost = |i: usize, j: usize, dist: f32| -> Option<f32> {
        if (g.barred[j] && !free(j)) || !near_line(j) {
            return None;
        }

        let grade = (g.heights[j] - g.heights[i]).abs() / dist;
        if grade > hard {
            return None;
        }

        let steep = 1.0 + (grade / soft).powi(2);
        let road = if g.road[i] && g.road[j] { s.road_cost.clamp(0.05, 1.0) } else { 1.0 };
        Some(dist * steep * road)
    };

    let goal = g.at(b);
    let heuristic = |i: usize| {
        let (x, z) = g.at(i);
        (x - goal.0).hypot(z - goal.1) * s.road_cost.clamp(0.05, 1.0)
    };
    let mut best = vec![f32::INFINITY; n];
    let mut came = vec![usize::MAX; n];
    let mut open = BinaryHeap::new();
    best[a] = 0.0;
    open.push(Open { f: heuristic(a), i: a });
    let d = std::f32::consts::SQRT_2;
    while let Some(Open { i, .. }) = open.pop() {
        if i == b {
            break;
        }

        let (x, z) = ((i % g.w) as isize, (i / g.w) as isize);
        for (dx, dz, k) in [(1, 0, 1.0), (-1, 0, 1.0), (0, 1, 1.0), (0, -1, 1.0), (1, 1, d), (1, -1, d), (-1, 1, d), (-1, -1, d)] {
            let (nx, nz) = (x + dx, z + dz);
            if nx < 0 || nz < 0 || nx >= g.w as isize || nz >= g.h as isize {
                continue;
            }

            let j = nz as usize * g.w + nx as usize;
            let Some(c) = cost(i, j, k * g.cell) else { continue };
            let total = best[i] + c;
            if total < best[j] {
                best[j] = total;
                came[j] = i;
                open.push(Open { f: total + heuristic(j), i: j });
            }
        }
    }

    if !best[b].is_finite() {
        return None;
    }

    let mut cells = vec![b];
    while let Some(&last) = cells.last() {
        if last == a {
            break;
        }

        cells.push(came[last]);
    }

    cells.reverse();
    let mut line = cells.iter().map(|&i| g.at(i)).collect::<Vec<_>>();
    // The ends where they were asked, not at their cells' centres.
    line[0] = from;
    *line.last_mut().expect("two ends") = to;
    Some(simplify(&line, s.tolerance.max(0.0)))
}

/// Douglas and Peucker's: the fewest points that stay within `tolerance` of the line.
#[must_use]
pub fn simplify(line: &[(f32, f32)], tolerance: f32) -> Vec<(f32, f32)> {
    if line.len() < 3 {
        return line.to_vec();
    }

    fn dp(line: &[(f32, f32)], tol: f32, out: &mut Vec<(f32, f32)>) {
        let (a, b) = (line[0], line[line.len() - 1]);
        let (dx, dz) = (b.0 - a.0, b.1 - a.1);
        let len = dx.hypot(dz).max(1e-6);
        let (mut far, mut at) = (0.0f32, 0);
        for (k, p) in line.iter().enumerate().take(line.len() - 1).skip(1) {
            let d = ((p.0 - a.0) * dz - (p.1 - a.1) * dx).abs() / len;
            if d > far {
                (far, at) = (d, k);
            }
        }

        if far > tol {
            dp(&line[..=at], tol, out);
            dp(&line[at..], tol, out);
        } else if out.last() != Some(&a) {
            out.push(a);
        }

        if far <= tol {
            out.push(b);
        }
    }

    let mut out = vec![];
    dp(line, tolerance, &mut out);
    out.dedup();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 100 × 60 m at 1 m, flat at 0, `height` and `barred` as given.
    fn grid(height: impl Fn(f32, f32) -> f32, barred: impl Fn(f32, f32) -> bool) -> PathGrid {
        let (w, h) = (101, 61);
        let at = |i: usize| ((i % w) as f32, (i / w) as f32);
        PathGrid {
            x0: 0.0,
            z0: 0.0,
            cell: 1.0,
            w,
            h,
            heights: (0..w * h).map(|i| height(at(i).0, at(i).1)).collect(),
            barred: (0..w * h).map(|i| barred(at(i).0, at(i).1)).collect(),
            road: vec![false; w * h],
        }
    }

    fn length(line: &[(f32, f32)]) -> f32 {
        line.windows(2).map(|w| (w[1].0 - w[0].0).hypot(w[1].1 - w[0].1)).sum()
    }

    #[test]
    fn on_flat_ground_the_way_is_straight() {
        let g = grid(|_, _| 0.0, |_, _| false);
        let p = find(&g, (10.0, 30.0), (90.0, 30.0), &PathSettings::default()).unwrap();
        assert_eq!(p, vec![(10.0, 30.0), (90.0, 30.0)]);
    }

    #[test]
    fn a_wall_is_gone_round_and_water_is_not_crossed() {
        // A wall across x = 50 from z = 0 to 45: the way goes round its end.
        let g = grid(|_, _| 0.0, |x, z| (x - 50.0).abs() < 1.0 && z < 45.0);
        let p = find(&g, (10.0, 10.0), (90.0, 10.0), &PathSettings::default()).unwrap();
        assert!(p.iter().any(|q| q.1 >= 44.0), "round the end: {p:?}");
        assert!(length(&p) > 100.0, "longer than straight (80 m): {}", length(&p));

        // A river right across: no way.
        let g = grid(|_, _| 0.0, |x, _| (x - 50.0).abs() < 2.0);
        assert!(find(&g, (10.0, 10.0), (90.0, 10.0), &PathSettings::default()).is_none());
    }

    #[test]
    fn a_hill_is_gone_round_or_climbed_slantwise() {
        // A hill 30 m high in the middle: straight over it is steeper than the limit.
        let hill = |x: f32, z: f32| (30.0 - (x - 50.0).hypot(z - 30.0) * 1.2).max(0.0);
        let g = grid(hill, |_, _| false);
        let p = find(&g, (5.0, 30.0), (95.0, 30.0), &PathSettings::default()).unwrap();
        let top = p.iter().map(|q| hill(q.0, q.1)).fold(0.0f32, f32::max);
        assert!(top < 15.0, "not over the top: {p:?}");
        // No step of the line steeper than the hard limit (on the cells it went through; the simplified line may cut a little).
        for w in p.windows(2) {
            let mid = ((w[0].0 + w[1].0) * 0.5, (w[0].1 + w[1].1) * 0.5);
            assert!(hill(mid.0, mid.1) < 25.0);
        }
    }

    #[test]
    fn a_road_draws_the_way_to_it() {
        // A road along z = 50: from (10, 45) to (90, 45) the way goes onto it rather than straight.
        let mut g = grid(|_, _| 0.0, |_, _| false);
        for x in 0..101 {
            g.road[50 * 101 + x] = true;
        }

        let s = PathSettings { road_cost: 0.2, ..PathSettings::default() };
        let p = find(&g, (10.0, 40.0), (90.0, 40.0), &s).unwrap();
        assert!(p.iter().any(|q| (q.1 - 50.0).abs() < 1.0), "along the road: {p:?}");
    }
}
