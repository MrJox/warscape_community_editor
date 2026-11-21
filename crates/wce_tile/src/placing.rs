//! What the generated detail (biomes, texture rules) shares: the terrain sampled on a grid, a placed tree, plant or rock, whether a point
//! is inside a polygon, and the building categories that are real buildings. Taken out of the ground coverage (shelved 2026-11-21).

/// The terrain's heights on a grid: `nx * nz` samples (rows along z), `cell` metres apart, sample (0, 0) at (`x0`, `z0`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Grid {
    pub x0: f32,
    pub z0: f32,
    pub cell: f32,
    pub nx: usize,
    pub nz: usize,
    pub heights: Vec<f32>,
}

impl Grid {
    /// Samples `height(x, z)` (metres) over [x0, x1] x [z0, z1], `cell` metres apart.
    pub fn sample(x0: f32, z0: f32, x1: f32, z1: f32, cell: f32, height: impl Fn(f32, f32) -> f32) -> Self {
        let nx = (((x1 - x0) / cell).floor() as usize + 1).max(1);
        let nz = (((z1 - z0) / cell).floor() as usize + 1).max(1);
        let mut heights = Vec::with_capacity(nx * nz);
        for z in 0..nz {
            for x in 0..nx {
                heights.push(height(x0 + x as f32 * cell, z0 + z as f32 * cell));
            }
        }

        Self { x0, z0, cell, nx, nz, heights }
    }

    /// The sample nearest a point; none off the grid (more than half a cell out).
    pub fn index_of(&self, x: f32, z: f32) -> Option<usize> {
        let (fx, fz) = (((x - self.x0) / self.cell).round(), ((z - self.z0) / self.cell).round());
        (fx >= 0.0 && fz >= 0.0 && (fx as usize) < self.nx && (fz as usize) < self.nz).then(|| fz as usize * self.nx + fx as usize)
    }

    /// The column, the row of the sample nearest an x, a z (clamped to the grid).
    pub fn index_x(&self, x: f32) -> usize {
        (((x - self.x0) / self.cell).round().max(0.0) as usize).min(self.nx - 1)
    }

    pub fn index_z(&self, z: f32) -> usize {
        (((z - self.z0) / self.cell).round().max(0.0) as usize).min(self.nz - 1)
    }

    /// Whether a point is on the tile (between the first and the last samples).
    pub fn contains(&self, x: f32, z: f32) -> bool {
        x >= self.x0 && z >= self.z0 && x <= self.x0 + (self.nx - 1) as f32 * self.cell && z <= self.z0 + (self.nz - 1) as f32 * self.cell
    }

    /// The centre of a sample, in metres.
    pub fn at(&self, i: usize) -> (f32, f32) {
        (self.x0 + (i % self.nx) as f32 * self.cell, self.z0 + (i / self.nx) as f32 * self.cell)
    }

    /// The height at a point, bilinear between the samples (clamped to the grid).
    pub fn height(&self, x: f32, z: f32) -> f32 {
        let fx = ((x - self.x0) / self.cell).clamp(0.0, (self.nx - 1) as f32);
        let fz = ((z - self.z0) / self.cell).clamp(0.0, (self.nz - 1) as f32);
        let (ix, iz) = ((fx as usize).min(self.nx.saturating_sub(2)), (fz as usize).min(self.nz.saturating_sub(2)));
        let (tx, tz) = (fx - ix as f32, fz - iz as f32);
        let h = |x: usize, z: usize| self.heights[z.min(self.nz - 1) * self.nx + x.min(self.nx - 1)];
        let a = h(ix, iz) + (h(ix + 1, iz) - h(ix, iz)) * tx;
        let b = h(ix, iz + 1) + (h(ix + 1, iz + 1) - h(ix, iz + 1)) * tx;
        a + (b - a) * tz
    }
}

/// The categories of `battlefield_buildings` the generated detail keeps off (and the texture rules measure to): the real buildings. Not `incidental` (rocks, props, barricades) or
/// `ground_platform` (path borders, mounds: part of the ground), nor a key of no known category (the invisible dummies among them).
pub const AVOIDED_CATEGORIES: [&str; 6] = ["generic", "fort_wall", "fort_tower", "unbreachable_fort_wall", "gate", "bridge"];

/// A placed tree, plant or rock: its key, where (metres, `y` the height of its origin), its scale and its turn (degrees).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Placed {
    pub key: String,
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub scale: f32,
    pub yaw: f32,
}

/// Whether a point is inside a polygon (even-odd).
pub fn inside(poly: &[(f32, f32)], x: f32, z: f32) -> bool {
    let mut is = false;
    let mut j = poly.len().wrapping_sub(1);
    for i in 0..poly.len() {
        let (a, b) = (poly[i], poly[j]);
        if (a.1 > z) != (b.1 > z) && x < (b.0 - a.0) * (z - a.1) / (b.1 - a.1) + a.0 {
            is = !is;
        }

        j = i;
    }

    is
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_grid_samples_and_interpolates() {
        let g = Grid::sample(0.0, 0.0, 10.0, 4.0, 2.0, |x, z| x + z * 10.0);
        assert_eq!((g.nx, g.nz), (6, 3));
        assert_eq!(g.height(3.0, 1.0), 13.0);
        assert_eq!(g.index_of(4.1, 2.2), Some(8));
        assert!(g.contains(10.0, 4.0) && !g.contains(10.5, 0.0));
    }

    #[test]
    fn inside_is_even_odd() {
        let square = [(0.0, 0.0), (4.0, 0.0), (4.0, 4.0), (0.0, 4.0)];
        assert!(inside(&square, 2.0, 2.0));
        assert!(!inside(&square, 5.0, 2.0));
    }
}
