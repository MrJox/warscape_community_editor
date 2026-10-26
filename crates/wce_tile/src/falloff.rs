//! A brush's falloff as a curve (TEd's Brush Falloff Editor; wce's own curve, TEd's is not reverse-engineered): the brush's weight (0..1)
//! by the distance from its centre (0 at the centre, 1 at its edge), through points the user moves. The curve goes through every point,
//! without overshooting between them (monotone cubic, Fritsch-Carlson), and is sampled into a table the brushes read.

/// Samples of the table, from the centre to the edge.
pub const SAMPLES: usize = 65;

/// The curve sampled: `Copy`, so that a brush's settings stay plain values.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FalloffLut(pub [f32; SAMPLES]);

impl FalloffLut {
    /// The weight at a distance from the centre (0..1 of the radius), linear between samples; 0 outside.
    pub fn at(&self, d: f64) -> f64 {
        if !(0.0..1.0).contains(&d) {
            return if d < 0.0 { f64::from(self.0[0]) } else { 0.0 };
        }

        let t = d * (SAMPLES - 1) as f64;
        let i = t.floor() as usize;
        let f = t - i as f64;
        f64::from(self.0[i]) * (1.0 - f) + f64::from(self.0[(i + 1).min(SAMPLES - 1)]) * f
    }
}

/// The curve's points, (distance, weight), both 0..1: sorted by distance, the first at 0 and the last at 1.
#[derive(Clone, Debug, PartialEq)]
pub struct FalloffCurve {
    pub points: Vec<[f32; 2]>,
}

/// Ready-made curves: name and points.
pub const PRESETS: [(&str, &[[f32; 2]]); 5] = [
    ("smooth", &[[0.0, 1.0], [0.5, 0.5], [1.0, 0.0]]),
    ("linear", &[[0.0, 1.0], [1.0, 0.0]]),
    ("plateau", &[[0.0, 1.0], [0.6, 0.95], [1.0, 0.0]]),
    ("spike", &[[0.0, 1.0], [0.25, 0.25], [1.0, 0.0]]),
    ("hard", &[[0.0, 1.0], [0.95, 1.0], [1.0, 0.0]]),
];

impl Default for FalloffCurve {
    fn default() -> Self {
        Self::from_points(PRESETS[0].1)
    }
}

impl FalloffCurve {
    /// The points tidied: clamped to 0..1, sorted, the ends at distance 0 and 1 (added when missing), no two at the same distance.
    pub fn from_points(points: &[[f32; 2]]) -> Self {
        let mut p = points.iter().map(|&[x, y]| [x.clamp(0.0, 1.0), y.clamp(0.0, 1.0)]).filter(|p| p[0].is_finite() && p[1].is_finite()).collect::<Vec<_>>();
        p.sort_by(|a, b| a[0].total_cmp(&b[0]));
        p.dedup_by(|b, a| (b[0] - a[0]).abs() < 1e-4);
        if p.first().is_none_or(|f| f[0] > 0.0) {
            p.insert(0, [0.0, p.first().map_or(1.0, |f| f[1])]);
        }

        if p.last().is_none_or(|l| l[0] < 1.0) {
            p.push([1.0, 0.0]);
        }

        Self { points: p }
    }

    /// The weight at a distance (0..1), through the points.
    pub fn eval(&self, d: f32) -> f32 {
        let p = &self.points;
        if p.len() < 2 {
            return p.first().map_or(1.0, |q| q[1]);
        }

        let d = d.clamp(0.0, 1.0);
        let k = p.windows(2).position(|w| d <= w[1][0]).unwrap_or(p.len() - 2);
        let (x0, y0, x1, y1) = (p[k][0], p[k][1], p[k + 1][0], p[k + 1][1]);
        let h = x1 - x0;
        if h <= 0.0 {
            return y1;
        }

        // Hermite with Fritsch-Carlson tangents: monotone between the points.
        let (m0, m1) = (self.tangent(k), self.tangent(k + 1));
        let t = (d - x0) / h;
        let (t2, t3) = (t * t, t * t * t);
        let y = (2.0 * t3 - 3.0 * t2 + 1.0) * y0 + (t3 - 2.0 * t2 + t) * h * m0 + (-2.0 * t3 + 3.0 * t2) * y1 + (t3 - t2) * h * m1;
        y.clamp(0.0, 1.0)
    }

    fn slope(&self, k: usize) -> f32 {
        let (a, b) = (self.points[k], self.points[k + 1]);
        if b[0] > a[0] { (b[1] - a[1]) / (b[0] - a[0]) } else { 0.0 }
    }

    fn tangent(&self, k: usize) -> f32 {
        let n = self.points.len();
        if k == 0 {
            return self.slope(0);
        }

        if k == n - 1 {
            return self.slope(n - 2);
        }

        let (a, b) = (self.slope(k - 1), self.slope(k));
        if a * b <= 0.0 {
            return 0.0;
        }

        // The harmonic mean keeps the piece monotone.
        2.0 * a * b / (a + b)
    }

    pub fn lut(&self) -> FalloffLut {
        let mut out = [0.0f32; SAMPLES];
        for (i, v) in out.iter_mut().enumerate() {
            *v = self.eval(i as f32 / (SAMPLES - 1) as f32);
        }

        FalloffLut(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_curve_goes_through_its_points_without_overshooting() {
        let c = FalloffCurve::from_points(&[[0.6, 0.95], [0.0, 1.0], [1.0, 0.0]]);
        for p in &c.points {
            assert!((c.eval(p[0]) - p[1]).abs() < 1e-5);
        }

        let lut = c.lut();
        assert!(lut.0.windows(2).all(|w| w[1] <= w[0] + 1e-6), "a falling curve stays falling");
        assert!(lut.0.iter().all(|&v| (0.0..=1.0).contains(&v)));
        assert_eq!(lut.at(1.0), 0.0);
        assert!((lut.at(0.0) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn missing_ends_are_added() {
        let c = FalloffCurve::from_points(&[[0.5, 0.3]]);
        assert_eq!(c.points, vec![[0.0, 0.3], [0.5, 0.3], [1.0, 0.0]]);
    }
}
