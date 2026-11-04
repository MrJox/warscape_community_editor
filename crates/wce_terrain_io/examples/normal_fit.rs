//! Exploration: how a kit map's `normal.dds` relates to its `lf_heights.tif`: for row offsets and gradient conventions, the correlation of
//! the normal's x and y with the LF's differences, and the slope factor (normal = normalize(-k dx, -k dy, 1)).
//!
//!   cargo run --release -p wce_terrain_io --example normal_fit -- <map folder>

use std::path::PathBuf;

use wce_formats::image_containers::TiffFile;

fn main() {
    let dir = PathBuf::from(std::env::args().nth(1).expect("map folder"));
    let big = std::fs::read(dir.join("normal.dds")).expect("normal.dds");
    let u = |o: usize| u32::from_le_bytes(big[o..o + 4].try_into().unwrap());
    let (nh, nw) = (u(12) as i64, u(16) as i64);
    let t = TiffFile::parse(&std::fs::read(dir.join("lf_heights.tif")).expect("lf")).expect("tiff");
    let (lw, lh) = (i64::from(t.width), i64::from(t.height));
    let lf: Vec<f64> = t.pixels.chunks_exact(2).map(|c| f64::from(u16::from_le_bytes([c[0], c[1]]))).collect();
    println!("normal.dds {nw} x {nh}, LF {lw} x {lh}");
    let n = |x: i64, y: i64| {
        let o = 128 + ((y * nw + x) * 4) as usize;
        [f64::from(big[o + 2]) / 127.5 - 1.0, f64::from(big[o + 1]) / 127.5 - 1.0, f64::from(big[o]) / 127.5 - 1.0]
    };
    let l = |x: i64, y: i64| lf[(y.clamp(0, lh - 1) * lw + x.clamp(0, lw - 1)) as usize];

    let dy = 0i64;
    let flip = false;
    for span in [1i64, 2, 4, 8, 16, 32, 64] {
        {
            // Correlations of nx with dx and ny with dy (central differences of the LF), over a grid of samples.
            let (mut sxx, mut sxy, mut syy, mut cnt) = (0f64, 0f64, 0f64, 0f64);
            let (mut syy2, mut sxy2, mut sxx2) = (0f64, 0f64, 0f64);
            let mut ratios = vec![];
            for y in (40..nh.min(lh) - 40).step_by(37) {
                for x in (40..nw.min(lw) - 40).step_by(41) {
                    let ly = if flip { lh - 1 - (y + dy) } else { y + dy };
                    let gx = (l(x + span, ly) - l(x - span, ly)) / (2.0 * span as f64);
                    let gy = (l(x, ly + span) - l(x, ly - span)) / (2.0 * span as f64);
                    let v = n(x, y);
                    sxx += gx * gx;
                    sxy += gx * v[0];
                    syy += v[0] * v[0];
                    sxx2 += gy * gy;
                    sxy2 += gy * v[1];
                    syy2 += v[1] * v[1];
                    cnt += 1.0;
                    if gx.abs() > 30.0 && v[2] > 0.05 {
                        ratios.push(-(v[0] / v[2]) / gx);
                    }
                }
            }

            ratios.sort_by(|a, b| a.partial_cmp(b).unwrap());
            let med = ratios.get(ratios.len() / 2).copied().unwrap_or(0.0);
            println!(
                "span {span} (row offset {dy:+3} flip {flip}): corr(nx, dx) {:+.3}, corr(ny, dy) {:+.3}, median -(nx/nz)/dx {med:.6} ({} samples, {cnt})",
                sxy / (sxx * syy).sqrt(), sxy2 / (sxx2 * syy2).sqrt(), ratios.len()
            );
        }
    }
}
