//! Lists the game's pack files under a prefix (with their sizes), or writes one out.
//!
//!   cargo run --release -p wce_vfs --example vfs_ls -- <game dir> <prefix> [--vanilla] [--extract <path> <out file>]
//!
//! `$WCE_GAME_KEY` chooses the game (`rome_2`; Attila by default). `--vanilla` leaves the enabled mods out (the game's own packs only).

use std::path::PathBuf;

use wce_vfs::{Config, GameProfile, Vfs};

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let (Some(game), Some(prefix)) = (a.first(), a.get(1)) else {
        eprintln!("vfs_ls <game dir> <prefix> [--vanilla] [--extract <path> <out file>]");
        std::process::exit(2);
    };

    let mut config = Config::new(PathBuf::from(game), GameProfile::from_env().clone());
    config.use_mods = !a.iter().any(|v| v == "--vanilla");
    let vfs = Vfs::mount(&config).expect("game files");
    if let Some(i) = a.iter().position(|v| v == "--extract") {
        let (Some(path), Some(out)) = (a.get(i + 1), a.get(i + 2)) else { return };
        let bytes = vfs.read(path).expect("a pack file");
        std::fs::write(out, &bytes).expect("written");
        println!("{path}: {} bytes -> {out}", bytes.len());
        return;
    }

    for path in vfs.list(prefix, "") {
        println!("{path}  {}", vfs.read(path).map_or(0, |b| b.len()));
    }
}
