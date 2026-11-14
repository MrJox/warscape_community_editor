//! The walkway platforms of a warped piece: carved where the ground drops, cut into strips, welded and merged, wound as the game winds them.

use super::*;

/// A strip of a platform: its index among the new platforms, the x range it covers before the warp, and its outline on the ground (x,
/// z) before the warp.
pub(super) struct Strip {
    index: usize,
    x: (f32, f32),
    ground: Vec<[f32; 2]>,
}

/// Platforms less the boxes the steps replace (`drops`: their x ranges; `zy`: the cross-section, z then y; none: all of it), as the
/// pieces of each platform outside them (x before the range, after it, and within it the z on either side of the box). A platform wholly
/// outside every box is kept as it is. Per platform, its pieces.
pub(super) fn carve_platforms(old: &[Value], drops: &[(f32, f32)], zy: Option<((f32, f32), (f32, f32))>) -> Vec<Vec<Value>> {
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
                next.extend([
                    half_plane(&poly, 0, a, true),
                    half_plane(&poly, 0, b, false),
                    half_plane(&mid, 2, z0, true),
                    half_plane(&mid, 2, z1, false),
                ]);
            }

            pieces = next.into_iter().filter(|q| q.len() >= 3 && area_xz(q) > 1e-4).collect();
        }

        out.push(
            pieces
                .into_iter()
                .map(|poly| {
                    let mut q = p.clone();
                    q["vertices"] = json!({ "outline": poly.iter().map(|&v| point3(v)).collect::<Vec<_>>() });
                    q
                })
                .collect(),
        );
    }

    out
}

/// A polygon cut by the plane `axis` = `at`, the side below it (`below`) or above it kept (Sutherland-Hodgman).
pub(super) fn half_plane(poly: &[[f32; 3]], axis: usize, at: f32, below: bool) -> Vec<[f32; 3]> {
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
pub(super) fn area_xz(poly: &[[f32; 3]]) -> f32 {
    (0..poly.len())
        .map(|i| {
            let (a, b) = (poly[i], poly[(i + 1) % poly.len()]);
            a[0] * b[2] - b[0] * a[2]
        })
        .sum::<f32>()
        .abs()
        / 2.0
}

/// Platforms (each given as its pieces) cut along x and warped, the strips whose middle `keep` refuses left out; per original platform,
/// the strips of all its pieces (indices from `offset`).
pub(super) fn platform_strips(
    old: &[Vec<Value>],
    warp: &dyn Warp,
    cuts: &Cuts,
    keep: &dyn Fn([f32; 3]) -> bool,
    offset: usize,
    notes: &mut LogicNotes,
) -> (Vec<Value>, Vec<Vec<Strip>>) {
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
pub(super) const FACE_GRID: f32 = 0.5;

/// A platform's warped points (`w`, from the source's `s`) with those on a corner's faces on one grid along the face, measured from the
/// corner: each slid along the face to the nearest multiple of `FACE_GRID`, and a point at every multiple along an edge that runs on
/// the face. Both pieces of a corner, whose faces are the same line, then have the same points on it, and their platforms share edges
/// across it (user 2026-11-05: the base's slanted platform edges crossed each face at points of its own, and a grid of depths across
/// each arm drifted apart where the arms meet the bisector at different angles; the corner's two sides shared no edge).
pub(super) fn on_face_grid(s: &[[f32; 3]], w: &[[f32; 3]], faces: &[CornerFace]) -> Vec<[f32; 3]> {
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
pub(super) fn place_lines(
    old: &[Value],
    warp: &dyn Warp,
    cuts: &Cuts,
    strips: &[Vec<Strip>],
    keep: &dyn Fn([f32; 3]) -> bool,
    notes: &mut LogicNotes,
) -> Vec<Value> {
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
    let parts = parts
        .into_iter()
        .filter_map(|mut l| {
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
        })
        .collect::<Vec<_>>();

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
        let inside = mine
            .iter()
            .find(|s| in_ring(&s.ground, [mid[0], mid[2]]))
            .or_else(|| mine.iter().find(|s| s.x.0 - 1e-3 <= mid[0] && mid[0] <= s.x.1 + 1e-3));
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
pub(super) fn in_ring(ring: &[[f32; 2]], p: [f32; 2]) -> bool {
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

pub(super) fn gap(s: &Strip, x: f32) -> f32 {
    (s.x.0 - x).max(x - s.x.1).max(0.0)
}

/// A 3D outline (`{"outline": [...]}`) with points along its edges, warped; closed when it has more than two points.
pub(super) fn warp_outline(o: &mut Value, warp: &dyn Warp, step: f32, planes: &[[f32; 3]]) {
    let points = outline(o);
    let closed = points.len() > 2;
    let points = clip_to(&points, planes, closed);
    let out = subdivide(&points, closed, step).into_iter().map(|(p, _)| point3(warp.point(p))).collect::<Vec<_>>();
    o["outline"] = Value::Array(out);
}

/// A footprint ring (`[{"vertex": {x, y}, "vertex_type"}]`, y being z) with points along its edges, warped; an added point takes the type
/// of the point that starts its edge.
pub(super) fn warp_footprint(ring: &mut Value, warp: &dyn Warp, step: f32, planes: &[[f32; 3]]) {
    let Some(old) = ring.as_array().cloned() else { return };
    let points = old
        .iter()
        .map(|v| {
            let p = p2(&v["vertex"]);
            [p[0], 0.0, p[1]]
        })
        .collect::<Vec<_>>();
    let points = clip_to(&points, planes, points.len() > 2);
    let kind = old.first().cloned().unwrap_or(Value::Null);
    let out = subdivide(&points, points.len() > 2, step)
        .into_iter()
        .map(|(p, from)| {
            let q = warp.point(p);
            let mut v = old.get(from).cloned().unwrap_or_else(|| kind.clone());
            v["vertex"] = json!({ "x": q[0], "y": q[2] });
            v
        })
        .collect();
    *ring = Value::Array(out);
}

/// How near two platform corners are one, and a corner is on another platform's edge.
pub(super) const WELD: f32 = 0.01;

/// A level's platforms joined as the game's are, edge to edge (the game connects platforms on shared edges: the stepped bastion's stair
/// shares its top edge's two corners with the landing; ours met the walkway's pieces partway along their edges, and units did not go
/// from one to the other; user 2026-11-05): corners within `WELD` of each other become one point, and a corner on another platform's
/// edge becomes a corner of that edge too. Their order, their count and everything else is kept.
pub(super) fn weld_platforms(platforms: &mut [Value]) {
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

            let mut on = others
                .iter()
                .filter_map(|&p| {
                    let t = ((p[0] - a[0]) * d[0] + (p[1] - a[1]) * d[1] + (p[2] - a[2]) * d[2]) / len2;
                    let off = [a[0] + d[0] * t - p[0], a[1] + d[1] * t - p[1], a[2] + d[2] * t - p[2]];
                    let dist = (off[0] * off[0] + off[1] * off[1] + off[2] * off[2]).sqrt();
                    (t > 1e-4 && t < 1.0 - 1e-4 && dist <= WELD).then_some((t, p))
                })
                .collect::<Vec<_>>();
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

/// A level's neighbouring platforms merged where they lie in one plane, have the same flags, share edges and stay convex, the thinnest
/// first, their firing lines renumbered. The game's pieces have few, solid, convex platforms (2530 in the fort pieces: none concave, 11
/// under 0.2 across); cutting along a wall left hundreds of strips and slivers, and at a corner soldiers spun from one to another or
/// fell where they were thinnest (user 2026-11-05). Corners on the outline stay, even in a straight line: other platforms share them.
pub(super) fn merge_platforms(d: &mut Value) {
    let Some(mut list) = d["platforms"].as_array().cloned() else { return };
    let n0 = list.len();
    let mut map: Vec<usize> = (0..n0).collect();
    let mut outlines = list.iter().map(|p| outline(&p["vertices"])).collect::<Vec<_>>();
    let mut alive = vec![true; n0];
    let flags = |p: &Value| (p["flag_1"].as_bool(), p["flag_2"].as_bool(), p["flag_3"].as_bool());
    let width = |o: &[[f32; 3]]| {
        let per = (0..o.len())
            .map(|i| {
                let (a, b) = (o[i], o[(i + 1) % o.len()]);
                (b[0] - a[0]).hypot(b[2] - a[2])
            })
            .sum::<f32>();
        2.0 * area_xz(o) / per.max(1e-6)
    };

    loop {
        let mut order = (0..n0).filter(|&i| alive[i]).collect::<Vec<_>>();
        order.sort_by(|&a, &b| width(&outlines[a]).total_cmp(&width(&outlines[b])));
        let mut merged = false;
        'all: for &i in &order {
            let mut partners = order.iter().copied().filter(|&j| j != i && flags(&list[i]) == flags(&list[j])).collect::<Vec<_>>();
            partners.sort_by(|&a, &b| width(&outlines[a]).total_cmp(&width(&outlines[b])));
            for j in partners {
                if let Some(o) = merged_outline(&outlines[i], &outlines[j]) {
                    outlines[i] = o;
                    alive[j] = false;
                    map.iter_mut().filter(|m| **m == j).for_each(|m| *m = i);
                    merged = true;
                    break 'all;
                }
            }
        }

        if !merged {
            break;
        }
    }

    // What is left, in order.
    let mut index = vec![0; n0];
    let mut kept = vec![];
    for i in (0..n0).filter(|&i| alive[i]) {
        index[i] = kept.len();
        let mut p = std::mem::take(&mut list[i]);
        p["vertices"] = json!({ "outline": outlines[i].iter().map(|&v| point3(v)).collect::<Vec<_>>() });
        kept.push(p);
    }

    for l in array_mut(d, "ef_lines") {
        if let Some(&m) = map.get(l["parent_index"].as_u64().unwrap_or(0) as usize) {
            l["parent_index"] = json!(index[m]);
        }
    }

    d["platforms"] = Value::Array(kept);
}

/// Two platforms' outline as one, when they lie in one plane, share one run of edges (each the other way round) and the result is convex.
pub(super) fn merged_outline(a: &[[f32; 3]], b: &[[f32; 3]]) -> Option<Vec<[f32; 3]>> {
    const NEAR: f32 = 1e-3;
    let same = |p: [f32; 3], q: [f32; 3]| (0..3).all(|k| (p[k] - q[k]).abs() < NEAR);

    // In one plane: b's points on a's.
    let n = normalised(newell(a));
    if n.iter().all(|v| v.abs() < 1e-6)
        || b.iter().any(|q| ((q[0] - a[0][0]) * n[0] + (q[1] - a[0][1]) * n[1] + (q[2] - a[0][2]) * n[2]).abs() > 2e-3)
    {
        return None;
    }

    // The directed edges of both less those that meet their reverse; what is left must be one loop.
    let edges = a
        .iter()
        .enumerate()
        .map(|(i, &p)| (p, a[(i + 1) % a.len()]))
        .chain(b.iter().enumerate().map(|(i, &p)| (p, b[(i + 1) % b.len()])))
        .collect::<Vec<_>>();
    let open = edges.iter().copied().filter(|&(p, q)| !edges.iter().any(|&(u, v)| same(p, v) && same(q, u))).collect::<Vec<_>>();
    if open.len() == edges.len() || open.len() < 3 {
        return None;
    }

    let mut out = vec![open[0].0];
    let mut at = open[0].1;
    while !same(at, out[0]) {
        if out.len() > open.len() {
            return None;
        }

        out.push(at);
        at = open.iter().find(|e| same(e.0, at))?.1;
    }

    if out.len() != open.len() {
        return None;
    }

    // Convex, either way round, with corners in a straight line allowed; going round once.
    let turns = (0..out.len())
        .map(|i| {
            let (p, q, r) = (out[(i + out.len() - 1) % out.len()], out[i], out[(i + 1) % out.len()]);
            let (u, v) = ([q[0] - p[0], q[2] - p[2]], [r[0] - q[0], r[2] - q[2]]);
            (u[0] * v[1] - u[1] * v[0]) / (u[0].hypot(u[1]) * v[0].hypot(v[1])).max(1e-9)
        })
        .collect::<Vec<_>>();
    let convex = turns.iter().all(|&t| t <= CONVEX) || turns.iter().all(|&t| t >= -CONVEX);
    convex.then_some(out)
}

/// How much (the sine of the angle) a merged platform's outline may turn the wrong way at a corner: float noise on a straight edge.
pub(super) const CONVEX: f32 = 2e-3;

/// A platform's corners in the order the game's go round (every platform of the game's pieces, its stairs too: clockwise seen from
/// above, a negative area over x and z). Two neighbours then run their shared edge opposite ways; the stairs, made the other way round,
/// were the only platforms that did not, and units never stepped onto them (user 2026-11-05).
pub(crate) fn wind_as_the_game(p: &mut Value) {
    let o = outline(&p["vertices"]);
    let area = (0..o.len())
        .map(|i| {
            let (a, b) = (o[(i + o.len() - 1) % o.len()], o[i]);
            a[0] * b[2] - b[0] * a[2]
        })
        .sum::<f32>();
    if area > 0.0 {
        p["vertices"] = json!({ "outline": o.into_iter().rev().map(point3).collect::<Vec<_>>() });
    }
}

/// A level's platforms the weld folded (a sliver: fewer than 3 corners, or no area) taken out, its firing lines' platforms renumbered
/// (a line on a folded one goes to the platform under its middle, else the nearest). Pipes name no platforms (a polyline and its type).
pub(super) fn drop_folded(d: &mut Value) {
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
