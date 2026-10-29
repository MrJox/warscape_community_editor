//! What is inside a building, for its broken faces: the generalised winding number of its triangles (robust on the game's open, buried
//! meshes: inside where it is over a half) and the distance to its surface, through a grid of triangles. A solid part is everything
//! inside; a shell (a house) only what is inside within its wall thickness of the surface.

use crate::geom::{Aabb, V3};

/// Triangles and a grid over them.
pub struct Solid {
    tris: Vec<[V3; 3]>,
    origin: V3,
    cell: f32,
    dims: [usize; 3],
    buckets: Vec<Vec<u32>>,

    /// The wall thickness of a shell; `None` for a solid.
    pub shell: Option<f32>,
}

impl Solid {
    pub fn new(tris: Vec<[V3; 3]>, shell: Option<f32>) -> Self {
        let bounds = Aabb::of(tris.iter().flatten().copied()).expanded(0.5);
        let size = bounds.size();
        let cell = (size.x * size.y * size.z / 32768.0).cbrt().max(0.5);
        let dims = [0, 1, 2].map(|a| ((size[a] / cell).ceil() as usize).clamp(1, 128));
        let mut s = Self { tris, origin: bounds.min, cell, dims, buckets: vec![vec![]; dims[0] * dims[1] * dims[2]], shell };
        for (i, t) in s.tris.iter().enumerate() {
            let b = Aabb::of(t.iter().copied());
            let (lo, hi) = (s.coord(&b.min), s.coord(&b.max));
            for z in lo[2]..=hi[2] {
                for y in lo[1]..=hi[1] {
                    for x in lo[0]..=hi[0] {
                        let k = (z * dims[1] + y) * dims[0] + x;
                        s.buckets[k].push(i as u32);
                    }
                }
            }
        }

        s
    }

    pub fn is_empty(&self) -> bool {
        self.tris.is_empty()
    }

    fn coord(&self, p: &V3) -> [usize; 3] {
        [0, 1, 2].map(|a| (((p[a] - self.origin[a]) / self.cell).floor().max(0.0) as usize).min(self.dims[a] - 1))
    }

    /// The generalised winding number at a point (1 inside a closed surface, 0 outside, about a half on an open sheet's side).
    pub fn winding(&self, p: &V3) -> f32 {
        let mut sum = 0.0f64;
        for t in &self.tris {
            let (a, b, c) = (t[0] - p, t[1] - p, t[2] - p);
            let (la, lb, lc) = (a.norm(), b.norm(), c.norm());
            if la < 1e-7 || lb < 1e-7 || lc < 1e-7 {
                continue;
            }

            let num = a.dot(&b.cross(&c));
            let den = la * lb * lc + a.dot(&b) * lc + b.dot(&c) * la + c.dot(&a) * lb;
            sum += 2.0 * f64::from(num).atan2(f64::from(den));
        }

        (sum / (4.0 * std::f64::consts::PI)) as f32
    }

    /// The distance to the nearest triangle, searched up to `limit` (beyond it: `limit`).
    pub fn distance(&self, p: &V3, limit: f32) -> f32 {
        let r = (limit / self.cell).ceil() as usize;
        let c = self.coord(p);
        let mut best = limit;
        let lo = c.map(|x| x.saturating_sub(r));
        let hi = [0, 1, 2].map(|a| (c[a] + r).min(self.dims[a] - 1));
        for z in lo[2]..=hi[2] {
            for y in lo[1]..=hi[1] {
                for x in lo[0]..=hi[0] {
                    for &i in &self.buckets[(z * self.dims[1] + y) * self.dims[0] + x] {
                        best = best.min(point_triangle(p, &self.tris[i as usize]));
                    }
                }
            }
        }

        best
    }

    /// Whether a point is in the solid.
    pub fn inside(&self, p: &V3) -> bool {
        if self.tris.is_empty() {
            return false;
        }

        match self.shell {
            None => self.winding(p) > 0.5,
            Some(t) => self.distance(p, t) < t && self.winding(p) > 0.5,
        }
    }

    /// The triangles near a box (their indices), each once.
    pub fn near(&self, b: &Aabb) -> Vec<usize> {
        let (lo, hi) = (self.coord(&b.min), self.coord(&b.max));
        let mut out = vec![];
        for z in lo[2]..=hi[2] {
            for y in lo[1]..=hi[1] {
                for x in lo[0]..=hi[0] {
                    out.extend(self.buckets[(z * self.dims[1] + y) * self.dims[0] + x].iter().map(|&i| i as usize));
                }
            }
        }

        out.sort_unstable();
        out.dedup();
        out
    }

    pub fn triangle(&self, i: usize) -> &[V3; 3] {
        &self.tris[i]
    }
}

/// The distance from a point to a triangle (Ericson, Real-Time Collision Detection 5.1.5).
pub fn point_triangle(p: &V3, t: &[V3; 3]) -> f32 {
    let (a, b, c) = (t[0], t[1], t[2]);
    let (ab, ac, ap) = (b - a, c - a, p - a);
    let (d1, d2) = (ab.dot(&ap), ac.dot(&ap));
    if d1 <= 0.0 && d2 <= 0.0 {
        return ap.norm();
    }

    let bp = p - b;
    let (d3, d4) = (ab.dot(&bp), ac.dot(&bp));
    if d3 >= 0.0 && d4 <= d3 {
        return bp.norm();
    }

    let vc = d1 * d4 - d3 * d2;
    if vc <= 0.0 && d1 >= 0.0 && d3 <= 0.0 {
        let v = d1 / (d1 - d3);
        return (p - (a + ab * v)).norm();
    }

    let cp = p - c;
    let (d5, d6) = (ab.dot(&cp), ac.dot(&cp));
    if d6 >= 0.0 && d5 <= d6 {
        return cp.norm();
    }

    let vb = d5 * d2 - d1 * d6;
    if vb <= 0.0 && d2 >= 0.0 && d6 <= 0.0 {
        let w = d2 / (d2 - d6);
        return (p - (a + ac * w)).norm();
    }

    let va = d3 * d6 - d5 * d4;
    if va <= 0.0 && (d4 - d3) >= 0.0 && (d5 - d6) >= 0.0 {
        let w = (d4 - d3) / ((d4 - d3) + (d5 - d6));
        return (p - (b + (c - b) * w)).norm();
    }

    let denom = 1.0 / (va + vb + vc);
    let (v, w) = (vb * denom, vc * denom);
    (p - (a + ab * v + ac * w)).norm()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A closed unit cube, from 0 to 1, outward triangles.
    pub fn cube(size: f32) -> Vec<[V3; 3]> {
        let p = |x: f32, y: f32, z: f32| V3::new(x, y, z) * size;
        let q = [
            [p(0., 0., 0.), p(0., 1., 0.), p(1., 1., 0.), p(1., 0., 0.)],
            [p(0., 0., 1.), p(1., 0., 1.), p(1., 1., 1.), p(0., 1., 1.)],
            [p(0., 0., 0.), p(1., 0., 0.), p(1., 0., 1.), p(0., 0., 1.)],
            [p(0., 1., 0.), p(0., 1., 1.), p(1., 1., 1.), p(1., 1., 0.)],
            [p(0., 0., 0.), p(0., 0., 1.), p(0., 1., 1.), p(0., 1., 0.)],
            [p(1., 0., 0.), p(1., 1., 0.), p(1., 1., 1.), p(1., 0., 1.)],
        ];
        q.iter().flat_map(|f| [[f[0], f[1], f[2]], [f[0], f[2], f[3]]]).collect()
    }

    #[test]
    fn inside_a_cube_and_its_shell() {
        let s = Solid::new(cube(4.0), None);
        assert!((s.winding(&V3::new(2.0, 2.0, 2.0)) - 1.0).abs() < 1e-3);
        assert!(s.winding(&V3::new(5.0, 2.0, 2.0)).abs() < 1e-3);
        assert!((s.distance(&V3::new(2.0, 2.0, 0.5), 3.0) - 0.5).abs() < 1e-4);
        let shell = Solid::new(cube(4.0), Some(0.4));
        assert!(shell.inside(&V3::new(2.0, 2.0, 0.2)));
        assert!(!shell.inside(&V3::new(2.0, 2.0, 2.0)));
        assert!(!shell.inside(&V3::new(2.0, 2.0, -0.2)));
    }
}
