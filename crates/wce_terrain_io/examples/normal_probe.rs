//! Exploration: where a game LF normal chunk (`normals/normal_N.dds`, 576 x 576 DXT5) lies in a kit map's `normal.dds` (ARGB 32-bit),
//! and how its channels map. Prints the best offset per chunk and the per-channel error there.
//!
//!   cargo run --release -p wce_terrain_io --example normal_probe -- <kit normal.dds> <chunk.dds> <chunk index> [columns]

use wce_formats::dds_decode::{decode_bc1_block, decode_bc3_alpha_block};

fn dxt5(b: &[u8]) -> (u32, u32, Vec<[u8; 4]>) {
    let u = |o: usize| u32::from_le_bytes(b[o..o + 4].try_into().unwrap());
    let (h, w) = (u(12), u(16));
    let mut px = vec![[0u8; 4]; (w * h) as usize];
    let data = &b[128..];
    let bw = w.div_ceil(4);
    for by in 0..h.div_ceil(4) {
        for bx in 0..bw {
            let blk = &data[((by * bw + bx) * 16) as usize..][..16];
            let a = decode_bc3_alpha_block(&blk[..8]);
            let c = decode_bc1_block(&blk[8..], false);
            for i in 0..16 {
                let (x, y) = (bx * 4 + (i as u32 % 4), by * 4 + (i as u32 / 4));
                if x < w && y < h {
                    px[(y * w + x) as usize] = [c[i][0], c[i][1], c[i][2], a[i]];
                }
            }
        }
    }

    (w, h, px)
}

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let big = std::fs::read(&a[0]).expect("normal.dds");
    let u = |o: usize| u32::from_le_bytes(big[o..o + 4].try_into().unwrap());
    let (bh, bw) = (u(12), u(16));
    let masks = (u(92), u(96), u(100), u(104));
    println!("normal.dds {bw} x {bh}, bits {}, masks {:08x} {:08x} {:08x} {:08x}", u(88), masks.0, masks.1, masks.2, masks.3);
    let at = |x: i64, y: i64| -> Option<[u8; 4]> {
        if x < 0 || y < 0 || x >= i64::from(bw) || y >= i64::from(bh) {
            return None;
        }

        let o = 128 + ((y as usize) * bw as usize + x as usize) * 4;
        // BGRA in the file.
        Some([big[o + 2], big[o + 1], big[o], big[o + 3]])
    };

    let (w, h, px) = dxt5(&std::fs::read(&a[1]).expect("chunk"));
    let k: i64 = a[2].parse().unwrap();
    let cols: i64 = a.get(3).and_then(|v| v.parse().ok()).unwrap_or(32);
    let (col, row) = (k % cols, k / cols);
    println!("chunk {k}: {w} x {h}, col {col} row {row}; centre pixel {:?}", px[(h / 2 * w + w / 2) as usize]);
    let mut best = (f64::MAX, 0i64, 0i64, 0usize, false);
    // Candidate perms of the chunk's channels onto normal.dds's, and a vertical flip of the big image.
    let perms: [[usize; 3]; 6] = [[0, 1, 2], [0, 2, 1], [1, 0, 2], [1, 2, 0], [2, 0, 1], [2, 1, 0]];
    for flip in [false, true] {
        for dy in (-96..=96).step_by(4) {
            for dx in (-96..=96).step_by(4) {
                let (ox, oy) = (col * 256 + dx, row * 256 + dy);
                for (pi, p) in perms.iter().enumerate() {
                    let (mut err, mut n) = (0f64, 0);
                    for sy in (0..h as i64).step_by(23) {
                        for sx in (0..w as i64).step_by(23) {
                            let by = if flip { i64::from(bh) - 1 - (oy + sy) } else { oy + sy };
                            let Some(q) = at(ox + sx, by) else { continue };
                            let c = px[(sy * i64::from(w) + sx) as usize];
                            for ch in 0..3 {
                                err += (f64::from(c[p[ch]]) - f64::from(q[ch])).abs();
                            }
                            n += 1;
                        }
                    }

                    if n > 200 {
                        let e = err / f64::from(n * 3);
                        if e < best.0 {
                            best = (e, dx, dy, pi, flip);
                        }
                    }
                }
            }
        }
    }

    println!("best: mean error {:.2} at offset ({}, {}) from (col*256, row*256), perm {:?}, flip {}", best.0, best.1, best.2, perms[best.3], best.4);
    let (ox, oy) = (col * 256 + best.1, row * 256 + best.2);
    for (sx, sy) in [(0, 0), (288, 288), (575, 575), (100, 400)] {
        let by = if best.4 { i64::from(bh) - 1 - (oy + sy) } else { oy + sy };
        println!("  ({sx}, {sy}): chunk {:?} normal.dds {:?}", px[(sy * i64::from(w) + sx) as usize], at(ox + sx, by));
    }
}
