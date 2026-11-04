//! Exploration: the vertical scale the game draws a campaign map's LF at, from its own `lf_normal.dds` (DXT5, the half-size LF's grid:
//! x in alpha, z in green) and BOB's `lf_height_map.dds` (the same grid, L16): per sloped sample the normal's tilt `|n_xz| / n_y` over the
//! LF's slope in LF metres (`v * lf_height`) a scene unit (128 a cell, `cells_per_sample` cells a sample); the median is how many scene
//! units of height an LF metre is drawn as (times the normal's own scale, `normal_lf_scale`).
//!
//!   cargo run --release -p wce_terrain_io --example campaign_normal_scale -- <lf_normal.dds> <lf_height_map.dds> <lf_height> <samples a cell>

use wce_formats::dds_decode::dds_decode_mip;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let normal = dds_decode_mip(&std::fs::read(&a[0]).expect("normal"), 0).expect("dds");
    let lf = std::fs::read(&a[1]).expect("lf");
    let lf_height: f64 = a[2].parse().expect("lf_height");
    let per_cell: f64 = a[3].parse().expect("samples a cell");
    let u = |o: usize| u32::from_le_bytes(lf[o..o + 4].try_into().unwrap());
    let (h, w) = (u(12), u(16));
    println!("normal {} x {}, lf {w} x {h}", normal.w, normal.h);
    let at = |x: u32, y: u32| f64::from(u16::from_le_bytes([lf[128 + 2 * (y * w + x) as usize], lf[129 + 2 * (y * w + x) as usize]])) / 65535.0 * lf_height;
    let step = 128.0 / per_cell;
    let mut ratios = vec![];
    let mut channels = [[0f64; 2]; 4];
    for y in (8..h.min(normal.h) - 8).step_by(7) {
        for x in (8..w.min(normal.w) - 8).step_by(7) {
            let (gx, gz) = ((at(x + 1, y) - at(x - 1, y)) / (2.0 * step), (at(x, y + 1) - at(x, y - 1)) / (2.0 * step));
            let g = gx.hypot(gz);
            if g < 0.02 {
                continue;
            }

            let p = normal.at(x, y);
            if std::env::var("WCE_SHOW").is_ok() && ratios.len() < 3 && channels[0][1] < 5.0 {
                channels[0][1] += 1.0;
                println!("  at {x} {y}: pixel {p:?} slope {gx:.3} {gz:.3}");
            }

            // The campaign's DXT5 (x in alpha, z in green) or the battle's RGB normal (x red, z green, up blue; alpha 255).
            let rgb = std::env::var("WCE_RGB").is_ok();
            let (nx, nz) = (f64::from(if rgb { p[0] } else { p[3] }) / 127.5 - 1.0, f64::from(p[1]) / 127.5 - 1.0);
            let xz = nx.hypot(nz);
            if xz >= 0.999 {
                continue;
            }

            let ny = (1.0 - xz * xz).sqrt();
            ratios.push((xz / ny) / g);
            // Which channel follows which slope (a sanity check of the axes).
            channels[0][0] += f64::from(p[3]) * gx;
            channels[1][0] += f64::from(p[1]) * gz;
            channels[2][0] += f64::from(p[3]) * gz;
            channels[3][0] += f64::from(p[1]) * gx;
        }
    }

    ratios.sort_by(f64::total_cmp);
    if ratios.is_empty() {
        println!("no sloped samples with a tilted normal");
        return;
    }

    let q = |f: f64| ratios[((ratios.len() as f64 * f) as usize).min(ratios.len() - 1)];
    println!("{} sloped samples: tilt / slope: 25 % {:.3}, median {:.3}, 75 % {:.3}", ratios.len(), q(0.25), q(0.5), q(0.75));
    println!("channel x slope sums (alpha~x, green~z, alpha~z, green~x): {:?}", channels.map(|c| c[0] as i64));
    let _ = &mut channels;
}
