//! Walls of the kit's own pieces (docs/plans/kit_walls.md), phase 1: a culture's fort pieces read from their logic files and measured,
//! what a wall with *Procedural* off is laid with.
//!
//! A culture is a procedural wall base (`wall_base`, `profile::BASES`): its pieces are the fort keys that start with the base's prefix
//! (`roman_fort_straight` -> `roman_fort_`). Each piece's joints are its end caps' (`logic::ends`: the 15 m pieces, towers, corners,
//! joiners), else the ends of its wall-walk ([`walkway_ends`]: the 8 m pieces and the curves have no caps). Measured from the first
//! joint to the second: the span between them, the turn (degrees, positive from +x towards +z, as the wall goes in by the first and out by
//! the second), the rise (the walk's height at the second over the first) and the shift across the wall (a kink's). Nothing is oriented
//! or laid here (phase 2).
//!
//! In the pieces' own frame and units (the models' units: x, z on the ground, y up).

use std::io::Cursor;

use rpfm_lib::files::Decodeable;
use rpfm_lib::files::cs2_parsed::Cs2Parsed;
use serde_json::Value;

use crate::logic::PieceEnd;

/// What a piece is laid as.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Role {
    /// Joints in line: the wall's plain run.
    Straight,

    /// A corner or a joiner: a sharp turn.
    Turn,

    /// A kink: the joints in parallel but shifted across the wall (the 8 m `kink_left` / `kink_right`: 20 along, 10 across).
    Jog,

    /// A curved piece (`curve`, `arc`, the 600 m ones): a turn spread along it, at any angle.
    Curve,

    /// The walk climbs from one joint to the other (the stepped bastions, `step_up` / `step_down`).
    Step,

    /// A tower or a bastion: laid in place of a piece with its joints, at the wall's defence spacing.
    Defence,

    /// A door section (`_with_doors`, `_with_pipes`): laid in place of a piece with its joints, at the wall's access spacing.
    Access,

    /// One joint: closes a free end (`endcap`, `tower_end`).
    Cap,
}

/// A piece of a culture's set, measured.
#[derive(Clone, Debug, PartialEq)]
pub struct KitPiece {
    pub key: String,
    pub role: Role,

    /// Its joints: two, a cap's one.
    pub joints: Vec<PieceEnd>,

    /// From the first joint to the second (x / z), the turn there (degrees), the rise (y) and the shift across the wall (x / z, positive
    /// from the way in towards +z as `turn`). A cap's: zero.
    pub span: f32,
    pub turn: f32,
    pub rise: f32,
    pub shift: f32,

    /// The walk's height at the first joint.
    pub walk: f32,

    /// Where it stands (level 0, x / z): its `orange_thingies` rings, else its collision outlines.
    pub footprint: Vec<Vec<[f32; 2]>>,
}

/// A culture's pieces, and the keys of its prefix that are not laid, each with why.
#[derive(Clone, Debug, Default)]
pub struct Catalogue {
    pub base: String,
    pub pieces: Vec<KitPiece>,
    pub left_out: Vec<(String, String)>,
}

impl Catalogue {
    /// Its pieces of a role.
    pub fn of(&self, role: Role) -> impl Iterator<Item = &KitPiece> {
        self.pieces.iter().filter(move |p| p.role == role)
    }
}

/// Keys never laid: gates and gateways (what a wall attaches to, placed by hand), stairs, TDD's helper pieces.
const NOT_LAID: [&str; 8] = ["gate", "bridging", "watchtower", "stair", "dummy", "origin", "display", "destroyed"];

/// A turn smaller than this (degrees) is none: a straight's joints are in line within it.
const IN_LINE: f32 = 0.5;

/// A shift larger than this (model units) across the wall, the joints in parallel, is a jog.
const JOG_SHIFT: f32 = 0.5;

/// A rise larger than this (model units) is a step.
const STEP_RISE: f32 = 0.5;

/// The culture prefix of a base wall's key (`roman_fort_straight` -> `roman_fort_`).
#[must_use]
pub fn prefix_of(base: &str) -> String {
    let b = base.to_lowercase();
    b.strip_suffix("straight").unwrap_or(&b).to_owned()
}

/// The keys of a culture: those of `keys` (every building with a logic file) that start with its prefix, sorted; not those of another
/// base whose prefix is longer and starts with it (`eastern_fort_` is not `eastern_fort_small_wall_`, the 8 m culture: `profile::BASES`).
#[must_use]
pub fn culture_keys<'a>(base: &str, keys: impl Iterator<Item = &'a str>) -> Vec<String> {
    let prefix = prefix_of(base);
    let others = crate::profile::BASES.iter().map(|b| prefix_of(b.key)).filter(|o| o.len() > prefix.len() && o.starts_with(&prefix)).collect::<Vec<_>>();
    let mut out = keys.map(str::to_lowercase).filter(|k| k.starts_with(&prefix) && !others.iter().any(|o| k.starts_with(o.as_str()))).collect::<Vec<_>>();
    out.sort();
    out.dedup();
    out
}

/// A culture's catalogue: each of `keys` read (`read`: a key's logic file, `<key>_tech.cs2.parsed`), measured and sorted by role.
pub fn catalogue(base: &str, keys: &[String], read: &dyn Fn(&str) -> Option<Vec<u8>>) -> Catalogue {
    let mut c = Catalogue { base: base.to_owned(), ..Catalogue::default() };
    for key in keys {
        let k = key.to_lowercase();
        let stepped = k.contains("step");
        if !stepped && let Some(w) = NOT_LAID.iter().find(|w| k.contains(*w)) {
            c.left_out.push((key.clone(), format!("not laid ({w})")));
            continue;
        }

        let Some(bytes) = read(key) else {
            c.left_out.push((key.clone(), "no logic file".to_owned()));
            continue;
        };

        match measure(key, &bytes) {
            Ok(p) => c.pieces.push(p),
            Err(why) => c.left_out.push((key.clone(), why)),
        }
    }

    // One walk height: the culture's straights' (a piece at another is of another set: it would not join).
    if let Some(walk) = c.pieces.iter().find(|p| p.role == Role::Straight && p.key.eq_ignore_ascii_case(base)).or_else(|| c.of(Role::Straight).next()).map(|p| p.walk) {
        let (keep, other): (Vec<_>, Vec<_>) = c.pieces.into_iter().partition(|p| p.role == Role::Step || (p.walk - walk).abs() < 0.1);
        c.pieces = keep;
        c.left_out.extend(other.into_iter().map(|p| (p.key, format!("its walk is at {} (the culture's at {walk})", p.walk))));
    }

    c.pieces.sort_by(|a, b| (a.role, &a.key).cmp(&(b.role, &b.key)));
    c
}

/// A piece measured from its logic file.
///
/// # Errors
///
/// Its joints don't read as one (a cap) or two, or a one-jointed piece is not a cap.
pub fn measure(key: &str, bytes: &[u8]) -> Result<KitPiece, String> {
    let joints = joints(bytes);
    let footprint = footprint(bytes).unwrap_or_default();
    let m = classify(key, &joints)?;
    Ok(KitPiece { key: key.to_owned(), role: m.role, walk: joints[0].at[1], joints, span: m.span, turn: m.turn, rise: m.rise, shift: m.shift, footprint })
}

/// A piece's role and measures from its joints and its key (see the module's doc).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Measures {
    pub role: Role,
    pub span: f32,
    pub turn: f32,
    pub rise: f32,
    pub shift: f32,
}

/// A piece's role from its joints and its key, and its span, turn, rise and shift (see the module's doc).
///
/// # Errors
///
/// Its joints are not one (a cap) or two, or a one-jointed piece is not a cap.
pub fn classify(key: &str, joints: &[PieceEnd]) -> Result<Measures, String> {
    let k = key.to_lowercase();
    match joints {
        [_] if k.contains("endcap") || k.contains("tower_end") => Ok(Measures { role: Role::Cap, span: 0.0, turn: 0.0, rise: 0.0, shift: 0.0 }),
        [_] => Err("one joint, and not a cap".to_owned()),
        [a, b] => {
            let span = (b.at[0] - a.at[0]).hypot(b.at[2] - a.at[2]);
            let rise = b.at[1] - a.at[1];
            // In by the first joint (against its way out), out by the second.
            let way = [-a.out[0], -a.out[1]];
            let turn = signed_angle(way, b.out);
            let (dx, dz) = (b.at[0] - a.at[0], b.at[2] - a.at[2]);
            let shift = way[0] * dz - way[1] * dx;
            let role = if rise.abs() > STEP_RISE || k.contains("step") && rise.abs() > 0.05 {
                Role::Step
            } else if k.contains("tower") || k.contains("bastion") {
                Role::Defence
            } else if k.contains("_with_doors") || k.contains("_with_pipes") {
                Role::Access
            } else if turn.abs() <= IN_LINE && shift.abs() > JOG_SHIFT {
                Role::Jog
            } else if turn.abs() <= IN_LINE {
                Role::Straight
            } else if k.contains("curve") || k.contains("arc") || k.contains("600m") {
                Role::Curve
            } else {
                Role::Turn
            };

            Ok(Measures { role, span, turn, rise, shift })
        }

        [] => Err("no joints (no end caps, no walk ends: TDD ships some pieces with no platforms)".to_owned()),
        many => Err(format!("{} joints", many.len())),
    }
}

/// The angle from `a` to `b` (x / z, degrees), positive from +x towards +z.
fn signed_angle(a: [f32; 2], b: [f32; 2]) -> f32 {
    (a[0] * b[1] - a[1] * b[0]).atan2(a[0] * b[0] + a[1] * b[1]).to_degrees()
}

/// A piece's joints: its end caps' (`logic::ends`), else its wall-walk's ends.
#[must_use]
pub fn joints(bytes: &[u8]) -> Vec<PieceEnd> {
    match crate::logic::ends(bytes) {
        Ok(e) if !e.is_empty() => e,
        _ => walkway_ends(bytes).unwrap_or_default(),
    }
}

fn parsed(bytes: &[u8]) -> Result<Value, String> {
    let p = Cs2Parsed::decode(&mut Cursor::new(bytes), &None).map_err(|e| format!("cs2.parsed: {e}"))?;
    serde_json::to_value(&p).map_err(|e| e.to_string())
}

fn level0(piece: &Value) -> Value {
    let levels = piece["destructs"].as_array().cloned().unwrap_or_default();
    levels.iter().find(|d| d["index"].as_u64() == Some(0)).or(levels.first()).cloned().unwrap_or(Value::Null)
}

fn point(p: &Value) -> [f32; 3] {
    [p["x"].as_f64().unwrap_or(0.0) as f32, p["y"].as_f64().unwrap_or(0.0) as f32, p["z"].as_f64().unwrap_or(0.0) as f32]
}

/// The joints of a piece without end caps (the 8 m pieces, the curves): the ends of its wall-walk. An end is an edge of a flat platform of
/// the main piece (level 0) that no other platform shares (a step piece's flats share theirs with its ramps), 3 to 9 units long (the walk
/// across: a platform's own short sides are 2); of those, the two farthest apart (on a curve the walk's sides are cut in edges as long as it
/// is wide). The wall leaves through it, away from its platform; its outer side is the end's corner farther from the platforms more than a
/// unit under it (the bank behind the wall), zero when there is none.
///
/// # Errors
///
/// The file doesn't read.
pub fn walkway_ends(bytes: &[u8]) -> Result<Vec<PieceEnd>, String> {
    let v = parsed(bytes)?;
    let Some(main) = v["pieces"].as_array().and_then(|p| p.first()).cloned() else { return Ok(vec![]) };
    let platforms = level0(&main)["platforms"].as_array().cloned().unwrap_or_default().iter().map(|p| {
        p["vertices"]["outline"].as_array().cloned().unwrap_or_default().iter().map(point).collect::<Vec<_>>()
    }).filter(|r| r.len() >= 3).collect::<Vec<_>>();
    let flat = |r: &Vec<[f32; 3]>| r.iter().all(|q| (q[1] - r[0][1]).abs() < 0.05);
    let same = |a: [f32; 3], b: [f32; 3]| (a[0] - b[0]).abs() < 0.02 && (a[2] - b[2]).abs() < 0.02 && (a[1] - b[1]).abs() < 0.05;
    let mut found = vec![];
    for (k, r) in platforms.iter().enumerate().filter(|(_, r)| flat(r)) {
        let n = r.len();
        let c = r.iter().fold([0.0f32; 2], |s, q| [s[0] + q[0] / n as f32, s[1] + q[2] / n as f32]);
        for i in 0..n {
            let (a, b) = (r[i], r[(i + 1) % n]);
            let shared = platforms.iter().enumerate().any(|(j, o)| {
                j != k && (0..o.len()).any(|m| {
                    let (p, q) = (o[m], o[(m + 1) % o.len()]);
                    (same(p, b) && same(q, a)) || (same(p, a) && same(q, b))
                })
            });
            let (dx, dz) = (b[0] - a[0], b[2] - a[2]);
            let len = dx.hypot(dz);
            if shared || !(3.0..=9.0).contains(&len) {
                continue;
            }

            let mid = [(a[0] + b[0]) * 0.5, a[1], (a[2] + b[2]) * 0.5];
            let mut o = [dz / len, -dx / len];
            if (mid[0] - c[0]) * o[0] + (mid[2] - c[1]) * o[1] < 0.0 {
                o = [-o[0], -o[1]];
            }

            found.push((a, b, mid, o));
        }
    }

    if found.len() > 2 {
        let mut best = (0, 1, f32::MIN);
        for i in 0..found.len() {
            for j in i + 1..found.len() {
                let d = (found[i].2[0] - found[j].2[0]).hypot(found[i].2[2] - found[j].2[2]);
                if d > best.2 {
                    best = (i, j, d);
                }
            }
        }

        found = vec![found[best.0], found[best.1]];
    }

    let low = found.iter().map(|f| f.2[1]).fold(f32::MAX, f32::min);
    let lower = platforms.iter().flatten().filter(|q| q[1] < low - 1.0).copied().collect::<Vec<_>>();
    let far = |q: [f32; 3]| lower.iter().map(|l| (l[0] - q[0]).hypot(l[2] - q[2])).fold(f32::MAX, f32::min);
    Ok(found.into_iter().map(|(a, b, mid, o)| {
        let side = if lower.is_empty() {
            [0.0, 0.0]
        } else {
            let outer = if far(a) > far(b) { a } else { b };
            let (sx, sz) = (outer[0] - mid[0], outer[2] - mid[2]);
            let l = sx.hypot(sz).max(1e-9);
            [sx / l, sz / l]
        };

        PieceEnd { at: mid, out: o, side }
    }).collect())
}

/// A piece's footprint (level 0, x / z): its `orange_thingies` rings, else its collision outlines: where the ground is shaped under it.
///
/// # Errors
///
/// The file doesn't read.
pub fn footprint(bytes: &[u8]) -> Result<Vec<Vec<[f32; 2]>>, String> {
    let v = parsed(bytes)?;
    let (mut rings, mut outlines) = (vec![], vec![]);
    for piece in v["pieces"].as_array().into_iter().flatten() {
        let d = level0(piece);
        for ring in d["orange_thingies"].as_array().into_iter().flatten() {
            let r = ring.as_array().into_iter().flatten().map(|q| [q["vertex"]["x"].as_f64().unwrap_or(0.0) as f32, q["vertex"]["y"].as_f64().unwrap_or(0.0) as f32]).collect::<Vec<_>>();
            if r.len() >= 3 {
                rings.push(r);
            }
        }

        for o in d["collision_outlines"].as_array().into_iter().flatten() {
            let r = o["vertices"]["outline"].as_array().into_iter().flatten().map(|q| [q["x"].as_f64().unwrap_or(0.0) as f32, q["z"].as_f64().unwrap_or(0.0) as f32]).collect::<Vec<_>>();
            if r.len() >= 3 {
                outlines.push(r);
            }
        }
    }

    Ok(if rings.is_empty() { outlines } else { rings })
}

// ---- laying (phase 2: on flat ground)

/// A two-jointed piece the way a wall goes through it: in by `entry`, out by `exit` (its frame), and its measures that way.
#[derive(Clone, Debug, PartialEq)]
pub struct Way {
    pub key: String,
    pub role: Role,
    pub entry: PieceEnd,
    pub exit: PieceEnd,
    pub span: f32,
    pub turn: f32,
    pub shift: f32,
    pub rise: f32,
}

/// A quarter turn towards the outer face of an unflipped wall: the wall's way rotated from +x towards +z (the procedural walls'
/// `z_axis`, `solver.rs`).
fn outer_of(way: [f32; 2]) -> [f32; 2] {
    [-way[1], way[0]]
}

fn dot(a: [f32; 2], b: [f32; 2]) -> f32 {
    a[0] * b[0] + a[1] * b[1]
}

/// The ways a piece can be gone through with the wall's outer face on its outer side: by its joints' `side` where one says it; a curve
/// that says neither has its outer face on its convex side (a ring of wall faces out); anything else either way (TDD's gondor step, whose
/// one piece goes down one way and up the other: user 2026-11-12).
#[must_use]
pub fn ways(piece: &KitPiece) -> Vec<Way> {
    let [a, b] = piece.joints.as_slice() else { return vec![] };
    let mut out = vec![];
    for (entry, exit) in [(*a, *b), (*b, *a)] {
        let way_in = [-entry.out[0], -entry.out[1]];
        let Ok(m) = classify(&piece.key, &[entry, exit]) else { continue };
        let known = if entry.side != [0.0, 0.0] { Some(dot(entry.side, outer_of(way_in)) > 0.0) } else if exit.side != [0.0, 0.0] { Some(dot(exit.side, outer_of(exit.out)) > 0.0) } else { None };
        let fits = match known {
            Some(f) => f,
            None if m.turn.abs() > IN_LINE => m.turn < 0.0,
            None => true,
        };

        if fits {
            out.push(Way { key: piece.key.clone(), role: piece.role, entry, exit, span: m.span, turn: m.turn, shift: m.shift, rise: m.rise });
        }
    }

    out
}

/// A piece laid: the key, its frame (origin in scene units; yaw: its +x in the world, radians from +x towards +z, as
/// `pieces::Piece`), its role, and the joints it joins at (x, z).
#[derive(Clone, Debug, PartialEq)]
pub struct Placed {
    pub key: String,
    pub role: Role,
    pub origin: [f64; 3],
    pub yaw: f64,
    pub joints: [[f64; 2]; 2],
}

/// How a wall of kit pieces is laid.
#[derive(Clone, Copy, Debug, Default)]
pub struct LayOptions {
    /// The base level (scene y) the wall stands at (flat: phase 2).
    pub level: f64,

    /// A tower or bastion every this much along the wall, a door section every this much (scene units; 0: none).
    pub defence_every: f64,
    pub access_every: f64,

    /// The ends closed with the culture's caps.
    pub caps: [bool; 2],
}

/// A wall laid with kit pieces, and how well it follows its line.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct KitLayout {
    pub placed: Vec<Placed>,

    /// The most a joint strayed from the line, and the line's length the pieces did not reach (scene units).
    pub stray: f64,
    pub short_by: f64,

    /// What could not be done (no straight, no cap of a side...).
    pub notes: Vec<String>,
}

/// A polyline with its lengths.
struct Line {
    pts: Vec<[f64; 2]>,
    s: Vec<f64>,
}

impl Line {
    fn new(pts: &[[f64; 2]]) -> Self {
        let mut s = vec![0.0];
        for w in pts.windows(2) {
            s.push(s.last().copied().unwrap_or(0.0) + (w[1][0] - w[0][0]).hypot(w[1][1] - w[0][1]));
        }

        Self { pts: pts.to_vec(), s }
    }

    fn length(&self) -> f64 {
        self.s.last().copied().unwrap_or(0.0)
    }

    /// The nearest point to `p`: its distance along, its distance from the line, and the line's way there (radians).
    fn nearest(&self, p: [f64; 2]) -> (f64, f64, f64) {
        let mut best = (0.0, f64::MAX, 0.0);
        for (i, w) in self.pts.windows(2).enumerate() {
            let (a, b) = (w[0], w[1]);
            let (dx, dz) = (b[0] - a[0], b[1] - a[1]);
            let len2 = dx * dx + dz * dz;
            let t = if len2 > 0.0 { (((p[0] - a[0]) * dx + (p[1] - a[1]) * dz) / len2).clamp(0.0, 1.0) } else { 0.0 };
            let d = (p[0] - a[0] - dx * t).hypot(p[1] - a[1] - dz * t);
            if d < best.1 {
                best = (self.s[i] + t * len2.sqrt(), d, dz.atan2(dx));
            }
        }

        best
    }
}

fn wrap(a: f64) -> f64 {
    let t = std::f64::consts::TAU;
    let a = a.rem_euclid(t);
    if a > std::f64::consts::PI { a - t } else { a }
}

/// A local (x, z) turned by `yaw` (from +x towards +z).
fn turned(v: [f64; 2], yaw: f64) -> [f64; 2] {
    let (s, c) = yaw.sin_cos();
    [v[0] * c - v[1] * s, v[0] * s + v[1] * c]
}

fn xz(e: &PieceEnd) -> [f64; 2] {
    [f64::from(e.at[0]), f64::from(e.at[2])]
}

fn angle_of(v: [f32; 2]) -> f64 {
    f64::from(v[1]).atan2(f64::from(v[0]))
}

/// Where a way takes a wall at joint `p` going `heading`: the piece placed, and the joint and heading it leaves by.
fn place(w: &Way, p: [f64; 2], heading: f64, level: f64) -> (Placed, [f64; 2], f64) {
    let yaw = wrap(heading - angle_of([-w.entry.out[0], -w.entry.out[1]]));
    let e = turned(xz(&w.entry), yaw);
    let origin = [p[0] - e[0], level, p[1] - e[1]];
    let x = turned(xz(&w.exit), yaw);
    let out = [origin[0] + x[0], origin[2] + x[1]];
    (Placed { key: w.key.clone(), role: w.role, origin, yaw, joints: [p, out] }, out, wrap(yaw + angle_of(w.exit.out)))
}

/// How many pieces a choice is judged with (its own and the best after it), what each piece costs and what its length is worth (scene
/// units of straying): of two choices that follow the line as well, the fewer, longer pieces.
const AHEAD: usize = 3;
const PIECE_COST: f64 = 1.5;
const LENGTH_WORTH: f64 = 0.1;

/// How far a heading off the line's counts, in scene units a radian.
const HEADING_WEIGHT: f64 = 12.0;

/// How well a joint at `p` going `heading` follows the line past `s_from` (a piece of `span` must go forward a quarter of itself), and
/// where along the line it is. `None` when it went back.
fn fit(line: &Line, p: [f64; 2], heading: f64, s_from: f64, span: f64) -> Option<(f64, f64)> {
    let (at, d, way) = line.nearest(p);
    if at < s_from + span * 0.25 {
        return None;
    }

    let over = (at - line.length()).max(0.0);
    Some((d + wrap(heading - way).abs() * HEADING_WEIGHT + over, at))
}

/// The cost of laying `w` at a joint, with the best `depth - 1` ways after it (each later one counting less); `None` when it goes back.
fn ahead(line: &Line, ways: &[&Way], w: &Way, (p, heading): ([f64; 2], f64), at: f64, depth: usize) -> Option<f64> {
    let (_, q, h) = place(w, p, heading, 0.0);
    let (e, at1) = fit(line, q, h, at, f64::from(w.span))?;
    let e = e + PIECE_COST - LENGTH_WORTH * f64::from(w.span);
    if depth <= 1 || at1 >= line.length() - 1e-6 {
        return Some(e);
    }

    let next = ways.iter().filter_map(|&w2| ahead(line, ways, w2, (q, h), at1, depth - 1)).fold(f64::MAX, f64::min);
    Some(e + if next < f64::MAX { next * 0.7 } else { 0.0 })
}

/// A way of the same shape as `w` among `alts` (a tower or door section in its place): the same span, turn and shift.
fn same_shape<'a>(w: &Way, alts: &[&'a Way]) -> Option<&'a Way> {
    alts.iter().copied().find(|a| (a.span - w.span).abs() < 0.05 && (a.turn - w.turn).abs() < 0.1 && (a.shift - w.shift).abs() < 0.05)
}

/// Lays a line (x, z, scene units, the wall's way: an unflipped wall's outer face on its left as `outer_of` turns) with a culture's pieces,
/// on flat ground: from its first point along its first segment, each joint the way that, with the best two after it, follows the line
/// best, until the line's end is nearer than half a straight. Every `defence_every` a tower or bastion of the same shape takes the place of
/// the piece laid, every `access_every` a door section; caps close the ends asked.
#[must_use]
pub fn lay(line: &[[f64; 2]], cat: &Catalogue, options: LayOptions) -> KitLayout {
    let mut out = KitLayout::default();
    if line.len() < 2 {
        return out;
    }

    let all = cat.pieces.iter().flat_map(ways).collect::<Vec<_>>();
    let laid = all.iter().filter(|w| matches!(w.role, Role::Straight | Role::Jog | Role::Turn | Role::Curve)).collect::<Vec<_>>();
    let (defences, accesses) = (all.iter().filter(|w| w.role == Role::Defence).collect::<Vec<_>>(), all.iter().filter(|w| w.role == Role::Access).collect::<Vec<_>>());
    let Some(straight) = laid.iter().filter(|w| w.role == Role::Straight).map(|w| f64::from(w.span)).reduce(f64::max) else {
        out.notes.push(format!("{}: no straight piece", cat.base));
        return out;
    };

    let line = Line::new(line);
    let mut p = line.pts[0];
    let mut heading = (line.pts[1][1] - p[1]).atan2(line.pts[1][0] - p[0]);
    let mut at = 0.0;
    let (mut since_defence, mut since_access) = (0.0, 0.0);
    for _ in 0..2000 {
        if at >= line.length() - straight * 0.5 {
            break;
        }

        let best = laid.iter().filter_map(|&w| ahead(&line, &laid, w, (p, heading), at, AHEAD).map(|c| (w, c))).min_by(|a, b| a.1.total_cmp(&b.1));
        let Some((mut w, _)) = best else { break };
        let span = f64::from(w.span);
        if options.defence_every > 0.0 && since_defence + span >= options.defence_every && let Some(d) = same_shape(w, &defences) {
            w = d;
            since_defence = 0.0;
        } else if options.access_every > 0.0 && since_access + span >= options.access_every && let Some(a) = same_shape(w, &accesses) {
            w = a;
            since_access = 0.0;
        }

        since_defence += span;
        since_access += span;
        let (placed, q, h) = place(w, p, heading, options.level);
        out.placed.push(placed);
        (p, heading) = (q, h);
        let (s, d, _) = line.nearest(p);
        out.stray = out.stray.max(d);
        at = s;
    }

    out.short_by = (line.length() - at).max(0.0);

    // The caps: a cap's one joint on the wall's end, its outer side on the wall's.
    let caps = cat.of(Role::Cap).collect::<Vec<_>>();
    let start = (line.pts[0], (line.pts[1][1] - line.pts[0][1]).atan2(line.pts[1][0] - line.pts[0][0]));
    for (k, (joint, wall_way)) in [(0, start), (1, (p, heading))] {
        if !options.caps[k] {
            continue;
        }

        // The way the wall leaves the cap through its joint: on along the wall at the start, back along it at the end.
        let leave = if k == 0 { wall_way } else { wrap(wall_way + std::f64::consts::PI) };
        let wall_dir = [wall_way.cos() as f32, wall_way.sin() as f32];
        let cap = caps.iter().find(|c| {
            let j = c.joints[0];
            j.side == [0.0, 0.0] || {
                // The cap's side turned into the world as it will be laid, against the wall's outer side there.
                let yaw = leave - angle_of(j.out);
                let s = turned([f64::from(j.side[0]), f64::from(j.side[1])], yaw);
                dot([s[0] as f32, s[1] as f32], outer_of(wall_dir)) > 0.0
            }
        });
        let Some(cap) = cap else {
            out.notes.push(format!("{}: no cap for the {} end", cat.base, if k == 0 { "first" } else { "last" }));
            continue;
        };

        let j = cap.joints[0];
        let yaw = wrap(leave - angle_of(j.out));
        let e = turned(xz(&j), yaw);
        let placed = Placed { key: cap.key.clone(), role: Role::Cap, origin: [joint[0] - e[0], options.level, joint[1] - e[1]], yaw, joints: [joint, joint] };
        if k == 0 { out.placed.insert(0, placed) } else { out.placed.push(placed) }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn end(at: [f32; 3], out: [f32; 2]) -> PieceEnd {
        PieceEnd { at, out, side: [0.0, -1.0] }
    }


    /// A small culture: a 20 long straight, a 90 degree corner each way, a tower of the straight's shape, two caps.
    fn culture() -> Catalogue {
        let side = [0.0, 1.0];
        let j = |at: [f32; 3], out: [f32; 2]| PieceEnd { at, out, side };
        let piece = |key: &str, joints: Vec<PieceEnd>| {
            let m = classify(key, &joints).unwrap();
            KitPiece { key: key.to_owned(), role: m.role, walk: joints[0].at[1], joints, span: m.span, turn: m.turn, rise: m.rise, shift: m.shift, footprint: vec![] }
        };
        let straight = vec![j([-10.0, 15.0, 0.0], [-1.0, 0.0]), j([10.0, 15.0, 0.0], [1.0, 0.0])];
        // The corners: in along +x, out towards -z (turning away from the outer side, +z) or +z.
        let c = |turn_to: f32| vec![j([-10.0, 15.0, 0.0], [-1.0, 0.0]), PieceEnd { at: [0.0, 15.0, 10.0 * turn_to], out: [0.0, turn_to], side: [1.0, 0.0] }];
        Catalogue {
            base: "x_fort_straight".to_owned(),
            pieces: vec![
                piece("x_fort_straight", straight.clone()),
                piece("x_fort_tower_c_straight", straight),
                piece("x_fort_external_corner", c(-1.0)),
                piece("x_fort_internal_corner", c(1.0)),
                piece("x_fort_endcap_left", vec![j([0.0, 15.0, 0.0], [1.0, 0.0])]),
                piece("x_fort_endcap_right", vec![PieceEnd { at: [0.0, 15.0, 0.0], out: [-1.0, 0.0], side }]),
            ],
            left_out: vec![],
        }
    }

    #[test]
    fn a_straight_line_is_laid_with_straights_joint_to_joint() {
        let laid = lay(&[[0.0, 0.0], [100.0, 0.0]], &culture(), LayOptions::default());
        assert_eq!(laid.placed.len(), 5, "{:?}", laid.placed.iter().map(|p| &p.key).collect::<Vec<_>>());
        assert!(laid.placed.iter().all(|p| p.key == "x_fort_straight"));
        for w in laid.placed.windows(2) {
            let (a, b) = (w[0].joints[1], w[1].joints[0]);
            assert!((a[0] - b[0]).hypot(a[1] - b[1]) < 1e-9, "joint to joint");
        }

        assert!(laid.stray < 1e-6 && laid.short_by < 1e-6, "{} {}", laid.stray, laid.short_by);
        // A piece's frame: its first joint at -10 along its x, so its origin is 10 along the wall.
        assert!((laid.placed[0].origin[0] - 10.0).abs() < 1e-9 && laid.placed[0].yaw.abs() < 1e-9, "{:?}", laid.placed[0]);
    }

    #[test]
    fn an_l_takes_a_corner_and_towers_come_at_their_spacing_and_caps_close_the_ends() {
        let line = [[0.0, 0.0], [100.0, 0.0], [100.0, -100.0]];
        let laid = lay(&line, &culture(), LayOptions { defence_every: 60.0, caps: [true, true], ..LayOptions::default() });
        let keys = laid.placed.iter().map(|p| p.key.as_str()).collect::<Vec<_>>();
        assert!(keys.contains(&"x_fort_external_corner"), "{keys:?}");
        assert!(keys.contains(&"x_fort_tower_c_straight"), "{keys:?}");
        assert_eq!((keys[0], keys[keys.len() - 1]), ("x_fort_endcap_left", "x_fort_endcap_right"), "{keys:?}");
        assert!(laid.stray < 12.0, "{}", laid.stray);
    }

    #[test]
    fn a_piece_goes_only_the_ways_that_keep_its_outer_face_out() {
        let c = culture();
        // A straight with its outer side known: one way; a corner: one way; a step that says no side: both.
        assert_eq!(ways(&c.pieces[0]).len(), 1);
        assert_eq!(ways(&c.pieces[2]).len(), 1);
        let step = KitPiece { key: "x_fort_straight_stepped_down_bastion_a".to_owned(), role: Role::Step, joints: vec![PieceEnd { at: [10.0, 15.0, 0.0], out: [1.0, 0.0], side: [0.0, 0.0] }, PieceEnd { at: [-10.0, 25.0, 0.0], out: [-1.0, 0.0], side: [0.0, 0.0] }], span: 20.0, turn: 0.0, rise: 10.0, shift: 0.0, walk: 15.0, footprint: vec![] };
        let w = ways(&step);
        assert_eq!(w.len(), 2);
        assert!(w.iter().any(|w| w.rise > 5.0) && w.iter().any(|w| w.rise < -5.0), "up one way, down the other");
    }

    #[test]
    fn the_prefix_and_the_culture_s_keys() {
        assert_eq!(prefix_of("roman_fort_straight"), "roman_fort_");
        let keys = ["roman_fort_straight", "Roman_Fort_Corner_External", "greek_fort_straight", "roman_fort_straight"];
        assert_eq!(culture_keys("roman_fort_straight", keys.iter().copied()), ["roman_fort_corner_external", "roman_fort_straight"]);
        let keys = ["eastern_fort_straight", "eastern_fort_small_wall_straight"];
        assert_eq!(culture_keys("eastern_fort_straight", keys.iter().copied()), ["eastern_fort_straight"]);
        assert_eq!(culture_keys("eastern_fort_small_wall_straight", keys.iter().copied()), ["eastern_fort_small_wall_straight"]);
    }

    #[test]
    fn angles_turn_from_x_towards_z() {
        assert!((signed_angle([1.0, 0.0], [0.0, 1.0]) - 90.0).abs() < 1e-4);
        assert!((signed_angle([1.0, 0.0], [0.0, -1.0]) + 90.0).abs() < 1e-4);
        assert!(signed_angle([1.0, 0.0], [1.0, 0.0]).abs() < 1e-4);
    }

    #[test]
    fn pieces_are_sorted_by_their_joints_and_names() {
        let (x0, x1) = (end([-10.0, 15.0, 0.0], [-1.0, 0.0]), end([10.0, 15.0, 0.0], [1.0, 0.0]));
        let m = classify("x_fort_straight", &[x0, x1]).unwrap();
        assert_eq!(m.role, Role::Straight);
        assert!((m.span - 20.0).abs() < 1e-4 && m.rise.abs() < 1e-4 && m.turn.abs() < 1e-4 && m.shift.abs() < 1e-4);

        // A kink: parallel joints, 10 across.
        let m = classify("x_fort_kink_left", &[x0, end([10.0, 15.0, 10.0], [1.0, 0.0])]).unwrap();
        assert!(m.role == Role::Jog && (m.shift - 10.0).abs() < 1e-4, "{m:?}");

        // A corner: out along +z after coming in along +x.
        let m = classify("x_fort_corner_external", &[x0, end([2.0, 15.0, 12.0], [0.0, 1.0])]).unwrap();
        assert!(m.role == Role::Turn && (m.turn - 90.0).abs() < 1e-3, "{m:?}");

        // A curve: 30 degrees, named so.
        let out = [30f32.to_radians().cos(), 30f32.to_radians().sin()];
        let m = classify("x_fort_curve_60m_radius", &[x0, end([10.0, 15.0, 3.0], out)]).unwrap();
        assert!(m.role == Role::Curve && (m.turn - 30.0).abs() < 1e-3, "{m:?}");

        // A step (even named a bastion), a tower, a door section, a cap.
        assert_eq!(classify("x_fort_straight_stepped_up_bastion_a", &[x0, end([10.0, 25.0, 0.0], [1.0, 0.0])]).unwrap().role, Role::Step);
        assert_eq!(classify("x_fort_tower_c_straight", &[x0, x1]).unwrap().role, Role::Defence);
        assert_eq!(classify("x_fort_straight_with_doors_a", &[x0, x1]).unwrap().role, Role::Access);
        assert_eq!(classify("x_fort_endcap_left", &[x0]).unwrap().role, Role::Cap);
        assert!(classify("x_fort_straight", &[x0]).is_err() && classify("x_fort_straight", &[]).is_err());
    }
}
