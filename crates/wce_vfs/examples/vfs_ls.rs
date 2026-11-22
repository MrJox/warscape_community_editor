//! Lists the game's pack files under a prefix (with their sizes), or writes one out, or writes out every one under the prefix.
//!
//!   cargo run --release -p wce_vfs --example vfs_ls -- <game dir> <prefix> [--vanilla] [--extract <path> <out file>]
//!   cargo run --release -p wce_vfs --example vfs_ls -- <game dir> <prefix> [--vanilla] [--ext <extension>] --extract-all <out dir>
//!
//! `--ext` keeps the files of one extension (`rigid_model_v2`). `--extract-all` keeps the files' paths under `<out dir>` (scripts/gates.sh fills the XML and model gates' folder with it).
//!
//! `$WCE_GAME_KEY` chooses the game (`rome_2`; Attila by default). `--vanilla` leaves the enabled mods out (the game's own packs only).

use std::path::PathBuf;

use wce_vfs::{Config, GameProfile, Vfs};

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let (Some(game), Some(prefix)) = (a.first(), a.get(1)) else {
        eprintln!("vfs_ls <game dir> <prefix> [--vanilla] [--ext <extension>] [--extract <path> <out file> | --extract-all <out dir>]");
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

    if let Some(i) = a.iter().position(|v| v == "--extract-all") {
        let Some(out) = a.get(i + 1) else { return };
        let (mut files, mut bytes) = (0, 0);
        let ext = a.iter().position(|v| v == "--ext").and_then(|i| a.get(i + 1)).map_or("", |e| e.as_str());
        for path in vfs.list(prefix, ext) {
            let data = vfs.read(path).expect("a pack file");
            let to = PathBuf::from(out).join(path);
            if let Some(dir) = to.parent() {
                std::fs::create_dir_all(dir).expect("a folder");
            }
            std::fs::write(&to, &data).expect("written");
            files += 1;
            bytes += data.len();
        }
        println!("{prefix}: {files} files, {bytes} bytes -> {out}");
        return;
    }

    for path in vfs.list(prefix, "") {
        println!("{path}  {}", vfs.read(path).map_or(0, |b| b.len()));
    }
}
