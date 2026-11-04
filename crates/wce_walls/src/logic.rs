//! A piece's logic file (`<key>_tech.cs2.parsed`) under a warp. rpfm_lib keeps `Cs2Parsed`'s fields private; it is edited through its
//! serde form, as the editor reads it (`wce_tile_editor`'s columns).
//!
//! Per piece and destruction level:
//! - **platforms** (the walkable surfaces) are cut into strips along x, warped, and get their normal back from their new outline;
//! - **firing lines** (`ef_lines`) are moved and turned; their `parent_index` (the platform they stand on) follows the strip under them;
//! - **collision outlines**, the **footprint** (`orange_thingies`, x / z) and **docking lines** get points along their long edges first;
//! - **pipes** (door and ladder links) are moved point by point;
//! - every **transform** (attached buildings, emitters, the map flag) is moved and turned; the pieces' own `node_transform` is not;
//! - **boxes** are recomputed around the warped box.

use std::io::Cursor;

use rpfm_lib::files::cs2_parsed::Cs2Parsed;
use rpfm_lib::files::{Decodeable, Encodeable};
use serde_json::{Map, Value, json};

use crate::warp::{Cuts, Warp, clip_to, inside, newell, normalised, slice_along_x, subdivide, transform, turn};

/// What warping a logic file did.
#[derive(Clone, Debug, Default)]
pub struct LogicNotes {
    /// Platforms before and after cutting.
    pub platforms: (usize, usize),

    /// Firing lines whose platform was not found among the strips (kept on the strip nearest them).
    pub orphan_lines: usize,

    /// What the warp does not handle and was left as it was.
    pub warnings: Vec<String>,
}

/// Another building's part brought into a piece (a step section): its logic file's platforms and firing lines over `x` of its frame
/// (and inside `zy`, z then y, when given), warped by its own `warp`, added to the first piece's levels that the part has (the part's
/// level of the same number). A level the part lacks (a step piece has no rubble) gets nothing of it, and keeps the base whole.
pub struct Splice<'a> {
    pub logic: &'a [u8],
    pub warp: &'a dyn Warp,
    pub x: (f32, f32),
    pub zy: Option<((f32, f32), (f32, f32))>,
    pub band: Option<(f32, f32, f32)>,

    /// The base's x range (its frame) it replaces where it is brought: the base's platforms and firing lines there (inside the
    /// `drop_zy` box) are left out.
    pub replaces: Option<(f32, f32)>,

    /// Each of the part's pieces into the piece of the same number (a copy of the base itself), not only its first into the first.
    pub per_piece: bool,

    /// Its collision outlines and footprints too (a copy of the base filling a mitred corner's outer side: without them the corner had
    /// walkway but no footprint nor hard outline; user 2026-11-04).
    pub shapes_too: bool,
}

impl Splice<'_> {
    /// If a point of the part's frame is in what is brought.
    pub fn holds(&self, p: [f32; 3]) -> bool {
        crate::mesh::splice_holds(p, self.x, self.zy, self.band)
    }
}

/// A logic file warped; platforms are cut along x at `cuts`, long outlines get a point at least every `cuts.step`.
///
/// # Errors
///
/// The file doesn't read, or doesn't write back.
pub fn warp_logic(bytes: &[u8], warp: &dyn Warp, cuts: &Cuts) -> Result<(Vec<u8>, LogicNotes), String> {
    warp_logic_with(bytes, warp, cuts, None, &[])
}

/// [`warp_logic`] with step sections: per level, the splices' platforms and firing lines added, the base's over what they replace (inside
/// `drop_zy` when given) left out.
///
/// # Errors
///
/// A file doesn't read, or the result doesn't write back.
pub fn warp_logic_with(bytes: &[u8], warp: &dyn Warp, cuts: &Cuts, drop_zy: Option<((f32, f32), (f32, f32))>, splices: &[Splice]) -> Result<(Vec<u8>, LogicNotes), String> {
    let step = cuts.step;
    let parsed = Cs2Parsed::decode(&mut Cursor::new(bytes), &None).map_err(|e| format!("cs2.parsed: {e}"))?;
    let mut v = serde_json::to_value(&parsed).map_err(|e| e.to_string())?;
    let mut notes = LogicNotes::default();
    let in_box = |p: [f32; 3]| drop_zy.is_none_or(|((z0, z1), (y0, y1))| p[2] >= z0 && p[2] <= z1 && p[1] >= y0 && p[1] <= y1);
    let planes = warp.clip_planes();

    // The splices' levels (their first piece's destructs).
    let mut parts = vec![];
    for sp in splices {
        let parsed = Cs2Parsed::decode(&mut Cursor::new(sp.logic), &None).map_err(|e| format!("cs2.parsed (step section): {e}"))?;
        let pv = serde_json::to_value(&parsed).map_err(|e| e.to_string())?;
        parts.push(pv["pieces"].as_array().into_iter().flatten().map(|p| p["destructs"].as_array().cloned().unwrap_or_default()).collect::<Vec<_>>());
    }

    let mut piece_index = 0;
    for piece in array_mut(&mut v, "pieces") {
        let piece_name = piece["name"].as_str().unwrap_or_default().to_owned();
        for (k, d) in array_mut(piece, "destructs").enumerate() {
            let name = d["name"].as_str().unwrap_or(&piece_name).to_owned();
            // The splices brought into this level: the ones that have it.
            let here = splices.iter().zip(&parts).filter_map(|(sp, pieces)| {
                let levels = match (sp.per_piece, piece_index) {
                    (true, i) => pieces.get(i),
                    (false, 0) => pieces.first(),
                    _ => None,
                };

                levels.and_then(|l| l.get(k)).map(|sd| (sp, sd))
            }).collect::<Vec<_>>();
            // What a step replaces goes from every piece of the base (its end caps' platforms are pieces of their own), not only from the
            // one its own platforms go into; at the levels the step has (a level it lacks keeps the base whole).
            let drop = splices.iter().zip(&parts).filter(|(sp, pieces)| !sp.per_piece && pieces.first().is_some_and(|l| l.get(k).is_some()))
                .filter_map(|(sp, _)| sp.replaces).collect::<Vec<_>>();
            let kept = |p: [f32; 3]| inside(&planes, p) && !(in_box(p) && drop.iter().any(|&(a, b)| p[0] > a + 1e-3 && p[0] < b - 1e-3));
            // The base's platforms less what the steps replace: cut out of them (a platform wider than the step's cross-section keeps the
            // rest of its width), so the walkway and a step never lie one over the other.
            // Grouped by the platform they come from: the firing lines' `parent_index` counts the platforms as they were.
            let old = carve_platforms(d["platforms"].as_array().map_or(&[][..], Vec::as_slice), &drop, drop_zy);
            let on_planes = |p: [f32; 3]| inside(&planes, p);
            let (mut platforms, strips) = platform_strips(&old, warp, cuts, &on_planes, 0, &mut notes);
            let lines = d["ef_lines"].as_array().cloned().unwrap_or_default();
            let mut ef = place_lines(&lines, warp, cuts, &strips, &kept, &mut notes);
            let mut shapes = vec![];
            for (sp, sd) in here {
                if sp.shapes_too {
                    shapes.push((sp, sd.clone()));
                }

                let part_cuts = Cuts { step, at: vec![sp.x.0, sp.x.1] };
                let part_planes = sp.warp.clip_planes();
                let holds = |p: [f32; 3]| sp.holds(p) && inside(&part_planes, p);
                let theirs = sd["platforms"].as_array().into_iter().flatten().map(|p| vec![p.clone()]).collect::<Vec<_>>();
                let (more, s2) = platform_strips(&theirs, sp.warp, &part_cuts, &holds, platforms.len(), &mut notes);
                platforms.extend(more);
                ef.extend(place_lines(sd["ef_lines"].as_array().map_or(&[][..], Vec::as_slice), sp.warp, &part_cuts, &s2, &holds, &mut notes));
            }

            d["platforms"] = Value::Array(platforms);
            d["ef_lines"] = Value::Array(ef);
            for o in array_mut(d, "collision_outlines") {
                warp_outline(&mut o["vertices"], warp, step, &planes);
            }

            for ring in array_mut(d, "orange_thingies") {
                warp_footprint(ring, warp, step, &planes);
            }

            for (sp, mut sd) in shapes {
                let part_planes = sp.warp.clip_planes();
                for o in array_mut(&mut sd, "collision_outlines") {
                    warp_outline(&mut o["vertices"], sp.warp, step, &part_planes);
                }

                for ring in array_mut(&mut sd, "orange_thingies") {
                    warp_footprint(ring, sp.warp, step, &part_planes);
                }

                let kept = |v: &Value, min: usize| v.as_array().is_some_and(|a| a.len() >= min);
                for (key, inner) in [("collision_outlines", true), ("orange_thingies", false)] {
                    let more = sd[key].as_array().into_iter().flatten().filter(|o| if inner { kept(&o["vertices"]["outline"], 2) } else { kept(o, 3) }).cloned().collect::<Vec<_>>();
                    if let Some(list) = d.get_mut(key).and_then(Value::as_array_mut) {
                        list.extend(more);
                    }
                }
            }

            // Pipes cut off by a corner are left out.
            if let Some(pipes) = d.get_mut("pipes").and_then(Value::as_array_mut) {
                pipes.retain(|pipe| pipe["line"]["outline"].as_array().into_iter().flatten().all(|q| inside(&planes, p3(q))));
            }

            // A pipe's points at the ground (a door's or a ladder's foot, under 1 unit up) go onto the terrain.
            for pipe in array_mut(d, "pipes") {
                for p in array_mut(&mut pipe["line"], "outline") {
                    let q = p3(p);
                    set_p3(p, if q[1] < 1.0 { warp.ground_point(q) } else { warp.point(q) });
                }
            }

            for l in array_mut(d, "docking_lines") {
                let (s, e) = (p2(&l["start"]), p2(&l["end"]));
                let mid = [(s[0] + e[0]) / 2.0, 0.0, (s[1] + e[1]) / 2.0];
                let a = warp.yaw(mid);
                for k in ["start", "end"] {
                    let p = p2(&l[k]);
                    let q = warp.point([p[0], 0.0, p[1]]);
                    l[k] = json!({ "x": q[0], "y": q[2] });
                }

                let t = turn(p2(&l["direction"]), a);
                l["direction"] = json!({ "x": t[0], "y": t[1] });
            }

            if d["bin_data"].as_array().is_some_and(|b| !b.is_empty()) {
                notes.warnings.push(format!("{name}: bin_data kept as it is (not understood)"));
            }

            if d["gates"].as_array().is_some_and(|b| !b.is_empty()) {
                notes.warnings.push(format!("{name}: gates kept as they are"));
            }

            warp_box(&mut d["bounding_box"], warp, step);
        }

        piece_index += 1;
    }

    warp_box(&mut v["bounding_box"], warp, step);
    warp_transforms(&mut v, warp);

    let mut parsed: Cs2Parsed = serde_json::from_value(v).map_err(|e| e.to_string())?;
    let mut out = vec![];
    parsed.encode(&mut out, &None).map_err(|e| format!("cs2.parsed: {e}"))?;
    Ok((out, notes))
}

/// The x range of a logic file's box (where the piece starts and ends along the wall).
///
/// # Errors
///
/// The file doesn't read.
pub fn extent_x(bytes: &[u8]) -> Result<(f32, f32), String> {
    let parsed = Cs2Parsed::decode(&mut Cursor::new(bytes), &None).map_err(|e| format!("cs2.parsed: {e}"))?;
    let v = serde_json::to_value(&parsed).map_err(|e| e.to_string())?;
    let f = |k: &str| v["bounding_box"][k].as_f64().unwrap_or(0.0) as f32;
    Ok((f("min_x"), f("max_x")))
}

/// The z extent of a piece's walkway: its intact platforms lying at `height` (within a few centimetres). None without any.
pub fn walkway_z(bytes: &[u8], height: f32) -> Option<(f32, f32)> {
    let parsed = Cs2Parsed::decode(&mut Cursor::new(bytes), &None).ok()?;
    let v = serde_json::to_value(&parsed).ok()?;
    let zs = v["pieces"].as_array().into_iter().flatten().filter_map(|p| p["destructs"].as_array().and_then(|d| d.first()))
        .flat_map(|d| d["platforms"].as_array().into_iter().flatten().map(|q| outline(&q["vertices"])).collect::<Vec<_>>())
        .filter(|o| !o.is_empty() && o.iter().all(|q| (q[1] - height).abs() < 0.05))
        .flat_map(|o| o.into_iter().map(|q| q[2]))
        .collect::<Vec<_>>();
    (!zs.is_empty()).then(|| zs.iter().fold((f32::MAX, f32::MIN), |(a, b), &z| (a.min(z), b.max(z))))
}

/// The x extent a wall attaches to on a fort piece: where its walkway ends (its platforms' x extent, every piece's intact level), else
/// its box (`extent_x`) when the two differ by more than half a metre. TDD's gondor pieces' box is -10.53..10.76 while their walkways end
/// at -10.5 and 10.5: a wall on the box's end left a gap of a quarter of a metre (user, 2026-11-04).
///
/// # Errors
///
/// The file doesn't read.
pub fn joint_extent_x(bytes: &[u8]) -> Result<(f32, f32), String> {
    let (lo, hi) = extent_x(bytes)?;
    let parsed = Cs2Parsed::decode(&mut Cursor::new(bytes), &None).map_err(|e| format!("cs2.parsed: {e}"))?;
    let v = serde_json::to_value(&parsed).map_err(|e| e.to_string())?;
    let xs = v["pieces"].as_array().into_iter().flatten().filter_map(|p| p["destructs"].as_array().and_then(|d| d.first()))
        .flat_map(|d| d["platforms"].as_array().into_iter().flatten().flat_map(|q| outline(&q["vertices"])).map(|q| q[0]).collect::<Vec<_>>())
        .collect::<Vec<_>>();
    let (wlo, whi) = xs.iter().fold((f32::MAX, f32::MIN), |(a, b), &x| (a.min(x), b.max(x)));
    let near = |w: f32, b: f32| (w - b).abs() <= 0.5;
    Ok((if near(wlo, lo) { wlo } else { lo }, if near(whi, hi) { whi } else { hi }))
}

/// One end of a fort piece where another joins it: the middle of the joint (the outer side of the end's cap platform, piece frame), the
/// direction the wall leaves the piece there (x, z, unit) and the wall-walk's height at the joint.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PieceEnd {
    pub at: [f32; 3],
    pub out: [f32; 2],

    /// Across the wall towards its outer face (x, z, unit; the cap lies on the wall-walk's outer band); zero when not known. Two pieces
    /// join with their outer faces on the same side.
    pub side: [f32; 2],
}

/// A fort piece's ends from its logic file (level 0): every end has a cap, a platform of its own piece (not the first) about 2 m along
/// the wall by 2.5 m across (the kit's Roman walls, towers, corners, joiners, gateways, stepped bastions; 2026-10-31). The joint is the
/// cap's side across the wall that is farther from the main piece's platforms; the wall leaves through it. Empty when no cap is found.
///
/// # Errors
///
/// The file doesn't read.
pub fn ends(bytes: &[u8]) -> Result<Vec<PieceEnd>, String> {
    let parsed = Cs2Parsed::decode(&mut Cursor::new(bytes), &None).map_err(|e| format!("cs2.parsed: {e}"))?;
    let v = serde_json::to_value(&parsed).map_err(|e| e.to_string())?;
    let pt = |p: &Value| [p["x"].as_f64().unwrap_or(0.0) as f32, p["y"].as_f64().unwrap_or(0.0) as f32, p["z"].as_f64().unwrap_or(0.0) as f32];
    // The intact level: index 0 (a piece's levels are not always listed in order).
    let level0 = |piece: &Value| {
        let levels = piece["destructs"].as_array().cloned().unwrap_or_default();
        levels.iter().find(|d| d["index"].as_u64() == Some(0)).or(levels.first()).cloned().unwrap_or(Value::Null)
    };
    let platforms = |level: &Value| -> Vec<Vec<[f32; 3]>> {
        level["platforms"].as_array().map_or(vec![], |ps| ps.iter().map(|p| p["vertices"]["outline"].as_array().map_or(vec![], |o| o.iter().map(pt).collect())).collect())
    };

    let pieces = v["pieces"].as_array().cloned().unwrap_or_default();
    let Some(main) = pieces.first() else { return Ok(vec![]) };
    let body: Vec<[f32; 3]> = platforms(&level0(main)).into_iter().flatten().collect();
    if body.is_empty() {
        return Ok(vec![]);
    }

    let centre = body.iter().fold([0.0f32; 2], |c, p| [c[0] + p[0], c[1] + p[2]]).map(|c| c / body.len() as f32);
    let flat: Vec<[f32; 2]> = body.iter().map(|p| [p[0], p[2]]).collect();
    let mut out = vec![];
    for piece in pieces.iter().skip(1) {
        for cap in platforms(&level0(piece)).into_iter().filter(|c| c.len() == 4) {
            out.extend(cap_end(&cap, centre, &flat));
        }
    }

    Ok(out)
}

/// The joint of an end cap (4 points): its side across the wall (about 2.5 m) that is farthest from `centre` (x, z); the wall leaves
/// through it, square to it. The caps are 2 m by 2.5 m rectangles, or a stepped bastion's quadrilateral with a slanted inner side. Its outer
/// side: where the cap lies across the wall against the main piece's wall-walk on the joint line (`body`, x and z of the main piece's
/// platform points), else against `centre`.
fn cap_end(cap: &[[f32; 3]], centre: [f32; 2], body: &[[f32; 2]]) -> Option<PieceEnd> {
    let len = |a: [f32; 3], b: [f32; 3]| (b[0] - a[0]).hypot(b[2] - a[2]);
    let mid = |a: [f32; 3], b: [f32; 3]| [(a[0] + b[0]) * 0.5, (a[1] + b[1]) * 0.5, (a[2] + b[2]) * 0.5];
    let d = |m: [f32; 3]| (m[0] - centre[0]).hypot(m[2] - centre[1]);
    let span = (0..4).flat_map(|i| (i + 1..4).map(move |j| (i, j))).map(|(i, j)| len(cap[i], cap[j])).fold(0.0f32, f32::max);
    if !(2.0..=4.5).contains(&span) {
        return None;
    }

    // The joint: the side of about 2.5 m farthest from the centre.
    let joint = (0..4).map(|i| (cap[i], cap[(i + 1) % 4])).filter(|&(a, b)| (2.2..=2.8).contains(&len(a, b))).max_by(|x, y| d(mid(x.0, x.1)).total_cmp(&d(mid(y.0, y.1))))?;
    let outer = mid(joint.0, joint.1);

    // Out of the piece: square to the joint, away from the cap's middle.
    let cap_c = cap.iter().fold([0.0f32; 3], |c, p| [c[0] + p[0] * 0.25, c[1] + p[1] * 0.25, c[2] + p[2] * 0.25]);
    let (ex, ez) = (joint.1[0] - joint.0[0], joint.1[2] - joint.0[2]);
    let n = ex.hypot(ez).max(1e-6);
    let mut out = [ez / n, -ex / n];
    if (outer[0] - cap_c[0]) * out[0] + (outer[2] - cap_c[2]) * out[1] < 0.0 {
        out = [-out[0], -out[1]];
    }

    // The outer face: the side of the wall-walk the cap is on, across the wall. The walk is the main piece's platform points on the joint
    // line (the walk beside the cap ends there too); without any, the body's centre.
    let cap_mid = cap.iter().fold([0.0f32; 2], |c, p| [c[0] + p[0] * 0.25, c[1] + p[2] * 0.25]);
    let across = [-out[1], out[0]];
    let along = |q: [f32; 2]| (q[0] - outer[0]) * out[0] + (q[1] - outer[2]) * out[1];
    let cross = |q: [f32; 2]| (q[0] - outer[0]) * across[0] + (q[1] - outer[2]) * across[1];
    let near: Vec<f32> = body.iter().filter(|&&q| along(q).abs() <= 0.3).map(|&q| cross(q)).collect();
    let k = if near.is_empty() {
        (cap_mid[0] - centre[0]) * across[0] + (cap_mid[1] - centre[1]) * across[1]
    } else {
        let (lo, hi) = near.iter().fold((f32::MAX, f32::MIN), |(lo, hi), &c| (lo.min(c), hi.max(c)));
        cross(cap_mid) - (lo + hi) * 0.5
    };
    let side = if k.abs() < 0.25 { [0.0, 0.0] } else if k > 0.0 { across } else { [-across[0], -across[1]] };
    Some(PieceEnd { at: outer, out, side })
}

/// If a logic file writes back to the same bytes through its serde form unchanged.
///
/// # Errors
///
/// The file doesn't read or doesn't write back.
pub fn round_trips(bytes: &[u8]) -> Result<bool, String> {
    let parsed = Cs2Parsed::decode(&mut Cursor::new(bytes), &None).map_err(|e| format!("cs2.parsed: {e}"))?;
    let v = serde_json::to_value(&parsed).map_err(|e| e.to_string())?;
    let mut back: Cs2Parsed = serde_json::from_value(v).map_err(|e| e.to_string())?;
    let mut out = vec![];
    back.encode(&mut out, &None).map_err(|e| format!("cs2.parsed: {e}"))?;
    Ok(out == bytes)
}

/// A strip of a platform: its index among the new platforms, the x range it covers before the warp, and its outline on the ground (x,
/// z) before the warp.
struct Strip {
    index: usize,
    x: (f32, f32),
    ground: Vec<[f32; 2]>,
}

/// Platforms less the boxes the steps replace (`drops`: their x ranges; `zy`: the cross-section, z then y; none: all of it), as the
/// pieces of each platform outside them (x before the range, after it, and within it the z on either side of the box). A platform wholly
/// outside every box is kept as it is. Per platform, its pieces.
fn carve_platforms(old: &[Value], drops: &[(f32, f32)], zy: Option<((f32, f32), (f32, f32))>) -> Vec<Vec<Value>> {
    let ((z0, z1), (y0, y1)) = zy.unwrap_or(((f32::MIN, f32::MAX), (f32::MIN, f32::MAX)));
    let mut out = vec![];
    for p in old {
        let mut pieces = vec![outline(&p["vertices"])];
        for &(a, b) in drops {
            let mut next = vec![];
            for poly in pieces {
                let ys = poly.iter().fold((f32::MAX, f32::MIN), |(lo, hi), q| (lo.min(q[1]), hi.max(q[1])));
                if poly.iter().all(|q| q[0] <= a) || poly.iter().all(|q| q[0] >= b) || ys.1 < y0 || ys.0 > y1 {
                    next.push(poly);
                    continue;
                }

                let mid = half_plane(&half_plane(&poly, 0, a, false), 0, b, true);
                next.extend([half_plane(&poly, 0, a, true), half_plane(&poly, 0, b, false), half_plane(&mid, 2, z0, true), half_plane(&mid, 2, z1, false)]);
            }

            pieces = next.into_iter().filter(|q| q.len() >= 3 && area_xz(q) > 1e-4).collect();
        }

        out.push(pieces.into_iter().map(|poly| {
            let mut q = p.clone();
            q["vertices"] = json!({ "outline": poly.iter().map(|&v| point3(v)).collect::<Vec<_>>() });
            q
        }).collect());
    }

    out
}

/// A polygon cut by the plane `axis` = `at`, the side below it (`below`) or above it kept (Sutherland-Hodgman).
fn half_plane(poly: &[[f32; 3]], axis: usize, at: f32, below: bool) -> Vec<[f32; 3]> {
    let inside = |q: &[f32; 3]| if below { q[axis] <= at } else { q[axis] >= at };
    let mut out = vec![];
    for i in 0..poly.len() {
        let (a, b) = (poly[i], poly[(i + 1) % poly.len()]);
        if inside(&a) {
            out.push(a);
        }

        if inside(&a) != inside(&b) {
            let t = (at - a[axis]) / (b[axis] - a[axis]);
            out.push([a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t]);
        }
    }

    out
}

/// A polygon's area on the ground (x, z).
fn area_xz(poly: &[[f32; 3]]) -> f32 {
    (0..poly.len()).map(|i| {
        let (a, b) = (poly[i], poly[(i + 1) % poly.len()]);
        a[0] * b[2] - b[0] * a[2]
    }).sum::<f32>().abs() / 2.0
}

/// Platforms (each given as its pieces) cut along x and warped, the strips whose middle `keep` refuses left out; per original platform,
/// the strips of all its pieces (indices from `offset`).
fn platform_strips(old: &[Vec<Value>], warp: &dyn Warp, cuts: &Cuts, keep: &dyn Fn([f32; 3]) -> bool, offset: usize, notes: &mut LogicNotes) -> (Vec<Value>, Vec<Vec<Strip>>) {
    let mut new = vec![];
    let mut strips = vec![];
    for group in old {
        let mut mine = vec![];
        for p in group {
            let points = outline(&p["vertices"]);
            let normal = p3(&p["normal"]);
            let planes = warp.clip_planes();
            let mut slabs = slice_along_x(&points, cuts);
            if slabs.is_empty() {
                slabs = vec![points.clone()];
            }

            let slabs = slabs.into_iter().map(|s| clip_to(&s, &planes, true)).filter(|s| s.len() >= 3).collect::<Vec<_>>();

            for s in slabs {
                let n = s.len() as f32;
                let mid = s.iter().fold([0.0; 3], |m, q| [m[0] + q[0] / n, m[1] + q[1] / n, m[2] + q[2] / n]);
                if !keep(mid) {
                    continue;
                }

                let x = s.iter().fold((f32::MAX, f32::MIN), |(lo, hi), q| (lo.min(q[0]), hi.max(q[0])));
                let w = s.iter().map(|&q| warp.point(q)).collect::<Vec<_>>();
                let mut nn = normalised(newell(&w));
                if nn[0] * normal[0] + nn[1] * normal[1] + nn[2] * normal[2] < 0.0 {
                    nn = [-nn[0], -nn[1], -nn[2]];
                }

                let mut q = p.clone();
                q["vertices"] = json!({ "outline": w.iter().map(|&v| point3(v)).collect::<Vec<_>>() });
                q["normal"] = point3(nn);
                mine.push(Strip { index: offset + new.len(), x, ground: s.iter().map(|q| [q[0], q[2]]).collect() });
                new.push(q);
            }
        }

        strips.push(mine);
    }

    notes.platforms.0 += old.len();
    notes.platforms.1 += new.len();
    (new, strips)
}

/// Firing lines cut where the platforms are (a line along the wall, the length of the piece, would be a chord of a curve: off the
/// walkway; user 2026-11-04), moved and turned, each part on the strip of its platform that is under it; those whose middle `keep`
/// refuses, or whose platform kept no strip, left out.
fn place_lines(old: &[Value], warp: &dyn Warp, cuts: &Cuts, strips: &[Vec<Strip>], keep: &dyn Fn([f32; 3]) -> bool, notes: &mut LogicNotes) -> Vec<Value> {
    let mut parts = vec![];
    for l in old {
        let (s, e) = (p3(&l["start"]), p3(&l["end"]));
        let bounds = cuts.bounds(s[0].min(e[0]), s[0].max(e[0]));
        if bounds.len() <= 2 || (e[0] - s[0]).abs() < 1e-4 {
            parts.push(l.clone());
            continue;
        }

        // In the line's own order, start to end.
        let xs: Vec<f32> = if s[0] <= e[0] { bounds } else { bounds.into_iter().rev().collect() };
        let at = |x: f32| {
            let t = (x - s[0]) / (e[0] - s[0]);
            [x, s[1] + (e[1] - s[1]) * t, s[2] + (e[2] - s[2]) * t]
        };
        for w in xs.windows(2) {
            if (w[1] - w[0]).abs() < 0.05 {
                continue;
            }

            let mut q = l.clone();
            set_p3(&mut q["start"], at(w[0]));
            set_p3(&mut q["end"], at(w[1]));
            parts.push(q);
        }
    }

    let mut out = vec![];
    for l in &parts {
        let (s, e) = (p3(&l["start"]), p3(&l["end"]));
        let mid = [(s[0] + e[0]) / 2.0, (s[1] + e[1]) / 2.0, (s[2] + e[2]) / 2.0];
        if !keep(mid) {
            continue;
        }

        let parent = l["parent_index"].as_u64().unwrap_or(0) as usize;
        let Some(mine) = strips.get(parent).filter(|m| !m.is_empty()) else {
            notes.orphan_lines += 1;
            continue;
        };

        // The strip under it (a carved platform has several over the same x: its sides of a step), else one over its x.
        let inside = mine.iter().find(|s| in_ring(&s.ground, [mid[0], mid[2]])).or_else(|| mine.iter().find(|s| s.x.0 - 1e-3 <= mid[0] && mid[0] <= s.x.1 + 1e-3));
        let strip = inside.unwrap_or_else(|| {
            notes.orphan_lines += 1;
            mine.iter().min_by(|a, b| gap(a, mid[0]).total_cmp(&gap(b, mid[0]))).expect("not empty")
        });

        let a = warp.yaw(mid);
        let mut q = l.clone();
        set_p3(&mut q["start"], warp.point(s));
        set_p3(&mut q["end"], warp.point(e));
        let dir = p3(&l["direction"]);
        let t = turn([dir[0], dir[2]], a);
        set_p3(&mut q["direction"], [t[0], dir[1], t[1]]);
        q["parent_index"] = json!(strip.index);
        out.push(q);
    }

    out
}

/// If a point (x, z) is inside a ring, its edges included.
fn in_ring(ring: &[[f32; 2]], p: [f32; 2]) -> bool {
    let mut inside = false;
    for i in 0..ring.len() {
        let (a, b) = (ring[i], ring[(i + 1) % ring.len()]);
        let (dx, dz) = (b[0] - a[0], b[1] - a[1]);
        let len2 = dx * dx + dz * dz;
        let t = if len2 > 0.0 { (((p[0] - a[0]) * dx + (p[1] - a[1]) * dz) / len2).clamp(0.0, 1.0) } else { 0.0 };
        if (a[0] + dx * t - p[0]).hypot(a[1] + dz * t - p[1]) < 1e-3 {
            return true;
        }

        if (a[1] > p[1]) != (b[1] > p[1]) && p[0] < a[0] + dx * (p[1] - a[1]) / dz {
            inside = !inside;
        }
    }

    inside
}

fn gap(s: &Strip, x: f32) -> f32 {
    (s.x.0 - x).max(x - s.x.1).max(0.0)
}

/// A 3D outline (`{"outline": [...]}`) with points along its edges, warped; closed when it has more than two points.
fn warp_outline(o: &mut Value, warp: &dyn Warp, step: f32, planes: &[[f32; 3]]) {
    let points = outline(o);
    let closed = points.len() > 2;
    let points = clip_to(&points, planes, closed);
    let out = subdivide(&points, closed, step).into_iter().map(|(p, _)| point3(warp.point(p))).collect::<Vec<_>>();
    o["outline"] = Value::Array(out);
}

/// A footprint ring (`[{"vertex": {x, y}, "vertex_type"}]`, y being z) with points along its edges, warped; an added point takes the type
/// of the point that starts its edge.
fn warp_footprint(ring: &mut Value, warp: &dyn Warp, step: f32, planes: &[[f32; 3]]) {
    let Some(old) = ring.as_array().cloned() else { return };
    let points = old.iter().map(|v| { let p = p2(&v["vertex"]); [p[0], 0.0, p[1]] }).collect::<Vec<_>>();
    let points = clip_to(&points, planes, points.len() > 2);
    let kind = old.first().cloned().unwrap_or(Value::Null);
    let out = subdivide(&points, points.len() > 2, step).into_iter().map(|(p, from)| {
        let q = warp.point(p);
        let mut v = old.get(from).cloned().unwrap_or_else(|| kind.clone());
        v["vertex"] = json!({ "x": q[0], "y": q[2] });
        v
    }).collect();
    *ring = Value::Array(out);
}

/// A box (`min_x` .. `max_z`) around the warp of the old one (its edges sampled every `step` metres along x).
fn warp_box(b: &mut Value, warp: &dyn Warp, step: f32) {
    let f = |k: &str| b[k].as_f64().unwrap_or(0.0) as f32;
    let (x0, x1, y0, y1, z0, z1) = (f("min_x"), f("max_x"), f("min_y"), f("max_y"), f("min_z"), f("max_z"));
    if x0.is_nan() || x1.is_nan() || x0 > x1 {
        return;
    }

    let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
    let parts = ((x1 - x0) / step).ceil().max(1.0) as usize;
    for k in 0..=parts {
        let x = x0 + (x1 - x0) * k as f32 / parts as f32;
        for y in [y0, y1] {
            for z in [z0, z1] {
                let q = warp.point([x, y, z]);
                for i in 0..3 {
                    lo[i] = lo[i].min(q[i]);
                    hi[i] = hi[i].max(q[i]);
                }
            }
        }
    }

    for (k, v) in [("min_x", lo[0]), ("min_y", lo[1]), ("min_z", lo[2]), ("max_x", hi[0]), ("max_y", hi[1]), ("max_z", hi[2])] {
        b[k] = json!(v);
    }
}

/// Every transform under a value (an object with `m00` .. `m33`), except the pieces' `node_transform`.
fn warp_transforms(v: &mut Value, warp: &dyn Warp) {
    match v {
        Value::Object(o) if is_transform(o) => {
            let mut m = [[0.0f32; 4]; 4];
            for (r, row) in m.iter_mut().enumerate() {
                for (c, x) in row.iter_mut().enumerate() {
                    *x = o[&format!("m{r}{c}")].as_f64().unwrap_or(0.0) as f32;
                }
            }

            transform(&mut m, warp);
            for (r, row) in m.iter().enumerate() {
                for (c, x) in row.iter().enumerate() {
                    o.insert(format!("m{r}{c}"), json!(x));
                }
            }
        }

        Value::Object(o) => {
            for (k, child) in o.iter_mut() {
                if k != "node_transform" {
                    warp_transforms(child, warp);
                }
            }
        }

        Value::Array(a) => a.iter_mut().for_each(|c| warp_transforms(c, warp)),
        _ => {}
    }
}

fn is_transform(o: &Map<String, Value>) -> bool {
    o.contains_key("m00") && o.contains_key("m33")
}

fn array_mut<'a>(v: &'a mut Value, key: &str) -> impl Iterator<Item = &'a mut Value> {
    v.get_mut(key).and_then(Value::as_array_mut).into_iter().flatten()
}

fn outline(o: &Value) -> Vec<[f32; 3]> {
    o["outline"].as_array().into_iter().flatten().map(p3).collect()
}

fn p3(v: &Value) -> [f32; 3] {
    let f = |k: &str| v[k].as_f64().unwrap_or(0.0) as f32;
    [f("x"), f("y"), f("z")]
}

fn p2(v: &Value) -> [f32; 2] {
    let f = |k: &str| v[k].as_f64().unwrap_or(0.0) as f32;
    [f("x"), f("y")]
}

fn set_p3(v: &mut Value, p: [f32; 3]) {
    *v = point3(p);
}

fn point3([x, y, z]: [f32; 3]) -> Value {
    json!({ "x": x, "y": y, "z": z })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A walkway (x 0..20, z 3..9 at y 15) under a step over x 5..10 whose cross-section is z 5.1..8.9: the step's box goes, the walkway
    /// keeps its outer and inner edges there and all of itself elsewhere; a platform under the box's heights is left whole.
    #[test]
    fn a_step_cuts_its_box_out_of_the_walkway() {
        let quad = |x0: f32, x1: f32, z0: f32, z1: f32, y: f32| json!({ "normal": { "x": 0.0, "y": 1.0, "z": 0.0 }, "vertices": { "outline": [
            { "x": x0, "y": y, "z": z0 }, { "x": x1, "y": y, "z": z0 }, { "x": x1, "y": y, "z": z1 }, { "x": x0, "y": y, "z": z1 } ] } });
        let out = carve_platforms(&[quad(0.0, 20.0, 3.0, 9.0, 15.0), quad(0.0, 20.0, 3.0, 9.0, 2.0)], &[(5.0, 10.0)], Some(((5.1, 8.9), (14.0, 26.5)))).into_iter().flatten().collect::<Vec<_>>();
        let area = |ps: &[Value]| ps.iter().map(|p| area_xz(&outline(&p["vertices"]))).sum::<f32>();
        let (walkway, low): (Vec<Value>, Vec<Value>) = out.into_iter().partition(|p| outline(&p["vertices"])[0][1] > 10.0);
        assert!((area(&walkway) - (120.0 - 5.0 * 3.8)).abs() < 1e-3, "{}", area(&walkway));
        assert!((area(&low) - 120.0).abs() < 1e-3);
        let inside_box = |q: &[f32; 3]| q[0] > 5.01 && q[0] < 9.99 && q[2] > 5.11 && q[2] < 8.89;
        assert!(!walkway.iter().any(|p| outline(&p["vertices"]).iter().any(inside_box)));
    }

    /// A step carves the walkway (platform 0) into pieces; the parapet's platform (1) and a firing line on it (parent 1) come after it:
    /// the line stays on the parapet's strip, and lines on the walkway's sides by the step on their own side's piece.
    #[test]
    fn firing_lines_keep_their_platform_when_one_before_is_carved() {
        let quad = |x0: f32, x1: f32, z0: f32, z1: f32, y: f32| json!({ "normal": { "x": 0.0, "y": 1.0, "z": 0.0 }, "vertices": { "outline": [
            { "x": x0, "y": y, "z": z0 }, { "x": x1, "y": y, "z": z0 }, { "x": x1, "y": y, "z": z1 }, { "x": x0, "y": y, "z": z1 } ] } });
        let line = |x: f32, z: f32, parent: usize| json!({ "start": { "x": x - 0.25, "y": 15.0, "z": z }, "end": { "x": x + 0.25, "y": 15.0, "z": z },
            "direction": { "x": 0.0, "y": 0.0, "z": -1.0 }, "parent_index": parent });
        let carved = carve_platforms(&[quad(0.0, 20.0, 3.0, 9.0, 15.0), quad(0.0, 20.0, 1.0, 3.0, 15.0)], &[(5.0, 10.0)], Some(((5.1, 8.9), (14.0, 26.5))));
        let mut notes = LogicNotes::default();
        let cuts = Cuts { step: 100.0, at: vec![] };
        let (platforms, strips) = platform_strips(&carved, &crate::warp::Identity, &cuts, &|_| true, 0, &mut notes);
        let lines = place_lines(&[line(15.0, 2.0, 1), line(7.0, 4.0, 0), line(7.0, 8.95, 0)], &crate::warp::Identity, &cuts, &strips, &|_| true, &mut notes);
        let under = |l: &Value| outline(&platforms[l["parent_index"].as_u64().unwrap() as usize]["vertices"]);
        let parapet = under(&lines[0]);
        assert!(parapet.iter().all(|q| q[2] <= 3.0 + 1e-3), "the parapet's line on {parapet:?}");
        let outer = under(&lines[1]);
        assert!(outer.iter().all(|q| q[2] <= 5.1 + 1e-3 && q[0] >= 5.0 - 1e-3 && q[0] <= 10.0 + 1e-3), "the walkway's line on {outer:?}");
        let inner = under(&lines[2]);
        assert!(inner.iter().all(|q| q[2] >= 8.9 - 1e-3), "the walkway's inner line on {inner:?}");
        assert_eq!(notes.orphan_lines, 0);
    }

    /// A firing line the length of the piece, along a bent wall: cut where the platform is (every 5), each part's ends on the bend (not a
    /// chord across it), in the line's order.
    #[test]
    fn a_long_firing_line_follows_the_bend() {
        let quad = json!({ "normal": { "x": 0.0, "y": 1.0, "z": 0.0 }, "vertices": { "outline": [
            { "x": -10.0, "y": 15.0, "z": 3.0 }, { "x": 10.0, "y": 15.0, "z": 3.0 }, { "x": 10.0, "y": 15.0, "z": 9.0 }, { "x": -10.0, "y": 15.0, "z": 9.0 } ] } });
        let line = json!({ "start": { "x": 9.84, "y": 15.0, "z": 8.5 }, "end": { "x": -9.84, "y": 15.0, "z": 8.5 },
            "direction": { "x": 0.0, "y": 0.0, "z": 0.38 }, "parent_index": 0 });
        let bend = crate::warp::Bend::turning(-10.0, 10.0, 0.6);
        let cuts = Cuts { step: 5.0, at: vec![] };
        let mut notes = LogicNotes::default();
        let (_, strips) = platform_strips(&[vec![quad]], &bend, &cuts, &|_| true, 0, &mut notes);
        let lines = place_lines(&[line], &bend, &cuts, &strips, &|_| true, &mut notes);
        assert_eq!(lines.len(), 4);
        let x = |l: &Value, k: &str| p3(&l[k]);
        assert_eq!(x(&lines[0], "start"), bend.point([9.84, 15.0, 8.5]));
        assert_eq!(x(&lines[3], "end"), bend.point([-9.84, 15.0, 8.5]));
        for (k, w) in [(0, 5.0), (1, 0.0), (2, -5.0)] {
            let at = bend.point([w, 15.0, 8.5]);
            let (a, b) = (x(&lines[k], "end"), x(&lines[k + 1], "start"));
            assert!((a[0] - at[0]).abs() < 1e-4 && (a[2] - at[2]).abs() < 1e-4 && a == b, "{a:?} {b:?} {at:?}");
        }

        assert_eq!(notes.orphan_lines, 0);
    }

    #[test]
    fn an_end_cap_gives_its_outer_side_and_the_way_out() {
        // roman_fort_straight's left cap (x -10..-8, z 0.5..3, 15 m up): the joint at x -10, the wall leaving along -x.
        let cap = [[-10.0, 15.0, 0.5], [-8.0, 15.0, 0.5], [-8.0, 15.0, 3.0], [-10.0, 15.0, 3.0]];
        let e = cap_end(&cap, [0.0, 5.0], &[]).expect("a cap");
        assert_eq!(e.at, [-10.0, 15.0, 1.75]);
        assert_eq!(e.out, [-1.0, 0.0]);
        assert_eq!(e.side, [0.0, -1.0], "the outer face towards -z (the cap at z 0.5..3 of the walk's 0.5..9)");

        // roman_fort_internal_corner's other cap (x 0.5..3, z 0..2), the body towards +z: the wall leaves along -z.
        let cap = [[0.5, 15.0, 0.0], [3.0, 15.0, 0.0], [3.0, 15.0, 2.0], [0.5, 15.0, 2.0]];
        let e = cap_end(&cap, [-2.0, 18.0], &[]).expect("a cap");
        assert_eq!(e.at, [1.75, 15.0, 0.0]);
        assert_eq!(e.out, [0.0, -1.0]);

        // The bastion's high cap, its inner side slanted: the joint at x 10, out along +x.
        let cap = [[8.0, 25.0, 0.5], [8.97, 25.0, 1.95], [10.0, 25.0, 3.0], [10.0, 25.0, 0.5]];
        let e = cap_end(&cap, [0.0, 4.0], &[]).expect("a cap");
        assert_eq!(e.at, [10.0, 25.0, 1.75]);
        assert_eq!(e.out, [1.0, 0.0]);

        // Not a cap: a long platform.
        // The bastion's high end: the body's centre is beyond the cap (the tower juts out), the walk by the cap is not (z 1.95..9).
        let cap = [[8.0, 25.0, 0.5], [10.0, 25.0, 0.5], [10.0, 25.0, 3.0], [8.0, 25.0, 3.0]];
        let walk = [[6.2, 1.95], [10.0, 1.95], [10.0, 9.0], [6.2, 9.0], [-5.0, -5.0], [8.0, -5.0], [8.0, 4.5]];
        assert_eq!(cap_end(&cap, [0.0, -1.0], &walk).expect("a cap").side, [0.0, -1.0]);

        assert!(cap_end(&[[-10.0, 15.0, 3.0], [10.0, 15.0, 3.0], [10.0, 15.0, 9.0], [-10.0, 15.0, 9.0]], [0.0, 5.0], &[]).is_none());
    }
}
