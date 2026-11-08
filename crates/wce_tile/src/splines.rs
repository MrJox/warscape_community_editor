//! The splines of tile.ted: rivers (`SplineRiverInstance`) and roads (`SEGMENTED_SPLINE_INSTANCE`). Each point has a `position` and two
//! Bezier handles, `controlIn` and `controlOut`, all local to the instance; segment i -> i + 1 is the cubic Bezier through point i, its
//! `controlOut`, point i + 1's `controlIn` and point i + 1 (as `river_files` writes it for the export).
//!
//! As TEd writes them (the kit's 254 rivers and 4 roads): the first point is at the instance's position (`0,0,0`), the handles are at their
//! point's height and mirror each other around it, and the handles no segment uses (the first point's `controlIn`, the last one's
//! `controlOut`) are `0,0,0`.

use wce_foundation::math::Double3;

use crate::ted_scene::{TedScene, parse_vec3};

/// The spline types.
pub const SPLINE_KINDS: [&str; 2] = ["SplineRiverInstance", "SEGMENTED_SPLINE_INSTANCE"];

pub fn is_spline(kind: &str) -> bool {
    SPLINE_KINDS.contains(&kind)
}

/// The editor's own splines (procedural roads and walls): polygons whose points may carry handles as TEd's splines do (user 2026-11-08:
/// to turn a point, not only move it). A handle not written (missing or empty) is the Catmull-Rom one ([`catmull_handles`]), the curve
/// they had before handles: the road's exactly, the wall's (centripetal) near it.
pub const OWN_KINDS: [&str; 2] = [crate::new_instances::ROAD_SPLINE, crate::new_instances::WALL_SPLINE];

/// Whether an element's points have handles: TEd's splines and the editor's own.
pub fn has_handles(kind: &str) -> bool {
    is_spline(kind) || OWN_KINDS.contains(&kind)
}

/// The handles written on a spline's points (local; `controlIn`, `controlOut`), `None` where not.
pub fn written(scene: &TedScene, elem: usize) -> Vec<[Option<Double3>; 2]> {
    let doc = scene.doc();
    scene.polygon_points(elem).into_iter().filter(|&p| doc.get(p, "position").is_some()).map(|p| {
        let vec = |name: &str| doc.get(p, name).and_then(|v| parse_vec3(&v, ""));
        [vec("controlIn"), vec("controlOut")]
    }).collect()
}

/// The handles of point `i` that make the curve a uniform Catmull-Rom through the points: a sixth of the way from the previous point to
/// the next on each side (the ends: of their one segment).
pub fn catmull_handles(positions: &[Double3], i: usize) -> (Double3, Double3) {
    let last = positions.len().saturating_sub(1);
    let (prev, next) = (positions[i.saturating_sub(1)], positions[(i + 1).min(last)]);
    let d = (next - prev) * (1.0 / 6.0);
    (positions[i] - d, positions[i] + d)
}

/// A point of a spline, local to it.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SplinePoint {
    pub position: Double3,
    pub control_in: Double3,
    pub control_out: Double3,
}

/// The points of a spline element (a handle that is missing or can't be read is the point itself; on the editor's own splines, the
/// Catmull-Rom one).
pub fn read(scene: &TedScene, elem: usize) -> Vec<SplinePoint> {
    let doc = scene.doc();
    let mut pts = scene.polygon_points(elem).into_iter().filter_map(|p| {
        let vec = |name: &str| doc.get(p, name).and_then(|v| parse_vec3(&v, ""));
        let position = vec("position")?;
        Some(SplinePoint { position, control_in: vec("controlIn").unwrap_or(position), control_out: vec("controlOut").unwrap_or(position) })
    }).collect::<Vec<_>>();

    if OWN_KINDS.contains(&scene.type_of(elem)) {
        let positions = pts.iter().map(|p| p.position).collect::<Vec<_>>();
        for (i, w) in written(scene, elem).into_iter().enumerate().take(pts.len()) {
            let (cin, cout) = catmull_handles(&positions, i);
            pts[i].control_in = w[0].unwrap_or(cin);
            pts[i].control_out = w[1].unwrap_or(cout);
        }
    }

    pts
}

/// The point of a cubic Bezier at `t`.
pub fn bezier(p0: Double3, c0: Double3, c1: Double3, p1: Double3, t: f64) -> Double3 {
    let s = 1.0 - t;
    p0 * (s * s * s) + c0 * (3.0 * s * s * t) + c1 * (3.0 * s * t * t) + p1 * (t * t * t)
}

/// The curve through the points, `steps` samples per segment (and the closing segment when `closed`).
pub fn curve(points: &[SplinePoint], closed: bool, steps: usize) -> Vec<Double3> {
    let steps = steps.max(1);
    let n = points.len();
    let segments = if closed && n > 2 { n } else { n.saturating_sub(1) };
    let mut out = Vec::with_capacity(segments * steps + 1);
    if let Some(first) = points.first() {
        out.push(first.position);
    }

    for i in 0..segments {
        let (a, b) = (&points[i], &points[(i + 1) % n]);
        for k in 1..=steps {
            out.push(bezier(a.position, a.control_out, b.control_in, b.position, k as f64 / steps as f64));
        }
    }

    out
}

fn flat_length(d: Double3) -> f64 {
    (d.x * d.x + d.z * d.z).sqrt()
}

/// The handles of point `i` from its neighbours: along the line from the previous point to the next one (only its own segment at the ends),
/// at the point's height, a third of the mean length of its segments away. The ends' unused handles are `0,0,0`.
pub fn auto_handles(positions: &[Double3], i: usize) -> (Double3, Double3) {
    let p = positions[i];
    let (prev, next) = (i.checked_sub(1).map(|j| positions[j]), positions.get(i + 1).copied());
    let (dir, len) = match (prev, next) {
        (Some(a), Some(b)) => (b - a, (flat_length(p - a) + flat_length(b - p)) / 2.0),
        (None, Some(b)) => (b - p, flat_length(b - p)),
        (Some(a), None) => (p - a, flat_length(p - a)),
        (None, None) => return (Double3::ZERO, Double3::ZERO),
    };

    let dl = flat_length(dir);
    let off = if dl > 1e-9 { Double3::new(dir.x / dl, 0.0, dir.z / dl) * (len / 3.0) } else { Double3::ZERO };
    let control_in = if prev.is_some() { p - off } else { Double3::ZERO };
    let control_out = if next.is_some() { p + off } else { Double3::ZERO };
    (control_in, control_out)
}

/// A new spline's points from its positions, every handle made by [`auto_handles`].
pub fn new_points(positions: &[Double3]) -> Vec<SplinePoint> {
    (0..positions.len()).map(|i| {
        let (control_in, control_out) = auto_handles(positions, i);
        SplinePoint { position: positions[i], control_in, control_out }
    }).collect()
}

/// The handle mirroring `handle` around `position`.
pub fn mirror(position: Double3, handle: Double3) -> Double3 {
    position * 2.0 - handle
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handles_are_flat_mirrored_and_the_ends_unused_are_zero() {
        let pts = [Double3::new(0.0, 0.0, 0.0), Double3::new(0.0, 3.0, 30.0), Double3::new(30.0, 0.0, 30.0)];
        let s = new_points(&pts);
        assert_eq!(s[0].control_in, Double3::ZERO);
        assert_eq!(s[2].control_out, Double3::ZERO);
        assert!((s[0].control_out.z - 10.0).abs() < 1e-9 && s[0].control_out.x.abs() < 1e-9, "{:?}", s[0]);
        assert_eq!(s[1].control_in.y, 3.0, "at the point's height");
        assert!((mirror(s[1].position, s[1].control_in) - s[1].control_out).max_abs_diff(Double3::ZERO) < 1e-9, "mirrored");

        let c = curve(&s, false, 4);
        assert_eq!(c.len(), 9);
        assert_eq!(c[0], pts[0]);
        assert!(c[4].max_abs_diff(pts[1]) < 1e-9 && c[8].max_abs_diff(pts[2]) < 1e-9, "through the points");
    }
}
