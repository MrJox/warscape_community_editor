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
fn corner(seed: u64, i: i64, j: i64) -> f64 {
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

/// The relief with a river's valley in it: within `half_width` of the river's line the ground goes to the valley's floor (all of it on
/// the line, smoothly less out to the edge of the valley), so that the river runs between banks over its water, not over hills; still
/// faded to 0 at the tile's edges.
#[must_use]
pub fn relief_in_valley(seed: u64, x: f64, z: f64, extent: (f64, f64), s: &ReliefSettings, valley: &Valley) -> f64 {
    let hills = fbm(seed, x, z, s.scale) * s.amplitude;
    let t = (distance_to_line([x, z], &valley.line) / valley.half_width.max(1e-6)).clamp(0.0, 1.0);
    let w = 1.0 - t * t * (3.0 - 2.0 * t);
    (hills + (valley.floor - hills) * w) * edge_weight(x, z, extent, s.edge_fade)
}

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
        Self { straight: 30.0, step: 45.0, meander: 0.22, margin: 40.0 }
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

        // Past the valley, the hills as they are.
        for x in [100.0, 128.0, 150.0] {
            assert!((relief_in_valley(5, x, 30.0, e, &s, &valley) - relief(5, x, 30.0, e, &s)).abs() < 1e-9);
        }

        assert!((distance_to_line([5.0, 3.0], &[[0.0, 0.0], [10.0, 0.0]]) - 3.0).abs() < 1e-12);
        let smooth = smooth_line(&[[0.0, 0.0], [10.0, 10.0], [20.0, 0.0]], 4);
        assert_eq!((smooth.first(), smooth.last()), (Some(&[0.0, 0.0]), Some(&[20.0, 0.0])));
        assert!(smooth.contains(&[10.0, 10.0]), "through its points");
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
            assert_eq!(line[1], [24.0, 192.0], "straight in from its edge");
            assert_eq!(line[line.len() - 2], [64.0, 232.0], "straight out to its edge");
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
