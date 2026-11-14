//! A piece's ends (`ends`): the end caps of its logic, paired across the piece.

use super::*;

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
/// the wall by 2.5 m across (TDD's gondor pieces 2.5 by 1.45) (the kit's Roman walls, towers, corners, joiners, gateways, stepped bastions; 2026-10-31). The joint is the
/// cap's side across the wall that is farther from the main piece's platforms; the wall leaves through it. Empty when no cap is found.
///
/// # Errors
///
/// The file doesn't read.
pub fn ends(bytes: &[u8]) -> Result<Vec<PieceEnd>, String> {
    let parsed = Cs2Parsed::decode(&mut Cursor::new(bytes), &None).map_err(|e| format!("cs2.parsed: {e}"))?;
    let v = serde_json::to_value(&parsed).map_err(|e| e.to_string())?;
    let pt =
        |p: &Value| [p["x"].as_f64().unwrap_or(0.0) as f32, p["y"].as_f64().unwrap_or(0.0) as f32, p["z"].as_f64().unwrap_or(0.0) as f32];
    // The intact level: index 0 (a piece's levels are not always listed in order).
    let level0 = |piece: &Value| {
        let levels = piece["destructs"].as_array().cloned().unwrap_or_default();
        levels.iter().find(|d| d["index"].as_u64() == Some(0)).or(levels.first()).cloned().unwrap_or(Value::Null)
    };
    let platforms = |level: &Value| -> Vec<Vec<[f32; 3]>> {
        level["platforms"].as_array().map_or(vec![], |ps| {
            ps.iter().map(|p| p["vertices"]["outline"].as_array().map_or(vec![], |o| o.iter().map(pt).collect())).collect()
        })
    };

    let pieces = v["pieces"].as_array().cloned().unwrap_or_default();
    let Some(main) = pieces.first() else { return Ok(vec![]) };
    let body: Vec<[f32; 3]> = platforms(&level0(main)).into_iter().flatten().collect();
    if body.is_empty() {
        return Ok(vec![]);
    }

    let centre = body.iter().fold([0.0f32; 2], |c, p| [c[0] + p[0], c[1] + p[2]]).map(|c| c / body.len() as f32);
    let flat: Vec<[f32; 2]> = body.iter().map(|p| [p[0], p[2]]).collect();
    let edges = platforms(&level0(main))
        .iter()
        .flat_map(|r| (0..r.len()).map(|i| ([r[i][0], r[i][2]], [r[(i + 1) % r.len()][0], r[(i + 1) % r.len()][2]])).collect::<Vec<_>>())
        .collect::<Vec<_>>();
    // Every platform's corners (the caps' too): what a joint's line has on one side.
    let caps = pieces.iter().skip(1).flat_map(|piece| platforms(&level0(piece))).collect::<Vec<_>>();
    let all = flat.iter().copied().chain(caps.iter().flatten().map(|p| [p[0], p[2]])).collect::<Vec<_>>();
    // A piece with two caps (a wall piece: a straight, a corner, a joiner): the pair of joints, one a side of each cap on the piece's
    // boundary, the farthest apart that does not turn back on itself. Every wrong pair of the game's and TDD's pieces is shorter (a 45
    // degree joiner's arms are shorter than its walk is wide, a tower's walk is cut by the tower, TDD's gondor caps are 2.5 along the wall
    // by 1.45 across: no rule on one cap alone held for all of them; 2026-11-12) or a U-turn.
    //
    // Of the pairs nearly as far apart as the farthest ([`PAIR_NEAR`]), the one whose sides the walk's end continues (`continued_by`: the
    // shortest such edge of the cap's sides, the walk's width, not an arm's length): a Roman external corner's two caps each have a side on
    // the outer face along an arm, and the pair of those is farther apart (26.2 to 25.8) than its joints. Not further: a 45 degree joiner's
    // arms are shorter than its walk is wide, and its joints' pair is the farther by 8.3 to 7.4 (2026-11-12, docs/plans/kit_walls.md phase 3).
    let four = caps.iter().filter(|c| c.len() == 4).cloned().collect::<Vec<_>>();
    if let [c1, c2] = four.as_slice() {
        let ends = |c: &Vec<[f32; 3]>| {
            let sides = cap_candidates(c, &all);
            let lens = sides.iter().map(|s| continued_by(s.0, s.1, &edges)).collect::<Vec<_>>();
            let shortest = lens.iter().flatten().copied().fold(f32::MAX, f32::min);
            sides
                .into_iter()
                .zip(lens)
                .filter_map(|(s, l)| {
                    let walk_end = shortest < f32::MAX && l.is_some_and(|x| x <= shortest + 0.5);
                    cap_end(c, centre, &flat, &edges, &all, Some(s)).map(|e| (e, walk_end))
                })
                .collect::<Vec<_>>()
        };
        let (e1, e2) = (ends(c1), ends(c2));
        let span = |p: &(&(PieceEnd, bool), &(PieceEnd, bool))| (p.0.0.at[0] - p.1.0.at[0]).hypot(p.0.0.at[2] - p.1.0.at[2]);
        let pairs = e1
            .iter()
            .flat_map(|a| e2.iter().map(move |b| (a, b)))
            .filter(|(a, b)| a.0.out[0] * b.0.out[0] + a.0.out[1] * b.0.out[1] < 0.5)
            .collect::<Vec<_>>();
        let far = pairs.iter().map(span).fold(0.0f32, f32::max);
        let best = pairs.into_iter().filter(|p| span(p) >= far * PAIR_NEAR).max_by(|x, y| {
            let walk_ends = |p: &(&(PieceEnd, bool), &(PieceEnd, bool))| u8::from(p.0.1) + u8::from(p.1.1);
            walk_ends(x).cmp(&walk_ends(y)).then(span(x).total_cmp(&span(y)))
        });
        if let Some((a, b)) = best {
            return Ok(vec![a.0, b.0]);
        }
    }

    let mut out = vec![];
    for piece in pieces.iter().skip(1) {
        for cap in platforms(&level0(piece)).into_iter().filter(|c| c.len() == 4) {
            out.extend(cap_end(&cap, centre, &flat, &edges, &all, None));
        }
    }

    Ok(out)
}

/// The joint of an end cap (4 points): its side across the wall (about 2.5 m) that is farthest from `centre` (x, z); the wall leaves
/// through it, square to it. The caps are 2 m by 2.5 m rectangles, or a stepped bastion's quadrilateral with a slanted inner side. Its outer
/// side: where the cap lies across the wall against the main piece's wall-walk on the joint line (`body`, x and z of the main piece's
/// platform points), else against `centre`.
/// A cap's sides on the piece's own boundary (every platform of the piece, caps included, on one side of its line): where its joint can
/// be. All its sides when none is.
pub(super) fn cap_candidates(cap: &[[f32; 3]], all: &[[f32; 2]]) -> Vec<([f32; 3], [f32; 3])> {
    let len = |a: [f32; 3], b: [f32; 3]| (b[0] - a[0]).hypot(b[2] - a[2]);
    let sides = (0..4).map(|i| (cap[i], cap[(i + 1) % 4])).filter(|&(a, b)| len(a, b) > 0.5).collect::<Vec<_>>();
    let supporting = |a: [f32; 3], b: [f32; 3]| {
        let l = len(a, b).max(1e-6);
        let n = [(b[2] - a[2]) / l, -(b[0] - a[0]) / l];
        let (lo, hi) = all
            .iter()
            .map(|q| (q[0] - a[0]) * n[0] + (q[1] - a[2]) * n[1])
            .fold((f32::MAX, f32::MIN), |(lo, hi), x| (lo.min(x), hi.max(x)));
        lo >= -0.05 || hi <= 0.05
    };
    let on = sides.iter().copied().filter(|&(a, b)| supporting(a, b)).collect::<Vec<_>>();
    if on.is_empty() { sides } else { on }
}

/// A pair of a two-capped piece's cap sides at least this part of the farthest pair's span apart is weighed by its sides (`ends`).
pub(super) const PAIR_NEAR: f32 = 0.97;

/// The longest of the main piece's platform edges (x, z) that continues a cap's side `a`-`b` on its line and touches it: the walk's end
/// (as wide as the walk) for a joint, an arm's long edge for a side along the wall. `None` when none does.
pub(super) fn continued_by(a: [f32; 3], b: [f32; 3], body_edges: &[([f32; 2], [f32; 2])]) -> Option<f32> {
    let (ux, uz) = (b[0] - a[0], b[2] - a[2]);
    let l = ux.hypot(uz).max(1e-6);
    let n = [uz / l, -ux / l];
    let along = |q: [f32; 2]| ((q[0] - a[0]) * ux + (q[1] - a[2]) * uz) / l;
    body_edges
        .iter()
        .filter(|(c, e)| {
            let on = |q: &[f32; 2]| ((q[0] - a[0]) * n[0] + (q[1] - a[2]) * n[1]).abs() < 0.05;
            let (tc, te) = (along(*c), along(*e));
            on(c) && on(e) && tc.min(te) <= l + 0.05 && tc.max(te) >= -0.05
        })
        .map(|(c, e)| (e[0] - c[0]).hypot(e[1] - c[1]))
        .fold(None, |m: Option<f32>, x| Some(m.map_or(x, |m| m.max(x))))
}

/// `joint`: the side to take as the joint (the pair of a two-capped piece's, `ends`), else chosen here.
pub(super) fn cap_end(
    cap: &[[f32; 3]],
    centre: [f32; 2],
    body: &[[f32; 2]],
    body_edges: &[([f32; 2], [f32; 2])],
    all: &[[f32; 2]],
    joint: Option<([f32; 3], [f32; 3])>,
) -> Option<PieceEnd> {
    let len = |a: [f32; 3], b: [f32; 3]| (b[0] - a[0]).hypot(b[2] - a[2]);
    let mid = |a: [f32; 3], b: [f32; 3]| [(a[0] + b[0]) * 0.5, (a[1] + b[1]) * 0.5, (a[2] + b[2]) * 0.5];
    let d = |m: [f32; 3]| (m[0] - centre[0]).hypot(m[2] - centre[1]);
    let span = (0..4).flat_map(|i| (i + 1..4).map(move |j| (i, j))).map(|(i, j)| len(cap[i], cap[j])).fold(0.0f32, f32::max);
    if !(2.0..=4.5).contains(&span) {
        return None;
    }

    // The joint (2026-11-12, every cap of the game's and TDD's fort pieces): a side of the cap on the piece's own boundary (every platform
    // of the piece, caps included, on one side of its line), the one the wall-walk's end continues (below); the farthest from the centre
    // when two do as well. By length alone (the game's
    // caps are 2 m along the wall by 2.5 across) TDD's gondor caps (2.5 along by 1.45 across) turned out of their outer side, and an
    // internal corner's arm read as a straight.
    let sides = (0..4).map(|i| (cap[i], cap[(i + 1) % 4])).filter(|&(a, b)| len(a, b) > 0.5).collect::<Vec<_>>();
    let normal = |a: [f32; 3], b: [f32; 3]| {
        let (ux, uz) = (b[0] - a[0], b[2] - a[2]);
        let n = ux.hypot(uz).max(1e-6);
        [uz / n, -ux / n]
    };
    let supporting = |a: [f32; 3], b: [f32; 3]| {
        let n = normal(a, b);
        let v = all.iter().map(|q| (q[0] - a[0]) * n[0] + (q[1] - a[2]) * n[1]);
        let (lo, hi) = v.fold((f32::MAX, f32::MIN), |(lo, hi), x| (lo.min(x), hi.max(x)));
        lo >= -0.05 || hi <= 0.05
    };
    // The wall-walk's edge that continues a side on its line, touching it: the walk's end (as wide as the walk) for the joint, the long
    // edge of an arm for the side along the wall. The joint is the side whose edge is the shortest.
    let continued = |a: [f32; 3], b: [f32; 3]| continued_by(a, b, body_edges);
    let mut cands = sides.iter().copied().filter(|&(a, b)| supporting(a, b)).collect::<Vec<_>>();
    if cands.is_empty() {
        cands = sides.clone();
    }

    // No edge continues any (the 45 degree joiners' caps): the side of about 2.5 m, the game's caps' joint, if one is a candidate.
    let shortest = cands.iter().filter_map(|s| continued(s.0, s.1)).fold(f32::MAX, f32::min);
    if shortest == f32::MAX && cands.iter().any(|s| (2.2..=2.8).contains(&len(s.0, s.1))) {
        cands.retain(|s| (2.2..=2.8).contains(&len(s.0, s.1)));
    }

    let joint = match joint {
        Some(j) => j,
        None => cands
            .into_iter()
            .filter(|s| shortest == f32::MAX || continued(s.0, s.1).is_some_and(|x| x <= shortest + 0.5))
            .max_by(|x, y| d(mid(x.0, x.1)).total_cmp(&d(mid(y.0, y.1))))?,
    };
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
    let side = if k.abs() < 0.25 {
        [0.0, 0.0]
    } else if k > 0.0 {
        across
    } else {
        [-across[0], -across[1]]
    };
    Some(PieceEnd { at: outer, out, side })
}
