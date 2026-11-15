//! What the game's definitions put in their slots: per slot name, the folders of the models and definitions its alternatives name. A report
//! for the Unit tool's pickers (what a slot expects).
//!
//!   cargo run --release -p wce_unit_tool --example slot_kinds [names]      (the game folder: the settings or WCE_GAME; WCE_MODS: mod packs)
//!
//! `names`: every slot name, with how many slots have it and in how many files, instead of the folders.

use std::collections::{BTreeMap, BTreeSet};

use wce_formats::variant_mesh::{Alternative, VariantMesh, VariantMeshFile};
use wce_foundation::settings::Settings;
use wce_vfs::{Config, GameProfile, Vfs, normalize};

fn folder(path: &str) -> String {
    let p = normalize(path);
    p.rsplit_once('/').map_or(p.clone(), |(f, _)| f.to_owned())
}

fn names(mesh: &VariantMesh, out: &mut BTreeMap<String, usize>) {
    for slot in &mesh.slots {
        *out.entry(slot.name.clone()).or_default() += 1;
        for a in &slot.alternatives {
            if let Alternative::Mesh(m) = a {
                names(m, out);
            }
        }
    }
}

fn walk(mesh: &VariantMesh, out: &mut BTreeMap<String, BTreeMap<String, usize>>) {
    for slot in &mesh.slots {
        for a in &slot.alternatives {
            let f = match a {
                Alternative::Mesh(m) => m.model.as_deref().filter(|m| !m.trim().is_empty()).map(folder),
                Alternative::Reference { definition, .. } => Some(folder(definition)),
            };
            if let Some(f) = f {
                *out.entry(slot.name.to_lowercase()).or_default().entry(f).or_default() += 1;
            }

            if let Alternative::Mesh(m) = a {
                walk(m, out);
            }
        }
    }
}

fn main() {
    let game = Settings::load_default().game_dir().expect("the game folder");
    let mut config = Config::new(game, GameProfile::from_env().clone());
    config.use_mods = std::env::var("WCE_MODS").is_ok();
    config.mod_packs = std::env::var("WCE_MODS").ok().map(|m| m.split(',').map(str::to_owned).collect());
    let vfs = Vfs::mount(&config).expect("mounts");
    if std::env::args().nth(1).as_deref() == Some("names") {
        // Each name as written (case kept), its slots and files.
        let (mut slots, mut files) = (BTreeMap::<String, usize>::new(), BTreeMap::<String, usize>::new());
        for p in vfs.list("variantmeshes/", ".variantmeshdefinition") {
            if let Some(f) = vfs.read(p).ok().and_then(|b| VariantMeshFile::parse(b).ok()) {
                let mut here = BTreeMap::new();
                names(&f.root, &mut here);
                for (n, k) in here {
                    *slots.entry(n.clone()).or_default() += k;
                    *files.entry(n).or_default() += 1;
                }
            }
        }

        println!("{} names", slots.len());
        for (n, k) in &slots {
            println!("{n}\t{k}\t{}", files[n]);
        }

        return;
    }

    let mut out = BTreeMap::new();
    for p in vfs.list("variantmeshes/", ".variantmeshdefinition") {
        if let Some(f) = vfs.read(p).ok().and_then(|b| VariantMeshFile::parse(b).ok()) {
            walk(&f.root, &mut out);
        }
    }

    let mut by_count: Vec<(&String, usize, &BTreeMap<String, usize>)> = out.iter().map(|(k, v)| (k, v.values().sum(), v)).collect();
    by_count.sort_by_key(|x| std::cmp::Reverse(x.1));
    println!("{} slot names", by_count.len());
    for (name, n, folders) in by_count.iter().take(60) {
        let mut f: Vec<(&String, &usize)> = folders.iter().collect();
        f.sort_by_key(|x| std::cmp::Reverse(*x.1));
        let tops: BTreeSet<String> = f.iter().map(|(p, _)| p.split('/').take(4).collect::<Vec<_>>().join("/")).collect();
        println!(
            "{name}: {n} alternatives, {} folders; top: {:?}; under {:?}",
            f.len(),
            f.iter().take(4).collect::<Vec<_>>(),
            tops.iter().take(6).collect::<Vec<_>>()
        );
    }
}
