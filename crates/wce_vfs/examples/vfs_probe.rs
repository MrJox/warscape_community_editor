//! Mounts the packs and prints what the editor would see, in the format of the C++ `vfs_probe`.
//!
//!   WCE_GAME=<game folder> [WCE_GAME_KEY=<game key>] cargo run -p wce_vfs --example vfs_probe [--find <substring>]... [--extract <prefix> <folder>]... [path-to-read...]
//!
//! `--extract` writes every file under a path prefix, as the packs give it, below a folder.

use wce_vfs::{Config, GameProfile, Vfs};

fn main() {
    let mut finds = vec![];
    let mut reads = vec![];
    let mut extracts = vec![];
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--find" => finds.push(args.next().expect("--find needs a substring")),
            "--extract" => extracts.push((args.next().expect("--extract needs a prefix"), args.next().expect("--extract needs a folder"))),
            _ => reads.push(arg),
        }
    }

    let game_dir = std::env::var("WCE_GAME").expect("WCE_GAME must be the game folder");
    let started = std::time::Instant::now();
    let vfs = Vfs::mount(&Config::new(game_dir, GameProfile::from_env().clone())).unwrap();
    println!("mounted {} packs, {} files in {:.1} s", vfs.packs().len(), vfs.files().len(), started.elapsed().as_secs_f32());
    for mod_name in vfs.missing_mods() {
        println!("  MISSING mod {mod_name} (in used_mods.txt, not in data/)");
    }

    for pack in vfs.packs() {
        let kind = if pack.loose { "loose  " } else if pack.is_mod { "mod    " } else { "vanilla" };
        println!("  {kind} {}", pack.name);
    }

    println!(".environment {}, .lighting {}, vfx xml {}, fxc {}, building rmv2 {}",
        vfs.list("", ".environment").len(), vfs.list("", ".lighting").len(), vfs.list("vfx/", ".xml").len(),
        vfs.list("fxc/", ".fxc").len(), vfs.list("rigidmodels/buildings/", ".rigid_model_v2").len());

    if !finds.is_empty() {
        for (path, pack) in vfs.files() {
            if finds.iter().all(|find| path.contains(find.as_str())) {
                println!("find: {path}  ({})", vfs.packs()[*pack].name);
            }
        }
    }

    for path in reads {
        match vfs.read(&path) {
            Ok(bytes) => println!("{path} -> {} : {} bytes", vfs.packs()[vfs.owner(&path).unwrap()].name, bytes.len()),
            Err(error) => println!("{path}: {error}"),
        }
    }

    for (prefix, folder) in extracts {
        let paths = vfs.list(&prefix, "");
        for path in &paths {
            let target = std::path::Path::new(&folder).join(path);
            std::fs::create_dir_all(target.parent().unwrap()).unwrap();
            std::fs::write(&target, vfs.read(path).unwrap()).unwrap();
        }

        println!("extracted {} files under {prefix} to {folder}", paths.len());
    }
}
