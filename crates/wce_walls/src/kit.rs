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

/// The keys of a culture: those of `keys` (every building with a logic file) that start with its prefix, sorted.
#[must_use]
pub fn culture_keys<'a>(base: &str, keys: impl Iterator<Item = &'a str>) -> Vec<String> {
    let prefix = prefix_of(base);
    let mut out = keys.map(str::to_lowercase).filter(|k| k.starts_with(&prefix)).collect::<Vec<_>>();
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

#[cfg(test)]
mod tests {
    use super::*;

    fn end(at: [f32; 3], out: [f32; 2]) -> PieceEnd {
        PieceEnd { at, out, side: [0.0, -1.0] }
    }

    #[test]
    fn the_prefix_and_the_culture_s_keys() {
        assert_eq!(prefix_of("roman_fort_straight"), "roman_fort_");
        let keys = ["roman_fort_straight", "Roman_Fort_Corner_External", "greek_fort_straight", "roman_fort_straight"];
        assert_eq!(culture_keys("roman_fort_straight", keys.iter().copied()), ["roman_fort_corner_external", "roman_fort_straight"]);
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
