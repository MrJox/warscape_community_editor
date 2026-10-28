//! A model's top surface seen from above: per cell of a grid over its x, z bounds, the highest point of its triangles in that cell. The
//! renderer's model cache builds it from LOD 0; the editor drapes its polygons over the buildings with it.

use wce_foundation::math::{Float3, Float4x4};

/// Cell size in metres; the grid grows coarser past [`MAX_CELLS`] a side.
const CELL: f32 = 0.5;
const MAX_CELLS: usize = 256;

/// No geometry in the cell.
pub const EMPTY: f32 = f32::NEG_INFINITY;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ModelTop {
    /// The grid's corner (model space).
    pub x0: f32,
    pub z0: f32,
    pub cell: f32,
    pub nx: usize,
    pub nz: usize,

    /// Row-major (z rows), [`EMPTY`] where nothing is.
    pub heights: Vec<f32>,
}

impl ModelTop {

    /// From triangles (three points each, model space). `None` when there are none.
    pub fn from_triangles(tris: &[[Float3; 3]]) -> Option<Self> {
        let (mut lo, mut hi) = ([f32::MAX; 2], [f32::MIN; 2]);
        for p in tris.iter().flatten() {
            lo = [lo[0].min(p.x), lo[1].min(p.z)];
            hi = [hi[0].max(p.x), hi[1].max(p.z)];
        }

        if tris.is_empty() || !(lo[0] <= hi[0] && lo[1] <= hi[1]) {
            return None;
        }

        let extent = (hi[0] - lo[0]).max(hi[1] - lo[1]);
        let cell = CELL.max(extent / MAX_CELLS as f32);
        let nx = (((hi[0] - lo[0]) / cell).floor() as usize + 1).min(MAX_CELLS + 1);
        let nz = (((hi[1] - lo[1]) / cell).floor() as usize + 1).min(MAX_CELLS + 1);
        let mut top = Self { x0: lo[0], z0: lo[1], cell, nx, nz, heights: vec![EMPTY; nx * nz] };
        let mut poly = Vec::with_capacity(8);
        let mut next = Vec::with_capacity(8);
        for t in tris {
            let (mut a, mut b) = ([f32::MAX; 2], [f32::MIN; 2]);
            for p in t {
                a = [a[0].min(p.x), a[1].min(p.z)];
                b = [b[0].max(p.x), b[1].max(p.z)];
            }

            let (i0, i1) = (top.cell_x(a[0]), top.cell_x(b[0]));
            let (j0, j1) = (top.cell_z(a[1]), top.cell_z(b[1]));

            // One cell: its highest vertex.
            if i0 == i1 && j0 == j1 {
                let h = &mut top.heights[j0 * nx + i0];
                *h = h.max(t[0].y.max(t[1].y).max(t[2].y));
                continue;
            }

            // Else the triangle clipped to each cell it may touch (in 3D, by the cell's vertical sides): its highest point there is one of
            // the clipped polygon's corners.
            for j in j0..=j1 {
                for i in i0..=i1 {
                    let (cx0, cz0) = (top.x0 + i as f32 * cell, top.z0 + j as f32 * cell);
                    poly.clear();
                    poly.extend_from_slice(t);
                    for (axis, edge, keep_above) in [(0, cx0, true), (0, cx0 + cell, false), (2, cz0, true), (2, cz0 + cell, false)] {
                        clip(&poly, &mut next, axis, edge, keep_above);
                        std::mem::swap(&mut poly, &mut next);
                        if poly.is_empty() {
                            break;
                        }
                    }

                    if let Some(y) = poly.iter().map(|p| p.y).reduce(f32::max) {
                        let h = &mut top.heights[j * nx + i];
                        *h = h.max(y);
                    }
                }
            }
        }

        Some(top)
    }

    fn cell_x(&self, x: f32) -> usize {
        (((x - self.x0) / self.cell).floor().max(0.0) as usize).min(self.nx - 1)
    }

    fn cell_z(&self, z: f32) -> usize {
        (((z - self.z0) / self.cell).floor().max(0.0) as usize).min(self.nz - 1)
    }

    /// Each non-empty cell's top in the world through `world`: its centre and corners at the cell's height (so a coarse cell still covers
    /// its footprint), as (x, y, z).
    pub fn for_each_world_point(&self, world: &Float4x4, mut f: impl FnMut(Float3)) {
        let h = self.cell * 0.5;
        for j in 0..self.nz {
            for i in 0..self.nx {
                let y = self.heights[j * self.nx + i];
                if y == EMPTY {
                    continue;
                }

                let (cx, cz) = (self.x0 + (i as f32 + 0.5) * self.cell, self.z0 + (j as f32 + 0.5) * self.cell);
                for (dx, dz) in [(0.0, 0.0), (-h, -h), (h, -h), (-h, h), (h, h)] {
                    f(world.transform_point(Float3::new(cx + dx, y, cz + dz)));
                }
            }
        }
    }

    /// The x, z rectangle of the grid in the world through `world` ((min x, min z), (max x, max z)).
    pub fn world_rect(&self, world: &Float4x4) -> ([f32; 2], [f32; 2]) {
        let (x1, z1) = (self.x0 + self.nx as f32 * self.cell, self.z0 + self.nz as f32 * self.cell);
        let (mut lo, mut hi) = ([f32::MAX; 2], [f32::MIN; 2]);
        for (x, z) in [(self.x0, self.z0), (x1, self.z0), (self.x0, z1), (x1, z1)] {
            let p = world.transform_point(Float3::new(x, 0.0, z));
            lo = [lo[0].min(p.x), lo[1].min(p.z)];
            hi = [hi[0].max(p.x), hi[1].max(p.z)];
        }

        (lo, hi)
    }
}

/// Sutherland-Hodgman against one axis-aligned vertical plane (`axis` 0: x, 2: z).
fn clip(input: &[Float3], out: &mut Vec<Float3>, axis: usize, edge: f32, keep_above: bool) {
    out.clear();
    let v = |p: &Float3| if axis == 0 { p.x } else { p.z };
    let inside = |p: &Float3| if keep_above { v(p) >= edge } else { v(p) <= edge };
    for k in 0..input.len() {
        let (a, b) = (input[k], input[(k + 1) % input.len()]);
        let (ia, ib) = (inside(&a), inside(&b));
        if ia {
            out.push(a);
        }

        if ia != ib {
            let t = (edge - v(&a)) / (v(&b) - v(&a));
            out.push(a + (b - a) * t);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_flat_roof_and_a_wall() {
        // A 4 x 4 m roof at 5 m and a vertical wall at x = 10 up to 8 m.
        let p = Float3::new;
        let tris = [
            [p(0.0, 5.0, 0.0), p(4.0, 5.0, 0.0), p(4.0, 5.0, 4.0)],
            [p(0.0, 5.0, 0.0), p(4.0, 5.0, 4.0), p(0.0, 5.0, 4.0)],
            [p(10.0, 0.0, 0.0), p(10.0, 8.0, 0.0), p(10.0, 8.0, 4.0)],
        ];

        let top = ModelTop::from_triangles(&tris).expect("a top");
        let at = |x: f32, z: f32| top.heights[top.cell_z(z) * top.nx + top.cell_x(x)];
        assert_eq!(at(2.0, 2.0), 5.0);
        assert_eq!(at(10.0, 3.9), 8.0);
        assert_eq!(at(7.0, 2.0), EMPTY);
    }
}
