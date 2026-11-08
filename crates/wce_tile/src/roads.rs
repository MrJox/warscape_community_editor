//! Procedural roads (docs/plans/procedural_roads.md): what a road does to the terrain, from its line and settings. No map here: a road gives
//! heights, texture weights and whether a point is road, and the document writes them into the layers the road owns. The same inputs give the
//! same road, so its layers are derived data: they are made again, never edited.
//!
//! World units throughout (the tile's x, z and height). The bridges over a road's gaps are made by `wce_walls::bridge`.

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

    /// How far a bridge's deck stands over the water at least (metres).
    pub clearance: f64,

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
    centre_line_handled(points, &[], sharp)
}

/// As [`centre_line_sharp`], with the handles written on the points (`controlIn`, `controlOut`, x and z): a segment with one at either
/// end is the Bezier through them, the handle not written the Catmull-Rom one (`splines::catmull_handles`: the curve without handles is
/// the same). A corner's handles are not used: it is a corner.
#[must_use]
pub fn centre_line_handled(points: &[[f64; 2]], handles: &[[Option<[f64; 2]>; 2]], sharp: &[bool]) -> (Vec<[f64; 2]>, Vec<usize>) {
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

        // A segment with a handle written at either end: the Bezier.
        let written = |k: isize, side: usize| (!is_sharp(k)).then(|| handles.get(k as usize).and_then(|h| h[side])).flatten();
        let (h1, h2) = (written(i, 1), written(i + 1, 0));
        if h1.is_some() || h2.is_some() {
            let b1 = h1.unwrap_or([p1[0] + (p2[0] - p0[0]) / 6.0, p1[1] + (p2[1] - p0[1]) / 6.0]);
            let b2 = h2.unwrap_or([p2[0] - (p3[0] - p1[0]) / 6.0, p2[1] - (p3[1] - p1[1]) / 6.0]);
            let d = |a: [f64; 2], b: [f64; 2]| (a[0] - b[0]).hypot(a[1] - b[1]);
            let n = (((d(p1, b1) + d(b1, b2) + d(b2, p2) + len) * 0.5 / SAMPLE).ceil() as usize).max(1);
            for k in 0..n {
                let t = k as f64 / n as f64;
                let s = 1.0 - t;
                let c = |a: f64, b: f64, c: f64, d: f64| a * s * s * s + b * 3.0 * s * s * t + c * 3.0 * s * t * t + d * t * t * t;
                out.push([c(p1[0], b1[0], b2[0], p2[0]), c(p1[1], b1[1], b2[1], p2[1])]);
            }

            continue;
        }

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
    with_bump_ends(line, heights, s, on_other, [true, true])
}

/// As [`with_bump`], the bump coming in from the ends `ramp_ends` says (first, last): not from an end on another road, where the road
/// meets that one's surface with its own bump.
#[must_use]
pub fn with_bump_ends(line: &[[f64; 2]], heights: &[f64], s: &RoadSettings, on_other: &dyn Fn(f64, f64) -> bool, ramp_ends: [bool; 2]) -> Vec<f64> {
    let n = heights.len();
    let reach = BUMP_RAMP as usize;
    let off = line.iter().map(|p| on_other(p[0], p[1])).collect::<Vec<_>>();
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let from_ends = match ramp_ends {
            [true, true] => i.min(n - 1 - i),
            [true, false] => i,
            [false, true] => n - 1 - i,
            [false, false] => n,
        };
        let ends = (from_ends as f64 / BUMP_RAMP).min(1.0);
        let near = (i.saturating_sub(reach)..(i + reach + 1).min(n)).filter(|&k| off[k]).map(|k| k.abs_diff(i) as f64 / BUMP_RAMP).fold(1.0, f64::min);
        out.push(heights[i] + s.bump * smooth(ends.min(near)));
    }

    out
}

/// The ground under a line averaged along it, then limited to the maximum grade both ways.
#[must_use]
pub fn graded(line: &[[f64; 2]], raw: &[f64], s: &RoadSettings) -> Vec<f64> {
    graded_pinned(line, raw, &[], s)
}

/// As [`graded`], the samples with a pin held at it (a ford's, on the bed): the road around comes to them at its grade.
#[must_use]
pub fn graded_pinned(line: &[[f64; 2]], raw: &[f64], pins: &[Option<f64>], s: &RoadSettings) -> Vec<f64> {
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

    let pin = |i: usize| pins.get(i).copied().flatten();
    for (i, v) in y.iter_mut().enumerate() {
        if let Some(p) = pin(i) {
            *v = p;
        }
    }

    let slope = s.max_grade.to_radians().tan();
    for i in 1..y.len() {
        let d = (along[i] - along[i - 1]) * slope;
        if pin(i).is_none() {
            y[i] = y[i].clamp(y[i - 1] - d, y[i - 1] + d);
        }
    }

    for i in (0..y.len() - 1).rev() {
        let d = (along[i + 1] - along[i]) * slope;
        if pin(i).is_none() {
            y[i] = y[i].clamp(y[i + 1] - d, y[i + 1] + d);
        }
    }

    y
}

/// Under this, water a road crosses is a ford; this or deeper, a bridge (metres; user, 2026-11-08).
pub const FORD_DEPTH: f64 = 0.75;

/// Water less deep than this over the ground is not water to a road (metres): a river's ribbon lies on its banks' edges.
const WET: f64 = 0.05;

/// Wet stretches nearer each other than this are one crossing (metres).
const MERGE: f64 = 8.0;

/// The water of a tile, as a road sees it: where it is and the height of its surface (world metres).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct WaterMap {
    /// Each river's samples along it: x, z, its surface's height, half its width.
    pub rivers: Vec<Vec<[f64; 4]>>,

    /// Each water plane (a lake): its corners on the ground in order, and its height.
    pub planes: Vec<([[f64; 2]; 4], f64)>,

    /// Whether the sea is under the tile (a tile of the sea's sets): water at 0 m wherever the ground is lower.
    pub sea: bool,
}

impl WaterMap {
    /// The highest water surface over a place, if any is.
    #[must_use]
    pub fn level(&self, x: f64, z: f64) -> Option<f64> {
        let mut best: Option<f64> = self.sea.then_some(0.0);
        let mut take = |y: f64| best = Some(best.map_or(y, |b: f64| b.max(y)));
        for river in &self.rivers {
            for w in river.windows(2) {
                let (a, b) = (w[0], w[1]);
                let d = [b[0] - a[0], b[1] - a[1]];
                let l2 = (d[0] * d[0] + d[1] * d[1]).max(1e-12);
                let t = (((x - a[0]) * d[0] + (z - a[1]) * d[1]) / l2).clamp(0.0, 1.0);
                let (px, pz) = (a[0] + d[0] * t, a[1] + d[1] * t);
                if (x - px).hypot(z - pz) <= a[3] + (b[3] - a[3]) * t {
                    take(a[2] + (b[2] - a[2]) * t);
                }
            }
        }

        for (corners, y) in &self.planes {
            // Inside a convex quadrilateral: on the same side of every edge.
            let side = |i: usize| {
                let (a, b) = (corners[i], corners[(i + 1) % 4]);
                (b[0] - a[0]) * (z - a[1]) - (b[1] - a[1]) * (x - a[0])
            };

            let s = (0..4).map(side).collect::<Vec<_>>();
            if s.iter().all(|&v| v >= 0.0) || s.iter().all(|&v| v <= 0.0) {
                take(*y);
            }
        }

        best
    }
}

/// A stretch of a road's line under water: its first and last sample, its deepest water over the ground and its highest surface.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Wet {
    pub from: usize,
    pub to: usize,
    pub depth: f64,
    pub level: f64,
}

/// Where a road's line runs under water (`raw`: the ground under each sample; `water`: the water's surface over it, if any), stretches
/// nearer each other than [`MERGE`] taken as one.
#[must_use]
pub fn wet_stretches(line: &[[f64; 2]], raw: &[f64], water: &[Option<f64>]) -> Vec<Wet> {
    let mut out: Vec<Wet> = vec![];
    let along = |a: usize, b: usize| line[a..=b].windows(2).map(|w| (w[1][0] - w[0][0]).hypot(w[1][1] - w[0][1])).sum::<f64>();
    for i in 0..raw.len().min(water.len()) {
        let Some(y) = water[i].filter(|&y| y - raw[i] > WET) else { continue };
        let depth = y - raw[i];
        match out.last_mut() {
            Some(w) if w.to + 1 == i || along(w.to, i) < MERGE => {
                (w.to, w.depth, w.level) = (i, w.depth.max(depth), w.level.max(y));
            }

            _ => out.push(Wet { from: i, to: i, depth, level: y }),
        }
    }

    out
}

/// A road laid over its ground: its height at each sample, the stretches its bridges carry (the terrain left alone under them), and its
/// fords (the road on the bed: the terrain's heights left alone, its texture and ground type laid).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Plan {
    pub heights: Vec<f64>,
    pub gaps: Vec<(usize, usize)>,
    pub fords: Vec<(usize, usize)>,

    /// Each bridge over water: its stretch and the least height of its deck.
    pub decks: Vec<(usize, usize, f64)>,
}

/// Lays a road over its ground (`raw`) and the water on it (`water`, per sample): water under [`FORD_DEPTH`] deep is forded, the road
/// pinned to the bed; deeper water is bridged, the deck at least `clearance` over it and the road climbing to it at its maximum grade on
/// each side; and where the ground drops away from the road (a canyon) it is bridged too, rim to rim ([`find_gaps`]). Without bridges
/// (`s.bridges` off) every crossing is a ford and a drop is graded through. `on_other`: whether a place is on an earlier road (no bump).
#[must_use]
pub fn plan(line: &[[f64; 2]], raw: &[f64], water: &[Option<f64>], s: &RoadSettings, on_other: &dyn Fn(f64, f64) -> bool) -> Plan {
    let wet = wet_stretches(line, raw, water);
    let (shallow, deep): (Vec<Wet>, Vec<Wet>) = wet.into_iter().partition(|w| !s.bridges || w.depth < FORD_DEPTH);
    let fords = shallow.iter().map(|w| (w.from, w.to)).collect::<Vec<_>>();
    let mut pins = vec![None; raw.len()];
    for &(a, b) in &fords {
        for i in a..=b {
            pins[i] = Some(raw[i]);
        }
    }

    // On an earlier road the road is at that one's surface (in `raw`: the ground has it, bump and all): pinned under it by its own bump,
    // which it keeps up to it, and the road around comes to it at its grade. Its own ground averaged along it and its bump ramped off near
    // it, it stood lower there: the end of a T's branch, its flat and its banks written over the through road, sank a hollow in it (user,
    // 2026-11-08).
    let on_earlier = line.iter().map(|p| on_other(p[0], p[1])).collect::<Vec<_>>();
    for (i, &on) in on_earlier.iter().enumerate() {
        if pins[i].is_none() && on {
            pins[i] = Some(raw[i] - s.bump);
        }
    }

    // The deep water's stretches, out to the banks: a bridge spans them.
    let decks = deep.iter().map(|w| (w.from.saturating_sub(1), (w.to + 1).min(raw.len() - 1), w.level + s.clearance)).collect::<Vec<_>>();
    let union = |mut g: Vec<(usize, usize)>| {
        g.extend(decks.iter().map(|d| (d.0, d.1)));
        g.sort_unstable();
        let mut out: Vec<(usize, usize)> = vec![];
        for (a, b) in g {
            match out.last_mut() {
                Some(l) if a <= l.1 + 1 => l.1 = l.1.max(b),
                _ => out.push((a, b)),
            }
        }

        out
    };

    // Graded on the ground under an earlier road's bump, as its own is (averaged with that bump, the road stood higher by it near one).
    let under = raw.iter().zip(&on_earlier).map(|(&h, &on)| if on { h - s.bump } else { h }).collect::<Vec<_>>();
    let grade = |gaps: &[(usize, usize)], pins: &[Option<f64>]| graded_pinned(line, &over_gaps(&under, gaps), pins, s);
    let mut gaps = union(vec![]);
    if s.bridges {
        for _ in 0..4 {
            let found = union(find_gaps(line, raw, &grade(&gaps, &pins), s));
            if found == gaps {
                break;
            }

            gaps = found;
        }
    }

    let along = {
        let mut a = vec![0.0; line.len()];
        for i in 1..line.len() {
            a[i] = a[i - 1] + (line[i][0] - line[i - 1][0]).hypot(line[i][1] - line[i - 1][1]);
        }

        a
    };

    let slope = s.max_grade.to_radians().tan();
    // The bump stays off a ford; it does not come in from an end on another road.
    let on_ford = |x: f64, z: f64| fords.iter().any(|&(a, b)| line[a..=b].iter().any(|p| (p[0] - x).hypot(p[1] - z) < 1.0));
    let ramp_ends = [!on_earlier.first().copied().unwrap_or(false), !on_earlier.last().copied().unwrap_or(false)];
    let heights_with = |pins: &[Option<f64>]| {
        // A deck over water stands at least its clearance up; the road climbs to it at its grade.
        let mut heights = grade(&gaps, pins);
        for &(a, b, deck) in &decks {
            for (i, h) in heights.iter_mut().enumerate() {
                let off = if i < a { along[a] - along[i] } else if i > b { along[i] - along[b] } else { 0.0 };
                *h = h.max(deck - off * slope);
            }
        }

        let mut heights = with_bump_ends(line, &heights, s, &on_ford, ramp_ends);
        for &(a, b) in &fords {
            heights[a..=b].copy_from_slice(&raw[a..=b]);
        }

        heights
    };

    let mut heights = heights_with(&pins);
    // Where the road comes back over itself, it is at the height it was at the first time, as on another road (user 2026-11-08: graded
    // apart, its two ways crossed at two heights, one cut into the other).
    let again = over_itself(line, &along, s.width);
    if !again.is_empty() {
        for _ in 0..2 {
            let mut more = pins.clone();
            for &(i, k) in &again {
                if more[i].is_none() {
                    more[i] = Some(heights[k] - s.bump);
                }
            }

            heights = heights_with(&more);
        }
    }

    Plan { heights, gaps, fords, decks }
}

/// The samples where a line comes back over itself, each with the earlier sample it lies on (within half `width`, the earlier one more
/// than twice the width back along it).
fn over_itself(line: &[[f64; 2]], along: &[f64], width: f64) -> Vec<(usize, usize)> {
    let mut out = vec![];
    for i in 0..line.len() {
        let p = line[i];
        let found = (0..i).take_while(|&k| along[i] - along[k] > width * 2.0).map(|k| (k, (line[k][0] - p[0]).hypot(line[k][1] - p[1])))
            .filter(|&(_, d)| d <= width * 0.5).min_by(|a, b| a.1.total_cmp(&b.1));
        if let Some((k, _)) = found {
            out.push((i, k));
        }
    }

    out
}

/// A road laid out: its samples, found by position.
pub struct Strip {
    pub line: Vec<[f64; 2]>,
    pub heights: Vec<f64>,
    settings: RoadSettings,

    /// The samples a bridge carries: the road leaves the terrain under them as it is.
    open: Vec<bool>,

    /// The samples of a ford: the road leaves the terrain's heights as they are, and lays its texture and ground type.
    kept: Vec<bool>,
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

        let (open, kept) = (vec![false; line.len()], vec![false; line.len()]);
        Self { line, heights, settings, open, kept, origin, across, cells, reach }
    }

    /// Leaves the terrain alone over these stretches (first and last sample).
    pub fn open_gaps(&mut self, gaps: &[(usize, usize)]) {
        for &(a, b) in gaps {
            self.open[a..=b].fill(true);
        }
    }

    /// Leaves the terrain's heights alone over these stretches (first and last sample): a ford's.
    pub fn keep_heights(&mut self, fords: &[(usize, usize)]) {
        for &(a, b) in fords {
            self.kept[a..=b].fill(true);
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
    fn nearest(&self, x: f64, z: f64) -> Option<(f64, f64, bool)> {
        self.nearest_open(x, z).filter(|n| !n.2).map(|n| (n.0, n.1, n.3))
    }

    /// As [`Self::nearest`] (and whether the place on the line is a ford's), and whether it is over a gap.
    fn nearest_open(&self, x: f64, z: f64) -> Option<(f64, f64, bool, bool)> {
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
        let (mut near, mut own, mut open, mut kept) = (f64::MAX, 0.0, false, false);
        for k in i.saturating_sub(4)..(i + 5).min(self.line.len()) {
            let p = self.line[k];
            let d = (p[0] - x).hypot(p[1] - z);
            if d < near {
                (near, own, open, kept) = (d, self.heights[k], self.open[k], self.kept[k]);
            }
        }

        // Near the road its exact height, for a surface with no ripple from the samples the cells left out; the jumps are further out.
        let far = smooth((near - self.settings.width * 0.5) / NEAR);
        Some((near, own + (sum / weights - own) * far, open, kept))
    }

    /// The terrain's height at a point once the road is there, `ground` its height without; none where the road leaves it alone. On the road
    /// the road's own height. Beyond its edge a bank back to the ground, wide enough to stay under the maximum side slope.
    #[must_use]
    pub fn height_at(&self, x: f64, z: f64, ground: f64) -> Option<f64> {
        let s = &self.settings;
        let (d, y, kept) = self.nearest(x, z)?;
        if kept {
            return None;
        }

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
        let Some((d, _, _)) = self.nearest(x, z) else { return 0.0 };
        let edge = s.width * 0.5 + (noise(x / 5.0, z / 5.0, s.seed) - 0.5) * s.feather;
        (1.0 - smooth((d - edge) / s.feather.max(0.01) + 0.5)) as f32
    }

    /// Whether a point is road, for the ground type.
    #[must_use]
    pub fn on_road(&self, x: f64, z: f64) -> bool {
        self.nearest(x, z).is_some_and(|(d, _, _)| d <= self.settings.width * 0.5)
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
        RoadSettings { width: 6.0, shoulder: 3.0, bump: 0.25, max_grade: 10.0, max_side: 30.0, channel: 5, ground: [255, 96, 0], bridges: true, clearance: 2.0, feather: 2.0, seed: 1 }
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

    /// A road along x over flat ground at 0, a river 12 m wide across it at x = 50, its bed `bed` deep and its water at 0.
    fn river_crossing(bed: f64) -> (Vec<[f64; 2]>, Vec<f64>, Vec<Option<f64>>) {
        let l = straight();
        let raw = l.iter().map(|p| if (p[0] - 50.0).abs() < 6.0 { -bed } else { 0.0 }).collect::<Vec<_>>();
        let water = WaterMap { rivers: vec![vec![[50.0, -50.0, 0.0, 6.0], [50.0, 50.0, 0.0, 6.0]]], ..Default::default() };
        let wet = l.iter().map(|p| water.level(p[0], p[1])).collect::<Vec<_>>();
        (l, raw, wet)
    }

    #[test]
    fn shallow_water_is_forded_on_its_bed() {
        let s = settings();
        let (l, raw, water) = river_crossing(0.5);
        let p = plan(&l, &raw, &water, &s, &|_, _| false);
        assert!(p.gaps.is_empty() && p.decks.is_empty(), "{:?}", p.gaps);
        assert_eq!(p.fords.len(), 1);
        let (a, b) = p.fords[0];
        assert!((l[a][0] - 45.0).abs() <= 1.5 && (l[b][0] - 55.0).abs() <= 1.5, "{} {}", l[a][0], l[b][0]);
        for i in a..=b {
            assert!((p.heights[i] - raw[i]).abs() < 1e-9, "the road keeps to the bed at {i}");
        }

        // The terrain's heights are left as they are on it, its texture laid.
        let mut strip = Strip::new(l.clone(), p.heights.clone(), s);
        strip.keep_heights(&p.fords);
        assert!(strip.height_at(50.0, 0.0, -0.5).is_none());
        assert!(strip.texture_at(50.0, 0.0) > 0.99 && strip.on_road(50.0, 0.0));
    }

    #[test]
    fn deep_water_is_bridged_over_its_clearance() {
        let s = settings();
        let (l, raw, water) = river_crossing(3.0);
        let p = plan(&l, &raw, &water, &s, &|_, _| false);
        assert!(p.fords.is_empty());
        assert_eq!(p.decks.len(), 1, "{:?}", p.decks);
        let (a, b, deck) = p.decks[0];
        assert!((deck - 2.0).abs() < 1e-9);
        assert!(p.gaps.iter().any(|&(g0, g1)| g0 <= a && g1 >= b), "{:?} over {a}..{b}", p.gaps);
        for i in a..=b {
            assert!(p.heights[i] >= deck - 1e-9, "the deck at {i}: {}", p.heights[i]);
        }

        // It climbs to it at its grade: 20 m before, at most 20 x tan(10 degrees) under it.
        let slope = s.max_grade.to_radians().tan();
        let before = (0..a).rev().find(|&i| l[a][0] - l[i][0] >= 20.0).unwrap();
        assert!(p.heights[before] >= deck - (l[a][0] - l[before][0]) * slope - 1e-6, "{}", p.heights[before]);
    }

    #[test]
    fn without_bridges_deep_water_is_forded_too() {
        let s = RoadSettings { bridges: false, ..settings() };
        let (l, raw, water) = river_crossing(3.0);
        let p = plan(&l, &raw, &water, &s, &|_, _| false);
        assert!(p.gaps.is_empty() && p.decks.is_empty());
        assert_eq!(p.fords.len(), 1);
    }

    #[test]
    fn a_road_ending_on_another_meets_it_at_its_surface() {
        // A T's branch along x, ending at 100 on a through road (its last 4 m, raised 0.25 by that one's bump) over level ground.
        let s = settings();
        let l = straight();
        let on = |x: f64, _: f64| x > 96.0;
        let raw = l.iter().map(|p| if on(p[0], p[1]) { 0.25 } else { 0.0 }).collect::<Vec<_>>();
        let p = plan(&l, &raw, &vec![None; l.len()], &s, &on);
        for (i, q) in l.iter().enumerate().filter(|(_, q)| on(q[0], q[1])) {
            assert!((p.heights[i] - 0.25).abs() < 1e-9, "at {q:?}: {}", p.heights[i]);
        }

        // Its own bump away from it and up to it: no dip beside it (its bump ramped off there, it was one).
        assert!((p.heights[50] - s.bump).abs() < 1e-9, "{}", p.heights[50]);
        for (i, q) in l.iter().enumerate().filter(|(_, q)| (80.0..=96.0).contains(&q[0])) {
            assert!((p.heights[i] - 0.25).abs() < 0.05, "at {q:?}: {}", p.heights[i]);
        }

        // Its other end, on the ground, still comes down to it.
        assert!(p.heights[0].abs() < 1e-9, "{}", p.heights[0]);
    }

    #[test]
    fn a_road_over_itself_is_at_one_height_where_it_crosses() {
        // A loop: east 60 m, round, and back across its first stretch at 30 m, over ground rising along the way (graded alone, its second
        // pass stood higher where it crossed the first).
        let pts = [[0.0, 0.0], [60.0, 0.0], [60.0, 30.0], [30.0, 30.0], [30.0, -30.0]];
        let l = centre_line(&pts);
        let along = (0..l.len()).map(|i| i as f64).collect::<Vec<_>>();
        let raw = along.iter().map(|a| a * 0.05).collect::<Vec<_>>();
        let s = settings();
        let p = plan(&l, &raw, &vec![None; l.len()], &s, &|_, _| false);
        let mut met = 0;
        for i in 0..l.len() {
            for k in 0..i.saturating_sub(20) {
                if (l[i][0] - l[k][0]).hypot(l[i][1] - l[k][1]) < 1.0 {
                    met += 1;
                    assert!((p.heights[i] - p.heights[k]).abs() < 0.1, "at {:?}: {} {}", l[i], p.heights[i], p.heights[k]);
                }
            }
        }

        assert!(met > 0, "it crosses itself");
    }

    #[test]
    fn a_line_with_its_catmull_rom_handles_written_is_the_same_and_a_turned_one_turns_it() {
        let pts = [[0.0, 0.0], [40.0, 10.0], [80.0, 0.0], [120.0, 30.0]];
        let (plain, _) = centre_line_sharp(&pts, &[]);
        // Every handle written as the Catmull-Rom one: the same curve (as many samples a segment, near enough).
        let h = (0..pts.len()).map(|i| {
            let (a, b) = (pts[i.saturating_sub(1)], pts[(i + 1).min(pts.len() - 1)]);
            let d = [(b[0] - a[0]) / 6.0, (b[1] - a[1]) / 6.0];
            [Some([pts[i][0] - d[0], pts[i][1] - d[1]]), Some([pts[i][0] + d[0], pts[i][1] + d[1]])]
        }).collect::<Vec<_>>();
        let (same, _) = centre_line_handled(&pts, &h, &[]);
        let off = |a: &[[f64; 2]], p: [f64; 2]| a.iter().map(|q| (q[0] - p[0]).hypot(q[1] - p[1])).fold(f64::MAX, f64::min);
        assert!(plain.iter().all(|&p| off(&same, p) < 0.6), "the curve moved");

        // The second point's handles turned across: the line leaves it that way.
        let mut turned = vec![[None, None]; pts.len()];
        turned[1] = [Some([40.0, 0.0]), Some([40.0, 20.0])];
        let (line, at) = centre_line_handled(&pts, &turned, &[]);
        let q = line[at[1] + 2];
        assert!((q[0] - 40.0).abs() < 1.0 && q[1] > 11.0, "{q:?}");
    }

    #[test]
    fn a_dry_canyon_is_bridged_rim_to_rim() {
        let s = settings();
        let l = straight();
        let raw = l.iter().map(|p| if (30.0..70.0).contains(&p[0]) { -20.0 } else { 0.0 }).collect::<Vec<_>>();
        let p = plan(&l, &raw, &vec![None; l.len()], &s, &|_, _| false);
        assert!(p.fords.is_empty() && p.decks.is_empty());
        assert_eq!(p.gaps.len(), 1, "{:?}", p.gaps);
    }

    #[test]
    fn the_water_map_finds_rivers_lakes_and_the_sea() {
        let mut w = WaterMap { rivers: vec![vec![[0.0, 0.0, 1.0, 5.0], [100.0, 0.0, 3.0, 5.0]]], ..Default::default() };
        assert!((w.level(50.0, 4.0).unwrap() - 2.0).abs() < 1e-9, "inside the ribbon, its height between its samples");
        assert!(w.level(50.0, 6.0).is_none());
        w.planes.push(([[200.0, 0.0], [300.0, 0.0], [300.0, 100.0], [200.0, 100.0]], -6.9));
        assert_eq!(w.level(250.0, 50.0), Some(-6.9));
        assert!(w.level(350.0, 50.0).is_none());
        w.sea = true;
        assert_eq!(w.level(350.0, 50.0), Some(0.0));
        assert_eq!(w.level(250.0, 50.0), Some(0.0), "the highest water");
    }
}
