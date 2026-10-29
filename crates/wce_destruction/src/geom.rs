//! Geometry shared by the generator: vectors, planes, boxes, convex polygons and a seeded random generator (every output is the same
//! for the same seed).

use nalgebra::Vector3;

pub type V3 = Vector3<f32>;

pub fn v3(p: [f32; 3]) -> V3 {
    V3::new(p[0], p[1], p[2])
}

/// A plane; the inside is where `side(p) <= 0`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Plane {
    pub n: V3,
    pub d: f32,
}

impl Plane {
    pub fn side(&self, p: &V3) -> f32 {
        self.n.dot(p) - self.d
    }

    pub fn flipped(&self) -> Self {
        Self { n: -self.n, d: -self.d }
    }
}

/// An axis-aligned box.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Aabb {
    pub min: V3,
    pub max: V3,
}

impl Default for Aabb {
    fn default() -> Self {
        Self::EMPTY
    }
}

impl Aabb {
    pub const EMPTY: Self = Self { min: V3::new(f32::MAX, f32::MAX, f32::MAX), max: V3::new(f32::MIN, f32::MIN, f32::MIN) };

    pub fn of(points: impl IntoIterator<Item = V3>) -> Self {
        let mut b = Self::EMPTY;
        for p in points {
            b.add(&p);
        }
        b
    }

    pub fn add(&mut self, p: &V3) {
        self.min = self.min.inf(p);
        self.max = self.max.sup(p);
    }

    pub fn merge(&mut self, o: &Self) {
        if !o.is_empty() {
            self.add(&o.min);
            self.add(&o.max);
        }
    }

    pub fn is_empty(&self) -> bool {
        self.min.x > self.max.x
    }

    pub fn centre(&self) -> V3 {
        (self.min + self.max) * 0.5
    }

    pub fn size(&self) -> V3 {
        if self.is_empty() { V3::zeros() } else { self.max - self.min }
    }

    pub fn expanded(&self, by: f32) -> Self {
        Self { min: self.min - V3::repeat(by), max: self.max + V3::repeat(by) }
    }

    pub fn overlaps(&self, o: &Self) -> bool {
        self.min.x <= o.max.x && o.min.x <= self.max.x && self.min.y <= o.max.y && o.min.y <= self.max.y && self.min.z <= o.max.z && o.min.z <= self.max.z
    }

    pub fn contains(&self, p: &V3) -> bool {
        p.x >= self.min.x && p.x <= self.max.x && p.y >= self.min.y && p.y <= self.max.y && p.z >= self.min.z && p.z <= self.max.z
    }

    pub fn volume(&self) -> f32 {
        let s = self.size();
        s.x * s.y * s.z
    }
}

/// The part of a convex (or star-shaped) polygon inside a plane (Sutherland-Hodgman).
pub fn clip_polygon(poly: &[V3], plane: &Plane) -> Vec<V3> {
    let mut out = Vec::with_capacity(poly.len() + 1);
    for i in 0..poly.len() {
        let (a, b) = (&poly[i], &poly[(i + 1) % poly.len()]);
        let (sa, sb) = (plane.side(a), plane.side(b));
        if sa <= 0.0 {
            out.push(*a);
        }

        if (sa <= 0.0) != (sb <= 0.0) {
            out.push(a + (b - a) * (sa / (sa - sb)));
        }
    }

    out
}

/// A polygon's (Newell) normal, not normalised: its length is twice the area.
pub fn newell(poly: &[V3]) -> V3 {
    let mut n = V3::zeros();
    for i in 0..poly.len() {
        let (a, b) = (poly[i], poly[(i + 1) % poly.len()]);
        n += V3::new((a.y - b.y) * (a.z + b.z), (a.z - b.z) * (a.x + b.x), (a.x - b.x) * (a.y + b.y));
    }
    n
}

pub fn centroid(points: &[V3]) -> V3 {
    if points.is_empty() {
        return V3::zeros();
    }
    points.iter().sum::<V3>() / points.len() as f32
}

/// Two unit vectors making a right-handed basis (u, v, n) with a unit normal.
pub fn basis(n: &V3) -> (V3, V3) {
    let a = if n.x.abs() < 0.9 { V3::x() } else { V3::y() };
    let u = n.cross(&a).normalize();
    (u, n.cross(&u))
}

/// A quaternion (x y z w) as a rotation matrix's action on a vector.
pub fn rotate(q: [f32; 4], v: &V3) -> V3 {
    let (qv, w) = (V3::new(q[0], q[1], q[2]), q[3]);
    let t = qv.cross(v) * 2.0;
    v + t * w + qv.cross(&t)
}

/// A small, fast, seeded generator (SplitMix64): the same seed gives the same building.
#[derive(Clone, Debug)]
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Self(seed ^ 0x9e37_79b9_7f4a_7c15)
    }

    /// Another generator from this one and a label (independent streams per level, per fragment).
    pub fn fork(&self, label: u64) -> Self {
        let mut r = Self(self.0 ^ label.wrapping_mul(0xbf58_476d_1ce4_e5b9));
        r.next();
        r
    }

    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    /// In [0, 1).
    pub fn f(&mut self) -> f32 {
        (self.next() >> 40) as f32 / (1u64 << 24) as f32
    }

    pub fn range(&mut self, a: f32, b: f32) -> f32 {
        a + (b - a) * self.f()
    }

    /// A direction, uniform on the sphere.
    pub fn unit(&mut self) -> V3 {
        let z = self.range(-1.0, 1.0);
        let a = self.range(0.0, std::f32::consts::TAU);
        let r = (1.0 - z * z).max(0.0).sqrt();
        V3::new(r * a.cos(), z, r * a.sin())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clips_and_rotates() {
        let sq = vec![V3::new(0.0, 0.0, 0.0), V3::new(2.0, 0.0, 0.0), V3::new(2.0, 2.0, 0.0), V3::new(0.0, 2.0, 0.0)];
        let half = clip_polygon(&sq, &Plane { n: V3::x(), d: 1.0 });
        assert!((newell(&half).z - 4.0).abs() < 1e-5, "half the square: area 2");
        let s = std::f32::consts::FRAC_1_SQRT_2;
        let r = rotate([0.0, s, 0.0, s], &V3::x());
        assert!((r - V3::new(0.0, 0.0, -1.0)).norm() < 1e-5, "{r:?}");
        let (mut a, mut b) = (Rng::new(7), Rng::new(7));
        assert_eq!(a.next(), b.next());
        assert!((0..1000).map(|_| a.f()).all(|x| (0.0..1.0).contains(&x)));
    }
}
