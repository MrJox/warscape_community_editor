//! Exclusion polygons that keep a tile's static trees and leave the rest of it to the generator, made from what a packed tree list shows
//! (docs/formats/tile_decompile.md, "Vegetation").
//!
//! BOB keeps a static tree only inside a `VegetationExclusionPolygonInstance`, and generates nothing inside one. A decompiled tile has its
//! static trees but not the polygons they stood in, so these are rebuilt: the squares of a grid near a static tree, without those where the
//! packed lists have a generated object (there was no exclusion there: BOB had generated it), the small holes left inside filled, and each
//! patch outlined as one polygon (split around a larger hole: an outline has none). Every static tree's own square stays in, so no static tree is dropped.

use std::collections::{HashMap, HashSet};

/// The grid's square, in metres.
pub const STEP: f64 = 2.0;

/// How far from a static tree the squares go, in metres (a forest's gaps between its trees are covered: no generated trees in them).
pub const REACH: f64 = 6.0;

/// Whether a packed list's object stands where BOB's generator puts them: the centre of one of its cells, 128 / `density` metres wide
/// (x, z in the list's metres).
#[must_use]
pub fn on_generator_grid(x: f64, z: f64, density: u32) -> bool {
    let whole = |v: f64| {
        let g = v * f64::from(density) / 128.0 - 0.5;
        (g - g.round()).abs() < 1e-4
    };
    whole(x) && whole(z)
}

/// The outlines (x, z points, counter-clockwise with x right and z up; closed, the first point not repeated) covering the static trees
/// (x, z metres, in any frame both share) on a tile of `extent` metres, without the squares of the generated objects at their edges. The
/// outlines at the tile's edges go a metre past it (trees stand up to the edge).
#[must_use]
pub fn static_cover(statics: &[(f64, f64)], generated: &[(f64, f64)], extent: (f64, f64)) -> Vec<Vec<(f64, f64)>> {
    let nx = (extent.0 / STEP).ceil().max(1.0) as usize;
    let nz = (extent.1 / STEP).ceil().max(1.0) as usize;
    let at = |v: f64, n: usize| ((v / STEP).floor().max(0.0) as usize).min(n - 1);
    let mut on = vec![false; nx * nz];
    let r = (REACH / STEP).ceil() as i64;
    for &(x, z) in statics {
        let (cx, cz) = (at(x, nx) as i64, at(z, nz) as i64);
        for dz in -r..=r {
            for dx in -r..=r {
                let (gx, gz) = (cx + dx, cz + dz);
                if gx < 0 || gz < 0 || gx >= nx as i64 || gz >= nz as i64 {
                    continue;
                }

                // The square's nearest point to the tree.
                let (x0, z0) = (gx as f64 * STEP, gz as f64 * STEP);
                let (px, pz) = (x.clamp(x0, x0 + STEP), z.clamp(z0, z0 + STEP));
                if (px - x).hypot(pz - z) <= REACH {
                    on[gz as usize * nx + gx as usize] = true;
                }
            }
        }
    }

    for &(x, z) in generated {
        on[at(z, nz) * nx + at(x, nx)] = false;
    }

    for &(x, z) in statics {
        on[at(z, nz) * nx + at(x, nx)] = true;
    }

    fill_small_holes(&mut on, nx, nz);
    let vertex = |v: usize, n: usize, e: f64| {
        if v == 0 {
            -1.0
        } else if v == n {
            (n as f64 * STEP).max(e) + 1.0
        } else {
            v as f64 * STEP
        }
    };
    pieces(&on, nx, nz)
        .iter()
        .flat_map(|cells| outlines(cells))
        .map(|l| l.into_iter().map(|(c, r)| (vertex(c, nx, extent.0), vertex(r, nz, extent.1))).collect())
        .collect()
}

/// The largest hole in a patch that is filled (squares): a generated object's square or a few, inside a forest. Larger ones (a clearing,
/// the inside of a ring of trees along the tile's edges) stay, the patch split around them.
const MAX_HOLE: usize = 32;

/// The 4-connected regions of the squares `want` says, as lists of (column, row).
fn regions(nx: usize, nz: usize, want: &dyn Fn(usize, usize) -> bool) -> Vec<Vec<(usize, usize)>> {
    let mut seen = vec![false; nx * nz];
    let mut out = vec![];
    for r0 in 0..nz {
        for c0 in 0..nx {
            if seen[r0 * nx + c0] || !want(c0, r0) {
                continue;
            }

            let mut region = vec![];
            let mut stack = vec![(c0, r0)];
            seen[r0 * nx + c0] = true;
            while let Some((c, r)) = stack.pop() {
                region.push((c, r));
                let mut push = |c: usize, r: usize| {
                    if !seen[r * nx + c] && want(c, r) {
                        seen[r * nx + c] = true;
                        stack.push((c, r));
                    }
                };

                if c > 0 {
                    push(c - 1, r);
                }

                if c + 1 < nx {
                    push(c + 1, r);
                }

                if r > 0 {
                    push(c, r - 1);
                }

                if r + 1 < nz {
                    push(c, r + 1);
                }
            }

            out.push(region);
        }
    }

    out
}

/// The holes of at most [`MAX_HOLE`] squares (off, not reaching the grid's edge) turned on.
fn fill_small_holes(on: &mut [bool], nx: usize, nz: usize) {
    let holes = regions(nx, nz, &|c, r| !on[r * nx + c]);
    for h in holes {
        if h.len() <= MAX_HOLE && !h.iter().any(|&(c, r)| c == 0 || r == 0 || c == nx - 1 || r == nz - 1) {
            for (c, r) in h {
                on[r * nx + c] = true;
            }
        }
    }
}

/// The patches of squares on, each without holes (an outline can't have one): a patch around a hole is split at the hole's column, again
/// until none is left.
fn pieces(on: &[bool], nx: usize, nz: usize) -> Vec<Vec<(usize, usize)>> {
    let mut work = regions(nx, nz, &|c, r| on[r * nx + c]);
    let mut out = vec![];
    while let Some(cells) = work.pop() {
        match hole_column(&cells) {
            None => out.push(cells),
            Some(cut) => {
                // Each side may fall apart: its regions again.
                for side in [
                    cells.iter().filter(|p| p.0 < cut).copied().collect::<Vec<_>>(),
                    cells.iter().filter(|p| p.0 >= cut).copied().collect(),
                ] {
                    work.extend(split_regions(&side));
                }
            }
        }
    }

    out
}

/// The 4-connected regions of a set of squares.
fn split_regions(cells: &[(usize, usize)]) -> Vec<Vec<(usize, usize)>> {
    let Some(((c0, r0), (c1, r1))) = bounds(cells) else { return vec![] };
    let (w, h) = (c1 - c0 + 1, r1 - r0 + 1);
    let mut member = vec![false; w * h];
    for &(c, r) in cells {
        member[(r - r0) * w + (c - c0)] = true;
    }

    regions(w, h, &|c, r| member[r * w + c]).into_iter().map(|reg| reg.into_iter().map(|(c, r)| (c + c0, r + r0)).collect()).collect()
}

fn bounds(cells: &[(usize, usize)]) -> Option<((usize, usize), (usize, usize))> {
    let c0 = cells.iter().map(|p| p.0).min()?;
    let r0 = cells.iter().map(|p| p.1).min()?;
    let c1 = cells.iter().map(|p| p.0).max()?;
    let r1 = cells.iter().map(|p| p.1).max()?;
    Some(((c0, r0), (c1, r1)))
}

/// A column through a hole of the patch (squares off it encloses), None when it has none.
fn hole_column(cells: &[(usize, usize)]) -> Option<usize> {
    let ((c0, r0), (c1, r1)) = bounds(cells)?;
    // Its box with a square of margin all round: what the margin reaches is outside.
    let (w, h) = (c1 - c0 + 3, r1 - r0 + 3);
    let mut member = vec![false; w * h];
    for &(c, r) in cells {
        member[(r - r0 + 1) * w + (c - c0 + 1)] = true;
    }

    let outside = regions(w, h, &|c, r| !member[r * w + c]);
    outside.into_iter().find(|reg| !reg.iter().any(|&(c, r)| c == 0 || r == 0 || c == w - 1 || r == h - 1)).map(|reg| {
        // The middle of the hole's columns, back in the grid's.
        let lo = reg.iter().map(|p| p.0).min().unwrap_or(1);
        let hi = reg.iter().map(|p| p.0).max().unwrap_or(1);
        (lo + hi) / 2 + c0 - 1
    })
}

/// The loops of the squares' outer edges (grid corners), the squares on their left, without the corners along straight runs. Where two
/// squares touch only at a corner, the loop turns left there: two loops meeting at that corner.
fn outlines(cells: &[(usize, usize)]) -> Vec<Vec<(usize, usize)>> {
    let member = cells.iter().copied().collect::<HashSet<_>>();
    let is_on = |c: i64, r: i64| c >= 0 && r >= 0 && member.contains(&(c as usize, r as usize));
    let mut edges: Vec<((usize, usize), (usize, usize))> = vec![];
    for &(c, r) in cells {
        let (ci, ri) = (c as i64, r as i64);
        if !is_on(ci, ri - 1) {
            edges.push(((c, r), (c + 1, r)));
        }

        if !is_on(ci + 1, ri) {
            edges.push(((c + 1, r), (c + 1, r + 1)));
        }

        if !is_on(ci, ri + 1) {
            edges.push(((c + 1, r + 1), (c, r + 1)));
        }

        if !is_on(ci - 1, ri) {
            edges.push(((c, r + 1), (c, r)));
        }
    }

    let mut from: HashMap<(usize, usize), Vec<usize>> = HashMap::new();
    for (i, e) in edges.iter().enumerate() {
        from.entry(e.0).or_default().push(i);
    }

    let dir = |e: &((usize, usize), (usize, usize))| (e.1.0 as i64 - e.0.0 as i64, e.1.1 as i64 - e.0.1 as i64);
    let mut used = vec![false; edges.len()];
    let mut loops = vec![];
    for start in 0..edges.len() {
        if used[start] {
            continue;
        }

        let mut corners = vec![];
        let mut e = start;
        loop {
            used[e] = true;
            corners.push(edges[e].0);
            let d = dir(&edges[e]);
            // Left, straight on, right.
            let next = from.get(&edges[e].1).and_then(|outs| {
                [(-d.1, d.0), d, (d.1, -d.0)]
                    .into_iter()
                    .find_map(|want| outs.iter().copied().find(|&o| !used[o] && dir(&edges[o]) == want))
            });
            match next {
                Some(n) => e = n,
                None => break,
            }
        }

        // Only the corners where the outline turns.
        let n = corners.len();
        let turns = (0..n)
            .filter(|&i| {
                let (a, b, c) = (corners[(i + n - 1) % n], corners[i], corners[(i + 1) % n]);
                (b.0 as i64 - a.0 as i64, b.1 as i64 - a.1 as i64) != (c.0 as i64 - b.0 as i64, c.1 as i64 - b.1 as i64)
            })
            .map(|i| corners[i])
            .collect::<Vec<_>>();
        if turns.len() >= 3 {
            loops.push(turns);
        }
    }

    loops
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vegetation::generate::point_in;

    fn inside(polys: &[Vec<(f64, f64)>], x: f64, z: f64) -> bool {
        polys.iter().any(|p| point_in(&p.iter().map(|&(x, z)| (x as f32, z as f32)).collect::<Vec<_>>(), x as f32, z as f32))
    }

    #[test]
    fn a_forest_is_one_polygon_and_keeps_its_trees() {
        let statics = (0..5).flat_map(|i| (0..5).map(move |j| (50.0 + 4.0 * f64::from(i), 50.0 + 4.0 * f64::from(j)))).collect::<Vec<_>>();
        let polys = static_cover(&statics, &[], (128.0, 128.0));
        assert_eq!(polys.len(), 1);
        assert!(statics.iter().all(|&(x, z)| inside(&polys, x, z)));
        assert!(!inside(&polys, 10.0, 10.0));
    }

    #[test]
    fn a_generated_object_at_the_edge_stays_out_but_a_static_tree_stays_in() {
        let polys = static_cover(&[(64.5, 64.5)], &[(70.5, 64.5), (64.6, 64.6)], (128.0, 128.0));
        assert!(!inside(&polys, 70.5, 64.5));
        assert!(inside(&polys, 64.5, 64.5));
    }

    #[test]
    fn a_hole_inside_a_forest_is_filled() {
        let statics = (0..9).flat_map(|i| (0..9).map(move |j| (40.0 + 4.0 * f64::from(i), 40.0 + 4.0 * f64::from(j)))).collect::<Vec<_>>();
        let polys = static_cover(&statics, &[(57.0, 57.0)], (128.0, 128.0));
        assert_eq!(polys.len(), 1);
        assert!(inside(&polys, 57.0, 57.0));
    }

    #[test]
    fn a_ring_of_trees_along_the_edges_leaves_the_inside_to_the_generator() {
        let mut statics = vec![];
        for i in 0..=32 {
            let v = 4.0 * f64::from(i);
            statics.extend([(v, 1.0), (v, 127.0), (1.0, v), (127.0, v)]);
        }

        let polys = static_cover(&statics, &[], (128.0, 128.0));
        assert!(statics.iter().all(|&(x, z)| inside(&polys, x, z)));
        assert!(!inside(&polys, 64.0, 64.0));
        assert!(!inside(&polys, 30.0, 90.0));
    }

    #[test]
    fn two_forests_apart_are_two_polygons() {
        let polys = static_cover(&[(20.0, 20.0), (100.0, 100.0)], &[], (128.0, 128.0));
        assert_eq!(polys.len(), 2);
    }

    #[test]
    fn trees_at_the_edges_are_covered() {
        let statics = [(0.0, 0.0), (128.0, 128.0), (127.9, 0.2)];
        let polys = static_cover(&statics, &[], (128.0, 128.0));
        assert!(statics.iter().all(|&(x, z)| inside(&polys, x, z)));
    }

    #[test]
    fn generated_objects_are_on_the_grid() {
        assert!(on_generator_grid(0.5, 1.5, 128));
        assert!(!on_generator_grid(0.7, 1.5, 128));
        assert!(on_generator_grid(1.0, 3.0, 64));
    }
}
