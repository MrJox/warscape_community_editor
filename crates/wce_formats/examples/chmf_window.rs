//! A window of a CHMF height file's samples (`hf_height_map.data`, the battle map's `lf_height_map.data`), as the raw 16-bit values.
//!
//!   cargo run --release -p wce_formats --example chmf_window -- <file.data> <x0> <y0> <x1> <y1>      (samples, end exclusive)

use wce_formats::chmf::ChmfFile;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let n: Vec<u32> = a.iter().skip(1).filter_map(|v| v.parse().ok()).collect();
    let (Some(path), &[x0, y0, x1, y1]) = (a.first(), &n[..]) else {
        eprintln!("chmf_window <file.data> <x0> <y0> <x1> <y1>");
        std::process::exit(2);
    };

    let f = ChmfFile::parse(&std::fs::read(path).expect("read")).expect("a CHMF");
    let mut grid = vec![None; ((x1 - x0) * (y1 - y0)) as usize];
    for (k, r) in f.regions.iter().enumerate() {
        let (sx, sy) = (r.rect[0] * f.cells_x, r.rect[1] * f.cells_y);
        let (ex, ey) = (r.rect[2] * f.cells_x, r.rect[3] * f.cells_y);
        if ex <= x0 || sx >= x1 || ey <= y0 || sy >= y1 {
            continue;
        }

        let (w, _, s) = f.region_samples(k).expect("a region");
        for y in y0.max(sy)..y1.min(ey) {
            for x in x0.max(sx)..x1.min(ex) {
                grid[((y - y0) * (x1 - x0) + x - x0) as usize] = Some(s[((y - sy) * w + x - sx) as usize]);
            }
        }
    }

    println!("{} x {} samples, {} per cell, min {} max {}", f.width, f.height, f.cells_x, f.min, f.max);
    for y in y0..y1 {
        let row: Vec<String> = (x0..x1).map(|x| grid[((y - y0) * (x1 - x0) + x - x0) as usize].map_or("    -".to_owned(), |v| format!("{v:5}"))).collect();
        println!("{y:5} {}", row.join(" "));
    }
}
