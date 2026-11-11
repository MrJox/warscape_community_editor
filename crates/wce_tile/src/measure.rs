//! Tape measures (`new_instances::MEASURE_LINE`, TEd's `TAPE_MEASURE_LINE`): the lengths of an open line of points on the ground. TEd shows
//! a `length` and an `arc_length`; here each stretch between two points, and the whole line, has its straight length (through the air),
//! its length on the map (x and z only), its length along the ground (over the terrain's rises and dips between the points) and its rise
//! (its grade from that: the rise over the length on the map). An angle finder (`new_instances::ANGLE_FINDER`, TEd's `ANGLE_FINDER_INSTANCE`)
//! is a corner between two arms: three points, the corner the middle one.

/// How far apart the ground is sampled for the length along it (metres).
pub const GROUND_STEP: f64 = 0.5;

/// The lengths of a stretch of the line, or of all of it (metres).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Lengths {
    /// Straight, through the air, from point to point.
    pub straight: f64,

    /// On the map: x and z only.
    pub horizontal: f64,

    /// Along the ground between the points.
    pub ground: f64,

    /// The height at the end less the height at the start.
    pub rise: f64,
}

impl Lengths {
    /// The grade in percent: the rise over the length on the map (0 for a stretch with no length on the map).
    pub fn grade_percent(&self) -> f64 {
        if self.horizontal > 1e-9 { 100.0 * self.rise / self.horizontal } else { 0.0 }
    }

    /// The grade in degrees (up positive).
    pub fn grade_degrees(&self) -> f64 {
        if self.horizontal > 1e-9 { self.rise.atan2(self.horizontal).to_degrees() } else { 0.0 }
    }
}

/// A line's measurements: each stretch, and the whole line (the lengths summed, the rise from the first point to the last).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Measure {
    pub stretches: Vec<Lengths>,
    pub total: Lengths,
}

/// Measures a line through `points` (world metres, x y z) over the ground `height` (metres at an x, z), sampled every [`GROUND_STEP`].
pub fn measure(points: &[[f64; 3]], height: &dyn Fn(f64, f64) -> f64) -> Measure {
    let stretches = points.windows(2).map(|w| stretch(w[0], w[1], height)).collect::<Vec<_>>();
    let mut total = stretches.iter().fold(Lengths::default(), |a, s| Lengths {
        straight: a.straight + s.straight,
        horizontal: a.horizontal + s.horizontal,
        ground: a.ground + s.ground,
        rise: 0.0,
    });

    if let (Some(first), Some(last)) = (points.first(), points.last()) {
        total.rise = last[1] - first[1];
    }

    Measure { stretches, total }
}

/// One stretch: the ground between its ends sampled every [`GROUND_STEP`] or less, the ends at their own heights.
fn stretch(a: [f64; 3], b: [f64; 3], height: &dyn Fn(f64, f64) -> f64) -> Lengths {
    let (dx, dy, dz) = (b[0] - a[0], b[1] - a[1], b[2] - a[2]);
    let horizontal = dx.hypot(dz);
    let n = ((horizontal / GROUND_STEP).ceil() as usize).max(1);
    let at = |i: usize| -> f64 {
        match i {
            0 => a[1],
            _ if i == n => b[1],
            _ => {
                let s = i as f64 / n as f64;
                height(a[0] + dx * s, a[2] + dz * s)
            }
        }
    };

    let step = horizontal / n as f64;
    let ground = (1..=n).map(|i| step.hypot(at(i) - at(i - 1))).sum();
    Lengths { straight: (horizontal * horizontal + dy * dy).sqrt(), horizontal, ground, rise: dy }
}

/// An angle finder's angle at its corner, and its two arms measured from the corner out.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Angle {
    /// Between the arms seen from above (x and z only), 0 .. 180 degrees.
    pub on_map: f64,

    /// Between the arms in space, 0 .. 180 degrees.
    pub in_space: f64,

    /// From the corner to the first point, and to the last.
    pub arms: [Lengths; 2],
}

/// The angle at the middle of three points (world metres, x y z) over the ground `height`; `None` unless there are exactly three. An arm
/// of no length makes an angle of 0.
pub fn angle(points: &[[f64; 3]], height: &dyn Fn(f64, f64) -> f64) -> Option<Angle> {
    let &[a, c, b] = points else { return None };
    let between = |u: [f64; 3], v: [f64; 3]| {
        let (lu, lv) = ((u[0] * u[0] + u[1] * u[1] + u[2] * u[2]).sqrt(), (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt());
        if lu < 1e-9 || lv < 1e-9 {
            return 0.0;
        }

        ((u[0] * v[0] + u[1] * v[1] + u[2] * v[2]) / (lu * lv)).clamp(-1.0, 1.0).acos().to_degrees()
    };

    let (u, v) = ([a[0] - c[0], a[1] - c[1], a[2] - c[2]], [b[0] - c[0], b[1] - c[1], b[2] - c[2]]);
    Some(Angle {
        on_map: between([u[0], 0.0, u[2]], [v[0], 0.0, v[2]]),
        in_space: between(u, v),
        arms: [stretch(c, a, height), stretch(c, b, height)],
    })
}

/// A length for people: centimetres under a metre, metres with two decimals under a kilometre, kilometres with three after.
pub fn format_length(m: f64) -> String {
    match m.abs() {
        a if a < 1.0 => format!("{:.0} cm", m * 100.0),
        a if a < 1000.0 => format!("{m:.2} m"),
        _ => format!("{:.3} km", m / 1000.0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-6
    }

    #[test]
    fn flat_ground() {
        let m = measure(&[[0.0, 0.0, 0.0], [3.0, 0.0, 4.0], [3.0, 0.0, 14.0]], &|_, _| 0.0);
        assert_eq!(m.stretches.len(), 2);
        assert!(close(m.stretches[0].straight, 5.0) && close(m.stretches[0].horizontal, 5.0) && close(m.stretches[0].ground, 5.0));
        assert!(close(m.total.straight, 15.0) && close(m.total.horizontal, 15.0) && close(m.total.ground, 15.0) && close(m.total.rise, 0.0));
    }

    #[test]
    fn a_slope() {
        // A plane rising 1 m per 2 m along x: every length on it is sqrt(5) / 2 of the length on the map.
        let h = |x: f64, _z: f64| x * 0.5;
        let m = measure(&[[0.0, 0.0, 0.0], [10.0, 5.0, 0.0]], &h);
        let s = m.stretches[0];
        assert!(close(s.horizontal, 10.0) && close(s.straight, 125f64.sqrt()) && close(s.ground, 125f64.sqrt()) && close(s.rise, 5.0));
        assert!(close(s.grade_percent(), 50.0) && close(s.grade_degrees(), 0.5f64.atan().to_degrees()));
    }

    #[test]
    fn a_hill_between_the_points() {
        // A 2 m high bump in the middle of a flat stretch: the ground is longer than the straight line, which does not see it.
        let h = |x: f64, _z: f64| if (4.0..=6.0).contains(&x) { 2.0 } else { 0.0 };
        let m = measure(&[[0.0, 0.0, 0.0], [10.0, 0.0, 0.0]], &h);
        let s = m.stretches[0];
        assert!(close(s.straight, 10.0) && close(s.rise, 0.0));
        assert!(s.ground > 13.0, "{}", s.ground);
    }

    #[test]
    fn total_rise_is_end_to_end() {
        let m = measure(&[[0.0, 1.0, 0.0], [5.0, 4.0, 0.0], [10.0, 2.0, 0.0]], &|_, _| 0.0);
        assert!(close(m.total.rise, 1.0));
        assert!(close(m.stretches[0].rise, 3.0) && close(m.stretches[1].rise, -2.0));
    }

    #[test]
    fn single_point_or_none() {
        assert_eq!(measure(&[], &|_, _| 0.0), Measure::default());
        let m = measure(&[[1.0, 2.0, 3.0]], &|_, _| 0.0);
        assert!(m.stretches.is_empty() && close(m.total.straight, 0.0) && close(m.total.rise, 0.0));
    }

    #[test]
    fn angles() {
        // A right angle on flat ground, then the same with one arm climbing: the map's angle stays, the one in space opens.
        let flat = angle(&[[10.0, 0.0, 0.0], [0.0, 0.0, 0.0], [0.0, 0.0, 10.0]], &|_, _| 0.0).unwrap();
        assert!(close(flat.on_map, 90.0) && close(flat.in_space, 90.0) && close(flat.arms[0].horizontal, 10.0));
        let up = angle(&[[10.0, 10.0, 0.0], [0.0, 0.0, 0.0], [0.0, 0.0, 10.0]], &|x, _| x).unwrap();
        assert!(close(up.on_map, 90.0) && close(up.in_space, 90.0), "{up:?}");
        let both = angle(&[[10.0, 10.0, 0.0], [0.0, 0.0, 0.0], [0.0, 10.0, 10.0]], &|x, z| x.max(z)).unwrap();
        assert!(close(both.on_map, 90.0) && close(both.in_space, 60.0), "{both:?}");
        assert!(close(up.arms[0].rise, 10.0) && close(up.arms[0].grade_percent(), 100.0) && close(up.arms[1].rise, 0.0));

        // Straight on, folded back, an arm of no length, the wrong count.
        assert!(close(angle(&[[-5.0, 0.0, 0.0], [0.0, 0.0, 0.0], [5.0, 0.0, 0.0]], &|_, _| 0.0).unwrap().on_map, 180.0));
        assert!(close(angle(&[[5.0, 0.0, 0.0], [0.0, 0.0, 0.0], [5.0, 0.0, 0.0]], &|_, _| 0.0).unwrap().on_map, 0.0));
        assert!(close(angle(&[[0.0, 0.0, 0.0], [0.0, 0.0, 0.0], [5.0, 0.0, 0.0]], &|_, _| 0.0).unwrap().on_map, 0.0));
        assert!(angle(&[[0.0, 0.0, 0.0], [1.0, 0.0, 0.0]], &|_, _| 0.0).is_none());
    }

    #[test]
    fn formatting() {
        assert_eq!(format_length(0.256), "26 cm");
        assert_eq!(format_length(12.345), "12.35 m");
        assert_eq!(format_length(1234.5), "1.234 km");
    }
}
