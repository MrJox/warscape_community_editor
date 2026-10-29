//! Every `.anim` and `.meta` under `rigidmodels/buildings/` of the mounted packs written back through wce_formats' lossless models
//! (`AnimFile`, `MetaFile`): each must come back to the byte. Also counts how the collapses use their tracks.
//!
//!   cargo run --release -p wce_destruction --example anim_meta_corpus -- <game folder> [--mods a.pack;b.pack]

use std::path::PathBuf;

use wce_formats::anim::AnimFile;
use wce_formats::anim_meta::MetaFile;
use wce_vfs::{Config, GameProfile, Vfs};

fn main() {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    let Some(game) = args.first() else {
        eprintln!("usage: anim_meta_corpus <game folder> [--mods a.pack;b.pack]");
        std::process::exit(2);
    };

    let mut config = Config::new(PathBuf::from(game), GameProfile::ATTILA);
    config.mod_packs = args.iter().position(|a| a == "--mods").and_then(|i| args.get(i + 1)).map(|m| m.split(';').filter(|s| !s.is_empty()).map(str::to_owned).collect());
    let vfs = Vfs::mount(&config).expect("packs");
    let (mut anims, mut metas, mut bad) = (0, 0, 0);
    let (mut shared, mut max_bones) = (0, 0);
    for path in vfs.list("rigidmodels/buildings/", ".anim") {
        let bytes = vfs.read(path).expect("read");
        anims += 1;
        match AnimFile::parse(&bytes) {
            Some(f) if f.encode() == bytes => {
                if f.translation_count as usize != f.bones.len() || f.rotation_count as usize != f.bones.len() {
                    shared += 1;
                }

                if path.ends_with("_anim.anim") && !path.contains("weighted") {
                    max_bones = max_bones.max(f.bones.len());
                }
            }

            Some(_) => {
                bad += 1;
                println!("differs: {path}");
            }

            None => {
                bad += 1;
                println!("not read: {path}");
            }
        }
    }

    for path in vfs.list("rigidmodels/buildings/", ".meta") {
        let bytes = vfs.read(path).expect("read");
        metas += 1;
        if MetaFile::parse(&bytes).map(|m| m.encode()) != Some(bytes) {
            bad += 1;
            println!("meta not written back: {path}");
        }
    }

    println!("{anims} .anim, {metas} .meta, {bad} not written back; {shared} animations with fewer tracks than bones; most bones in a collapse {max_bones}");
    if bad > 0 {
        std::process::exit(1);
    }
}
