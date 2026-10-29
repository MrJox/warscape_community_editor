//! The broken faces: where a removed cell's face crosses the solid. On each face the solid's section is sampled on a grid and contoured
//! (marching squares, the crossings found by bisection, so they lie on the surface within a sixteenth of the grid), clipped to the face; a
//! face inside the solid throughout is one polygon. A face between a removed cell and what still stands gives two caps: the fragment's
//! (out of the cell) and the standing part's (into it), the latter shown from the cell's level until its neighbour's.

use std::collections::{BTreeMap, HashMap};

use crate::cells::{Across, Cells};
use crate::geom::{Plane, V3, basis, clip_polygon, newell};
use crate::poly2d::{P2, area2, contains, simplify, triangulate};
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
    let reach = solid.bounds().expanded(h);
    for c in ids {
        let shape = &cells.shapes[&c];
        if !shape.bounds().overlaps(&reach) {
            continue;
        }

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

/// The solid's section on a convex face (its polygons, counter-clockwise about the plane's normal). The face is sampled on a grid
/// (outside the face counts as outside the solid, so the contours close on the face's edges); the contours are chained into rings,
/// simplified, clipped to the face and triangulated, holes kept (a shell's section is a ring).
pub fn section(face: &[V3], plane: &Plane, solid: &Solid, h: f32) -> Vec<Vec<V3>> {
    let n = plane.n;
    let (u, v) = basis(&n);
    let to3 = |x: f32, y: f32| u * x + v * y + n * plane.d;
    let pts = face.iter().map(|p| (p.dot(&u), p.dot(&v))).collect::<Vec<_>>();
    let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
    for &(x, y) in &pts {
        (x0, y0, x1, y1) = (x0.min(x), y0.min(y), x1.max(x), y1.max(y));
    }

    if newell(face).norm() * 0.5 < 1e-4 {
        return vec![];
    }

    let nx = (((x1 - x0) / h).ceil() as usize).clamp(1, 400);
    let ny = (((y1 - y0) / h).ceil() as usize).clamp(1, 400);
    let (hx, hy) = ((x1 - x0) / nx as f32, (y1 - y0) / ny as f32);
    // One cell of margin all round, outside the face: every contour closes.
    let (gx, gy) = (nx + 3, ny + 3);
    let corner = |i: usize, j: usize| (x0 + (i as f32 - 1.0) * hx, y0 + (j as f32 - 1.0) * hy);
    let on_face = |x: f32, y: f32| in_convex(&pts, x, y, 1e-5);
    let test = |x: f32, y: f32| on_face(x, y) && solid.inside(&to3(x, y));
    let mut inside = vec![false; gx * gy];
    let (mut any, mut all) = (false, true);
    for j in 0..gy {
        for i in 0..gx {
            let (x, y) = corner(i, j);
            if !on_face(x, y) {
                continue;
            }

            let k = solid.inside(&to3(x, y));
            inside[j * gx + i] = k;
            any |= k;
            all &= k;
        }
    }

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

    // Crossings, one per grid edge: horizontal edges (i, j)-(i+1, j) are 2 * (j * gx + i), vertical (i, j)-(i, j+1) one more.
    let at = |i: usize, j: usize| inside[j * gx + i];
    let mut points: HashMap<usize, (f32, f32)> = HashMap::new();
    let mut crossing = |e: usize, a: (usize, usize), b: (usize, usize)| -> (f32, f32) {
        *points.entry(e).or_insert_with(|| {
            let (pa, pb) = (corner(a.0, a.1), corner(b.0, b.1));
            let a_in = at(a.0, a.1);
            let (mut lo, mut hi) = (0.0f32, 1.0f32);
            for _ in 0..5 {
                let m = 0.5 * (lo + hi);
                if test(pa.0 + (pb.0 - pa.0) * m, pa.1 + (pb.1 - pa.1) * m) == a_in { lo = m } else { hi = m }
            }
            let m = 0.5 * (lo + hi);
            (pa.0 + (pb.0 - pa.0) * m, pa.1 + (pb.1 - pa.1) * m)
        })
    };

    // Per cell, the contour's segments with the inside on their left: from where an inside run along the cell's boundary (walked
    // counter-clockwise) ends to where it began.
    let mut next: HashMap<usize, usize> = HashMap::new();
    for j in 0..gy - 1 {
        for i in 0..gx - 1 {
            let c = [(i, j), (i + 1, j), (i + 1, j + 1), (i, j + 1)];
            let ins = c.map(|(a, b)| at(a, b));
            if ins.iter().all(|&x| x) || ins.iter().all(|&x| !x) {
                continue;
            }

            let edge = [2 * (j * gx + i), 2 * (j * gx + i + 1) + 1, 2 * ((j + 1) * gx + i), 2 * (j * gx + i) + 1];
            let mut entering = vec![];
            let mut leaving = vec![];
            for k in 0..4 {
                let (a, b) = (k, (k + 1) % 4);
                if ins[a] != ins[b] {
                    crossing(edge[k], c[a], c[b]);
                    if ins[b] { entering.push((k, edge[k])) } else { leaving.push((k, edge[k])) }
                }
            }

            // Each leaving crossing goes to the entering crossing that began its run (the latest before it, cyclically).
            for &(kl, el) in &leaving {
                let e = entering.iter().filter(|(ke, _)| *ke < kl).max_by_key(|(ke, _)| *ke).or_else(|| entering.iter().max_by_key(|(ke, _)| *ke));
                if let Some(&(_, ee)) = e {
                    next.insert(el, ee);
                }
            }
        }
    }

    // The rings.
    let mut starts = next.keys().copied().collect::<Vec<_>>();
    starts.sort_unstable();
    let mut rings: Vec<Vec<P2>> = vec![];
    for s in starts {
        if !next.contains_key(&s) {
            continue;
        }

        let mut ring = vec![];
        let mut e = s;
        while let Some(n) = next.remove(&e) {
            ring.push(points[&e]);
            e = n;
            if e == s {
                break;
            }
        }

        if ring.len() >= 3 {
            rings.push(simplify(ring, h * 0.15));
        }
    }

    let face_planes = (0..pts.len()).map(|k| {
        let (a, b) = (pts[k], pts[(k + 1) % pts.len()]);
        let e = V3::new(b.0 - a.0, b.1 - a.1, 0.0);
        let nrm = V3::new(e.y, -e.x, 0.0).normalize();
        Plane { n: nrm, d: nrm.x * a.0 + nrm.y * a.1 }
    }).collect::<Vec<_>>();
    let clip = |r: &[P2]| -> Vec<P2> {
        let poly = r.iter().map(|p| V3::new(p.0, p.1, 0.0)).collect::<Vec<_>>();
        face_planes.iter().fold(poly, |p, pl| if p.len() < 3 { p } else { clip_polygon(&p, pl) }).iter().map(|p| (p.x, p.y)).collect()
    };

    let (outers, holes): (Vec<_>, Vec<_>) = rings.into_iter().filter(|r| r.len() >= 3 && area2(r).abs() > 1e-4).partition(|r| area2(r) > 0.0);
    let mut out = vec![];
    for o in outers {
        let mine = holes.iter().filter(|hl| contains(&o, hl[0])).cloned().collect::<Vec<_>>();
        let o = clip(&o);
        if o.len() < 3 || area2(&o) <= 1e-4 {
            continue;
        }

        let (ps, tris) = triangulate(&o, &mine);
        for t in tris {
            let tri = t.map(|k| to3(ps[k].0, ps[k].1)).to_vec();
            if newell(&tri).norm() > 1e-7 {
                out.push(tri);
            }
        }
    }

    out
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
