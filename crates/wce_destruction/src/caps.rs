//! The broken faces: where a removed cell's face crosses the solid. On each face the solid's section is sampled on a grid and contoured
//! (marching squares, the crossings found by bisection, so they lie on the surface within a sixteenth of the grid), clipped to the face; a
//! face inside the solid throughout is one polygon. A face between a removed cell and what still stands gives two caps: the fragment's
//! (out of the cell) and the standing part's (into it), the latter shown from the cell's level until its neighbour's.

use std::collections::BTreeMap;

use crate::cells::{Across, Cells};
use crate::geom::{Plane, V3, basis, clip_polygon, newell};
use crate::solid::Solid;

/// A cap polygon (counter-clockwise seen from its normal's side).
#[derive(Clone, Debug)]
pub struct Cap {
    pub poly: Vec<V3>,
    pub normal: V3,
}

/// A cap of the standing part, shown at the damage levels `from..until` (`until` `None`: from then on).
#[derive(Clone, Debug)]
pub struct StandingCap {
    pub cap: Cap,
    pub from: usize,
    pub until: Option<usize>,
}

#[derive(Clone, Debug, Default)]
pub struct Caps {
    pub fragments: BTreeMap<usize, Vec<Cap>>,
    pub standing: Vec<StandingCap>,
}

impl Caps {
    /// The standing part's caps at damage level `l`.
    pub fn standing_at(&self, l: usize) -> impl Iterator<Item = &Cap> {
        self.standing.iter().filter(move |s| s.from <= l && s.until.is_none_or(|u| l < u)).map(|s| &s.cap)
    }
}

/// The caps of every removed cell, sampled every `h` (m).
pub fn caps(cells: &Cells, solid: &Solid, h: f32) -> Caps {
    let mut out = Caps::default();
    if solid.is_empty() {
        return out;
    }

    let mut ids = cells.shapes.keys().copied().collect::<Vec<_>>();
    ids.sort_unstable();
    for c in ids {
        let shape = &cells.shapes[&c];
        let lc = cells.rank(c);
        let mut mine = vec![];
        for f in &shape.faces {
            let other = match f.across {
                Across::Domain(false) => continue,
                Across::Domain(true) => usize::MAX,
                Across::Cell(j) => cells.rank(j),
            };

            let polys = section(&f.poly, &f.plane, solid, h);
            for poly in polys {
                mine.push(Cap { poly: poly.clone(), normal: f.plane.n });
                if other > lc {
                    let mut rev = poly;
                    rev.reverse();
                    out.standing.push(StandingCap { cap: Cap { poly: rev, normal: -f.plane.n }, from: lc, until: (other != usize::MAX).then_some(other) });
                }
            }
        }

        out.fragments.insert(c, mine);
    }

    out
}

/// The solid's section on a convex face (its polygons, counter-clockwise about the plane's normal).
pub fn section(face: &[V3], plane: &Plane, solid: &Solid, h: f32) -> Vec<Vec<V3>> {
    let n = plane.n;
    let (u, v) = basis(&n);
    let to3 = |x: f32, y: f32| u * x + v * y + n * plane.d;
    let pts = face.iter().map(|p| (p.dot(&u), p.dot(&v))).collect::<Vec<_>>();
    let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
    for &(x, y) in &pts {
        (x0, y0, x1, y1) = (x0.min(x), y0.min(y), x1.max(x), y1.max(y));
    }

    let area = newell(face).norm() * 0.5;
    if area < 1e-4 {
        return vec![];
    }

    let nx = (((x1 - x0) / h).ceil() as usize).clamp(1, 400);
    let ny = (((y1 - y0) / h).ceil() as usize).clamp(1, 400);
    let (hx, hy) = ((x1 - x0) / nx as f32, (y1 - y0) / ny as f32);
    let corner = |i: usize, j: usize| (x0 + i as f32 * hx, y0 + j as f32 * hy);
    let mut inside = vec![false; (nx + 1) * (ny + 1)];
    let (mut any, mut all) = (false, true);
    for j in 0..=ny {
        for i in 0..=nx {
            let (x, y) = corner(i, j);
            let p = to3(x, y);
            let k = solid.inside(&p);
            inside[j * (nx + 1) + i] = k;
            // Only the corners on the face count for "all inside".
            if in_convex(&pts, x, y, 1e-4) {
                any |= k;
                all &= k;
            }
        }
    }

    // The face's own corners, too.
    for p in face {
        let k = solid.inside(p);
        any |= k;
        all &= k;
    }

    if !any {
        return vec![];
    }

    if all {
        return vec![face.to_vec()];
    }

    let at = |i: usize, j: usize| inside[j * (nx + 1) + i];
    let crossing = |a: (f32, f32), b: (f32, f32), a_in: bool| {
        let (mut lo, mut hi) = (0.0f32, 1.0f32);
        for _ in 0..4 {
            let m = 0.5 * (lo + hi);
            let p = to3(a.0 + (b.0 - a.0) * m, a.1 + (b.1 - a.1) * m);
            if solid.inside(&p) == a_in { lo = m } else { hi = m }
        }
        let m = 0.5 * (lo + hi);
        (a.0 + (b.0 - a.0) * m, a.1 + (b.1 - a.1) * m)
    };

    let face_planes = (0..pts.len()).map(|k| {
        let (a, b) = (pts[k], pts[(k + 1) % pts.len()]);
        // Inside is on the left of a counter-clockwise edge.
        let e = V3::new(b.0 - a.0, b.1 - a.1, 0.0);
        let nrm = V3::new(e.y, -e.x, 0.0).normalize();
        Plane { n: nrm, d: nrm.x * a.0 + nrm.y * a.1 }
    }).collect::<Vec<_>>();
    let clip2 = |poly: Vec<V3>| face_planes.iter().fold(poly, |p, pl| if p.len() < 3 { p } else { clip_polygon(&p, pl) });

    let mut out = vec![];
    for j in 0..ny {
        let mut i = 0;
        while i < nx {
            let full = |i: usize| at(i, j) && at(i + 1, j) && at(i, j + 1) && at(i + 1, j + 1);
            if full(i) {
                // A run of full cells: one rectangle.
                let start = i;
                while i < nx && full(i) {
                    i += 1;
                }

                let (a, b) = (corner(start, j), corner(i, j + 1));
                let rect = vec![V3::new(a.0, a.1, 0.0), V3::new(b.0, a.1, 0.0), V3::new(b.0, b.1, 0.0), V3::new(a.0, b.1, 0.0)];
                push(&mut out, clip2(rect), &to3);
                continue;
            }

            let quad = [(i, j), (i + 1, j), (i + 1, j + 1), (i, j + 1)];
            if quad.iter().any(|&(a, b)| at(a, b)) {
                let mut poly = vec![];
                for k in 0..4 {
                    let (a, b) = (quad[k], quad[(k + 1) % 4]);
                    let (ia, ib) = (at(a.0, a.1), at(b.0, b.1));
                    let (pa, pb) = (corner(a.0, a.1), corner(b.0, b.1));
                    if ia {
                        poly.push(V3::new(pa.0, pa.1, 0.0));
                    }

                    if ia != ib {
                        let c = crossing(pa, pb, ia);
                        poly.push(V3::new(c.0, c.1, 0.0));
                    }
                }

                push(&mut out, clip2(poly), &to3);
            }

            i += 1;
        }
    }

    out
}

fn push(out: &mut Vec<Vec<V3>>, poly2: Vec<V3>, to3: &dyn Fn(f32, f32) -> V3) {
    if poly2.len() < 3 {
        return;
    }

    let poly = poly2.iter().map(|p| to3(p.x, p.y)).collect::<Vec<_>>();
    if newell(&poly).norm() > 1e-6 {
        out.push(poly);
    }
}

fn in_convex(pts: &[(f32, f32)], x: f32, y: f32, eps: f32) -> bool {
    (0..pts.len()).all(|k| {
        let (a, b) = (pts[k], pts[(k + 1) % pts.len()]);
        (b.0 - a.0) * (y - a.1) - (b.1 - a.1) * (x - a.0) >= -eps
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geom::Aabb;
    use crate::params::{Params, Volume};

    fn cube(lo: V3, hi: V3) -> Vec<[V3; 3]> {
        let p = |x: bool, y: bool, z: bool| V3::new(if x { hi.x } else { lo.x }, if y { hi.y } else { lo.y }, if z { hi.z } else { lo.z });
        let (f, t) = (false, true);
        let q = [
            [p(f, f, f), p(f, t, f), p(t, t, f), p(t, f, f)],
            [p(f, f, t), p(t, f, t), p(t, t, t), p(f, t, t)],
            [p(f, f, f), p(t, f, f), p(t, f, t), p(f, f, t)],
            [p(f, t, f), p(f, t, t), p(t, t, t), p(t, t, f)],
            [p(f, f, f), p(f, f, t), p(f, t, t), p(f, t, f)],
            [p(t, f, f), p(t, t, f), p(t, t, t), p(t, f, t)],
        ];
        q.iter().flat_map(|f| [[f[0], f[1], f[2]], [f[0], f[2], f[3]]]).collect()
    }

    #[test]
    fn a_block_cut_through_shows_its_section() {
        // A 20 x 10 x 4 m block, its middle broken out.
        let (lo, hi) = (V3::new(-10.0, 0.0, 0.0), V3::new(10.0, 10.0, 4.0));
        let solid = Solid::new(cube(lo, hi), None);
        let b = Aabb { min: lo, max: hi };
        let mut p = Params::defaults("x", &b, "fort_wall");
        p.set_level_count(1, &b, "fort_wall");
        p.levels[0].volumes = vec![Volume { centre: [0.0, 7.0, 2.0], half: [3.0, 3.5, 3.0], yaw: 0.0, ellipsoid: false }];
        p.levels[0].fragment_size = 1.5;
        let cells = Cells::make(&b, &p);
        let caps = caps(&cells, &solid, 0.25);
        let area = |c: &Cap| newell(&c.poly).norm() * 0.5;
        let standing = caps.standing_at(1).map(area).sum::<f32>();
        // The hole's walls inside the block: at least its two sides (about 6 x 3.5 m each, through 4 m) and its floor.
        assert!(standing > 30.0, "{standing}");
        for c in caps.standing_at(1) {
            assert!(newell(&c.poly).dot(&c.normal) > 0.0, "caps wind about their normal");
            for q in &c.poly {
                assert!(q.z >= -0.05 && q.z <= 4.05, "inside the block's depth: {q:?}");
            }
        }

        assert_eq!(caps.standing_at(0).count(), 0);
        assert!(caps.fragments.values().flatten().count() > 0);
    }
}
