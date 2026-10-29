//! The rubble heap of a level: a height field over the ground where the debris lies (each fragment's share of the rubble spread around
//! where it landed), relaxed until no slope is steeper than the angle of repose. It gives the heap's mesh (in the culture's rubble
//! material) and, when the rubble is walkable, the level's platforms over it.

use crate::geom::{Aabb, V3};

/// The angle of repose of rubble (tangent): about 35 degrees.
pub const REPOSE: f32 = 0.7;

#[derive(Clone, Debug, Default)]
pub struct Heap {
    pub x0: f32,
    pub z0: f32,
    pub step: f32,
    pub nx: usize,
    pub nz: usize,
    pub h: Vec<f32>,
}

impl Heap {
    /// An empty heap over a box's x / z (grown by 4 m), every `step`.
    pub fn new(region: &Aabb, step: f32) -> Self {
        let r = region.expanded(4.0);
        let nx = ((r.size().x / step).ceil() as usize).clamp(2, 512) + 1;
        let nz = ((r.size().z / step).ceil() as usize).clamp(2, 512) + 1;
        Self { x0: r.min.x, z0: r.min.z, step, nx, nz, h: vec![0.0; nx * nz] }
    }

    pub fn at(&self, i: usize, j: usize) -> f32 {
        self.h[j * self.nx + i]
    }

    fn point(&self, i: usize, j: usize) -> V3 {
        V3::new(self.x0 + i as f32 * self.step, self.at(i, j), self.z0 + j as f32 * self.step)
    }

    /// A volume (m^3) of rubble spread as a cone of radius `r` around (x, z).
    pub fn deposit(&mut self, x: f32, z: f32, volume: f32, r: f32) {
        let r = r.max(self.step);
        let cone = std::f32::consts::PI * r * r / 3.0;
        let peak = volume / cone;
        let cells = (r / self.step).ceil() as i64;
        let (ci, cj) = (((x - self.x0) / self.step).round() as i64, ((z - self.z0) / self.step).round() as i64);
        for j in (cj - cells).max(0)..=(cj + cells).min(self.nz as i64 - 1) {
            for i in (ci - cells).max(0)..=(ci + cells).min(self.nx as i64 - 1) {
                let (px, pz) = (self.x0 + i as f32 * self.step, self.z0 + j as f32 * self.step);
                let d = ((px - x).powi(2) + (pz - z).powi(2)).sqrt();
                if d < r {
                    self.h[j as usize * self.nx + i as usize] += peak * (1.0 - d / r);
                }
            }
        }
    }

    /// Lets rubble slide down where the slope is steeper than the angle of repose (volume kept).
    pub fn relax(&mut self, iterations: usize) {
        let max = REPOSE * self.step;
        for _ in 0..iterations {
            let mut moved = false;
            for j in 0..self.nz {
                for i in 0..self.nx {
                    for (di, dj) in [(1usize, 0usize), (0, 1)] {
                        let (i2, j2) = (i + di, j + dj);
                        if i2 >= self.nx || j2 >= self.nz {
                            continue;
                        }

                        let (a, b) = (j * self.nx + i, j2 * self.nx + i2);
                        let d = self.h[a] - self.h[b];
                        if d.abs() > max {
                            let m = (d.abs() - max) * 0.25 * d.signum();
                            self.h[a] -= m;
                            self.h[b] += m;
                            moved = true;
                        }
                    }
                }
            }

            if !moved {
                break;
            }
        }
    }

    /// The heap's height at a point (bilinear; 0 outside).
    pub fn height(&self, x: f32, z: f32) -> f32 {
        let (fx, fz) = ((x - self.x0) / self.step, (z - self.z0) / self.step);
        if fx < 0.0 || fz < 0.0 || fx >= (self.nx - 1) as f32 || fz >= (self.nz - 1) as f32 {
            return 0.0;
        }

        let (i, j) = (fx as usize, fz as usize);
        let (u, v) = (fx - i as f32, fz - j as f32);
        let (a, b, c, d) = (self.at(i, j), self.at(i + 1, j), self.at(i, j + 1), self.at(i + 1, j + 1));
        (a * (1.0 - u) + b * u) * (1.0 - v) + (c * (1.0 - u) + d * u) * v
    }

    pub fn max_height(&self) -> f32 {
        self.h.iter().copied().fold(0.0, f32::max)
    }

    /// The heap's surface where it is over `min` high, its edge sunk into the ground; triangles counter-clockwise seen from above, with
    /// their normals per vertex.
    pub fn mesh(&self, min: f32) -> Vec<[(V3, V3); 3]> {
        let mut out = vec![];
        let sunk = |p: V3| if p.y <= min { V3::new(p.x, -0.3, p.z) } else { p };
        let normal = |i: usize, j: usize| {
            let l = self.at(i.saturating_sub(1), j);
            let r = self.at((i + 1).min(self.nx - 1), j);
            let d = self.at(i, j.saturating_sub(1));
            let u = self.at(i, (j + 1).min(self.nz - 1));
            V3::new(l - r, 2.0 * self.step, d - u).normalize()
        };

        for j in 0..self.nz - 1 {
            for i in 0..self.nx - 1 {
                let quad = [(i, j), (i + 1, j), (i + 1, j + 1), (i, j + 1)];
                if quad.iter().all(|&(a, b)| self.at(a, b) <= min) {
                    continue;
                }

                let v = quad.map(|(a, b)| (sunk(self.point(a, b)), normal(a, b)));
                // Counter-clockwise from above (+y): (0,0) -> (0,1) -> (1,1) in x / z.
                out.push([v[0], v[3], v[2]]);
                out.push([v[0], v[2], v[1]]);
            }
        }

        out
    }

    /// Walkable platforms over the heap: quads every `cell` m where it stands over `min` and is not steeper than `max_slope`, those of
    /// a row that lie in about the same plane joined; their outlines counter-clockwise from above, on the heap.
    pub fn platforms(&self, cell: f32, min: f32, max_slope: f32) -> Vec<Vec<V3>> {
        let (x1, z1) = (self.x0 + (self.nx - 1) as f32 * self.step, self.z0 + (self.nz - 1) as f32 * self.step);
        let (nx, nz) = (((x1 - self.x0) / cell).floor() as usize, ((z1 - self.z0) / cell).floor() as usize);
        let at = |x: f32, z: f32| V3::new(x, self.height(x, z).max(0.0), z);
        let mut out = vec![];
        for j in 0..nz {
            let z = self.z0 + j as f32 * cell;
            let mut run: Option<(usize, V3)> = None;
            let flush = |run: &mut Option<(usize, V3)>, end: usize, out: &mut Vec<Vec<V3>>| {
                if let Some((start, _)) = run.take() {
                    let (xa, xb) = (self.x0 + start as f32 * cell, self.x0 + end as f32 * cell);
                    out.push(vec![at(xa, z), at(xa, z + cell), at(xb, z + cell), at(xb, z)]);
                }
            };

            for i in 0..nx {
                let x = self.x0 + i as f32 * cell;
                let corners = [at(x, z), at(x, z + cell), at(x + cell, z + cell), at(x + cell, z)];
                let n = crate::geom::newell(&corners);
                let ok = corners.iter().any(|p| p.y > min) && n.y > 0.0 && (n.x * n.x + n.z * n.z).sqrt() / n.y <= max_slope;
                if !ok {
                    flush(&mut run, i, &mut out);
                    continue;
                }

                let n = n.normalize();
                match run {
                    Some((_, rn)) if rn.dot(&n) > 0.99 => {}
                    Some(_) => {
                        flush(&mut run, i, &mut out);
                        run = Some((i, n));
                    }
                    None => run = Some((i, n)),
                }
            }

            flush(&mut run, nx, &mut out);
        }

        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pile_slumps_to_its_angle_and_keeps_its_volume() {
        let mut h = Heap::new(&Aabb { min: V3::new(-5.0, 0.0, -5.0), max: V3::new(5.0, 1.0, 5.0) }, 0.5);
        h.deposit(0.0, 0.0, 40.0, 1.0);
        let vol = |h: &Heap| h.h.iter().sum::<f32>() * h.step * h.step;
        let before = vol(&h);
        assert!((before - 40.0).abs() / 40.0 < 0.25, "{before}");
        h.relax(500);
        assert!((vol(&h) - before).abs() < 1e-2);
        assert!(h.max_height() < 10.0);
        let mesh = h.mesh(0.05);
        assert!(!mesh.is_empty());
        for t in &mesh {
            let n = (t[1].0 - t[0].0).cross(&(t[2].0 - t[0].0));
            assert!(n.y >= 0.0, "faces up");
        }
        assert!(!h.platforms(2.0, 0.15, 1.0).is_empty());
    }
}
