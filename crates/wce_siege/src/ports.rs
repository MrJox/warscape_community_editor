//! A port's sea lines (docs/plans/siege_ai.md, "Ports"; CA's: docs/re/siege_ai.md): the defender's deployment lines out at sea, and the
//! beaches an attacker lands on with the lines just inland of them.

use crate::generate::{CELL, remove_spurs, trace};
use crate::raster::{Grid, Mask, N8, dist, simplify};
use crate::{P, Sea};

/// The deployment lines: this far offshore at most (CA's median: 280), half this long (CA's: ~250 long).
const DEPLOY_OFFSHORE: f64 = 280.0;
const DEPLOY_HALF: f64 = 125.0;
const DEPLOY_APART: f64 = 220.0;

/// A beach: coast at most this far from the perimeter, where the ground within `BEACH_REACH` inland stays under `BEACH_RISE` metres,
/// at least `BEACH_MIN` long.
const BEACH_FROM_TOWN: f64 = 500.0;
const BEACH_REACH: f64 = 30.0;
const BEACH_RISE: f64 = 8.0;
const BEACH_MIN: f64 = 60.0;
const MAX_BEACHES: usize = 5;

/// The beach landing's fort approach: this far inland (CA's median: 40), half this long (CA's: ~100 long).
const BEACH_INLAND: f64 = 40.0;
const BEACH_APPROACH_HALF: f64 = 50.0;

/// How a line turns around the town: > 0 when the town is on its left going along it in (x, z) as cross(end - start, town - start).
pub fn winding(l: &[P; 2], town: P) -> f64 {
    let (d, t) = ([l[1][0] - l[0][0], l[1][1] - l[0][1]], [town[0] - l[0][0], town[1] - l[0][1]]);
    d[0] * t[1] - d[1] * t[0]
}

/// Up to four lines out at sea in the defender's zone (`lobe`: its water), as far offshore as `DEPLOY_OFFSHORE` and `DEPLOY_APART`
/// from each other, across the direction from the town; turned like CA's (the town on the other side from the approach lines').
pub fn deployment_lines(g: &Grid, lobe: &Mask, to_land: &[f64], town: P) -> Vec<[P; 2]> {
    let in_lobe = |p: P| g.at(p).is_some_and(|k| lobe[k]);
    let mut candidates: Vec<(f64, P)> = (0..g.len()).step_by(3).filter(|&k| lobe[k] && to_land[k] >= 30.0).map(|k| (to_land[k].min(DEPLOY_OFFSHORE), g.centre(k))).collect();
    candidates.sort_by(|a, b| b.0.total_cmp(&a.0));
    let mut out: Vec<[P; 2]> = vec![];
    let mut taken: Vec<P> = vec![];
    for (_, p) in candidates {
        if out.len() == 4 {
            break;
        }

        if taken.iter().any(|&q| dist(p, q) < DEPLOY_APART) {
            continue;
        }

        let d = dist(p, town).max(1e-9);
        let t = [-(p[1] - town[1]) / d, (p[0] - town[0]) / d];
        for half in [DEPLOY_HALF, 90.0, 60.0] {
            let l = [[p[0] - t[0] * half, p[1] - t[1] * half], [p[0] + t[0] * half, p[1] + t[1] * half]];
            if in_lobe(l[0]) && in_lobe(l[1]) {
                out.push(if winding(&l, town) < 0.0 { l } else { [l[1], l[0]] });
                taken.push(p);
                break;
            }
        }
    }

    out
}

/// The beaches: the coast (land next to the sea) near the town, inside it too, where the ground rises gently; traced into lines, the
/// nearest `MAX_BEACHES`, turned like CA's. And beside each, its fort approach: a line `BEACH_INLAND` inland along it.
#[allow(clippy::too_many_arguments)]
pub fn beaches(g: &Grid, sea: &Mask, nogo: &Mask, dout: &[f64], heights: &Sea, to_sea: &[f64], town: P) -> (Vec<Vec<P>>, Vec<[P; 2]>) {
    let mut coast = g.mask();
    for k in 0..g.len() {
        if sea[k] || nogo[k] || dout[k] > BEACH_FROM_TOWN || !N8.iter().any(|&d| g.step(k, d).is_some_and(|n| sea[n])) {
            continue;
        }

        let p = g.centre(k);
        let gentle = (0..8).all(|i| {
            let a = f64::from(i) * std::f64::consts::FRAC_PI_4;
            let q = [p[0] + a.cos() * BEACH_REACH, p[1] + a.sin() * BEACH_REACH];
            heights.height(q) <= BEACH_RISE
        });
        if gentle {
            coast[k] = true;
        }
    }

    let mut skel = g.thin(&g.dilate(&coast, CELL));
    remove_spurs(g, &mut skel, 10.0);
    let mut lines: Vec<(f64, Vec<P>)> = trace(g, &skel)
        .into_iter()
        .map(|cells| {
            let near = cells.iter().map(|&k| dout[k]).fold(f64::MAX, f64::min);
            (near, cells.iter().map(|&k| g.centre(k)).collect::<Vec<P>>())
        })
        .filter(|(_, l)| l.windows(2).map(|w| dist(w[0], w[1])).sum::<f64>() >= BEACH_MIN)
        .collect();
    lines.sort_by(|a, b| a.0.total_cmp(&b.0));
    lines.truncate(MAX_BEACHES);

    let on_land = |p: P| g.at(p).is_some_and(|k| !sea[k] && !nogo[k]);
    let mut out = vec![];
    let mut approaches = vec![];
    for (_, l) in lines {
        let mut l = simplify(&l, 2.0);
        if winding(&[l[0], l[l.len() - 1]], town) > 0.0 {
            l.reverse();
        }

        // The middle by length, the coast's direction there, and inland.
        let total: f64 = l.windows(2).map(|w| dist(w[0], w[1])).sum();
        let (m, t) = point_at(&l, total / 2.0);
        let n = [-t[1], t[0]];
        let depth = |s: f64| g.at([m[0] + n[0] * s * BEACH_INLAND, m[1] + n[1] * s * BEACH_INLAND]).map_or(0.0, |k| to_sea[k]);
        let side = if depth(1.0) >= depth(-1.0) { 1.0 } else { -1.0 };
        let half = BEACH_APPROACH_HALF.min(total / 2.0);
        for inland in [BEACH_INLAND, 60.0, 25.0] {
            let p = [m[0] + n[0] * side * inland, m[1] + n[1] * side * inland];
            let a = [[p[0] - t[0] * half, p[1] - t[1] * half], [p[0] + t[0] * half, p[1] + t[1] * half]];
            if on_land(a[0]) && on_land(a[1]) && on_land(p) {
                approaches.push(if winding(&a, town) > 0.0 { a } else { [a[1], a[0]] });
                break;
            }
        }

        out.push(l);
    }

    (out, approaches)
}

/// The point `s` along a line, and the line's direction there (unit).
fn point_at(l: &[P], s: f64) -> (P, P) {
    let mut left = s;
    for w in l.windows(2) {
        let d = dist(w[0], w[1]);
        if d > 1e-9 && left <= d {
            let u = [(w[1][0] - w[0][0]) / d, (w[1][1] - w[0][1]) / d];
            return ([w[0][0] + u[0] * left, w[0][1] + u[1] * left], u);
        }

        left -= d;
    }

    let n = l.len();
    let d = dist(l[n - 2], l[n - 1]).max(1e-9);
    (l[n - 1], [(l[n - 1][0] - l[n - 2][0]) / d, (l[n - 1][1] - l[n - 2][1]) / d])
}
