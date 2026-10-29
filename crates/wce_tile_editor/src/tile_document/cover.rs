//! The polygons' draped cover: a smooth surface over the terrain and the buildings under a polygon (or line), which its fill, edges and
//! point handles follow instead of the points' stored heights (display only: the file keeps its heights).
//!
//! The surface is a grid of nodes over the polygon's rectangle. A node is at least as high as the terrain in the cells around it, so the
//! cover never cuts into a hill between two nodes; the buildings add a bump (their tops over the ground, from the models' [`ModelTop`]s)
//! that is dilated and then box blurred by no more than the dilation, so it stays over every roof and falls off smoothly around it.

use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;

use wce_foundation::math::{Float3, Float4x4};
use wce_foundation::wce_prof;
use wce_scene::model_top::ModelTop;
use wce_scene::render_scene::{Overlay, Payload, RenderScene};

use super::Tile;

/// The finest node spacing (metres), and the most nodes a cover has (the spacing grows past it).
const MIN_CELL: f32 = 1.0;
const MAX_NODES: f32 = 65536.0;

/// The terrain is read every this many metres (finer than its pixels, so the highest sample in a cell is about its highest point), up to
/// this many samples a cover (coarser on large covers).
const SAMPLE_M: f32 = 0.5;
const MAX_SAMPLES: f32 = 1_000_000.0;

/// How far (metres) a building's bump spreads before it falls off.
const SMOOTH_M: f32 = 4.0;

/// Over the surface: the fill, and the edges over the fill.
const FILL_LIFT: f32 = 0.25;
const EDGE_LIFT: f32 = 0.35;

/// The edges' ribbon width (metres).
const RIBBON_M: f32 = 0.6;

/// A model piece standing in the cover's way, in the world.
pub(crate) struct Solid {
    pub top: Arc<ModelTop>,
    pub world: Float4x4,
    pub elem: usize,
}

/// The cover's surface: node heights (world metres, lifts not included).
#[derive(Clone, Debug, Default)]
pub(crate) struct Cover {
    x0: f32,
    z0: f32,
    cell: f32,
    nx: usize,
    nz: usize,
    h: Vec<f32>,

    /// The elements whose pieces raised it.
    pub sources: BTreeSet<usize>,

    /// What it was built from (`Tile::cover_for`): a cover with the same key is the same surface.
    pub key: u64,

    /// The polygon's fill and edges drawn on it, and what they were drawn from (its points and look): drawn again only when that changed.
    pub drawn: Option<(u64, Overlay)>,
}

impl Cover {

    /// The rectangle a cover over `lo`..`hi` covers: with a margin for the edges' ribbons and the bumps' fall-off.
    pub(crate) fn extent(lo: [f32; 2], hi: [f32; 2]) -> ([f32; 2], [f32; 2]) {
        let margin = SMOOTH_M + RIBBON_M + MIN_CELL;
        ([lo[0] - margin, lo[1] - margin], [hi[0] + margin, hi[1] + margin])
    }

    /// Whether a solid stands in a cover's extent (it raises the cover then).
    pub(crate) fn touches(s: &Solid, lo: [f32; 2], hi: [f32; 2]) -> bool {
        let (a, b) = s.top.world_rect(&s.world);
        !(b[0] < lo[0] || a[0] > hi[0] || b[1] < lo[1] || a[1] > hi[1])
    }

    /// Over the rectangle `lo`..`hi` (x, z) and a margin for the edges' ribbons and the bumps' fall-off.
    pub fn build(lo: [f32; 2], hi: [f32; 2], terrain: &dyn Fn(f32, f32) -> f32, solids: &[Solid]) -> Self {
        wce_prof!("tile.cover");
        let (lo, hi) = Self::extent(lo, hi);
        let (w, d) = (hi[0] - lo[0], hi[1] - lo[1]);
        let cell = MIN_CELL.max((w * d / MAX_NODES).sqrt());
        let (nx, nz) = ((w / cell).ceil() as usize + 1, (d / cell).ceil() as usize + 1);
        let (cx, cz) = (nx - 1, nz - 1);

        // The terrain's highest sample in each cell, on a lattice k times finer than the nodes (k + 1 samples a side per cell).
        let k = ((cell / SAMPLE_M).ceil() as usize).min(((MAX_SAMPLES / (cx * cz) as f32).sqrt() as usize).max(1)).max(1);
        let (lx, lz) = (cx * k + 1, cz * k + 1);
        let step = cell / k as f32;
        let lattice = (0..lz).flat_map(|j| (0..lx).map(move |i| (i, j))).map(|(i, j)| terrain(lo[0] + i as f32 * step, lo[1] + j as f32 * step)).collect::<Vec<_>>();
        let mut ground = vec![f32::MIN; cx * cz];
        for j in 0..cz {
            for i in 0..cx {
                let mut m = f32::MIN;
                for b in 0..=k {
                    let row = (j * k + b) * lx + i * k;
                    m = lattice[row..=row + k].iter().copied().fold(m, f32::max);
                }

                ground[j * cx + i] = m;
            }
        }

        // The buildings' tops over that ground, per cell.
        let mut bump = vec![0.0f32; cx * cz];
        let mut sources = BTreeSet::new();
        for s in solids {
            if !Self::touches(s, lo, hi) {
                continue;
            }

            sources.insert(s.elem);
            s.top.for_each_world_point(&s.world, |p| {
                let (fi, fj) = ((p.x - lo[0]) / cell, (p.z - lo[1]) / cell);
                if fi < 0.0 || fj < 0.0 || fi >= cx as f32 || fj >= cz as f32 {
                    return;
                }

                let c = fj as usize * cx + fi as usize;
                bump[c] = bump[c].max(p.y - ground[c]);
            });
        }

        // To the nodes: the highest of the (up to four) cells around each.
        let around = |f: &[f32], i: usize, j: usize| {
            let mut m = f32::MIN;
            for (a, b) in [(i.wrapping_sub(1), j.wrapping_sub(1)), (i, j.wrapping_sub(1)), (i.wrapping_sub(1), j), (i, j)] {
                if a < cx && b < cz {
                    m = m.max(f[b * cx + a]);
                }
            }

            m
        };

        let node_ground = (0..nz).flat_map(|j| (0..nx).map(move |i| (i, j))).map(|(i, j)| around(&ground, i, j)).collect::<Vec<_>>();
        let mut node_bump = (0..nz).flat_map(|j| (0..nx).map(move |i| (i, j))).map(|(i, j)| around(&bump, i, j)).collect::<Vec<_>>();

        // The bump's envelope: dilated by r nodes, then two box blurs whose radii add up to r (every node in a window is within r of the
        // centre, so it is at least the centre's bump).
        if !sources.is_empty() {
            let r = (SMOOTH_M / cell).ceil() as usize;
            separable(&mut node_bump, nx, nz, r, |w| w.iter().copied().fold(f32::MIN, f32::max));
            for radius in [r.div_ceil(2), r / 2] {
                separable(&mut node_bump, nx, nz, radius, |w| w.iter().sum::<f32>() / w.len() as f32);
            }
        }

        let h = node_ground.iter().zip(&node_bump).map(|(g, b)| g + b.max(0.0)).collect();
        Self { x0: lo[0], z0: lo[1], cell, nx, nz, h, sources, key: 0, drawn: None }
    }

    /// The surface's height at (x, z), clamped to the grid.
    pub fn at(&self, x: f32, z: f32) -> f32 {
        let fi = ((x - self.x0) / self.cell).clamp(0.0, (self.nx - 1) as f32);
        let fj = ((z - self.z0) / self.cell).clamp(0.0, (self.nz - 1) as f32);
        let (i, j) = ((fi as usize).min(self.nx.saturating_sub(2)), (fj as usize).min(self.nz.saturating_sub(2)));
        let (u, v) = (fi - i as f32, fj - j as f32);
        let h = |a: usize, b: usize| self.h[(b.min(self.nz - 1)) * self.nx + a.min(self.nx - 1)];
        (h(i, j) * (1.0 - u) + h(i + 1, j) * u) * (1.0 - v) + (h(i, j + 1) * (1.0 - u) + h(i + 1, j + 1) * u) * v
    }

    /// A point on the cover (where the handles stand).
    pub fn drape(&self, p: Float3) -> Float3 {
        Float3::new(p.x, self.at(p.x, p.z) + EDGE_LIFT, p.z)
    }

    /// The x, z rectangle the cover's nodes span.
    pub fn rect(&self) -> ([f32; 2], [f32; 2]) {
        ([self.x0, self.z0], [self.x0 + (self.nx - 1) as f32 * self.cell, self.z0 + (self.nz - 1) as f32 * self.cell])
    }

    /// The fill of a closed polygon (x, z of `points`) as triangles on the cover, cut at the grid's cells so they follow it.
    pub fn fill(&self, points: &[Float3], rgba: u32, o: &mut Overlay) {
        let pts = points.iter().map(|p| [p.x, p.z]).collect::<Vec<_>>();
        let mut piece = Vec::with_capacity(8);
        let mut next = Vec::with_capacity(8);
        for t in ear_clip(&pts) {
            let (a, b) = (
                [t[0][0].min(t[1][0]).min(t[2][0]), t[0][1].min(t[1][1]).min(t[2][1])],
                [t[0][0].max(t[1][0]).max(t[2][0]), t[0][1].max(t[1][1]).max(t[2][1])],
            );
            let cell_of = |v: f32, o: f32, n: usize| (((v - o) / self.cell).floor().max(0.0) as usize).min(n - 2);
            let (i0, i1) = (cell_of(a[0], self.x0, self.nx), cell_of(b[0], self.x0, self.nx));
            let (j0, j1) = (cell_of(a[1], self.z0, self.nz), cell_of(b[1], self.z0, self.nz));
            for j in j0..=j1 {
                for i in i0..=i1 {
                    let (cx0, cz0) = (self.x0 + i as f32 * self.cell, self.z0 + j as f32 * self.cell);
                    piece.clear();
                    piece.extend_from_slice(&t);
                    for (axis, edge, above) in [(0, cx0, true), (0, cx0 + self.cell, false), (1, cz0, true), (1, cz0 + self.cell, false)] {
                        clip2(&piece, &mut next, axis, edge, above);
                        std::mem::swap(&mut piece, &mut next);
                        if piece.len() < 3 {
                            break;
                        }
                    }

                    if piece.len() < 3 {
                        continue;
                    }

                    let v = |p: [f32; 2]| Float3::new(p[0], self.at(p[0], p[1]) + FILL_LIFT, p[1]);
                    for k in 1..piece.len() - 1 {
                        o.triangle(v(piece[0]), v(piece[k]), v(piece[k + 1]), rgba);
                    }
                }
            }
        }
    }

    /// The edges of a polyline (closing it when `closed`) as ribbons on the cover, with a 1 px line along their middle (it stays visible
    /// from afar), and a square on each point when `corners` (not on a spline's curve samples).
    pub fn edges(&self, points: &[Float3], closed: bool, corners: bool, rgba: u32, o: &mut Overlay) {
        let n = points.len();
        let segments = if closed && n > 2 { n } else { n.saturating_sub(1) };
        let half = RIBBON_M * 0.5;
        let on = |x: f32, z: f32| Float3::new(x, self.at(x, z) + EDGE_LIFT, z);
        for s in 0..segments {
            let (a, b) = (points[s], points[(s + 1) % n]);
            let (dx, dz) = (b.x - a.x, b.z - a.z);
            let len = (dx * dx + dz * dz).sqrt();
            if len < 1e-4 {
                continue;
            }

            let (nx, nz) = (-dz / len * half, dx / len * half);
            let steps = (len / self.cell.min(2.0)).ceil().max(1.0) as usize;
            let at = |k: usize| (a.x + dx * k as f32 / steps as f32, a.z + dz * k as f32 / steps as f32);
            for k in 0..steps {
                let ((x0, z0), (x1, z1)) = (at(k), at(k + 1));
                let (p0, p1, q0, q1) = (on(x0 + nx, z0 + nz), on(x0 - nx, z0 - nz), on(x1 + nx, z1 + nz), on(x1 - nx, z1 - nz));
                o.triangle(p0, p1, q1, rgba);
                o.triangle(p0, q1, q0, rgba);
                let lift = Float3::new(0.0, 0.02, 0.0);
                o.line(on(x0, z0) + lift, on(x1, z1) + lift, rgba | 0xFF00_0000);
            }
        }

        for p in points.iter().filter(|_| corners) {
            let (a, b, c, d) = (on(p.x - half, p.z - half), on(p.x + half, p.z - half), on(p.x + half, p.z + half), on(p.x - half, p.z + half));
            o.triangle(a, b, c, rgba);
            o.triangle(a, c, d, rgba);
        }
    }
}

/// `f` over windows of `radius` nodes to each side along x, then along z (the windows shrink at the borders).
fn separable(v: &mut [f32], nx: usize, nz: usize, radius: usize, f: impl Fn(&[f32]) -> f32) {
    if radius == 0 {
        return;
    }

    let mut line = vec![];
    let mut out = vec![];
    for (len, count, index) in [(nx, nz, (|a: usize, b: usize, nx: usize| b * nx + a) as fn(usize, usize, usize) -> usize), (nz, nx, |a, b, nx| a * nx + b)] {
        for b in 0..count {
            line.clear();
            line.extend((0..len).map(|a| v[index(a, b, nx)]));
            out.clear();
            out.extend((0..len).map(|a| f(&line[a.saturating_sub(radius)..(a + radius + 1).min(len)])));
            for (a, &x) in out.iter().enumerate() {
                v[index(a, b, nx)] = x;
            }
        }
    }
}

/// Sutherland-Hodgman of a convex x, z polygon against one axis-aligned line (`axis` 0: x, 1: z).
fn clip2(input: &[[f32; 2]], out: &mut Vec<[f32; 2]>, axis: usize, edge: f32, keep_above: bool) {
    out.clear();
    let inside = |p: &[f32; 2]| if keep_above { p[axis] >= edge } else { p[axis] <= edge };
    for k in 0..input.len() {
        let (a, b) = (input[k], input[(k + 1) % input.len()]);
        let (ia, ib) = (inside(&a), inside(&b));
        if ia {
            out.push(a);
        }

        if ia != ib {
            let t = (edge - a[axis]) / (b[axis] - a[axis]);
            out.push([a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t]);
        }
    }
}

fn cross(o: [f32; 2], a: [f32; 2], b: [f32; 2]) -> f32 {
    (a[0] - o[0]) * (b[1] - o[1]) - (a[1] - o[1]) * (b[0] - o[0])
}

/// Triangles of a simple polygon (either winding) by ear clipping. A self-crossing polygon stops where no ear is left: what was cut so far
/// is returned.
pub(crate) fn ear_clip(points: &[[f32; 2]]) -> Vec<[[f32; 2]; 3]> {
    let mut idx = (0..points.len()).collect::<Vec<_>>();
    idx.dedup_by(|a, b| points[*a] == points[*b]);
    if idx.len() > 1 && points[idx[0]] == points[idx[idx.len() - 1]] {
        idx.pop();
    }

    let area = (0..idx.len()).map(|k| {
        let (p, q) = (points[idx[k]], points[idx[(k + 1) % idx.len()]]);
        p[0] * q[1] - q[0] * p[1]
    }).sum::<f32>();
    let sign = if area >= 0.0 { 1.0 } else { -1.0 };
    let mut out = Vec::with_capacity(idx.len().saturating_sub(2));
    let mut k = 0;
    let mut stuck = 0;
    while idx.len() > 3 && stuck < idx.len() {
        let n = idx.len();
        let (ia, ib, ic) = (idx[(k + n - 1) % n], idx[k % n], idx[(k + 1) % n]);
        let (a, b, c) = (points[ia], points[ib], points[ic]);
        let convex = cross(a, b, c) * sign > 0.0;
        let empty = convex && idx.iter().all(|&j| {
            if j == ia || j == ib || j == ic {
                return true;
            }

            let p = points[j];
            !(cross(a, b, p) * sign >= 0.0 && cross(b, c, p) * sign >= 0.0 && cross(c, a, p) * sign >= 0.0)
        });

        if empty {
            out.push([a, b, c]);
            idx.remove(k % n);
            stuck = 0;
        } else {
            // A degenerate (collinear) corner goes without a triangle.
            if cross(a, b, c).abs() < 1e-9 {
                idx.remove(k % n);
                stuck = 0;
                continue;
            }

            k += 1;
            stuck += 1;
        }

        k %= idx.len().max(1);
    }

    if idx.len() == 3 {
        out.push([points[idx[0]], points[idx[1]], points[idx[2]]]);
    }

    out
}

impl Tile {

    /// The building pieces the covers go over, none when the display keeps them on the terrain (and the paths of the tops not known yet, asked for at the next [`Tile::poll_tops`]).
    pub(crate) fn cover_solids(&mut self, scene: &RenderScene) -> Vec<Solid> {
        let mut out = vec![];
        if !self.display.polygon_over_buildings {
            return out;
        }

        let mut pieces = vec![];
        for &e in &self.instances {
            if self.first_entity.get(e).is_none_or(|&f| f == 0) || self.tscene().type_of(e) != "BUILDING_INSTANCE" {
                continue;
            }

            for k in 0..u32::from(self.entity_count[e]) {
                pieces.push((e, self.first_entity[e] + k));
            }
        }

        for (&e, ids) in self.wall_preview.entities_by_wall() {
            pieces.extend(ids.iter().map(|&id| (e, id)));
        }

        for (e, id) in pieces {
            let Some(en) = scene.find(id).filter(|en| scene.drawable(en)) else { continue };
            let Payload::MeshInstance(mi) = &en.payload else { continue };
            match self.tops.get(&mi.model) {
                Some(Some(top)) => out.push(Solid { top: top.clone(), world: en.world, elem: e }),
                Some(None) => {}
                None => {
                    self.tops_wanted.insert(mi.model.clone());
                }
            }
        }

        out
    }

    /// Per frame: the model tops asked for by the covers (`peek` says `None` while a model loads, `Some(None)` for one without a top). The
    /// covers are rebuilt when tops came, at most once a second or so while models keep coming.
    pub fn poll_tops(&mut self, peek: &dyn Fn(&str) -> Option<Option<Arc<ModelTop>>>) {
        if self.tops_wanted.is_empty() {
            return;
        }

        let mut came = false;
        self.tops_wanted.retain(|path| match peek(path) {
            Some(top) => {
                self.tops.insert(path.clone(), top);
                came = true;
                false
            }
            None => true,
        });

        self.tops_polls += 1;
        self.tops_came |= came;
        if self.tops_came && (self.tops_wanted.is_empty() || self.tops_polls >= 60) {
            self.tops_came = false;
            self.tops_polls = 0;
            self.polygons_dirty = true;
        }
    }

    /// A polygon's cover over `lo`..`hi`: the one built from the same terrain and buildings before (kept across a rebuild of every
    /// polygon: adding one building to a city made its 277 covers be built twice, 0.3 s), else a new one.
    pub(crate) fn cover_for(&mut self, lo: [f32; 2], hi: [f32; 2], solids: &[Solid]) -> Cover {
        let (elo, ehi) = Cover::extent(lo, hi);
        let near = solids.iter().filter(|s| Cover::touches(s, elo, ehi)).collect::<Vec<_>>();
        // The heights: the whole map's, hashed once for the rebuild of every polygon (`spare_covers`).
        let ground = self.cover_ground;
        let mix = |h: u64, v: u64| (h ^ v).wrapping_mul(0x0100_0000_01b3);
        let mut key = [lo[0], lo[1], hi[0], hi[1]].iter().fold(mix(0xcbf2_9ce4_8422_2325, ground), |h, v| mix(h, u64::from(v.to_bits())));
        for s in &near {
            key = mix(key, Arc::as_ptr(&s.top) as u64);
            key = s.world.m.iter().flatten().fold(key, |h, v| mix(h, u64::from(v.to_bits())));
        }

        // The same surface; its sources are the elements now (an insert renumbers them).
        if let Some(mut c) = self.cover_spare.remove(&key) {
            c.sources = near.iter().map(|s| s.elem).collect();
            return c;
        }

        let mut c = Cover::build(lo, hi, &|x, z| self.terrain_height(x, z), solids);
        c.key = key;
        c
    }

    /// Keeps the covers for `cover_for` before every polygon is rebuilt (`end_spare_covers` drops the ones not used again), with the hash
    /// of the heights they were built on.
    pub(crate) fn spare_covers(&mut self) {
        self.cover_ground = self.project.height_region_hash([f64::MIN, f64::MIN], [f64::MAX, f64::MAX]);
        let covers = std::mem::take(&mut self.covers);
        self.cover_spare.extend(covers.into_values().filter(|c| c.key != 0).map(|c| (c.key, c)));
    }

    /// After the rebuild of every polygon: the covers not used again go (a single polygon's rebuild builds its own).
    pub(crate) fn end_spare_covers(&mut self) {
        self.cover_spare.clear();
        self.cover_ground = 0;
    }

    /// The polygons whose cover `elem` (a building) raised or may raise now, rebuilt at the next update.
    pub(crate) fn building_moved(&mut self, elem: usize) {
        // Covers on the terrain only (Polygons over buildings off): no building raises them.
        if !self.display.polygon_over_buildings {
            return;
        }

        let p = self.world_position(elem);
        let reach = self.model_of(elem).map_or(10.0, |m| {
            let e = m.bmax - m.bmin;
            (e.x.abs().max(e.y.abs()).max(e.z.abs()) / self.unit() as f32).max(1.0)
        });

        let hit = self.covers.iter().filter(|(_, c)| {
            let (lo, hi) = c.rect();
            c.sources.contains(&elem) || (p.x + reach >= lo[0] && p.x - reach <= hi[0] && p.z + reach >= lo[1] && p.z - reach <= hi[1])
        }).map(|(&e, _)| e).collect::<Vec<_>>();
        self.covers_dirty.extend(hit);
    }

    /// Where a polygon point is drawn: on its cover (the point's own position until the cover is built).
    pub fn draped_point(&self, elem: usize, p: Float3) -> Float3 {
        self.covers.get(&elem).map_or(p, |c| c.drape(p))
    }

    /// The polygon's points where they are drawn (see [`Tile::draped_point`]).
    pub fn polygon_display_points(&self, elem: usize) -> Vec<Float3> {
        let pts = self.polygon_world_points(elem);
        match self.covers.get(&elem) {
            Some(c) => pts.into_iter().map(|p| c.drape(p)).collect(),
            None => pts,
        }
    }
}

/// The draped covers by polygon element.
pub(crate) type Covers = HashMap<usize, Cover>;

#[cfg(test)]
mod tests {
    use super::*;

    fn area(t: &[[f32; 2]; 3]) -> f32 {
        cross(t[0], t[1], t[2]).abs() * 0.5
    }

    #[test]
    fn ear_clipping_covers_a_concave_polygon() {
        // An L: 3 x 3 minus its 2 x 2 top right corner (area 5), both windings.
        let l = [[0.0, 0.0], [3.0, 0.0], [3.0, 1.0], [1.0, 1.0], [1.0, 3.0], [0.0, 3.0]];
        for pts in [l.to_vec(), l.iter().rev().copied().collect()] {
            let tris = ear_clip(&pts);
            assert_eq!(tris.len(), 4);
            assert!((tris.iter().map(area).sum::<f32>() - 5.0).abs() < 1e-4);
        }
    }

    #[test]
    fn the_cover_stays_over_the_terrain_and_a_roof() {
        let terrain = |x: f32, z: f32| (x * 0.3).sin() * 3.0 + (z * 0.2).cos() * 2.0;
        let roof = ModelTop::from_triangles(&[
            [Float3::new(-2.0, 6.0, -2.0), Float3::new(2.0, 6.0, -2.0), Float3::new(2.0, 6.0, 2.0)],
            [Float3::new(-2.0, 6.0, -2.0), Float3::new(2.0, 6.0, 2.0), Float3::new(-2.0, 6.0, 2.0)],
        ]).expect("a roof");
        let mut world = Float4x4::IDENTITY;
        world.m[0][3] = 20.0;
        world.m[2][3] = 20.0;
        let solids = [Solid { top: Arc::new(roof), world, elem: 1 }];
        let c = Cover::build([0.0, 0.0], [40.0, 40.0], &terrain, &solids);
        for j in 0..160 {
            for i in 0..160 {
                let (x, z) = (i as f32 * 0.25, j as f32 * 0.25);
                assert!(c.at(x, z) >= terrain(x, z) - 0.02, "under the terrain at {x} {z}");
            }
        }

        // Over the roof (6 m) everywhere on it.
        assert!(c.at(20.0, 20.0) >= 6.0 - 1e-3);
        assert!(c.at(20.0 + 1.9, 20.0 - 1.9) >= 6.0 - 1e-3);

        // Far from the roof it hugs the ground.
        assert!(c.at(5.0, 5.0) - terrain(5.0, 5.0) < 1.0);
    }
}
