//! Procedural roads (docs/plans/procedural_roads.md): what a road does to the terrain, from its line and settings. No map here: a road gives
//! heights, texture weights and whether a point is road, and the document writes them into the layers the road owns. The same inputs give the
//! same road, so its layers are derived data: they are made again, never edited.
//!
//! World units throughout (the tile's x, z and height), but for the bridges, whose pieces are in the file's.

use wce_foundation::math::{Double3, Double3x3};

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

    /// Whether it crosses gaps on bridges.
    pub bridges: bool,

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

/// How far under the road the ground must be to be a gap, and over how long a stretch (world units).
const GAP_DEPTH: f64 = 4.0;
const GAP_LENGTH: f64 = 8.0;

/// Over how much of the road its bump comes in: from its ends, and from another road it meets.
const BUMP_RAMP: f64 = 8.0;

/// A chain of the kit's bridge pieces. From their logic files: a piece's platform runs along its z, `pitch` long, at height 1, and an end's is a
/// ramp `RAMP` long from height 0 at z = -`RAMP` up to height 1 at its origin. Pieces join because their platforms meet edge to edge.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Chain {
    piece: &'static str,
    end: &'static str,
    pitch: f64,
}

const NARROW: Chain = Chain { piece: "bridge_wood_1", end: "bridge_wood_1_end", pitch: 10.0 };
const WIDE: Chain = Chain { piece: "bridge_wood_2", end: "bridge_wood_2_end", pitch: 20.0 };

/// Across a `WIDE` chain's platform.
const WIDE_WIDTH: f64 = 10.0;
const RAMP: f64 = 2.0;

/// `bridge_wood_complete`, one building and one walkable surface: where its ramps meet the ground, (height, along its z), and the gaps it fits.
const WHOLE: &str = "bridge_wood_complete";
const WHOLE_FEET: [(f64, f64); 2] = [(-3.7, -58.49), (-3.4, 57.97)];
const WHOLE_GAPS: (f64, f64) = (60.0, 104.0);

/// How a gap is crossed.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Bridge {
    Whole,
    Pieces(Chain, usize),
}

/// A bridge piece, placed.
#[derive(Clone, Debug, PartialEq)]
pub struct BridgePiece {
    pub key: &'static str,
    pub position: [f64; 3],
    pub rotation: Double3x3,
}

/// The bridge for a gap and a road's width (the file's units), and how far apart its feet are.
#[must_use]
pub fn choose_bridge(gap: f64, width: f64) -> (Bridge, f64) {
    if (WHOLE_GAPS.0..=WHOLE_GAPS.1).contains(&gap) && width >= WIDE_WIDTH {
        let [(ya, a), (yb, b)] = WHOLE_FEET;
        return (Bridge::Whole, (b - a).hypot(yb - ya));
    }

    let chain = if width >= WIDE_WIDTH { WIDE } else { NARROW };
    let n = ((gap / chain.pitch).ceil() as usize).max(1);
    (Bridge::Pieces(chain, n), n as f64 * chain.pitch + 2.0 * RAMP)
}

/// The rotation that takes a piece's +z along `d` (a unit vector), its x level and its y up.
fn along(d: [f64; 3]) -> Double3x3 {
    let flat = d[0].hypot(d[2]).max(1e-9);
    let x = [d[2] / flat, 0.0, -d[0] / flat];
    let y = [d[1] * x[2] - d[2] * x[1], d[2] * x[0] - d[0] * x[2], d[0] * x[1] - d[1] * x[0]];
    let y = if y[1] < 0.0 { y.map(|v| -v) } else { y };
    Double3x3 { m: [[x[0], y[0], d[0]], [x[1], y[1], d[1]], [x[2], y[2], d[2]]] }
}

/// Lays a bridge from one foot to the other (where its ramps meet the road; the file's units, as far apart as [`choose_bridge`] says). The whole
/// bridge lies on the straight line between them, level or not, so every platform meets the next edge to edge: one walkable surface, ground to ground.
#[must_use]
pub fn lay_bridge(bridge: Bridge, a: [f64; 3], b: [f64; 3]) -> Vec<BridgePiece> {
    let delta = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
    let length = (delta[0] * delta[0] + delta[1] * delta[1] + delta[2] * delta[2]).sqrt().max(1e-9);
    let d = delta.map(|v| v / length);
    let forward = along(d);
    let back = along(d.map(|v| -v));
    let on = |distance: f64| [a[0] + d[0] * distance, a[1] + d[1] * distance, a[2] + d[2] * distance];
    match bridge {
        Bridge::Whole => {
            // Its feet are not level in its own frame: the line between them is turned by that first.
            let [(ya, za), (yb, zb)] = WHOLE_FEET;
            let own = (yb - ya).atan2(zb - za);
            let flat = d[0].hypot(d[2]).max(1e-9);
            let tilt = d[1].atan2(flat) - own;
            let axis = [d[0] / flat * tilt.cos(), tilt.sin(), d[2] / flat * tilt.cos()];
            let rotation = along(axis);
            let foot = rotation * Double3::new(0.0, ya, za);
            vec![BridgePiece { key: WHOLE, position: [a[0] - foot.x, a[1] - foot.y, a[2] - foot.z], rotation }]
        }

        Bridge::Pieces(chain, n) => {
            let mut out = vec![BridgePiece { key: chain.end, position: on(RAMP), rotation: forward }];
            out.extend((0..n).map(|i| BridgePiece { key: chain.piece, position: on(RAMP + chain.pitch * (i as f64 + 0.5)), rotation: forward }));
            out.push(BridgePiece { key: chain.end, position: on(RAMP + chain.pitch * n as f64), rotation: back });
            out
        }
    }
}

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
    centre_line_sharp(points, &[]).0
}

/// As [`centre_line`], with the points marked `sharp` corners: the curve comes into one straight from each side, as if the road ended
/// there, so a corner piece's arms lie on it. Also the sample each point is at.
#[must_use]
pub fn centre_line_sharp(points: &[[f64; 2]], sharp: &[bool]) -> (Vec<[f64; 2]>, Vec<usize>) {
    if points.len() < 2 {
        return (vec![], vec![]);
    }

    let last = points.len() as isize - 1;
    let is_sharp = |i: isize| sharp.get(i as usize).copied().unwrap_or(false);
    let at = |i: isize| points[i.clamp(0, last) as usize];
    let mut out = vec![];
    let mut at_point = vec![];
    for i in 0..last {
        let (p1, p2) = (at(i), at(i + 1));
        let p0 = if is_sharp(i) { p1 } else { at(i - 1) };
        let p3 = if is_sharp(i + 1) { p2 } else { at(i + 2) };
        let len = (p2[0] - p1[0]).hypot(p2[1] - p1[1]);
        let n = ((len / SAMPLE).ceil() as usize).max(1);
        at_point.push(out.len());
        for k in 0..n {
            let t = k as f64 / n as f64;
            let (t2, t3) = (t * t, t * t * t);
            let c = |a: f64, b: f64, c: f64, d: f64| 0.5 * (2.0 * b + (c - a) * t + (2.0 * a - 5.0 * b + 4.0 * c - d) * t2 + (3.0 * b - a - 3.0 * c + d) * t3);
            out.push([c(p0[0], p1[0], p2[0], p3[0]), c(p0[1], p1[1], p2[1], p3[1])]);
        }
    }

    at_point.push(out.len());
    out.push(points[points.len() - 1]);
    (out, at_point)
}

/// Which of a road's points are corners: those marked so, and those it turns at by 90 degrees or more (an inside angle of 90 or less). Its
/// ends are not.
#[must_use]
pub fn sharp_points(points: &[[f64; 2]], marked: &[bool]) -> Vec<bool> {
    (0..points.len()).map(|i| {
        if i == 0 || i + 1 >= points.len() {
            return false;
        }

        let (a, b, c) = (points[i - 1], points[i], points[i + 1]);
        let (u, v) = ([b[0] - a[0], b[1] - a[1]], [c[0] - b[0], c[1] - b[1]]);
        let turn = (u[0] * v[1] - u[1] * v[0]).atan2(u[0] * v[0] + u[1] * v[1]).abs();
        marked.get(i).copied().unwrap_or(false) || turn >= std::f64::consts::FRAC_PI_2 - 1e-9
    }).collect()
}

/// The road's height at each sample: the ground under it averaged along the line, then limited to the maximum grade both ways, and the bump.
#[must_use]
pub fn profile(line: &[[f64; 2]], ground: &dyn Fn(f64, f64) -> f64, s: &RoadSettings) -> Vec<f64> {
    let raw: Vec<f64> = line.iter().map(|p| ground(p[0], p[1])).collect();
    with_bump(line, &graded(line, &raw, s), s, &|_, _| false)
}

/// The stretches of a line (first and last sample) where the ground is well under the road: gaps for a bridge. `raw`: the ground under each
/// sample; `heights`: the road's. A stretch runs out to where the ground comes back up to the road, and on up the walls while they are steeper
/// than the road may climb: a road following the ground down has met it partway down the wall, and a bridge from there would start in the canyon.
#[must_use]
pub fn find_gaps(line: &[[f64; 2]], raw: &[f64], heights: &[f64], s: &RoadSettings) -> Vec<(usize, usize)> {
    let slope = s.max_grade.to_radians().tan();
    let climbs = |from: usize, to: usize| {
        let run = (line[to][0] - line[from][0]).hypot(line[to][1] - line[from][1]).max(1e-9);
        (raw[to] - raw[from]) / run > slope
    };

    let mut out: Vec<(usize, usize)> = vec![];
    let mut i = 0;
    while i < raw.len() {
        if heights[i] - raw[i] < GAP_DEPTH {
            i += 1;
            continue;
        }

        let (mut a, mut b) = (i, i);
        while a > 0 && heights[a - 1] - raw[a - 1] > 1.0 {
            a -= 1;
        }

        while b + 1 < raw.len() && heights[b + 1] - raw[b + 1] > 1.0 {
            b += 1;
        }

        while a > 0 && climbs(a, a - 1) {
            a -= 1;
        }

        while b + 1 < raw.len() && climbs(b, b + 1) {
            b += 1;
        }

        let length = line[a..=b].windows(2).map(|w| (w[1][0] - w[0][0]).hypot(w[1][1] - w[0][1])).sum::<f64>();
        if length >= GAP_LENGTH && a > 0 && b + 1 < raw.len() && out.last().is_none_or(|l| l.1 < a) {
            out.push((a, b));
        }

        i = b + 1;
    }

    out
}

/// The ground with its gaps taken out: straight from rim to rim, for the road to be graded as if they were not there, and not dip into them.
#[must_use]
pub fn over_gaps(raw: &[f64], gaps: &[(usize, usize)]) -> Vec<f64> {
    let mut out = raw.to_vec();
    for &(a, b) in gaps {
        let n = (b - a).max(1) as f64;
        for (k, v) in out[a..=b].iter_mut().enumerate() {
            *v = raw[a] + (raw[b] - raw[a]) * k as f64 / n;
        }
    }

    out
}

/// Adds the bump. It comes in from the road's ends, so that a road ending at a tile's edge ends on the ground, and it stays off where the
/// road runs on another (`on_other`): that one's bump is already in the ground.
#[must_use]
pub fn with_bump(line: &[[f64; 2]], heights: &[f64], s: &RoadSettings, on_other: &dyn Fn(f64, f64) -> bool) -> Vec<f64> {
    let n = heights.len();
    let reach = BUMP_RAMP as usize;
    let off = line.iter().map(|p| on_other(p[0], p[1])).collect::<Vec<_>>();
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let ends = (i.min(n - 1 - i) as f64 / BUMP_RAMP).min(1.0);
        let near = (i.saturating_sub(reach)..(i + reach + 1).min(n)).filter(|&k| off[k]).map(|k| k.abs_diff(i) as f64 / BUMP_RAMP).fold(1.0, f64::min);
        out.push(heights[i] + s.bump * smooth(ends.min(near)));
    }

    out
}

/// The ground under a line averaged along it, then limited to the maximum grade both ways.
#[must_use]
pub fn graded(line: &[[f64; 2]], raw: &[f64], s: &RoadSettings) -> Vec<f64> {
    if line.is_empty() {
        return vec![];
    }

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

    y
}

/// A road laid out: its samples, found by position.
pub struct Strip {
    pub line: Vec<[f64; 2]>,
    pub heights: Vec<f64>,
    settings: RoadSettings,

    /// The samples a bridge carries: the road leaves the terrain under them as it is.
    open: Vec<bool>,
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

        let open = vec![false; line.len()];
        Self { line, heights, settings, open, origin, across, cells, reach }
    }

    /// Leaves the terrain alone over these stretches (first and last sample).
    pub fn open_gaps(&mut self, gaps: &[(usize, usize)]) {
        for &(a, b) in gaps {
            self.open[a..=b].fill(true);
        }
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
        self.nearest_open(x, z).filter(|n| !n.2).map(|n| (n.0, n.1))
    }

    /// As [`Self::nearest`], and whether the place on the line is over a gap.
    fn nearest_open(&self, x: f64, z: f64) -> Option<(f64, f64, bool)> {
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
        let (mut near, mut own, mut open) = (f64::MAX, 0.0, false);
        for k in i.saturating_sub(4)..(i + 5).min(self.line.len()) {
            let p = self.line[k];
            let d = (p[0] - x).hypot(p[1] - z);
            if d < near {
                (near, own, open) = (d, self.heights[k], self.open[k]);
            }
        }

        // Near the road its exact height, for a surface with no ripple from the samples the cells left out; the jumps are further out.
        let far = smooth((near - self.settings.width * 0.5) / NEAR);
        Some((near, own + (sum / weights - own) * far, open))
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
        RoadSettings { width: 6.0, shoulder: 3.0, bump: 0.25, max_grade: 10.0, max_side: 30.0, channel: 5, ground: [255, 96, 0], bridges: true, feather: 2.0, seed: 1 }
    }

    #[test]
    fn a_canyon_is_a_gap_and_a_ditch_is_not() {
        let l = straight();
        let s = settings();
        let raw = |depth: f64, a: f64, b: f64| l.iter().map(|p| if p[0] > a && p[0] < b { -depth } else { 0.0 }).collect::<Vec<_>>();
        let canyon = raw(30.0, 40.0, 60.0);
        let gaps = find_gaps(&l, &canyon, &graded(&l, &over_gaps(&canyon, &[(30, 70)]), &s), &s);
        assert_eq!(gaps.len(), 1);
        let (a, b) = gaps[0];
        assert!(l[a][0] <= 41.0 && l[a][0] >= 38.0 && l[b][0] >= 59.0 && l[b][0] <= 62.0, "{:?} {:?}", l[a], l[b]);

        // Shallow, or short: the road just goes over it.
        let ditch = raw(2.0, 40.0, 60.0);
        assert!(find_gaps(&l, &ditch, &vec![0.0; l.len()], &s).is_empty());
        let notch = raw(30.0, 49.0, 52.0);
        assert!(find_gaps(&l, &notch, &vec![0.0; l.len()], &s).is_empty());
    }

    #[test]
    fn a_gap_runs_to_the_top_of_its_walls() {
        let l = straight();
        let s = settings();

        // Walls 30 high over 30 units each side, a flat floor between them.
        let raw = l.iter().map(|p| {
            let d = (p[0] - 50.0).abs();
            if d < 10.0 { -30.0 } else if d < 40.0 { -30.0 + d - 10.0 } else { 0.0 }
        }).collect::<Vec<_>>();
        let gaps = find_gaps(&l, &raw, &graded(&l, &raw, &s), &s);
        assert_eq!(gaps.len(), 1, "{gaps:?}");
        let (a, b) = gaps[0];
        assert!(raw[a] > -2.0 && raw[b] > -2.0, "ends at {} and {}", raw[a], raw[b]);
    }

    #[test]
    fn a_road_stays_up_over_a_gap() {
        let raw = [0.0, 0.0, -30.0, -30.0, 2.0, 2.0];
        assert_eq!(over_gaps(&raw, &[(1, 4)]), [0.0, 0.0, 2.0 / 3.0, 4.0 / 3.0, 2.0, 2.0]);
    }

    #[test]
    fn the_terrain_under_a_bridge_stays() {
        let l = straight();
        let s = settings();
        let y = vec![10.0; l.len()];
        let mut strip = Strip::new(l, y, s);
        strip.open_gaps(&[(40, 60)]);
        assert!(strip.height_at(50.0, 0.0, -20.0).is_none());
        assert_eq!(strip.texture_at(50.0, 0.0), 0.0);
        assert!(!strip.on_road(50.0, 0.0));
        assert_eq!(strip.height_at(20.0, 0.0, -20.0), Some(10.0));
    }

    /// A point of a piece, in the world.
    fn world(p: &BridgePiece, local: [f64; 3]) -> [f64; 3] {
        let v = p.rotation * Double3::new(local[0], local[1], local[2]);
        [p.position[0] + v.x, p.position[1] + v.y, p.position[2] + v.z]
    }

    fn close(a: [f64; 3], b: [f64; 3]) -> bool {
        (0..3).all(|k| (a[k] - b[k]).abs() < 1e-6)
    }

    #[test]
    fn a_chain_is_one_surface_from_foot_to_foot() {
        for (rise, gap) in [(0.0, 30.0), (6.0, 47.0), (-9.0, 130.0)] {
            let (bridge, length) = choose_bridge(gap, 12.0);
            let Bridge::Pieces(chain, n) = bridge else { panic!("a chain") };
            assert!(length >= gap, "{length} for {gap}");
            let run = (length * length - rise * rise).sqrt();
            let (a, b) = ([0.0, 5.0, 0.0], [run * 0.6, 5.0 + rise, run * 0.8]);
            let p = lay_bridge(bridge, a, b);
            assert_eq!(p.len(), n + 2);

            // The ramps start on the feet.
            assert!(close(world(&p[0], [0.0, 0.0, -RAMP]), a), "{:?}", world(&p[0], [0.0, 0.0, -RAMP]));
            assert!(close(world(&p[n + 1], [0.0, 0.0, -RAMP]), b), "{:?}", world(&p[n + 1], [0.0, 0.0, -RAMP]));

            // Every platform's edge is the next one's.
            assert!(close(world(&p[0], [0.0, 1.0, 0.0]), world(&p[1], [0.0, 1.0, -chain.pitch * 0.5])));
            for w in p[1..=n].windows(2) {
                assert!(close(world(&w[0], [0.0, 1.0, chain.pitch * 0.5]), world(&w[1], [0.0, 1.0, -chain.pitch * 0.5])));
            }

            assert!(close(world(&p[n], [0.0, 1.0, chain.pitch * 0.5]), world(&p[n + 1], [0.0, 1.0, 0.0])));
            assert!(p.iter().all(|q| (q.rotation * Double3::new(0.0, 1.0, 0.0)).y > 0.9), "upright");
        }
    }

    #[test]
    fn a_gap_of_its_size_takes_the_whole_bridge_and_its_feet_touch() {
        let (bridge, length) = choose_bridge(90.0, 12.0);
        assert_eq!(bridge, Bridge::Whole);
        let (a, b) = ([10.0, 3.0, 0.0], [10.0, 7.0, (length * length - 16.0).sqrt()]);
        let p = lay_bridge(bridge, a, b);
        assert_eq!(p.len(), 1);
        let [(ya, za), (yb, zb)] = WHOLE_FEET;
        assert!(close(world(&p[0], [0.0, ya, za]), a), "{:?}", world(&p[0], [0.0, ya, za]));
        assert!(close(world(&p[0], [0.0, yb, zb]), b), "{:?}", world(&p[0], [0.0, yb, zb]));
    }

    #[test]
    fn a_narrow_road_takes_the_narrow_pieces() {
        assert!(matches!(choose_bridge(30.0, 6.0).0, Bridge::Pieces(c, 3) if c == NARROW));
        assert!(matches!(choose_bridge(90.0, 6.0).0, Bridge::Pieces(c, 9) if c == NARROW));
        assert!(matches!(choose_bridge(150.0, 12.0).0, Bridge::Pieces(c, 8) if c == WIDE));
    }

    #[test]
    fn the_bump_comes_in_from_the_ends_and_stays_off_another_road() {
        let l = straight();
        let s = settings();
        let y = with_bump(&l, &vec![0.0; l.len()], &s, &|x, _| (45.0..55.0).contains(&x));
        assert_eq!(y[0], 0.0);
        assert_eq!(*y.last().unwrap(), 0.0);
        assert!((y[25] - s.bump).abs() < 1e-9);
        assert_eq!(y[50], 0.0);
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
        assert!(y[10..y.len() - 10].iter().all(|v| (v - 10.25).abs() < 1e-9));
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
