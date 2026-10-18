//! Compares the battle map fill (wce_tile::battle_map) with BOB's `terrain.tile_list` of every kit map that has one.
//!
//!   cargo run --release -p wce_tile --example battle_map_check -- <kit raw_data/terrain> <folder of <map>.tile_list> [map filter] [--diff]

use std::path::PathBuf;

use wce_tile::battle_map::tile_db::TileDatabase;
use wce_tile::battle_map::tile_list::TileList;
use wce_tile::battle_map::fill_kit_map;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (Some(terrain), Some(oracles)) = (args.first(), args.get(1)) else {
        eprintln!("battle_map_check <kit raw_data/terrain> <oracle folder> [filter] [--diff]");
        std::process::exit(2);
    };

    let filter = args.get(2).filter(|a| !a.starts_with("--")).cloned().unwrap_or_default();
    let diff = args.iter().any(|a| a == "--diff");
    let terrain = PathBuf::from(terrain);
    let db = match TileDatabase::load(&terrain.join("tiles/battle/_tile_database")) {
        Ok(db) => db,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    };

    println!("database: {} sets, {} climates, {} tiles", db.sets.len(), db.climates.len(), db.tiles.len());
    let mut files: Vec<PathBuf> = std::fs::read_dir(oracles).map(|d| d.filter_map(Result::ok).map(|e| e.path()).collect()).unwrap_or_default();
    files.retain(|p| p.extension().is_some_and(|e| e == "tile_list"));
    files.sort();
    let (mut exact, mut total) = (0, 0);
    for f in files {
        let map = f.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        if !map.contains(&filter) {
            continue;
        }

        let Ok(want) = std::fs::read(&f).map_err(|e| e.to_string()).and_then(|b| TileList::parse(&b)) else { continue };
        total += 1;
        let got = match fill_kit_map(&db, &terrain.join("battles").join(&map)) {
            Ok((l, _)) => l,
            Err(e) => {
                println!("{map}: {e}");
                continue;
            }
        };

        if got == want {
            exact += 1;
            println!("{map}: exact ({} placements)", want.placements.len());
            continue;
        }

        let name = |l: &TileList, i: usize| l.names.get(i).cloned().unwrap_or_default();
        let first = got.placements.iter().zip(&want.placements).position(|(a, b)| name(&got, a.name) != name(&want, b.name) || (a.x, a.y, a.flags, a.climate) != (b.x, b.y, b.flags, b.climate));
        let lf_only = first.is_none() && got.placements.len() == want.placements.len();
        println!(
            "{map}: DIFF placements {} / {}{}{}",
            got.placements.len(), want.placements.len(),
            first.map_or(String::new(), |i| format!(", first at #{i}")),
            if lf_only { ", tiles agree (header / heights / names differ)" } else { "" },
        );

        if diff {
            let show = |l: &TileList, i: usize| {
                l.placements.get(i).map_or("-".to_owned(), |p| format!("{} ({}, {}) {:#06x} c{} lf {:?}", name(l, p.name), p.x, p.y, p.flags, p.climate, p.lf))
            };

            let from = first.unwrap_or(0).saturating_sub(2);
            for i in from..from + 6 {
                println!("   #{i} got  {}", show(&got, i));
                println!("   #{i} want {}", show(&want, i));
            }

            if lf_only {
                println!("   names equal {}, strings {:?} / {:?}, campaign {} / {}, version {} / {}", got.names == want.names, got.strings, want.strings, got.is_campaign, want.is_campaign, got.version, want.version);
                let lf: Vec<_> = got.placements.iter().zip(&want.placements).enumerate().filter(|(_, (a, b))| a.lf != b.lf).collect();
                println!("   lf differs on {} placements; first {:?}", lf.len(), lf.iter().take(3).map(|(i, (a, b))| (*i, a.lf, b.lf, a.x, a.y)).collect::<Vec<_>>());
                println!("   header got  {:?} {:?} {:?} {:?} {:?} {:?}", got.lf_position, got.lf_scale, got.battle_size, got.battle_centre, got.origin_offset, got.bounds);
                println!("   header want {:?} {:?} {:?} {:?} {:?} {:?}", want.lf_position, want.lf_scale, want.battle_size, want.battle_centre, want.origin_offset, want.bounds);
            }
        }
    }

    println!("{exact} / {total} exact");
}
