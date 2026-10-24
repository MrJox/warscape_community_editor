//! What the game needs of a layout (docs/re/siege_ai.md, "The engine"), checked. The game reports nothing when a graph is broken, so
//! this is the only warning there is. Runs on CA's layouts too (the check example), to tell our rules from theirs.

use crate::raster::{area2, dist, inside};
use crate::{Capture, EdgeKind, Layout, NodeKind, P};

#[derive(Clone, Debug, Default)]
pub struct Report {
    /// What breaks the game's reading of the graph.
    pub problems: Vec<String>,

    /// What CA's layouts never do.
    pub notes: Vec<String>,
}

/// A node's rectangle as the game reads it (`FUN_102262a0`): centre the corners' mean, axis p3 -> p0, width |p1 - p0|, height |p2 - p1|.
fn rect_of(b: &[P; 4]) -> (P, P, f64, f64) {
    let c = [(b[0][0] + b[1][0] + b[2][0] + b[3][0]) / 4.0, (b[0][1] + b[1][1] + b[2][1] + b[3][1]) / 4.0];
    let a = [b[0][0] - b[3][0], b[0][1] - b[3][1]];
    let l = a[0].hypot(a[1]).max(1e-12);
    (c, [a[0] / l, a[1] / l], dist(b[0], b[1]), dist(b[1], b[2]))
}

/// Whether the game puts p in the rectangle: |d . axis| <= half the extent along both axes. The axis is p3 -> p0, which runs along
/// the height side (p1 -> p2); the width goes across it.
pub fn in_rect(b: &[P; 4], p: P) -> bool {
    let (c, u, w, h) = rect_of(b);
    let d = [p[0] - c[0], p[1] - c[1]];
    let along = d[0] * u[0] + d[1] * u[1];
    let across = -d[0] * u[1] + d[1] * u[0];
    along.abs() <= 0.5 * h && across.abs() <= 0.5 * w
}

fn overlap(a: &[P; 4], b: &[P; 4]) -> bool {
    let axes = |r: &[P; 4]| [[r[1][0] - r[0][0], r[1][1] - r[0][1]], [r[3][0] - r[0][0], r[3][1] - r[0][1]]];
    for axis in axes(a).into_iter().chain(axes(b)) {
        let proj = |cs: &[P; 4]| cs.iter().map(|p| p[0] * axis[0] + p[1] * axis[1]).fold((f64::MAX, f64::MIN), |(lo, hi), x| (lo.min(x), hi.max(x)));
        let (pa, pb) = (proj(a), proj(b));
        if pa.1 <= pb.0 + 1e-9 || pb.1 <= pa.0 + 1e-9 {
            return false;
        }
    }

    true
}

/// The nodes whose rectangles hold p (the game takes the last).
fn holders(l: &Layout, p: P) -> Vec<usize> {
    (0..l.nodes.len()).filter(|&i| in_rect(&l.nodes[i].bbox, p)).collect()
}

pub fn check(l: &Layout) -> Report {
    let mut r = Report::default();
    if l.perimeter.len() < 3 {
        r.problems.push("no fort perimeter (the game reportedly crashes without one)".to_owned());
    }

    for (i, n) in l.nodes.iter().enumerate() {
        if area2(&n.bbox).abs() / 2.0 < 0.01 {
            r.problems.push(format!("node {i}: its rectangle has no area (the game drops it)"));
        }

        if n.boundary.len() < 3 {
            r.problems.push(format!("node {i}: no Boundary"));
        }
    }

    for i in 0..l.nodes.len() {
        for j in i + 1..l.nodes.len() {
            if overlap(&l.nodes[i].bbox, &l.nodes[j].bbox) {
                r.notes.push(format!("nodes {i} and {j}: their rectangles overlap"));
            }
        }
    }

    let mut joined = vec![];
    for (e, edge) in l.edges.iter().enumerate() {
        if edge.line.len() < 2 {
            r.problems.push(format!("edge {e}: its line has fewer than 2 points"));
            continue;
        }

        if matches!(edge.kind, EdgeKind::Wall | EdgeKind::Palisade) {
            continue;
        }

        let (first, last) = (edge.line[0], edge.line[edge.line.len() - 1]);
        let (a, b) = (holders(l, first), holders(l, last));
        if a.is_empty() || b.is_empty() {
            r.problems.push(format!("edge {e} ({:?}): an end in no node's rectangle (at ({:.0}, {:.0}))", edge.kind, if a.is_empty() { first[0] } else { last[0] }, if a.is_empty() { first[1] } else { last[1] }));
            continue;
        }

        if a.len() > 1 || b.len() > 1 {
            r.notes.push(format!("edge {e}: an end in {} rectangles (the game takes the last)", a.len().max(b.len())));
        }

        let (na, nb) = (a[a.len() - 1], b[b.len() - 1]);
        if na == nb {
            r.problems.push(format!("edge {e}: both ends in node {na}"));
            continue;
        }

        joined.push((na, nb));
        if edge.kind == EdgeKind::Street {
            if edge.sides.len() < 2 {
                r.problems.push(format!("edge {e}: a street without its two sides"));
            }

            for s in &edge.sides {
                let (sa, sb) = (s[0], s[s.len() - 1]);
                let (ba, bb) = (&l.nodes[na].boundary, &l.nodes[nb].boundary);
                let fits = (inside(sa, ba) && inside(sb, bb)) || (inside(sb, ba) && inside(sa, bb));
                if !fits {
                    r.notes.push(format!("edge {e}: a side's ends are not in the two nodes' Boundaries"));
                }
            }
        }
    }

    // The street graph in one piece.
    let n = l.nodes.len();
    let mut parent: Vec<usize> = (0..n).collect();
    fn find(p: &mut [usize], mut x: usize) -> usize {
        while p[x] != x {
            p[x] = p[p[x]];
            x = p[x];
        }

        x
    }

    for (a, b) in joined {
        let (ra, rb) = (find(&mut parent, a), find(&mut parent, b));
        parent[ra] = rb;
    }

    let parts = (0..n).filter(|&x| find(&mut parent, x) == x).count();
    if parts > 1 {
        r.notes.push(format!("the street graph is in {parts} parts"));
    }

    r
}

/// Captures in the street graph: each in a node's Boundary (the main one in an area's). Those on the walls are left out by CA too.
pub fn check_captures(l: &Layout, captures: &[Capture]) -> Vec<String> {
    let mut out = vec![];
    for c in captures {
        let held = l.nodes.iter().any(|n| (!c.major || n.kind == NodeKind::Area) && n.boundary.len() >= 3 && inside(c.at, &n.boundary));
        if !held && c.major {
            out.push(format!("the main capture point at ({:.0}, {:.0}) is in no area", c.at[0], c.at[1]));
        }
    }

    out
}
