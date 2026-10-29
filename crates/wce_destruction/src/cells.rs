//! The cells a building breaks into: seeds scattered in its box (finer in the damage volumes), their Voronoi cells clipped to the
//! domain (the building's box above the ground: what is below never breaks). A cell whose seed is in a damage level's volumes (with
//! noise, for a ragged edge) is removed at that level; one set of cells serves every LOD and every level, so they all cut the same way.

use std::collections::HashMap;

use crate::geom::{Aabb, Plane, Rng, V3, basis, centroid, clip_polygon};
use crate::params::{GROUND, Params};

/// What lies on the other side of a cell's face.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Across {
    Cell(usize),
    /// The domain's side; `true` for its bottom (the ground).
    Domain(bool),
}

/// A face of a cell: its plane (normal out of the cell), its polygon (counter-clockwise seen from outside) and its neighbour.
#[derive(Clone, Debug)]
pub struct Face {
    pub plane: Plane,
    pub poly: Vec<V3>,
    pub across: Across,
}

/// A convex cell.
#[derive(Clone, Debug, Default)]
pub struct Polyhedron {
    pub faces: Vec<Face>,
}

impl Polyhedron {
    fn of_box(b: &Aabb) -> Self {
        let (lo, hi) = (b.min, b.max);
        let corner = |x: bool, y: bool, z: bool| V3::new(if x { hi.x } else { lo.x }, if y { hi.y } else { lo.y }, if z { hi.z } else { lo.z });
        let mut faces = vec![];
        for axis in 0..3 {
            for high in [false, true] {
                let mut n = V3::zeros();
                n[axis] = if high { 1.0 } else { -1.0 };
                let d = if high { hi[axis] } else { -lo[axis] };
                let (u, v) = ((axis + 1) % 3, (axis + 2) % 3);
                let pt = |a: bool, b: bool| {
                    let mut f = [false; 3];
                    f[axis] = high;
                    f[u] = a;
                    f[v] = b;
                    corner(f[0], f[1], f[2])
                };
                let mut poly = vec![pt(false, false), pt(true, false), pt(true, true), pt(false, true)];
                if crate::geom::newell(&poly).dot(&n) < 0.0 {
                    poly.reverse();
                }

                faces.push(Face { plane: Plane { n, d }, poly, across: Across::Domain(axis == 1 && !high) });
            }
        }

        Self { faces }
    }

    pub fn vertices(&self) -> impl Iterator<Item = &V3> {
        self.faces.iter().flat_map(|f| f.poly.iter())
    }

    pub fn bounds(&self) -> Aabb {
        Aabb::of(self.vertices().copied())
    }

    /// Whether a point is inside (every face's plane), within `eps`.
    pub fn contains(&self, p: &V3, eps: f32) -> bool {
        self.faces.iter().all(|f| f.plane.side(p) <= eps)
    }

    /// The cell cut by a plane, keeping its inside; the new face is across `across`.
    fn clip(&mut self, plane: &Plane, across: Across) {
        const EPS: f32 = 1e-5;
        if self.vertices().all(|p| plane.side(p) <= EPS) {
            return;
        }

        let mut cut = vec![];
        let mut faces = vec![];
        for f in self.faces.drain(..) {
            let poly = clip_polygon(&f.poly, plane);
            for p in &poly {
                if plane.side(p).abs() <= 1e-4 {
                    cut.push(*p);
                }
            }

            if poly.len() >= 3 {
                faces.push(Face { poly, ..f });
            }
        }

        self.faces = faces;
        if cut.len() >= 3 {
            let c = centroid(&cut);
            let (u, v) = basis(&plane.n);
            cut.sort_by(|a, b| {
                let (da, db) = (a - c, b - c);
                da.dot(&v).atan2(da.dot(&u)).total_cmp(&db.dot(&v).atan2(db.dot(&u)))
            });
            cut.dedup_by(|a, b| (*a - *b).norm_squared() < 1e-10);
            if cut.len() >= 3 {
                self.faces.push(Face { plane: *plane, poly: cut, across });
            }
        }
    }
}

/// The building's cells.
#[derive(Clone, Debug, Default)]
pub struct Cells {
    pub seeds: Vec<V3>,

    /// Per seed, the damage level (1-based: 1 is the first damage level) it is removed at, `None` if it stays.
    pub level: Vec<Option<u8>>,
    pub domain: Aabb,

    /// The removed cells' shapes.
    pub shapes: HashMap<usize, Polyhedron>,

    grid: SeedGrid,
}

/// Seeds bucketed for nearest-seed queries.
#[derive(Clone, Debug, Default)]
struct SeedGrid {
    origin: V3,
    cell: f32,
    dims: [usize; 3],
    buckets: Vec<Vec<u32>>,
}

impl SeedGrid {
    fn new(seeds: &[V3], domain: &Aabb, cell: f32) -> Self {
        let size = domain.size();
        let dims = [0, 1, 2].map(|a| ((size[a] / cell).ceil() as usize).clamp(1, 256));
        let mut g = Self { origin: domain.min, cell, dims, buckets: vec![vec![]; dims[0] * dims[1] * dims[2]] };
        for (i, s) in seeds.iter().enumerate() {
            let k = g.index(&g.coord(s));
            g.buckets[k].push(i as u32);
        }
        g
    }

    fn coord(&self, p: &V3) -> [usize; 3] {
        [0, 1, 2].map(|a| (((p[a] - self.origin[a]) / self.cell).floor().max(0.0) as usize).min(self.dims[a] - 1))
    }

    fn index(&self, c: &[usize; 3]) -> usize {
        (c[2] * self.dims[1] + c[1]) * self.dims[0] + c[0]
    }

    fn nearest(&self, seeds: &[V3], p: &V3) -> Option<usize> {
        if seeds.is_empty() {
            return None;
        }

        let c = self.coord(p);
        let mut best: Option<(f32, usize)> = None;
        for r in 0..=self.dims.iter().copied().max().unwrap_or(1) {
            // Shell r: once something is found, one more shell settles it.
            let lo = c.map(|x| x.saturating_sub(r));
            let hi = [0, 1, 2].map(|a| (c[a] + r).min(self.dims[a] - 1));
            for z in lo[2]..=hi[2] {
                for y in lo[1]..=hi[1] {
                    for x in lo[0]..=hi[0] {
                        let on_shell = [x, y, z].iter().zip(&c).any(|(v, cc)| v.abs_diff(*cc) == r);
                        if !on_shell {
                            continue;
                        }

                        for &i in &self.buckets[self.index(&[x, y, z])] {
                            let d = (seeds[i as usize] - p).norm_squared();
                            if best.is_none_or(|(b, _)| d < b) {
                                best = Some((d, i as usize));
                            }
                        }
                    }
                }
            }

            if let Some((d, i)) = best {
                if d.sqrt() <= r as f32 * self.cell {
                    return Some(i);
                }
            }
        }

        best.map(|(_, i)| i)
    }
}

impl Cells {
    /// The cells of a building of these bounds for the parameters' levels.
    pub fn make(bounds: &Aabb, params: &Params) -> Self {
        let mut domain = bounds.expanded(1.0);
        domain.min.y = GROUND.max(bounds.min.y);
        let rng = Rng::new(params.seed);
        let mut seeds: Vec<V3> = vec![];
        let mut level: Vec<Option<u8>> = vec![];
        let mut finest = f32::MAX;

        for (k, lp) in params.levels.iter().enumerate() {
            if lp.volumes.is_empty() {
                continue;
            }

            let mut r = rng.fork(100 + k as u64);
            let inside = |p: &V3| lp.volumes.iter().map(|v| v.sdf(p)).fold(f32::MAX, f32::min);
            let mut region = Aabb::EMPTY;
            for v in &lp.volumes {
                region.merge(&v.bounds());
            }

            region.min = region.min.sup(&domain.min);
            region.max = region.max.inf(&domain.max);
            if region.is_empty() {
                continue;
            }

            // The spacing: the fragment size, larger if the level would have too many fragments (counted over the building's box).
            let solid = region.volume().max(1e-3);
            let mut s = lp.fragment_size.max(0.3);
            while solid / s.powi(3) > lp.max_fragments.max(1) as f32 * 2.5 {
                s *= 1.1;
            }

            finest = finest.min(s);
            let jitter = |r: &mut Rng, p: V3, s: f32| p + V3::new(r.range(-0.45, 0.45), r.range(-0.45, 0.45), r.range(-0.45, 0.45)) * s;
            let n = region.size().map(|x| (x / s).ceil().max(1.0) as usize);
            for iz in 0..n.z {
                for iy in 0..n.y {
                    for ix in 0..n.x {
                        let at = region.min + V3::new(ix as f32 + 0.5, iy as f32 + 0.5, iz as f32 + 0.5) * s;
                        let p = jitter(&mut r, at, s);
                        let noise = r.range(-1.0, 1.0) * lp.roughness * s * 0.5;
                        if !domain.contains(&p) || inside(&p) + noise > 0.0 {
                            continue;
                        }

                        // A seed already there from an earlier level keeps its level.
                        if seeds.iter().any(|q| (q - p).norm_squared() < (0.5 * s).powi(2)) {
                            continue;
                        }

                        seeds.push(p);
                        level.push(Some(k as u8 + 1));
                    }
                }
            }
        }

        // Seeds that stay, around the damage: they bound the removed cells.
        if finest < f32::MAX {
            let mut r = rng.fork(7);
            let s = finest * 1.6;
            let mut region = Aabb::EMPTY;
            for lp in &params.levels {
                for v in &lp.volumes {
                    region.merge(&v.bounds());
                }
            }

            let region = Aabb { min: region.min.sup(&domain.min) - V3::repeat(2.0 * s), max: region.max.inf(&domain.max) + V3::repeat(2.0 * s) };
            let n = region.size().map(|x| (x / s).ceil().max(1.0) as usize);
            for iz in 0..n.z {
                for iy in 0..n.y {
                    for ix in 0..n.x {
                        let at = region.min + V3::new(ix as f32 + 0.5, iy as f32 + 0.5, iz as f32 + 0.5) * s;
                        let p = at + V3::new(r.range(-0.4, 0.4), r.range(-0.4, 0.4), r.range(-0.4, 0.4)) * s;
                        if !domain.expanded(s).contains(&p) || seeds.iter().any(|q| (q - p).norm_squared() < (0.45 * s).powi(2)) {
                            continue;
                        }

                        seeds.push(p);
                        level.push(None);
                    }
                }
            }
        }

        let grid = SeedGrid::new(&seeds, &domain.expanded(4.0), finest.clamp(0.5, 4.0) * 1.5);
        let mut cells = Self { seeds, level, domain, shapes: HashMap::new(), grid };
        cells.shapes = (0..cells.seeds.len()).filter(|&i| cells.level[i].is_some()).map(|i| (i, cells.shape(i))).filter(|(_, p)| p.faces.len() >= 4).collect();
        cells.level.iter_mut().enumerate().for_each(|(i, l)| {
            if l.is_some() && !cells.shapes.contains_key(&i) {
                *l = None;
            }
        });
        cells
    }

    /// The cell of seed `i`: the domain cut by the bisector planes of the seeds near it.
    fn shape(&self, i: usize) -> Polyhedron {
        let s = self.seeds[i];
        let mut poly = Polyhedron::of_box(&self.domain);
        let mut others = (0..self.seeds.len()).filter(|&j| j != i).map(|j| ((self.seeds[j] - s).norm_squared(), j)).collect::<Vec<_>>();
        others.sort_by(|a, b| a.0.total_cmp(&b.0));
        for (d2, j) in others {
            let reach = poly.vertices().map(|p| (p - s).norm_squared()).fold(0.0, f32::max);
            if d2 > 4.0 * reach {
                break;
            }

            let o = self.seeds[j];
            let n = (o - s).normalize();
            let plane = Plane { n, d: n.dot(&((o + s) * 0.5)) };
            poly.clip(&plane, Across::Cell(j));
            if poly.faces.len() < 4 {
                break;
            }
        }

        poly
    }

    /// The seed whose cell holds a point.
    pub fn cell_of(&self, p: &V3) -> Option<usize> {
        self.grid.nearest(&self.seeds, p)
    }

    /// The level a point is removed at (1-based), if any: inside the domain, in a removed cell.
    pub fn removed_level(&self, p: &V3) -> Option<u8> {
        if p.y < self.domain.min.y || !self.domain.contains(p) {
            return None;
        }

        self.cell_of(p).and_then(|i| self.level[i])
    }

    /// Whether a point is gone at damage level `l` (0: intact).
    pub fn removed_by(&self, p: &V3, l: usize) -> bool {
        self.removed_level(p).is_some_and(|k| (k as usize) <= l)
    }

    /// The removed cells of damage level `l` exactly (sorted).
    pub fn of_level(&self, l: usize) -> Vec<usize> {
        let mut v = self.shapes.keys().copied().filter(|&i| self.level[i] == Some(l as u8)).collect::<Vec<_>>();
        v.sort_unstable();
        v
    }

    /// The level of a cell, as a number to compare (kept cells are after every level).
    pub fn rank(&self, i: usize) -> usize {
        self.level[i].map_or(usize::MAX, usize::from)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::params::Params;

    #[test]
    fn cells_tile_the_damage() {
        let b = Aabb { min: V3::new(-10.0, -2.0, -2.0), max: V3::new(10.0, 16.0, 10.0) };
        let p = Params::defaults("x", &b, "fort_wall");
        let c = Cells::make(&b, &p);
        assert!(!c.shapes.is_empty());
        let mut volume = 0.0;
        for (i, poly) in &c.shapes {
            assert!(poly.contains(&c.seeds[*i], 1e-3), "a cell holds its seed");
            for f in &poly.faces {
                assert!(crate::geom::newell(&f.poly).dot(&f.plane.n) > 0.0, "faces wind outward");
            }

            let ctr = centroid(&poly.vertices().copied().collect::<Vec<_>>());
            volume += poly.faces.iter().map(|f| crate::geom::newell(&f.poly).dot(&(f.poly[0] - ctr)) / 6.0).sum::<f32>();
            assert_eq!(c.cell_of(&ctr), Some(*i), "a cell's centre finds it");
        }

        assert!(volume > 50.0, "{volume}");
        assert_eq!(c.removed_by(&V3::new(0.0, 15.5, 4.0), 2), true);
        assert_eq!(c.removed_by(&V3::new(9.8, 3.0, 4.0), 2), false);
        assert!(c.of_level(1).len() + c.of_level(2).len() == c.shapes.len());
        let again = Cells::make(&b, &p);
        assert_eq!(again.seeds, c.seeds, "deterministic");
    }
}
