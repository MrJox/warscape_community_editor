//! Inventory of the exported (binary) tiles in packs (docs/plans/tile_roundtrip.md, Phase 2 step 1).
//!
//!   cargo run --release -p wce_tile_io --example tile_pack_inventory -- <pack or folder of packs>... [--filter TEXT[,TEXT...]]
//!       [--extract DIR] [--per-tile]
//!
//! Every file under `terrain/tiles/` of every pack, grouped by variation folder (the folder that holds the files): the file kinds (digits ->
//! N, climate prefixes -> X), how many folders have each, per pack and per tile kind (battle / campaign). `--per-tile` prints each folder's
//! file set; `--filter` keeps the paths that contain any of the texts; `--extract` writes the matching files below DIR as `<pack name>/<path>`. Packs are read lazily (rpfm_lib); a folder of packs
//! takes every `*.pack` in it.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use rpfm_lib::files::pack::Pack;
use rpfm_lib::games::supported_games::SupportedGames;

/// `rome_med.tree_list.bin` -> `X.tree_list.bin`, `river_spline3.xml` -> `river_splineN.xml`.
fn kind_of(name: &str) -> String {
    let name = name.to_lowercase();
    let name = ["rome_", "att_", "tdd_"].iter().find_map(|p| name.strip_prefix(p)).map_or(name.clone(), |rest| {
        let tail = rest.find(['.', '_']).map_or("", |i| &rest[i..]);
        format!("X{tail}")
    });

    let mut out = String::new();
    let mut digit = false;
    for c in name.chars() {
        if c.is_ascii_digit() {
            if !digit {
                out.push('N');
            }

            digit = true;
        } else {
            out.push(c);
            digit = false;
        }
    }

    out
}

fn packs(arg: &Path, out: &mut Vec<PathBuf>) {
    if arg.is_dir() {
        let mut found = std::fs::read_dir(arg).map(|d| d.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "pack")).collect::<Vec<_>>()).unwrap_or_default();
        found.sort();
        out.extend(found);
    } else {
        out.push(arg.to_path_buf());
    }
}

fn main() {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    let mut inputs = vec![];
    let (mut filter, mut extract, mut per_tile) = (None, None, false);
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--filter" => filter = it.next().map(|f| f.to_lowercase()),
            "--extract" => extract = it.next().map(PathBuf::from),
            "--per-tile" => per_tile = true,
            _ => packs(Path::new(a), &mut inputs),
        }
    }

    if inputs.is_empty() {
        eprintln!("usage: tile_pack_inventory <pack or folder>... [--filter TEXT] [--extract DIR] [--per-tile]");
        std::process::exit(2);
    }

    let games = SupportedGames::default();
    let game = games.game("attila").expect("attila");
    // kind of tile -> file kind -> folders that have it; kind of tile -> folders.
    let mut kinds: BTreeMap<String, BTreeMap<String, usize>> = BTreeMap::new();
    let mut folders_per_kind: BTreeMap<String, usize> = BTreeMap::new();
    for pack_path in &inputs {
        let pack_name = pack_path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let mut pack = match Pack::read_and_merge(std::slice::from_ref(pack_path), game, true, false, false) {
            Ok(p) => p,
            Err(e) => {
                println!("{pack_name}: cannot read: {e}");
                continue;
            }
        };

        let mut names = pack.files().keys().filter(|n| n.starts_with("terrain/tiles/")).cloned().collect::<Vec<_>>();
        if let Some(f) = &filter {
            names.retain(|n| f.split(',').any(|t| n.contains(t)));
        }

        names.sort();
        let mut folders: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        for n in &names {
            let (dir, file) = n.rsplit_once('/').unwrap_or(("", n));
            folders.entry(dir.to_owned()).or_default().insert(file.to_owned());
        }

        println!("{pack_name}: {} tile files in {} folders", names.len(), folders.len());
        for (dir, files) in &folders {
            // battle / campaign, and a settlement tile apart (they carry the logic files).
            let kind = match dir.split('/').nth(2) {
                Some(k) if dir.contains("/settlement") => format!("{k}/settlement"),
                Some(k) => k.to_owned(),
                None => "?".to_owned(),
            };

            *folders_per_kind.entry(kind.clone()).or_default() += 1;
            let set = files.iter().map(|f| kind_of(f)).collect::<BTreeSet<_>>();
            for k in &set {
                *kinds.entry(kind.clone()).or_default().entry(k.clone()).or_default() += 1;
            }

            if per_tile {
                println!("  {dir}: {}", set.into_iter().collect::<Vec<_>>().join(" "));
            }
        }

        if let Some(dir) = &extract {
            for n in &names {
                let Some(rf) = pack.files_mut().get_mut(n) else { continue };
                let Ok(Some(bytes)) = rf.encode(&None, false, false, true) else {
                    println!("  {n}: cannot read");
                    continue;
                };

                let target = dir.join(&pack_name).join(n);
                std::fs::create_dir_all(target.parent().expect("a parent")).expect("create folder");
                std::fs::write(&target, bytes).expect("write");
            }
        }
    }

    println!("file kinds (folders that have one), per kind of tile:");
    for (kind, files) in &kinds {
        println!("  {kind}: {} folders", folders_per_kind[kind]);
        let mut files = files.iter().collect::<Vec<_>>();
        files.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
        for (k, n) in files {
            println!("    {n:>6} {k}");
        }
    }
}
