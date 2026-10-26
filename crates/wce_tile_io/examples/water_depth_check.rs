//! The water depth under the painted water ground types: for raw battle tiles (kit layout), BOB's water surface over its final heights
//! (`terrain_mesh::water_fields`) at every pixel of `ground_type_map.png` painted deep water (0, 0, 255), shallow water (93, 255, 255) or
//! nothing, as depth percentiles. What the water depth view's auto-paint threshold is measured from (docs/plans/tile_ted_parity.md, 4d).
//!
//!   cargo run --release -p wce_tile_io --example water_depth_check -- <kit raw_data> <raw tile folder>...

use std::path::PathBuf;

use wce_formats::png_file::PngFile;
use wce_tile_io::attila::TedProject;
use wce_tile_io::attila::terrain_mesh::water_fields;
use wce_tile_io::{TileFiles, TileFormat};

fn main() {
    let args = std::env::args().skip(1).map(PathBuf::from).collect::<Vec<_>>();
    let kit = &args[0];
    let models = wce_tile_io::attila::stage_b::pack_models(&kit.join("../../data"));
    let (mut deep, mut shallow, mut none) = (vec![], vec![], vec![]);
    for dir in &args[1..] {
        let files = TileFiles::from_kit_dir(dir).expect("the raw tile");
        let (project, _) = TedProject.read(&files).expect("the project");
        let f = match water_fields(&project, kit, &models) {
            Ok(f) => f,
            Err(e) => {
                println!("{}: {e}", dir.display());
                continue;
            }
        };

        let Some(water) = &f.water else { continue };
        let Ok(png) = std::fs::read(dir.join("ground_type_map.png")).map_err(|e| e.to_string()).and_then(|b| PngFile::parse(&b).map_err(|e| e.to_string())) else { continue };
        let bpp = png.bytes_per_pixel();
        let (pw, ph) = (png.width as usize, png.height as usize);
        // The map is bordered by one cell; the grid is the tile's own vertices, one per pixel at the same density.
        let border = (pw - (f.w - 1)) / 2;
        let (mut d, mut s, mut n) = (0, 0, 0);
        for j in 0..f.h {
            for i in 0..f.w {
                let k = j * f.w + i;
                if water[k] <= -999.0 {
                    continue;
                }

                let depth = water[k] - f.heights[k];
                let (px, py) = (i + border, j + border);
                if px >= pw || py >= ph {
                    continue;
                }

                let c = &png.pixels[(py * pw + px) * bpp..(py * pw + px) * bpp + 3];
                match c {
                    [0, 0, 255] => (deep.push(depth), d += 1),
                    [93, 255, 255] => (shallow.push(depth), s += 1),
                    _ => (none.push(depth), n += 1),
                };
            }
        }

        println!("{}: {} x {} grid, water pixels painted deep {d}, shallow {s}, unpainted {n}", dir.display(), f.w, f.h);
    }

    let pct = |v: &mut Vec<f32>| {
        if v.is_empty() {
            return "none".to_owned();
        }

        v.sort_by(f32::total_cmp);
        let at = |p: f64| v[((v.len() - 1) as f64 * p) as usize];
        format!("{} px: min {:.2}  p5 {:.2}  p25 {:.2}  median {:.2}  p75 {:.2}  p95 {:.2}  max {:.2}", v.len(), at(0.0), at(0.05), at(0.25), at(0.5), at(0.75), at(0.95), at(1.0))
    };

    println!("deep water:    {}", pct(&mut deep));
    println!("shallow water: {}", pct(&mut shallow));
    println!("unpainted:     {}", pct(&mut none));
}
