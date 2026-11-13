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

    /// Per joint, its platforms' edges on it (`joint_edges`): two pieces join where an edge of each has the same corners.
    pub edges: Vec<Vec<JointEdge>>,

    /// Where it stands (level 0, x / z): its `orange_thingies` rings, else its collision outlines.
    pub footprint: Vec<Vec<[f32; 2]>>,

    /// The ground it stands over (x / z, a convex ring): the hull of its level 0 platforms' corners and its footprint (the 8 m sets'
    /// `orange_thingies` are a strip along the outer face only, their earth bank is 14 units deep). Empty when there is nothing to read.
    pub area: Vec<[f32; 2]>,
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

    // Joints that join some joint of the culture's (the layout keeps every joint to one that joins it, `lay`): a piece with a joint that
    // joins none is of no use.
    let all = c.pieces.iter().flat_map(|p| p.edges.clone()).filter(|e| !e.is_empty()).collect::<Vec<_>>();
    let lone = |p: &KitPiece| p.edges.iter().position(|e| !e.is_empty() && !all.iter().any(|f| joins(e, f)));
    let (keep, other): (Vec<_>, Vec<_>) = c.pieces.into_iter().partition(|p| lone(p).is_none());
    c.pieces = keep;
    c.left_out.extend(other.into_iter().map(|p| {
        let k = lone(&p).unwrap_or_default();
        let across = p.edges[k].iter().map(|e| format!("{:.2}..{:.2}", e.lo, e.hi)).collect::<Vec<_>>().join(" ");
        (p.key.clone(), format!("its joint {k} ({across}) joins no joint of the culture's corner for corner"))
    }));

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
    let edges = joint_edges(bytes, &joints);
    let area = base_area(bytes, &footprint);
    Ok(KitPiece { key: key.to_owned(), role: m.role, walk: joints[0].at[1], joints, span: m.span, turn: m.turn, rise: m.rise, shift: m.shift, footprint, edges, area })
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
/// A platform's edge on a joint: across the wall from the joint's middle (`lo` < `hi`, positive on the left of the way out of the
/// piece), and the heights at its two ends over the joint's.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct JointEdge {
    pub lo: f32,
    pub hi: f32,
    pub y_lo: f32,
    pub y_hi: f32,
}

/// Per joint, the edges of the level 0 platforms (of any of the piece's pieces) on it: both corners within 0.05 of the joint's line
/// (through its middle, across its way out), at the walk's height (within 1) or under it.
#[must_use]
pub fn joint_edges(bytes: &[u8], joints: &[PieceEnd]) -> Vec<Vec<JointEdge>> {
    let Ok(v) = parsed(bytes) else { return vec![vec![]; joints.len()] };
    let rings = v["pieces"].as_array().into_iter().flatten().flat_map(|piece| {
        level0(piece)["platforms"].as_array().cloned().unwrap_or_default().iter().map(|p| {
            p["vertices"]["outline"].as_array().cloned().unwrap_or_default().iter().map(point).collect::<Vec<_>>()
        }).collect::<Vec<_>>()
    }).filter(|r| r.len() >= 3).collect::<Vec<_>>();
    joints.iter().map(|j| {
        let t = [-j.out[1], j.out[0]];
        let on = |q: [f32; 3]| ((q[0] - j.at[0]) * j.out[0] + (q[2] - j.at[2]) * j.out[1]).abs() <= 0.05 && q[1] <= j.at[1] + 1.0;
        let across = |q: [f32; 3]| (q[0] - j.at[0]) * t[0] + (q[2] - j.at[2]) * t[1];
        let mut out = rings.iter().flat_map(|r| (0..r.len()).map(move |i| (r[i], r[(i + 1) % r.len()]))).filter(|(a, b)| on(*a) && on(*b) && (across(*a) - across(*b)).abs() > 0.05).map(|(a, b)| {
            let (a, b) = if across(a) < across(b) { (a, b) } else { (b, a) };
            JointEdge { lo: across(a), hi: across(b), y_lo: a[1] - j.at[1], y_hi: b[1] - j.at[1] }
        }).collect::<Vec<_>>();
        out.sort_by(|a, b| a.lo.total_cmp(&b.lo));
        out.dedup_by(|a, b| (a.lo - b.lo).abs() < 1e-4 && (a.hi - b.hi).abs() < 1e-4);
        out
    }).collect()
}

/// How far apart two corners may be and still join (across; in height), as Check platforms joins walks (`docs/re/platforms_and_bridges.md`).
const CORNER_ACROSS: f32 = 0.0141;
const CORNER_HEIGHT: f32 = 0.1;

/// Whether two joints facing each other join: an edge of each at the walk's height with the same corners (across the one is the other's
/// mirrored: they face each other).
#[must_use]
pub fn joins(a: &[JointEdge], b: &[JointEdge]) -> bool {
    a.iter().filter(|e| e.y_lo.abs() <= CORNER_HEIGHT && e.y_hi.abs() <= CORNER_HEIGHT).any(|e| b.iter().any(|f| {
        (e.lo + f.hi).abs() <= CORNER_ACROSS && (e.hi + f.lo).abs() <= CORNER_ACROSS && (e.y_lo - f.y_hi).abs() <= CORNER_HEIGHT && (e.y_hi - f.y_lo).abs() <= CORNER_HEIGHT
    }))
}

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

/// The convex hull (x / z, counter-clockwise from +x towards +z) of a piece's level 0 platforms' corners (all its pieces) and its
/// footprint's.
#[must_use]
pub fn base_area(bytes: &[u8], footprint: &[Vec<[f32; 2]>]) -> Vec<[f32; 2]> {
    let mut pts = footprint.iter().flatten().copied().collect::<Vec<_>>();
    if let Ok(v) = parsed(bytes) {
        for piece in v["pieces"].as_array().into_iter().flatten() {
            for p in level0(piece)["platforms"].as_array().into_iter().flatten() {
                pts.extend(p["vertices"]["outline"].as_array().into_iter().flatten().map(point).map(|q| [q[0], q[2]]));
            }
        }
    }

    hull(pts)
}

/// The convex hull of points (Andrew's monotone chain), without the last point repeated.
fn hull(mut pts: Vec<[f32; 2]>) -> Vec<[f32; 2]> {
    pts.sort_by(|a, b| a[0].total_cmp(&b[0]).then(a[1].total_cmp(&b[1])));
    pts.dedup();
    if pts.len() < 3 {
        return pts;
    }

    let cross = |o: [f32; 2], a: [f32; 2], b: [f32; 2]| (a[0] - o[0]) * (b[1] - o[1]) - (a[1] - o[1]) * (b[0] - o[0]);
    let mut lower: Vec<[f32; 2]> = vec![];
    for &p in &pts {
        while lower.len() >= 2 && cross(lower[lower.len() - 2], lower[lower.len() - 1], p) <= 0.0 {
            lower.pop();
        }
        lower.push(p);
    }

    let mut upper: Vec<[f32; 2]> = vec![];
    for &p in pts.iter().rev() {
        while upper.len() >= 2 && cross(upper[upper.len() - 2], upper[upper.len() - 1], p) <= 0.0 {
            upper.pop();
        }
        upper.push(p);
    }

    lower.pop();
    upper.pop();
    lower.extend(upper);
    lower
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

    /// The platforms' edges on the entry and the exit (`KitPiece::edges`; empty: not known, joins anything).
    pub entry_edges: Vec<JointEdge>,
    pub exit_edges: Vec<JointEdge>,
    pub span: f32,
    pub turn: f32,
    pub shift: f32,
    pub rise: f32,
}

impl Way {
    /// The same piece gone through the other way (its entry the exit), with its measures that way.
    fn reversed(&self) -> Self {
        let m = classify(&self.key, &[self.exit, self.entry]).map_or((-self.span, -self.turn, -self.shift, -self.rise), |m| (m.span, m.turn, m.shift, m.rise));
        Self { key: self.key.clone(), role: self.role, entry: self.exit, exit: self.entry, entry_edges: self.exit_edges.clone(), exit_edges: self.entry_edges.clone(), span: m.0.abs(), turn: m.1, shift: m.2, rise: m.3 }
    }
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
    let edges = |i: usize| piece.edges.get(i).cloned().unwrap_or_default();
    let mut out = vec![];
    for ((entry, exit), (entry_edges, exit_edges)) in [((*a, *b), (edges(0), edges(1))), ((*b, *a), (edges(1), edges(0)))] {
        let way_in = [-entry.out[0], -entry.out[1]];
        let Ok(m) = classify(&piece.key, &[entry, exit]) else { continue };
        let known = if entry.side != [0.0, 0.0] { Some(dot(entry.side, outer_of(way_in)) > 0.0) } else if exit.side != [0.0, 0.0] { Some(dot(exit.side, outer_of(exit.out)) > 0.0) } else { None };
        let fits = match known {
            Some(f) => f,
            None if m.turn.abs() > IN_LINE => m.turn < 0.0,
            None => true,
        };

        if fits {
            out.push(Way { key: piece.key.clone(), role: piece.role, entry, exit, entry_edges, exit_edges, span: m.span, turn: m.turn, shift: m.shift, rise: m.rise });
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

    /// The level the wall stands on at each joint (scene y: the walk's height less the culture's walk): the same but at a step, whose
    /// ground goes from the one to the other along it (the wall's height layer, phase 4).
    pub levels: [f64; 2],
}

/// A building's joint a wall's end is attached to (phase 3; the world, scene units): a gate's, tower's or wall piece's end
/// (`logic::ends` through its instance's transform).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct EndJoint {
    pub at: [f64; 2],

    /// The way the wall leaves the building there (x, z, unit): away from it.
    pub out: [f64; 2],

    /// Across the wall towards its outer face (x, z, unit), zero when not known.
    pub side: [f64; 2],

    /// The joint's walk height, and the level the building stands on (scene y).
    pub walk: f64,
    pub level: f64,

    /// Its platforms' edges on it (`joint_edges`; empty: not known).
    pub edges: Vec<JointEdge>,
}

/// A procedural stretch of a kit wall (phase 3): from a kit piece's joint to a building's that the kit pieces could not meet cleanly, or
/// the other way. Both ends held where they are: their place, the wall's way there (x, z, unit; the outer face on its left as
/// `outer_of` turns) and the level the pieces stand on (at a building, its walk's height less the culture's walk: the walks meet,
/// the stretch climbing or dropping to it).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Stretch {
    pub from: [f64; 2],
    pub from_way: [f64; 2],
    pub from_level: f64,
    pub to: [f64; 2],
    pub to_way: [f64; 2],
    pub to_level: f64,
}

impl Stretch {
    /// The procedural wall it is (`solver::solve`'s input): its two ends held at their levels and on their joints' planes, the line
    /// between them the Bezier that leaves and arrives along the ways (handles a third of the way in).
    #[must_use]
    pub fn input(&self, tolerance: f64) -> crate::solver::WallInput {
        let d = (self.to[0] - self.from[0]).hypot(self.to[1] - self.from[1]) / 3.0;
        let out = [self.from[0] + self.from_way[0] * d, self.from[1] + self.from_way[1] * d];
        let into = [self.to[0] - self.to_way[0] * d, self.to[1] - self.to_way[1] * d];
        crate::solver::WallInput {
            points: vec![self.from, self.to],
            sharp: vec![false, false],
            handles: vec![[None, Some(out)], [Some(into), None]],
            flip: false,
            tolerance,
            ends: [Some(self.from_level), Some(self.to_level)],
            end_planes: [Some(self.from_way), Some(self.to_way)],
            access_every: 0.0,
            defence_every: 0.0,
        }
    }

    fn reversed(&self) -> Self {
        let neg = |v: [f64; 2]| [-v[0], -v[1]];
        Self { from: self.to, from_way: neg(self.to_way), from_level: self.to_level, to: self.from, to_way: neg(self.from_way), to_level: self.from_level }
    }

    #[must_use]
    pub fn length(&self) -> f64 {
        (self.to[0] - self.from[0]).hypot(self.to[1] - self.from[1])
    }
}

/// How a wall of kit pieces is laid.
#[derive(Clone, Debug, Default)]
pub struct LayOptions {
    /// The base level (scene y) the wall stands at when its first end is free, or attached to a building it cannot meet cleanly (flat:
    /// phases 2 and 3).
    pub level: f64,

    /// A tower or bastion every this much along the wall, a door section every this much (scene units; 0: none).
    pub defence_every: f64,
    pub access_every: f64,

    /// The ends closed with the culture's caps (the free ones: an attached end has none).
    pub caps: [bool; 2],

    /// The buildings' joints the ends are attached to.
    pub ends: [Option<EndJoint>; 2],
}

/// A wall laid with kit pieces, and how well it follows its line.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct KitLayout {
    pub placed: Vec<Placed>,

    /// The most a joint strayed from the line, and the line's length the pieces did not reach at a free end (scene units).
    pub stray: f64,
    pub short_by: f64,

    /// The procedural stretches at the attached ends the pieces could not meet cleanly, in the wall's way.
    pub stretches: Vec<Stretch>,

    /// Per end: free (`None`), or attached and met by a kit piece (`Some(true)`) or by a procedural stretch (`Some(false)`).
    pub attached: [Option<bool>; 2],

    /// What could not be done (no straight, no cap of a side...).
    pub notes: Vec<String>,

    /// The culture's walk over the level (scene units): a joint's walk is its level plus this.
    pub walk: f64,
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

    /// The point at `s` along, and the line's way there (radians).
    fn point_at(&self, s: f64) -> ([f64; 2], f64) {
        let i = self.s.partition_point(|&x| x <= s).clamp(1, self.pts.len() - 1);
        let (a, b) = (self.pts[i - 1], self.pts[i]);
        let len = self.s[i] - self.s[i - 1];
        let t = if len > 0.0 { ((s - self.s[i - 1]) / len).clamp(0.0, 1.0) } else { 0.0 };
        ([a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t], (b[1] - a[1]).atan2(b[0] - a[0]))
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

/// Where a way takes a wall at joint `p` going `heading` with its walk at `walk` (scene y): the piece placed (its entry joint's walk on
/// it: a piece whose walk is not quite its culture's, TDD's barbarian `600m` curves 0.42 off, stands that much higher or lower), and the
/// joint and heading it leaves by (its exit's walk: `walk` plus its rise).
fn place(w: &Way, p: [f64; 2], heading: f64, walk: f64) -> (Placed, [f64; 2], f64) {
    let yaw = wrap(heading - angle_of([-w.entry.out[0], -w.entry.out[1]]));
    let e = turned(xz(&w.entry), yaw);
    let origin = [p[0] - e[0], walk - f64::from(w.entry.at[1]), p[1] - e[1]];
    let x = turned(xz(&w.exit), yaw);
    let out = [origin[0] + x[0], origin[2] + x[1]];
    (Placed { key: w.key.clone(), role: w.role, origin, yaw, joints: [p, out], levels: [0.0; 2] }, out, wrap(yaw + angle_of(w.exit.out)))
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

/// The cost of laying `w` at a joint, with the best `depth - 1` ways after it that join it (each later one counting less); `None` when it
/// goes back or leads nowhere.
fn ahead(line: &Line, ways: &[&Way], w: &Way, (p, heading): ([f64; 2], f64), at: f64, depth: usize) -> Option<f64> {
    let (_, q, h) = place(w, p, heading, 0.0);
    let (e, at1) = fit(line, q, h, at, f64::from(w.span))?;
    let e = e + PIECE_COST - LENGTH_WORTH * f64::from(w.span);
    if depth <= 1 || at1 >= line.length() - 1e-6 {
        return Some(e);
    }

    // No way on from it (none joins its exit): a dead end.
    let next = ways.iter().filter(|w2| meets(&w.exit_edges, &w2.entry_edges)).filter_map(|&w2| ahead(line, ways, w2, (q, h), at1, depth - 1)).fold(f64::MAX, f64::min);
    (next < f64::MAX).then_some(e + next * 0.7)
}

/// The defence for the `n`-th slot, of the same shape as `w` among `alts`: a tower and a bastion in turn, the tower first (as a procedural
/// wall's `profile.defences`); the other kind where the culture has only one of that shape.
fn defence_of<'a>(w: &Way, alts: &[&'a Way], n: usize) -> Option<&'a Way> {
    let shaped = alts.iter().copied().filter(|a| same_shape(w, &[*a]).is_some()).collect::<Vec<_>>();
    let tower = n.is_multiple_of(2);
    shaped.iter().copied().find(|a| crate::towers::is_tower(&a.key) == tower).or_else(|| shaped.first().copied())
}

/// A way of the same shape as `w` among `alts` (a tower or door section in its place): the same span, turn and shift.
fn same_shape<'a>(w: &Way, alts: &[&'a Way]) -> Option<&'a Way> {
    alts.iter().copied().find(|a| (a.span - w.span).abs() < 0.05 && (a.turn - w.turn).abs() < 0.1 && (a.shift - w.shift).abs() < 0.05)
}

/// How far apart two joints may be and still meet (scene units, across; in height), as Check platforms joins walks
/// (`docs/re/platforms_and_bridges.md`, rule 6).
pub const JOINT_ACROSS: f64 = 0.01;
pub const JOINT_HEIGHT: f64 = 0.1;

/// How far two joints' ways may be off and still meet (radians: 0.1 degrees, a thousandth of a unit at a joint's corners).
const JOINT_TURN: f64 = 0.1 * std::f64::consts::PI / 180.0;

/// How many pieces an exact finish on an attached end may take, and from how far (straights) it is looked for.
const FINISH_PIECES: usize = 3;
const FINISH_REACH: f64 = 3.5;

/// How far a building's walk may be over its level from the culture's and the pieces still meet it (scene units): they stand that much
/// higher or lower.
const WALK_SLACK: f64 = 0.5;

/// How much worse (straights of straying) a step laid in place of a curve or kink may follow the line than the piece it replaces.
const STEP_DETOUR: f64 = 0.3;

/// A procedural stretch at an attached end: about a straight long (from half of one to one and a half).
const STRETCH_MIN: f64 = 0.5;
const STRETCH_MAX: f64 = 1.5;

/// Lays a line (x, z, scene units, the wall's way: an unflipped wall's outer face on its left as `outer_of` turns) with a culture's pieces,
/// on flat ground: from its first point along its first segment, each joint the way that, with the best two after it, follows the line
/// best, until the line's end is nearer than half a straight. Every `defence_every` a tower or bastion of the same shape takes the place of
/// the piece laid, every `access_every` a door section; caps close the free ends asked. **Every joint joins** (`joins`: an edge of each
/// with the same corners, as the game links platforms): a piece follows another only where its entry joins the other's exit (TDD's
/// gondor curves only after a curve or the curved joiner, its barbarian `600m` curves only after each other).
///
/// An attached end (phase 3) leaves or meets its building's joint along the building's way (the line's end made straight along it for a
/// straight's length). A first end is met by the pieces when the building's walk is the culture's (within [`WALK_SLACK`]; the wall at the
/// building's walk) and a piece joins its joint, else by a procedural stretch a straight long. A last end is met by the pieces when up to
/// [`FINISH_PIECES`] of them land on its joint exactly, joining it, at its walk, else by a procedural stretch from the last piece. With
/// only its last end attached, a wall is laid from that end, so that the pieces meet it.
#[must_use]
pub fn lay(line: &[[f64; 2]], cat: &Catalogue, options: LayOptions) -> KitLayout {
    lay_on(line, cat, options, None)
}

/// [`lay`] over the ground (phase 4; scene y at x, z, the ground **without** the wall's own height layer: docs/plans/kit_walls.md rule 1):
/// where an in-line piece is laid, a step of the culture's that joins there takes its place when the ground ahead (over the next piece and
/// a half) is nearer the level it steps to than the level the wall is at (only where it helps, rule 2). Without a ground, flat.
#[must_use]
pub fn lay_on(line: &[[f64; 2]], cat: &Catalogue, options: LayOptions, ground: Option<&dyn Fn(f64, f64) -> f64>) -> KitLayout {
    if options.ends[0].is_none() && options.ends[1].is_some() && line.len() >= 2 {
        let back = line.iter().rev().copied().collect::<Vec<_>>();
        let o = LayOptions { caps: [options.caps[1], options.caps[0]], ends: [options.ends[1].clone(), None], ..options };
        let mut laid = lay_from(&back, cat, &o, true, ground);
        laid.placed.reverse();
        for p in &mut laid.placed {
            p.joints.swap(0, 1);
            p.levels.swap(0, 1);
        }

        laid.stretches = laid.stretches.iter().rev().map(Stretch::reversed).collect();
        laid.attached.swap(0, 1);
        return laid;
    }

    lay_from(line, cat, &options, false, ground)
}

/// The line with its first `lead` made straight from `at` along `out` (the rest from where it is past `lead` along).
fn lead_in(pts: &[[f64; 2]], at: [f64; 2], out: [f64; 2], lead: f64) -> Vec<[f64; 2]> {
    let line = Line::new(pts);
    let mut v = vec![at, [at[0] + out[0] * lead, at[1] + out[1] * lead]];
    v.extend(pts.iter().zip(&line.s).filter(|(_, s)| **s > lead * 1.5).map(|(p, _)| *p));
    if v.len() == 2 {
        v.push(pts[pts.len() - 1]);
    }

    v
}

/// Whether a joint with these edges can be followed by one with those: they join, or either is not known.
fn meets(a: &[JointEdge], b: &[JointEdge]) -> bool {
    a.is_empty() || b.is_empty() || joins(a, b)
}

/// Up to `depth` ways from a joint that land on `goal` going `goal_way` (radians) exactly: the fewest.
fn finish<'a>(ways: &[&'a Way], (p, heading, edges): ([f64; 2], f64, &[JointEdge]), (goal, goal_way, goal_edges): ([f64; 2], f64, &[JointEdge]), depth: usize) -> Option<Vec<&'a Way>> {
    let met = |q: [f64; 2], h: f64| (q[0] - goal[0]).hypot(q[1] - goal[1]) <= JOINT_ACROSS && wrap(h - goal_way).abs() <= JOINT_TURN;
    if met(p, heading) && meets(edges, goal_edges) {
        return Some(vec![]);
    }

    let reach = (p[0] - goal[0]).hypot(p[1] - goal[1]);
    if depth == 0 || reach > ways.iter().map(|w| f64::from(w.span)).fold(0.0, f64::max) * depth as f64 + JOINT_ACROSS {
        return None;
    }

    ways.iter().filter(|w| meets(edges, &w.entry_edges)).filter_map(|&w| {
        let (_, q, h) = place(w, p, heading, 0.0);
        let mut rest = finish(ways, (q, h, &w.exit_edges), (goal, goal_way, goal_edges), depth - 1)?;
        rest.insert(0, w);
        Some(rest)
    }).min_by_key(Vec::len)
}

/// Whether two joints' edges are the same (what joins the one joins the other); either not known: the same.
fn same_edges(a: &[JointEdge], b: &[JointEdge]) -> bool {
    a.is_empty() || b.is_empty() || (a.len() == b.len() && a.iter().zip(b).all(|(e, f)| (e.lo - f.lo).abs() <= CORNER_ACROSS && (e.hi - f.hi).abs() <= CORNER_ACROSS && (e.y_lo - f.y_lo).abs() <= CORNER_HEIGHT && (e.y_hi - f.y_hi).abs() <= CORNER_HEIGHT))
}

/// `lay`, from the first end (`right`: the outer face on the line's right, a wall laid from its last end).
fn lay_from(line: &[[f64; 2]], cat: &Catalogue, options: &LayOptions, right: bool, ground: Option<&dyn Fn(f64, f64) -> f64>) -> KitLayout {
    let mut out = KitLayout::default();
    if line.len() < 2 {
        return out;
    }

    let all = cat.pieces.iter().flat_map(ways).map(|w| if right { w.reversed() } else { w }).collect::<Vec<_>>();
    let laid = all.iter().filter(|w| matches!(w.role, Role::Straight | Role::Jog | Role::Turn | Role::Curve)).collect::<Vec<_>>();
    let (defences, accesses) = (all.iter().filter(|w| w.role == Role::Defence).collect::<Vec<_>>(), all.iter().filter(|w| w.role == Role::Access).collect::<Vec<_>>());
    let in_line = |w: &Way| w.turn.abs() < IN_LINE && w.shift.abs() < JOG_SHIFT;
    let steps = all.iter().filter(|w| w.role == Role::Step && in_line(w)).collect::<Vec<_>>();
    let Some(longest) = laid.iter().filter(|w| w.role == Role::Straight).max_by(|a, b| a.span.total_cmp(&b.span)).copied() else {
        out.notes.push(format!("{}: no straight piece", cat.base));
        return out;
    };

    let straight = f64::from(longest.span);
    let walk = f64::from(longest.entry.at[1]);
    out.walk = walk;
    let outer = |way: f64| {
        let o = outer_of([way.cos() as f32, way.sin() as f32]);
        if right { [-f64::from(o[0]), -f64::from(o[1])] } else { [f64::from(o[0]), f64::from(o[1])] }
    };

    // The line's attached ends made straight along their buildings' ways.
    let mut pts = line.to_vec();
    let lead = straight.min(Line::new(&pts).length() * 0.3);
    if let Some(j) = &options.ends[0] {
        pts = lead_in(&pts, j.at, j.out, lead);
    }

    if let Some(j) = &options.ends[1] {
        let back = pts.iter().rev().copied().collect::<Vec<_>>();
        pts = lead_in(&back, j.at, j.out, lead).into_iter().rev().collect();
    }

    let line = Line::new(&pts);
    let way_of = |a: f64| [a.cos(), a.sin()];
    let mut level = options.level;
    let mut p = line.pts[0];
    let mut heading = (line.pts[1][1] - p[1]).atan2(line.pts[1][0] - p[0]);
    let mut at = 0.0;

    // The edges of the joint the next piece joins (empty: anything): a procedural stretch's piece ends as the culture's straight.
    let mut edges: Vec<JointEdge> = vec![];
    let side_off = |j: &EndJoint, way: f64| j.side != [0.0, 0.0] && {
        let o = outer(way);
        j.side[0] * o[0] + j.side[1] * o[1] < 0.0
    };

    // The first end on its building: on its joint at its walk when the walks match and a piece joins it, else a straight further on after
    // a stretch.
    if let Some(j) = &options.ends[0] {
        let way = j.out[1].atan2(j.out[0]);
        if side_off(j, way) {
            out.notes.push("the first end has its outer side the other way from the building's: flip the wall".to_owned());
        }

        let walks = ((j.walk - j.level) - walk).abs() <= WALK_SLACK;
        let joined = laid.iter().any(|w| meets(&j.edges, &w.entry_edges));
        if walks && joined {
            (level, p, heading) = (j.walk - walk, j.at, way);
            edges.clone_from(&j.edges);
            out.attached[0] = Some(true);
        } else {
            let why = if walks { "no piece joins its joint corner for corner".to_owned() } else { format!("its walk is {:.2} up, the pieces' {walk:.2}", j.walk - j.level) };
            let also = if meets(&j.edges, &longest.entry_edges) { "" } else { " (its pieces do not join it corner for corner either: another culture's)" };
            out.notes.push(format!("the first end's building: {why}: a procedural stretch meets it{also}"));
            at = straight.min(line.length() * 0.5);
            (p, heading) = line.point_at(at);
            out.stretches.push(Stretch { from: j.at, from_way: j.out, from_level: j.walk - walk, to: p, to_way: way_of(heading), to_level: level });
            edges.clone_from(&longest.exit_edges);
            out.attached[0] = Some(false);
        }
    }

    let first_edges = edges.clone();

    // The walk's height at the joint (scene y): the pieces are placed by it.
    let mut walk_at = level + walk;
    let goal = options.ends[1].as_ref().map(|j| (j, (-j.out[1]).atan2(-j.out[0])));
    let can_meet = goal.is_some_and(|(j, _)| (j.walk - walk_at).abs() <= JOINT_HEIGHT);
    let mut met = false;
    let mut history: Vec<([f64; 2], f64, f64, Vec<JointEdge>)> = vec![];

    // Per piece laid, its entry's edges (what the first end's cap joins).
    let mut entries: Vec<Vec<JointEdge>> = vec![];
    let (mut since_defence, mut since_access) = (0.0, 0.0);
    let mut defences_laid = 0usize;
    for _ in 0..2000 {
        let left = line.length() - at;
        match goal {
            Some((j, goal_way)) => {
                if can_meet && left <= straight * FINISH_REACH && let Some(ws) = finish(&laid, (p, heading, &edges), (j.at, goal_way, &j.edges), FINISH_PIECES) {
                    for w in ws {
                        let (mut placed, q, h) = place(w, p, heading, walk_at);
                        let before = walk_at;
                        walk_at = placed.origin[1] + f64::from(w.exit.at[1]);
                        placed.levels = [before - walk, walk_at - walk];
                        out.placed.push(placed);
                        entries.push(w.entry_edges.clone());
                        (p, heading) = (q, h);
                        edges.clone_from(&w.exit_edges);
                    }

                    met = true;
                    break;
                }

                if left <= straight * STRETCH_MAX {
                    break;
                }
            }
            None if left <= straight * 0.5 => break,
            None => {}
        }

        let best = laid.iter().filter(|w| meets(&edges, &w.entry_edges)).filter_map(|&w| ahead(&line, &laid, w, (p, heading), at, AHEAD).map(|c| (w, c))).min_by(|a, b| a.1.total_cmp(&b.1));
        let Some((mut w, _)) = best else {
            out.notes.push(format!("{}: no piece goes on from the joint at ({:.1}, {:.1})", cat.base, p[0], p[1]));
            break;
        };

        // A step in its place where the ground ahead is nearer the level it goes to (a step that joins here and that a piece goes on from).
        // In place of an in-line piece; in place of a curve or a kink too where the ground is a whole step or more off, when the step follows
        // the line nearly as well (a long curve up a hill would otherwise keep its level and be cut into it).
        if let Some(g) = ground {
            let level = walk_at - walk;
            let d = [heading.cos(), heading.sin()];
            let span = f64::from(w.span);
            let there = [0.5, 0.75, 1.0, 1.25, 1.5].iter().map(|k| g(p[0] + d[0] * span * k, p[1] + d[1] * span * k)).sum::<f64>() / 5.0;
            let off = |rise: f64| (there - level - rise).abs();
            let cost = |x: &Way| ahead(&line, &laid, x, (p, heading), at, AHEAD);
            let best_cost = if in_line(w) { None } else { cost(w) };
            let step = steps.iter().copied()
                .filter(|s| meets(&edges, &s.entry_edges) && laid.iter().any(|n| meets(&s.exit_edges, &n.entry_edges)))
                .filter(|s| off(f64::from(s.rise)) < off(0.0))
                .filter(|s| in_line(w) || off(0.0) >= f64::from(s.rise.abs()) && best_cost.zip(cost(s)).is_some_and(|(b, c)| c <= b + straight * STEP_DETOUR))
                .min_by(|a, b| off(f64::from(a.rise)).total_cmp(&off(f64::from(b.rise))));
            if let Some(s) = step {
                w = s;
            }
        }

        // A tower or door section in its place: of the same shape, joining as it does (not a step's).
        let span = f64::from(w.span);
        let fits = |a: &&Way| w.role != Role::Step && meets(&edges, &a.entry_edges) && same_edges(&a.exit_edges, &w.exit_edges);
        let (d_ok, a_ok) = (defences.iter().copied().filter(fits).collect::<Vec<_>>(), accesses.iter().copied().filter(fits).collect::<Vec<_>>());
        if options.defence_every > 0.0 && since_defence + span >= options.defence_every && let Some(d) = defence_of(w, &d_ok, defences_laid) {
            w = d;
            since_defence = 0.0;
            defences_laid += 1;
        } else if options.access_every > 0.0 && since_access + span >= options.access_every && let Some(a) = same_shape(w, &a_ok) {
            w = a;
            since_access = 0.0;
        }

        since_defence += span;
        since_access += span;
        history.push((p, heading, walk_at, edges.clone()));
        let (mut placed, q, h) = place(w, p, heading, walk_at);
        let before = walk_at;
        walk_at = placed.origin[1] + f64::from(w.exit.at[1]);
        placed.levels = [before - walk, walk_at - walk];
        out.placed.push(placed);
        entries.push(w.entry_edges.clone());
        (p, heading) = (q, h);
        edges.clone_from(&w.exit_edges);
        let (s, d, _) = line.nearest(p);
        out.stray = out.stray.max(d);
        at = s;
    }

    // The last end on its building: met by the pieces, else a stretch from the last of them (taken back while it came too near, or ends
    // on a joint the stretch's straight does not join).
    if let Some((j, goal_way)) = goal {
        if !met {
            while ((p[0] - j.at[0]).hypot(p[1] - j.at[1]) < straight * STRETCH_MIN || !meets(&edges, &longest.entry_edges)) && let Some((q, h, wh, e)) = history.pop() {
                out.placed.pop();
                entries.pop();
                (p, heading, walk_at, edges) = (q, h, wh, e);
            }

            let why = if !can_meet {
                format!("its walk is {:.2} up, the pieces' {walk:.2}", j.walk - j.level)
            } else if laid.iter().any(|w| meets(&w.exit_edges, &j.edges)) {
                "the pieces do not land on its joint".to_owned()
            } else {
                "no piece joins its joint corner for corner".to_owned()
            };
            let also = if meets(&longest.exit_edges, &j.edges) { "" } else { " (its pieces do not join it corner for corner either: another culture's)" };
            out.notes.push(format!("the last end: {why}: a procedural stretch meets it{also}"));
            out.stretches.push(Stretch { from: p, from_way: way_of(heading), from_level: walk_at - walk, to: j.at, to_way: way_of(goal_way), to_level: j.walk - walk });
        }

        if side_off(j, goal_way) {
            out.notes.push("the last end has its outer side the other way from the building's: flip the wall".to_owned());
        }

        out.attached[1] = Some(met);
    } else {
        out.short_by = (line.length() - at).max(0.0);
    }

    // The caps of the free ends: a cap's one joint on the wall's end, joining it, its outer side on the wall's.
    let caps = cat.of(Role::Cap).collect::<Vec<_>>();
    let start = (line.pts[0], (line.pts[1][1] - line.pts[0][1]).atan2(line.pts[1][0] - line.pts[0][0]), level + walk, entries.first().cloned().unwrap_or(first_edges));
    for (k, (joint, wall_way, cap_walk, end_edges)) in [(0, start), (1, (p, heading, walk_at, edges))] {
        if !options.caps[k] || options.ends[k].is_some() {
            continue;
        }

        // The way the wall leaves the cap through its joint: on along the wall at the start, back along it at the end.
        let leave = if k == 0 { wall_way } else { wrap(wall_way + std::f64::consts::PI) };
        let wall_outer = outer(wall_way);
        let cap = caps.iter().filter(|c| meets(c.edges.first().map_or(&[][..], Vec::as_slice), &end_edges)).find(|c| {
            let j = c.joints[0];
            j.side == [0.0, 0.0] || {
                // The cap's side turned into the world as it will be laid, against the wall's outer side there.
                let yaw = leave - angle_of(j.out);
                let s = turned([f64::from(j.side[0]), f64::from(j.side[1])], yaw);
                s[0] * wall_outer[0] + s[1] * wall_outer[1] > 0.0
            }
        });
        let Some(cap) = cap else {
            out.notes.push(format!("{}: no cap for the {} end", cat.base, if k == 0 { "first" } else { "last" }));
            continue;
        };

        let j = cap.joints[0];
        let yaw = wrap(leave - angle_of(j.out));
        let e = turned(xz(&j), yaw);
        let placed = Placed { key: cap.key.clone(), role: Role::Cap, origin: [joint[0] - e[0], cap_walk - f64::from(j.at[1]), joint[1] - e[1]], yaw, joints: [joint, joint], levels: [cap_walk - walk; 2] };
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
            KitPiece { key: key.to_owned(), role: m.role, walk: joints[0].at[1], joints, span: m.span, turn: m.turn, rise: m.rise, shift: m.shift, footprint: vec![], edges: vec![], area: vec![] }
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
        assert!(laid.stray < 12.0, "{} {:?}", laid.stray, laid.placed.iter().map(|p| (&p.key, p.joints)).collect::<Vec<_>>());
    }

    #[test]
    fn the_defences_take_a_tower_and_a_bastion_in_turn() {
        let mut c = culture();
        let straight = c.pieces.iter().find(|p| p.key == "x_fort_straight").unwrap().clone();
        c.pieces.push(KitPiece { key: "x_fort_straight_with_solid_squared_bastion".to_owned(), role: Role::Defence, ..straight });
        let laid = lay(&[[0.0, 0.0], [400.0, 0.0]], &c, LayOptions { defence_every: 60.0, ..LayOptions::default() });
        let defences = laid.placed.iter().filter(|p| p.role == Role::Defence).map(|p| p.key.as_str()).collect::<Vec<_>>();
        assert!(defences.len() >= 4, "{defences:?}");
        for (n, k) in defences.iter().enumerate() {
            assert_eq!(crate::towers::is_tower(k), n % 2 == 0, "{defences:?}");
        }

        // A culture with only one kind of that shape: that one at every slot.
        let laid = lay(&[[0.0, 0.0], [400.0, 0.0]], &culture(), LayOptions { defence_every: 60.0, ..LayOptions::default() });
        assert!(laid.placed.iter().filter(|p| p.role == Role::Defence).count() >= 4);
    }

    /// A building's end at `at`, the wall leaving it along `out`, its walk `walk` over its level 0.
    fn joint(at: [f64; 2], out: [f64; 2], walk: f64) -> EndJoint {
        EndJoint { at, out, side: [0.0, 0.0], walk, level: 0.0, edges: vec![] }
    }

    fn joined(laid: &KitLayout) {
        for w in laid.placed.windows(2) {
            let (a, b) = (w[0].joints[1], w[1].joints[0]);
            assert!((a[0] - b[0]).hypot(a[1] - b[1]) < 1e-6, "joint to joint: {:?}", laid.placed);
        }
    }

    #[test]
    fn ends_on_buildings_a_whole_number_of_pieces_apart_are_met_by_the_pieces() {
        let ends = [Some(joint([0.0, 0.0], [1.0, 0.0], 15.0)), Some(joint([100.0, 0.0], [-1.0, 0.0], 15.0))];
        let laid = lay(&[[0.0, 0.0], [100.0, 0.0]], &culture(), LayOptions { caps: [true, true], ends, ..LayOptions::default() });
        let keys = laid.placed.iter().map(|p| p.key.as_str()).collect::<Vec<_>>();
        assert_eq!(keys, ["x_fort_straight"; 5], "no caps on attached ends");
        assert!(laid.stretches.is_empty() && laid.attached == [Some(true), Some(true)], "{laid:?}");
        let last = laid.placed[4].joints[1];
        assert!((last[0] - 100.0).abs() < 1e-6 && last[1].abs() < 1e-6);
        joined(&laid);
    }

    #[test]
    fn an_end_the_pieces_cannot_meet_gets_a_procedural_stretch_about_a_straight_long() {
        let ends = [Some(joint([0.0, 0.0], [1.0, 0.0], 15.0)), Some(joint([95.0, 0.0], [-1.0, 0.0], 15.0))];
        let laid = lay(&[[0.0, 0.0], [95.0, 0.0]], &culture(), LayOptions { ends, ..LayOptions::default() });
        assert_eq!(laid.attached, [Some(true), Some(false)]);
        let [st] = laid.stretches.as_slice() else { panic!("one stretch: {laid:?}") };
        assert!(st.length() >= 10.0 - 1e-6 && st.length() <= 30.0 + 1e-6, "{st:?}");
        assert!((st.to[0] - 95.0).abs() < 1e-9 && st.to_way == [1.0, 0.0], "on the building's joint, going into it: {st:?}");
        let last = laid.placed.last().unwrap().joints[1];
        assert!((st.from[0] - last[0]).abs() < 1e-9 && (st.from[1] - last[1]).abs() < 1e-9, "from the last piece's joint");
        joined(&laid);
    }

    #[test]
    fn a_wall_attached_at_its_last_end_only_is_laid_from_it() {
        let ends = [None, Some(joint([93.0, 0.0], [-1.0, 0.0], 15.0))];
        let laid = lay(&[[0.0, 0.0], [93.0, 0.0]], &culture(), LayOptions { caps: [true, true], ends, ..LayOptions::default() });
        assert!(laid.stretches.is_empty() && laid.attached == [None, Some(true)], "{laid:?}");
        assert_eq!(laid.placed[0].key, "x_fort_endcap_left", "the free first end capped");
        let last = laid.placed.last().unwrap();
        assert_eq!(last.key, "x_fort_straight");
        assert!((last.joints[1][0] - 93.0).abs() < 1e-6 && last.joints[1][1].abs() < 1e-6, "on the joint: {last:?}");
        joined(&laid);
    }

    #[test]
    fn a_first_end_on_a_building_of_another_walk_height_starts_with_a_stretch() {
        let ends = [Some(joint([0.0, 0.0], [1.0, 0.0], 9.0)), None];
        let laid = lay(&[[0.0, 0.0], [100.0, 0.0]], &culture(), LayOptions { caps: [true, true], ends, ..LayOptions::default() });
        assert_eq!(laid.attached, [Some(false), None]);
        let [st] = laid.stretches.as_slice() else { panic!("one stretch: {laid:?}") };
        assert!(st.from == [0.0, 0.0] && (st.length() - 20.0).abs() < 1e-6, "{st:?}");
        assert!((laid.placed[0].joints[0][0] - 20.0).abs() < 1e-6, "the pieces from the stretch's end: {:?}", laid.placed[0]);
        assert_eq!(laid.placed.last().unwrap().key, "x_fort_endcap_right");
    }

    #[test]
    fn a_piece_follows_another_only_where_their_joints_join() {
        // The straights' joints: one edge 4 wide; the curves': one 6 wide (they join each other, not a straight); a joiner between.
        let e = |half: f32| vec![JointEdge { lo: -half, hi: half, y_lo: 0.0, y_hi: 0.0 }];
        let mut c = culture();
        for p in &mut c.pieces {
            p.edges = vec![e(2.0); p.joints.len()];
        }

        let side = [0.0, 1.0];
        let turn = 10f32.to_radians();
        let curve_joints = |a: [f32; 3]| vec![PieceEnd { at: a, out: [-1.0, 0.0], side }, PieceEnd { at: [a[0] + 20.0, 15.0, -1.7], out: [turn.cos(), -turn.sin()], side: [turn.sin(), turn.cos()] }];
        let curve = |key: &str, edges: Vec<Vec<JointEdge>>| {
            let joints = curve_joints([-10.0, 15.0, 0.0]);
            let m = classify(key, &joints).unwrap();
            KitPiece { key: key.to_owned(), role: m.role, walk: 15.0, joints, span: m.span, turn: m.turn, rise: m.rise, shift: m.shift, footprint: vec![], edges, area: vec![] }
        };
        c.pieces.push(curve("x_fort_curve_wide", vec![e(3.0), e(3.0)]));
        let line = (0..=40).map(|i| {
            let a = -std::f64::consts::FRAC_PI_2 * f64::from(i) / 40.0;
            [150.0 * a.sin().abs(), -150.0 + 150.0 * a.cos()]
        }).collect::<Vec<_>>();
        // Never side by side: a wide curve and a piece of the straights' joints (a straight, a corner, a tower, a cap).
        let apart = |l: &KitLayout| {
            let k = l.placed.iter().map(|p| p.key.as_str()).collect::<Vec<_>>();
            let wide = |k: &str| k == "x_fort_curve_wide";
            let narrow = |k: &str| !wide(k) && k != "x_fort_curved_joiner";
            assert!(k.windows(2).all(|w| !(wide(w[0]) && narrow(w[1]) || narrow(w[0]) && wide(w[1]))), "{k:?}");
            k.iter().filter(|k| wide(k)).count()
        };
        let laid = lay(&line, &c, LayOptions { caps: [true, true], ..LayOptions::default() });
        apart(&laid);

        // With a joiner (the straights' joint on one end, the curves' on the other) a wall can go from straights to curves.
        c.pieces.push(curve("x_fort_curved_joiner", vec![e(2.0), e(3.0)]));
        let mut l2 = line.clone();
        l2.insert(0, [-60.0, -150.0]);
        let laid = lay(&l2, &c, LayOptions { caps: [true, true], ..LayOptions::default() });
        assert!(apart(&laid) > 0, "{:?}", laid.placed.iter().map(|p| &p.key).collect::<Vec<_>>());
    }

    /// The test culture with a step: 20 long, its walk 10 higher at its second joint; its joints say no outer side (as TDD's gondor step),
    /// so that the other way it is a step down.
    fn culture_with_steps() -> Catalogue {
        let mut c = culture();
        let side = [0.0, 0.0];
        let joints = vec![PieceEnd { at: [-10.0, 15.0, 0.0], out: [-1.0, 0.0], side }, PieceEnd { at: [10.0, 25.0, 0.0], out: [1.0, 0.0], side }];
        let m = classify("x_fort_straight_stepped_up_bastion_a", &joints).unwrap();
        c.pieces.push(KitPiece { key: "x_fort_straight_stepped_up_bastion_a".to_owned(), role: m.role, walk: 15.0, joints, span: m.span, turn: m.turn, rise: m.rise, shift: m.shift, footprint: vec![], edges: vec![], area: vec![] });
        c
    }

    #[test]
    fn a_step_goes_where_the_ground_rises_by_more_than_half_of_it_and_the_levels_follow() {
        let c = culture_with_steps();
        let line = [[0.0, 0.0], [200.0, 0.0]];
        let ground = |x: f64, _: f64| if x < 100.0 { 0.0 } else { 10.0 };
        let laid = lay_on(&line, &c, LayOptions::default(), Some(&ground));
        let steps = laid.placed.iter().filter(|p| p.role == Role::Step).collect::<Vec<_>>();
        let [st] = steps.as_slice() else { panic!("one step: {:?}", laid.placed.iter().map(|p| (&p.key, p.joints, p.levels)).collect::<Vec<_>>()) };
        assert!(st.joints[0][0] >= 60.0 && st.joints[1][0] <= 120.0, "at the rise: {st:?}");
        assert_eq!(st.levels, [0.0, 10.0]);
        for p in &laid.placed {
            let want = if p.joints[0][0] >= st.joints[1][0] - 1e-6 { 10.0 } else if p.joints[1][0] <= st.joints[0][0] + 1e-6 { 0.0 } else { continue };
            assert!(p.levels.iter().all(|l| (l - want).abs() < 1e-9), "{p:?}");
            assert!((p.origin[1] - want).abs() < 1e-9, "a straight stands on its level: {p:?}");
        }

        // Down again where it drops; and none where the ground rises by less than half a step (it would leave the wall farther off).
        let down = |x: f64, _: f64| if x < 100.0 { 10.0 } else { 0.0 };
        let laid = lay_on(&line, &c, LayOptions { level: 10.0, ..LayOptions::default() }, Some(&down));
        assert!(laid.placed.iter().any(|p| p.role == Role::Step && p.levels == [10.0, 0.0]), "{:?}", laid.placed.iter().map(|p| (&p.key, p.levels)).collect::<Vec<_>>());
        let bump = |x: f64, _: f64| if x < 100.0 { 0.0 } else { 4.0 };
        assert!(lay_on(&line, &c, LayOptions::default(), Some(&bump)).placed.iter().all(|p| p.role != Role::Step));
    }

    #[test]
    fn a_piece_goes_only_the_ways_that_keep_its_outer_face_out() {
        let c = culture();
        // A straight with its outer side known: one way; a corner: one way; a step that says no side: both.
        assert_eq!(ways(&c.pieces[0]).len(), 1);
        assert_eq!(ways(&c.pieces[2]).len(), 1);
        let step = KitPiece { key: "x_fort_straight_stepped_down_bastion_a".to_owned(), role: Role::Step, joints: vec![PieceEnd { at: [10.0, 15.0, 0.0], out: [1.0, 0.0], side: [0.0, 0.0] }, PieceEnd { at: [-10.0, 25.0, 0.0], out: [-1.0, 0.0], side: [0.0, 0.0] }], span: 20.0, turn: 0.0, rise: 10.0, shift: 0.0, walk: 15.0, footprint: vec![], edges: vec![], area: vec![] };
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
    fn the_hull_of_points_is_their_convex_ring() {
        let h = hull(vec![[0.0, 0.0], [2.0, 0.0], [1.0, 1.0], [2.0, 2.0], [0.0, 2.0], [1.0, 0.0]]);
        assert_eq!(h, vec![[0.0, 0.0], [2.0, 0.0], [2.0, 2.0], [0.0, 2.0]]);
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

