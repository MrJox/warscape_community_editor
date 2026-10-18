//! The height alpha map BOB multiplies a list's heights by (`WS_SCENE_NODE_TERRAIN_EDITOR_V2::build_height_alpha_map` and
//! `build_hf_influence_map`, Warscape.AssemblyKit.dll): 1 inside the tile, falling to 0 over the outer half of every subtile whose neighbour
//! on that side is not terrain (an invalid subtile, the tile's edge) unless a tile link joins there. docs/re/vegetation.md has the RE.

use std::collections::HashSet;

/// `FUN_104bdef0`: the influence along one axis of a subtile, `a` at the low edge and `b` at the high one, rising to 1 over half the subtile.
fn edge(size: u32, i: u32, a: f32, b: f32) -> f32 {
    let half = ((f64::from(size) + 0.5) * 0.5) as f32;
    let (d, at) = if f64::from(size + 1) * 0.5 > f64::from(i) { (f64::from(i), a) } else { (f64::from(size - i), b) };
    let f = ((d - 2.0) as f32).clamp(0.0, 5000.0);
    (1.0 - at) * (f / half) + at
}

/// `__libm_sse2_powf`: computed in double, rounded once.
fn powf(base: f32, exp: f32) -> f32 {
    f64::from(base).powf(f64::from(exp)) as f32
}

/// The map, `(width + 2) * size + 1` by `(height + 2) * size + 1` (a subtile of border all round), row 0 at generator y 0.
pub struct HeightAlpha {
    pub width: usize,
    pub height: usize,
    pub values: Vec<f32>,
}

impl HeightAlpha {
    /// `size` is the subtile in pixels; `cells` the valid subtiles (x, y as the generator stores them); `links` the tile links that blend
    /// (cell x, y of the tile database).
    pub fn build(width: u32, height: u32, size: u32, cells: &HashSet<(i32, i32)>, links: &[(i32, i32)]) -> Self {
        let (w, h) = (width as i32, height as i32);
        let fw = ((width + 2) * size + 1) as usize;
        let fh = ((height + 2) * size + 1) as usize;
        let mut values = vec![0.0f32; fw * fh];
        let valid = |x: i32, y: i32| -> f32 { if cells.contains(&(x, y)) { 1.0 } else { 0.0 } };
        let links: Vec<(i32, i32)> = links.iter().map(|&(x, y)| (x, h - y - 1)).collect();
        for y in 0..h {
            for x in 0..w {
                if valid(x, y) == 0.0 {
                    continue;
                }

                let mut up = if y == 0 { 0.0 } else { valid(x, y - 1) };
                let mut down = if y == h - 1 { 0.0 } else { valid(x, y + 1) };
                let mut left = if x == 0 { 0.0 } else { valid(x - 1, y) };
                let mut right = if x == w - 1 { 0.0 } else { valid(x + 1, y) };
                let mut ul = if x == 0 || y == 0 { 0.0 } else { valid(x - 1, y - 1) };
                let mut ur = if x == w - 1 || y == 0 { 0.0 } else { valid(x + 1, y - 1) };
                let mut dl = if x == 0 || y == h - 1 { 0.0 } else { valid(x - 1, y + 1) };
                let mut dr = if x == w - 1 || y == h - 1 { 0.0 } else { valid(x + 1, y + 1) };

                // A link on the tile's rim joins the neighbour there: no falloff on that side.
                for &(lx, ly) in &links {
                    if lx < 1 || lx >= w || ly < 1 || ly >= h {
                        if lx == x - 1 && ly == y {
                            left = 1.0;
                        }
                        if lx == x + 1 && ly == y {
                            right = 1.0;
                        }
                        if ly == y - 1 && lx == x {
                            up = 1.0;
                        }
                        if ly == y + 1 && lx == x {
                            down = 1.0;
                        }
                    }
                }

                if ul == 0.0 && left != up {
                    ul = 1.0;
                }
                if dl == 0.0 && left != down {
                    dl = 1.0;
                }
                if ur == 0.0 && right != up {
                    ur = 1.0;
                }
                if dr == 0.0 && right != down {
                    dr = 1.0;
                }

                for u in 0..=size {
                    let a = edge(size, u, up, down);
                    let row = (size + y as u32 * size + u) as usize;
                    for v in 0..=size {
                        let b = edge(size, v, left, right);
                        let c = edge(size, v, ul, ur);
                        let d = edge(size, v, dl, dr);
                        let e = edge(size, u, c, d);
                        let t = a * b * e * 2.1;
                        let p = powf(std::f32::consts::E, t * t);
                        let col = (size + x as u32 * size + v) as usize;
                        values[row * fw + col] = (1.0 - 1.0 / p) / f32::from_bits(0x3f7c_e366);
                    }
                }
            }
        }

        // Then the border subtile of every link (after the tile's, so on the shared edge the link's values stay): sides on the tile's rows /
        // columns, or next to another link point, are 1; corners where the two sides differ are 1; no 0.98784 normalisation.
        for &(lx, ly) in &links {
            let x_out = lx < 0 || lx >= w;
            let y_out = ly < 0 || ly >= h;
            let (mut up, mut down) = if x_out { (0.0f32, 0.0f32) } else { (1.0, 1.0) };
            let (mut left, mut right) = if y_out { (0.0f32, 0.0f32) } else { (1.0, 1.0) };
            for &(mx, my) in &links {
                if ly - 1 == my {
                    up = 1.0;
                }
                if ly + 1 == my {
                    down = 1.0;
                }
                if lx - 1 == mx {
                    left = 1.0;
                }
                if lx + 1 == mx {
                    right = 1.0;
                }
            }

            let ul = if left != up { 1.0 } else { 0.0 };
            let dl = if left != down { 1.0 } else { 0.0 };
            let ur = if right != up { 1.0 } else { 0.0 };
            let dr = if right != down { 1.0 } else { 0.0 };
            for u in 0..=size {
                let a = edge(size, u, up, down);
                let row = (ly + 1) * size as i32 + u as i32;
                for v in 0..=size {
                    let b = edge(size, v, left, right);
                    let c = edge(size, v, ul, ur);
                    let d = edge(size, v, dl, dr);
                    let e = edge(size, u, c, d);
                    let t = a * b * e * 2.1;
                    let col = (lx + 1) * size as i32 + v as i32;
                    if row >= 0 && col >= 0 && (row as usize) < fh && (col as usize) < fw {
                        values[row as usize * fw + col as usize] = 1.0 - 1.0 / powf(std::f32::consts::E, t * t);
                    }
                }
            }
        }

        Self { width: fw, height: fh, values }
    }

    pub fn get(&self, col: usize, row: usize) -> f32 {
        if col < self.width && row < self.height { self.values[row * self.width + col] } else { 0.0 }
    }
}
