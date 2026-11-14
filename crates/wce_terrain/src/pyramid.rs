//! A min / max pyramid over a field of 16-bit heights (a map's LF): where a ray meets the field, in a few box tests instead of a march
//! over the map (docs/plans/terrain_editor.md, "Per-page min / max and a pyramid"). The field is bilinear between its samples, as the
//! game's terrain vertex shader reads the LF; a cell's surface is met exactly (the ray's height against the bilinear height along it is a
//! quadratic).
//!
//! The field is solid below its surface: a ray that comes in through a side of the field (or from under it) meets it there.
//!
//! Space: x and z in samples (sample (i, j) at (i, j)), y in the field's raw units (0 .. 65535). A caller maps its world into it with a
//! scale per axis (a linear map keeps the ray's parameter).

use rayon::prelude::*;

/// Cells a side of a leaf block.
pub const LEAF: u32 = 8;

/// The bottom of every block's box: the field is solid below its surface.
const FLOOR: f32 = -1.0e30;

#[derive(Clone, Debug, Default)]
struct Level {
    /// Blocks across and down, and the cells a side of one.
    bw: u32,
    bh: u32,
    cells: u32,

    /// Min and max per block, row by row.
    mm: Vec<[u16; 2]>,
}

#[derive(Clone, Debug, Default)]
pub struct HeightPyramid {
    w: u32,
    h: u32,

    /// Level 0 the leaves, the last one block.
    levels: Vec<Level>,
}

/// The min and max of the samples a leaf block's cells touch (its corners included).
fn leaf_mm(w: u32, h: u32, bx: u32, by: u32, at: &(impl Fn(u32, u32) -> u16 + Sync)) -> [u16; 2] {
    let (x0, y0) = (bx * LEAF, by * LEAF);
    let (x1, y1) = ((x0 + LEAF).min(w - 1), (y0 + LEAF).min(h - 1));
    let mut mm = [u16::MAX, 0];
    for y in y0..=y1 {
        for x in x0..=x1 {
            let v = at(x, y);
            mm = [mm[0].min(v), mm[1].max(v)];
        }
    }

    mm
}

impl HeightPyramid {
    /// The pyramid of a `w` x `h` field whose sample (x, y) is `at(x, y)`.
    pub fn new(w: u32, h: u32, at: impl Fn(u32, u32) -> u16 + Sync) -> Self {
        if w < 2 || h < 2 {
            return Self { w, h, levels: vec![] };
        }

        let (bw, bh) = ((w - 1).div_ceil(LEAF), (h - 1).div_ceil(LEAF));
        let mut leaf = Level { bw, bh, cells: LEAF, mm: vec![[0; 2]; (bw * bh) as usize] };
        leaf.mm.par_chunks_mut(bw as usize).enumerate().for_each(|(by, row)| {
            for (bx, m) in row.iter_mut().enumerate() {
                *m = leaf_mm(w, h, bx as u32, by as u32, &at);
            }
        });

        let mut p = Self { w, h, levels: vec![leaf] };
        while p.levels.last().is_some_and(|l| l.bw > 1 || l.bh > 1) {
            let below = p.levels.last().expect("a level");
            let (bw, bh) = (below.bw.div_ceil(2), below.bh.div_ceil(2));
            let mut l = Level { bw, bh, cells: below.cells * 2, mm: vec![[u16::MAX, 0]; (bw * bh) as usize] };
            for y in 0..below.bh {
                for x in 0..below.bw {
                    let (c, m) = (below.mm[(y * below.bw + x) as usize], &mut l.mm[((y / 2) * bw + x / 2) as usize]);
                    *m = [m[0].min(c[0]), m[1].max(c[1])];
                }
            }

            p.levels.push(l);
        }

        p
    }

    /// The samples in a rectangle (x0, y0, x1, y1; end exclusive) changed: their blocks are measured again, up to the root.
    pub fn update(&mut self, rect: (u32, u32, u32, u32), at: impl Fn(u32, u32) -> u16 + Sync) {
        let Some(leaf) = self.levels.first_mut() else { return };
        let (w, h) = (self.w, self.h);

        // A sample is a corner of the cells on both sides of it: the blocks one sample around.
        let bx0 = rect.0.saturating_sub(1) / LEAF;
        let by0 = rect.1.saturating_sub(1) / LEAF;
        let bx1 = (rect.2 / LEAF).min(leaf.bw - 1);
        let by1 = (rect.3 / LEAF).min(leaf.bh - 1);
        for by in by0..=by1 {
            for bx in bx0..=bx1 {
                leaf.mm[(by * leaf.bw + bx) as usize] = leaf_mm(w, h, bx, by, &at);
            }
        }

        let (mut x0, mut y0, mut x1, mut y1) = (bx0, by0, bx1, by1);
        for k in 1..self.levels.len() {
            let (lo, hi) = self.levels.split_at_mut(k);
            let (below, l) = (&lo[k - 1], &mut hi[0]);
            (x0, y0, x1, y1) = (x0 / 2, y0 / 2, x1 / 2, y1 / 2);
            for y in y0..=y1 {
                for x in x0..=x1 {
                    let mut m = [u16::MAX, 0];
                    for (cx, cy) in [(2 * x, 2 * y), (2 * x + 1, 2 * y), (2 * x, 2 * y + 1), (2 * x + 1, 2 * y + 1)] {
                        if cx < below.bw && cy < below.bh {
                            let c = below.mm[(cy * below.bw + cx) as usize];
                            m = [m[0].min(c[0]), m[1].max(c[1])];
                        }
                    }

                    l.mm[(y * l.bw + x) as usize] = m;
                }
            }
        }
    }

    /// The lowest and highest sample of the whole field.
    pub fn range(&self) -> Option<[u16; 2]> {
        self.levels.last().and_then(|l| l.mm.first().copied())
    }

    /// Where the ray `o + t d` (t >= 0) first meets the field: its `t`. `at` gives the samples (the field the pyramid was made of).
    pub fn intersect(&self, o: [f32; 3], d: [f32; 3], at: impl Fn(u32, u32) -> u16) -> Option<f32> {
        let top = self.levels.len().checked_sub(1)?;
        let mut stack: Vec<(usize, u32, u32)> = vec![(top, 0, 0)];
        while let Some((k, bx, by)) = stack.pop() {
            let l = &self.levels[k];
            let mm = l.mm[(by * l.bw + bx) as usize];
            let (x0, z0) = ((bx * l.cells) as f32, (by * l.cells) as f32);
            let (x1, z1) = (((bx + 1) * l.cells).min(self.w - 1) as f32, ((by + 1) * l.cells).min(self.h - 1) as f32);
            // The field is solid below its surface (a ray that comes in through a side or from under it meets it there): a block's box
            // reaches down without end, up to its highest sample.
            let Some((ta, tb)) = slab(o, d, [x0, FLOOR, z0], [x1, f32::from(mm[1]), z1]) else { continue };
            if k == 0 {
                if let Some(t) = self.leaf_hit(o, d, (x0 as u32, z0 as u32, x1 as u32, z1 as u32), ta, tb, &at) {
                    return Some(t);
                }

                continue;
            }

            // The children, nearest last on the stack (taken first).
            let below = &self.levels[k - 1];
            let mut kids: Vec<(f32, u32, u32)> = vec![];
            for (cx, cy) in [(2 * bx, 2 * by), (2 * bx + 1, 2 * by), (2 * bx, 2 * by + 1), (2 * bx + 1, 2 * by + 1)] {
                if cx >= below.bw || cy >= below.bh {
                    continue;
                }

                let c = below.mm[(cy * below.bw + cx) as usize];
                let (a0, b0) = ((cx * below.cells) as f32, (cy * below.cells) as f32);
                let (a1, b1) = (((cx + 1) * below.cells).min(self.w - 1) as f32, ((cy + 1) * below.cells).min(self.h - 1) as f32);
                if let Some((t, _)) = slab(o, d, [a0, FLOOR, b0], [a1, f32::from(c[1]), b1]) {
                    kids.push((t, cx, cy));
                }
            }

            kids.sort_by(|a, b| b.0.total_cmp(&a.0));
            stack.extend(kids.into_iter().map(|(_, x, y)| (k - 1, x, y)));
        }

        None
    }

    /// The first hit in a leaf block's cells (x0, z0, x1, z1 in samples), the ray inside it over [ta, tb]: the cells it crosses in order
    /// (a 2D walk), each against its bilinear surface.
    fn leaf_hit(&self, o: [f32; 3], d: [f32; 3], b: (u32, u32, u32, u32), ta: f32, tb: f32, at: &impl Fn(u32, u32) -> u16) -> Option<f32> {
        let p = [o[0] + d[0] * ta, o[2] + d[2] * ta];
        let cell = |v: f32, lo: u32, hi: u32| (v.floor() as i64).clamp(i64::from(lo), i64::from(hi) - 1);
        let (mut cx, mut cz) = (cell(p[0], b.0, b.2), cell(p[1], b.1, b.3));
        let (sx, sz) = (if d[0] >= 0.0 { 1 } else { -1 }, if d[2] >= 0.0 { 1 } else { -1 });
        let next = |c: i64, s: i64, o: f32, d: f32| {
            if d == 0.0 {
                return f32::INFINITY;
            }

            let edge = if s > 0 { (c + 1) as f32 } else { c as f32 };
            (edge - o) / d
        };

        let mut t0 = ta;
        loop {
            let (tx, tz) = (next(cx, sx, o[0], d[0]), next(cz, sz, o[2], d[2]));
            let t1 = tx.min(tz).min(tb);
            if let Some(t) = cell_hit(o, d, cx as u32, cz as u32, t0, t1, at) {
                return Some(t);
            }

            if t1 >= tb {
                return None;
            }

            if tx < tz {
                cx += sx
            } else {
                cz += sz
            }
            if cx < i64::from(b.0) || cx >= i64::from(b.2) || cz < i64::from(b.1) || cz >= i64::from(b.3) {
                return None;
            }

            t0 = t1;
        }
    }
}

/// The ray's parameter range inside a box (t >= 0), none when it misses.
fn slab(o: [f32; 3], d: [f32; 3], lo: [f32; 3], hi: [f32; 3]) -> Option<(f32, f32)> {
    let (mut t0, mut t1) = (0.0f32, f32::INFINITY);
    for a in 0..3 {
        if d[a].abs() < 1e-12 {
            if o[a] < lo[a] || o[a] > hi[a] {
                return None;
            }

            continue;
        }

        let (u, v) = ((lo[a] - o[a]) / d[a], (hi[a] - o[a]) / d[a]);
        let (u, v) = if u < v { (u, v) } else { (v, u) };
        (t0, t1) = (t0.max(u), t1.min(v));
        if t0 > t1 {
            return None;
        }
    }

    Some((t0, t1))
}

/// Where the ray meets the bilinear surface of cell (cx, cz) for t in [t0, t1]: the smallest root of the ray's height less the surface's.
fn cell_hit(o: [f32; 3], d: [f32; 3], cx: u32, cz: u32, t0: f32, t1: f32, at: &impl Fn(u32, u32) -> u16) -> Option<f32> {
    let [h00, h10, h01, h11] = [at(cx, cz), at(cx + 1, cz), at(cx, cz + 1), at(cx + 1, cz + 1)].map(f64::from);

    // In the cell: u = ux + uv t, v = vx + vv t; h(u, v) = h00 + a u + b v + c u v.
    let (a, b, c) = (h10 - h00, h01 - h00, h11 - h10 - h01 + h00);
    let (ux, uv) = (f64::from(o[0]) - f64::from(cx), f64::from(d[0]));
    let (vx, vv) = (f64::from(o[2]) - f64::from(cz), f64::from(d[2]));
    let (oy, dy) = (f64::from(o[1]), f64::from(d[1]));

    // f(t) = oy + dy t - h(u(t), v(t)) = q2 t^2 + q1 t + q0.
    let q2 = -c * uv * vv;
    let q1 = dy - a * uv - b * vv - c * (ux * vv + vx * uv);
    let q0 = oy - h00 - a * ux - b * vx - c * ux * vx;
    let (t0, t1) = (f64::from(t0), f64::from(t1));
    let f = |t: f64| (q2 * t + q1) * t + q0;
    if f(t0) <= 0.0 {
        return Some(t0 as f32);
    }

    let mut roots = vec![];
    if q2.abs() < 1e-12 {
        if q1.abs() > 1e-12 {
            roots.push(-q0 / q1);
        }
    } else {
        let disc = q1 * q1 - 4.0 * q2 * q0;
        if disc >= 0.0 {
            let s = disc.sqrt();
            roots.extend([(-q1 - s) / (2.0 * q2), (-q1 + s) / (2.0 * q2)]);
        }
    }

    roots.into_iter().filter(|&t| t >= t0 && t <= t1).fold(None, |m: Option<f64>, t| Some(m.map_or(t, |m| m.min(t)))).map(|t| t as f32)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn field(w: u32, h: u32) -> Vec<u16> {
        (0..w * h)
            .map(|i| {
                let (x, y) = (f64::from(i % w), f64::from(i / w));
                (20_000.0 + 9_000.0 * (x * 0.21).sin() * (y * 0.17).cos() + 300.0 * (x * 1.7 + y * 2.3).sin()) as u16
            })
            .collect()
    }

    /// The ray's first point under the bilinear field by a fine march (the reference).
    fn march(v: &[u16], w: u32, h: u32, o: [f32; 3], d: [f32; 3]) -> Option<f32> {
        let bil = |x: f32, z: f32| {
            let (i, j) = ((x.floor() as u32).min(w - 2), (z.floor() as u32).min(h - 2));
            let (u, t) = (x - i as f32, z - j as f32);
            let s = |a: u32, b: u32| f32::from(v[(b * w + a) as usize]);
            (s(i, j) * (1.0 - u) + s(i + 1, j) * u) * (1.0 - t) + (s(i, j + 1) * (1.0 - u) + s(i + 1, j + 1) * u) * t
        };

        let mut t = 0.0f32;
        while t < 2000.0 {
            let (x, y, z) = (o[0] + d[0] * t, o[1] + d[1] * t, o[2] + d[2] * t);
            if x >= 0.0 && z >= 0.0 && x <= (w - 1) as f32 && z <= (h - 1) as f32 && y <= bil(x, z) {
                return Some(t);
            }

            t += 0.002;
        }

        None
    }

    #[test]
    fn rays_meet_the_field_where_a_march_does() {
        let (w, h) = (130, 97);
        let v = field(w, h);
        let at = |x: u32, y: u32| v[(y * w + x) as usize];
        let p = HeightPyramid::new(w, h, at);
        let mut checked = 0;
        for k in 0..240 {
            // Every direction: the four signs of x and z in turn, from inside and outside the field.
            let (sx, sz) = ([1.0, -1.0, 1.0, -1.0][k % 4], [1.0, 1.0, -1.0, -1.0][k % 4]);
            let k = k as f32;
            let o = [65.0 - sx * (20.0 + k * 0.3), 40_000.0, 48.0 - sz * (15.0 + k * 0.2)];
            let d = [sx * (0.6 + (k * 0.37).sin() * 0.3), -400.0 - k * 10.0, sz * (0.5 + (k * 0.23).cos() * 0.3)];
            let (got, want) = (p.intersect(o, d, at), march(&v, w, h, o, d));
            match (got, want) {
                (Some(a), Some(b)) => assert!((a - b).abs() < 0.01, "ray {k}: {a} against {b}"),
                (a, b) => assert_eq!(a.is_some(), b.is_some(), "ray {k}: {a:?} against {b:?}"),
            }

            checked += usize::from(want.is_some());
        }

        assert!(checked > 150);
    }

    #[test]
    fn an_update_follows_an_edit() {
        let (w, h) = (70, 70);
        let mut v = vec![1000u16; (w * h) as usize];
        let mut p = HeightPyramid::new(w, h, |x, y| v[(y * w + x) as usize]);
        assert_eq!(p.range(), Some([1000, 1000]));
        let o = [35.5, 9000.0, 35.5];
        assert!(p.intersect(o, [0.0, -1.0, 0.0], |x, y| v[(y * w + x) as usize]).is_some_and(|t| (t - 8000.0).abs() < 0.01));
        v[(35 * w + 35) as usize] = 5000;
        p.update((35, 35, 36, 36), |x, y| v[(y * w + x) as usize]);
        assert_eq!(p.range(), Some([1000, 5000]));
        // Over the raised sample's corner: the bilinear surface at (35.5, 35.5) is a quarter of the way up from the three others.
        let t = p.intersect(o, [0.0, -1.0, 0.0], |x, y| v[(y * w + x) as usize]).expect("a hit");
        assert!((t - (9000.0 - 2000.0)).abs() < 0.01, "{t}");
    }
}
