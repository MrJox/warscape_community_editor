//! A river tile made by a seed (docs/plans/river_bridges.md, step 3): fresh relief and a river's line from link to link. Pure: the
//! editor's `river_tile_gen.rs` lays them on the open tile (the relief as its heights, the line as a river snapped to its links, carved,
//! with its shore). Everything is in the editor's metres, x and z on the tile from its corner.

/// The relief's settings.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ReliefSettings {
    /// How far the ground rises and falls about 0 (metres).
    pub amplitude: f64,

    /// The size of its largest hills (metres, the noise's first wavelength).
    pub scale: f64,

    /// How far in from the edges it fades to 0 (metres): the export holds the land edges at 0 to meet the neighbours
    /// (docs/re/battle_maps.md).
    pub edge_fade: f64,
}

impl Default for ReliefSettings {
    fn default() -> Self {
        Self { amplitude: 14.0, scale: 220.0, edge_fade: 64.0 }
    }
}

/// A number from -1 to 1 of a seed and a grid corner.
pub(crate) fn corner(seed: u64, i: i64, j: i64) -> f64 {
    let mut z = seed ^ (i as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15) ^ (j as u64).wrapping_mul(0xc2b2_ae3d_27d4_eb4f);
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^= z >> 31;
    (z >> 11) as f64 / (1u64 << 52) as f64 - 1.0
}

/// Smooth value noise, -1 to 1, of cells `size` across.
fn value_noise(seed: u64, x: f64, z: f64, size: f64) -> f64 {
    let (u, v) = (x / size, z / size);
    let (i, j) = (u.floor() as i64, v.floor() as i64);
    let (fu, fv) = (u - u.floor(), v - v.floor());
    let s = |t: f64| t * t * (3.0 - 2.0 * t);
    let (su, sv) = (s(fu), s(fv));
    let a = corner(seed, i, j) + (corner(seed, i + 1, j) - corner(seed, i, j)) * su;
    let b = corner(seed, i, j + 1) + (corner(seed, i + 1, j + 1) - corner(seed, i, j + 1)) * su;
    a + (b - a) * sv
}

/// Fractal noise: four octaves, each half the size and half the weight of the last; about -1 to 1.
#[must_use]
pub fn fbm(seed: u64, x: f64, z: f64, size: f64) -> f64 {
    let (mut sum, mut weight, mut total, mut s) = (0.0, 1.0, 0.0, size);
    for k in 0..4u64 {
        sum += value_noise(seed.wrapping_add(k.wrapping_mul(0x51_7cc1_b727_220a)), x, z, s) * weight;
        total += weight;
        weight *= 0.5;
        s *= 0.5;
    }

    sum / total
}

/// How much of the relief stands at a place: 0 at the edges, 1 from `fade` in, smoothly.
fn edge_weight(x: f64, z: f64, extent: (f64, f64), fade: f64) -> f64 {
    if fade <= 0.0 {
        return 1.0;
    }

    let d = x.min(z).min(extent.0 - x).min(extent.1 - z).max(0.0);
    let t = (d / fade).min(1.0);
    t * t * (3.0 - 2.0 * t)
}

/// The relief's height at a place (metres about 0): rolling ground by the seed, faded to 0 at the tile's edges.
#[must_use]
pub fn relief(seed: u64, x: f64, z: f64, extent: (f64, f64), s: &ReliefSettings) -> f64 {
    fbm(seed, x, z, s.scale) * s.amplitude * edge_weight(x, z, extent, s.edge_fade)
}

/// A river's valley in the relief: the ground near its line pulled to `floor` (a little over its water), smoothly out to `half_width`.
#[derive(Clone, Debug, PartialEq)]
pub struct Valley {
    /// The river's line, smoothed (`smooth_line`).
    pub line: Vec<[f64; 2]>,
    pub floor: f64,
    pub half_width: f64,
}

/// The outer bank of a line's bend: between the line's samples `from` and `to`, on its `side` (1: its left, going along it; -1: its
/// right). A generated tile's cliff bank (the river's points there marked, `river_bed::CliffBank`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bend {
    pub from: usize,
    pub to: usize,
    pub side: f64,
}

/// The nearest place of a line to a point: its distance, the line's sample index there (with the share along that segment), and the
/// side of the line it is on (1: left, -1: right, going along the line).
#[must_use]
pub fn nearest_on_line(p: [f64; 2], line: &[[f64; 2]]) -> (f64, f64, f64) {
    let mut best = (f64::INFINITY, 0.0, 1.0);
    for (i, w) in line.windows(2).enumerate() {
        let (a, b) = (w[0], w[1]);
        let (dx, dz) = (b[0] - a[0], b[1] - a[1]);
        let l2 = dx * dx + dz * dz;
        let t = if l2 > 0.0 { (((p[0] - a[0]) * dx + (p[1] - a[1]) * dz) / l2).clamp(0.0, 1.0) } else { 0.0 };
        let d = (p[0] - a[0] - t * dx).hypot(p[1] - a[1] - t * dz);
        if d < best.0 {
            let cross = dx * (p[1] - a[1]) - dz * (p[0] - a[0]);
            best = (d, i as f64 + t, if cross >= 0.0 { 1.0 } else { -1.0 });
        }
    }

    best
}

/// The outer bank of a line's sharpest bend (its middle half only: the ends run square to the tile's edges): `length` samples long,
/// centred there. None when the line hardly bends.
#[must_use]
pub fn outer_bend(line: &[[f64; 2]], length: usize) -> Option<Bend> {
    let n = line.len();
    if n < 8 {
        return None;
    }

    // The turn at each sample, over a few samples either side (left turns positive).
    let k = 3;
    let turn = |i: usize| {
        let (a, b, c) = (line[i - k], line[i], line[i + k]);
        let (u, v) = ([b[0] - a[0], b[1] - a[1]], [c[0] - b[0], c[1] - b[1]]);
        (u[0] * v[1] - u[1] * v[0]).atan2(u[0] * v[0] + u[1] * v[1])
    };
    let (lo, hi) = (n / 4, n - n / 4);
    let (at, angle) = (lo.max(k)..hi.min(n - k)).map(|i| (i, turn(i))).max_by(|a, b| a.1.abs().total_cmp(&b.1.abs()))?;
    if angle.abs() < 0.08 {
        return None;
    }

    // The outer side of a left turn is the right.
    let half = length / 2;
    Some(Bend { from: at.saturating_sub(half), to: (at + half).min(n - 1), side: -angle.signum() })
}

/// Where a ford goes on a river's line (docs/plans/river_bridges.md, step 6): the sample nearest the tile's middle on a straight stretch
/// (its turn over a few samples either side counted as metres off the middle), not within `clear` samples of the stretch `avoid` (a
/// cliff's), nudged by the seed; none on a line too short or with every sample taken.
#[must_use]
pub fn ford_place(line: &[[f64; 2]], extent: (f64, f64), avoid: Option<(usize, usize)>, clear: usize, seed: u64) -> Option<usize> {
    let n = line.len();
    let k = 3;
    if n < 2 * k + 2 {
        return None;
    }

    let mid = [extent.0 * 0.5, extent.1 * 0.5];
    let (lo, hi) = (n / 5, n - n / 5);
    (lo.max(k)..hi.min(n - k))
        .filter(|&i| avoid.is_none_or(|(a, b)| i + clear < a || i > b + clear))
        .map(|i| {
            let (a, b, c) = (line[i - k], line[i], line[i + k]);
            let (u, v) = ([b[0] - a[0], b[1] - a[1]], [c[0] - b[0], c[1] - b[1]]);
            let turn = (u[0] * v[1] - u[1] * v[0]).atan2(u[0] * v[0] + u[1] * v[1]).abs();
            let nudge = (corner(seed ^ FORD_SALT, i as i64, 0) + 1.0) * 12.0;
            (i, (b[0] - mid[0]).hypot(b[1] - mid[1]) + turn * 80.0 + nudge)
        })
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(i, _)| i)
}

/// The line across a river at its line's sample `at`: `reach` metres to either side, square to its way there.
#[must_use]
pub fn across_line(line: &[[f64; 2]], at: usize, reach: f64) -> Vec<[f64; 2]> {
    let (a, b) = (line[at.saturating_sub(1)], line[(at + 1).min(line.len() - 1)]);
    let (dx, dz) = (b[0] - a[0], b[1] - a[1]);
    let len = dx.hypot(dz).max(1e-6);
    let (n, p) = ([-dz / len, dx / len], line[at]);
    vec![[p[0] - n[0] * reach, p[1] - n[1] * reach], [p[0] + n[0] * reach, p[1] + n[1] * reach]]
}

const FORD_SALT: u64 = 0xf04d_0000_0000_0006;

/// The relief with a river's valley in it: within `half_width` of the river's line the ground goes to the valley's floor (all of it on
/// the line, smoothly less out to the edge of the valley), so that the river runs between banks over its water, not over hills; still
/// faded to 0 at the tile's edges; the hills [`HILLS_OVER_FLOOR_M`] or more over the floor.
#[must_use]
pub fn relief_in_valley(seed: u64, x: f64, z: f64, extent: (f64, f64), s: &ReliefSettings, valley: &Valley) -> f64 {
    let hills = fbm(seed, x, z, s.scale) * s.amplitude;
    let (d, ..) = nearest_on_line([x, z], &valley.line);
    let t = (d / valley.half_width.max(1e-6)).clamp(0.0, 1.0);
    let w = 1.0 - t * t * (3.0 - 2.0 * t);
    // The hills never down to the valley's floor (the river's water would stand in the dips as ponds, its shores round them).
    let hills = hills.max(valley.floor + HILLS_OVER_FLOOR_M);
    let h = hills + (valley.floor - hills) * w;
    h * edge_weight(x, z, extent, s.edge_fade)
}

/// How far over the valley's floor the hills stay at least (metres).
pub const HILLS_OVER_FLOOR_M: f64 = 2.0;

/// The forest areas' settings.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ForestSettings {
    /// How far apart the places looked at for a wood are (metres).
    pub spacing: f64,

    /// How much of the forest noise a place needs for a wood (-1 .. 1: higher, fewer woods).
    pub threshold: f64,

    /// A wood's radius, from and to (metres).
    pub radius: (f64, f64),

    /// How close to the tile's edges a wood may come (metres).
    pub margin: f64,
}

impl Default for ForestSettings {
    fn default() -> Self {
        Self { spacing: 60.0, threshold: -0.15, radius: (22.0, 48.0), margin: 6.0 }
    }
}

/// Woods on a tile by a seed: blob-shaped areas (closed outlines, x, z) where a forest noise is high, kept `clearance` metres off each
/// line to keep clear of (the river with its banks, a road), and off each other.
#[must_use]
pub fn forest_areas(seed: u64, extent: (f64, f64), keep_off: &[(&[[f64; 2]], f64)], s: &ForestSettings) -> Vec<Vec<[f64; 2]>> {
    const POINTS: usize = 14;
    let seed = seed ^ FOREST_SALT;
    let clear = |p: [f64; 2]| keep_off.iter().all(|(line, c)| distance_to_line(p, line) >= *c);
    let mut out: Vec<(Vec<[f64; 2]>, [f64; 2], f64)> = vec![];
    let (nx, nz) = ((extent.0 / s.spacing).ceil() as i64, (extent.1 / s.spacing).ceil() as i64);
    for j in 0..nz {
        for i in 0..nx {
            // A place in the cell, jittered by the seed.
            let jx = (corner(seed, i, j) * 0.5 + 0.5) * s.spacing;
            let jz = (corner(seed ^ 0x77, i, j) * 0.5 + 0.5) * s.spacing;
            let c = [i as f64 * s.spacing + jx, j as f64 * s.spacing + jz];
            if fbm(seed, c[0], c[1], 160.0) < s.threshold || !clear(c) {
                continue;
            }

            let r = s.radius.0 + (corner(seed ^ 0x99, i, j) * 0.5 + 0.5) * (s.radius.1 - s.radius.0);
            if out.iter().any(|(_, o, or)| (o[0] - c[0]).hypot(o[1] - c[1]) < (or + r) * 1.1) {
                continue;
            }

            // Its outline: round, its radius swelling and shrinking by a noise, each point pulled in until it is clear.
            let mut outline = vec![];
            for k in 0..POINTS {
                let a = std::f64::consts::TAU * k as f64 / POINTS as f64;
                let (ca, sa) = (a.cos(), a.sin());
                let mut rk = r * (0.75 + 0.35 * (fbm(seed ^ 0x55, c[0] + ca * 50.0, c[1] + sa * 50.0, 40.0) * 0.5 + 0.5));
                let at = |rk: f64| {
                    [(c[0] + ca * rk).clamp(s.margin, extent.0 - s.margin), (c[1] + sa * rk).clamp(s.margin, extent.1 - s.margin)]
                };
                while rk > 1.0 && !clear(at(rk)) {
                    rk *= 0.85;
                }

                // Pulled in all the way: its centre (clear, inside).
                outline.push(if clear(at(rk)) { at(rk) } else { at(0.0) });
            }

            // A wood squeezed to nothing (all its points pulled in) is left out.
            let area = outline.iter().zip(outline.iter().cycle().skip(1)).map(|(a, b)| a[0] * b[1] - b[0] * a[1]).sum::<f64>().abs() * 0.5;
            if area > 300.0 {
                out.push((outline, c, r));
            }
        }
    }

    out.into_iter().map(|(o, _, _)| o).collect()
}

/// The salt the woods' noises take.
const FOREST_SALT: u64 = 0xf0e5_7000_0000_0002;

/// A line smoothed through its points (Catmull-Rom, `per_segment` samples between two points), as a spline drawn through them runs.
#[must_use]
pub fn smooth_line(points: &[[f64; 2]], per_segment: usize) -> Vec<[f64; 2]> {
    let n = points.len();
    if n < 3 || per_segment < 2 {
        return points.to_vec();
    }

    let mut out = vec![];
    for i in 0..n - 1 {
        let (p0, p1, p2, p3) = (points[i.saturating_sub(1)], points[i], points[i + 1], points[(i + 2).min(n - 1)]);
        for k in 0..per_segment {
            let t = k as f64 / per_segment as f64;
            let (t2, t3) = (t * t, t * t * t);
            let c = |a: f64, b: f64, c: f64, d: f64| {
                0.5 * (2.0 * b + (c - a) * t + (2.0 * a - 5.0 * b + 4.0 * c - d) * t2 + (3.0 * b - a - 3.0 * c + d) * t3)
            };
            out.push([c(p0[0], p1[0], p2[0], p3[0]), c(p0[1], p1[1], p2[1], p3[1])]);
        }
    }

    out.push(points[n - 1]);
    out
}

/// How far a place is from a line (its nearest segment).
#[must_use]
pub fn distance_to_line(p: [f64; 2], line: &[[f64; 2]]) -> f64 {
    match line {
        [] => f64::INFINITY,
        [a] => (p[0] - a[0]).hypot(p[1] - a[1]),
        _ => line
            .windows(2)
            .map(|w| {
                let (a, b) = (w[0], w[1]);
                let (dx, dz) = (b[0] - a[0], b[1] - a[1]);
                let l2 = dx * dx + dz * dz;
                let t = if l2 > 0.0 { (((p[0] - a[0]) * dx + (p[1] - a[1]) * dz) / l2).clamp(0.0, 1.0) } else { 0.0 };
                (p[0] - a[0] - t * dx).hypot(p[1] - a[1] - t * dz)
            })
            .fold(f64::INFINITY, f64::min),
    }
}

/// A river link's end as the generator takes it: where the river's end goes and the way into the tile there (x, z).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LinkEnd {
    pub at: [f64; 2],
    pub inward: [f64; 2],
}

/// The river line's settings.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LineSettings {
    /// How far each end runs square to its edge before it may bend (metres).
    pub straight: f64,

    /// How far apart its points are along it (metres, about).
    pub step: f64,

    /// How far a meander may swing to the side, as a share of the line's length between its straight ends.
    pub meander: f64,

    /// How close to the tile's edges its middle may come (metres).
    pub margin: f64,
}

impl Default for LineSettings {
    fn default() -> Self {
        Self { straight: 50.0, step: 45.0, meander: 0.22, margin: 40.0 }
    }
}

/// A river's line from one link to another (x, z points, the first at `from.at`, the last at `to.at`): square to the edges for
/// `straight` metres at each end, then through points spaced along the line between those ends, each pushed to one side by a smooth
/// random amount (meanders, by the seed). Each point keeps its place along that line, so the river never turns back on itself; its middle
/// stays `margin` from the edges.
#[must_use]
pub fn river_line(from: LinkEnd, to: LinkEnd, extent: (f64, f64), seed: u64, s: &LineSettings) -> Vec<[f64; 2]> {
    let a = [from.at[0] + from.inward[0] * s.straight, from.at[1] + from.inward[1] * s.straight];
    let b = [to.at[0] + to.inward[0] * s.straight, to.at[1] + to.inward[1] * s.straight];
    let (dx, dz) = (b[0] - a[0], b[1] - a[1]);
    let length = dx.hypot(dz);
    let mut out = vec![from.at, a];
    if length > 1e-6 {
        let side = [-dz / length, dx / length];
        let n = ((length / s.step).round() as usize).max(1);
        let swing = length * s.meander;
        for k in 1..n {
            let t = k as f64 / n as f64;
            // A smooth curve along the line (no swing at its ends), the noise's sign and size by the seed.
            let envelope = (std::f64::consts::PI * t).sin();
            let offset = fbm(seed ^ LINE_SALT, t * 400.0, 0.0, 160.0) * 2.0 * swing * envelope;
            let p = [a[0] + dx * t + side[0] * offset, a[1] + dz * t + side[1] * offset];
            let clamp = |v: f64, max: f64| v.clamp(s.margin, (max - s.margin).max(s.margin));
            out.push([clamp(p[0], extent.0), clamp(p[1], extent.1)]);
        }
    }

    out.push(b);
    out.push(to.at);
    out
}

/// The salt the river line's noise takes (so that its meanders and the relief's hills, from one seed, do not follow each other).
const LINE_SALT: u64 = 0x5eed_0072_1fe5_0001;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_relief_rolls_inside_and_is_flat_at_the_edges() {
        let s = ReliefSettings::default();
        let e = (256.0, 256.0);
        for i in 0..=32 {
            let t = f64::from(i) * 8.0;
            for (x, z) in [(0.0, t), (256.0, t), (t, 0.0), (t, 256.0)] {
                assert!(relief(3, x, z, e, &s).abs() < 1e-9, "flat at the edge ({x}, {z})");
            }
        }

        let inside = (0..20).flat_map(|i| (0..20).map(move |j| relief(3, 70.0 + f64::from(i) * 6.0, 70.0 + f64::from(j) * 6.0, e, &s)));
        let (lo, hi) = inside.fold((f64::MAX, f64::MIN), |(l, h), v| (l.min(v), h.max(v)));
        assert!(hi - lo > 1.0 && hi <= s.amplitude && lo >= -s.amplitude, "it rolls: {lo} .. {hi}");
        assert_ne!(relief(3, 128.0, 128.0, e, &s), relief(4, 128.0, 128.0, e, &s), "another seed, other hills");
    }

    #[test]
    fn a_valley_holds_the_ground_at_its_floor_along_the_river_and_hills_beyond() {
        let s = ReliefSettings::default();
        let e = (256.0, 256.0);
        let line = smooth_line(&[[0.0, 128.0], [90.0, 120.0], [170.0, 140.0], [256.0, 128.0]], 8);
        let valley = Valley { line: line.clone(), floor: -2.0, half_width: 60.0 };
        for p in line.iter().filter(|p| p[0] > 80.0 && p[0] < 176.0) {
            assert!((relief_in_valley(5, p[0], p[1], e, &s, &valley) + 2.0).abs() < 1e-9, "at its floor on the river: {p:?}");
        }

        // Past the valley, the hills as they are, never down to its floor.
        for x in [100.0, 128.0, 150.0] {
            let (h, hills) = (relief_in_valley(5, x, 30.0, e, &s, &valley), relief(5, x, 30.0, e, &s));
            let low = (-2.0 + HILLS_OVER_FLOOR_M) * edge_weight(x, 30.0, e, s.edge_fade);
            assert!((h - hills.max(low)).abs() < 1e-9, "{x}: {h} against {hills}");
        }

        assert!((distance_to_line([5.0, 3.0], &[[0.0, 0.0], [10.0, 0.0]]) - 3.0).abs() < 1e-12);
        let smooth = smooth_line(&[[0.0, 0.0], [10.0, 10.0], [20.0, 0.0]], 4);
        assert_eq!((smooth.first(), smooth.last()), (Some(&[0.0, 0.0]), Some(&[20.0, 0.0])));
        assert!(smooth.contains(&[10.0, 10.0]), "through its points");
    }

    #[test]
    fn the_outer_bank_of_a_bend_is_away_from_its_inside() {
        // A bend to the left (seen going along): west to east, then turning north (-z) round x 128.
        let line = smooth_line(&[[0.0, 200.0], [60.0, 200.0], [110.0, 190.0], [140.0, 150.0], [150.0, 90.0], [150.0, 0.0]], 10);
        let bend = outer_bend(&line, 16).expect("it bends");
        let mid = (bend.from + bend.to) / 2;
        let (a, b) = (line[mid], line[mid + 1]);
        let len = (b[0] - a[0]).hypot(b[1] - a[1]);
        let left = [-(b[1] - a[1]) / len, (b[0] - a[0]) / len];
        let at = |d: f64| [a[0] + left[0] * d, a[1] + left[1] * d];
        let (out, inn) = (at(35.0 * bend.side), at(-35.0 * bend.side));
        let inside = [60.0, 100.0];
        let far = |p: [f64; 2]| (p[0] - inside[0]).hypot(p[1] - inside[1]);
        assert!(far(out) > far(inn), "the outer bank away from the bend's inside");
        assert_eq!(nearest_on_line(out, &line).2, bend.side);
        assert!(bend.to - bend.from == 16 || bend.to == line.len() - 1);
        assert_eq!(
            outer_bend(&[[0.0, 0.0], [10.0, 0.0], [20.0, 0.0], [30.0, 0.0], [40.0, 0.0], [50.0, 0.0], [60.0, 0.0], [70.0, 0.0]], 4),
            None
        );
    }

    #[test]
    fn a_ford_goes_near_the_middle_on_a_straight_stretch_clear_of_the_cliff() {
        let e = (256.0, 256.0);
        // Straight west to east through the middle, then a hard bend north near the east edge.
        let line = smooth_line(&[[0.0, 128.0], [60.0, 128.0], [128.0, 128.0], [180.0, 128.0], [215.0, 100.0], [230.0, 0.0]], 10);
        for seed in 0..8 {
            let i = ford_place(&line, e, None, 4, seed).expect("a place");
            assert!((line[i][0] - 128.0).abs() < 40.0 && (line[i][1] - 128.0).abs() < 1.0, "seed {seed}: by the middle: {:?}", line[i]);
        }

        // With the middle taken (a cliff's stretch), elsewhere, clear of it.
        let m = line.iter().position(|p| p[0] >= 128.0).unwrap();
        let i = ford_place(&line, e, Some((m - 8, m + 8)), 4, 1).expect("a place");
        assert!(i + 4 < m - 8 || i > m + 12, "clear of the cliff: {i} by {m}");
        let across = across_line(&line, i, 30.0);
        let (u, w) = (
            [across[1][0] - across[0][0], across[1][1] - across[0][1]],
            [line[i + 1][0] - line[i - 1][0], line[i + 1][1] - line[i - 1][1]],
        );
        let cos = (u[0] * w[0] + u[1] * w[1]) / (u[0].hypot(u[1]) * w[0].hypot(w[1]));
        assert!(cos.abs() < 0.01 && (u[0].hypot(u[1]) - 60.0).abs() < 1e-6, "square across it: {across:?}");
    }

    #[test]
    fn woods_grow_away_from_the_river_and_from_each_other() {
        let river = smooth_line(&[[0.0, 128.0], [100.0, 110.0], [180.0, 150.0], [256.0, 128.0]], 8);
        let keep: [(&[[f64; 2]], f64); 1] = [(&river, 50.0)];
        let mut any = 0;
        for seed in 0..10 {
            let woods = forest_areas(seed, (256.0, 256.0), &keep, &ForestSettings::default());
            any += woods.len();
            for w in &woods {
                assert!(w.iter().all(|p| distance_to_line(*p, &river) >= 50.0 - 1e-6), "seed {seed}: clear of the river");
                assert!(w.iter().all(|p| (6.0..=250.0).contains(&p[0]) && (6.0..=250.0).contains(&p[1])), "inside the tile");
            }
        }

        assert!(any >= 20, "woods on most seeds: {any}");
        assert_ne!(
            forest_areas(1, (256.0, 256.0), &keep, &ForestSettings::default()),
            forest_areas(2, (256.0, 256.0), &keep, &ForestSettings::default())
        );
    }

    #[test]
    fn a_river_line_runs_from_link_to_link_square_to_the_edges_and_never_turns_back() {
        // In from the west edge at z 192, out by the south (+z) edge at x 64: a bend.
        let from = LinkEnd { at: [-6.0, 192.0], inward: [1.0, 0.0] };
        let to = LinkEnd { at: [64.0, 262.0], inward: [0.0, -1.0] };
        let s = LineSettings::default();
        for seed in 0..20 {
            let line = river_line(from, to, (256.0, 256.0), seed, &s);
            assert_eq!(line.first(), Some(&from.at));
            assert_eq!(line.last(), Some(&to.at));
            assert_eq!(line[1], [-6.0 + s.straight, 192.0], "straight in from its edge");
            assert_eq!(line[line.len() - 2], [64.0, 262.0 - s.straight], "straight out to its edge");
            // Along the line between the straight ends, each point further than the last.
            let (a, b) = (line[1], line[line.len() - 2]);
            let along = |p: [f64; 2]| (p[0] - a[0]) * (b[0] - a[0]) + (p[1] - a[1]) * (b[1] - a[1]);
            let inner = &line[1..line.len() - 1];
            assert!(inner.windows(2).all(|w| along(w[1]) > along(w[0]) - 1e-6), "seed {seed}: {inner:?}");
            for p in &line[2..line.len() - 2] {
                assert!(p[0] >= s.margin && p[0] <= 256.0 - s.margin && p[1] >= s.margin && p[1] <= 256.0 - s.margin, "{p:?}");
            }
        }

        let straight = river_line(
            LinkEnd { at: [-6.0, 128.0], inward: [1.0, 0.0] },
            LinkEnd { at: [262.0, 128.0], inward: [-1.0, 0.0] },
            (256.0, 256.0),
            1,
            &s,
        );
        let bends = (0..10).map(|seed| {
            river_line(
                LinkEnd { at: [-6.0, 128.0], inward: [1.0, 0.0] },
                LinkEnd { at: [262.0, 128.0], inward: [-1.0, 0.0] },
                (256.0, 256.0),
                seed,
                &s,
            )
        });
        assert!(bends.clone().any(|l| l.iter().any(|p| (p[1] - 128.0).abs() > 10.0)), "it meanders: {straight:?}");
        assert!(
            bends.map(|l| l.iter().map(|p| (p[1] * 10.0) as i64).collect::<Vec<_>>()).collect::<std::collections::HashSet<_>>().len() > 1
        );
    }
}
