//! The building technology overlays' gate (tile_document/tech.rs): every `<key>_tech.cs2.parsed` of the game's building folders reads into
//! shapes (the file decodes, nothing panics), and what the files hold, by kind of shape: how many files have each, how many shapes and points
//! in all. A file that does not read is listed and fails the gate.
//!
//!   cargo run --release -p wce_tile_editor --example tech_check -- <game folder> [--mods a.pack;b.pack] [--show N]
//!
//! `$WCE_GAME_KEY` picks the game (Attila by default). `--show N` lists the N files with the most points (what the overlay's line limit meets).

use std::path::PathBuf;
use std::time::Instant;

use wce_tile_editor::tile_document::{KINDS, tech_counts};
use wce_vfs::{Config, GameProfile, Vfs};

fn main() -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mods = args.iter().position(|a| a == "--mods").and_then(|i| args.get(i + 1)).cloned();
    let show = args.iter().position(|a| a == "--show").and_then(|i| args.get(i + 1)).and_then(|n| n.parse::<usize>().ok()).unwrap_or(5);
    let Some(game) = args.first().filter(|a| !a.starts_with("--")) else { return Err("usage: tech_check <game folder> [--mods a.pack;b.pack] [--show N]".to_owned()) };

    let mut config = Config::new(PathBuf::from(game), GameProfile::from_env().clone());
    config.mod_packs = mods.map(|m| m.split(';').filter(|s| !s.is_empty()).map(str::to_owned).collect());
    let vfs = Vfs::mount(&config).map_err(|e| e.to_string())?;

    let t0 = Instant::now();
    let paths = vfs.list("rigidmodels/buildings/", "_tech.cs2.parsed");
    let (mut files_with, mut shapes, mut bad) = ([0usize; 15], [0usize; 15], vec![]);
    let mut by_points: Vec<(usize, String)> = vec![];
    for path in &paths {
        let bytes = match vfs.read(path) {
            Ok(b) => b,
            Err(e) => {
                bad.push(format!("{path}: {e}"));
                continue;
            }
        };

        match tech_counts(&bytes) {
            Ok((counts, points)) => {
                for k in 0..15 {
                    files_with[k] += usize::from(counts[k] > 0);
                    shapes[k] += counts[k];
                }

                by_points.push((points, path.to_string()));
            }

            Err(e) => bad.push(format!("{path}: {e}")),
        }
    }

    println!("{} logic file(s), {} read, {:.1} s", paths.len(), paths.len() - bad.len(), t0.elapsed().as_secs_f64());
    println!("{:<20} {:>8} {:>10}", "kind", "files", "shapes");
    for (k, kind) in KINDS.iter().enumerate() {
        println!("{:<20} {:>8} {:>10}", kind.info().name, files_with[k], shapes[k]);
    }

    by_points.sort_by_key(|p| std::cmp::Reverse(p.0));
    println!("most points:");
    for (points, path) in by_points.iter().take(show) {
        println!("  {points:>8}  {path}");
    }

    for b in &bad {
        println!("FAIL {b}");
    }

    if bad.is_empty() { Ok(()) } else { Err(format!("{} file(s) do not read", bad.len())) }
}
