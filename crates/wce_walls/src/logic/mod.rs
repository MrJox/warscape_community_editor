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

mod collision;
mod piece_ends;
mod platforms;

use collision::*;
pub use piece_ends::PieceEnd;
pub use piece_ends::ends;
pub(crate) use platforms::wind_as_the_game;
use platforms::*;

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
pub fn warp_logic_with(
    bytes: &[u8],
    warp: &dyn Warp,
    cuts: &Cuts,
    drop_zy: Option<((f32, f32), (f32, f32))>,
    splices: &[Splice],
) -> Result<(Vec<u8>, LogicNotes), String> {
    warp_logic_framed(bytes, warp, cuts, drop_zy, splices, None)
}

/// [`warp_logic_with`], the base's platforms cut to its frame's x (`frame`): a piece's walkway ends on its joints exactly (some bases'
/// run a little past theirs, and the next piece's edges were 4 or 5 cm off: not one edge for units to cross; user 2026-11-05).
///
/// # Errors
///
/// The file doesn't read, or doesn't write back.
pub fn warp_logic_framed(
    bytes: &[u8],
    warp: &dyn Warp,
    cuts: &Cuts,
    drop_zy: Option<((f32, f32), (f32, f32))>,
    splices: &[Splice],
    frame: Option<(f32, f32)>,
) -> Result<(Vec<u8>, LogicNotes), String> {
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
        parts.push(
            pv["pieces"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|p| p["destructs"].as_array().cloned().unwrap_or_default())
                .collect::<Vec<_>>(),
        );
    }

    for (piece_index, piece) in array_mut(&mut v, "pieces").enumerate() {
        let piece_name = piece["name"].as_str().unwrap_or_default().to_owned();
        for (k, d) in array_mut(piece, "destructs").enumerate() {
            let name = d["name"].as_str().unwrap_or(&piece_name).to_owned();
            // The splices brought into this level: the ones that have it.
            let here = splices
                .iter()
                .zip(&parts)
                .filter_map(|(sp, pieces)| {
                    let levels = match (sp.per_piece, piece_index) {
                        (true, i) => pieces.get(i),
                        (false, 0) => pieces.first(),
                        _ => None,
                    };

                    levels.and_then(|l| l.get(k)).map(|sd| (sp, sd))
                })
                .collect::<Vec<_>>();
            // What a step replaces goes from every piece of the base (its end caps' platforms are pieces of their own), not only from the
            // one its own platforms go into; at the levels the step has (a level it lacks keeps the base whole).
            let drop = splices
                .iter()
                .zip(&parts)
                .filter(|(sp, pieces)| !sp.per_piece && pieces.first().is_some_and(|l| l.get(k).is_some()))
                .filter_map(|(sp, _)| sp.replaces)
                .collect::<Vec<_>>();
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
                ef.extend(place_lines(
                    sd["ef_lines"].as_array().map_or(&[][..], Vec::as_slice),
                    sp.warp,
                    &part_cuts,
                    &s2,
                    &holds,
                    &mut notes,
                ));
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
                    let more = sd[key]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter(|o| if inner { kept(&o["vertices"]["outline"], 2) } else { kept(o, 3) })
                        .cloned()
                        .collect::<Vec<_>>();
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

                merge_platforms(d);
            }
        }
    }

    warp_box(&mut v["bounding_box"], warp, step);
    let corners = v["pieces"]
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|p| p["destructs"].as_array().into_iter().flatten())
        .flat_map(|d| {
            let b = &d["bounding_box"];
            let f = |k: &str| b[k].as_f64().unwrap_or(0.0) as f32;
            [[f("min_x"), f("min_y"), f("min_z")], [f("max_x"), f("max_y"), f("max_z")]]
        })
        .collect::<Vec<_>>();
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

/// The x and z ranges of a logic file's box (its footprint on the ground).
///
/// # Errors
///
/// The file doesn't read.
pub fn extent_xz(bytes: &[u8]) -> Result<[(f32, f32); 2], String> {
    let parsed = Cs2Parsed::decode(&mut Cursor::new(bytes), &None).map_err(|e| format!("cs2.parsed: {e}"))?;
    let v = serde_json::to_value(&parsed).map_err(|e| e.to_string())?;
    let f = |k: &str| v["bounding_box"][k].as_f64().unwrap_or(0.0) as f32;
    Ok([(f("min_x"), f("max_x")), (f("min_z"), f("max_z"))])
}

/// The z extent of a piece's walkway: its intact platforms lying at `height` (within a few centimetres). None without any.
pub fn walkway_z(bytes: &[u8], height: f32) -> Option<(f32, f32)> {
    let parsed = Cs2Parsed::decode(&mut Cursor::new(bytes), &None).ok()?;
    let v = serde_json::to_value(&parsed).ok()?;
    let zs = v["pieces"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|p| p["destructs"].as_array().and_then(|d| d.first()))
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
    let xs = v["pieces"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|p| p["destructs"].as_array().and_then(|d| d.first()))
        .flat_map(|d| {
            d["platforms"].as_array().into_iter().flatten().flat_map(|q| outline(&q["vertices"])).map(|q| q[0]).collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    let (wlo, whi) = xs.iter().fold((f32::MAX, f32::MIN), |(a, b), &x| (a.min(x), b.max(x)));
    let near = |w: f32, b: f32| (w - b).abs() <= 0.5;
    Ok((if near(wlo, lo) { wlo } else { lo }, if near(whi, hi) { whi } else { hi }))
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
    use super::piece_ends::cap_end;
    use super::*;

    /// A walkway (x 0..20, z 3..9 at y 15) under a step over x 5..10 whose cross-section is z 5.1..8.9: the step's box goes, the walkway
    /// keeps its outer and inner edges there and all of itself elsewhere; a platform under the box's heights is left whole.
    #[test]
    fn a_step_cuts_its_box_out_of_the_walkway() {
        let quad = |x0: f32, x1: f32, z0: f32, z1: f32, y: f32| {
            json!({ "normal": { "x": 0.0, "y": 1.0, "z": 0.0 }, "vertices": { "outline": [
            { "x": x0, "y": y, "z": z0 }, { "x": x1, "y": y, "z": z0 }, { "x": x1, "y": y, "z": z1 }, { "x": x0, "y": y, "z": z1 } ] } })
        };
        let out = carve_platforms(
            &[quad(0.0, 20.0, 3.0, 9.0, 15.0), quad(0.0, 20.0, 3.0, 9.0, 2.0)],
            &[(5.0, 10.0)],
            Some(((5.1, 8.9), (14.0, 26.5))),
        )
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
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
        let quad = |x0: f32, x1: f32, z0: f32, z1: f32, y: f32| {
            json!({ "normal": { "x": 0.0, "y": 1.0, "z": 0.0 }, "vertices": { "outline": [
            { "x": x0, "y": y, "z": z0 }, { "x": x1, "y": y, "z": z0 }, { "x": x1, "y": y, "z": z1 }, { "x": x0, "y": y, "z": z1 } ] } })
        };
        let line = |x: f32, z: f32, parent: usize| {
            json!({ "start": { "x": x - 0.25, "y": 15.0, "z": z }, "end": { "x": x + 0.25, "y": 15.0, "z": z },
            "direction": { "x": 0.0, "y": 0.0, "z": -1.0 }, "parent_index": parent })
        };
        let carved = carve_platforms(
            &[quad(0.0, 20.0, 3.0, 9.0, 15.0), quad(0.0, 20.0, 1.0, 3.0, 15.0)],
            &[(5.0, 10.0)],
            Some(((5.1, 8.9), (14.0, 26.5))),
        );
        let mut notes = LogicNotes::default();
        let cuts = Cuts { step: 100.0, at: vec![] };
        let (platforms, strips) = platform_strips(&carved, &crate::warp::Identity, &cuts, &|_| true, 0, &mut notes);
        let lines = place_lines(
            &[line(15.0, 2.0, 1), line(7.0, 4.0, 0), line(7.0, 8.95, 0)],
            &crate::warp::Identity,
            &cuts,
            &strips,
            &|_| true,
            &mut notes,
        );
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
        let corners = [
            [-10.0, 0.0, 0.0],
            [10.0, 0.0, 0.0],
            [10.0, 0.0, 9.0],
            [-10.0, 0.0, 9.0],
            [-10.0, 15.0, 0.0],
            [10.0, 15.0, 0.0],
            [10.0, 15.0, 9.0],
            [-10.0, 15.0, 9.0],
        ];
        let faces = [
            [0, 2, 1],
            [0, 3, 2],
            [4, 5, 6],
            [4, 6, 7],
            [0, 1, 5],
            [0, 5, 4],
            [1, 2, 6],
            [1, 6, 5],
            [2, 3, 7],
            [2, 7, 6],
            [3, 0, 4],
            [3, 4, 7],
        ];
        let mut hull = json!({
            "name": "piece01_destruct01_collision3d", "uk_1": 1, "uk_2": 0, "zero_4": 0,
            "bounding_box": { "min_x": 0.0, "min_y": 0.0, "min_z": 0.0, "max_x": 0.0, "max_y": 0.0, "max_z": 0.0 },
            "vertices": corners.iter().map(|&p| point3(p)).collect::<Vec<_>>(),
            "triangles": faces.iter().map(|f| json!({ "vertex_1": f[0], "vertex_2": f[1], "vertex_3": f[2] })).collect::<Vec<_>>(),
        });
        let bend = crate::warp::Bend::turning(-10.0, 10.0, 0.6);
        let flat = hull.clone();
        let polys =
            collision_polygons(&hull, &bend, &Cuts { step: 5.0, at: vec![] }, &[], &[], None).into_iter().map(|p| p.1).collect::<Vec<_>>();
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
        let polys = collision_polygons(
            &flat,
            &crate::warp::Identity,
            &Cuts { step: 5.0, at: vec![] },
            &[],
            &[((2.0, 6.0), (14.5, 15.5))],
            Some((0.0, 9.0)),
        );
        let pts = polys.iter().flat_map(|p| p.1.iter().copied()).collect::<Vec<_>>();
        assert!(pts.iter().any(|p| (p[0] - 4.0).abs() < 1.8 && (p[1] - (15.0 - SINK)).abs() < 1e-4), "nothing sunk");
        let wrong = pts
            .iter()
            .filter(|p| p[1] > 14.0)
            .filter(|p| {
                !(if p[0] > 2.0 + SINK_IN - 1e-3 && p[0] < 6.0 - SINK_IN + 1e-3 {
                    (p[1] - (15.0 - SINK)).abs() < 1e-4
                } else {
                    (p[1] - 15.0).abs() < 1e-4
                })
            })
            .collect::<Vec<_>>();
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
        let area = |p: &Value| {
            let o = outline(&p["vertices"]);
            (0..o.len())
                .map(|i| {
                    let (a, b) = (o[(i + o.len() - 1) % o.len()], o[i]);
                    a[0] * b[2] - b[0] * a[2]
                })
                .sum::<f32>()
        };
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
        use crate::solver::{WallInput, solve};
        let ground = |_: f64, _: f64| 0.0;
        let p = crate::profile::test_profile("gondor_fort_straight").unwrap();
        let quad = |pts: &[[f32; 3]]| json!({ "normal": { "x": 0.0, "y": 1.0, "z": 0.0 }, "vertices": { "outline": pts.iter().map(|&q| point3(q)).collect::<Vec<_>>() } });
        // gondor_fort_straight's walkway platforms (piece01, intact).
        let base = vec![
            vec![quad(&[[-10.5, 15.0, 8.5], [10.5, 15.0, 8.5], [10.5, 15.0, 3.25], [8.005, 15.0, 3.25]])],
            vec![quad(&[[8.005, 15.0, 3.25], [8.005, 15.0, 1.8], [-8.005, 15.0, 3.25], [-10.5, 15.0, 8.5]])],
            vec![quad(&[[8.005, 15.0, 1.8], [-8.005, 15.0, 1.8], [-8.005, 15.0, 3.25]])],
            vec![quad(&[[-8.005, 15.0, 3.25], [-10.5, 15.0, 3.25], [-10.5, 15.0, 8.5]])],
        ];
        for (tip, turn) in [([80.0, 40.0], "left"), ([80.0, -40.0], "right"), ([60.0, 70.0], "sharp")] {
            let input = WallInput {
                points: vec![[0.0, 0.0], tip, [160.0, 0.0]],
                sharp: vec![false, true, false],
                tolerance: 0.5,
                ..Default::default()
            };
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
                let scene = |q: [f32; 3]| {
                    let (x, z) = (f64::from(q[0]), f64::from(q[2]));
                    [ps[k].origin[0] + x * cs - z * sn, ps[k].origin[2] + x * sn + z * cs]
                };
                platforms
                    .iter()
                    .flat_map(|pl| {
                        let o = outline(&pl["vertices"]).into_iter().map(scene).collect::<Vec<_>>();
                        (0..o.len()).map(move |j| (o[j], o[(j + 1) % o.len()])).collect::<Vec<_>>()
                    })
                    .collect::<Vec<_>>()
            };
            let (ea, eb) = (edges(i), edges(i + 1));
            let near = |p: [f64; 2], q: [f64; 2]| (p[0] - q[0]).hypot(p[1] - q[1]) < 0.005;
            let on_b = |p: [f64; 2]| eb.iter().any(|&(x, y)| near(p, x) || near(p, y));
            let face = ea.iter().filter(|&&(x, y)| on_b(x) && on_b(y) && (x[0] - y[0]).hypot(x[1] - y[1]) > 0.05).collect::<Vec<_>>();
            assert!(face.len() >= 4, "{turn}: only {} edges on the face", face.len());
            for &&(x, y) in &face {
                assert!(
                    eb.iter().any(|&(u, v)| (near(x, u) && near(y, v)) || (near(x, v) && near(y, u))),
                    "{turn}: the edge {x:?} {y:?} is not the other side's"
                );
            }

            // The firing lines along the inner side (LowWall, Overflow: the length of the piece) stay on their own side of the bisector,
            // nothing reaching over the other side's walkway.
            let c = crate::pieces::sample_at(&plan, s).centre;
            let lines = [8.5f32, 7.5, 6.5].map(|z| {
                json!({ "start": { "x": 10.34, "y": 15.0, "z": z }, "end": { "x": -10.34, "y": 15.0, "z": z },
                "direction": { "x": 0.0, "y": 0.0, "z": 0.38 }, "parent_index": 0 })
            });
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
                let scene = |q: [f32; 3]| {
                    let (x, z) = (f64::from(q[0]), f64::from(q[2]));
                    [ps[k].origin[0] + x * cs - z * sn, ps[k].origin[2] + x * sn + z * cs]
                };
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
            c["triangles"]
                .as_array()
                .unwrap()
                .iter()
                .map(|t| {
                    let (a, b, d) = (
                        v[t["vertex_1"].as_u64().unwrap() as usize],
                        v[t["vertex_2"].as_u64().unwrap() as usize],
                        v[t["vertex_3"].as_u64().unwrap() as usize],
                    );
                    a[0] * (b[1] * d[2] - b[2] * d[1]) - a[1] * (b[0] * d[2] - b[2] * d[0]) + a[2] * (b[0] * d[1] - b[1] * d[0])
                })
                .sum::<f32>()
                / 6.0
        };
        let mut hull = json!({ "bounding_box": { "min_x": 0.0, "max_x": 0.0, "min_y": 0.0, "max_y": 0.0, "min_z": 0.0, "max_z": 0.0 },
            "vertices": verts.iter().map(|&p| point3(p)).collect::<Vec<_>>(),
            "triangles": faces.iter().map(|f| json!({ "vertex_1": f[0], "vertex_2": f[1], "vertex_3": f[2] })).collect::<Vec<_>>() });
        let flat_hull = hull.clone();
        write_collision(&mut hull, &faces.iter().map(|f| f.iter().map(|&i| verts[i]).collect::<Vec<_>>()).collect::<Vec<_>>());
        assert!(volume(&hull) > 0.0, "the test hull is wound out");

        for plane in [[1.0f32, 0.0, 2.0], [1.0, 0.6, 3.0], [-1.0, 0.8, 4.0]] {
            let polys = collision_polygons(&flat_hull, &crate::warp::Identity, &Cuts { step: 5.0, at: vec![] }, &[plane], &[], None)
                .into_iter()
                .map(|p| p.1)
                .collect::<Vec<_>>();
            let mut cut = flat_hull.clone();
            write_collision(&mut cut, &polys);
            let open = cut["triangles"]
                .as_array()
                .unwrap()
                .iter()
                .flat_map(|t| (1..=3).map(move |k| t[format!("across_face_index_{k}")].as_i64().unwrap()))
                .filter(|&a| a < 0)
                .count();
            assert_eq!(open, 0, "{plane:?}: open edges");
            assert!(volume(&cut) > 0.0 && volume(&cut) < volume(&hull), "{plane:?}: volume {} of {}", volume(&cut), volume(&hull));
        }
    }

    /// Neighbouring platforms become one where they stay flat together and convex, the firing lines following; a step in height or a
    /// concave result keeps them apart.
    #[test]
    fn platforms_merge_where_they_stay_convex() {
        let plat = |pts: &[[f32; 3]]| json!({ "flag_1": false, "flag_2": false, "flag_3": false, "normal": { "x": 0.0, "y": 1.0, "z": 0.0 }, "vertices": { "outline": pts.iter().map(|&p| point3(p)).collect::<Vec<_>>() } });
        // Clockwise from above, as the game's: x back along z 0, x on along z 2.
        let strip = |x0: f32, x1: f32, y: f32| plat(&[[x1, y, 0.0], [x0, y, 0.0], [x0, y, 2.0], [x1, y, 2.0]]);
        let line = |parent: usize| json!({ "start": { "x": 0.0, "y": 15.0, "z": 1.0 }, "end": { "x": 1.0, "y": 15.0, "z": 1.0 }, "direction": { "x": 0.0, "y": 0.0, "z": 1.0 }, "parent_index": parent });

        // Three strips in a row, one higher beyond them: the three are one, its line on it, the higher one's on what it became.
        let mut d = json!({ "platforms": [strip(0.0, 0.1, 15.0), strip(0.1, 3.0, 15.0), strip(3.0, 3.05, 15.0), strip(3.05, 5.0, 15.5)], "ef_lines": [line(0), line(2), line(3)] });
        merge_platforms(&mut d);
        let ps = d["platforms"].as_array().unwrap();
        assert_eq!(ps.len(), 2, "{ps:?}");
        let parents = d["ef_lines"].as_array().unwrap().iter().map(|l| l["parent_index"].as_u64().unwrap() as usize).collect::<Vec<_>>();
        assert_eq!(parents[0], parents[1]);
        assert_ne!(parents[0], parents[2]);
        let flat = outline(&ps[parents[0]]["vertices"]);
        assert!(flat.iter().all(|q| q[1] == 15.0) && (area_xz(&flat) - 6.1).abs() < 1e-3, "{flat:?}");

        // An L: its two arms stay two platforms (no platform of the game's is concave).
        let mut d = json!({ "platforms": [strip(0.0, 4.0, 15.0), plat(&[[1.0, 15.0, 2.0], [0.0, 15.0, 2.0], [0.0, 15.0, 5.0], [1.0, 15.0, 5.0]])], "ef_lines": [] });
        merge_platforms(&mut d);
        assert_eq!(d["platforms"].as_array().unwrap().len(), 2);

        // A corner a third platform shares, in a straight line on the merged outline, stays.
        let mut d = json!({ "platforms": [plat(&[[1.0, 15.0, 0.0], [0.0, 15.0, 0.0], [0.0, 15.0, 2.0], [1.0, 15.0, 2.0]]), plat(&[[2.0, 15.0, 0.0], [1.0, 15.0, 0.0], [1.0, 15.0, 2.0], [2.0, 15.0, 2.0]])], "ef_lines": [] });
        merge_platforms(&mut d);
        let o = outline(&d["platforms"][0]["vertices"]);
        assert_eq!(d["platforms"].as_array().unwrap().len(), 1);
        assert!(o.contains(&[1.0, 15.0, 0.0]) && o.contains(&[1.0, 15.0, 2.0]), "{o:?}");
    }

    #[test]
    fn an_end_cap_gives_its_outer_side_and_the_way_out() {
        // roman_fort_straight's left cap (x -10..-8, z 0.5..3, 15 m up): the joint at x -10, the wall leaving along -x.
        let cap = [[-10.0, 15.0, 0.5], [-8.0, 15.0, 0.5], [-8.0, 15.0, 3.0], [-10.0, 15.0, 3.0]];
        let e = cap_end(&cap, [0.0, 5.0], &[], &[], &[], None).expect("a cap");
        assert_eq!(e.at, [-10.0, 15.0, 1.75]);
        assert_eq!(e.out, [-1.0, 0.0]);
        assert_eq!(e.side, [0.0, -1.0], "the outer face towards -z (the cap at z 0.5..3 of the walk's 0.5..9)");

        // roman_fort_internal_corner's other cap (x 0.5..3, z 0..2), the body towards +z: the wall leaves along -z.
        let cap = [[0.5, 15.0, 0.0], [3.0, 15.0, 0.0], [3.0, 15.0, 2.0], [0.5, 15.0, 2.0]];
        let e = cap_end(&cap, [-2.0, 18.0], &[], &[], &[], None).expect("a cap");
        assert_eq!(e.at, [1.75, 15.0, 0.0]);
        assert_eq!(e.out, [0.0, -1.0]);

        // The bastion's high cap, its inner side slanted: the joint at x 10, out along +x.
        let cap = [[8.0, 25.0, 0.5], [8.97, 25.0, 1.95], [10.0, 25.0, 3.0], [10.0, 25.0, 0.5]];
        let e = cap_end(&cap, [0.0, 4.0], &[], &[], &[], None).expect("a cap");
        assert_eq!(e.at, [10.0, 25.0, 1.75]);
        assert_eq!(e.out, [1.0, 0.0]);

        // Not a cap: a long platform.
        // The bastion's high end: the body's centre is beyond the cap (the tower juts out), the walk by the cap is not (z 1.95..9).
        let cap = [[8.0, 25.0, 0.5], [10.0, 25.0, 0.5], [10.0, 25.0, 3.0], [8.0, 25.0, 3.0]];
        let walk = [[6.2, 1.95], [10.0, 1.95], [10.0, 9.0], [6.2, 9.0], [-5.0, -5.0], [8.0, -5.0], [8.0, 4.5]];
        assert_eq!(cap_end(&cap, [0.0, -1.0], &walk, &[], &[], None).expect("a cap").side, [0.0, -1.0]);

        assert!(
            cap_end(&[[-10.0, 15.0, 3.0], [10.0, 15.0, 3.0], [10.0, 15.0, 9.0], [-10.0, 15.0, 9.0]], [0.0, 5.0], &[], &[], &[], None)
                .is_none()
        );
    }

    /// The name the synthetic logic file's ui flag has (its length places the pieces' count in the bytes).
    const UI_FLAG: &str = "ui_flag";

    /// A logic file made in code, written at `version`: one piece with one level holding a walkway, a hard outline, a footprint ring, a
    /// door link and a firing line on the walkway, in a box x -10..10, y 0..15, z 0..9.
    fn synthetic_logic(version: u32) -> (Cs2Parsed, Vec<u8>) {
        use rpfm_lib::files::cs2_parsed::{Destruct, EFLine, Piece, Pipe};

        let ring = |pts: &[[f32; 3]]| json!({ "outline": pts.iter().map(|&p| point3(p)).collect::<Vec<_>>() });
        let walk = [[-10.0, 15.0, 0.5], [10.0, 15.0, 0.5], [10.0, 15.0, 9.0], [-10.0, 15.0, 9.0]];
        let cube = json!({ "min_x": -10.0, "min_y": 0.0, "min_z": 0.0, "max_x": 10.0, "max_y": 15.0, "max_z": 9.0 });

        let mut line = serde_json::to_value(EFLine::default()).unwrap();
        line["name"] = json!("ef_line_0");
        line["start"] = point3([-8.0, 15.0, 1.0]);
        line["end"] = point3([8.0, 15.0, 1.0]);
        line["direction"] = point3([0.0, 0.0, -1.0]);

        let mut pipe = serde_json::to_value(Pipe::default()).unwrap();
        pipe["name"] = json!("door_0");
        pipe["line"] = ring(&[[0.0, 0.0, 10.0], [0.0, 15.0, 8.0]]);

        let mut d = serde_json::to_value(Destruct::default()).unwrap();
        d["name"] = json!("piece01_destruct01");
        d["bounding_box"] = cube.clone();
        d["platforms"] =
            json!([{ "normal": point3([0.0, 1.0, 0.0]), "vertices": ring(&walk), "flag_1": true, "flag_2": false, "flag_3": true }]);
        d["collision_outlines"] = json!([{ "name": "outline_0", "vertices": ring(&[[-10.0, 0.0, 0.0], [10.0, 0.0, 0.0], [10.0, 0.0, 9.0], [-10.0, 0.0, 9.0]]), "uk_1": 7 }]);
        d["orange_thingies"] = json!([[
            { "vertex": { "x": -10.0, "y": 0.0 }, "vertex_type": 1 },
            { "vertex": { "x": 10.0, "y": 0.0 }, "vertex_type": 0 },
            { "vertex": { "x": 10.0, "y": 9.0 }, "vertex_type": 0 },
        ]]);
        d["pipes"] = json!([pipe]);
        d["ef_lines"] = json!([line]);
        d["f_1"] = json!(0.25);

        let mut piece = serde_json::to_value(Piece::default()).unwrap();
        piece["name"] = json!("piece01");
        piece["node_name"] = json!("piece01_node");
        piece["destructs"] = json!([d]);
        piece["f_6"] = json!(1.5);

        let mut v = serde_json::to_value(Cs2Parsed::default()).unwrap();
        v["version"] = json!(version);
        v["bounding_box"] = cube;
        v["ui_flag"]["name"] = json!(UI_FLAG);
        v["int_1"] = json!(3);
        v["pieces"] = json!([piece]);

        let mut parsed: Cs2Parsed = serde_json::from_value(v).unwrap();
        let mut bytes = vec![];
        parsed.encode(&mut bytes, &None).unwrap();
        (parsed, bytes)
    }

    /// A logic file made in code (no game file) reads back to what was written and writes back to the same bytes, at every version the
    /// reader knows; its serde form, as the warp edits it, too; and what the module reads of it is what was put in.
    #[test]
    fn a_synthetic_logic_file_round_trips_byte_for_byte() {
        for version in [0, 8, 9, 10, 11, 12, 13, 18, 20, 21] {
            let (built, bytes) = synthetic_logic(version);
            let mut once = Cs2Parsed::decode(&mut Cursor::new(&bytes), &None).unwrap_or_else(|e| panic!("v{version}: {e}"));
            let mut again = vec![];
            once.encode(&mut again, &None).unwrap();
            assert_eq!(again, bytes, "v{version}: written again, not the same bytes");
            let twice = Cs2Parsed::decode(&mut Cursor::new(&again), &None).unwrap();
            assert_eq!(once, twice, "v{version}");
            assert_eq!(round_trips(&bytes), Ok(true), "v{version}");

            // The file's box is in versions 8 to 18 (0, 20 and 21 have none).
            if (8..=18).contains(&version) {
                assert_eq!(extent_x(&bytes), Ok((-10.0, 10.0)), "v{version}");
                assert_eq!(extent_xz(&bytes), Ok([(-10.0, 10.0), (0.0, 9.0)]), "v{version}");
            }

            // Version 11 holds every field set here (13 has no level's `f_1`, and so on), so it reads back to what was built.
            if version == 11 {
                assert_eq!(once, built);
                assert_eq!(walkway_z(&bytes, 15.0), Some((0.5, 9.0)));
            }
        }
    }

    /// A logic file cut short anywhere, with a byte too many, of a version the reader does not know, or with a count far past its end is
    /// an error, never a panic.
    #[test]
    fn a_truncated_or_malformed_logic_file_is_an_error() {
        let (_, bytes) = synthetic_logic(13);
        for n in 0..bytes.len() {
            assert!(round_trips(&bytes[..n]).is_err(), "cut at {n} of {}", bytes.len());
            assert!(extent_x(&bytes[..n]).is_err(), "cut at {n}");
            assert!(ends(&bytes[..n]).is_err(), "cut at {n}");
            assert!(walkway_z(&bytes[..n], 15.0).is_none(), "cut at {n}");
        }

        let mut longer = bytes.clone();
        longer.push(0);
        assert!(round_trips(&longer).is_err());

        let mut unknown = bytes.clone();
        unknown[..4].copy_from_slice(&99u32.to_le_bytes());
        assert!(round_trips(&unknown).is_err());
        assert!(warp_logic(&unknown, &crate::warp::Bend::turning(-10.0, 10.0, 0.0), &Cuts { step: 5.0, at: vec![] }).is_err());

        // The pieces' count: after the version, the box (6 f32), the ui flag's name (u16 length, UTF-16) and transform (16 f32), and
        // `int_1`.
        let at = 4 + 24 + 2 + 2 * UI_FLAG.len() + 64 + 4;
        assert_eq!(bytes[at..at + 4], 1u32.to_le_bytes());
        let mut huge = bytes.clone();
        huge[at..at + 4].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(round_trips(&huge).is_err());

        assert!(round_trips(&[]).is_err());
    }
}
