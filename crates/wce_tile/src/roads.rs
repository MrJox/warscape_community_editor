//! Procedural roads (docs/plans/procedural_roads.md): what a road does to the terrain, from its line and settings. No map here: a road gives
//! heights, texture weights and whether a point is road, and the document writes them into the layers the road owns. The same inputs give the
//! same road, so its layers are derived data: they are made again, never edited.
//!
//! World units throughout (the tile's x, z and height).

/// A road's settings (the attributes of a `WceRoadSpline`).
#[derive(Clone, Debug, PartialEq)]
pub struct RoadSettings {
    /// Across the flat of the road.
    pub width: f64,

    /// The least width of the ground that takes the road back to the terrain, each side.
    pub shoulder: f64,

    /// How far the road stands over the smoothed ground.
    pub bump: f64,

    /// The steepest the road climbs (degrees).
    pub max_grade: f64,

    /// The steepest its banks are (degrees): a bank widens until it holds, up to [`MAX_BANK`].
    pub max_side: f64,

    /// Blend map channel of the road's texture (group * 4 + variation).
    pub channel: usize,

    /// The colour of its ground type.
    pub ground: [u8; 3],

    /// How far the texture fades out past the road's edge.
    pub feather: f64,
    pub seed: u32,
}

/// The widest a bank gets. Past this the road stands on a slope steeper than asked (a canyon: it is the bridges' business).
pub const MAX_BANK: f64 = 60.0;

/// Distance between the centre line's samples.
const SAMPLE: f64 = 1.0;

/// How far along the line the ground under it is averaged, each way.
const SMOOTHING: f64 = 15.0;

/// How far past the road's edge its height goes from the line's exact one to the blend of what is near.
const NEAR: f64 = 12.0;

/// The size of the cells nearest points are looked for in.
const CELL: f64 = 8.0;

fn smooth(t: f64) -> f64 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// The curve through a road's points, a sample about every metre: a Catmull-Rom spline (centripetal parts aside, the road needs no handles:
/// its points are where it goes). Fewer than two points: none.
#[must_use]
pub fn centre_line(points: &[[f64; 2]]) -> Vec<[f64; 2]> {
    if points.len() < 2 {
        return vec![];
    }

    let at = |i: isize| points[i.clamp(0, points.len() as isize - 1) as usize];
    let mut out = vec![];
    for i in 0..points.len() as isize - 1 {
        let (p0, p1, p2, p3) = (at(i - 1), at(i), at(i + 1), at(i + 2));
        let len = (p2[0] - p1[0]).hypot(p2[1] - p1[1]);
        let n = ((len / SAMPLE).ceil() as usize).max(1);
        for k in 0..n {
            let t = k as f64 / n as f64;
            let (t2, t3) = (t * t, t * t * t);
            let c = |a: f64, b: f64, c: f64, d: f64| 0.5 * (2.0 * b + (c - a) * t + (2.0 * a - 5.0 * b + 4.0 * c - d) * t2 + (3.0 * b - a - 3.0 * c + d) * t3);
            out.push([c(p0[0], p1[0], p2[0], p3[0]), c(p0[1], p1[1], p2[1], p3[1])]);
        }
    }

    out.push(points[points.len() - 1]);
    out
}

/// The road's height at each sample: the ground under it averaged along the line, then limited to the maximum grade both ways, and the bump.
#[must_use]
pub fn profile(line: &[[f64; 2]], ground: &dyn Fn(f64, f64) -> f64, s: &RoadSettings) -> Vec<f64> {
    if line.is_empty() {
        return vec![];
    }

    let raw: Vec<f64> = line.iter().map(|p| ground(p[0], p[1])).collect();
    let mut along = vec![0.0; line.len()];
    for i in 1..line.len() {
        along[i] = along[i - 1] + (line[i][0] - line[i - 1][0]).hypot(line[i][1] - line[i - 1][1]);
    }

    let (mut from, mut to) = (0, 0);
    let mut y = Vec::with_capacity(raw.len());
    for i in 0..raw.len() {
        while along[i] - along[from] > SMOOTHING {
            from += 1;
        }

        while to + 1 < raw.len() && along[to + 1] - along[i] <= SMOOTHING {
            to += 1;
        }

        y.push(raw[from..=to].iter().sum::<f64>() / (to - from + 1) as f64);
    }

    let slope = s.max_grade.to_radians().tan();
    for i in 1..y.len() {
        let d = (along[i] - along[i - 1]) * slope;
        y[i] = y[i].clamp(y[i - 1] - d, y[i - 1] + d);
    }

    for i in (0..y.len() - 1).rev() {
        let d = (along[i + 1] - along[i]) * slope;
        y[i] = y[i].clamp(y[i + 1] - d, y[i + 1] + d);
    }

    y.iter().map(|v| v + s.bump).collect()
}

/// A road laid out: its samples, found by position.
pub struct Strip {
    pub line: Vec<[f64; 2]>,
    pub heights: Vec<f64>,
    settings: RoadSettings,
    origin: [f64; 2],
    across: usize,
    cells: Vec<Vec<u32>>,
    reach: f64,
}

impl Strip {
    #[must_use]
    pub fn new(line: Vec<[f64; 2]>, heights: Vec<f64>, settings: RoadSettings) -> Self {
        let reach = settings.width * 0.5 + MAX_BANK.max(settings.feather) + CELL;
        let (mut lo, mut hi) = ([f64::MAX; 2], [f64::MIN; 2]);
        for p in &line {
            (lo[0], lo[1], hi[0], hi[1]) = (lo[0].min(p[0]), lo[1].min(p[1]), hi[0].max(p[0]), hi[1].max(p[1]));
        }

        let origin = [lo[0] - reach, lo[1] - reach];
        let across = (((hi[0] + reach - origin[0]) / CELL) as usize + 1).max(1);
        let down = (((hi[1] + reach - origin[1]) / CELL) as usize + 1).max(1);
        let mut cells = vec![vec![]; across * down];
        let spread = (reach / CELL).ceil() as isize;
        for (i, p) in line.iter().enumerate() {
            let (cx, cz) = (((p[0] - origin[0]) / CELL) as isize, ((p[1] - origin[1]) / CELL) as isize);
            for z in (cz - spread).max(0)..=(cz + spread).min(down as isize - 1) {
                for x in (cx - spread).max(0)..=(cx + spread).min(across as isize - 1) {
                    let cell = &mut cells[z as usize * across + x as usize];
                    if cell.len() < 4096 && (i % 4 == 0 || cell.is_empty()) {
                        cell.push(i as u32);
                    }
                }
            }
        }

        Self { line, heights, settings, origin, across, cells, reach }
    }

    /// The rectangle it can touch (x0, z0, x1, z1), past its reach nothing.
    #[must_use]
    pub fn bounds(&self) -> [f64; 4] {
        let (mut lo, mut hi) = ([f64::MAX; 2], [f64::MIN; 2]);
        for p in &self.line {
            (lo[0], lo[1], hi[0], hi[1]) = (lo[0].min(p[0]), lo[1].min(p[1]), hi[0].max(p[0]), hi[1].max(p[1]));
        }

        [lo[0] - self.reach, lo[1] - self.reach, hi[0] + self.reach, hi[1] + self.reach]
    }

    /// The distance from a point to the line and the road's height at the nearest place on it.
    fn nearest(&self, x: f64, z: f64) -> Option<(f64, f64)> {
        let (cx, cz) = ((x - self.origin[0]) / CELL, (z - self.origin[1]) / CELL);
        if cx < 0.0 || cz < 0.0 {
            return None;
        }

        let cell = self.cells.get(cz as usize * self.across + cx as usize).filter(|_| (cx as usize) < self.across)?;
        // The road's height from all of it near, by distance: taken from the nearest sample alone, it jumps where a point stops being nearer one
        // stretch of road than another, which showed as steps across the banks.
        let mut best: Option<(f64, usize)> = None;
        let (mut sum, mut weights) = (0.0, 0.0);
        for &i in cell {
            let p = self.line[i as usize];
            let d = (p[0] - x).hypot(p[1] - z);
            if best.is_none_or(|b| d < b.0) {
                best = Some((d, i as usize));
            }

            let w = 1.0 / (d * d + 0.01).powi(3);
            sum += self.heights[i as usize] * w;
            weights += w;
        }

        // The cells keep every fourth sample: the rest of the way between the neighbours of the best one.
        let (_, i) = best?;
        let (mut near, mut own) = (f64::MAX, 0.0);
        for k in i.saturating_sub(4)..(i + 5).min(self.line.len()) {
            let p = self.line[k];
            let d = (p[0] - x).hypot(p[1] - z);
            if d < near {
                (near, own) = (d, self.heights[k]);
            }
        }

        // Near the road its exact height, for a surface with no ripple from the samples the cells left out; the jumps are further out.
        let far = smooth((near - self.settings.width * 0.5) / NEAR);
        Some((near, own + (sum / weights - own) * far))
    }

    /// The terrain's height at a point once the road is there, `ground` its height without; none where the road leaves it alone. On the road
    /// the road's own height. Beyond its edge a bank back to the ground, wide enough to stay under the maximum side slope.
    #[must_use]
    pub fn height_at(&self, x: f64, z: f64, ground: f64) -> Option<f64> {
        let s = &self.settings;
        let (d, y) = self.nearest(x, z)?;
        let past = d - s.width * 0.5;
        if past <= 0.0 {
            return Some(y);
        }

        // The steepest part of `smooth` is 1.5 times the mean slope.
        let rise = (ground - y).abs();
        let bank = (rise * 1.5 / s.max_side.to_radians().tan()).max(s.shoulder).min(MAX_BANK);
        (past < bank).then(|| y + (ground - y) * smooth(past / bank))
    }

    /// How much of the road's texture a point takes: all of it on the road, fading out past the edge, the edge broken up by noise.
    #[must_use]
    pub fn texture_at(&self, x: f64, z: f64) -> f32 {
        let s = &self.settings;
        let Some((d, _)) = self.nearest(x, z) else { return 0.0 };
        let edge = s.width * 0.5 + (noise(x / 5.0, z / 5.0, s.seed) - 0.5) * s.feather;
        (1.0 - smooth((d - edge) / s.feather.max(0.01) + 0.5)) as f32
    }

    /// Whether a point is road, for the ground type.
    #[must_use]
    pub fn on_road(&self, x: f64, z: f64) -> bool {
        self.nearest(x, z).is_some_and(|(d, _)| d <= self.settings.width * 0.5)
    }
}

/// Smooth noise, 0 to 1: values at a lattice's points blended between.
fn noise(x: f64, z: f64, seed: u32) -> f64 {
    let at = |ix: i64, iz: i64| {
        let mut h = (ix as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ (iz as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F) ^ u64::from(seed).wrapping_mul(0x1656_67B1_9E37_79F9);
        h ^= h >> 29;
        h = h.wrapping_mul(0xBF58_476D_1CE4_E5B9);
        h ^= h >> 32;
        (h & 0xFFFF) as f64 / 65535.0
    };

    let (ix, iz) = (x.floor(), z.floor());
    let (fx, fz) = (smooth(x - ix), smooth(z - iz));
    let (ix, iz) = (ix as i64, iz as i64);
    let top = at(ix, iz) + (at(ix + 1, iz) - at(ix, iz)) * fx;
    let bottom = at(ix, iz + 1) + (at(ix + 1, iz + 1) - at(ix, iz + 1)) * fx;
    top + (bottom - top) * fz
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings() -> RoadSettings {
        RoadSettings { width: 6.0, shoulder: 3.0, bump: 0.25, max_grade: 10.0, max_side: 30.0, channel: 5, ground: [255, 96, 0], feather: 2.0, seed: 1 }
    }

    fn straight() -> Vec<[f64; 2]> {
        centre_line(&[[0.0, 0.0], [100.0, 0.0]])
    }

    #[test]
    fn the_line_goes_through_its_points_about_every_metre() {
        let l = centre_line(&[[0.0, 0.0], [50.0, 0.0], [50.0, 50.0]]);
        assert_eq!(l[0], [0.0, 0.0]);
        assert_eq!(*l.last().unwrap(), [50.0, 50.0]);
        assert!(l.iter().any(|p| (p[0] - 50.0).abs() < 1e-9 && p[1].abs() < 1e-9));
        assert!(l.windows(2).all(|w| (w[0][0] - w[1][0]).hypot(w[0][1] - w[1][1]) < 2.0));
        assert!(centre_line(&[[1.0, 1.0]]).is_empty());
    }

    #[test]
    fn flat_ground_gives_a_flat_road_at_its_bump() {
        let y = profile(&straight(), &|_, _| 10.0, &settings());
        assert!(y.iter().all(|v| (v - 10.25).abs() < 1e-9));
    }

    #[test]
    fn a_cliff_in_the_ground_is_climbed_at_the_maximum_grade() {
        let l = straight();
        let s = settings();
        let y = profile(&l, &|x, _| if x < 50.0 { 0.0 } else { 40.0 }, &s);
        let limit = s.max_grade.to_radians().tan() * 1.0001;
        for (w, p) in y.windows(2).zip(l.windows(2)) {
            let run = (p[1][0] - p[0][0]).hypot(p[1][1] - p[0][1]);
            assert!((w[1] - w[0]).abs() <= limit * run, "{} to {} over {run}", w[0], w[1]);
        }
    }

    #[test]
    fn the_road_is_flat_across_and_the_bank_meets_the_ground() {
        let l = straight();
        let s = settings();
        let y = profile(&l, &|_, _| 0.0, &s);
        let strip = Strip::new(l, y, s);

        // On the road its height, whatever the ground.
        assert!((strip.height_at(50.0, 2.9, 5.0).unwrap() - 0.25).abs() < 1e-9);

        // Partway down the bank, between the two.
        let h = strip.height_at(50.0, 4.5, 0.0).unwrap();
        assert!(h > 0.0 && h < 0.25, "{h}");

        // Past it, nothing.
        assert!(strip.height_at(50.0, 7.0, 0.0).is_none());
    }

    #[test]
    fn a_bank_widens_to_hold_the_maximum_side_slope() {
        let l = straight();
        let s = settings();
        let y = profile(&l, &|_, _| 0.0, &s);
        let strip = Strip::new(l, y, s.clone());

        // A 20 high step at the road's side: sampled down the bank, no step is steeper than asked.
        let ground = 20.0;
        let limit = s.max_side.to_radians().tan() * 1.02;
        let mut last = strip.height_at(50.0, 3.0, ground).unwrap();
        let mut z = 3.1;
        while z < 3.0 + MAX_BANK {
            let h = strip.height_at(50.0, z, ground).unwrap_or(ground);
            assert!((h - last).abs() / 0.1 <= limit, "at {z}: {last} to {h}");
            last = h;
            z += 0.1;
        }
    }

    #[test]
    fn the_texture_fades_out_past_the_edge() {
        let l = straight();
        let s = settings();
        let y = profile(&l, &|_, _| 0.0, &s);
        let strip = Strip::new(l, y, s);
        assert_eq!(strip.texture_at(50.0, 0.0), 1.0);
        assert_eq!(strip.texture_at(50.0, 6.0), 0.0);
        assert!(strip.on_road(50.0, 2.9) && !strip.on_road(50.0, 3.1));
    }

    #[test]
    fn the_terrain_a_road_leaves_has_no_steps() {
        let l = centre_line(&[[30.0, 60.0], [90.0, 110.0], [150.0, 90.0], [215.0, 150.0]]);
        let s = RoadSettings { width: 8.0, ..settings() };
        let ground = |x: f64, z: f64| (x * 0.05).sin() * 8.0 + (z * 0.07).cos() * 8.0 + 20.0;
        let y = profile(&l, &ground, &s);
        let strip = Strip::new(l, y, s);
        let at = |x: f64, z: f64| { let g = ground(x, z); strip.height_at(x, z, g).unwrap_or(g) };
        let mut worst = (0.0, 0.0, 0.0);
        for j in 0..2400 {
            for i in 0..2400 {
                let (x, z) = (f64::from(i) * 0.1, f64::from(j) * 0.1);
                let d = (at(x, z) - at(x + 0.1, z)).abs();
                if d > worst.0 {
                    worst = (d, x, z);
                }
            }
        }

        assert!(worst.0 < 0.5, "a step of {:.2} at {:.1}, {:.1}", worst.0, worst.1, worst.2);
    }

    #[test]
    fn the_same_road_comes_out_the_same() {
        let run = || {
            let l = straight();
            let y = profile(&l, &|x, z| (x * 0.1).sin() * 3.0 + z, &settings());
            let strip = Strip::new(l, y, settings());
            (0..200).map(|i| strip.texture_at(f64::from(i) * 0.5, 3.5)).collect::<Vec<_>>()
        };

        assert_eq!(run(), run());
    }
}
