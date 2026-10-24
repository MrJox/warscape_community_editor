//! A wall's layout cut into pieces and each piece made from the base (docs/plans/procedural_walls.md, "The generator"; phase 2).
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
use crate::solver::{Plan, STEP, Sample};
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
}

/// The pieces of a layout: between the ends and corners, `n` pieces of equal length, `n` the stretch's length over the base's, rounded
/// (at least 1).
pub fn pieces(plan: &Plan, profile: &Profile) -> Vec<Piece> {
    if plan.samples.len() < 2 {
        return vec![];
    }

    let mut out = vec![];
    for (a, b) in sections(plan) {
        let len = b - a;
        let n = (len / profile.length()).round().max(1.0) as usize;
        let piece = len / n as f64;
        for k in 0..n {
            let s0 = a + piece * k as f64;
            let at = sample_at(plan, s0);
            out.push(Piece { s: (s0, s0 + piece), origin: [at.centre[0], at.level, at.centre[1]], yaw: at.dir[1].atan2(at.dir[0]), scale: piece / profile.length() });
        }
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

/// A door section every this many units along a 15 m wall (the first at half of it).
pub const DOOR_EVERY: f64 = 200.0;

/// The square inside the wall where a door's units come down must be free: this wide (units), starting at the door.
pub const LANDING: f64 = 20.0;

/// The pieces that become door sections: for a base with one, the piece nearest `DOOR_EVERY / 2`, then every `DOOR_EVERY`, if it is
/// a plain piece (no step section in it, not at a corner or an end) and `landing_ok` says the square in front of its doors is free
/// (corners of the square, scene x z: units walk down there). Returns the chosen pieces and the skipped places, with why.
pub fn door_pieces(plan: &Plan, profile: &Profile, ps: &[Piece], ground: &dyn Fn(f64, f64) -> f64, landing_ok: &dyn Fn(&[[f64; 2]; 4]) -> bool) -> (Vec<usize>, Vec<String>) {
    let (mut chosen, mut skipped) = (vec![], vec![]);
    if profile.door.is_empty() || ps.is_empty() {
        return (chosen, skipped);
    }

    let ends = sections(plan).into_iter().flat_map(|(a, b)| [a, b]).collect::<Vec<_>>();
    let mut at = DOOR_EVERY / 2.0;
    while at < plan.length {
        let k = ps.iter().enumerate().min_by(|a, b| (((a.1.s.0 + a.1.s.1) / 2.0 - at).abs()).total_cmp(&(((b.1.s.0 + b.1.s.1) / 2.0 - at).abs()))).map(|(k, _)| k).expect("pieces");
        let p = ps[k];
        let stepped = plan.steps.iter().any(|st| st.s.1 > p.s.0 && st.s.0 < p.s.1);
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

        at += DOOR_EVERY;
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

    let span = (b - a).min(1.0);
    (d(a, a + span), d(b - span, b))
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
        if s < a {
            let at = sample_at(self.plan, a);
            let d = self.dirs.0;
            return ([at.centre[0] + d[0] * (s - a), at.centre[1] + d[1] * (s - a)], d, at.level);
        }

        if s > b {
            let at = sample_at(self.plan, b);
            let d = self.dirs.1;
            return ([at.centre[0] + d[0] * (s - b), at.centre[1] + d[1] * (s - b)], d, at.level);
        }

        let at = sample_at(self.plan, s);
        let dir = if s - a < 1.0 { self.dirs.0 } else if b - s < 1.0 { self.dirs.1 } else { at.dir };
        (at.centre, dir, at.level)
    }

    /// A point of the base's frame in the scene.
    pub fn scene(&self, [x, y, z]: [f64; 3]) -> [f64; 3] {
        let (c, dir, level) = self.along(self.s_of(x, z));
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
        let (_, dir, _) = self.along(self.s_of(f64::from(x), f64::from(z)));
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
}

impl SlabWarp<'_> {
    fn scene(&self, [x, y, z]: [f64; 3]) -> [f64; 3] {
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
        if i > 0 || p.plan.end_planes[0].is_some() {
            // s(x) = s.0 + (x - x.0) k >= section.0 + sh z
            out.push([-k as f32, p.shears.0 as f32, (-(p.section.0 - self.s.0 + k * self.x.0)) as f32]);
        }

        if i + 1 < all.len() || p.plan.end_planes[1].is_some() {
            out.push([k as f32, -p.shears.1 as f32, (p.section.1 - self.s.0 + k * self.x.0) as f32]);
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
    plan.steps.iter().filter(|st| st.s.1 > piece.s.0 && st.s.0 < piece.s.1).map(|st| (to_x(st.s.0.max(piece.s.0)), to_x(st.s.1.min(piece.s.1)))).collect()
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
}

/// The step sections that fall in a piece (each clipped to the piece; a section across two pieces is brought in two halves).
fn steps_in<'a>(warp: &'a PieceWarp<'a>, sources: StepFiles<'a>) -> Vec<InPiece<'a>> {
    let (plan, profile, piece) = (warp.plan, warp.profile, warp.piece);
    let scale = piece.scale.max(1e-9);
    let to_x = |s: f64| (profile.x.0 + (s - piece.s.0) / scale) as f32;
    let mut out = vec![];
    for st in &plan.steps {
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
            warp: SlabWarp { inner: warp, x, walk, own_rise, s: st.s, from: st.from, to: st.to },
            band: profile.slab_band.map(|h| (walk_at(part.0), walk_at(part.1), h as f32)),
        });
    }

    out
}

/// A piece made from the base's files under a new key: the standing meshes of every level warped, the collapses left out; with
/// `logic`, the logic and zone files warped too (the preview needs only the meshes).
///
/// # Errors
///
/// A file of the base doesn't read or write back.
pub fn make_piece(base: &BuildingFiles, key: &str, warp: &PieceWarp, steps: StepFiles, logic_too: bool, only_level: Option<u32>) -> Result<BuildingFiles, String> {
    let cuts = cuts_of(warp.plan, &warp.piece, warp.profile);
    let sections = steps_in(warp, steps);
    let cut_out = if warp.profile.step_replaces { sections.iter().map(|s| s.drop).collect::<Vec<_>>() } else { vec![] };
    let zy = warp.profile.slab_zy.map(|((z0, z1), (y0, y1))| ((z0 as f32, z1 as f32), (y0 as f32, y1 as f32)));
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

            // The step sections go into the first piece's meshes (their own pieces' meshes at the same level, or their last).
            let level = level_of(name);
            let first = name.contains("_piece01_");
            let mut parts = vec![];
            for sec in sections.iter().filter(|_| first) {
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
            *bytes = mesh::warp_mesh_with(bytes, warp, &cuts, &cut_out, zy, &parts)?;
        } else if *name == logic_name && logic_too {
            let own = bytes.clone();
            let mut splices = sections.iter().filter_map(|sec| {
                sec.files.files.get(&sec.files.logic_name()).map(|l| logic::Splice { logic: l, warp: &sec.warp, x: sec.part, zy, band: sec.band, per_piece: false })
            }).collect::<Vec<_>>();
            splices.extend(shifted.iter().map(|w| logic::Splice { logic: &own, warp: w, x: base_x, zy: None, band: None, per_piece: true }));
            *bytes = logic::warp_logic_with(bytes, warp, &cuts, &cut_out, zy, &splices)?.0;
        } else if *name == zones_name && logic_too {
            *bytes = zones::warp_zones(&String::from_utf8_lossy(bytes), warp, cuts.step)?.into_bytes();
        } else if !name.ends_with(".meta") {
            drop.push(name.clone());
        }
    }

    for n in drop {
        b.files.remove(&n);
    }

    Ok(b)
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
    use crate::solver::{WallInput, solve};

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
