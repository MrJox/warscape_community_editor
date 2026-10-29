//! Plane polygons: rings simplified, holes bridged into their outer ring, and ear clipping (the caps' sections, the footprints).

pub type P2 = (f32, f32);

/// Twice the signed area (positive counter-clockwise).
pub fn area2(r: &[P2]) -> f32 {
    (0..r.len()).map(|k| {
        let (a, b) = (r[k], r[(k + 1) % r.len()]);
        a.0 * b.1 - b.0 * a.1
    }).sum()
}

fn cross(o: P2, a: P2, b: P2) -> f32 {
    (a.0 - o.0) * (b.1 - o.1) - (a.1 - o.1) * (b.0 - o.0)
}

/// A ring without the points that move it by less than `tol` (Visvalingam-like: the flattest point first).
pub fn simplify(mut r: Vec<P2>, tol: f32) -> Vec<P2> {
    r.dedup_by(|a, b| (a.0 - b.0).abs() < 1e-6 && (a.1 - b.1).abs() < 1e-6);
    loop {
        let n = r.len();
        if n <= 3 {
            return r;
        }

        let (mut best, mut at) = (f32::MAX, 0);
        for k in 0..n {
            let (a, b, c) = (r[(k + n - 1) % n], r[k], r[(k + 1) % n]);
            let len = ((c.0 - a.0).powi(2) + (c.1 - a.1).powi(2)).sqrt().max(1e-6);
            let d = cross(a, c, b).abs() / len;
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

pub fn contains(r: &[P2], p: P2) -> bool {
    let mut inside = false;
    for k in 0..r.len() {
        let (a, b) = (r[k], r[(k + 1) % r.len()]);
        if (a.1 > p.1) != (b.1 > p.1) && p.0 < a.0 + (p.1 - a.1) / (b.1 - a.1) * (b.0 - a.0) {
            inside = !inside;
        }
    }
    inside
}

fn segments_cross(a: P2, b: P2, c: P2, d: P2) -> bool {
    let (d1, d2, d3, d4) = (cross(c, d, a), cross(c, d, b), cross(a, b, c), cross(a, b, d));
    ((d1 > 1e-9 && d2 < -1e-9) || (d1 < -1e-9 && d2 > 1e-9)) && ((d3 > 1e-9 && d4 < -1e-9) || (d3 < -1e-9 && d4 > 1e-9))
}

/// A counter-clockwise ring with clockwise holes as triangles (indices into the returned points).
pub fn triangulate(outer: &[P2], holes: &[Vec<P2>]) -> (Vec<P2>, Vec<[usize; 3]>) {
    let mut ring = outer.to_vec();
    let mut holes = holes.iter().filter(|h| h.len() >= 3).cloned().collect::<Vec<_>>();
    holes.sort_by(|a, b| max_x(b).total_cmp(&max_x(a)));
    for h in holes {
        let (hi, hp) = h.iter().copied().enumerate().max_by(|a, b| a.1.0.total_cmp(&b.1.0)).expect("a hole has points");
        // The nearest ring point the bridge reaches without crossing an edge.
        let mut best: Option<(f32, usize)> = None;
        for (i, &p) in ring.iter().enumerate() {
            let d = (p.0 - hp.0).powi(2) + (p.1 - hp.1).powi(2);
            if best.is_some_and(|(b, _)| d >= b) {
                continue;
            }

            let blocked = (0..ring.len()).any(|k| {
                let (a, b) = (ring[k], ring[(k + 1) % ring.len()]);
                k != i && (k + 1) % ring.len() != i && segments_cross(hp, p, a, b)
            }) || (0..h.len()).any(|k| segments_cross(hp, p, h[k], h[(k + 1) % h.len()]));
            if !blocked {
                best = Some((d, i));
            }
        }

        let Some((_, i)) = best else { continue };
        let mut merged = ring[..=i].to_vec();
        merged.extend((0..=h.len()).map(|k| h[(hi + k) % h.len()]));
        merged.push(ring[i]);
        merged.extend_from_slice(&ring[i + 1..]);
        ring = merged;
    }

    let tris = ear_clip(&ring);
    (ring, tris)
}

fn max_x(r: &[P2]) -> f32 {
    r.iter().map(|p| p.0).fold(f32::MIN, f32::max)
}

/// Ear clipping of a counter-clockwise ring (bridged holes allowed); what is left when no ear is found is fanned.
pub fn ear_clip(r: &[P2]) -> Vec<[usize; 3]> {
    let mut idx = (0..r.len()).collect::<Vec<_>>();
    let mut out = vec![];
    let mut guard = 0;
    while idx.len() > 3 && guard < r.len() * r.len() + 16 {
        guard += 1;
        let n = idx.len();
        let mut found = false;
        for k in 0..n {
            let (ia, ib, ic) = (idx[(k + n - 1) % n], idx[k], idx[(k + 1) % n]);
            let (a, b, c) = (r[ia], r[ib], r[ic]);
            if cross(a, b, c) <= 1e-10 {
                continue;
            }

            let blocked = idx.iter().any(|&j| {
                if j == ia || j == ib || j == ic {
                    return false;
                }
                let p = r[j];
                if p == a || p == b || p == c {
                    return false;
                }
                cross(a, b, p) >= 0.0 && cross(b, c, p) >= 0.0 && cross(c, a, p) >= 0.0
            });
            if blocked {
                continue;
            }

            out.push([ia, ib, ic]);
            idx.remove(k);
            found = true;
            break;
        }

        if !found {
            break;
        }
    }

    if idx.len() >= 3 {
        for k in 1..idx.len() - 1 {
            if cross(r[idx[0]], r[idx[k]], r[idx[k + 1]]) > 0.0 {
                out.push([idx[0], idx[k], idx[k + 1]]);
            }
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_square_with_a_hole() {
        let outer = vec![(0.0, 0.0), (4.0, 0.0), (4.0, 4.0), (0.0, 4.0)];
        let hole = vec![(1.0, 1.0), (1.0, 3.0), (3.0, 3.0), (3.0, 1.0)];
        let (pts, tris) = triangulate(&outer, &[hole]);
        let area = tris.iter().map(|t| cross(pts[t[0]], pts[t[1]], pts[t[2]]) * 0.5).sum::<f32>();
        assert!((area - 12.0).abs() < 1e-4, "{area}");
        assert!(tris.iter().all(|t| cross(pts[t[0]], pts[t[1]], pts[t[2]]) > 0.0));
        let l = simplify(vec![(0.0, 0.0), (1.0, 0.001), (2.0, 0.0), (2.0, 2.0), (0.0, 2.0)], 0.01);
        assert_eq!(l.len(), 4);
    }
}
