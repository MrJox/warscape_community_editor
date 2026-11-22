//! Where the tile's water stands over its ground: the rivers' ribbons and the lakes' planes, wherever the ground is under their water.
//! Grass and trees are left out there (`grass.rs`, `vegetation.rs`): a generated list was made before the river or the lake, and a
//! river's bed need not be painted with a texture that rules them out.

use super::Tile;

/// The mask's cell, metres.
const CELL: f32 = 2.0;

/// How far under the water the ground must be for a place to be under it, metres (a bank the water laps stays green).
const DEPTH: f32 = 0.05;

/// The places under water, a cell of [`CELL`] metres each over the tile.
pub(super) struct WaterMask {
    w: usize,
    h: usize,
    wet: Vec<bool>,
}

impl WaterMask {
    /// Whether a place (world metres) is under water.
    pub(super) fn under(&self, x: f32, z: f32) -> bool {
        if self.wet.is_empty() || x < 0.0 || z < 0.0 {
            return false;
        }

        let (i, j) = ((x / CELL) as usize, (z / CELL) as usize);
        i < self.w && j < self.h && self.wet[j * self.w + i]
    }
}

impl Tile {
    /// The tile's water over its ground, as drawn now (the viewed variation's rivers and lakes).
    pub(super) fn water_mask(&self) -> WaterMask {
        let (w, h) = ((self.extent_x() as f32 / CELL).ceil() as usize + 1, (self.extent_z() as f32 / CELL).ceil() as usize + 1);
        let mut wet = vec![false; w * h];
        let mut any = false;
        let centre = |i: usize, j: usize| ((i as f32 + 0.5) * CELL, (j as f32 + 0.5) * CELL);
        let mut mark = |i: usize, j: usize, water: f32, wet: &mut Vec<bool>| {
            let (x, z) = centre(i, j);
            if !wet[j * w + i] && self.terrain_height(x, z) < water - DEPTH {
                wet[j * w + i] = true;
                any = true;
            }
        };
        let cells = |lo: f32, hi: f32, n: usize| ((lo / CELL).floor().max(0.0) as usize, ((hi / CELL).ceil().max(0.0) as usize).min(n));

        // The rivers: each stretch between two samples, as wide as the ribbon, its water from one to the other.
        for (_, samples) in self.river_ribbons() {
            for s in samples.windows(2) {
                let (a, b) = (&s[0], &s[1]);
                let half = (a.width.max(b.width) * 0.5) as f32;
                let (ax, az, bx, bz) = (a.x as f32, a.z as f32, b.x as f32, b.z as f32);
                let (i0, i1) = cells(ax.min(bx) - half, ax.max(bx) + half, w);
                let (j0, j1) = cells(az.min(bz) - half, az.max(bz) + half, h);
                let (dx, dz) = (bx - ax, bz - az);
                let l2 = dx * dx + dz * dz;
                for j in j0..j1 {
                    for i in i0..i1 {
                        let (x, z) = centre(i, j);
                        let t = if l2 > 1e-9 { (((x - ax) * dx + (z - az) * dz) / l2).clamp(0.0, 1.0) } else { 0.0 };
                        let width = (a.width + (b.width - a.width) * f64::from(t)) as f32 * 0.5;
                        if (x - ax - dx * t).hypot(z - az - dz * t) <= width {
                            mark(i, j, (a.y + (b.y - a.y) * f64::from(t)) as f32, &mut wet);
                        }
                    }
                }
            }
        }

        // The lakes: their plane over the outline's rectangle (`lakes.rs`), wherever the ground is under it.
        let sc = self.tscene();
        for e in sc.find_all("SIMPLE_MESH_INSTANCE") {
            let Some(text) = sc.get(e, super::lakes::OUTLINE_ATTR) else { continue };
            if !self.shown(e) || !self.of_tile_variation(e) {
                continue;
            }

            let poly = super::lakes::lake_outline(&text);
            if poly.len() < 3 {
                continue;
            }

            let water = self.world_position(e).y;
            let m = super::lakes::PLANE_MARGIN_M as f32;
            let xs = poly.iter().map(|p| p[0] as f32);
            let zs = poly.iter().map(|p| p[1] as f32);
            let (i0, i1) = cells(xs.clone().fold(f32::INFINITY, f32::min) - m, xs.fold(f32::NEG_INFINITY, f32::max) + m, w);
            let (j0, j1) = cells(zs.clone().fold(f32::INFINITY, f32::min) - m, zs.fold(f32::NEG_INFINITY, f32::max) + m, h);
            for j in j0..j1 {
                for i in i0..i1 {
                    mark(i, j, water, &mut wet);
                }
            }
        }

        if !any {
            wet.clear();
        }

        WaterMask { w, h, wet }
    }
}
