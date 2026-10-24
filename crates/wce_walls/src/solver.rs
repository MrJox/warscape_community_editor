//! A wall's spline to its layout (docs/plans/procedural_walls.md, "The solver"): the centre line, its corners, the ground along the outer
//! face and the stepped walkway. Scene units throughout (x, z horizontal, y up; the models' units). Pure: the ground comes in as a
//! function, so the editor and the tests share it.
//!
//! - **Centre line:** the points in order. A point is a corner when it is marked sharp or its internal angle is under
//!   [`SHARP_ANGLE`]; between corners (and the ends) the line is a centripetal Catmull-Rom through the points. Sampled every [`STEP`].
//! - **Outer foot:** the centre line moved to the base's outer face (its local -z side, or +z with `flip`); the ground is read there.
//! - **Runs:** flat stretches of walkway, each at the ground at its middle, as long as the ground stays within `tolerance` of it and at
//!   least the base's `min_run` long; an attached end's run is at the height it was given.
//! - **Steps:** between two runs, as many stairs as the rise needs (each at most the base's `step_rise`, the rise shared equally), each as
//!   long as its stair at its own slope or, when that is steeper than the base's `max_slope`, as the slope allows; chained `step_gap`
//!   apart, centred on the boundary between the runs.
//! - **Joints with the game's pieces:** the towers and bastions are chosen (`pieces::find_defence_slots`), then the centre line is made
//!   straight under them and leaves an attached end along the building's axis ([`straighten`]): every joint with a game's piece is then
//!   exact in place and direction, as the game's own walls are (in game, a joint off by a fraction of a unit or a degree let ladders and
//!   towers dock while units could not cross onto the wall).

use crate::profile::Profile;

/// Samples along the wall, in scene units.
pub const STEP: f64 = 0.5;

/// A point whose internal angle is at most this (degrees) is a corner even when it is not marked sharp (a right angle or sharper).
pub const SHARP_ANGLE: f64 = 90.0;

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

    /// The spacing of the access pieces (15 m: the door sections) and of the towers and bastions along the wall; 0: none.
    pub access_every: f64,
    pub defence_every: f64,
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

    /// A whole step piece (15 m: the stepped bastion) instead of a stair: the walls's level under it is the lower one of its two.
    pub whole: bool,
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

    /// The ends attached to nothing (closed with the base's caps), in the plan's direction.
    pub free_ends: [bool; 2],

    /// `WallInput::access_every`, `defence_every`.
    pub access_every: f64,
    pub defence_every: f64,

    /// The towers and bastions (their start along the wall, and key), and the places skipped with why.
    pub defences: Vec<(f64, &'static str)>,
    pub defences_skipped: Vec<String>,
}

impl Plan {
    /// The walkway's height at a sample.
    pub fn walkway(&self, i: usize, profile: &Profile) -> f64 {
        self.samples[i].level + profile.walkway
    }
}

/// Which points are corners: the ends, the points marked sharp, and the points whose internal angle is at most [`SHARP_ANGLE`].
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
        cos.acos().to_degrees() <= SHARP_ANGLE + 1e-6
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
            access_every: input.access_every,
            defence_every: input.defence_every,
        };

        // The corners name the points as the input does.
        let mut plan = solve(&reversed, profile, ground);
        for c in &mut plan.corners {
            c.1 = n - 1 - c.1;
        }

        return plan;
    }

    let mut plan = Plan { end_planes: input.end_planes, free_ends: [input.ends[0].is_none(), input.ends[1].is_none()], access_every: input.access_every, defence_every: input.defence_every, ..Plan::default() };
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

    // A mitre shears its pieces by tan(turn / 2) per unit across the wall: past about 168 degrees it is clamped (`pieces::MAX_SHEAR`).
    for &(s, i) in &plan.corners {
        let (a, b, c) = (input.points[i - 1], input.points[i], input.points[i + 1]);
        let (u, v) = ([b[0] - a[0], b[1] - a[1]], [c[0] - b[0], c[1] - b[1]]);
        let turn = (u[0] * v[1] - u[1] * v[0]).atan2(u[0] * v[0] + u[1] * v[1]).abs().to_degrees();
        if turn > 168.0 {
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

    plan.runs = runs(&plan.samples, input.tolerance, profile.min_run.max(MIN_RUN), input.ends);
    let corner_s = plan.corners.iter().map(|&(s, _)| s).collect::<Vec<_>>();
    let fixed = [input.ends[0].is_some(), input.ends[1].is_some()];
    plan.steps = steps(&mut plan.runs, &plan.samples, &corner_s, fixed, plan.length, profile, &mut plan.warnings);
    separate(&mut plan.steps, plan.length, &mut plan.warnings);
    for sm in &mut plan.samples {
        sm.level = level_at(&plan.runs, &plan.steps, sm.s);
    }

    let (defences, skipped) = crate::pieces::find_defence_slots(&plan, profile);
    plan.defences = defences;
    plan.defences_skipped = skipped;
    straighten(&mut plan, profile, ground);
    plan
}

/// A straight stretch of the centre line: from, to, the line's point at `from`, its direction.
type Straight = (f64, f64, [f64; 2], [f64; 2]);

/// The centre line made straight where a game's piece joins it: over each tower or bastion (the line through its start along the
/// direction there, so its far end meets the next piece exactly), and at an attached end (the line leaves along the building's axis,
/// the normal of the face it meets: the joint is straight, as every joint of the game's walls). Around each, the line blends back into
/// the spline (the straight line's continuation, weighted by a smoothstep, so place and direction are continuous) over a base's length
/// at most, half the way to the next such stretch at most, and within its section (not across a corner). The samples' directions, outer
/// feet and ground follow; the runs and steps are kept (the line moves a fraction of a unit).
fn straighten(plan: &mut Plan, profile: &Profile, ground: &dyn Fn(f64, f64) -> f64) {
    let n = plan.samples.len();
    if n < 2 {
        return;
    }

    let len = profile.length();
    let unit = |v: [f64; 2]| {
        let l = v[0].hypot(v[1]).max(1e-12);
        [v[0] / l, v[1] / l]
    };

    let outward = |m: [f64; 2], d: [f64; 2]| {
        let m = unit(m);
        if m[0] * d[0] + m[1] * d[1] < 0.0 { [-m[0], -m[1]] } else { m }
    };

    let (first, last) = (plan.samples[0], plan.samples[n - 1]);
    let mut fixed: Vec<Straight> = vec![];
    if let Some(m) = plan.end_planes[0] {
        fixed.push((0.0, 0.0, first.centre, outward(m, first.dir)));
    }

    for &(p0, _) in &plan.defences {
        let at = crate::pieces::sample_at(plan, p0);
        fixed.push((p0, p0 + len, at.centre, at.dir));
    }

    if let Some(m) = plan.end_planes[1] {
        fixed.push((plan.length, plan.length, last.centre, outward(m, last.dir)));
    }

    if fixed.is_empty() {
        return;
    }

    fixed.sort_by(|a, b| a.0.total_cmp(&b.0));
    let sections = crate::pieces::sections(plan);
    let section_of = |s: f64| sections.iter().copied().find(|&(a, b)| s >= a - 1e-9 && s <= b + 1e-9).unwrap_or((0.0, plan.length));
    let smooth = |t: f64| {
        let t = t.clamp(0.0, 1.0);
        t * t * (3.0 - 2.0 * t)
    };

    let on = |f: &Straight, s: f64| [f.2[0] + f.3[0] * (s - f.0), f.2[1] + f.3[1] * (s - f.0)];
    let old = plan.samples.clone();
    let mut straight = vec![false; n];
    for (i, sm) in plan.samples.iter_mut().enumerate() {
        let s = old[i].s;
        if let Some(f) = fixed.iter().find(|f| s >= f.0 - 1e-9 && s <= f.1 + 1e-9) {
            sm.centre = on(f, s);
            sm.dir = f.3;
            straight[i] = true;
            continue;
        }

        let (a, b) = section_of(s);
        let c = old[i].centre;
        let mut pull = [0.0, 0.0];

        // The stretch before: its line going on.
        if let Some(f) = fixed.iter().rev().find(|f| f.1 <= s && f.1 >= a - 1e-9) {
            let next = fixed.iter().find(|g| g.0 > f.1 + 1e-9).map_or(b, |g| g.0.min(b));
            let reach = len.min((next - f.1) / 2.0).min(b - f.1);
            if reach > 1e-6 && s - f.1 < reach {
                let (w, p) = (1.0 - smooth((s - f.1) / reach), on(f, s));
                pull = [pull[0] + w * (p[0] - c[0]), pull[1] + w * (p[1] - c[1])];
            }
        }

        // The stretch after: its line coming back.
        if let Some(g) = fixed.iter().find(|g| g.0 >= s && g.0 <= b + 1e-9) {
            let prev = fixed.iter().rev().find(|f| f.1 < g.0 - 1e-9).map_or(a, |f| f.1.max(a));
            let reach = len.min((g.0 - prev) / 2.0).min(g.0 - a);
            if reach > 1e-6 && g.0 - s < reach {
                let (w, p) = (1.0 - smooth((g.0 - s) / reach), on(g, s));
                pull = [pull[0] + w * (p[0] - c[0]), pull[1] + w * (p[1] - c[1])];
            }
        }

        sm.centre = [c[0] + pull[0], c[1] + pull[1]];
    }

    let centres = plan.samples.iter().map(|sm| sm.centre).collect::<Vec<_>>();
    for (i, sm) in plan.samples.iter_mut().enumerate() {
        if !straight[i] {
            let (p, q) = (centres[i.saturating_sub(1)], centres[(i + 1).min(n - 1)]);
            sm.dir = unit([q[0] - p[0], q[1] - p[1]]);
        }

        let z_axis = [-sm.dir[1], sm.dir[0]];
        sm.foot = [sm.centre[0] + z_axis[0] * profile.outer_face, sm.centre[1] + z_axis[1] * profile.outer_face];
        sm.ground = ground(sm.foot[0], sm.foot[1]);
    }
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

/// Flat runs: each at the ground at its middle, grown while every sample under it is within `tolerance` or it is shorter than `min_run`.
/// An attached end's run is at the given level instead.
fn runs(samples: &[Sample], tolerance: f64, min_run: f64, ends: [Option<f64>; 2]) -> Vec<Run> {
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
        while j + 1 < n && (samples[j + 1].s - samples[i].s < min_run || fits(j + 1)) {
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

/// How long a step of `rise` is along the wall: its stair at its own slope, or longer when that is steeper than the wall may slope.
pub fn step_length(profile: &Profile, rise: f64) -> f64 {
    (profile.step_run * rise.abs() / profile.step_rise).max(rise.abs() / profile.max_slope)
}

/// A whole step is looked for over boundaries at most this far apart (the climb gathered into one bastion).
pub const WHOLE_WINDOW: f64 = 40.0;

/// A stretch is straight enough for a whole step when its direction turns at most this much over the step (radians, about 4 degrees).
pub const STRAIGHT: f64 = 0.07;

/// A stair placed on a curve that turns more than this over its length (radians, about 6 degrees) is penalised: steps go on the
/// straightest place their runs allow.
pub const CURVE: f64 = 0.1;

/// The direction at a distance.
fn dir_at(samples: &[Sample], s: f64) -> [f64; 2] {
    let i = ((s / STEP).round().max(0.0) as usize).min(samples.len().saturating_sub(1));
    samples.get(i).map_or([1.0, 0.0], |sm| sm.dir)
}

/// How much the direction turns between two distances (radians).
pub fn turn_between(samples: &[Sample], a: f64, b: f64) -> f64 {
    let n = ((b - a) / STEP).ceil().max(1.0) as usize;
    let mut total = 0.0;
    let mut prev = dir_at(samples, a);
    for k in 1..=n {
        let d = dir_at(samples, a + (b - a) * k as f64 / n as f64);
        total += (prev[0] * d[1] - prev[1] * d[0]).atan2(prev[0] * d[0] + prev[1] * d[1]).abs();
        prev = d;
    }

    total
}

/// The steps between runs. A climb of about `whole_rise` over boundaries at most [`WHOLE_WINDOW`] apart, on a straight stretch, is one
/// whole step piece (exactly `whole_rise`; the runs between are merged and the next one takes its level). Otherwise, per boundary, as
/// many stairs as the rise needs (each at most the base's `step_rise`, the rise shared equally), each [`step_length`] long, `step_gap`
/// apart. A chain goes where its runs allow it, away from corners and attached ends (their mitres; by the wall's depth), the straightest
/// place first, then the nearest to the boundary.
fn steps(runs: &mut Vec<Run>, samples: &[Sample], corners: &[f64], fixed: [bool; 2], length: f64, profile: &Profile, warnings: &mut Vec<String>) -> Vec<Step> {
    let margin = profile.depth.0.abs().max(profile.depth.1.abs()) + 2.0;
    let mut blocked = corners.iter().map(|&c| (c - margin, c + margin)).collect::<Vec<_>>();
    if fixed[0] {
        blocked.push((f64::MIN, margin));
    }

    if fixed[1] {
        blocked.push((length - margin, f64::MAX));
    }

    let free = |a: f64, b: f64| !blocked.iter().any(|&(x, y)| a < y && b > x);

    // Whole steps first: they merge runs.
    let mut wholes = vec![];
    if profile.whole_rise > 0.0 {
        let len = profile.length();
        let mut i = 0;
        while i + 1 < runs.len() {
            let bnd = |k: usize, runs: &[Run]| (runs[k].s.1 + runs[k + 1].s.0) / 2.0;
            let sign = (runs[i + 1].level - runs[i].level).signum();
            let mut best: Option<(usize, f64)> = None;
            for j in i..runs.len() - 1 {
                let d = runs[j + 1].level - runs[j].level;
                if d.signum() != sign || bnd(j, runs) - bnd(i, runs) > WHOLE_WINDOW {
                    break;
                }

                let total = (runs[j + 1].level - runs[i].level).abs();
                if total >= 0.7 * profile.whole_rise && total <= 1.3 * profile.whole_rise {
                    // The straight place nearest the middle of the climb, inside its first and last runs.
                    let mid = (bnd(i, runs) + bnd(j, runs)) / 2.0;
                    let (lo, hi) = (runs[i].s.0, runs[j + 1].s.1 - len);
                    let mut t = lo;
                    let mut found: Option<f64> = None;
                    while t <= hi {
                        if free(t, t + len) && turn_between(samples, t, t + len) <= STRAIGHT && found.is_none_or(|f| (t + len / 2.0 - mid).abs() < (f + len / 2.0 - mid).abs()) {
                            found = Some(t);
                        }
                        t += STEP;
                    }

                    if let Some(t) = found {
                        best = Some((j, t));
                    }
                }
            }

            if let Some((j, t)) = best {
                let to = runs[i].level + sign * profile.whole_rise;
                runs[i].s.1 = t;
                runs[j + 1].s.0 = t + len;
                runs[j + 1].level = to;
                runs.drain(i + 1..=j);
                wholes.push((i, t));
            }

            i += 1;
        }
    }

    let mut out = vec![];
    for chain in 0..runs.len().saturating_sub(1) {
        let (a, b) = (runs[chain], runs[chain + 1]);
        if let Some(&(_, t)) = wholes.iter().find(|w| w.0 == chain) {
            out.push(Step { s: (t, t + profile.length()), from: a.level, to: b.level, chain, whole: true });
            continue;
        }

        let rise = b.level - a.level;
        let count = (rise.abs() / profile.step_rise - 1e-9).ceil().max(1.0) as usize;
        let each = step_length(profile, rise / count as f64);
        let span = count as f64 * each + (count - 1) as f64 * profile.step_gap;
        let boundary = (a.s.1 + b.s.0) / 2.0;
        let room = (a.s.1 - a.s.0) / 2.0 + (b.s.1 - b.s.0) / 2.0;
        if span > room + 1e-9 {
            warnings.push(format!("at {boundary:.0}: {count} step section(s) for {rise:.1} need {span:.0} along the wall, the runs around leave {room:.0}"));
        }

        // The place: from the middle of the run before to the middle of the run after (the first and the last run: their ends), keeping
        // a quarter of `min_run` flat on each side so neighbouring steps never meet; not on a corner or an attached end; the straightest,
        // then the nearest to the boundary.
        let q = profile.min_run / 4.0;
        let n = runs.len();
        let lo = if chain == 0 { a.s.0 } else { (a.s.0 + a.s.1) / 2.0 + q };
        let hi_end = if chain + 2 == n { b.s.1 } else { (b.s.0 + b.s.1) / 2.0 - q };
        let (lo, hi) = if hi_end - span >= lo { (lo, hi_end - span) } else { (a.s.0, (b.s.1 - span).max(a.s.0)) };
        let cost = |t: f64| 100.0 * (turn_between(samples, t, t + span) - CURVE).max(0.0) + (t + span / 2.0 - boundary).abs() * 0.01;
        let mut t = lo;
        let mut best: Option<f64> = None;
        while t <= hi + 1e-9 {
            if free(t, t + span) && best.is_none_or(|b| cost(t) < cost(b)) {
                best = Some(t);
            }
            t += STEP;
        }

        let mut s = best.unwrap_or_else(|| {
            warnings.push(format!("at {boundary:.0}: no place for the step(s) away from a corner or an attached end"));
            (boundary - span / 2.0).clamp(a.s.0, (b.s.1 - span).max(a.s.0))
        });

        for k in 0..count {
            let (from, to) = (a.level + rise * k as f64 / count as f64, a.level + rise * (k + 1) as f64 / count as f64);
            out.push(Step { s: (s, s + each), from, to, chain, whole: false });
            s += each + profile.step_gap;
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

/// The level at a distance, exactly (not interpolated between samples: a whole step's level jumps at its ends).
pub fn wall_level(plan: &Plan, s: f64) -> f64 {
    level_at(&plan.runs, &plan.steps, s)
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
            // A whole step piece stands on the lower level and climbs by itself.
            if st.whole {
                return st.from.min(st.to);
            }

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
        let input = wall(&[[0.0, 0.0], [200.0, 0.0]]);
        let ground = |x: f64, _: f64| x * 0.05;
        let plan = solve(&input, eight(), &ground);
        assert!(!plan.steps.is_empty());

        // Each step at most one stair, as long as its stair at its own slope (8 m: the wall follows the stair's slope).
        for st in &plan.steps {
            let r = (st.to - st.from).abs();
            assert!(r <= eight().step_rise + 1e-9);
            assert!(((st.s.1 - st.s.0) - eight().step_run * r / eight().step_rise).abs() < 1e-9);
        }

        // Away from the steps the walkway is within the tolerance of the ground (a run is at least min_run long, so a little more there).
        for sm in &plan.samples {
            let near_step = plan.steps.iter().any(|st| sm.s >= st.s.0 - 5.0 && sm.s <= st.s.1 + 5.0);
            if !near_step {
                assert!((sm.level - sm.ground).abs() <= 0.5 + 0.05 * eight().min_run, "at {}: level {} ground {}", sm.s, sm.level, sm.ground);
            }
        }

        // The walkway only goes up; runs between steps no shorter than min_run (no pile-ups).
        assert!(plan.samples.windows(2).all(|w| w[1].level >= w[0].level - 1e-9));
        assert!(plan.runs.iter().skip(1).take(plan.runs.len().saturating_sub(2)).all(|r| r.s.1 - r.s.0 >= eight().min_run - 1.0));
    }

    #[test]
    fn a_15_m_wall_slopes_gently_over_its_steps() {
        let p = profile("roman_fort_straight").unwrap();
        let plan = solve(&wall(&[[0.0, 0.0], [400.0, 0.0]]), p, &|x, _| x * 0.02);
        assert!(!plan.steps.is_empty());
        for st in &plan.steps {
            let slope = (st.to - st.from).abs() / (st.s.1 - st.s.0);
            assert!(slope <= p.max_slope + 1e-9, "{st:?}");
        }

        // No pile-up: the steps are apart.
        assert!(plan.steps.windows(2).all(|w| w[1].s.0 >= w[0].s.1 - 1e-9));
    }

    #[test]
    fn a_15_m_wall_takes_a_whole_bastion_for_a_10_m_climb_on_a_straight() {
        let p = profile("roman_fort_straight").unwrap();
        let ground = |x: f64, _: f64| if x < 150.0 { 0.0 } else { 10.0 };
        let plan = solve(&wall(&[[0.0, 0.0], [300.0, 0.0]]), p, &ground);
        let whole = plan.steps.iter().filter(|st| st.whole).collect::<Vec<_>>();
        assert_eq!(whole.len(), 1, "{:?}", plan.steps);
        assert!(((whole[0].to - whole[0].from) - 10.0).abs() < 1e-9);
        assert!(((whole[0].s.1 - whole[0].s.0) - 20.0).abs() < 1e-9);
        assert!(plan.steps.iter().all(|st| st.whole));

        // On a curve, stairs instead.
        let curved = solve(&wall(&[[0.0, 0.0], [150.0, 60.0], [300.0, 0.0]]), p, &|x, _| if x < 150.0 { 0.0 } else { 10.0 });
        assert!(curved.steps.iter().all(|st| !st.whole) || curved.steps.iter().filter(|st| st.whole).all(|st| turn_between(&curved.samples, st.s.0, st.s.1) <= STRAIGHT));
    }

    #[test]
    fn steps_stay_away_from_corners() {
        // A right angle at 60, the ground rising past it: the stairs go on either side, not over the corner.
        let mut input = wall(&[[0.0, 0.0], [60.0, 0.0], [60.0, 120.0]]);
        input.sharp[1] = true;
        let plan = solve(&input, eight(), &|_, z| if z > 2.0 { 2.0 } else { 0.0 });
        let margin = eight().depth.1.abs().max(eight().depth.0.abs()) + 2.0;
        for st in &plan.steps {
            assert!(st.s.1 <= 60.0 - margin + 1e-6 || st.s.0 >= 60.0 + margin - 1e-6, "{st:?} over the corner");
        }
    }

    #[test]
    fn neighbouring_steps_keep_flat_walkway_between_them() {
        // A dip: down then up; the walkway between the two steps stays flat for at least half a run.
        let p = profile("roman_fort_straight").unwrap();
        let ground = |x: f64, _: f64| if (100.0..130.0).contains(&x) { -4.0 } else { 0.0 };
        let plan = solve(&wall(&[[0.0, 0.0], [300.0, 0.0]]), p, &ground);
        for w in plan.steps.windows(2).filter(|w| w[0].chain != w[1].chain) {
            assert!(w[1].s.0 - w[0].s.1 >= p.min_run / 2.0 - 1e-6, "{:?}", plan.steps);
        }
    }

    #[test]
    fn a_cliff_takes_chained_stairs_no_higher_than_one_each() {
        let ground = |x: f64, _: f64| if x < 100.0 { 0.0 } else { 6.0 };
        let plan = solve(&wall(&[[0.0, 0.0], [200.0, 0.0]]), eight(), &ground);
        assert_eq!(plan.steps.len(), 3);
        assert!((plan.steps[2].to - 6.0).abs() < 1e-9);
        assert!(plan.steps.iter().all(|st| ((st.to - st.from) - 2.0).abs() < 1e-9));
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
        // A right angle and sharper: corners; a gentler turn (135 internal): a curve unless marked.
        let pts = [[0.0, 0.0], [50.0, 0.0], [100.0, 50.0], [100.0, 100.0], [60.0, 120.0]];
        assert_eq!(corners(&pts, &[false; 5]), vec![true, false, false, false, true]);
        let right = [[0.0, 0.0], [50.0, 0.0], [50.0, 50.0]];
        assert_eq!(corners(&right, &[false; 3]), vec![true, true, true]);
        let acute = [[0.0, 0.0], [50.0, 0.0], [0.0, 18.0]];
        assert_eq!(corners(&acute, &[false; 3]), vec![true, true, true]);
        assert_eq!(corners(&pts, &[false, true, false, false, false]), vec![true, true, false, false, true]);

        // A corner keeps the line through its point with a kink; a smooth point bends it.
        let plan = solve(&wall(&right), eight(), &|_, _| 0.0);
        assert_eq!(plan.corners.len(), 1);
        assert!((plan.corners[0].0 - 50.0).abs() < 1e-6);
        assert!((plan.length - 100.0).abs() < 1e-6);
        let smooth = solve(&wall(&[[0.0, 0.0], [50.0, 0.0], [100.0, 50.0]]), eight(), &|_, _| 0.0);
        assert!(smooth.corners.is_empty());
    }
}
