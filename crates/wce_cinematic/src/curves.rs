//! Interpolation between keys: the key pair around a time, smoothstep for numbers, and a Catmull-Rom spline through points with
//! the keys' times as its knots (Barry-Goldman), so uneven key spacing keeps an even speed through each key.

use crate::scene::Interp;

/// The keys around `t` in times sorted ascending: `(i, j, u)` with `u` 0 at key i and 1 at key j; before the first or after the
/// last key, both indices are that key's. None without keys.
pub fn around(times: &[f64], t: f64) -> Option<(usize, usize, f64)> {
    let n = times.len();
    if n == 0 {
        return None;
    }
    if t <= times[0] {
        return Some((0, 0, 0.0));
    }
    if t >= times[n - 1] {
        return Some((n - 1, n - 1, 0.0));
    }
    // The last key at or before t.
    let i = times.partition_point(|k| *k <= t) - 1;
    let j = i + 1;
    let span = times[j] - times[i];
    let u = if span > 0.0 { (t - times[i]) / span } else { 0.0 };
    Some((i, j, u))
}

pub fn smoothstep(u: f64) -> f64 {
    let u = u.clamp(0.0, 1.0);
    u * u * (3.0 - 2.0 * u)
}

/// A number between two keys by the first key's interpolation.
pub fn scalar(a: f32, b: f32, u: f64, interp: Interp) -> f32 {
    let w = match interp {
        Interp::Step => 0.0,
        Interp::Linear => u,
        Interp::Smooth => smoothstep(u),
    };
    a + (b - a) * w as f32
}

fn lerp<const N: usize>(a: [f32; N], b: [f32; N], w: f64) -> [f32; N] {
    std::array::from_fn(|k| a[k] + (b[k] - a[k]) * w as f32)
}

/// A point between keys i and j (= i + 1) of `points` at `times`, by key i's interpolation. Smooth: Catmull-Rom through the
/// neighbours, which are the end keys themselves at the ends.
pub fn point<const N: usize>(points: &[[f32; N]], times: &[f64], i: usize, j: usize, u: f64, interp: Interp) -> [f32; N] {
    if i == j {
        return points[i];
    }
    match interp {
        Interp::Step => points[i],
        Interp::Linear => lerp(points[i], points[j], u),
        Interp::Smooth => {
            let h = if i > 0 { i - 1 } else { i };
            let k = if j + 1 < points.len() { j + 1 } else { j };
            let t = times[i] + (times[j] - times[i]) * u;
            barry_goldman(points[h], points[i], points[j], points[k], times[h], times[i], times[j], times[k], t)
        }
    }
}

/// Catmull-Rom with knots at the keys' times; a missing neighbour (the same key) gets a knot one span away.
#[allow(clippy::too_many_arguments)]
fn barry_goldman<const N: usize>(
    p0: [f32; N],
    p1: [f32; N],
    p2: [f32; N],
    p3: [f32; N],
    t0: f64,
    t1: f64,
    t2: f64,
    t3: f64,
    t: f64,
) -> [f32; N] {
    let span = (t2 - t1).max(1e-9);
    let t0 = if t0 < t1 { t0 } else { t1 - span };
    let t3 = if t3 > t2 { t3 } else { t2 + span };
    let mix = |a: [f32; N], b: [f32; N], ta: f64, tb: f64| lerp(a, b, (t - ta) / (tb - ta));
    let a1 = mix(p0, p1, t0, t1);
    let a2 = mix(p1, p2, t1, t2);
    let a3 = mix(p2, p3, t2, t3);
    let b1 = mix(a1, a2, t0, t2);
    let b2 = mix(a2, a3, t1, t3);
    mix(b1, b2, t1, t2)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn around_keys() {
        let t = [0.0, 1.0, 3.0];
        assert_eq!(around(&t, -1.0), Some((0, 0, 0.0)));
        assert_eq!(around(&t, 0.5), Some((0, 1, 0.5)));
        assert_eq!(around(&t, 1.0), Some((1, 2, 0.0)));
        assert_eq!(around(&t, 2.0), Some((1, 2, 0.5)));
        assert_eq!(around(&t, 9.0), Some((2, 2, 0.0)));
        assert_eq!(around(&[], 1.0), None);
    }

    #[test]
    fn smooth_passes_through_keys() {
        let p = [[0.0f32, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]];
        let t = [0.0, 1.0, 2.5, 3.0];
        for (i, j) in [(0, 1), (1, 2), (2, 3)] {
            assert_eq!(point(&p, &t, i, j, 0.0, Interp::Smooth), p[i]);
            let end = point(&p, &t, i, j, 1.0, Interp::Smooth);
            assert!((end[0] - p[j][0]).abs() < 1e-4 && (end[1] - p[j][1]).abs() < 1e-4);
        }
    }
}
