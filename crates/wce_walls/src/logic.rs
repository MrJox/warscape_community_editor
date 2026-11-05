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

use std::collections::HashMap;
use std::io::Cursor;

use rpfm_lib::files::cs2_parsed::Cs2Parsed;
use rpfm_lib::files::{Decodeable, Encodeable};
use serde_json::{Map, Value, json};

use crate::warp::{CornerFace, Cuts, Warp, clip_to, inside, newell, normalised, slice_along_x, subdivide, transform, turn};

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

    /// Platforms of its own instead of its logic file's (its frame): a stair's, following its treads.
    pub platforms: Option<Vec<Value>>,

    /// The base's collision hull at these heights (its frame: the walkway's) is lowered under the part, inside `replaces` and `drop_zy`'s
    /// z: the stair's platforms lie on its treads, above the hull.
    pub sink: Option<(f32, f32)>,
}

/// How far a base's collision hull is lowered under a stair (more than a tread's rise over the walkway's slope), and how far in from
/// the step's ends it starts going down.
const SINK: f32 = 0.35;
const SINK_IN: f32 = 0.3;

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
    warp_logic_framed(bytes, warp, cuts, drop_zy, splices, None)
}

/// [`warp_logic_with`], the base's platforms cut to its frame's x (`frame`): a piece's walkway ends on its joints exactly (some bases'
/// run a little past theirs, and the next piece's edges were 4 or 5 cm off: not one edge for units to cross; user 2026-11-05).
///
/// # Errors
///
/// The file doesn't read, or doesn't write back.
pub fn warp_logic_framed(bytes: &[u8], warp: &dyn Warp, cuts: &Cuts, drop_zy: Option<((f32, f32), (f32, f32))>, splices: &[Splice], frame: Option<(f32, f32)>) -> Result<(Vec<u8>, LogicNotes), String> {
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
            let mut old = carve_platforms(d["platforms"].as_array().map_or(&[][..], Vec::as_slice), &drop, drop_zy);
            if let Some((x0, x1)) = frame {
                for group in &mut old {
                    for p in group.iter_mut() {
                        let o = half_plane(&half_plane(&outline(&p["vertices"]), 0, x0, false), 0, x1, true);
                        p["vertices"] = json!({ "outline": o.into_iter().map(point3).collect::<Vec<_>>() });
                    }

                    group.retain(|p| outline(&p["vertices"]).len() >= 3);
                }
            }
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
                let theirs = sp.platforms.as_ref().map_or(theirs, |ps| ps.iter().map(|p| vec![p.clone()]).collect());
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

            // The collision hull bent with the piece (and the corner's copies' hulls with them): it stayed the base's, straight and
            // flat over steps and half a piece off (user 2026-11-04: the stairs showed as not walkable, the hull over their platform).
            let sinks = splices.iter().filter_map(|sp| Some((sp.replaces?, sp.sink?))).collect::<Vec<_>>();
            // `bin_data` groups the hull's triangles by their indices (the straight wall's 7 groups cover its 104 triangles once each;
            // a piece's single group, all of them): each new triangle goes into the groups of the triangle it was cut from (user
            // 2026-11-05: the remade hull kept the base's groups, 440 of 544 triangles in none, the 104 named others).
            let groups_of = |bins: &Value| {
                let mut of: HashMap<usize, Vec<usize>> = HashMap::new();
                for (g, list) in bins.as_array().into_iter().flatten().enumerate() {
                    for t in list.as_array().into_iter().flatten().filter_map(Value::as_u64) {
                        of.entry(t as usize).or_default().push(g);
                    }
                }

                of
            };
            let group_count = |bins: &Value| bins.as_array().map_or(0, Vec::len);
            let mut groups = group_count(&d["bin_data"]);
            let base_of = groups_of(&d["bin_data"]);
            let (mut hull, mut hull_groups) = (vec![], vec![]);
            for (t, poly) in collision_polygons(&d["collision_3d"], warp, cuts, &planes, &sinks, drop_zy.map(|(z, _)| z)) {
                hull_groups.push(base_of.get(&t).cloned().unwrap_or_default());
                hull.push(poly);
            }

            for (sp, sd) in &shapes {
                let of = groups_of(&sd["bin_data"]);
                groups = groups.max(group_count(&sd["bin_data"]));
                for (t, poly) in collision_polygons(&sd["collision_3d"], sp.warp, cuts, &sp.warp.clip_planes(), &[], None) {
                    hull_groups.push(of.get(&t).cloned().unwrap_or_default());
                    hull.push(poly);
                }
            }

            if d["collision_3d"].is_object() {
                let from = write_collision(&mut d["collision_3d"], &hull);
                if groups > 0 {
                    let mut bins = vec![vec![]; groups];
                    for (t, &p) in from.iter().enumerate() {
                        for &g in &hull_groups[p] {
                            bins[g].push(t);
                        }
                    }

                    d["bin_data"] = json!(bins);
                }
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

            if d["gates"].as_array().is_some_and(|b| !b.is_empty()) {
                notes.warnings.push(format!("{name}: gates kept as they are"));
            }

            warp_box(&mut d["bounding_box"], warp, step);

            // The level's box holds all of it (the game's is its hull's extent): a corner's copies, merged in from a length away, were
            // outside it by up to a whole piece.
            let mut pts = d["collision_3d"]["vertices"].as_array().map(|a| a.iter().map(p3).collect::<Vec<_>>()).unwrap_or_default();
            pts.extend(d["platforms"].as_array().into_iter().flatten().flat_map(|p| outline(&p["vertices"])));
            grow_box(&mut d["bounding_box"], &pts);
        }

        piece_index += 1;
    }

    // Each level's platforms, of all the pieces, welded together (an end cap's platforms are a piece of their own).
    let levels = v["pieces"].as_array().into_iter().flatten().map(|p| p["destructs"].as_array().map_or(0, Vec::len)).max().unwrap_or(0);
    for k in 0..levels {
        let mut all = vec![];
        let mut counts = vec![];
        for piece in array_mut(&mut v, "pieces") {
            let ps = piece["destructs"].get(k).and_then(|d| d["platforms"].as_array()).cloned().unwrap_or_default();
            counts.push(ps.len());
            all.extend(ps);
        }

        weld_platforms(&mut all);
        let mut rest = all.into_iter();
        for (piece, n) in array_mut(&mut v, "pieces").zip(counts) {
            let mine = rest.by_ref().take(n).collect::<Vec<_>>();
            if let Some(d) = piece["destructs"].get_mut(k) {
                d["platforms"] = Value::Array(mine);
                drop_folded(d);
                for p in array_mut(d, "platforms") {
                    wind_as_the_game(p);
                }
            }
        }
    }

    warp_box(&mut v["bounding_box"], warp, step);
    let corners = v["pieces"].as_array().into_iter().flatten().flat_map(|p| p["destructs"].as_array().into_iter().flatten()).flat_map(|d| {
        let b = &d["bounding_box"];
        let f = |k: &str| b[k].as_f64().unwrap_or(0.0) as f32;
        [[f("min_x"), f("min_y"), f("min_z")], [f("max_x"), f("max_y"), f("max_z")]]
    }).collect::<Vec<_>>();
    grow_box(&mut v["bounding_box"], &corners);
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

            let faces = warp.corner_faces();
        let slabs = slabs.into_iter().map(|s| clip_to(&s, &planes, true)).filter(|s| s.len() >= 3).collect::<Vec<_>>();

            for s in slabs {
                let n = s.len() as f32;
                let mid = s.iter().fold([0.0; 3], |m, q| [m[0] + q[0] / n, m[1] + q[1] / n, m[2] + q[2] / n]);
                if !keep(mid) {
                    continue;
                }

                let x = s.iter().fold((f32::MAX, f32::MIN), |(lo, hi), q| (lo.min(q[0]), hi.max(q[0])));
                let w = on_face_grid(&s, &s.iter().map(|&q| warp.point(q)).collect::<Vec<_>>(), &faces);
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

/// The spacing of a corner face's points along it, from the corner (`on_face_grid`).
const FACE_GRID: f32 = 0.5;

/// A platform's warped points (`w`, from the source's `s`) with those on a corner's faces on one grid along the face, measured from the
/// corner: each slid along the face to the nearest multiple of `FACE_GRID`, and a point at every multiple along an edge that runs on
/// the face. Both pieces of a corner, whose faces are the same line, then have the same points on it, and their platforms share edges
/// across it (user 2026-11-05: the base's slanted platform edges crossed each face at points of its own, and a grid of depths across
/// each arm drifted apart where the arms meet the bisector at different angles; the corner's two sides shared no edge).
fn on_face_grid(s: &[[f32; 3]], w: &[[f32; 3]], faces: &[CornerFace]) -> Vec<[f32; 3]> {
    let mut src = s.to_vec();
    let mut out = w.to_vec();
    for f in faces {
        let [a, b, c] = f.plane;
        let on = |p: [f32; 3]| (a * p[0] + b * p[2] - c).abs() < 1e-3;
        let along = |q: [f32; 3]| (q[0] - f.at[0]) * f.dir[0] + (q[2] - f.at[1]) * f.dir[1];
        let snap = |t: f32| (t / FACE_GRID).round() * FACE_GRID;
        let put = |t: f32, y: f32| [f.at[0] + f.dir[0] * t, y, f.at[1] + f.dir[1] * t];
        let (mut ns, mut nw) = (vec![], vec![]);
        for i in 0..out.len() {
            let j = (i + 1) % out.len();
            if !on(src[i]) {
                ns.push(src[i]);
                nw.push(out[i]);
                continue;
            }

            let (tp, tq) = (snap(along(out[i])), snap(along(out[j])));
            ns.push(src[i]);
            nw.push(put(tp, out[i][1]));
            if on(src[j]) {
                let n = ((tq - tp) / FACE_GRID).round() as i32;
                for k in 1..n.abs() {
                    let t = tp + FACE_GRID * (k * n.signum()) as f32;
                    let u = ((t - tp) / (tq - tp)).clamp(0.0, 1.0);
                    ns.push(src[i]);
                    nw.push(put(t, out[i][1] + (out[j][1] - out[i][1]) * u));
                }
            }
        }

        // Points made one by the snap.
        let mut keep_s = vec![];
        let mut keep_w: Vec<[f32; 3]> = vec![];
        for (p, q) in ns.into_iter().zip(nw) {
            if keep_w.last() != Some(&q) {
                keep_s.push(p);
                keep_w.push(q);
            }
        }

        if keep_w.len() > 1 && keep_w.first() == keep_w.last() {
            keep_w.pop();
            keep_s.pop();
        }

        src = keep_s;
        out = keep_w;
    }

    out
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

    // Each part cut to the piece's faces (a corner's mitre, an attached end): a part kept by its middle reached past the bisector over
    // the other side's walkway, and units sent there stuck at the corner's inner side (user 2026-11-05).
    let planes = warp.clip_planes();
    let parts = parts.into_iter().filter_map(|mut l| {
        let (mut s, mut e) = (p3(&l["start"]), p3(&l["end"]));
        for &[a, b, c] in &planes {
            let (fs, fe) = (a * s[0] + b * s[2] - c, a * e[0] + b * e[2] - c);
            if fs > 1e-4 && fe > 1e-4 {
                return None;
            }

            if fs > 1e-4 || fe > 1e-4 {
                let t = fs / (fs - fe);
                let m = [s[0] + (e[0] - s[0]) * t, s[1] + (e[1] - s[1]) * t, s[2] + (e[2] - s[2]) * t];
                if fs > 1e-4 { s = m } else { e = m }
            }
        }

        if (e[0] - s[0]).hypot(e[2] - s[2]) < 0.05 {
            return None;
        }

        set_p3(&mut l["start"], s);
        set_p3(&mut l["end"], e);
        Some(l)
    }).collect::<Vec<_>>();

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

/// How near two platform corners are one, and a corner is on another platform's edge.
const WELD: f32 = 0.01;

/// A level's platforms joined as the game's are, edge to edge (the game connects platforms on shared edges: the stepped bastion's stair
/// shares its top edge's two corners with the landing; ours met the walkway's pieces partway along their edges, and units did not go
/// from one to the other; user 2026-11-05): corners within `WELD` of each other become one point, and a corner on another platform's
/// edge becomes a corner of that edge too. Their order, their count and everything else is kept.
fn weld_platforms(platforms: &mut [Value]) {
    let mut outlines = platforms.iter().map(|p| outline(&p["vertices"])).collect::<Vec<_>>();

    // One point for corners that are one (the first met stands for them).
    let mut points: Vec<[f32; 3]> = vec![];
    for o in &mut outlines {
        for q in o.iter_mut() {
            match points.iter().find(|p| (0..3).all(|i| (p[i] - q[i]).abs() <= WELD)) {
                Some(p) => *q = *p,
                None => points.push(*q),
            }
        }
    }

    // Every corner lying on an edge (not at its ends) goes into it, in order along it.
    for o in &mut outlines {
        let others = points.iter().copied().filter(|p| !o.contains(p)).collect::<Vec<_>>();
        let mut out = vec![];
        for i in 0..o.len() {
            let (a, b) = (o[i], o[(i + 1) % o.len()]);
            out.push(a);
            let d = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
            let len2 = d[0] * d[0] + d[1] * d[1] + d[2] * d[2];
            if len2 < 1e-8 {
                continue;
            }

            let mut on = others.iter().filter_map(|&p| {
                let t = ((p[0] - a[0]) * d[0] + (p[1] - a[1]) * d[1] + (p[2] - a[2]) * d[2]) / len2;
                let off = [a[0] + d[0] * t - p[0], a[1] + d[1] * t - p[1], a[2] + d[2] * t - p[2]];
                let dist = (off[0] * off[0] + off[1] * off[1] + off[2] * off[2]).sqrt();
                (t > 1e-4 && t < 1.0 - 1e-4 && dist <= WELD).then_some((t, p))
            }).collect::<Vec<_>>();
            on.sort_by(|x, y| x.0.total_cmp(&y.0));
            out.extend(on.into_iter().map(|(_, p)| p));
        }

        // A snap may have made two corners in a row one.
        out.dedup();
        if out.len() > 1 && out.first() == out.last() {
            out.pop();
        }

        *o = out;
    }

    // A sliver the snap folded into a line stays so (`drop_folded` takes it out): its neighbours' corners are one now, their edges shared.
    for (p, o) in platforms.iter_mut().zip(outlines) {
        p["vertices"] = json!({ "outline": o.into_iter().map(point3).collect::<Vec<_>>() });
    }
}

/// A platform's corners in the order the game's go round (every platform of the game's pieces, its stairs too: clockwise seen from
/// above, a negative area over x and z). Two neighbours then run their shared edge opposite ways; the stairs, made the other way round,
/// were the only platforms that did not, and units never stepped onto them (user 2026-11-05).
fn wind_as_the_game(p: &mut Value) {
    let o = outline(&p["vertices"]);
    let area = (0..o.len()).map(|i| {
        let (a, b) = (o[(i + o.len() - 1) % o.len()], o[i]);
        a[0] * b[2] - b[0] * a[2]
    }).sum::<f32>();
    if area > 0.0 {
        p["vertices"] = json!({ "outline": o.into_iter().rev().map(point3).collect::<Vec<_>>() });
    }
}

/// A level's platforms the weld folded (a sliver: fewer than 3 corners, or no area) taken out, its firing lines' platforms renumbered
/// (a line on a folded one goes to the platform under its middle, else the nearest). Pipes name no platforms (a polyline and its type).
fn drop_folded(d: &mut Value) {
    let Some(platforms) = d["platforms"].as_array().cloned() else { return };
    let outlines = platforms.iter().map(|p| outline(&p["vertices"])).collect::<Vec<_>>();
    let folded = |o: &[[f32; 3]]| o.len() < 3 || area_xz(o) < 1e-4;
    if !outlines.iter().any(|o| folded(o)) {
        return;
    }

    let mut new_index = vec![None; outlines.len()];
    let mut kept = vec![];
    for (i, (p, o)) in platforms.into_iter().zip(&outlines).enumerate() {
        if !folded(o) {
            new_index[i] = Some(kept.len());
            kept.push(p);
        }
    }

    let kept_outlines = kept.iter().map(|p| outline(&p["vertices"])).collect::<Vec<_>>();
    let under = |m: [f32; 3]| {
        let ring = |o: &[[f32; 3]]| o.iter().map(|q| [q[0], q[2]]).collect::<Vec<_>>();
        kept_outlines.iter().position(|o| in_ring(&ring(o), [m[0], m[2]])).or_else(|| {
            (0..kept_outlines.len()).min_by(|&a, &b| {
                let d = |o: &[[f32; 3]]| o.iter().map(|q| (q[0] - m[0]).hypot(q[2] - m[2])).fold(f32::MAX, f32::min);
                d(&kept_outlines[a]).total_cmp(&d(&kept_outlines[b]))
            })
        })
    };

    for l in array_mut(d, "ef_lines") {
        let old = l["parent_index"].as_u64().unwrap_or(0) as usize;
        let (s, e) = (p3(&l["start"]), p3(&l["end"]));
        let mid = [(s[0] + e[0]) / 2.0, (s[1] + e[1]) / 2.0, (s[2] + e[2]) / 2.0];
        if let Some(n) = new_index.get(old).copied().flatten().or_else(|| under(mid)) {
            l["parent_index"] = json!(n);
        }
    }

    d["platforms"] = Value::Array(kept);
}

/// A collision hull (`collision_3d`: vertices, triangles) as polygons cut along x at `cuts`, cut to the half-planes, warped. A cut through
/// an edge gives the same point to both triangles on it (worked out from the edge's ends in one order), so the hull stays closed. Under
/// each `sinks` stair (its x range, the heights lowered) inside `sink_z`, the hull goes down by `SINK` from `SINK_IN` in from its ends.
fn collision_polygons(c: &Value, warp: &dyn Warp, cuts: &Cuts, planes: &[[f32; 3]], sinks: &[((f32, f32), (f32, f32))], sink_z: Option<(f32, f32)>) -> Vec<(usize, Vec<[f32; 3]>)> {
    let verts = c["vertices"].as_array().map(|a| a.iter().map(p3).collect::<Vec<_>>()).unwrap_or_default();
    let tris = c["triangles"].as_array().map(|a| a.iter().filter_map(|t| {
        let i = |k: &str| t[k].as_u64().map(|v| v as usize).filter(|&v| v < verts.len());
        Some([verts[i("vertex_1")?], verts[i("vertex_2")?], verts[i("vertex_3")?]])
    }).collect::<Vec<_>>()).unwrap_or_default();
    if tris.is_empty() {
        return vec![];
    }

    // The side of a point to a half-plane (a x + b z <= c inside), and a polygon's part on one side.
    let cut = |poly: &[[f32; 3]], f: &dyn Fn([f32; 3]) -> f32| -> Vec<[f32; 3]> {
        let mut out = vec![];
        for i in 0..poly.len() {
            let (p, q) = (poly[i], poly[(i + 1) % poly.len()]);
            let (fp, fq) = (f(p), f(q));
            if fp <= 0.0 {
                out.push(p);
            }

            if (fp <= 0.0) != (fq <= 0.0) {
                // From the edge's ends in a fixed order: the same point for the triangle on the other side.
                let (a, b, fa, fb) = if (p[0], p[1], p[2]) < (q[0], q[1], q[2]) { (p, q, fp, fq) } else { (q, p, fq, fp) };
                let t = fa / (fa - fb);
                out.push([a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t]);
            }
        }

        out
    };

    let (lo, hi) = tris.iter().flatten().fold((f32::MAX, f32::MIN), |(lo, hi), p| (lo.min(p[0]), hi.max(p[0])));
    let mut extra = cuts.at.clone();
    for &((a, b), _) in sinks {
        extra.extend([a, b]);
        if b - a > 2.0 * SINK_IN {
            extra.extend([a + SINK_IN, b - SINK_IN]);
        }
    }

    let bounds = Cuts { step: cuts.step, at: extra }.bounds(lo, hi);
    // Each polygon with the triangle it was cut from.
    let mut polys: Vec<(usize, Vec<[f32; 3]>)> = tris.iter().enumerate().map(|(i, t)| (i, t.to_vec())).collect();
    for &x in &bounds[1..bounds.len().saturating_sub(1)] {
        polys = polys.into_iter().flat_map(|(i, p)| [(i, cut(&p, &|q| q[0] - x)), (i, cut(&p, &|q| x - q[0]))]).filter(|p| p.1.len() >= 3).collect();
    }

    for &[a, b, c] in planes {
        polys = polys.into_iter().map(|(i, p)| (i, cut(&p, &|q| a * q[0] + b * q[2] - c))).filter(|p| p.1.len() >= 3).collect();
    }

    // Closed again where the planes cut it, as the game's hulls are (an open one at a corner's seam let soldiers drop through: user
    // 2026-11-05).
    let caps = hull_caps(&polys, planes);
    polys.extend(caps);

    let sunk = |q: [f32; 3]| {
        let (z0, z1) = sink_z.unwrap_or((f32::MIN, f32::MAX));
        let under = sinks.iter().any(|&((a, b), (y0, y1))| q[0] > a + SINK_IN - 1e-3 && q[0] < b - SINK_IN + 1e-3 && b - a > 2.0 * SINK_IN && q[1] >= y0 && q[1] <= y1);
        if under && q[2] >= z0 && q[2] <= z1 { [q[0], q[1] - SINK, q[2]] } else { q }
    };

    polys.into_iter().map(|(i, p)| (i, p.into_iter().map(|q| warp.point(sunk(q))).collect())).collect()
}

/// The caps that close a hull's polygons where `planes` cut them: the edges only one polygon has, lying on a plane, chained into loops,
/// each filled (ear clipping in the plane) facing out of the kept side (`a x + b z <= c`). Each cap takes the source triangle of the
/// polygon at its first edge.
fn hull_caps(polys: &[(usize, Vec<[f32; 3]>)], planes: &[[f32; 3]]) -> Vec<(usize, Vec<[f32; 3]>)> {
    let key = |p: [f32; 3]| p.map(|v| (v * 1e4).round() as i64);
    let mut uses: HashMap<([i64; 3], [i64; 3]), usize> = HashMap::new();
    for (_, p) in polys {
        for i in 0..p.len() {
            let (u, v) = (key(p[i]), key(p[(i + 1) % p.len()]));
            *uses.entry(if u < v { (u, v) } else { (v, u) }).or_default() += 1;
        }
    }

    let mut out = vec![];
    for &[a, b, c] in planes {
        let on = |p: [f32; 3]| (a * p[0] + b * p[2] - c).abs() < 1e-3;
        // The open edges on this plane, with the source of their polygon.
        let mut open: Vec<([f32; 3], [f32; 3], usize)> = vec![];
        for (src, p) in polys {
            for i in 0..p.len() {
                let (u, v) = (p[i], p[(i + 1) % p.len()]);
                let (ku, kv) = (key(u), key(v));
                if ku != kv && on(u) && on(v) && uses.get(&if ku < kv { (ku, kv) } else { (kv, ku) }) == Some(&1) {
                    open.push((u, v, *src));
                }
            }
        }

        // Chained into loops, each edge once (either way round).
        let mut used = vec![false; open.len()];
        for start in 0..open.len() {
            if used[start] {
                continue;
            }

            used[start] = true;
            let (first, mut at, src) = (open[start].0, open[start].1, open[start].2);
            let mut lp = vec![first];
            while key(at) != key(first) {
                lp.push(at);
                let Some(n) = (0..open.len()).find(|&j| !used[j] && (key(open[j].0) == key(at) || key(open[j].1) == key(at))) else { break };
                used[n] = true;
                at = if key(open[n].0) == key(at) { open[n].1 } else { open[n].0 };
            }

            if lp.len() < 3 || key(at) != key(first) {
                continue;
            }

            // In the plane: along it (horizontally) and up. Facing out: the normal along (a, 0, b).
            let len = a.hypot(b).max(1e-9);
            let t = [-b / len, a / len];
            // Facing out (its normal, (v2 - v1) x (v3 - v1) as the game's hull triangles, along (a, 0, b)): the loop's own normal says
            // which way round it goes.
            let n = newell(&lp);
            let lp = if n[0] * a + n[2] * b < 0.0 { lp.into_iter().rev().collect::<Vec<_>>() } else { lp };
            let flat = lp.iter().map(|p| [p[0] * t[0] + p[2] * t[1], p[1]]).collect::<Vec<_>>();
            // The loop's points in a straight line with their neighbours (where the cut crossed a face's diagonal) are left out of the
            // ears and put back into the triangle edge they lie on: the cap then meets the hull at every point, closed.
            let mut tris = ear_clip_indices(&flat);
            let on_seg = |p: [f32; 2], a: [f32; 2], b: [f32; 2]| {
                let (d, e) = ([b[0] - a[0], b[1] - a[1]], [p[0] - a[0], p[1] - a[1]]);
                let len2 = d[0] * d[0] + d[1] * d[1];
                let t = (e[0] * d[0] + e[1] * d[1]) / len2.max(1e-12);
                t > 1e-4 && t < 1.0 - 1e-4 && (e[0] - d[0] * t).hypot(e[1] - d[1] * t) < 1e-4
            };
            for p in 0..flat.len() {
                if tris.iter().any(|t| t.contains(&p)) {
                    continue;
                }

                let Some((ti, e)) = tris.iter().enumerate().find_map(|(ti, t)| (0..3).find(|&e| on_seg(flat[p], flat[t[e]], flat[t[(e + 1) % 3]])).map(|e| (ti, e))) else { continue };
                let t = tris.remove(ti);
                let (u, v, w) = (t[e], t[(e + 1) % 3], t[(e + 2) % 3]);
                tris.push([u, p, w]);
                tris.push([p, v, w]);
            }

            for [i, j, k] in tris {
                out.push((src, vec![lp[i], lp[j], lp[k]]));
            }
        }
    }

    out
}

/// A simple polygon (2D, either way round) cut into triangles (ear clipping): indices into it, in its order of turning.
fn ear_clip_indices(points: &[[f32; 2]]) -> Vec<[usize; 3]> {
    let cross = |a: [f32; 2], b: [f32; 2], c: [f32; 2]| (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0]);
    let mut idx = (0..points.len()).collect::<Vec<_>>();
    let area = (0..points.len()).map(|i| { let (p, q) = (points[i], points[(i + 1) % points.len()]); p[0] * q[1] - q[0] * p[1] }).sum::<f32>();
    let sign = if area >= 0.0 { 1.0 } else { -1.0 };
    let mut out = vec![];
    let (mut k, mut stuck) = (0, 0);
    while idx.len() > 3 && stuck < idx.len() {
        let n = idx.len();
        let (ia, ib, ic) = (idx[(k + n - 1) % n], idx[k % n], idx[(k + 1) % n]);
        let (a, b, c) = (points[ia], points[ib], points[ic]);
        let turn = cross(a, b, c) * sign;
        if turn.abs() < 1e-9 {
            idx.remove(k % n);
            stuck = 0;
            k %= idx.len().max(1);
            continue;
        }

        let empty = turn > 0.0 && idx.iter().all(|&j| {
            let p = points[j];
            j == ia || j == ib || j == ic || p == a || p == b || p == c || !(cross(a, b, p) * sign >= 0.0 && cross(b, c, p) * sign >= 0.0 && cross(c, a, p) * sign >= 0.0)
        });
        if empty {
            out.push([ia, ib, ic]);
            idx.remove(k % n);
            stuck = 0;
        } else {
            k += 1;
            stuck += 1;
        }

        k %= idx.len().max(1);
    }

    if idx.len() == 3 {
        out.push([idx[0], idx[1], idx[2]]);
    }

    out
}

/// How near two of a hull's points are one.
const HULL_WELD: f32 = 0.006;

/// A collision hull written from polygons (triangle fans; degenerate triangles left out): its vertices shared (within `HULL_WELD`), every triangle's edges and the triangles across them (-1 on an open edge), as the game's files have them. Per triangle
/// written, the polygon it is part of.
fn write_collision(c: &mut Value, polys: &[Vec<[f32; 3]>]) -> Vec<usize> {
    // One point for points within `HULL_WELD` (a cut along a hull's own end left cracks of a few millimetres): the first met stands for
    // them, found through the cells around.
    let cell = |p: [f32; 3]| p.map(|v| (v / (2.0 * HULL_WELD)).floor() as i64);
    let mut index: HashMap<[i64; 3], Vec<usize>> = HashMap::new();
    let mut verts: Vec<[f32; 3]> = vec![];
    let mut id = |p: [f32; 3]| {
        let c = cell(p);
        for dx in -1..=1 {
            for dy in -1..=1 {
                for dz in -1..=1 {
                    if let Some(&found) = index.get(&[c[0] + dx, c[1] + dy, c[2] + dz]).and_then(|l| l.iter().find(|&&i| (0..3).all(|k| (verts[i][k] - p[k]).abs() <= HULL_WELD))) {
                        return found;
                    }
                }
            }
        }

        verts.push(p);
        index.entry(c).or_default().push(verts.len() - 1);
        verts.len() - 1
    };

    let (mut tris, mut from): (Vec<[usize; 3]>, Vec<usize>) = (vec![], vec![]);
    for (k_poly, poly) in polys.iter().enumerate() {
        let ids = poly.iter().map(|&p| id(p)).collect::<Vec<_>>();
        for k in 1..ids.len().saturating_sub(1) {
            let t = [ids[0], ids[k], ids[k + 1]];
            if t[0] != t[1] && t[1] != t[2] && t[0] != t[2] {
                tris.push(t);
                from.push(k_poly);
            }
        }
    }

    let mut edges: HashMap<(usize, usize), Vec<usize>> = HashMap::new();
    for (f, t) in tris.iter().enumerate() {
        for k in 0..3 {
            let (a, b) = (t[k], t[(k + 1) % 3]);
            edges.entry((a.min(b), a.max(b))).or_default().push(f);
        }
    }

    let triangles = tris.iter().enumerate().map(|(f, t)| {
        let mut o = json!({ "face_index": f, "padding": 0, "vertex_1": t[0], "vertex_2": t[1], "vertex_3": t[2], "zero_4": 0 });
        for k in 0..3 {
            let (a, b) = (t[k], t[(k + 1) % 3]);
            let across = edges.get(&(a.min(b), a.max(b))).and_then(|fs| fs.iter().find(|&&g| g != f)).map_or(-1, |&g| g as i64);
            o[format!("edge_{}_vertex_1", k + 1)] = json!(a);
            o[format!("edge_{}_vertex_2", k + 1)] = json!(b);
            o[format!("face_index_{}", k + 1)] = json!(f);
            o[format!("zero_{}", k + 1)] = json!(k);
            o[format!("across_face_index_{}", k + 1)] = json!(across);
        }

        o
    }).collect::<Vec<_>>();

    // The box: kept empty when the game's is (a hull's box is all zero in the files seen), else around the hull.
    let empty = ["min_x", "min_y", "min_z", "max_x", "max_y", "max_z"].iter().all(|k| c["bounding_box"][*k].as_f64().unwrap_or(0.0) == 0.0);
    if !empty && !verts.is_empty() {
        let (lo, hi) = verts.iter().fold(([f32::MAX; 3], [f32::MIN; 3]), |(lo, hi), p| ([lo[0].min(p[0]), lo[1].min(p[1]), lo[2].min(p[2])], [hi[0].max(p[0]), hi[1].max(p[1]), hi[2].max(p[2])]));
        c["bounding_box"] = json!({ "min_x": lo[0], "min_y": lo[1], "min_z": lo[2], "max_x": hi[0], "max_y": hi[1], "max_z": hi[2] });
    }

    c["vertices"] = Value::Array(verts.into_iter().map(point3).collect());
    c["triangles"] = Value::Array(triangles);
    from
}

/// A box (`min_x` .. `max_z`) grown to hold these points.
fn grow_box(b: &mut Value, pts: &[[f32; 3]]) {
    for (i, (lo, hi)) in [("min_x", "max_x"), ("min_y", "max_y"), ("min_z", "max_z")].into_iter().enumerate() {
        let (a, z) = pts.iter().fold((b[lo].as_f64().unwrap_or(0.0) as f32, b[hi].as_f64().unwrap_or(0.0) as f32), |(a, z), p| (a.min(p[i]), z.max(p[i])));
        b[lo] = json!(a);
        b[hi] = json!(z);
    }
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

    /// A wall's collision hull (a closed box) bent with the wall: cut where the platforms are, closed still (every edge between two
    /// triangles, each naming the other), its points where the bend puts them (the middle of its top on the bend, not on a chord).
    #[test]
    fn a_collision_hull_is_bent_and_stays_closed() {
        let corners = [[-10.0, 0.0, 0.0], [10.0, 0.0, 0.0], [10.0, 0.0, 9.0], [-10.0, 0.0, 9.0], [-10.0, 15.0, 0.0], [10.0, 15.0, 0.0], [10.0, 15.0, 9.0], [-10.0, 15.0, 9.0]];
        let faces = [[0, 2, 1], [0, 3, 2], [4, 5, 6], [4, 6, 7], [0, 1, 5], [0, 5, 4], [1, 2, 6], [1, 6, 5], [2, 3, 7], [2, 7, 6], [3, 0, 4], [3, 4, 7]];
        let mut hull = json!({
            "name": "piece01_destruct01_collision3d", "uk_1": 1, "uk_2": 0, "zero_4": 0,
            "bounding_box": { "min_x": 0.0, "min_y": 0.0, "min_z": 0.0, "max_x": 0.0, "max_y": 0.0, "max_z": 0.0 },
            "vertices": corners.iter().map(|&p| point3(p)).collect::<Vec<_>>(),
            "triangles": faces.iter().map(|f| json!({ "vertex_1": f[0], "vertex_2": f[1], "vertex_3": f[2] })).collect::<Vec<_>>(),
        });
        let bend = crate::warp::Bend::turning(-10.0, 10.0, 0.6);
        let flat = hull.clone();
        let polys = collision_polygons(&hull, &bend, &Cuts { step: 5.0, at: vec![] }, &[], &[], None).into_iter().map(|p| p.1).collect::<Vec<_>>();
        write_collision(&mut hull, &polys);

        let tris = hull["triangles"].as_array().unwrap();
        assert!(tris.len() >= 12 * 3, "{} triangles", tris.len());
        for (f, t) in tris.iter().enumerate() {
            for k in 1..=3 {
                let across = t[format!("across_face_index_{k}")].as_i64().unwrap();
                assert!(across >= 0, "triangle {f} edge {k} open");
                let o = &tris[across as usize];
                let (a, b) = (t[format!("edge_{k}_vertex_1")].as_u64().unwrap(), t[format!("edge_{k}_vertex_2")].as_u64().unwrap());
                let theirs = [o["vertex_1"].as_u64().unwrap(), o["vertex_2"].as_u64().unwrap(), o["vertex_3"].as_u64().unwrap()];
                assert!(theirs.contains(&a) && theirs.contains(&b), "triangle {f} edge {k}: {across} does not share it");
            }
        }

        let verts = hull["vertices"].as_array().unwrap().iter().map(p3).collect::<Vec<_>>();
        for z in [0.0, 9.0] {
            let want = bend.point([0.0, 15.0, z]);
            assert!(verts.iter().any(|v| (0..3).all(|i| (v[i] - want[i]).abs() < 1e-3)), "no point at the top's middle, z {z}: {want:?}");
        }

        assert_eq!(hull["bounding_box"]["max_x"].as_f64(), Some(0.0), "an empty box stays empty");

        // Under a stair over x 2..6 the top goes down, from SINK_IN in from its ends; the bottom and the top elsewhere stay (the sides'
        // points at the cuts are in between).
        let polys = collision_polygons(&flat, &crate::warp::Identity, &Cuts { step: 5.0, at: vec![] }, &[], &[((2.0, 6.0), (14.5, 15.5))], Some((0.0, 9.0)));
        let pts = polys.iter().flat_map(|p| p.1.iter().copied()).collect::<Vec<_>>();
        assert!(pts.iter().any(|p| (p[0] - 4.0).abs() < 1.8 && (p[1] - (15.0 - SINK)).abs() < 1e-4), "nothing sunk");
        let wrong = pts.iter().filter(|p| p[1] > 14.0).filter(|p| !(if p[0] > 2.0 + SINK_IN - 1e-3 && p[0] < 6.0 - SINK_IN + 1e-3 { (p[1] - (15.0 - SINK)).abs() < 1e-4 } else { (p[1] - 15.0).abs() < 1e-4 })).collect::<Vec<_>>();
        assert!(wrong.is_empty(), "{wrong:?}");
        assert!(pts.iter().filter(|p| p[1] < 1.0).all(|p| p[1] == 0.0));
    }

    /// Platforms meet edge to edge: a corner of one on the other's edge becomes a corner there too, corners a few millimetres apart
    /// become one (a stair's end against the walkway's pieces).
    #[test]
    fn platforms_are_welded_edge_to_edge() {
        let quad = |pts: [[f32; 3]; 4]| json!({ "normal": { "x": 0.0, "y": 1.0, "z": 0.0 }, "vertices": { "outline": pts.iter().map(|&p| point3(p)).collect::<Vec<_>>() } });
        let mut ps = vec![
            quad([[0.0, 15.0, 0.0], [1.0, 15.0, 0.0], [1.0, 15.0, 2.0], [0.0, 15.0, 2.0]]),
            quad([[1.0, 15.0, 0.0], [2.0, 15.0, 0.0], [2.0, 15.0, 0.7], [1.0, 15.0, 0.7]]),
            quad([[1.0, 15.0, 0.7], [2.0, 15.0, 0.7], [2.0, 15.0, 2.003], [1.0, 15.0, 2.003]]),
        ];
        weld_platforms(&mut ps);
        let a = outline(&ps[0]["vertices"]);
        assert_eq!(a, vec![[0.0, 15.0, 0.0], [1.0, 15.0, 0.0], [1.0, 15.0, 0.7], [1.0, 15.0, 2.0], [0.0, 15.0, 2.0]]);
        assert_eq!(outline(&ps[2]["vertices"])[3], [1.0, 15.0, 2.0], "the corner 3 mm off is the other's");
        assert_eq!(outline(&ps[1]["vertices"]).len(), 4);
    }

    /// Platforms go round the game's way (clockwise from above: a negative area over x, z): one made the other way is turned round, one
    /// made the game's way is kept.
    #[test]
    fn platforms_go_round_the_games_way() {
        let quad = |pts: [[f32; 3]; 4]| json!({ "vertices": { "outline": pts.iter().map(|&p| point3(p)).collect::<Vec<_>>() } });
        let area = |p: &Value| { let o = outline(&p["vertices"]); (0..o.len()).map(|i| { let (a, b) = (o[(i + o.len() - 1) % o.len()], o[i]); a[0] * b[2] - b[0] * a[2] }).sum::<f32>() };
        // The stepped bastion's stair, as the game has it.
        let mut game = quad([[5.8, 15.0, 9.0], [5.8, 15.0, 5.0], [-6.2, 25.0, 5.0], [-6.2, 25.0, 9.0]]);
        let before = game.clone();
        wind_as_the_game(&mut game);
        assert_eq!(game, before);
        assert!(area(&game) < 0.0);

        let mut ours = quad([[0.0, 15.0, 5.0], [3.0, 14.0, 5.0], [3.0, 14.0, 9.0], [0.0, 15.0, 9.0]]);
        wind_as_the_game(&mut ours);
        assert!(area(&ours) < 0.0);
        assert_eq!(outline(&ours["vertices"])[0], [0.0, 15.0, 9.0]);
    }

    /// At a sharp corner the two pieces' platforms meet edge to edge on the bisector (the gondor base, its slanted platform edge too, as
    /// Amon Calen's upper prow): every edge one has on the face, the other has as well.
    #[test]
    fn a_corners_two_sides_share_their_edges() {
        use crate::pieces::{PieceWarp, cuts_of, pieces};
        use crate::profile::profile;
        use crate::solver::{WallInput, solve};
        let ground = |_: f64, _: f64| 0.0;
        let p = profile("gondor_fort_straight").unwrap();
        let quad = |pts: &[[f32; 3]]| json!({ "normal": { "x": 0.0, "y": 1.0, "z": 0.0 }, "vertices": { "outline": pts.iter().map(|&q| point3(q)).collect::<Vec<_>>() } });
        // gondor_fort_straight's walkway platforms (piece01, intact).
        let base = vec![
            vec![quad(&[[-10.5, 15.0, 8.5], [10.5, 15.0, 8.5], [10.5, 15.0, 3.25], [8.005, 15.0, 3.25]])],
            vec![quad(&[[8.005, 15.0, 3.25], [8.005, 15.0, 1.8], [-8.005, 15.0, 3.25], [-10.5, 15.0, 8.5]])],
            vec![quad(&[[8.005, 15.0, 1.8], [-8.005, 15.0, 1.8], [-8.005, 15.0, 3.25]])],
            vec![quad(&[[-8.005, 15.0, 3.25], [-10.5, 15.0, 3.25], [-10.5, 15.0, 8.5]])],
        ];
        for (tip, turn) in [([80.0, 40.0], "left"), ([80.0, -40.0], "right"), ([60.0, 70.0], "sharp")] {
            let input = WallInput { points: vec![[0.0, 0.0], tip, [160.0, 0.0]], sharp: vec![false, true, false], tolerance: 0.5, ..Default::default() };
            let plan = solve(&input, p, &ground);
            let s = plan.corners[0].0;
            let ps = pieces(&plan, p);
            let i = ps.iter().position(|q| (q.s.1 - s).abs() < 1e-6).unwrap();
            let edges = |k: usize| {
                let w = PieceWarp::new(&plan, p, ps[k], &ground);
                let mut notes = LogicNotes::default();
                let cuts = cuts_of(&plan, &ps[k], p);
                let (mut platforms, _) = platform_strips(&base, &w, &cuts, &|_| true, 0, &mut notes);
                // The copies of the base filling the corner's outer side, as `pieces::make_piece` brings them.
                for dx in w.overhangs() {
                    let copy = crate::warp::Shifted { inner: &w, dx };
                    platforms.extend(platform_strips(&base, &copy, &cuts, &|_| true, 0, &mut notes).0);
                }
                let (sn, cs) = ps[k].yaw.sin_cos();
                let scene = |q: [f32; 3]| { let (x, z) = (f64::from(q[0]), f64::from(q[2])); [ps[k].origin[0] + x * cs - z * sn, ps[k].origin[2] + x * sn + z * cs] };
                platforms.iter().flat_map(|pl| { let o = outline(&pl["vertices"]).into_iter().map(scene).collect::<Vec<_>>(); (0..o.len()).map(move |j| (o[j], o[(j + 1) % o.len()])).collect::<Vec<_>>() }).collect::<Vec<_>>()
            };
            let (ea, eb) = (edges(i), edges(i + 1));
            let near = |p: [f64; 2], q: [f64; 2]| (p[0] - q[0]).hypot(p[1] - q[1]) < 0.005;
            let on_b = |p: [f64; 2]| eb.iter().any(|&(x, y)| near(p, x) || near(p, y));
            let face = ea.iter().filter(|&&(x, y)| on_b(x) && on_b(y) && (x[0] - y[0]).hypot(x[1] - y[1]) > 0.05).collect::<Vec<_>>();
            assert!(face.len() >= 4, "{turn}: only {} edges on the face", face.len());
            for &&(x, y) in &face {
                assert!(eb.iter().any(|&(u, v)| (near(x, u) && near(y, v)) || (near(x, v) && near(y, u))), "{turn}: the edge {x:?} {y:?} is not the other side's");
            }

            // The firing lines along the inner side (LowWall, Overflow: the length of the piece) stay on their own side of the bisector,
            // nothing reaching over the other side's walkway.
            let c = crate::pieces::sample_at(&plan, s).centre;
            let lines = [8.5f32, 7.5, 6.5].map(|z| json!({ "start": { "x": 10.34, "y": 15.0, "z": z }, "end": { "x": -10.34, "y": 15.0, "z": z },
                "direction": { "x": 0.0, "y": 0.0, "z": 0.38 }, "parent_index": 0 }));
            for (k, sign) in [(i, -1.0), (i + 1, 1.0)] {
                let w = PieceWarp::new(&plan, p, ps[k], &ground);
                let cuts = cuts_of(&plan, &ps[k], p);
                let mut notes = LogicNotes::default();
                let mut out = vec![];
                let (_, strips) = platform_strips(&base, &w, &cuts, &|_| true, 0, &mut notes);
                out.extend(place_lines(&lines, &w, &cuts, &strips, &|q| crate::warp::inside(&w.clip_planes(), q), &mut notes));
                for dx in w.overhangs() {
                    let copy = crate::warp::Shifted { inner: &w, dx };
                    let (_, strips) = platform_strips(&base, &copy, &cuts, &|_| true, 0, &mut notes);
                    out.extend(place_lines(&lines, &copy, &cuts, &strips, &|q| crate::warp::inside(&copy.clip_planes(), q), &mut notes));
                }

                let (sn, cs) = ps[k].yaw.sin_cos();
                let scene = |q: [f32; 3]| { let (x, z) = (f64::from(q[0]), f64::from(q[2])); [ps[k].origin[0] + x * cs - z * sn, ps[k].origin[2] + x * sn + z * cs] };
                let (da, db) = crate::pieces::section_end_dirs(&plan, s);
                let m = [da[0] + db[0], da[1] + db[1]];
                for l in &out {
                    for key in ["start", "end"] {
                        let q = scene(p3(&l[key]));
                        let side = ((q[0] - c[0]) * m[0] + (q[1] - c[1]) * m[1]) * sign;
                        assert!(side > -0.02, "{turn}: a line of piece {k} reaches {side:.3} past the bisector at {q:?}");
                    }
                }
            }

            // Every point one side has on the face, the other has too (no edge of one ends inside an edge of the other).
            let pts_a = face.iter().flat_map(|&&(x, y)| [x, y]).collect::<Vec<_>>();
            let pts_b = eb.iter().flat_map(|&(x, y)| [x, y]).collect::<Vec<_>>();
            assert!(pts_a.iter().all(|&q| pts_b.iter().any(|&r| near(q, r))), "{turn}");
        }
    }

    /// A hull cut by a corner's face (or an attached end's) is closed again: no open edge, its triangles facing out as the game's (a
    /// positive volume), the cut filled even where the cross-section is not convex.
    #[test]
    fn a_cut_hull_is_closed() {
        // A wall's cross-section with a parapet (an L, not convex), along x from -10 to 10, wound out as the game's hulls.
        let section = [[0.0f32, 0.0], [9.0, 0.0], [9.0, 15.0], [2.0, 15.0], [2.0, 17.0], [0.0, 17.0]];
        let (n, mut verts, mut faces) = (section.len(), vec![], vec![]);
        for x in [-10.0f32, 10.0] {
            verts.extend(section.iter().map(|&[z, y]| [x, y, z]));
        }

        for i in 0..n {
            let j = (i + 1) % n;
            faces.push([i, n + i, n + j]);
            faces.push([i, n + j, j]);
        }

        let flat = section.iter().map(|&[z, y]| [z, y]).collect::<Vec<_>>();
        for [a, b, c] in ear_clip_indices(&flat) {
            faces.push([a, c, b]);
            faces.push([n + a, n + b, n + c]);
        }

        let volume = |c: &Value| {
            let v = c["vertices"].as_array().unwrap().iter().map(p3).collect::<Vec<_>>();
            c["triangles"].as_array().unwrap().iter().map(|t| {
                let (a, b, d) = (v[t["vertex_1"].as_u64().unwrap() as usize], v[t["vertex_2"].as_u64().unwrap() as usize], v[t["vertex_3"].as_u64().unwrap() as usize]);
                a[0] * (b[1] * d[2] - b[2] * d[1]) - a[1] * (b[0] * d[2] - b[2] * d[0]) + a[2] * (b[0] * d[1] - b[1] * d[0])
            }).sum::<f32>() / 6.0
        };
        let mut hull = json!({ "bounding_box": { "min_x": 0.0, "max_x": 0.0, "min_y": 0.0, "max_y": 0.0, "min_z": 0.0, "max_z": 0.0 },
            "vertices": verts.iter().map(|&p| point3(p)).collect::<Vec<_>>(),
            "triangles": faces.iter().map(|f| json!({ "vertex_1": f[0], "vertex_2": f[1], "vertex_3": f[2] })).collect::<Vec<_>>() });
        let flat_hull = hull.clone();
        write_collision(&mut hull, &faces.iter().map(|f| f.iter().map(|&i| verts[i]).collect::<Vec<_>>()).collect::<Vec<_>>());
        assert!(volume(&hull) > 0.0, "the test hull is wound out");

        for plane in [[1.0f32, 0.0, 2.0], [1.0, 0.6, 3.0], [-1.0, 0.8, 4.0]] {
            let polys = collision_polygons(&flat_hull, &crate::warp::Identity, &Cuts { step: 5.0, at: vec![] }, &[plane], &[], None).into_iter().map(|p| p.1).collect::<Vec<_>>();
            let mut cut = flat_hull.clone();
            write_collision(&mut cut, &polys);
            let open = cut["triangles"].as_array().unwrap().iter().flat_map(|t| (1..=3).map(move |k| t[format!("across_face_index_{k}")].as_i64().unwrap())).filter(|&a| a < 0).count();
            assert_eq!(open, 0, "{plane:?}: open edges");
            assert!(volume(&cut) > 0.0 && volume(&cut) < volume(&hull), "{plane:?}: volume {} of {}", volume(&cut), volume(&hull));
        }
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
