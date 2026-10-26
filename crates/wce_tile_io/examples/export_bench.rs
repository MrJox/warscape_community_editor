//! The export's time (Save as exported tile, Launch in game: stages A, B and C and the tile database) on kit tiles, and its output for
//! byte-for-byte comparisons between builds: each tile is exported into `<out>/<n>/` (n: its place in the list), then the scope timers
//! are printed.
//!
//!   cargo run --release -p wce_tile_io --example export_bench -- <game folder> <out dir> <project folder> [<project folder> ...]
//!
//! A project folder is a kit's variation folder (`.../raw_data/terrain/tiles/battle/plains/8x8/8x8_aa`); it is only read. `WCE_BENCH_RUNS`
//! repeats each export (the time printed is the best). `WCE_GAME_KEY` picks the game (Attila by default).

use std::path::PathBuf;
use std::time::Instant;

use wce_tile_io::attila::workflow::{Game, save_exported};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() < 3 {
        eprintln!("usage: export_bench <game folder> <out dir> <project folder> [<project folder> ...]");
        std::process::exit(2);
    }

    let runs: usize = std::env::var("WCE_BENCH_RUNS").ok().and_then(|v| v.parse().ok()).unwrap_or(1).max(1);
    let t = Instant::now();
    let game = Game::load(wce_foundation::game::GameProfile::from_env(), &PathBuf::from(&args[0])).unwrap_or_else(|e| panic!("game: {e}"));
    println!("game data: {:.2} s", t.elapsed().as_secs_f64());
    if std::env::var("WCE_BENCH_GAME_ONLY").is_ok() {
        println!("{}", wce_foundation::profile::report(false, 1.0));
        return;
    }

    let out = PathBuf::from(&args[1]);
    let mut total = 0.0;
    let mut failed = false;
    for (n, project) in args[2..].iter().enumerate() {
        let dir = out.join(n.to_string());
        let mut best = f64::MAX;
        for _ in 0..runs {
            let _ = std::fs::remove_dir_all(&dir);
            let t = Instant::now();
            if let Err(e) = save_exported(&game, &PathBuf::from(project), &dir) {
                eprintln!("{project}: {e}");
                failed = true;
                break;
            }

            best = best.min(t.elapsed().as_secs_f64());
        }

        total += best;
        println!("{n}: {best:7.2} s  {project}");
    }

    println!("total: {total:.2} s");
    println!("{}", wce_foundation::profile::report(false, 1.0));
    if failed {
        std::process::exit(1);
    }
}
