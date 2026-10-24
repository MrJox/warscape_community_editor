//! A wall's spline to its layout (docs/plans/procedural_walls.md, "The solver"): the centre line, its corners, the ground along the outer
//! face and the stepped walkway. Scene units throughout (x, z horizontal, y up; the models' units). Pure: the ground comes in as a
//! function, so the editor and the tests share it.
//!
//! - **Centre line:** the points in order. A point is a corner when it is marked sharp or its internal angle is under
//!   [`SHARP_ANGLE`]; between corners (and the ends) the line is a centripetal Catmull-Rom through the points. Sampled every [`STEP`].
//! - **Outer foot:** the centre line moved to the base's outer face (its local -z side, or +z with `flip`); the ground is read there.
//! - **Runs:** flat stretches of walkway, each at the ground at its middle, as long as the ground stays within `tolerance` of it; an
//!   attached end's run is at the height it was given.
//! - **Steps:** between two runs, step sections of the base's run, each rising at most the base's rise, centred on the boundary.

use crate::profile::Profile;

/// Samples along the wall, in scene units.
pub const STEP: f64 = 0.5;

/// A point whose internal angle is under this (degrees) is a corner even when it is not marked sharp (a turn over 155 degrees).
pub const SHARP_ANGLE: f64 = 25.0;

/// The shortest run (scene units): shorter stretches of uneven ground are taken as one run with the drift they have.
pub const MIN_RUN: f64 = 4.0;

/// A wall to lay out.
#[derive(Clone, Debug, Default)]
pub struct WallInput {
    /// The spline's points (x, z), in order.
    pub points: Vec<[f64; 2]>,

    /// Per point, marked sharp (a corner). The ends are always corners.
    pub sharp: Vec<bool>,

    /// The outer face on the other side: the wall is laid from its last point to its first (a rotation of the pieces, not a mirror), so
    /// distances along it start at the last point.
    pub flip: bool,

    /// How far the walkway's ground may drift from the ground before a step (scene units of height).
    pub tolerance: f64,

    /// The ground level an attached end is held to (the gate's or bastion's), per end.
    pub ends: [Option<f64>; 2],

    /// The face an attached end meets (the gate's or bastion's end): its normal (x, z), per end. The wall's end piece is sheared onto
    /// it.
    pub end_planes: [Option<[f64; 2]>; 2],
}

/// A point along the wall.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sample {
    /// Distance along the centre line.
    pub s: f64,

    /// On the centre line (x, z).
    pub centre: [f64; 2],

    /// The wall's direction (unit, x z).
    pub dir: [f64; 2],

    /// On the outer face (x, z), and the ground there.
    pub foot: [f64; 2],
    pub ground: f64,

    /// The level the pieces stand on here (the walkway is this plus the base's walkway height).
    pub level: f64,
}

/// A flat stretch of walkway.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Run {
    pub s: (f64, f64),
    pub level: f64,

    /// The most the ground under it is off its level.
    pub drift: f64,
}

/// A step section: from one level to another over `s`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Step {
    pub s: (f64, f64),
    pub from: f64,
    pub to: f64,

    /// The boundary between runs it climbs (the steps of one boundary are a chain).
    pub chain: usize,
}

/// A wall's layout.
#[derive(Clone, Debug, Default)]
pub struct Plan {
    pub samples: Vec<Sample>,
    pub length: f64,

    /// The corners (distance along the wall, and the point's index).
    pub corners: Vec<(f64, usize)>,
    pub runs: Vec<Run>,
    pub steps: Vec<Step>,

    /// What the layout could not do as asked (steps that don't fit between their runs ...).
    pub warnings: Vec<String>,

    /// The faces the ends meet (`WallInput::end_planes`, in the plan's direction).
    pub end_planes: [Option<[f64; 2]>; 2],
}

impl Plan {
    /// The walkway's height at a sample.
    pub fn walkway(&self, i: usize, profile: &Profile) -> f64 {
        self.samples[i].level + profile.walkway
    }
}

/// Which points are corners: the ends, the points marked sharp, and the points whose internal angle is under [`SHARP_ANGLE`].
pub fn corners(points: &[[f64; 2]], sharp: &[bool]) -> Vec<bool> {
    let n = points.len();
    (0..n).map(|i| {
        if i == 0 || i + 1 == n || sharp.get(i).copied().unwrap_or(false) {
            return true;
        }

        let (a, b, c) = (points[i - 1], points[i], points[i + 1]);
        let (u, v) = ([a[0] - b[0], a[1] - b[1]], [c[0] - b[0], c[1] - b[1]]);
        let (lu, lv) = (u[0].hypot(u[1]), v[0].hypot(v[1]));
        if lu < 1e-9 || lv < 1e-9 {
            return true;
        }

        let cos = ((u[0] * v[0] + u[1] * v[1]) / (lu * lv)).clamp(-1.0, 1.0);
        cos.acos().to_degrees() < SHARP_ANGLE
    }).collect()
}

/// Lays out a wall. `ground(x, z)` is the terrain's height.
pub fn solve(input: &WallInput, profile: &Profile, ground: &dyn Fn(f64, f64) -> f64) -> Plan {
    if input.flip {
        let n = input.points.len();
        let reversed = WallInput {
            points: input.points.iter().rev().copied().collect(),
            sharp: (0..n).rev().map(|i| input.sharp.get(i).copied().unwrap_or(false)).collect(),
            flip: false,
            tolerance: input.tolerance,
            ends: [input.ends[1], input.ends[0]],
            end_planes: [input.end_planes[1], input.end_planes[0]],
        };

        // The corners name the points as the input does.
        let mut plan = solve(&reversed, profile, ground);
        for c in &mut plan.corners {
            c.1 = n - 1 - c.1;
        }

        return plan;
    }

    let mut plan = Plan { end_planes: input.end_planes, ..Plan::default() };
    if input.points.len() < 2 {
        return plan;
    }

    // The centre line: Catmull-Rom between corners, then resampled by distance.
    let is_corner = corners(&input.points, &input.sharp);
    let mut line: Vec<([f64; 2], Option<usize>)> = vec![];
    let breaks = is_corner.iter().enumerate().filter(|(_, c)| **c).map(|(i, _)| i).collect::<Vec<_>>();
    for w in breaks.windows(2) {
        let section = &input.points[w[0]..=w[1]];
        let dense = catmull_rom(section, 16);
        let skip = usize::from(!line.is_empty());
        for (k, p) in dense.into_iter().enumerate().skip(skip) {
            let at = if k == 0 { Some(w[0]) } else if k == (section.len() - 1) * 16 { Some(w[1]) } else { None };
            line.push((p, at));
        }
    }

    let (samples, corner_s) = resample(&line);
    plan.length = samples.last().map_or(0.0, |s| s.0);
    plan.corners = corner_s.into_iter().filter(|&(_, i)| i != 0 && i + 1 != input.points.len()).collect();

    // A mitre shears its pieces by tan(turn / 2) per unit across the wall: past about 150 degrees it is clamped.
    for &(s, i) in &plan.corners {
        let (a, b, c) = (input.points[i - 1], input.points[i], input.points[i + 1]);
        let (u, v) = ([b[0] - a[0], b[1] - a[1]], [c[0] - b[0], c[1] - b[1]]);
        let turn = (u[0] * v[1] - u[1] * v[0]).atan2(u[0] * v[0] + u[1] * v[1]).abs().to_degrees();
        if turn > 150.0 {
            plan.warnings.push(format!("the corner at {s:.0} (point {i}) turns {turn:.0} degrees: its mitre is clamped, the pieces overlap"));
        }
    }

    // The outer foot and the ground there. The pieces' local +z is (-dz, dx) of their direction; the outer face is at `outer_face` on it.
    plan.samples = samples.iter().enumerate().map(|(i, &(s, c))| {
        let dir = direction(&samples, i);
        let z_axis = [-dir[1], dir[0]];
        let foot = [c[0] + z_axis[0] * profile.outer_face, c[1] + z_axis[1] * profile.outer_face];
        Sample { s, centre: c, dir, foot, ground: ground(foot[0], foot[1]), level: 0.0 }
    }).collect();

    plan.runs = runs(&plan.samples, input.tolerance, input.ends);
    plan.steps = steps(&plan.runs, profile, &mut plan.warnings);
    separate(&mut plan.steps, plan.length, &mut plan.warnings);
    for sm in &mut plan.samples {
        sm.level = level_at(&plan.runs, &plan.steps, sm.s);
    }

    plan
}

/// A centripetal Catmull-Rom through the points, `per` samples per segment (the ends extended by reflection).
fn catmull_rom(points: &[[f64; 2]], per: usize) -> Vec<[f64; 2]> {
    let n = points.len();
    if n == 2 {
        return (0..=per).map(|k| lerp(points[0], points[1], k as f64 / per as f64)).collect();
    }

    let get = |i: isize| -> [f64; 2] {
        if i < 0 {
            let (a, b) = (points[0], points[1]);
            [2.0 * a[0] - b[0], 2.0 * a[1] - b[1]]
        } else if i as usize >= n {
            let (a, b) = (points[n - 1], points[n - 2]);
            [2.0 * a[0] - b[0], 2.0 * a[1] - b[1]]
        } else {
            points[i as usize]
        }
    };

    let mut out = vec![points[0]];
    for i in 0..n - 1 {
        let (p0, p1, p2, p3) = (get(i as isize - 1), get(i as isize), get(i as isize + 1), get(i as isize + 2));
        let knot = |a: [f64; 2], b: [f64; 2]| (a[0] - b[0]).hypot(a[1] - b[1]).sqrt().max(1e-6);
        let (t1, t2, t3) = (knot(p0, p1), knot(p1, p2), knot(p2, p3));
        let (t0, t1, t2, t3) = (0.0, t1, t1 + t2, t1 + t2 + t3);
        for k in 1..=per {
            let t = t1 + (t2 - t1) * k as f64 / per as f64;
            let a1 = mix(p0, p1, (t1 - t) / (t1 - t0), (t - t0) / (t1 - t0));
            let a2 = mix(p1, p2, (t2 - t) / (t2 - t1), (t - t1) / (t2 - t1));
            let a3 = mix(p2, p3, (t3 - t) / (t3 - t2), (t - t2) / (t3 - t2));
            let b1 = mix(a1, a2, (t2 - t) / (t2 - t0), (t - t0) / (t2 - t0));
            let b2 = mix(a2, a3, (t3 - t) / (t3 - t1), (t - t1) / (t3 - t1));
            out.push(mix(b1, b2, (t2 - t) / (t2 - t1), (t - t1) / (t2 - t1)));
        }
    }

    out
}

/// The line resampled every [`STEP`] (the last sample at its end), with the distance of each point it passed that was a spline point.
fn resample(line: &[([f64; 2], Option<usize>)]) -> (Vec<(f64, [f64; 2])>, Vec<(f64, usize)>) {
    let mut cum = vec![0.0];
    for w in line.windows(2) {
        let (a, b) = (w[0].0, w[1].0);
        cum.push(cum.last().unwrap_or(&0.0) + (b[0] - a[0]).hypot(b[1] - a[1]));
    }

    let total = *cum.last().unwrap_or(&0.0);
    let marks = line.iter().zip(&cum).filter_map(|((_, at), &s)| at.map(|i| (s, i))).collect::<Vec<_>>();
    let count = (total / STEP).ceil().max(1.0) as usize;
    let mut out = Vec::with_capacity(count + 1);
    let mut j = 0;
    for k in 0..=count {
        let s = (k as f64 * STEP).min(total);
        while j + 2 < cum.len() && cum[j + 1] < s {
            j += 1;
        }

        let seg = (cum[j + 1] - cum[j]).max(1e-12);
        out.push((s, lerp(line[j].0, line[j + 1].0, ((s - cum[j]) / seg).clamp(0.0, 1.0))));
    }

    (out, marks)
}

/// The unit direction at a sample (central difference).
fn direction(samples: &[(f64, [f64; 2])], i: usize) -> [f64; 2] {
    let (a, b) = (samples[i.saturating_sub(1)].1, samples[(i + 1).min(samples.len() - 1)].1);
    let d = [b[0] - a[0], b[1] - a[1]];
    let l = d[0].hypot(d[1]);
    if l < 1e-12 { [1.0, 0.0] } else { [d[0] / l, d[1] / l] }
}

/// Flat runs: each at the ground at its middle, grown while every sample under it is within `tolerance` (or is shorter than
/// [`MIN_RUN`]). An attached end's run is at the given level instead.
fn runs(samples: &[Sample], tolerance: f64, ends: [Option<f64>; 2]) -> Vec<Run> {
    let n = samples.len();
    let mut out = vec![];
    let mut i = 0;
    while i < n {
        let fixed = if i == 0 { ends[0] } else { None };
        let level_of = |j: usize| fixed.unwrap_or(samples[(i + j) / 2].ground);
        let fits = |j: usize| {
            let level = level_of(j);
            samples[i..=j].iter().all(|sm| (sm.ground - level).abs() <= tolerance)
        };

        let mut j = i;
        while j + 1 < n && (samples[j + 1].s - samples[i].s < MIN_RUN || fits(j + 1)) {
            j += 1;
        }

        let level = level_of(j);
        let drift = samples[i..=j].iter().map(|sm| (sm.ground - level).abs()).fold(0.0, f64::max);
        out.push(Run { s: (samples[i].s, samples[j].s), level, drift });
        i = j + 1;
    }

    // The far end attached: its run is at the given level.
    if let (Some(h), Some(last)) = (ends[1], out.last_mut()) {
        last.level = h;
    }

    // Neighbouring runs at the same level are one.
    let mut merged: Vec<Run> = vec![];
    for r in out {
        match merged.last_mut() {
            Some(m) if (m.level - r.level).abs() < 1e-9 => {
                m.s.1 = r.s.1;
                m.drift = m.drift.max(r.drift);
            }
            _ => merged.push(r),
        }
    }

    merged
}

/// The step sections between runs: as many as the rise needs (each at most the base's rise), the base's run long, `step_gap` apart,
/// centred on the boundary between the runs, shifted to stay inside them.
fn steps(runs: &[Run], profile: &Profile, warnings: &mut Vec<String>) -> Vec<Step> {
    let mut out = vec![];
    for (chain, w) in runs.windows(2).enumerate() {
        let (a, b) = (w[0], w[1]);
        let rise = b.level - a.level;
        let count = (rise.abs() / profile.step_rise).ceil().max(1.0) as usize;
        let span = count as f64 * profile.step_run + (count - 1) as f64 * profile.step_gap;
        let boundary = (a.s.1 + b.s.0) / 2.0;
        let room = (a.s.1 - a.s.0) / 2.0 + (b.s.1 - b.s.0) / 2.0;
        if span > room + 1e-9 {
            warnings.push(format!("at {boundary:.0}: {count} step section(s) for {rise:.1} need {span:.0} along the wall, the runs around leave {room:.0}"));
        }

        // Centred on the boundary, but inside the two runs (so inside the wall: a short run at an attached end does not push the steps
        // past the end, where the wall would start at the steps' level instead of the building's).
        let mut s = (boundary - span / 2.0).clamp(a.s.0, (b.s.1 - span).max(a.s.0));

        for k in 0..count {
            let (from, to) = (a.level + rise * k as f64 / count as f64, a.level + rise * (k + 1) as f64 / count as f64);
            out.push(Step { s: (s, s + profile.step_run), from, to, chain });
            s += profile.step_run + profile.step_gap;
        }
    }

    out
}

/// Chains of steps one after the other: a chain that starts before the previous one ends (ground climbing faster than the steps can
/// follow) is moved on past it, and back from the wall's end if that takes it past (with a warning: its runs are shorter than its steps
/// need).
fn separate(steps: &mut [Step], length: f64, warnings: &mut Vec<String>) {
    let chains = steps.iter().map(|st| st.chain).max().map_or(0, |c| c + 1);
    let mut end_of_prev = f64::MIN;
    for c in 0..chains {
        let (lo, hi) = steps.iter().filter(|st| st.chain == c).fold((f64::MAX, f64::MIN), |(lo, hi), st| (lo.min(st.s.0), hi.max(st.s.1)));
        if lo > hi {
            continue;
        }

        let mut shift = (end_of_prev - lo).max(0.0);
        if hi + shift > length {
            shift = length - hi;
        }

        if shift.abs() > 1e-9 {
            for st in steps.iter_mut().filter(|st| st.chain == c) {
                st.s = (st.s.0 + shift, st.s.1 + shift);
            }

            if lo + shift < end_of_prev - 1e-6 {
                warnings.push(format!("at {:.0}: the ground climbs faster than the steps can follow; step sections overlap", lo + shift));
            }
        }

        end_of_prev = hi + shift;
    }
}

/// The level at a distance: inside a chain of steps (from its first section's start to its last one's end), the ramp or, between two
/// sections, the lower one's top; elsewhere the run's level.
fn level_at(runs: &[Run], steps: &[Step], s: f64) -> f64 {
    let chains = steps.iter().map(|st| st.chain).max().map_or(0, |c| c + 1);
    for c in 0..chains {
        let chain = steps.iter().filter(|st| st.chain == c).collect::<Vec<_>>();
        let (Some(first), Some(last)) = (chain.first(), chain.last()) else { continue };
        if s < first.s.0 || s > last.s.1 {
            continue;
        }

        if let Some(st) = chain.iter().find(|st| s >= st.s.0 && s <= st.s.1) {
            return st.from + (st.to - st.from) * (s - st.s.0) / (st.s.1 - st.s.0).max(1e-9);
        }

        return chain.iter().rev().find(|st| st.s.1 < s).map_or(first.from, |st| st.to);
    }

    runs.iter().find(|r| s <= r.s.1 + STEP / 2.0).or(runs.last()).map_or(0.0, |r| r.level)
}

fn lerp(a: [f64; 2], b: [f64; 2], t: f64) -> [f64; 2] {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t]
}

fn mix(a: [f64; 2], b: [f64; 2], wa: f64, wb: f64) -> [f64; 2] {
    [a[0] * wa + b[0] * wb, a[1] * wa + b[1] * wb]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profile::profile;

    fn eight() -> &'static Profile {
        profile("barbarian_fort_straight").unwrap()
    }

    fn wall(points: &[[f64; 2]]) -> WallInput {
        WallInput { points: points.to_vec(), sharp: vec![false; points.len()], tolerance: 0.5, ..Default::default() }
    }

    #[test]
    fn a_straight_wall_on_flat_ground_is_one_run() {
        let plan = solve(&wall(&[[0.0, 0.0], [100.0, 0.0]]), eight(), &|_, _| 3.0);
        assert!((plan.length - 100.0).abs() < 1e-6);
        assert_eq!(plan.runs.len(), 1);
        assert!(plan.steps.is_empty());
        assert!(plan.samples.iter().all(|s| (s.level - 3.0).abs() < 1e-9));
    }

    #[test]
    fn the_outer_face_is_on_the_pieces_minus_z_side() {
        // Along +x the pieces' +z is +z of the world: the 8 m outer face (z = -2.5) is at world z = -2.5; flipped, at +2.5.
        let plan = solve(&wall(&[[0.0, 0.0], [40.0, 0.0]]), eight(), &|_, _| 0.0);
        assert!((plan.samples[10].foot[1] - -2.5).abs() < 1e-9);
        let plan = solve(&WallInput { flip: true, ..wall(&[[0.0, 0.0], [40.0, 0.0]]) }, eight(), &|_, _| 0.0);
        assert!((plan.samples[10].foot[1] - 2.5).abs() < 1e-9);
    }

    #[test]
    fn a_slope_is_climbed_by_steps_and_the_walkway_stays_near_the_ground() {
        // 10 up over 200: steps every ~ 2 * tolerance of rise.
        let input = wall(&[[0.0, 0.0], [200.0, 0.0]]);
        let ground = |x: f64, _: f64| x * 0.05;
        let plan = solve(&input, eight(), &ground);
        assert!(plan.steps.len() >= 8, "{} steps", plan.steps.len());
        assert!(plan.steps.iter().all(|s| (s.to - s.from).abs() <= eight().step_rise + 1e-9));

        // Away from the steps the level is within the tolerance (plus one sample's slope) of the ground.
        for sm in &plan.samples {
            let on_step = plan.steps.iter().any(|st| sm.s >= st.s.0 - 1.0 && sm.s <= st.s.1 + 1.0);
            if !on_step {
                assert!((sm.level - sm.ground).abs() <= 0.5 + 0.05, "at {}: level {} ground {}", sm.s, sm.level, sm.ground);
            }
        }

        // The walkway only goes up.
        assert!(plan.samples.windows(2).all(|w| w[1].level >= w[0].level - 1e-9));
    }

    #[test]
    fn a_cliff_takes_chained_steps_no_higher_than_the_base_allows() {
        let ground = |x: f64, _: f64| if x < 100.0 { 0.0 } else { 6.0 };
        let plan = solve(&wall(&[[0.0, 0.0], [200.0, 0.0]]), eight(), &ground);
        assert_eq!(plan.steps.len(), 3);
        assert!((plan.steps[2].to - 6.0).abs() < 1e-9);
        assert!(plan.steps.windows(2).all(|w| (w[1].s.0 - w[0].s.1 - eight().step_gap).abs() < 1e-9));
    }

    #[test]
    fn steps_stay_inside_the_wall_next_to_a_short_attached_run() {
        // The building is 3 below the ground: the first run is short, the chain after it must not start before the wall.
        let input = WallInput { ends: [Some(0.0), None], ..wall(&[[0.0, 0.0], [100.0, 0.0]]) };
        let plan = solve(&input, eight(), &|_, _| 3.0);
        assert!(plan.steps.iter().all(|st| st.s.0 >= 0.0 && st.s.1 <= plan.length + 1e-9), "{:?}", plan.steps);
        assert!(plan.samples[0].level.abs() < 1e-9, "the wall starts at the building's level: {}", plan.samples[0].level);
    }

    #[test]
    fn attached_ends_hold_their_level() {
        let input = WallInput { ends: [Some(2.0), Some(4.0)], ..wall(&[[0.0, 0.0], [100.0, 0.0]]) };
        let plan = solve(&input, eight(), &|_, _| 3.0);
        assert!((plan.samples[0].level - 2.0).abs() < 1e-9);
        assert!((plan.samples.last().unwrap().level - 4.0).abs() < 1e-9);
        assert!(!plan.steps.is_empty());
    }

    #[test]
    fn corners_are_the_marked_points_and_the_sharp_turns() {
        // 90 degrees: not a corner unless marked; 20 degrees internal: a corner.
        let pts = [[0.0, 0.0], [50.0, 0.0], [50.0, 50.0], [0.0, 32.0]];
        let c = corners(&pts, &[false; 4]);
        assert_eq!(c, vec![true, false, false, true]);
        let pts2 = [[0.0, 0.0], [50.0, 0.0], [0.0, 18.0]];
        assert_eq!(corners(&pts2, &[false; 3]), vec![true, true, true]);
        assert_eq!(corners(&pts, &[false, true, false, false]), vec![true, true, false, true]);

        // A marked corner keeps the line through its point with a kink; a smooth point bends it.
        let mut input = wall(&[[0.0, 0.0], [50.0, 0.0], [50.0, 50.0]]);
        input.sharp[1] = true;
        let plan = solve(&input, eight(), &|_, _| 0.0);
        assert_eq!(plan.corners.len(), 1);
        assert!((plan.corners[0].0 - 50.0).abs() < 1e-6);
        assert!((plan.length - 100.0).abs() < 1e-6);
        let smooth = solve(&wall(&[[0.0, 0.0], [50.0, 0.0], [50.0, 50.0]]), eight(), &|_, _| 0.0);
        assert!(smooth.corners.is_empty());
        assert!(smooth.length > 100.0, "the curve overshoots the corner: {}", smooth.length);
    }
}
