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

use crate::warp::{Warp, newell, normalised, slice_along_x, subdivide, transform, turn};

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

/// A logic file warped; long outlines get a point at least every `step` metres.
///
/// # Errors
///
/// The file doesn't read, or doesn't write back.
pub fn warp_logic(bytes: &[u8], warp: &dyn Warp, step: f32) -> Result<(Vec<u8>, LogicNotes), String> {
    let parsed = Cs2Parsed::decode(&mut Cursor::new(bytes), &None).map_err(|e| format!("cs2.parsed: {e}"))?;
    let mut v = serde_json::to_value(&parsed).map_err(|e| e.to_string())?;
    let mut notes = LogicNotes::default();

    for piece in array_mut(&mut v, "pieces") {
        let piece_name = piece["name"].as_str().unwrap_or_default().to_owned();
        for d in array_mut(piece, "destructs") {
            let name = d["name"].as_str().unwrap_or(&piece_name).to_owned();
            let strips = warp_platforms(d, warp, step, &mut notes);
            warp_lines(d, warp, &strips, &mut notes);
            for o in array_mut(d, "collision_outlines") {
                warp_outline(&mut o["vertices"], warp, step);
            }

            for ring in array_mut(d, "orange_thingies") {
                warp_footprint(ring, warp, step);
            }

            for pipe in array_mut(d, "pipes") {
                for p in array_mut(&mut pipe["line"], "outline") {
                    set_p3(p, warp.point(p3(p)));
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

/// A strip of a platform: its index among the new platforms, and the x range it covers before the warp.
struct Strip {
    index: usize,
    x: (f32, f32),
}

/// The platforms cut and warped; per original platform, its strips.
fn warp_platforms(d: &mut Value, warp: &dyn Warp, step: f32, notes: &mut LogicNotes) -> Vec<Vec<Strip>> {
    let old = d["platforms"].as_array().cloned().unwrap_or_default();
    let mut new = vec![];
    let mut strips = vec![];
    for p in &old {
        let points = outline(&p["vertices"]);
        let normal = p3(&p["normal"]);
        let mut slabs = slice_along_x(&points, step);
        if slabs.is_empty() {
            slabs = vec![points.clone()];
        }

        let mut mine = vec![];
        for s in slabs {
            let x = s.iter().fold((f32::MAX, f32::MIN), |(lo, hi), q| (lo.min(q[0]), hi.max(q[0])));
            let w = s.iter().map(|&q| warp.point(q)).collect::<Vec<_>>();
            let mut n = normalised(newell(&w));
            if n[0] * normal[0] + n[1] * normal[1] + n[2] * normal[2] < 0.0 {
                n = [-n[0], -n[1], -n[2]];
            }

            let mut q = p.clone();
            q["vertices"] = json!({ "outline": w.iter().map(|&v| point3(v)).collect::<Vec<_>>() });
            q["normal"] = point3(n);
            mine.push(Strip { index: new.len(), x });
            new.push(q);
        }

        strips.push(mine);
    }

    notes.platforms.0 += old.len();
    notes.platforms.1 += new.len();
    d["platforms"] = Value::Array(new);
    strips
}

/// The firing lines moved and turned, on the strip of their platform that is under them.
fn warp_lines(d: &mut Value, warp: &dyn Warp, strips: &[Vec<Strip>], notes: &mut LogicNotes) {
    for l in array_mut(d, "ef_lines") {
        let (s, e) = (p3(&l["start"]), p3(&l["end"]));
        let mid = [(s[0] + e[0]) / 2.0, (s[1] + e[1]) / 2.0, (s[2] + e[2]) / 2.0];
        let a = warp.yaw(mid);
        set_p3(&mut l["start"], warp.point(s));
        set_p3(&mut l["end"], warp.point(e));
        let dir = p3(&l["direction"]);
        let t = turn([dir[0], dir[2]], a);
        set_p3(&mut l["direction"], [t[0], dir[1], t[1]]);

        let parent = l["parent_index"].as_u64().unwrap_or(0) as usize;
        let Some(mine) = strips.get(parent).filter(|m| !m.is_empty()) else {
            notes.orphan_lines += 1;
            continue;
        };

        let inside = mine.iter().find(|s| s.x.0 - 1e-3 <= mid[0] && mid[0] <= s.x.1 + 1e-3);
        let strip = inside.unwrap_or_else(|| {
            notes.orphan_lines += 1;
            mine.iter().min_by(|a, b| gap(a, mid[0]).total_cmp(&gap(b, mid[0]))).expect("not empty")
        });
        l["parent_index"] = json!(strip.index);
    }
}

fn gap(s: &Strip, x: f32) -> f32 {
    (s.x.0 - x).max(x - s.x.1).max(0.0)
}

/// A 3D outline (`{"outline": [...]}`) with points along its edges, warped; closed when it has more than two points.
fn warp_outline(o: &mut Value, warp: &dyn Warp, step: f32) {
    let points = outline(o);
    let closed = points.len() > 2;
    let out = subdivide(&points, closed, step).into_iter().map(|(p, _)| point3(warp.point(p))).collect::<Vec<_>>();
    o["outline"] = Value::Array(out);
}

/// A footprint ring (`[{"vertex": {x, y}, "vertex_type"}]`, y being z) with points along its edges, warped; an added point takes the type
/// of the point that starts its edge.
fn warp_footprint(ring: &mut Value, warp: &dyn Warp, step: f32) {
    let Some(old) = ring.as_array().cloned() else { return };
    let points = old.iter().map(|v| { let p = p2(&v["vertex"]); [p[0], 0.0, p[1]] }).collect::<Vec<_>>();
    let out = subdivide(&points, points.len() > 2, step).into_iter().map(|(p, from)| {
        let q = warp.point(p);
        let mut v = old[from].clone();
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
