//! The collision of a warped piece: its polygons, the caps that close its hull, their triangles, written back with the bounding box grown.

use super::*;

/// A collision hull (`collision_3d`: vertices, triangles) as polygons cut along x at `cuts`, cut to the half-planes, warped. A cut through
/// an edge gives the same point to both triangles on it (worked out from the edge's ends in one order), so the hull stays closed. Under
/// each `sinks` stair (its x range, the heights lowered) inside `sink_z`, the hull goes down by `SINK` from `SINK_IN` in from its ends.
pub(super) fn collision_polygons(
    c: &Value,
    warp: &dyn Warp,
    cuts: &Cuts,
    planes: &[[f32; 3]],
    sinks: &[((f32, f32), (f32, f32))],
    sink_z: Option<(f32, f32)>,
) -> Vec<(usize, Vec<[f32; 3]>)> {
    let verts = c["vertices"].as_array().map(|a| a.iter().map(p3).collect::<Vec<_>>()).unwrap_or_default();
    let tris = c["triangles"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|t| {
                    let i = |k: &str| t[k].as_u64().map(|v| v as usize).filter(|&v| v < verts.len());
                    Some([verts[i("vertex_1")?], verts[i("vertex_2")?], verts[i("vertex_3")?]])
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
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
        polys = polys
            .into_iter()
            .flat_map(|(i, p)| [(i, cut(&p, &|q| q[0] - x)), (i, cut(&p, &|q| x - q[0]))])
            .filter(|p| p.1.len() >= 3)
            .collect();
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
        let under = sinks.iter().any(|&((a, b), (y0, y1))| {
            q[0] > a + SINK_IN - 1e-3 && q[0] < b - SINK_IN + 1e-3 && b - a > 2.0 * SINK_IN && q[1] >= y0 && q[1] <= y1
        });
        if under && q[2] >= z0 && q[2] <= z1 { [q[0], q[1] - SINK, q[2]] } else { q }
    };

    polys.into_iter().map(|(i, p)| (i, p.into_iter().map(|q| warp.point(sunk(q))).collect())).collect()
}

/// The caps that close a hull's polygons where `planes` cut them: the edges only one polygon has, lying on a plane, chained into loops,
/// each filled (ear clipping in the plane) facing out of the kept side (`a x + b z <= c`). Each cap takes the source triangle of the
/// polygon at its first edge.
pub(super) fn hull_caps(polys: &[(usize, Vec<[f32; 3]>)], planes: &[[f32; 3]]) -> Vec<(usize, Vec<[f32; 3]>)> {
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
                let Some(n) = (0..open.len()).find(|&j| !used[j] && (key(open[j].0) == key(at) || key(open[j].1) == key(at))) else {
                    break;
                };
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

                let Some((ti, e)) = tris
                    .iter()
                    .enumerate()
                    .find_map(|(ti, t)| (0..3).find(|&e| on_seg(flat[p], flat[t[e]], flat[t[(e + 1) % 3]])).map(|e| (ti, e)))
                else {
                    continue;
                };
                let t = tris.remove(ti);
                let (u, v, w) = (t[e], t[(e + 1) % 3], t[(e + 2) % 3]);
                tris.push([u, p, w]);
                tris.push([p, v, w]);
            }

            // A loop that is not simple (it crosses itself) is not cut whole: the cap has a hole soldiers drop through. Said in the log,
            // by the area the triangles cover against the loop's own.
            let area = |q: &[[f32; 2]]| {
                (0..q.len()).map(|i| q[i][0] * q[(i + 1) % q.len()][1] - q[(i + 1) % q.len()][0] * q[i][1]).sum::<f32>().abs() * 0.5
            };
            let covered = tris.iter().map(|t| area(&[flat[t[0]], flat[t[1]], flat[t[2]]])).sum::<f32>();
            let whole = area(&flat);
            if whole > 1e-3 && covered < whole * 0.99 {
                wce_foundation::log::warn(format!(
                    "procedural walls: a collision cap of {} points is cut only in part ({:.0} % of its area): its loop crosses itself",
                    flat.len(),
                    covered / whole * 100.0
                ));
            }

            for [i, j, k] in tris {
                out.push((src, vec![lp[i], lp[j], lp[k]]));
            }
        }
    }

    out
}

/// A simple polygon (2D, either way round) cut into triangles (ear clipping): indices into it, in its order of turning.
pub(super) fn ear_clip_indices(points: &[[f32; 2]]) -> Vec<[usize; 3]> {
    let cross = |a: [f32; 2], b: [f32; 2], c: [f32; 2]| (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0]);
    let mut idx = (0..points.len()).collect::<Vec<_>>();
    let area = (0..points.len())
        .map(|i| {
            let (p, q) = (points[i], points[(i + 1) % points.len()]);
            p[0] * q[1] - q[0] * p[1]
        })
        .sum::<f32>();
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

        let empty = turn > 0.0
            && idx.iter().all(|&j| {
                let p = points[j];
                j == ia
                    || j == ib
                    || j == ic
                    || p == a
                    || p == b
                    || p == c
                    || !(cross(a, b, p) * sign >= 0.0 && cross(b, c, p) * sign >= 0.0 && cross(c, a, p) * sign >= 0.0)
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
pub(super) const HULL_WELD: f32 = 0.006;

/// A collision hull written from polygons (triangle fans; degenerate triangles left out): its vertices shared (within `HULL_WELD`), every triangle's edges and the triangles across them (-1 on an open edge), as the game's files have them. Per triangle
/// written, the polygon it is part of.
pub(super) fn write_collision(c: &mut Value, polys: &[Vec<[f32; 3]>]) -> Vec<usize> {
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
                    if let Some(&found) = index
                        .get(&[c[0] + dx, c[1] + dy, c[2] + dz])
                        .and_then(|l| l.iter().find(|&&i| (0..3).all(|k| (verts[i][k] - p[k]).abs() <= HULL_WELD)))
                    {
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

    let triangles = tris
        .iter()
        .enumerate()
        .map(|(f, t)| {
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
        })
        .collect::<Vec<_>>();

    // The box: kept empty when the game's is (a hull's box is all zero in the files seen), else around the hull.
    let empty = ["min_x", "min_y", "min_z", "max_x", "max_y", "max_z"].iter().all(|k| c["bounding_box"][*k].as_f64().unwrap_or(0.0) == 0.0);
    if !empty && !verts.is_empty() {
        let (lo, hi) = verts.iter().fold(([f32::MAX; 3], [f32::MIN; 3]), |(lo, hi), p| {
            ([lo[0].min(p[0]), lo[1].min(p[1]), lo[2].min(p[2])], [hi[0].max(p[0]), hi[1].max(p[1]), hi[2].max(p[2])])
        });
        c["bounding_box"] = json!({ "min_x": lo[0], "min_y": lo[1], "min_z": lo[2], "max_x": hi[0], "max_y": hi[1], "max_z": hi[2] });
    }

    c["vertices"] = Value::Array(verts.into_iter().map(point3).collect());
    c["triangles"] = Value::Array(triangles);
    from
}

/// A box (`min_x` .. `max_z`) grown to hold these points.
pub(super) fn grow_box(b: &mut Value, pts: &[[f32; 3]]) {
    for (i, (lo, hi)) in [("min_x", "max_x"), ("min_y", "max_y"), ("min_z", "max_z")].into_iter().enumerate() {
        let (a, z) = pts
            .iter()
            .fold((b[lo].as_f64().unwrap_or(0.0) as f32, b[hi].as_f64().unwrap_or(0.0) as f32), |(a, z), p| (a.min(p[i]), z.max(p[i])));
        b[lo] = json!(a);
        b[hi] = json!(z);
    }
}
