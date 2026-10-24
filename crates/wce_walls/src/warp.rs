//! The map from a base piece's frame to a generated piece's. The base runs along x, its outer side is -z, y is up (docs/plans/
//! procedural_walls.md, "The generator"). A warp moves points and turns directions; long polygons are cut along x first so they bend with
//! the mesh.

/// A map of a piece's frame.
pub trait Warp {

    /// Where a point goes.
    fn point(&self, p: [f32; 3]) -> [f32; 3];

    /// How much a direction at a point turns about the vertical axis (radians; +x towards +z).
    fn yaw(&self, p: [f32; 3]) -> f32;

    /// Where a point that stands on the ground goes: on the ground under where [`Warp::point`] puts it (a door's or a ladder's foot).
    /// Without a ground, the same as [`Warp::point`].
    fn ground_point(&self, p: [f32; 3]) -> [f32; 3] {
        self.point(p)
    }

    /// The half-planes of the source's frame what is kept must lie in (`a x + b z <= c`): where a mitred corner or an attached end cuts
    /// the piece. None by default.
    fn clip_planes(&self) -> Vec<[f32; 3]> {
        vec![]
    }
}

/// If a point is inside every half-plane (`a x + b z <= c`).
pub fn inside(planes: &[[f32; 3]], p: [f32; 3]) -> bool {
    planes.iter().all(|&[a, b, c]| a * p[0] + b * p[2] <= c + 1e-4)
}

/// A polygon cut to the half-planes (closed: Sutherland-Hodgman; open, a polyline: its parts inside, joined).
pub fn clip_to(points: &[[f32; 3]], planes: &[[f32; 3]], closed: bool) -> Vec<[f32; 3]> {
    let mut out = points.to_vec();
    for &[a, b, c] in planes {
        let side = |p: &[f32; 3]| c - (a * p[0] + b * p[2]);
        if out.iter().all(|p| side(p) >= -1e-4) {
            continue;
        }

        if closed {
            out = clip(&out, side);
        } else {
            let mut kept = vec![];
            for i in 0..out.len() {
                let p = out[i];
                if side(&p) >= 0.0 {
                    kept.push(p);
                }

                if let Some(&q) = out.get(i + 1) && (side(&p) >= 0.0) != (side(&q) >= 0.0) {
                    let (sp, sq) = (side(&p), side(&q));
                    kept.push(lerp(p, q, sp / (sp - sq)));
                }
            }

            out = kept;
        }
    }

    out
}

/// The identity.
pub struct Identity;

impl Warp for Identity {
    fn point(&self, p: [f32; 3]) -> [f32; 3] {
        p
    }

    fn yaw(&self, _: [f32; 3]) -> f32 {
        0.0
    }
}

/// A bend about a vertical axis: x = `x0` stays where it is, the piece curves towards +z (the inner side) with the given radius at
/// z = 0 (a negative radius curves towards -z). Arc length along z = 0 is kept.
pub struct Bend {
    pub x0: f32,
    pub radius: f32,
}

impl Bend {

    /// A bend that turns the piece's end at `x1` by `angle` radians.
    pub fn turning(x0: f32, x1: f32, angle: f32) -> Self {
        Self { x0, radius: (x1 - x0) / angle }
    }
}

impl Warp for Bend {
    fn point(&self, [x, y, z]: [f32; 3]) -> [f32; 3] {
        let a = (x - self.x0) / self.radius;
        let r = self.radius - z;
        [self.x0 + r * a.sin(), y, self.radius - r * a.cos()]
    }

    fn yaw(&self, [x, ..]: [f32; 3]) -> f32 {
        (x - self.x0) / self.radius
    }
}

/// A direction (x, z) turned by `a` radians: +x goes to (cos a, sin a).
pub fn turn([x, z]: [f32; 2], a: f32) -> [f32; 2] {
    let (s, c) = a.sin_cos();
    [x * c - z * s, x * s + z * c]
}

/// A 3D direction turned about the vertical axis.
pub fn turn3([x, y, z]: [f32; 3], a: f32) -> [f32; 3] {
    let [x, z] = turn([x, z], a);
    [x, y, z]
}

/// A row-vector transform (rows: the x, y and z axes, then the translation) moved by a warp: the translation is warped, the axes turned
/// by the yaw there.
pub fn transform(m: &mut [[f32; 4]; 4], warp: &dyn Warp) {
    let t = [m[3][0], m[3][1], m[3][2]];
    let a = warp.yaw(t);
    for row in m.iter_mut().take(3) {
        let [x, y, z] = turn3([row[0], row[1], row[2]], a);
        row[0] = x;
        row[1] = y;
        row[2] = z;
    }

    let [x, y, z] = warp.point(t);
    m[3][0] = x;
    m[3][1] = y;
    m[3][2] = z;
}

/// A polyline with points added so no segment spans more than `step` metres, closed back to its first point when `closed`. The added
/// points come after the original point that starts their segment; returns, per point, the index of the original it follows.
pub fn subdivide(points: &[[f32; 3]], closed: bool, step: f32) -> Vec<([f32; 3], usize)> {
    let mut out = vec![];
    let n = points.len();
    for i in 0..n {
        let a = points[i];
        out.push((a, i));
        if i + 1 == n && !closed {
            break;
        }

        let b = points[(i + 1) % n];
        let len = ((b[0] - a[0]).powi(2) + (b[1] - a[1]).powi(2) + (b[2] - a[2]).powi(2)).sqrt();
        let parts = (len / step).ceil().max(1.0) as usize;
        for k in 1..parts {
            out.push((lerp(a, b, k as f32 / parts as f32), i));
        }
    }

    out
}

/// Where long polygons and triangles are cut along x before a warp: at the multiples of `step`, and at positions of their own (where
/// the warp bends: a step ramp's ends).
#[derive(Clone, Debug, PartialEq)]
pub struct Cuts {
    pub step: f32,
    pub at: Vec<f32>,
}

impl Cuts {
    /// At the multiples of `step` only.
    pub fn every(step: f32) -> Self {
        Self { step, at: vec![] }
    }

    /// The slabs that cover `lo .. hi`: their bounds, in order (`lo`, the cuts strictly inside, `hi`).
    pub fn bounds(&self, lo: f32, hi: f32) -> Vec<f32> {
        let mut out = vec![lo];
        let (first, last) = ((lo / self.step).floor() as i64 + 1, (hi / self.step).ceil() as i64);
        out.extend((first..last).map(|k| k as f32 * self.step).filter(|&x| x > lo && x < hi));
        out.extend(self.at.iter().copied().filter(|&x| x > lo && x < hi));
        out.push(hi);
        out.sort_by(f32::total_cmp);
        out.dedup_by(|a, b| (*a - *b).abs() < 1e-4);
        out
    }
}

/// A convex polygon cut into slabs along x at `cuts`. Slabs with no area are left out.
pub fn slice_along_x(polygon: &[[f32; 3]], cuts: &Cuts) -> Vec<Vec<[f32; 3]>> {
    let (lo, hi) = polygon.iter().fold((f32::MAX, f32::MIN), |(lo, hi), p| (lo.min(p[0]), hi.max(p[0])));
    let bounds = cuts.bounds(lo, hi);
    let mut out = vec![];
    for w in bounds.windows(2) {
        let (a, b) = (w[0], w[1]);
        let slab = clip(&clip(polygon, |p| p[0] - a), |p| b - p[0]);
        if slab.len() >= 3 && area(&slab) > 1e-4 {
            out.push(slab);
        }
    }

    out
}

/// The part of a polygon where `side(p) >= 0` (Sutherland-Hodgman).
fn clip(polygon: &[[f32; 3]], side: impl Fn(&[f32; 3]) -> f32) -> Vec<[f32; 3]> {
    let mut out = vec![];
    let n = polygon.len();
    for i in 0..n {
        let (a, b) = (polygon[i], polygon[(i + 1) % n]);
        let (sa, sb) = (side(&a), side(&b));
        if sa >= 0.0 {
            out.push(a);
        }

        if (sa >= 0.0) != (sb >= 0.0) {
            out.push(lerp(a, b, sa / (sa - sb)));
        }
    }

    out.dedup_by(|a, b| dist2(*a, *b) < 1e-10);
    if out.len() > 1 && dist2(out[0], out[out.len() - 1]) < 1e-10 {
        out.pop();
    }

    out
}

/// A polygon's normal by Newell's method (not normalised: its length is twice the area).
pub fn newell(polygon: &[[f32; 3]]) -> [f32; 3] {
    let mut n = [0.0f32; 3];
    for i in 0..polygon.len() {
        let (a, b) = (polygon[i], polygon[(i + 1) % polygon.len()]);
        n[0] += (a[1] - b[1]) * (a[2] + b[2]);
        n[1] += (a[2] - b[2]) * (a[0] + b[0]);
        n[2] += (a[0] - b[0]) * (a[1] + b[1]);
    }

    n
}

/// A polygon's area.
pub fn area(polygon: &[[f32; 3]]) -> f32 {
    let n = newell(polygon);
    0.5 * (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt()
}

/// A unit vector.
pub fn normalised(v: [f32; 3]) -> [f32; 3] {
    let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    if l > 0.0 { [v[0] / l, v[1] / l, v[2] / l] } else { v }
}

fn lerp(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t]
}

fn dist2(a: [f32; 3], b: [f32; 3]) -> f32 {
    (a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bend_keeps_the_start_and_turns_the_end() {
        let bend = Bend::turning(0.0, 20.0, 10f32.to_radians());
        let p = bend.point([0.0, 8.0, -2.0]);
        assert!(dist2(p, [0.0, 8.0, -2.0]) < 1e-10);

        // The end's direction is turned by the angle, the centre line keeps its length.
        assert!((bend.yaw([20.0, 0.0, 0.0]) - 10f32.to_radians()).abs() < 1e-6);
        let (a, b) = (bend.point([19.99, 0.0, 0.0]), bend.point([20.0, 0.0, 0.0]));
        let d = normalised([b[0] - a[0], 0.0, b[2] - a[2]]);
        let t = turn([1.0, 0.0], 10f32.to_radians());
        assert!((d[0] - t[0]).abs() < 1e-3 && (d[2] - t[1]).abs() < 1e-3);
    }

    #[test]
    fn slices_cover_the_polygon() {
        let quad = [[0.0, 8.0, -2.4], [20.0, 8.0, -2.4], [20.0, 8.0, 2.3], [0.0, 8.0, 2.3]];
        let slabs = slice_along_x(&quad, &Cuts::every(2.0));
        assert_eq!(slabs.len(), 10);
        let total: f32 = slabs.iter().map(|s| area(s)).sum();
        assert!((total - area(&quad)).abs() < 1e-3);
    }

    #[test]
    fn cuts_are_the_multiples_and_the_extra_positions() {
        let c = Cuts { step: 5.0, at: vec![7.5, 12.0] };
        assert_eq!(c.bounds(0.0, 20.0), vec![0.0, 5.0, 7.5, 10.0, 12.0, 15.0, 20.0]);
        assert_eq!(Cuts::every(20.0).bounds(0.0, 20.0), vec![0.0, 20.0]);
    }

    #[test]
    fn subdivided_lines_keep_their_ends() {
        let line = [[0.0, 0.0, -2.5], [20.0, 0.0, -2.5]];
        let out = subdivide(&line, false, 1.0);
        assert_eq!(out.len(), 21);
        assert_eq!(out.last().unwrap().0, [20.0, 0.0, -2.5]);
        assert_eq!(subdivide(&line, true, 1.0).len(), 40);
    }
}
