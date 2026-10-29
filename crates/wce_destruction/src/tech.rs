//! The logic file (`<key>_tech.cs2.parsed`) of a destroyed building: per piece, the intact level as it was, then a level per damage
//! level made from the intact one (through rpfm_lib's serde form, as wce_walls::logic does):
//!
//! - names `pieceNN_destruct01...` renamed to the level's; the collision mesh's id (`collision_3d.uk_1`) a fresh one;
//! - **platforms** cut by the cells removed by then (the parts outside kept); on a walkable rubble heap, its platforms
//!   (flags `false, true, false`, as the game's rubble ramps);
//! - **firing lines** in what is gone left out, the others on the piece of their platform under them;
//! - the **hard outline** and the **footprint** (`orange_thingies`) from a raster of the intact ones less what is gone at the ground,
//!   plus the heap when it blocks; other outlines kept where they stand;
//! - **pipes**, **docking lines**, emitters, soft collisions, attachments and effects in what is gone left out;
//! - the **collision mesh** given by the caller (the intact one cut and capped), its edge adjacency rebuilt (`across_face_index_k`: the
//!   triangle sharing edge k, as every game file has it; -1 where none does);
//! - boxes from the level's geometry.

use std::collections::HashMap;
use std::io::Cursor;

use rpfm_lib::files::cs2_parsed::Cs2Parsed;
use rpfm_lib::files::{Decodeable, Encodeable};
use serde_json::{Value, json};

use crate::geom::{Aabb, Plane, V3, clip_polygon, newell};
use crate::rubble::Heap;

/// A damage level of a piece.
pub struct LevelLogic<'a> {
    /// Whether a point is gone at this level.
    pub gone: &'a dyn Fn(&V3) -> bool,

    /// The cells gone by this level (their boxes and planes), to cut platforms with.
    pub cells: Vec<(Aabb, Vec<Plane>)>,
    pub heap: Option<&'a Heap>,
    pub walkable: bool,
    pub collision: (Vec<V3>, Vec<[u32; 3]>),
    pub bounds: Aabb,

    /// A height where ladders dock (a docking line whose point there is gone is left out).
    pub dock_height: f32,
}

/// A piece (`piece01`) and its damage levels (the first is level 1).
pub struct PieceLogic<'a> {
    pub name: String,
    pub levels: Vec<LevelLogic<'a>>,
}

/// The intact level's collision mesh of each piece (by name), for the caller to cut.
///
/// # Errors
///
/// The file doesn't read.
pub fn intact_collisions(bytes: &[u8]) -> Result<HashMap<String, Vec<[V3; 3]>>, String> {
    let v = to_json(bytes)?;
    let mut out = HashMap::new();
    for piece in v["pieces"].as_array().into_iter().flatten() {
        let Some(d) = piece["destructs"].as_array().and_then(|d| d.first()) else { continue };
        let verts = d["collision_3d"]["vertices"].as_array().map(|a| a.iter().map(p3).collect::<Vec<_>>()).unwrap_or_default();
        let tris = d["collision_3d"]["triangles"].as_array().into_iter().flatten().filter_map(|t| {
            let i = |k: &str| t[k].as_i64().and_then(|x| usize::try_from(x).ok()).and_then(|x| verts.get(x)).copied();
            Some([i("vertex_1")?, i("vertex_2")?, i("vertex_3")?])
        }).collect::<Vec<_>>();
        out.insert(piece["name"].as_str().unwrap_or_default().to_owned(), tris);
    }

    Ok(out)
}

/// The logic file with each piece's levels made; notes on what was left out.
///
/// # Errors
///
/// The file doesn't read, or the result doesn't write.
pub fn make_logic(bytes: &[u8], pieces: &[PieceLogic]) -> Result<(Vec<u8>, Vec<String>), String> {
    let mut v = to_json(bytes)?;
    let mut notes = vec![];
    let mut next_id = max_collision_id(&v) + 1;
    let mut all = Aabb::EMPTY;
    for piece in v["pieces"].as_array_mut().into_iter().flatten() {
        let name = piece["name"].as_str().unwrap_or_default().to_owned();
        let Some(pl) = pieces.iter().find(|p| p.name.eq_ignore_ascii_case(&name)) else { continue };
        let Some(template) = piece["destructs"].as_array().and_then(|d| d.first()).cloned() else {
            notes.push(format!("{name}: no levels in the logic file"));
            continue;
        };

        let mut destructs = vec![template.clone()];
        let mut piece_box = Aabb::EMPTY;
        piece_box.merge(&cube(&template["bounding_box"]));
        for (k, lv) in pl.levels.iter().enumerate() {
            let level = k + 1;
            let mut d = template.clone();
            rename(&mut d, &format!("{name}_destruct01"), &format!("{name}_destruct{:02}", level + 1));
            d["index"] = json!(level);
            level_logic(&mut d, lv, &mut notes, &name, level);
            d["collision_3d"]["uk_1"] = json!(next_id);
            next_id += 1;
            piece_box.merge(&lv.bounds);
            destructs.push(d);
        }

        piece["destructs"] = Value::Array(destructs);
        all.merge(&piece_box);
    }

    if !all.is_empty() {
        let mut b = cube(&v["bounding_box"]);
        b.merge(&all);
        v["bounding_box"] = cube_json(&b);
    }

    let mut parsed: Cs2Parsed = serde_json::from_value(v).map_err(|e| e.to_string())?;
    let mut out = vec![];
    parsed.encode(&mut out, &None).map_err(|e| format!("cs2.parsed: {e}"))?;
    Ok((out, notes))
}

fn to_json(bytes: &[u8]) -> Result<Value, String> {
    let parsed = Cs2Parsed::decode(&mut Cursor::new(bytes), &None).map_err(|e| format!("cs2.parsed: {e}"))?;
    serde_json::to_value(&parsed).map_err(|e| e.to_string())
}

fn max_collision_id(v: &Value) -> i64 {
    v["pieces"].as_array().into_iter().flatten()
        .flat_map(|p| p["destructs"].as_array().into_iter().flatten())
        .filter_map(|d| d["collision_3d"]["uk_1"].as_i64())
        .max()
        .unwrap_or(0)
}

/// Every string of a value with `from` replaced by `to` (names of the level's parts).
fn rename(v: &mut Value, from: &str, to: &str) {
    match v {
        Value::String(s) if s.contains(from) => *s = s.replace(from, to),
        Value::Array(a) => a.iter_mut().for_each(|x| rename(x, from, to)),
        Value::Object(o) => o.values_mut().for_each(|x| rename(x, from, to)),
        _ => {}
    }
}

fn level_logic(d: &mut Value, lv: &LevelLogic, notes: &mut Vec<String>, piece: &str, level: usize) {
    let gone = lv.gone;

    // Platforms: each cut by the cells gone; per original platform, its pieces' new indices.
    let old = d["platforms"].as_array().cloned().unwrap_or_default();
    let mut platforms = vec![];
    let mut parts: Vec<Vec<(usize, Vec<V3>)>> = vec![];
    for p in &old {
        let outline = outline(&p["vertices"]);
        let mut mine = vec![];
        for piece in subtract(&outline, &lv.cells) {
            if piece.len() < 3 || newell(&piece).norm() < 0.02 {
                continue;
            }

            let mut q = p.clone();
            q["vertices"] = json!({ "outline": piece.iter().map(point3).collect::<Vec<_>>() });
            mine.push((platforms.len(), piece));
            platforms.push(q);
        }
        parts.push(mine);
    }

    if lv.walkable {
        if let Some(h) = lv.heap {
            for quad in h.platforms(3.0, 0.15, 1.0) {
                let n = newell(&quad).normalize();
                platforms.push(json!({ "flag_1": false, "flag_2": true, "flag_3": false, "normal": point3(&n), "vertices": { "outline": quad.iter().map(point3).collect::<Vec<_>>() } }));
            }
        }
    }

    // Firing lines on what stands, on the piece of their platform under them.
    let lines = d["ef_lines"].as_array().cloned().unwrap_or_default();
    let mut ef = vec![];
    for l in lines {
        let (s, e) = (p3(&l["start"]), p3(&l["end"]));
        let mid = (s + e) * 0.5;
        if gone(&s) || gone(&e) || gone(&mid) {
            continue;
        }

        let parent = l["parent_index"].as_u64().unwrap_or(0) as usize;
        let Some(mine) = parts.get(parent).filter(|m| !m.is_empty()) else { continue };
        let under = mine.iter().find(|(_, poly)| in_xz(poly, &mid)).or_else(|| mine.iter().min_by(|a, b| dist_xz(&a.1, &mid).total_cmp(&dist_xz(&b.1, &mid))));
        let Some((index, _)) = under else { continue };
        let mut q = l.clone();
        q["parent_index"] = json!(index);
        ef.push(q);
    }

    let dropped = d["ef_lines"].as_array().map_or(0, Vec::len) - ef.len();
    if dropped > 0 {
        notes.push(format!("{piece} level {level}: {dropped} firing lines in the damage left out"));
    }

    d["platforms"] = Value::Array(platforms);
    d["ef_lines"] = Value::Array(ef);

    // The hard outline and the footprint, from a raster.
    let hard = d["collision_outlines"].as_array().into_iter().flatten().filter(|o| is_hard(o)).map(|o| xz(&outline(&o["vertices"]))).collect::<Vec<_>>();
    let feet = d["orange_thingies"].as_array().into_iter().flatten().map(|ring| ring.as_array().into_iter().flatten().map(|p| (p["vertex"]["x"].as_f64().unwrap_or(0.0) as f32, p["vertex"]["y"].as_f64().unwrap_or(0.0) as f32)).collect::<Vec<_>>()).collect::<Vec<_>>();
    let blocks = if lv.walkable { None } else { lv.heap };
    let hard_now = footprint(&hard, gone, blocks);
    let feet_now = footprint(&feet, gone, blocks);
    if let Some(outlines) = d["collision_outlines"].as_array_mut() {
        let template = outlines.iter().find(|o| is_hard(o)).cloned();
        outlines.retain(|o| !is_hard(o) && outline(&o["vertices"]).iter().all(|p| !gone(&V3::new(p.x, p.y.max(0.5), p.z))));
        if let Some(t) = template {
            for (i, ring) in hard_now.iter().enumerate() {
                let mut o = t.clone();
                if i > 0 {
                    let n = o["name"].as_str().unwrap_or_default().to_owned();
                    o["name"] = json!(format!("{n}_{:02}", i + 1));
                }

                o["vertices"] = json!({ "outline": ring.iter().map(|&(x, z)| json!({ "x": x, "y": 0.0, "z": z })).collect::<Vec<_>>() });
                outlines.insert(i, o);
            }
        }
    }

    if !feet.is_empty() {
        d["orange_thingies"] = Value::Array(feet_now.iter().map(|ring| Value::Array(ring.iter().map(|&(x, z)| json!({ "vertex": { "x": x, "y": z }, "vertex_type": 2 })).collect())).collect());
    }

    if let Some(pipes) = d.get_mut("pipes").and_then(Value::as_array_mut) {
        pipes.retain(|p| outline(&p["line"]).iter().all(|q| !gone(q)));
    }

    if let Some(lines) = d.get_mut("docking_lines").and_then(Value::as_array_mut) {
        lines.retain(|l| {
            let (s, e) = (p2(&l["start"]), p2(&l["end"]));
            let mid = V3::new((s.0 + e.0) * 0.5, lv.dock_height, (s.1 + e.1) * 0.5);
            !gone(&mid)
        });
    }

    for key in ["arrow_emitters", "soft_collisions", "file_refs", "action_vfx", "action_vfx_attachments"] {
        if let Some(list) = d.get_mut(key).and_then(Value::as_array_mut) {
            list.retain(|x| transform_at(x).is_none_or(|p| !gone(&p)));
        }
    }

    set_collision(&mut d["collision_3d"], &lv.collision);
    if !lv.bounds.is_empty() {
        d["bounding_box"] = cube_json(&lv.bounds);
    }
}

fn is_hard(o: &Value) -> bool {
    o["name"].as_str().is_some_and(|n| n.contains("_hard"))
}

/// The translation of an item's transform (`transform` or `matrix_1`, row-vector: m30..m32), if it has one.
fn transform_at(x: &Value) -> Option<V3> {
    let t = x.get("transform").or_else(|| x.get("matrix_1"))?;
    let f = |k: &str| t.get(k).and_then(Value::as_f64).map(|v| v as f32);
    Some(V3::new(f("m30")?, f("m31")?, f("m32")?))
}

/// A polygon less every cell (convex) it reaches: the parts outside.
pub fn subtract(poly: &[V3], cells: &[(Aabb, Vec<Plane>)]) -> Vec<Vec<V3>> {
    let mut pieces = vec![poly.to_vec()];
    for (b, planes) in cells {
        let mut next = vec![];
        for p in pieces {
            if !b.overlaps(&Aabb::of(p.iter().copied())) || planes.iter().any(|pl| p.iter().all(|q| pl.side(q) >= -1e-5)) {
                next.push(p);
                continue;
            }

            let mut rest = p;
            for pl in planes {
                let outside = clip_polygon(&rest, &pl.flipped());
                if outside.len() >= 3 && newell(&outside).norm() > 1e-4 {
                    next.push(outside);
                }

                rest = clip_polygon(&rest, pl);
                if rest.len() < 3 {
                    break;
                }
            }
        }
        pieces = next;
    }

    pieces
}

/// Polygons (x / z) less what is gone near the ground, plus where the heap stands over half a metre (when it blocks): the outer
/// rings of a 0.25 m raster, simplified, counter-clockwise.
pub fn footprint(rings: &[Vec<(f32, f32)>], gone: &dyn Fn(&V3) -> bool, heap: Option<&Heap>) -> Vec<Vec<(f32, f32)>> {
    if rings.iter().all(|r| r.len() < 3) {
        return vec![];
    }

    let mut b = Aabb::EMPTY;
    for &(x, z) in rings.iter().flatten() {
        b.add(&V3::new(x, 0.0, z));
    }

    if let Some(h) = heap {
        b.add(&V3::new(h.x0, 0.0, h.z0));
        b.add(&V3::new(h.x0 + (h.nx - 1) as f32 * h.step, 0.0, h.z0 + (h.nz - 1) as f32 * h.step));
    }

    let cell = 0.25;
    let (x0, z0) = (b.min.x - cell, b.min.z - cell);
    let nx = ((b.size().x / cell).ceil() as usize + 3).min(2048);
    let nz = ((b.size().z / cell).ceil() as usize + 3).min(2048);
    let mut filled = vec![false; nx * nz];
    for j in 0..nz {
        for i in 0..nx {
            let (x, z) = (x0 + (i as f32 + 0.5) * cell, z0 + (j as f32 + 0.5) * cell);
            let inside = rings.iter().any(|r| in_ring(r, x, z));
            let standing = inside && !gone(&V3::new(x, 1.0, z));
            let heaped = heap.is_some_and(|h| h.height(x, z) > 0.5);
            filled[j * nx + i] = standing || heaped;
        }
    }

    trace(&filled, nx, nz).into_iter().map(|ring| ring.into_iter().map(|(i, j)| (x0 + i as f32 * cell, z0 + j as f32 * cell)).collect::<Vec<_>>())
        .map(|r| simplify(r, 0.15))
        .filter(|r| r.len() >= 3 && area(r) > 1.0)
        .collect()
}

fn in_ring(r: &[(f32, f32)], x: f32, z: f32) -> bool {
    let mut inside = false;
    for k in 0..r.len() {
        let (a, b) = (r[k], r[(k + 1) % r.len()]);
        if (a.1 > z) != (b.1 > z) && x < a.0 + (z - a.1) / (b.1 - a.1) * (b.0 - a.0) {
            inside = !inside;
        }
    }
    inside
}

fn area(r: &[(f32, f32)]) -> f32 {
    (0..r.len()).map(|k| {
        let (a, b) = (r[k], r[(k + 1) % r.len()]);
        a.0 * b.1 - b.0 * a.1
    }).sum::<f32>() * 0.5
}

/// The outer boundaries of a raster's filled cells, as corner coordinates (counter-clockwise in x / z; holes left out).
fn trace(filled: &[bool], nx: usize, nz: usize) -> Vec<Vec<(usize, usize)>> {
    let at = |i: i64, j: i64| i >= 0 && j >= 0 && (i as usize) < nx && (j as usize) < nz && filled[j as usize * nx + i as usize];
    let mut next: HashMap<(usize, usize), Vec<(usize, usize)>> = HashMap::new();
    for j in 0..nz as i64 {
        for i in 0..nx as i64 {
            if !at(i, j) {
                continue;
            }

            let (a, b) = (i as usize, j as usize);
            if !at(i, j - 1) {
                next.entry((a, b)).or_default().push((a + 1, b));
            }
            if !at(i + 1, j) {
                next.entry((a + 1, b)).or_default().push((a + 1, b + 1));
            }
            if !at(i, j + 1) {
                next.entry((a + 1, b + 1)).or_default().push((a, b + 1));
            }
            if !at(i - 1, j) {
                next.entry((a, b + 1)).or_default().push((a, b));
            }
        }
    }

    let mut starts = next.keys().copied().collect::<Vec<_>>();
    starts.sort_unstable();
    let mut rings = vec![];
    for s in starts {
        while next.get(&s).is_some_and(|v| !v.is_empty()) {
            let mut ring = vec![s];
            let mut at_ = s;
            loop {
                let Some(list) = next.get_mut(&at_) else { break };
                let Some(n) = list.pop() else { break };
                if n == s {
                    break;
                }
                ring.push(n);
                at_ = n;
                if ring.len() > nx * nz * 4 {
                    break;
                }
            }

            let r = ring.iter().map(|&(i, j)| (i as f32, j as f32)).collect::<Vec<_>>();
            if ring.len() >= 4 && area(&r) > 0.0 {
                rings.push(ring);
            }
        }
    }

    rings
}

/// A ring with its points that change it by less than `tol` (m) left out (collinear points first).
fn simplify(mut r: Vec<(f32, f32)>, tol: f32) -> Vec<(f32, f32)> {
    loop {
        if r.len() <= 3 {
            return r;
        }

        let n = r.len();
        let (mut best, mut at) = (f32::MAX, 0);
        for k in 0..n {
            let (a, b, c) = (r[(k + n - 1) % n], r[k], r[(k + 1) % n]);
            let len = ((c.0 - a.0).powi(2) + (c.1 - a.1).powi(2)).sqrt().max(1e-6);
            let d = ((c.0 - a.0) * (b.1 - a.1) - (c.1 - a.1) * (b.0 - a.0)).abs() / len;
            if d < best {
                (best, at) = (d, k);
            }
        }

        if best > tol {
            return r;
        }

        r.remove(at);
    }
}

fn set_collision(c: &mut Value, (verts, tris): &(Vec<V3>, Vec<[u32; 3]>)) {
    let mut edges: HashMap<(u32, u32), Vec<usize>> = HashMap::new();
    for (i, t) in tris.iter().enumerate() {
        for k in 0..3 {
            let (a, b) = (t[k], t[(k + 1) % 3]);
            edges.entry((a.min(b), a.max(b))).or_default().push(i);
        }
    }

    let across = |i: usize, a: u32, b: u32| edges.get(&(a.min(b), a.max(b))).and_then(|l| l.iter().find(|&&j| j != i)).map_or(-1, |&j| j as i64);
    c["vertices"] = Value::Array(verts.iter().map(point3).collect());
    c["triangles"] = Value::Array(tris.iter().enumerate().map(|(i, t)| {
        let mut o = serde_json::Map::new();
        o.insert("face_index".into(), json!(i));
        o.insert("padding".into(), json!(0));
        o.insert("vertex_1".into(), json!(t[0]));
        o.insert("vertex_2".into(), json!(t[1]));
        o.insert("vertex_3".into(), json!(t[2]));
        for k in 0..3 {
            let (a, b) = (t[k], t[(k + 1) % 3]);
            o.insert(format!("edge_{}_vertex_1", k + 1), json!(a));
            o.insert(format!("edge_{}_vertex_2", k + 1), json!(b));
            o.insert(format!("face_index_{}", k + 1), json!(i));
            o.insert(format!("zero_{}", k + 1), json!(k));
            o.insert(format!("across_face_index_{}", k + 1), json!(across(i, a, b)));
        }
        o.insert("zero_4".into(), json!(0));
        Value::Object(o)
    }).collect());
}

/// Triangles welded (within a millimetre) into vertices and indices, degenerate ones left out.
pub fn weld(tris: &[[V3; 3]]) -> (Vec<V3>, Vec<[u32; 3]>) {
    let mut map: HashMap<(i64, i64, i64), u32> = HashMap::new();
    let mut verts = vec![];
    let mut out = vec![];
    for t in tris {
        let ids = t.map(|p| {
            let k = ((p.x * 1000.0).round() as i64, (p.y * 1000.0).round() as i64, (p.z * 1000.0).round() as i64);
            *map.entry(k).or_insert_with(|| {
                verts.push(p);
                (verts.len() - 1) as u32
            })
        });
        if ids[0] != ids[1] && ids[1] != ids[2] && ids[0] != ids[2] {
            out.push(ids);
        }
    }
    (verts, out)
}

fn outline(v: &Value) -> Vec<V3> {
    v["outline"].as_array().map(|a| a.iter().map(p3).collect()).unwrap_or_default()
}

fn xz(points: &[V3]) -> Vec<(f32, f32)> {
    points.iter().map(|p| (p.x, p.z)).collect()
}

fn in_xz(poly: &[V3], p: &V3) -> bool {
    in_ring(&xz(poly), p.x, p.z)
}

fn dist_xz(poly: &[V3], p: &V3) -> f32 {
    poly.iter().map(|q| ((q.x - p.x).powi(2) + (q.z - p.z).powi(2)).sqrt()).fold(f32::MAX, f32::min)
}

fn p3(v: &Value) -> V3 {
    V3::new(v["x"].as_f64().unwrap_or(0.0) as f32, v["y"].as_f64().unwrap_or(0.0) as f32, v["z"].as_f64().unwrap_or(0.0) as f32)
}

fn p2(v: &Value) -> (f32, f32) {
    (v["x"].as_f64().unwrap_or(0.0) as f32, v["y"].as_f64().unwrap_or(0.0) as f32)
}

fn point3(p: &V3) -> Value {
    json!({ "x": p.x, "y": p.y, "z": p.z })
}

fn cube(v: &Value) -> Aabb {
    let f = |k: &str| v[k].as_f64().unwrap_or(0.0) as f32;
    Aabb { min: V3::new(f("min_x"), f("min_y"), f("min_z")), max: V3::new(f("max_x"), f("max_y"), f("max_z")) }
}

fn cube_json(b: &Aabb) -> Value {
    json!({ "min_x": b.min.x, "min_y": b.min.y, "min_z": b.min.z, "max_x": b.max.x, "max_y": b.max.y, "max_z": b.max.z })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_platform_cut_by_a_cell_and_a_footprint_with_a_breach() {
        let square = vec![V3::new(0.0, 15.0, 0.0), V3::new(0.0, 15.0, 4.0), V3::new(20.0, 15.0, 4.0), V3::new(20.0, 15.0, 0.0)];
        // A cell: the slab 8 < x < 12.
        let planes = vec![Plane { n: V3::x(), d: 12.0 }, Plane { n: -V3::x(), d: -8.0 }];
        let b = Aabb { min: V3::new(8.0, 0.0, -10.0), max: V3::new(12.0, 20.0, 10.0) };
        let pieces = subtract(&square, &[(b, planes)]);
        let total = pieces.iter().map(|p| newell(p).norm() * 0.5).sum::<f32>();
        assert_eq!(pieces.len(), 2);
        assert!((total - 64.0).abs() < 1e-3, "{total}");

        let ring = vec![(0.0, 0.0), (20.0, 0.0), (20.0, 4.0), (0.0, 4.0)];
        let gone = |p: &V3| p.x > 8.0 && p.x < 12.0;
        let f = footprint(&[ring], &gone, None);
        assert_eq!(f.len(), 2, "the breach splits the footprint: {f:?}");
        assert!(f.iter().all(|r| area(r) > 0.0 && r.len() <= 8));

        let (v, t) = weld(&[[V3::zeros(), V3::x(), V3::y()], [V3::x(), V3::new(1.0, 1.0, 0.0), V3::y()]]);
        assert_eq!((v.len(), t.len()), (4, 2));
    }
}
