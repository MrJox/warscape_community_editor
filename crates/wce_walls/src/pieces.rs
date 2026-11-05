//! A wall's layout cut into pieces and each piece made from the base (docs/plans/procedural_walls.md, "Design (as built)"; phase 2).
//!
//! - **Pieces:** between corners, a stretch is cut into pieces as near the base's length as divides it (one base each, scaled along x
//!   by the stretch's length over theirs).
//! - **Warp ([`PieceWarp`]):** the base's frame onto the wall, mitred at corners and attached ends: x along the centre line (arc length),
//!   z across it (the pieces' local +z is
//!   (-dz, dx) of the direction; a flipped wall is laid from its other end, see `WallInput::flip`), y over the layout's level there. Below the foot the mesh is stretched down to
//!   the terrain. The result is in the piece's own frame: its origin on the centre line at its start, at the level there, x along the
//!   wall's direction there, so the meshes keep the base's size and precision.
//!
//! - **Step sections** ([`SlabWarp`]): the start of the culture's step piece's stair, as much of it as the step rises (at its own slope
//!   and size; lengthened along the wall when the step is longer), brought in over the step's length; it replaces the base's walkway and
//!   palisade there (8 m: the base's bank stays, sloped) or is laid on the base's gently sloped walkway (15 m: the bastion's wooden stair).

use crate::building::{BuildingFiles, is_piece_mesh};
use crate::profile::Profile;
use crate::solver::{Plan, STEP, Sample, wall_level};
use crate::warp::{Cuts, Shifted, Warp};
use crate::{logic, mesh, zones};

/// The finest cut spacing (what the curve needs is clamped to it).
pub const SLICE: f32 = 1.0;

/// How far a chord may stand off the curve it replaces (scene units).
pub const CHORD_ERROR: f64 = 0.02;

/// One piece of a wall: where it runs along the centre line and its own frame (scene units).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Piece {
    pub s: (f64, f64),

    /// The frame's origin: on the centre line at `s.0`, at the level there.
    pub origin: [f64; 3],

    /// The frame's x direction (radians; +x of the scene towards +z).
    pub yaw: f64,

    /// The base's length is scaled by this along the wall.
    pub scale: f64,

    /// The building it is made from instead of the wall's base: a whole step piece, a cap, a tower or bastion (`None`: the base, or a door
    /// section).
    pub base: Option<&'static str>,

    /// Placed as the game's piece, not generated (a tower or bastion): `origin` is then the game piece's own origin, along the wall.
    pub rigid: bool,
}

/// The pieces of a layout: between the ends and corners, `n` pieces of equal length, `n` the stretch's length over the base's, rounded
/// (at least 1).
pub fn pieces(plan: &Plan, profile: &Profile) -> Vec<Piece> {
    if plan.samples.len() < 2 {
        return vec![];
    }

    let len = profile.length();
    let piece_at = |s0: f64, s1: f64, base: Option<&'static str>| {
        let at = sample_at(plan, s0);
        let level = wall_level(plan, (s0 + 1e-4).min((s0 + s1) / 2.0));
        Piece { s: (s0, s1), origin: [at.centre[0], level, at.centre[1]], yaw: at.dir[1].atan2(at.dir[0]), scale: (s1 - s0) / len, base, rigid: false }
    };

    let defences = defence_slots(plan, profile).0;
    let mut out = vec![];
    for (a, b) in sections(plan) {
        // The whole steps and the towers and bastions in the section are pieces of their own; the stretches around them are cut as near
        // the base's length as divides them.
        let mut fixed: Vec<(f64, f64, &'static str, bool)> = plan.steps.iter().filter(|st| st.whole && st.s.0 >= a - 1e-6 && st.s.1 <= b + 1e-6)
            .map(|st| (st.s.0, st.s.1, if st.to > st.from { profile.step_up } else { profile.step_down }, false)).collect();
        fixed.extend(defences.iter().filter(|d| d.0 >= a - 1e-6 && d.0 + len <= b + 1e-6).map(|d| (d.0, d.0 + len, d.1, true)));
        fixed.sort_by(|x, y| x.0.total_cmp(&y.0));
        let mut parts: Vec<(f64, f64, Option<&'static str>, bool)> = vec![];
        let mut at = a;
        for (s0, s1, key, rigid) in fixed {
            parts.push((at, s0, None, false));
            parts.push((s0, s1, Some(key), rigid));
            at = s1;
        }

        parts.push((at, b, None, false));
        for (p0, p1, base, rigid) in parts {
            let l = p1 - p0;
            if l < 1e-6 {
                continue;
            }

            if rigid {
                // The game's piece as it is: its own origin (x = 0 of its frame) on the straight, at the wall's level there.
                let at = sample_at(plan, p0);
                let level = wall_level(plan, p0 + len / 2.0);
                let d = at.dir;
                let origin = [at.centre[0] - d[0] * profile.x.0, level, at.centre[1] - d[1] * profile.x.0];
                out.push(Piece { s: (p0, p1), origin, yaw: d[1].atan2(d[0]), scale: 1.0, base, rigid: true });
                continue;
            }

            if base.is_some() {
                out.push(piece_at(p0, p1, base));
                continue;
            }

            let n = (l / len).round().max(1.0) as usize;
            let piece = l / n as f64;
            let mut cuts = (1..n).map(|k| p0 + piece * k as f64).collect::<Vec<_>>();

            // No joint inside a step (or within `JOINT_CLEAR` of it): a stair split between two pieces lost the end caps under it,
            // the walkway's way from one piece to the next (user 2026-11-05). The cut goes to the nearer side, the pieces kept at
            // least half a length.
            for k in 0..cuts.len() {
                let c = cuts[k];
                let Some(st) = plan.steps.iter().find(|st| !st.whole && c > st.s.0 - JOINT_CLEAR && c < st.s.1 + JOINT_CLEAR) else { continue };
                let (lo, hi) = (if k == 0 { p0 } else { cuts[k - 1] }, if k + 1 < cuts.len() { cuts[k + 1] } else { p1 });
                let options = [st.s.0 - JOINT_CLEAR, st.s.1 + JOINT_CLEAR];
                let fits = |x: f64| x - lo >= len / 2.0 && hi - x >= len / 2.0;
                let mut options = options.into_iter().filter(|&x| fits(x)).collect::<Vec<_>>();
                options.sort_by(|a, b| (a - c).abs().total_cmp(&(b - c).abs()));
                if let Some(&x) = options.first() {
                    cuts[k] = x;
                }
            }

            let mut s0 = p0;
            for s1 in cuts.into_iter().chain(std::iter::once(p1)) {
                out.push(piece_at(s0, s1, None));
                s0 = s1;
            }
        }
    }

    out
}

/// The caps placed past a wall's free ends as they are (the game's pieces, not bent nor fitted to the ground): their key and frame, along
/// the wall's end direction, at the level of the wall's end, their joining x on the wall's end.
pub fn cap_instances(plan: &Plan, profile: &Profile) -> Vec<(&'static str, Piece)> {
    if plan.samples.len() < 2 {
        return vec![];
    }

    let all = sections(plan);
    let (Some(&first), Some(&last)) = (all.first(), all.last()) else { return vec![] };
    let mut out = vec![];
    for (k, key, s, join, section) in [(0, profile.cap_start, 0.0, profile.cap_join.0, first), (1, profile.cap_end, plan.length, profile.cap_join.1, last)] {
        if !plan.free_ends[k] || key.is_empty() {
            continue;
        }

        let dirs = section_dirs(plan, section);
        let d = if k == 0 { dirs.0 } else { dirs.1 };
        let at = sample_at(plan, s);
        let level = wall_level(plan, if k == 0 { 1e-4 } else { plan.length - 1e-4 });
        let origin = [at.centre[0] - d[0] * join, level, at.centre[1] - d[1] * join];
        out.push((key, Piece { s: (s, s), origin, yaw: d[1].atan2(d[0]), scale: 1.0, base: Some(key), rigid: true }));
    }

    out
}

/// Where a piece's polygons and triangles are cut along x before the warp (the base's frame): as often as a chord on its tightest curve
/// stays within [`CHORD_ERROR`] (`8 r e` = chord squared; at least [`SLICE`], at most the base's length), and at the ends of the step
/// ramps inside it (the level bends there; between them it is straight).
pub fn cuts_of(plan: &Plan, piece: &Piece, profile: &Profile) -> Cuts {
    // The tightest curve: the most the direction turns per unit of length between samples.
    let inside = plan.samples.iter().filter(|sm| sm.s >= piece.s.0 && sm.s <= piece.s.1).collect::<Vec<_>>();
    let mut turn = 0.0f64;
    for w in inside.windows(2) {
        let (a, b) = (w[0], w[1]);
        let cross = a.dir[0] * b.dir[1] - a.dir[1] * b.dir[0];
        let dot = a.dir[0] * b.dir[0] + a.dir[1] * b.dir[1];
        turn = turn.max(cross.atan2(dot).abs() / (b.s - a.s).max(1e-9));
    }

    let scale = piece.scale.max(1e-9);
    let step = if turn < 1e-9 { profile.length() } else { (8.0 * CHORD_ERROR / turn).sqrt() / scale };
    let step = step.clamp(f64::from(SLICE), profile.length()) as f32;
    let to_x = |s: f64| (profile.x.0 + (s - piece.s.0) / scale) as f32;
    let at = plan.steps.iter().flat_map(|st| [st.s.0, st.s.1]).filter(|&s| s > piece.s.0 && s < piece.s.1).map(to_x).collect();
    Cuts { step, at }
}

/// The default spacing of the access pieces (15 m: the door sections) along a wall (the first at half of it).
pub const DOOR_EVERY: f64 = 200.0;

/// The towers and bastions of a wall: at `defence_every / 2`, then every `defence_every`, a slot of the base's length, on the straightest,
/// level place nearest (within half the spacing): no step under it, straight (at most [`crate::solver::STRAIGHT`] of turn), away from
/// corners and attached ends (the wall's depth + 2) and from the wall's ends, not over another. The culture's towers and bastions in
/// turn. Returns the slots (start, key) and the places skipped, with why. The solver keeps them in the plan ([`defence_slots`]) and makes
/// the line straight under them.
pub(crate) fn find_defence_slots(plan: &Plan, profile: &Profile) -> (Vec<(f64, &'static str)>, Vec<String>) {
    let (mut out, mut skipped) = (vec![], vec![]);
    if plan.defence_every <= 0.0 || profile.defences.is_empty() || plan.samples.len() < 2 {
        return (out, skipped);
    }

    let len = profile.length();
    let margin = profile.depth.0.abs().max(profile.depth.1.abs()) + 2.0;
    let all = sections(plan);
    let usable = |t: f64| {
        let in_section = all.iter().any(|&(a, b)| {
            let lo = if a > 1e-6 || plan.end_planes[0].is_some() { a + margin } else { a + 1.0 };
            let hi = if b < plan.length - 1e-6 || plan.end_planes[1].is_some() { b - margin } else { b - 1.0 };
            t >= lo && t + len <= hi
        });
        let flat = !plan.steps.iter().any(|st| st.s.1 > t - 1.0 && st.s.0 < t + len + 1.0);
        let straight = crate::solver::turn_between(&plan.samples, t, t + len) <= crate::solver::STRAIGHT;
        in_section && flat && straight
    };

    let mut target = plan.defence_every / 2.0;
    while target < plan.length {
        let window = plan.defence_every / 2.0;
        let mut best: Option<f64> = None;
        let mut t = (target - window).max(0.0);
        while t <= target + window - len {
            let free = !out.iter().any(|&(o, _): &(f64, &str)| t < o + len && t + len > o);
            if free && usable(t) && best.is_none_or(|b| (t + len / 2.0 - target).abs() < (b + len / 2.0 - target).abs()) {
                best = Some(t);
            }
            t += STEP;
        }

        match best {
            Some(t) => out.push((t, profile.defences[out.len() % profile.defences.len()])),
            None => skipped.push(format!("no tower or bastion near {target:.0}: no straight, level stretch away from corners and steps there")),
        }

        target += plan.defence_every;
    }

    (out, skipped)
}

/// The towers and bastions of a wall's plan ([`find_defence_slots`], chosen by the solver), and the places skipped.
pub fn defence_slots(plan: &Plan, _profile: &Profile) -> (Vec<(f64, &'static str)>, Vec<String>) {
    (plan.defences.clone(), plan.defences_skipped.clone())
}

/// The square inside the wall where a door's units come down must be free: this wide (units), starting at the door.
pub const LANDING: f64 = 20.0;

/// The pieces that become door sections: for a base with one, the piece nearest `access_every / 2`, then every `access_every`, if it is
/// a plain piece (no step section in it, not at a corner or an end) and `landing_ok` says the square in front of its doors is free
/// (corners of the square, scene x z: units walk down there). Returns the chosen pieces and the skipped places, with why.
pub fn door_pieces(plan: &Plan, profile: &Profile, ps: &[Piece], ground: &dyn Fn(f64, f64) -> f64, landing_ok: &dyn Fn(&[[f64; 2]; 4]) -> bool) -> (Vec<usize>, Vec<String>) {
    let (mut chosen, mut skipped) = (vec![], vec![]);
    if profile.door.is_empty() || ps.is_empty() {
        return (chosen, skipped);
    }

    if plan.access_every <= 0.0 {
        return (chosen, skipped);
    }

    let ends = sections(plan).into_iter().flat_map(|(a, b)| [a, b]).collect::<Vec<_>>();
    let mut at = plan.access_every / 2.0;
    while at < plan.length {
        let k = ps.iter().enumerate().min_by(|a, b| (((a.1.s.0 + a.1.s.1) / 2.0 - at).abs()).total_cmp(&(((b.1.s.0 + b.1.s.1) / 2.0 - at).abs()))).map(|(k, _)| k).expect("pieces");
        let p = ps[k];
        let stepped = plan.steps.iter().any(|st| st.s.1 > p.s.0 && st.s.0 < p.s.1) || p.base.is_some();
        let at_end = ends.iter().any(|&e| (e - p.s.0).abs() < 1e-6 || (e - p.s.1).abs() < 1e-6);
        let warp = PieceWarp::new(plan, profile, p, ground);
        let (dx, dz) = profile.door_at;
        let square = [[dx - LANDING / 2.0, dz], [dx + LANDING / 2.0, dz], [dx + LANDING / 2.0, dz + LANDING], [dx - LANDING / 2.0, dz + LANDING]]
            .map(|[x, z]| { let q = warp.scene([x, 0.0, z]); [q[0], q[2]] });
        if stepped || at_end {
            skipped.push(format!("no door at {at:.0}: the piece there {}", if stepped { "has a step section" } else { "is at a corner or an end" }));
        } else if !landing_ok(&square) {
            skipped.push(format!("no door at {at:.0}: the ground inside the wall there is not free (no-go, or another building)"));
        } else if !chosen.contains(&k) {
            chosen.push(k);
        }

        at += plan.access_every;
    }

    (chosen, skipped)
}

/// The layout at a distance, interpolated between its samples.
pub fn sample_at(plan: &Plan, s: f64) -> Sample {
    let n = plan.samples.len();
    let f = (s / STEP).clamp(0.0, (n - 1) as f64);
    let i = (f.floor() as usize).min(n - 2);
    let (a, b) = (plan.samples[i], plan.samples[i + 1]);
    let t = ((s - a.s) / (b.s - a.s).max(1e-12)).clamp(0.0, 1.0);
    let l = |x: f64, y: f64| x + (y - x) * t;
    let dir = [l(a.dir[0], b.dir[0]), l(a.dir[1], b.dir[1])];
    let dl = dir[0].hypot(dir[1]).max(1e-12);
    Sample {
        s,
        centre: [l(a.centre[0], b.centre[0]), l(a.centre[1], b.centre[1])],
        dir: [dir[0] / dl, dir[1] / dl],
        foot: [l(a.foot[0], b.foot[0]), l(a.foot[1], b.foot[1])],
        ground: l(a.ground, b.ground),
        level: l(a.level, b.level),
    }
}

/// How far a joint between two pieces stays from a step (`pieces`).
const JOINT_CLEAR: f64 = 0.5;

/// Over how much of the wall before a corner's face its points are moved onto the bisector (`PieceWarp::s_at`).
const FACE_BLEND: f64 = 3.0;

/// The most a mitre shears per unit across the wall: a turn of about 168 degrees (an internal angle of 12); sharper ones are clamped and
/// noted. The outer side is only stretched and the inner side cut, so a large shear doesn't fold the piece.
pub const MAX_SHEAR: f64 = 10.0;

/// The stretches between the ends and corners (distances along the wall).
pub fn sections(plan: &Plan) -> Vec<(f64, f64)> {
    let mut cuts = vec![0.0];
    cuts.extend(plan.corners.iter().map(|&(s, _)| s));
    cuts.push(plan.length);
    cuts.windows(2).filter(|w| w[1] - w[0] > 1e-6).map(|w| (w[0], w[1])).collect()
}

/// A section's direction at its start and end (from the first and last unit of it, so a corner's averaging doesn't bend it).
fn section_dirs(plan: &Plan, (a, b): (f64, f64)) -> ([f64; 2], [f64; 2]) {
    let d = |p: f64, q: f64| {
        let (u, v) = (sample_at(plan, p).centre, sample_at(plan, q).centre);
        let (dx, dz) = (v[0] - u[0], v[1] - u[1]);
        let l = dx.hypot(dz).max(1e-12);
        [dx / l, dz / l]
    };

    // At an attached end, the building's axis (the solver's line leaves along it: the joint is straight). At a corner, from a unit in:
    // the sample on the corner is rounded (Amon Calen's prow: 2 degrees off, the mitre's cut 0.4 m past the bisector at the inner edge).
    let n = plan.samples.len();
    let ((a0, a1), (b0, b1)) = end_spans(plan, (a, b));
    let start = if a < 1e-9 && plan.end_planes[0].is_some() { plan.samples[0].dir } else { d(a0, a1) };
    let end = if b > plan.length - 1e-9 && plan.end_planes[1].is_some() { plan.samples[n - 1].dir } else { d(b0, b1) };
    (start, end)
}

/// The stretches a section's directions at its ends are measured over: its first and last unit, a unit in from an end at a corner (or a
/// quarter of a short section).
fn end_spans(plan: &Plan, (a, b): (f64, f64)) -> ((f64, f64), (f64, f64)) {
    let span = ((b - a) / 4.0).min(1.0);
    let corner = |s: f64| plan.corners.iter().any(|&(c, _)| (c - s).abs() < 1e-6);
    let start = if corner(a) { (a + span, a + 2.0 * span) } else { (a, a + span) };
    let end = if corner(b) { (b - 2.0 * span, b - span) } else { (b - span, b) };
    (start, end)
}

/// How much a section's end is sheared along the wall per unit across it (`s' = s + z * shear`) so the end face lies on the plane of
/// normal `m` through the end: `-(n . m) / (d . m)`, `d` the section's direction there, `n` its +z axis.
fn shear(d: [f64; 2], m: [f64; 2]) -> f64 {
    let n = [-d[1], d[0]];
    let dm = d[0] * m[0] + d[1] * m[1];
    if dm.abs() < 1e-9 {
        return 0.0;
    }

    (-(n[0] * m[0] + n[1] * m[1]) / dm).clamp(-MAX_SHEAR, MAX_SHEAR)
}

/// The base's frame onto one piece of the wall, in the piece's frame. Inside its section the piece follows the centre line; its end at a
/// corner is sheared onto the corner's bisector (both pieces of the corner meet on it: a mitre), its end at an attached end onto the
/// face it meets; past its section's ends (where the shear takes it) it goes on straight.
pub struct PieceWarp<'a> {
    pub plan: &'a Plan,
    pub profile: &'a Profile,
    pub piece: Piece,
    pub ground: &'a dyn Fn(f64, f64) -> f64,
    section: (f64, f64),
    dirs: ([f64; 2], [f64; 2]),
    shears: (f64, f64),

    /// The piece is at the section's start, end (and stretched to the face there on its outer side).
    stretched: (bool, bool),
}

impl<'a> PieceWarp<'a> {
    pub fn new(plan: &'a Plan, profile: &'a Profile, piece: Piece, ground: &'a dyn Fn(f64, f64) -> f64) -> Self {
        let all = sections(plan);
        let mid = (piece.s.0 + piece.s.1) / 2.0;
        let k = all.iter().position(|&(a, b)| mid >= a && mid <= b).unwrap_or(0);
        let section = all.get(k).copied().unwrap_or(piece.s);
        let dirs = section_dirs(plan, section);
        let unit = |v: [f64; 2]| {
            let l = v[0].hypot(v[1]).max(1e-12);
            [v[0] / l, v[1] / l]
        };

        // The face each end of the section meets: a corner's bisector (normal: the sum of the two directions), or an attached end's face.
        let start_face = if k > 0 {
            let prev = section_dirs(plan, all[k - 1]).1;
            Some(unit([prev[0] + dirs.0[0], prev[1] + dirs.0[1]]))
        } else {
            plan.end_planes[0]
        };

        let end_face = if k + 1 < all.len() {
            let next = section_dirs(plan, all[k + 1]).0;
            Some(unit([dirs.1[0] + next[0], dirs.1[1] + next[1]]))
        } else {
            plan.end_planes[1]
        };

        // The faces of the section's ends, as shears (`s' = s + z * shear` on the face): every piece of the section is cut by them where
        // they cross it; the pieces at the ends are also stretched to them on their outer side.
        let shears = (start_face.map_or(0.0, |m| shear(dirs.0, m)), end_face.map_or(0.0, |m| shear(dirs.1, m)));
        let stretched = ((piece.s.0 - section.0).abs() < 1e-6, (piece.s.1 - section.1).abs() < 1e-6);
        Self { plan, profile, piece, ground, section, dirs, shears, stretched }
    }

    /// The faces' shears at the section's start and end (`s' = s + z * shear` on the face).
    pub fn shears(&self) -> (f64, f64) {
        self.shears
    }

    /// The distance along the wall of a point of the base's frame: nothing is stretched. At a corner the inner side is cut by the face
    /// ([`Warp::clip_planes`]); the outer side, past the piece's end, is filled by copies of the base going on straight ([`overhangs`]),
    /// cut by the same face.
    fn s_of(&self, x: f64, _z: f64) -> f64 {
        self.piece.s.0 + (x - self.profile.x.0) * self.piece.scale
    }

    /// Where along the wall a point of the base's frame goes: `s_of`, and near a corner's face, moved so that the face (the cut planes'
    /// straight shear) lands exactly on the corner's bisector, the move fading out over `FACE_BLEND` before it. On a curved arm the
    /// shear alone left the face off the bisector, more the further in (user 2026-11-05: a slit of light along Amon Calen's prow).
    fn s_at(&self, x: f64, z: f64) -> f64 {
        let mut s = self.s_of(x, z);
        for k in 0..2 {
            let Some((c, m)) = self.corner_face(k) else { continue };
            let (lin, shear) = if k == 1 { (self.section.1, self.shears.1) } else { (self.section.0, self.shears.0) };
            let lin = lin + shear * z;
            let gap = if k == 1 { lin - s } else { s - lin };
            if gap >= FACE_BLEND {
                continue;
            }

            let w = if gap <= 0.0 { 1.0 } else { 1.0 - gap / FACE_BLEND };
            s += (self.on_face(c, m, lin, z) - lin) * w;
        }

        s
    }

    /// A corner face at the section's start (0) or end (1): the corner and the bisector's normal, the same from both pieces of the
    /// corner (the two arms' lines from a unit in, met halfway).
    fn corner_face(&self, k: usize) -> Option<([f64; 2], [f64; 2])> {
        let all = sections(self.plan);
        let i = all.iter().position(|&s| s == self.section)?;
        let j = if k == 1 { i + 1 } else { i.checked_sub(1)? };
        let (before, after) = if k == 1 { (all[i], *all.get(j)?) } else { (all[j], all[i]) };
        let corner = before.1;
        let (da, db) = (section_dirs(self.plan, before).1, section_dirs(self.plan, after).0);
        let (_, b1) = end_spans(self.plan, before).1;
        let (a0, _) = end_spans(self.plan, after).0;
        let (pa, pb) = (sample_at(self.plan, b1).centre, sample_at(self.plan, a0).centre);
        let ca = [pa[0] + da[0] * (corner - b1), pa[1] + da[1] * (corner - b1)];
        let cb = [pb[0] - db[0] * (a0 - corner), pb[1] - db[1] * (a0 - corner)];
        let m = [da[0] + db[0], da[1] + db[1]];
        let l = m[0].hypot(m[1]);
        (l > 1e-9).then(|| ([(ca[0] + cb[0]) / 2.0, (ca[1] + cb[1]) / 2.0], [m[0] / l, m[1] / l]))
    }

    /// Where along the wall the point `z` across it lies on the plane through `c` of normal `m`, from `s0` (a few secant steps).
    fn on_face(&self, c: [f64; 2], m: [f64; 2], s0: f64, z: f64) -> f64 {
        let f = |s: f64| {
            let (p, dir, _) = self.along(s);
            let n = [-dir[1], dir[0]];
            (p[0] + n[0] * z - c[0]) * m[0] + (p[1] + n[1] * z - c[1]) * m[1]
        };

        let (mut a, mut b) = (s0, s0 + 0.25);
        let (mut fa, mut fb) = (f(a), f(b));
        for _ in 0..12 {
            if (fb - fa).abs() < 1e-12 {
                break;
            }

            let next = b - fb * (b - a) / (fb - fa);
            (a, fa) = (b, fb);
            b = next;
            fb = f(b);
            if fb.abs() < 1e-7 {
                break;
            }
        }

        if (b - s0).abs() < 4.0 { b } else { s0 }
    }

    /// The shifts (along x of the base's frame) of the copies of the base that fill the outer side of this piece's mitred ends: past its
    /// end for a face there, before its start for one there, as many lengths as the face reaches across the wall's depth.
    pub fn overhangs(&self) -> Vec<f32> {
        let (len, depth) = (self.profile.length(), self.profile.depth.0.abs().max(self.profile.depth.1.abs()));
        let count = |sh: f64| ((sh.abs() * depth / (len * self.piece.scale.max(1e-9))).ceil() as usize).max(1);
        let mut out = vec![];
        if self.stretched.1 && self.has_face(1) {
            out.extend((1..=count(self.shears.1)).map(|k| (k as f64 * len) as f32));
        }

        if self.stretched.0 && self.has_face(0) {
            out.extend((1..=count(self.shears.0)).map(|k| -(k as f64 * len) as f32));
        }

        out
    }

    /// If the section has a face at its start (0) or end (1): a corner, or an attached end.
    fn has_face(&self, k: usize) -> bool {
        let all = sections(self.plan);
        let i = all.iter().position(|&s| s == self.section).unwrap_or(0);
        if k == 0 { i > 0 || self.plan.end_planes[0].is_some() } else { i + 1 < all.len() || self.plan.end_planes[1].is_some() }
    }

    /// The centre line, its direction and the level at a distance: within the section, the layout (its direction held to the section's
    /// own near its ends); past them, straight on.
    pub(crate) fn along(&self, s: f64) -> ([f64; 2], [f64; 2], f64) {
        let (a, b) = self.section;

        // The level exactly, from inside this piece (its neighbour's level, a whole step's, never leaks in at its ends).
        let (p0, p1) = self.piece.s;
        let level = |t: f64| wall_level(self.plan, t.clamp(p0 + 1e-4, (p1 - 1e-4).max(p0 + 1e-4)));
        // Near and past an end the line goes straight on along the end's direction, from where that is measured (at a corner a unit in:
        // the corner's own sample is rounded).
        let ((a0, _), (_, b1)) = end_spans(self.plan, (a, b));
        if s < a0 {
            let at = sample_at(self.plan, a0);
            let d = self.dirs.0;
            return ([at.centre[0] + d[0] * (s - a0), at.centre[1] + d[1] * (s - a0)], d, level(s.max(a)));
        }

        if s > b1 {
            let at = sample_at(self.plan, b1);
            let d = self.dirs.1;
            return ([at.centre[0] + d[0] * (s - b1), at.centre[1] + d[1] * (s - b1)], d, level(s.min(b)));
        }

        let at = sample_at(self.plan, s);
        let dir = if s - a < 1.0 { self.dirs.0 } else if b - s < 1.0 { self.dirs.1 } else { at.dir };
        (at.centre, dir, level(s))
    }

    /// A point of the base's frame in the scene.
    pub fn scene(&self, [x, y, z]: [f64; 3]) -> [f64; 3] {
        let (c, dir, level) = self.along(self.s_at(x, z));
        let z_axis = [-dir[1], dir[0]];
        let (wx, wz) = (c[0] + z_axis[0] * z, c[1] + z_axis[1] * z);
        let (bottom, split) = self.profile.foot;
        let wy = if y >= split || split <= bottom {
            level + y
        } else {
            let target = ((self.ground)(wx, wz) - self.profile.bury).min(level + bottom);
            let t = (y - bottom) / (split - bottom);
            target + (level + split - target) * t
        };

        [wx, wy, wz]
    }

    /// A scene point in the piece's frame.
    pub fn local(&self, [x, y, z]: [f64; 3]) -> [f64; 3] {
        let (dx, dz) = (x - self.piece.origin[0], z - self.piece.origin[2]);
        let (s, c) = self.piece.yaw.sin_cos();
        [dx * c + dz * s, y - self.piece.origin[1], -dx * s + dz * c]
    }
}

impl Warp for PieceWarp<'_> {
    fn point(&self, p: [f32; 3]) -> [f32; 3] {
        let l = self.local(self.scene(p.map(f64::from)));
        l.map(|v| v as f32)
    }

    fn yaw(&self, [x, _, z]: [f32; 3]) -> f32 {
        let (_, dir, _) = self.along(self.s_at(f64::from(x), f64::from(z)));
        (dir[1].atan2(dir[0]) - self.piece.yaw) as f32
    }

    fn ground_point(&self, p: [f32; 3]) -> [f32; 3] {
        let mut q = self.scene(p.map(f64::from));
        q[1] = (self.ground)(q[0], q[2]);
        self.local(q).map(|v| v as f32)
    }

    /// The section's end faces (a corner's bisector, an attached end's face) in the base's frame: keep `s(x) - sh z` within the section's
    /// ends.
    fn clip_planes(&self) -> Vec<[f32; 3]> {
        let (scale, x0) = (self.piece.scale, self.profile.x.0);
        let mut out = vec![];
        let has_face = |k: usize| self.has_face(k);
        if has_face(0) {
            let d0 = self.section.0 - self.piece.s.0;
            out.push([-scale as f32, self.shears.0 as f32, (-(d0 + scale * x0)) as f32]);
        }

        if has_face(1) {
            let d1 = self.section.1 - self.piece.s.0;
            out.push([scale as f32, -self.shears.1 as f32, (d1 + scale * x0) as f32]);
        }

        out
    }
}

/// A step section of a step piece onto its place in a piece: its x range onto the step's distances, z across the wall as the piece, y
/// shifted so its walkway starts at the level before the step and rises (or falls) by the step's rise instead of its own; below the
/// foot stretched down to lower ground like the piece. In the piece's frame.
pub struct SlabWarp<'a> {
    pub inner: &'a PieceWarp<'a>,

    /// The section's x range in the step piece's frame, and the walkway's height where it starts there.
    pub x: (f64, f64),
    pub walk: f64,

    /// Its own rise (signed: + going up along +x).
    pub own_rise: f64,

    /// The step it makes: its distances along the wall and its levels.
    pub s: (f64, f64),
    pub from: f64,
    pub to: f64,

    /// Across the wall: the stair's z range in the step piece onto the piece's (the whole walkway: one stair from side to side, not a
    /// stair beside a ramp; user 2026-11-04). None: z as it is.
    pub across: Option<((f64, f64), (f64, f64))>,
}

impl SlabWarp<'_> {
    /// z of the step piece in the piece, and its factor.
    fn across(&self) -> (f64, f64) {
        self.across.map_or((0.0, 1.0), |((s0, s1), (t0, t1))| {
            let f = (t1 - t0) / (s1 - s0).max(1e-9);
            (t0 - s0 * f, f)
        })
    }

    fn scene(&self, [x, y, z]: [f64; 3]) -> [f64; 3] {
        let (z_at, z_by) = self.across();
        let z = z_at + z * z_by;
        let len = (self.x.1 - self.x.0).max(1e-9);
        let t = ((x - self.x.0) / len).clamp(0.0, 1.0);
        let s = self.s.0 + (x - self.x.0) / len * (self.s.1 - self.s.0);
        let (c, dir, _) = self.inner.along(s);
        let z_axis = [-dir[1], dir[0]];
        let (wx, wz) = (c[0] + z_axis[0] * z, c[1] + z_axis[1] * z);
        let p = self.inner.profile;
        let off = self.from - self.walk + p.walkway + ((self.to - self.from) - self.own_rise) * t;
        let (bottom, split) = p.foot;
        let wy = if y >= split || split <= bottom {
            y + off
        } else {
            let target = ((self.inner.ground)(wx, wz) - p.bury).min(off + bottom);
            target + (off + split - target) * (y - bottom) / (split - bottom)
        };

        [wx, wy, wz]
    }
}

impl Warp for SlabWarp<'_> {
    fn point(&self, p: [f32; 3]) -> [f32; 3] {
        self.inner.local(self.scene(p.map(f64::from))).map(|v| v as f32)
    }

    /// The piece's faces, in the step piece's frame (its x range maps onto the step's distances).
    fn clip_planes(&self) -> Vec<[f32; 3]> {
        let p = self.inner;
        let len = (self.x.1 - self.x.0).max(1e-9);
        let k = (self.s.1 - self.s.0) / len;
        let mut out = vec![];
        let all = sections(p.plan);
        let i = all.iter().position(|&s| s == p.section).unwrap_or(0);
        // The piece's z is z_at + z z_by of the step piece's.
        let (z_at, z_by) = self.across();
        if i > 0 || p.plan.end_planes[0].is_some() {
            // s(x) = s.0 + (x - x.0) k >= section.0 + sh z
            out.push([-k as f32, (p.shears.0 * z_by) as f32, (-(p.section.0 - self.s.0 + k * self.x.0) - p.shears.0 * z_at) as f32]);
        }

        if i + 1 < all.len() || p.plan.end_planes[1].is_some() {
            out.push([k as f32, (-p.shears.1 * z_by) as f32, (p.section.1 - self.s.0 + k * self.x.0 + p.shears.1 * z_at) as f32]);
        }

        out
    }

    fn yaw(&self, [x, ..]: [f32; 3]) -> f32 {
        let len = (self.x.1 - self.x.0).max(1e-9);
        let s = self.s.0 + (f64::from(x) - self.x.0) / len * (self.s.1 - self.s.0);
        let (_, dir, _) = self.inner.along(s);
        (dir[1].atan2(dir[0]) - self.inner.piece.yaw) as f32
    }
}

/// The x ranges (the base's frame) of the step sections that fall in a piece.
pub fn step_ranges(plan: &Plan, profile: &Profile, piece: &Piece) -> Vec<(f32, f32)> {
    let scale = piece.scale.max(1e-9);
    let to_x = |s: f64| (profile.x.0 + (s - piece.s.0) / scale) as f32;
    plan.steps.iter().filter(|st| !st.whole && st.s.1 > piece.s.0 && st.s.0 < piece.s.1).map(|st| (to_x(st.s.0.max(piece.s.0)), to_x(st.s.1.min(piece.s.1)))).collect()
}

/// The step pieces a wall's steps come from (going up and going down); without one, its steps stay a slope of the base.
#[derive(Clone, Copy, Default)]
pub struct StepFiles<'a> {
    pub up: Option<&'a BuildingFiles>,
    pub down: Option<&'a BuildingFiles>,
}

/// One step section inside a piece: where it lies (the base's frame, to cut the base out), what of the step piece is brought (its
/// frame), and how.
struct InPiece<'a> {
    drop: (f32, f32),
    part: (f32, f32),
    files: &'a BuildingFiles,
    warp: SlabWarp<'a>,

    /// The stair's slope over `part` (y at its two ends) and how near it what is brought lies.
    band: Option<(f32, f32, f32)>,

    /// The walkway's height at the part's ends (the step piece's frame).
    ends: (f32, f32),
}

/// The step sections that fall in a piece (each clipped to the piece; a section across two pieces is brought in two halves).
fn steps_in<'a>(warp: &'a PieceWarp<'a>, sources: StepFiles<'a>, across: Option<((f64, f64), (f64, f64))>) -> Vec<InPiece<'a>> {
    let (plan, profile, piece) = (warp.plan, warp.profile, warp.piece);
    let scale = piece.scale.max(1e-9);
    let to_x = |s: f64| (profile.x.0 + (s - piece.s.0) / scale) as f32;
    let mut out = vec![];
    for st in plan.steps.iter().filter(|st| !st.whole) {
        let (a, b) = (st.s.0.max(piece.s.0), st.s.1.min(piece.s.1));
        if b - a < 1e-6 {
            continue;
        }

        let up = st.to >= st.from;
        let Some(files) = (if up { sources.up } else { sources.down }) else { continue };
        let (full, walk) = if up { (profile.slab_up, profile.slab_walk.0) } else { (profile.slab_down, profile.slab_walk.1) };

        // The start of the stair, as much of it as the step rises: its own slope and size (lengthened along the wall when the step is
        // longer, never squashed in height).
        let frac = ((st.to - st.from).abs() / profile.step_rise).min(1.0);
        let x = (full.0, full.0 + (full.1 - full.0) * frac);
        let own_rise = st.to - st.from;
        let len = st.s.1 - st.s.0;
        let to_part = |s: f64| (x.0 + (s - st.s.0) / len.max(1e-9) * (x.1 - x.0)) as f32;
        let walk_at = |px: f32| (walk + own_rise * ((f64::from(px) - x.0) / (x.1 - x.0).max(1e-9))) as f32;
        let part = (to_part(a), to_part(b));
        out.push(InPiece {
            drop: (to_x(a), to_x(b)),
            part,
            files,
            warp: SlabWarp { inner: warp, x, walk, own_rise, s: st.s, from: st.from, to: st.to, across },
            band: profile.slab_band.map(|h| (walk_at(part.0), walk_at(part.1), h as f32)),
            ends: (walk_at(part.0), walk_at(part.1)),
        });
    }

    out
}

/// A piece made from the base's files under a new key: the standing meshes of every level warped, the collapses left out; with
/// `logic`, the logic and zone files warped too (the preview needs only the meshes). A step section goes into the levels its step piece
/// has (the same level); a level it lacks keeps the base whole there: the step pieces have no rubble, and a stair standing in the
/// rubble of a destroyed piece left a hole on each side of it.
///
/// # Errors
///
/// A file of the base doesn't read or write back.
pub fn make_piece(base: &BuildingFiles, key: &str, warp: &PieceWarp, steps: StepFiles, logic_too: bool, only_level: Option<u32>) -> Result<BuildingFiles, String> {
    let cuts = cuts_of(warp.plan, &warp.piece, warp.profile);
    let replaces = warp.profile.step_replaces;
    let zy = warp.profile.slab_zy.map(|((z0, z1), (y0, y1))| ((z0 as f32, z1 as f32), (y0 as f32, y1 as f32)));

    // A stair across the whole walkway: the step piece's stair (its `slab_zy` z) spread over the walkway's width (the base's own
    // platforms at its height, the stair's inner side when it reaches further); what the base has over that width there (platforms, firing lines; a metre more
    // outwards for the parapet's lines) gives way to it.
    let walk_z = if warp.profile.step_across { base.files.get(&base.logic_name()).and_then(|l| logic::walkway_z(l, warp.profile.walkway as f32)) } else { None };
    let across = walk_z.zip(zy).map(|((w0, w1), ((z0, z1), _))| ((f64::from(z0), f64::from(z1)), (f64::from(w0), f64::from(w1.max(z1)))));
    let drop_zy = match (walk_z, zy) {
        (Some((w0, w1)), Some(((_, z1), ys))) => Some(((w0 - 1.0, w1.max(z1) + 0.01), ys)),
        _ => zy,
    };
    let sections = steps_in(warp, steps, across);

    // The levels each section's step piece has.
    let levels = sections.iter().map(|sec| sec.files.files.keys().filter(|n| is_piece_mesh(n)).map(|n| level_of(n)).max().unwrap_or(0)).collect::<Vec<_>>();
    let shifted = warp.overhangs().into_iter().map(|dx| Shifted { inner: warp, dx }).collect::<Vec<_>>();
    let base_x = (warp.profile.x.0 as f32, warp.profile.x.1 as f32);
    let mut b = base.clone().renamed(key).without_collapses();
    let (logic_name, zones_name) = (b.logic_name(), b.zones_name());
    let mut drop = vec![];
    for (name, bytes) in &mut b.files {
        if is_piece_mesh(name) {
            if only_level.is_some_and(|l| level_of(name) != l) {
                drop.push(name.clone());
                continue;
            }

            // The step sections that have this level go into the first piece's meshes (their own pieces' meshes at the level, or their
            // last for a piece of theirs with fewer levels), and are cut out of every piece's.
            let level = level_of(name);
            let first = name.contains("_piece01_");
            let here = sections.iter().zip(&levels).filter(|&(_, &max)| level <= max).map(|(sec, _)| sec).collect::<Vec<_>>();
            let cut_out = if replaces { here.iter().map(|sec| sec.drop).collect::<Vec<_>>() } else { vec![] };
            let mut parts = vec![];
            for sec in here.iter().filter(|_| first) {
                for (pname, pbytes) in sec.files.files.iter().filter(|(n, _)| is_piece_mesh(n)) {
                    let max = sec.files.files.keys().filter(|n| is_piece_mesh(n) && same_piece(n, pname)).map(|n| level_of(n)).max().unwrap_or(1);
                    if level_of(pname) == level.min(max) {
                        parts.push(mesh::MeshSplice { mesh: pbytes, warp: &sec.warp, x: sec.part, zy, band: sec.band });
                    }
                }
            }

            // The copies of this mesh filling a mitred corner's outer side.
            let own = bytes.clone();
            parts.extend(shifted.iter().map(|w| mesh::MeshSplice { mesh: &own, warp: w, x: base_x, zy: None, band: None }));
            *bytes = mesh::warp_mesh_with(bytes, warp, &cuts, &cut_out, drop_zy, &parts)?;
        } else if *name == logic_name && logic_too {
            let own = bytes.clone();
            // The walkable surfaces are always the step's own where it is (one surface under the units: the base's walkway, sloped over
            // the step, under a 15 m wall's stair too, had them stuck); the meshes are cut only where the family's step replaces them.
            // A stair across the walkway walks on its treads (user 2026-11-04: units did not path over one slope through them, partly
            // under them): its platforms follow them, the base's hull sunk under it.
            let walk = warp.profile.walkway as f32;
            let mut splices = sections.iter().filter_map(|sec| {
                let stretch = ((sec.warp.s.1 - sec.warp.s.0) / (sec.warp.x.1 - sec.warp.x.0).max(1e-9)) as f32;
                let platforms = walk_z.and(zy).map(|((z0, z1), _)| stepped_platforms(&stair_treads(sec, (z0, z1)), sec.part, sec.ends, (z0, z1), stretch));
                let sink = platforms.as_ref().map(|_| (walk - 0.5, walk + 0.5));
                sec.files.files.get(&sec.files.logic_name()).map(|l| logic::Splice { logic: l, warp: &sec.warp, x: sec.part, zy, band: sec.band, replaces: Some(sec.drop), per_piece: false, shapes_too: false, platforms, sink })
            }).collect::<Vec<_>>();
            splices.extend(shifted.iter().map(|w| logic::Splice { logic: &own, warp: w, x: base_x, zy: None, band: None, replaces: None, per_piece: true, shapes_too: true, platforms: None, sink: None }));
            *bytes = logic::warp_logic_framed(bytes, warp, &cuts, drop_zy, &splices, Some(base_x))?.0;
        } else if *name == zones_name && logic_too {
            // A stair across the walkway lies in one zone region, as the stepped bastion's.
            let text = String::from_utf8_lossy(bytes).into_owned();
            let text = match across {
                Some((_, (t0, t1))) => zones::reshape_for_steps(&text, &sections.iter().map(|sec| sec.drop).collect::<Vec<_>>(), (t0 as f32, t1 as f32)),
                None => text,
            };
            *bytes = zones::warp_zones(&text, warp, cuts.step)?.into_bytes();
        } else if !name.ends_with(".meta") {
            drop.push(name.clone());
        }
    }

    for n in drop {
        b.files.remove(&n);
    }

    Ok(b)
}

/// A stair's platforms follow its treads (flat on each, a slope at each riser), or are one slope from end to end as the stepped
/// bastion's own (user 2026-11-05: units did not even step onto the stairs; a test of one platform instead of many small ones).
const STEPPED_STAIRS: bool = false;

/// The steepest a stair's platforms slope (rise over run): about 39 degrees, under the stepped bastion's 40.
const STAIR_SLOPE: f32 = 0.8;

/// A step section's treads (`mesh::treads`) from its step piece's first piece, intact: over the part brought, inside the stair's z, and
/// near the stair's slope (within `TREAD_BAND`; another upward face of the bastion 0.6 under it, over the stair's last stretch, was
/// taken for a tread and the stair went down past the walkway: Amon Calen's wall 6).
fn stair_treads(sec: &InPiece, z: (f32, f32)) -> Vec<(f32, f32, f32)> {
    let meshes = sec.files.files.iter().filter(|(n, _)| is_piece_mesh(n) && level_of(n) == 1 && n.contains("_piece01_")).map(|(_, b)| b.as_slice()).collect::<Vec<_>>();
    let ((x0, x1), (y0, y1)) = (sec.part, sec.ends);
    let line = |x: f32| y0 + (y1 - y0) * ((x - x0) / (x1 - x0).max(1e-6));
    mesh::treads(&meshes, sec.part, z).into_iter().filter(|&(y, a, b)| (y - line((a.max(x0) + b.min(x1)) / 2.0)).abs() <= TREAD_BAND).collect()
}

/// How far from the stair's slope a tread lies at most (its rise is 0.25).
const TREAD_BAND: f32 = 0.4;

/// The platforms of a stair (its frame) that follow its treads: flat on each, and at each riser a slope no steeper than `STAIR_SLOPE`
/// (measured on the wall: `stretch` is the wall's length per unit of the stair's frame) on the lower tread, at most nine tenths of it;
/// where there is no lower tread in the part (the walkway's own level beyond its ends), on the upper one. From the walkway's height at
/// the part's start to its height at its end (`ends`), over `z`. Without treads, one slope from end to end.
fn stepped_platforms(treads: &[(f32, f32, f32)], part: (f32, f32), ends: (f32, f32), z: (f32, f32), stretch: f32) -> Vec<serde_json::Value> {
    if !STEPPED_STAIRS {
        return vec![stair_quad((part.0, ends.0), (part.1, ends.1), z)];
    }

    // The levels in x order: the walkway before (no run inside the part), the treads inside the part, the walkway after; one level for
    // neighbours within 3 cm (the first or last tread is often the walkway's own height).
    let mut levels: Vec<(f32, f32, f32)> = vec![(ends.0, part.0, part.0)];
    for &(y, a, b) in treads.iter().filter(|t| t.2 > part.0 + 1e-3 && t.1 < part.1 - 1e-3) {
        levels.push((y, a.max(part.0), b.min(part.1)));
    }

    levels.push((ends.1, part.1, part.1));
    let mut merged: Vec<(f32, f32, f32)> = vec![];
    for l in levels {
        match merged.last_mut() {
            Some(m) if (l.0 - m.0).abs() < 0.03 => *m = (m.0, m.1.min(l.1), m.2.max(l.2)),
            _ => merged.push(l),
        }
    }

    // The ends at the walkway's own heights (a tread within 3 cm merged with it kept its own: the stair's top 2.3 cm off the walkway,
    // too far to be joined to it; Amon Calen's wall 1).
    if let Some(f) = merged.first_mut() {
        f.0 = ends.0;
    }

    if let Some(l) = merged.last_mut() {
        l.0 = ends.1;
    }

    let run = |l: &(f32, f32, f32)| (l.2 - l.1).max(0.0);
    let mut pts: Vec<(f32, f32)> = vec![(part.0, merged[0].0)];
    for w in merged.windows(2) {
        let (lo, hi) = (w[0], w[1]);
        let e = ((lo.2 + hi.1) / 2.0).clamp(part.0, part.1);
        let dy = hi.0 - lo.0;
        let want = dy.abs() / (STAIR_SLOPE * stretch.max(1e-6));

        // On the lower tread (before the riser going up, after it going down); without room there, on the upper.
        let (lower, upper) = if dy > 0.0 { (lo, hi) } else { (hi, lo) };
        let on_lower = run(&lower) > 1e-3;
        let len = want.min(0.9 * if on_lower { run(&lower) } else { run(&upper) }).max(1e-3);
        let before = (dy > 0.0) == on_lower;
        let (p, q) = if before { ((e - len).max(part.0), e) } else { (e, (e + len).min(part.1)) };
        pts.push((p, lo.0));
        pts.push((q, hi.0));
    }

    pts.push((part.1, merged.last().map_or(ends.1, |l| l.0)));

    // In order along x, nothing doubled.
    let mut clean: Vec<(f32, f32)> = vec![];
    for (x, y) in pts {
        let x = clean.last().map_or(x, |l: &(f32, f32)| x.max(l.0));
        match clean.last() {
            Some(l) if (x - l.0).abs() < 1e-4 && (y - l.1).abs() < 1e-4 => {}
            _ => clean.push((x, y)),
        }
    }

    clean.windows(2).filter(|w| w[1].0 - w[0].0 > 1e-4).map(|w| stair_quad(w[0], w[1], z)).collect()
}

/// One platform of a stair (its frame): from (x, y) to (x, y) along it, over `z`; its corners round the way the game's go (clockwise
/// from above, as the stepped bastion's stair: `logic::wind_as_the_game`).
fn stair_quad((xa, ya): (f32, f32), (xb, yb): (f32, f32), z: (f32, f32)) -> serde_json::Value {
    serde_json::json!({ "flag_1": false, "flag_2": false, "flag_3": false, "normal": { "x": 0.0, "y": 1.0, "z": 0.0 }, "vertices": { "outline": [
        { "x": xa, "y": ya, "z": z.1 }, { "x": xb, "y": yb, "z": z.1 }, { "x": xb, "y": yb, "z": z.0 }, { "x": xa, "y": ya, "z": z.0 } ] } })
}

/// If two piece files are of the same piece (`..._pieceNN_...`).
fn same_piece(a: &str, b: &str) -> bool {
    let piece = |n: &str| n.find("_piece").map(|at| n[at..].chars().take(8).collect::<String>());
    piece(a) == piece(b)
}

/// The level of a piece file (`..._destructNN...`: NN, 1 the intact one).
pub fn level_of(name: &str) -> u32 {
    let Some(at) = name.find("_destruct") else { return 0 };
    name[at + 9..].chars().take_while(char::is_ascii_digit).collect::<String>().parse().unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profile::profile;
    use crate::solver::{WallInput, solve, wall_level};

    fn plan_of(points: &[[f64; 2]], ground: &dyn Fn(f64, f64) -> f64) -> (Plan, &'static Profile) {
        let p = profile("barbarian_fort_straight").unwrap();
        let input = WallInput { points: points.to_vec(), sharp: vec![false; points.len()], tolerance: 0.5, ..Default::default() };
        (solve(&input, p, ground), p)
    }

    #[test]
    fn stretches_are_cut_into_pieces_near_the_base_length() {
        let (plan, p) = plan_of(&[[0.0, 0.0], [95.0, 0.0]], &|_, _| 0.0);
        let ps = pieces(&plan, p);
        assert_eq!(ps.len(), 5);
        assert!(ps.iter().all(|q| (q.scale - 0.95).abs() < 1e-6));
        assert!((ps[4].s.1 - 95.0).abs() < 1e-6);
    }

    #[test]
    fn a_straight_wall_on_flat_ground_maps_the_base_rigidly() {
        let ground = |_: f64, _: f64| 3.0;
        let (plan, p) = plan_of(&[[10.0, 5.0], [30.0, 5.0]], &ground);
        let ps = pieces(&plan, p);
        assert_eq!(ps.len(), 1);
        let w = PieceWarp::new(&plan, p, ps[0], &ground);

        // The base's (x, y, z) is (x, y, z) of the piece's frame, its origin at (10, 3, 5) of the scene.
        for q in [[0.0, 8.0, -2.4], [20.0, 0.0, 11.0], [7.5, 4.0, 2.0]] {
            let l = w.point(q);
            assert!((0..3).all(|i| (l[i] - q[i]).abs() < 1e-3), "{q:?} -> {l:?}");
        }

        let s = w.scene([0.0, 8.0, -2.5]);
        assert!((s[0] - 10.0).abs() < 1e-6 && (s[1] - 11.0).abs() < 1e-6 && (s[2] - 2.5).abs() < 1e-6, "{s:?}");
    }

    /// Every joint with a game's piece is exact, as between the game's own pieces: a wall attached to a building at an angle leaves it
    /// along its axis, and the bastions on a curve meet the pieces on both sides in place and direction (in game, joints off by a
    /// fraction of a unit or a degree let ladders and towers dock while units could not cross onto the wall).
    #[test]
    fn joints_with_the_games_pieces_are_exact() {
        let ground = |_: f64, _: f64| 0.0;
        let p = profile("barbarian_fort_straight").unwrap();
        let axis = [20f64.to_radians().cos(), 20f64.to_radians().sin()];
        let points = vec![[0.0, 0.0], [70.0, -10.0], [150.0, 15.0], [230.0, -5.0], [300.0, 10.0]];
        let input = WallInput {
            sharp: vec![false; points.len()],
            points,
            tolerance: 0.5,
            ends: [Some(0.0), None],
            end_planes: [Some(axis), None],
            defence_every: 90.0,
            ..Default::default()
        };
        let plan = solve(&input, p, &ground);
        let ps = pieces(&plan, p);
        let (x0, x1) = p.x;
        let len = x1 - x0;
        let angle = |a: f64, b: f64| (a - b + std::f64::consts::PI).rem_euclid(std::f64::consts::TAU) - std::f64::consts::PI;
        let end_of = |q: Piece, at: f64| {
            let w = PieceWarp::new(&plan, p, q, &ground);
            let c = w.scene([at, 0.0, 0.0]);
            ([c[0], c[2]], q.yaw + f64::from(w.yaw([at as f32, 0.0, 0.0])))
        };

        // The first piece leaves the building along its axis.
        assert!(angle(ps[0].yaw, axis[1].atan2(axis[0])).abs() < 1e-9, "first piece yaw {}", ps[0].yaw.to_degrees());
        let (_, start_yaw) = end_of(ps[0], x0);
        assert!(angle(start_yaw, ps[0].yaw).abs() < 1e-6, "its content starts at {}", start_yaw.to_degrees());

        let rigid = ps.iter().filter(|q| q.rigid).copied().collect::<Vec<_>>();
        assert!(rigid.len() >= 2, "{} bastions", rigid.len());
        for r in rigid {
            let (dir, start) = ([r.yaw.cos(), r.yaw.sin()], [r.origin[0] + r.yaw.cos() * x0, r.origin[2] + r.yaw.sin() * x0]);
            let end = [start[0] + dir[0] * len, start[1] + dir[1] * len];
            let before = ps.iter().find(|q| (q.s.1 - r.s.0).abs() < 1e-6).copied().expect("a piece before");
            let after = ps.iter().find(|q| (q.s.0 - r.s.1).abs() < 1e-6).copied().expect("a piece after");
            for (q, at, joint) in [(before, x1, start), (after, x0, end)] {
                let (c, yaw) = end_of(q, at);
                let d = (c[0] - joint[0]).hypot(c[1] - joint[1]);
                assert!(d < 1e-3, "bastion at {:.0}: the piece next to it is {d:.4} off", r.s.0);
                assert!(angle(yaw, r.yaw).abs() < 1e-3, "bastion at {:.0}: the piece next to it turns {:.3} degrees", r.s.0, angle(yaw, r.yaw).to_degrees());
            }
        }
    }

    /// At a sharp corner (as Amon Calen's prow, both ways round, several angles, 15 m bases of 20 and 21), what each side keeps of its
    /// walkway (its pieces and the copies filling the outer side, inside their cut planes) lies on its own side of the bisector: the two
    /// sides never lie over one another.
    #[test]
    fn a_corner_keeps_each_side_on_its_own_side_of_the_bisector() {
        let ground = |_: f64, _: f64| 0.0;
        for key in ["gondor_fort_straight", "roman_fort_straight"] {
            let p = profile(key).unwrap();
            let cases: [(Vec<[f64; 2]>, &str); 5] = [
                (vec![[0.0, 0.0], [80.0, 40.0], [160.0, 0.0]], "acute"),
                (vec![[0.0, 0.0], [90.0, 0.0], [160.0, 0.0]], "wide"),
                (vec![[0.0, 0.0], [80.0, -40.0], [160.0, 0.0]], "acute right"),
                (vec![[0.0, 0.0], [60.0, 70.0], [160.0, 0.0]], "sharp"),
                // The arm after the corner curves away from it (Amon Calen's prow).
                (vec![[0.0, 0.0], [80.0, 40.0], [130.0, 30.0], [160.0, -10.0]], "curved arm"),
            ];
            for (points, turn) in cases {
                let mut sharp = vec![false; points.len()];
                sharp[1] = true;
                let input = WallInput { points, sharp, tolerance: 0.5, ..Default::default() };
                let plan = solve(&input, p, &ground);
                assert_eq!(plan.corners.len(), 1, "{key} {turn}");
                let s = plan.corners[0].0;
                let c = sample_at(&plan, s).centre;
                let ps = pieces(&plan, p);
                let before = ps.iter().position(|q| (q.s.1 - s).abs() < 1e-6).expect("a piece ending at the corner");
                let (a, b) = (PieceWarp::new(&plan, p, ps[before], &ground), PieceWarp::new(&plan, p, ps[before + 1], &ground));
                let (da, db) = (a.dirs.1, b.dirs.0);
                let m = [da[0] + db[0], da[1] + db[1]];
                let side = |q: [f64; 3]| (q[0] - c[0]) * m[0] + (q[2] - c[1]) * m[1];
                let (x0, x1) = p.x;

                // Each side's cut face lies on the bisector: the two meet there, with no slit between them.
                let mlen = m[0].hypot(m[1]);
                for (w, end) in [(&a, 1usize), (&b, 0usize)] {
                    let (shear, face_s) = if end == 1 { (w.shears.1, w.section.1) } else { (w.shears.0, w.section.0) };
                    for z in [-6.0, 0.0, 3.0, 6.0, 9.0, 12.0] {
                        let s_face = face_s + shear * z;
                        let x = x0 + (s_face - w.piece.s.0) / w.piece.scale;
                        let q = w.scene([x, 15.0, z]);
                        assert!(side(q).abs() / mlen < 0.02, "{key} {turn}: the {} face at z {z} is {:.3} off the bisector", if end == 1 { "end" } else { "start" }, side(q) / mlen);
                    }
                }

                for (w, sign) in [(&a, -1.0), (&b, 1.0)] {
                    let shifts = std::iter::once(0.0).chain(w.overhangs().into_iter().map(f64::from)).collect::<Vec<_>>();
                    for dx in shifts {
                        let planes = crate::warp::Shifted { inner: w, dx: dx as f32 }.clip_planes();
                        for i in 0..=84 {
                            for z in [1.8, 3.0, 5.0, 7.0, 8.5] {
                                let x = x0 + (x1 - x0) * f64::from(i) / 84.0;
                                if !crate::warp::inside(&planes, [x as f32, 15.0, z as f32]) {
                                    continue;
                                }

                                let q = w.scene([x + dx, 15.0, z]);
                                assert!(side(q) * sign > -0.05, "{key} {turn}: {} keeps x {x:.2} (+{dx}) z {z} at {q:?}, past the bisector by {:.2}",
                                    if sign < 0.0 { "the side before" } else { "the side after" }, -side(q) * sign);
                            }
                        }
                    }
                }
            }
        }
    }

    /// A stair's platforms follow its treads (as Amon Calen's wall 9 step: 0.25 down every 0.6): one surface from the walkway before to
    /// the walkway after, flat on every tread at its height, a slope at each riser no steeper than the bastion's stair; both ways.
    #[test]
    fn a_stairs_platforms_follow_its_treads() {
        let outline_of = |p: &serde_json::Value| p["vertices"]["outline"].as_array().unwrap().iter().map(|q| (q["x"].as_f64().unwrap() as f32, q["y"].as_f64().unwrap() as f32)).collect::<Vec<_>>();
        let down = [(15.002, 0.0, 0.62), (14.752, 0.62, 1.24), (14.502, 1.24, 1.86), (14.252, 1.86, 2.48), (14.002, 2.48, 3.0)];
        let up = down.iter().map(|&(y, a, b)| (30.0 - y, a, b)).collect::<Vec<_>>();
        // Amon Calen's wall 6 step: its last tread runs to the part's end, 8 cm above the walkway after it.
        let short = [(15.002, 0.0, 0.62), (14.752, 0.62, 1.24), (14.502, 1.24, 3.0)];
        // Amon Calen's wall 1 step: its last tread 2.3 cm off the walkway after it, one level with it.
        let close = [(15.002, 0.0, 0.62), (15.252, 0.62, 1.24), (15.502, 1.24, 1.86), (15.752, 1.86, 3.0)];
        for (treads, ends) in [(down.to_vec(), (15.0, 14.06)), (up, (15.0, 15.94)), (short.to_vec(), (15.0, 14.42)), (close.to_vec(), (14.99, 15.729))] {
            let ps = stepped_platforms(&treads, (0.0, 3.0), ends, (5.1, 8.9), 1.0);
            if !STEPPED_STAIRS {
                // One platform, the bastion's way: from the walkway before to the walkway after.
                assert_eq!(ps.len(), 1);
                let o = outline_of(&ps[0]);
                assert_eq!((o[0], o[1]), ((0.0, ends.0), (3.0, ends.1)));
                continue;
            }

            let outline = |p: &serde_json::Value| p["vertices"]["outline"].as_array().unwrap().iter().map(|q| (q["x"].as_f64().unwrap() as f32, q["y"].as_f64().unwrap() as f32)).collect::<Vec<_>>();
            let segs = ps.iter().map(|p| { let o = outline(p); (o[0], o[1]) }).collect::<Vec<_>>();
            assert_eq!(segs[0].0, (0.0, ends.0), "{segs:?}");
            assert_eq!(segs.last().unwrap().1, (3.0, ends.1), "{segs:?}");
            assert!(segs.windows(2).all(|w| w[0].1 == w[1].0), "not one surface: {segs:?}");
            for &(y, a, b) in &treads[1..treads.len() - 1] {
                let flat = segs.iter().filter(|(p, q)| (p.1 - y).abs() < 1e-4 && (q.1 - y).abs() < 1e-4).map(|(p, q)| q.0 - p.0).sum::<f32>();
                assert!(flat > 0.2 && flat <= b - a + 0.6, "tread at {y}: flat for {flat}: {segs:?}");
            }

            assert!(segs.iter().all(|(p, q)| (q.1 - p.1).abs() <= STAIR_SLOPE * (q.0 - p.0) + 1e-4), "too steep: {segs:?}");
            assert!(segs.iter().filter(|(p, q)| (p.1 - q.1).abs() > 1e-4).all(|(p, q)| q.0 - p.0 <= 0.6 + 1e-4), "a long slope: {segs:?}");
        }
    }

    /// The walkway changes height only on its steps: between them it stays at the level the step before left (a step placed off its
    /// runs' boundary had the walkway jump back to the run's level after it, and again at the boundary).
    #[test]
    fn the_walkway_changes_height_only_on_its_steps() {
        let grounds: [&dyn Fn(f64, f64) -> f64; 3] = [&|x, _| x * 0.04, &|x, _| 6.0 * (-(x - 70.0) * (x - 70.0) / 900.0).exp(), &|x, _| (x / 23.0).sin() * 2.5];
        let p = profile("gondorean_fort_straight").unwrap();
        let mut steps = 0;
        for ground in grounds {
            for end in (0..12).map(|k| 120.0 + f64::from(k) * 6.1) {
                let input = WallInput { points: vec![[0.0, 0.0], [end, 0.0]], sharp: vec![false, false], tolerance: 0.5, ..Default::default() };
                let plan = solve(&input, p, ground);
                steps += plan.steps.len();
                let on_step = |s: f64| plan.steps.iter().any(|st| s >= st.s.0 - 1e-6 && s <= st.s.1 + 1e-6);
                let mut s = 0.0;
                while s + 0.25 <= plan.length {
                    let (a, b) = (wall_level(&plan, s), wall_level(&plan, s + 0.25));
                    assert!((a - b).abs() < 1e-9 || on_step(s) || on_step(s + 0.25), "wall of {end}: the walkway jumps from {a:.2} to {b:.2} at {s:.2}, off its steps {:?}",
                        plan.steps.iter().map(|st| (st.s, st.from, st.to)).collect::<Vec<_>>());
                    s += 0.25;
                }
            }
        }

        assert!(steps > 30, "only {steps} steps");

        // Near an attached end the steps keep away from it, off their boundary (Amon Calen's wall 5: the boundary 13 from its tower).
        let mut off = 0;
        for at in (0..16).map(|k| 120.0 + f64::from(k)) {
            let ground = move |x: f64, _: f64| if x < at { 0.0 } else { -0.7 };
            let input = WallInput { points: vec![[0.0, 0.0], [150.0, 0.0]], sharp: vec![false, false], tolerance: 0.5, ends: [None, Some(-0.7)], end_planes: [None, Some([1.0, 0.0])], ..Default::default() };
            let plan = solve(&input, p, &ground);
            off += plan.steps.iter().filter(|st| st.s.1 < at - 1.0 || st.s.0 > at + 1.0).count();
            let on_step = |s: f64| plan.steps.iter().any(|st| s >= st.s.0 - 1e-6 && s <= st.s.1 + 1e-6);
            let mut s = 0.0;
            while s + 0.25 <= plan.length {
                let (a, b) = (wall_level(&plan, s), wall_level(&plan, s + 0.25));
                assert!((a - b).abs() < 1e-9 || on_step(s) || on_step(s + 0.25), "the ground stepping at {at}: the walkway jumps from {a:.2} to {b:.2} at {s:.2}, off its steps {:?}",
                    plan.steps.iter().map(|st| (st.s, st.from, st.to)).collect::<Vec<_>>());
                s += 0.25;
            }
        }

        assert!(off > 0, "no step off its boundary: the case is not tested");
    }

    /// An attached end a little above the wall's run (within the tolerance) is met by the run itself, not by a step squeezed against the
    /// building with no room (Amon Calen's wall 1: 0.13 within its last metre, a near vertical riser).
    #[test]
    fn a_small_rise_to_an_attached_end_needs_no_step() {
        let p = profile("gondorean_fort_straight").unwrap();
        for len in [100.0, 120.5, 137.0] {
            let input = WallInput { points: vec![[0.0, 0.0], [len, 0.0]], sharp: vec![false, false], tolerance: 0.5, ends: [None, Some(0.2)], end_planes: [None, Some([1.0, 0.0])], ..Default::default() };
            // The ground changes in the wall's last metre: a run of its own there, held at the end's level.
            let ground = move |x: f64, _: f64| if x > len - 0.8 { 0.9 } else { 0.0 };
            let plan = solve(&input, p, &ground);
            assert!(plan.runs.len() >= 1 && plan.steps.is_empty(), "wall of {len}: runs {:?} steps {:?}", plan.runs.iter().map(|r| (r.s, r.level)).collect::<Vec<_>>(), plan.steps.iter().map(|st| st.s).collect::<Vec<_>>());
            assert!((wall_level(&plan, len - 0.1) - 0.2).abs() < 1e-9 && (wall_level(&plan, 1.0) - 0.2).abs() < 1e-9);
            assert!(plan.warnings.is_empty(), "{:?}", plan.warnings);
        }
    }

    /// No joint between two pieces falls inside a step (a stair split between two pieces lost the end caps under it): on a slope, walls
    /// of many lengths, every joint at least `JOINT_CLEAR` from every step.
    #[test]
    fn joints_keep_off_the_steps() {
        let ground = |x: f64, _: f64| x * 0.04;
        let p = profile("gondor_fort_straight").unwrap();
        let mut steps = 0;
        for end in (0..30).map(|k| 120.0 + f64::from(k) * 3.7) {
            let input = WallInput { points: vec![[0.0, 0.0], [end, 0.0]], sharp: vec![false, false], tolerance: 0.5, ..Default::default() };
            let plan = solve(&input, p, &ground);
            let ps = pieces(&plan, p);
            steps += plan.steps.iter().filter(|st| !st.whole).count();
            for w in ps.windows(2) {
                let joint = w[0].s.1;
                assert!(plan.steps.iter().filter(|st| !st.whole).all(|st| joint <= st.s.0 - JOINT_CLEAR + 1e-6 || joint >= st.s.1 + JOINT_CLEAR - 1e-6),
                    "wall of {end}: a joint at {joint:.2} in a step {:?}", plan.steps.iter().map(|st| st.s).collect::<Vec<_>>());
            }

            assert!(ps.iter().all(|q| q.scale > 0.5), "a piece too short");
        }

        assert!(steps > 20, "only {steps} steps: the test needs more");
    }

    #[test]
    fn a_right_angle_corner_is_mitred_on_its_bisector() {
        let ground = |_: f64, _: f64| 0.0;
        let p = profile("barbarian_fort_straight").unwrap();
        let input = WallInput { points: vec![[0.0, 0.0], [40.0, 0.0], [40.0, 40.0]], sharp: vec![false, true, false], tolerance: 0.5, ..Default::default() };
        let plan = solve(&input, p, &ground);
        let ps = pieces(&plan, p);
        assert_eq!(ps.len(), 4);
        let (a, b) = (PieceWarp::new(&plan, p, ps[1], &ground), PieceWarp::new(&plan, p, ps[2], &ground));

        // The direction turns from +x to +z: the bisector through (40, 0) is the line x - 40 = -z... a point of the end face of the first
        // piece and the same point of the start face of the second land on it, at the same place.
        // The outer side (z < 0 for this left turn) is filled by a copy past the end, kept up to the bisector: a point there is inside
        // the planes, one past the bisector is not; both pieces have a copy on their side of it.
        assert_eq!(a.overhangs(), vec![20.0]);
        assert_eq!(b.overhangs(), vec![-20.0]);
        assert!(crate::warp::inside(&a.clip_planes(), [21.0, 8.0, -2.0]));
        assert!(!crate::warp::inside(&a.clip_planes(), [23.0, 8.0, -2.0]));

        // Nothing is stretched: the piece's end is where its frame ends, on every z.
        for z in [-2.5, 0.0, 6.0] {
            let pa = a.scene([20.0, 8.0, z]);
            assert!((pa[0] - 40.0).abs() < 1e-6, "z {z}: {pa:?}");
        }

        // An acute corner (internal 30 degrees) does not fold: along the inner side the piece only moves forward.
        let acute = WallInput { points: vec![[0.0, 0.0], [60.0, 0.0], [8.0, 30.0]], sharp: vec![false, true, false], tolerance: 0.5, ..Default::default() };
        let plan = solve(&acute, p, &ground);
        let ps = pieces(&plan, p);
        let last = PieceWarp::new(&plan, p, ps[2], &ground);
        for z in [-12.0, -2.5, 0.0, 6.0, 11.0] {
            let xs = (0..=20).map(|x| last.scene([f64::from(x), 8.0, z])).collect::<Vec<_>>();
            assert!(xs.windows(2).all(|w| w[1][0] >= w[0][0] - 1e-9), "z {z} folds: {xs:?}");
        }

        // The cut planes of both pieces are the bisector: a point of the inner side past it is cut from both.
        let past = [19.0, 8.0, 6.0];
        assert!(!crate::warp::inside(&a.clip_planes(), past), "{:?}", a.clip_planes());
        assert!(crate::warp::inside(&a.clip_planes(), [10.0, 8.0, 6.0]));
    }

    #[test]
    fn the_wall_beside_a_stepped_bastion_stays_level_up_to_it() {
        let p = profile("roman_fort_straight").unwrap();
        let ground = |x: f64, _: f64| if x < 150.0 { 10.0 } else { 0.0 };
        let input = WallInput { points: vec![[0.0, 0.0], [300.0, 0.0]], sharp: vec![false; 2], tolerance: 0.5, ..Default::default() };
        let plan = solve(&input, p, &ground);
        let ps = pieces(&plan, p);
        let k = ps.iter().position(|q| q.base.is_some()).expect("a bastion");
        assert!(k > 0 && k + 1 < ps.len());
        let (before, after) = (PieceWarp::new(&plan, p, ps[k - 1], &ground), PieceWarp::new(&plan, p, ps[k + 1], &ground));

        // The walkway (y 15) of the piece before stays at its run's level to its very end; the one after from its very start.
        let top = |w: &PieceWarp, x: f64| w.scene([x, 15.0, 2.0])[1];
        assert!((top(&before, 9.99) - top(&before, 0.0)).abs() < 1e-6, "{} vs {}", top(&before, 9.99), top(&before, 0.0));
        assert!((top(&after, -9.99) - top(&after, 0.0)).abs() < 1e-6, "{} vs {}", top(&after, -9.99), top(&after, 0.0));
        assert!((top(&before, 0.0) - top(&after, 0.0) - 10.0).abs() < 1e-6);
    }

    #[test]
    fn towers_and_bastions_take_whole_slots_in_turn_on_straight_level_ground() {
        let p = profile("roman_fort_straight").unwrap();
        let ground = |_: f64, _: f64| 0.0;
        let input = WallInput { points: vec![[0.0, 0.0], [400.0, 0.0]], sharp: vec![false; 2], tolerance: 0.5, defence_every: 100.0, ..Default::default() };
        let plan = solve(&input, p, &ground);
        let ps = pieces(&plan, p);
        let rigid = ps.iter().filter(|q| q.rigid).collect::<Vec<_>>();
        assert_eq!(rigid.len(), 4);
        assert_eq!(rigid[0].base, Some("roman_fort_tower_c_straight_with_extension"));
        assert_eq!(rigid[1].base, Some("roman_fort_straight_with_solid_squared_bastion"));
        assert!(rigid.iter().all(|q| ((q.s.1 - q.s.0) - 20.0).abs() < 1e-9 && q.scale == 1.0));

        // The game piece's origin is its frame's x = 0: 10 past the slot's start for a 15 m piece.
        assert!((rigid[0].origin[0] - (rigid[0].s.0 + 10.0)).abs() < 1e-6);

        // The pieces cover the wall without gaps or overlaps.
        assert!(ps.windows(2).all(|w| (w[1].s.0 - w[0].s.1).abs() < 1e-6));
        assert!((ps.last().unwrap().s.1 - plan.length).abs() < 1e-6);

        // Off by default; none on a curve.
        let none = solve(&WallInput { defence_every: 0.0, ..input.clone() }, p, &ground);
        assert!(pieces(&none, p).iter().all(|q| !q.rigid));
        let curved = WallInput { points: vec![[0.0, 0.0], [100.0, 40.0], [200.0, 0.0]], sharp: vec![false; 3], tolerance: 0.5, defence_every: 100.0, ..Default::default() };
        let plan = solve(&curved, p, &ground);
        for q in pieces(&plan, p).iter().filter(|q| q.rigid) {
            assert!(crate::solver::turn_between(&plan.samples, q.s.0, q.s.1) <= crate::solver::STRAIGHT);
        }
    }

    #[test]
    fn the_foot_reaches_down_to_lower_ground() {
        // The ground drops to -5 on the inner side (z > 0); the level is held by the outer face's ground (0).
        let ground = |_: f64, z: f64| if z > 3.0 { -5.0 } else { 0.0 };
        let (plan, p) = plan_of(&[[0.0, 0.0], [20.0, 0.0]], &ground);
        let w = PieceWarp::new(&plan, p, pieces(&plan, p)[0], &ground);
        // The bottom (y = -2) of the inner side goes 2 under the ground there, y = 0 stays at the level.
        assert!((w.scene([10.0, -2.0, 8.0])[1] - -7.0).abs() < 1e-6);
        assert!((w.scene([10.0, 0.0, 8.0])[1] - 0.0).abs() < 1e-6);
        assert!((w.scene([10.0, -2.0, -2.0])[1] - -2.0).abs() < 1e-6);
    }
}
