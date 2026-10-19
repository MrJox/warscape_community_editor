//! Which tiles of a kit open with another min_height / scale than their entry's (the height map fits BOB's final_heights.dds with other numbers):
//! `TileProject::calibrate_from_final_heights`. Lists them with both sets of numbers and counts the tiles that have a final_heights.dds and the
//! ones that open with the entry's numbers.
//!
//!   cargo run --release -p wce_tile --example height_calibration_check -- <folder with the tiles, e.g. <kit>/raw_data/terrain/tiles>

use std::path::{Path, PathBuf};

use rayon::prelude::*;
use wce_tile::tile_project::TileProject;

fn teds(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            teds(&p, out);
        } else if p.file_name().is_some_and(|n| n == "tile.ted") {
            out.push(p.parent().map(Path::to_path_buf).unwrap_or_default());
        }
    }
}

fn main() {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    let mut dirs = vec![];
    teds(Path::new(&args[0]), &mut dirs);
    dirs.sort();
    let rows = dirs.par_iter().map(|d| {
        let has_final = d.join("final_heights.dds").exists();
        match TileProject::open(d) {
            Ok(p) => (d.clone(), has_final, p.height.is_some() && p.info.found, p.height_calibration.clone(), String::new()),
            Err(e) => (d.clone(), has_final, false, String::new(), e),
        }
    }).collect::<Vec<_>>();

    let (mut with_final, mut calibrated, mut failed) = (0, 0, 0);
    for (d, has_final, usable, note, err) in &rows {
        with_final += usize::from(*has_final && *usable);
        if !err.is_empty() {
            failed += 1;
            println!("FAILED {}: {err}", d.display());
        } else if !note.is_empty() {
            calibrated += 1;
            println!("{}: {note}", d.display());
        }
    }

    println!("{} tiles, {with_final} with a final_heights.dds and an entry, {calibrated} calibrated, {failed} failed", rows.len());
}
