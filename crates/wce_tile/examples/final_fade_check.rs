//! BOB's `final_heights.dds` against a tile's editing heights: how far inside the tile they agree, the edge fade's weight by distance
//! (final / editing, where the editing height is not near 0), and how two edges combine at a corner (the weight against the product
//! and the minimum of the two edges' weights).
//!
//!   cargo run --release -p wce_tile --example final_fade_check -- <tile folder> [...]

use std::path::PathBuf;

use wce_tile::tile_project::{FinalHeights, TileProject};

fn main() {
    for dir in std::env::args().skip(1).map(PathBuf::from) {
        let p = match TileProject::open(&dir) {
            Ok(p) => p,
            Err(e) => {
                println!("{}: {e}", dir.display());
                continue;
            }
        };

        let (ex, ez) = (p.extent_x(), p.extent_z());
        let Some(f) = std::fs::read(dir.join("final_heights.dds")).ok().and_then(|b| FinalHeights::parse(&b, ex, ez)) else {
            println!("{}: no final_heights.dds", dir.display());
            continue;
        };

        // The interior: the largest difference more than 64 m from every edge.
        let (mut worst, mut n) = (0.0f64, 0);
        let mut z = 70.0;
        while z < ez - 70.0 {
            let mut x = 70.0;
            while x < ex - 70.0 {
                worst = worst.max((f.at(x, z) - p.height_at(x, z)).abs());
                n += 1;
                x += 7.0;
            }

            z += 7.0;
        }

        println!("{}: {ex} x {ez} m, interior {n} samples, largest difference {worst:.3} m", dir.display());

        // The weight by distance from the west edge, over rows away from the corners (median of final / editing where |editing| > 2 m).
        let weight = |d: f64, pick: &dyn Fn(f64, f64) -> (f64, f64)| {
            let mut r = vec![];
            let mut t = 80.0;
            while t < ex.min(ez) - 80.0 {
                let (x, z) = pick(d, t);
                let e = p.height_at(x, z);
                if e.abs() > 2.0 {
                    r.push(f.at(x, z) / e);
                }

                t += 3.0;
            }

            r.sort_by(f64::total_cmp);
            r.get(r.len() / 2).copied()
        };

        let west = |d: f64, t: f64| (d, t);
        let line: Vec<String> = [0.0, 2.0, 4.0, 8.0, 12.0, 16.0, 24.0, 32.0, 40.0, 48.0, 56.0, 63.0, 64.0, 70.0]
            .iter().map(|&d| format!("{d}:{}", weight(d, &west).map_or("-".to_owned(), |w| format!("{w:.4}")))).collect();
        println!("  west edge weight by distance: {}", line.join(" "));

        // A corner: the weight at (dx, dz) against w(dx) * w(dz) and min(w(dx), w(dz)), w from the west edge's medians.
        let w1 = |d: f64| weight(d, &west).unwrap_or(f64::NAN);
        for (dx, dz) in [(8.0, 8.0), (16.0, 32.0), (32.0, 32.0), (16.0, 48.0), (48.0, 48.0)] {
            let e = p.height_at(dx, dz);
            if e.abs() > 2.0 {
                println!("  corner ({dx}, {dz}): {:.4}, product {:.4}, min {:.4}", f.at(dx, dz) / e, w1(dx) * w1(dz), w1(dx).min(w1(dz)));
            }
        }
    }
}
