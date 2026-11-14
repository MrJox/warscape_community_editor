//! Exploration: a game normal chunk against the game's own LF (`lf_height_map.dds`, L16): for difference spans and orientations, the
//! correlation of the chunk's x (alpha) and y (green) with the LF's differences, and the slope factor.
//!
//!   cargo run --release -p wce_terrain_io --example normal_fit2 -- <lf_height_map.dds> <chunk.dds> <chunk index>

use wce_formats::dds_decode::{decode_bc1_block, decode_bc3_alpha_block};

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let lf = std::fs::read(&a[0]).expect("lf");
    let u = |b: &[u8], o: usize| u32::from_le_bytes(b[o..o + 4].try_into().unwrap());
    let (lh, lw) = (i64::from(u(&lf, 12)), i64::from(u(&lf, 16)));
    let h = |x: i64, y: i64| {
        let (x, y) = (x.clamp(0, lw - 1), y.clamp(0, lh - 1));
        let o = 128 + ((y * lw + x) * 2) as usize;
        f64::from(u16::from_le_bytes([lf[o], lf[o + 1]]))
    };

    let c = std::fs::read(&a[1]).expect("chunk");
    let (ch, cw) = (u(&c, 12) as usize, u(&c, 16) as usize);
    let mut px = vec![[0u8; 2]; cw * ch];
    let bw = cw / 4;
    for by in 0..ch / 4 {
        for bx in 0..bw {
            let blk = &c[128 + (by * bw + bx) * 16..][..16];
            let al = decode_bc3_alpha_block(&blk[..8]);
            let co = decode_bc1_block(&blk[8..], false);
            for i in 0..16 {
                px[(by * 4 + i / 4) * cw + bx * 4 + i % 4] = [al[i], co[i][1]];
            }
        }
    }

    let k: i64 = a[2].parse().unwrap();
    let (ox, oy) = ((k % 32) * 256 - 32, (k / 32) * 256 - 32);
    println!("LF {lw} x {lh}; chunk {k} {cw} x {ch} at ({ox}, {oy})");
    for span in [1i64, 2, 4] {
        for (name, sx, sy) in [("x+ y+", 1.0, 1.0), ("x- y+", -1.0, 1.0), ("x+ y-", 1.0, -1.0), ("x- y-", -1.0, -1.0)] {
            let (mut a1, mut b1, mut c1, mut a2, mut b2, mut c2) = (0f64, 0f64, 0f64, 0f64, 0f64, 0f64);
            let mut ratios = vec![];
            for y in (8..ch as i64 - 8).step_by(3) {
                for x in (8..cw as i64 - 8).step_by(3) {
                    let (lx, ly) = (ox + x, oy + y);
                    if lx < span || ly < span || lx >= lw - span || ly >= lh - span {
                        continue;
                    }

                    let gx = sx * (h(lx + span, ly) - h(lx - span, ly)) / (2.0 * span as f64);
                    let gy = sy * (h(lx, ly - span) - h(lx, ly + span)) / (2.0 * span as f64);
                    let p = px[y as usize * cw + x as usize];
                    let (nx, ny) = (f64::from(p[0]) / 127.5 - 1.0, f64::from(p[1]) / 127.5 - 1.0);
                    a1 += gx * nx;
                    b1 += gx * gx;
                    c1 += nx * nx;
                    a2 += gy * ny;
                    b2 += gy * gy;
                    c2 += ny * ny;
                    let nz = (1.0 - nx * nx - ny * ny).max(1e-6).sqrt();
                    if gx.abs() > 20.0 {
                        ratios.push((nx / nz) / gx);
                    }
                }
            }

            ratios.sort_by(|a, b| a.partial_cmp(b).unwrap());
            println!(
                "  span {span} {name}: corr x {:+.3}, corr y {:+.3}, median (nx/nz)/dx {:.7} ({} samples)",
                a1 / (b1 * c1).sqrt(),
                a2 / (b2 * c2).sqrt(),
                ratios.get(ratios.len() / 2).copied().unwrap_or(0.0),
                ratios.len()
            );
        }
    }
}
