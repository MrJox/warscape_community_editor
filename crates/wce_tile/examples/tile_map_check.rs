//! Compares the battle map fill (wce_tile::battle_map) of a kit map with a saved tile map (the campaign map's `tile_map.index` +
//! `tile_map.tiles`, e.g. the game's from terrain.pack).
//!
//!   cargo run --release -p wce_tile --example tile_map_check -- <kit raw_data/terrain> <map> <folder with tile_map.index / .tiles> [--write <dir>]
//!
//! `--write` also saves the fill's tile map there.

use std::collections::BTreeMap;
use std::path::PathBuf;

use wce_tile::battle_map::fill_kit_tile_map;
use wce_tile::battle_map::tile_db::TileDatabase;
use wce_tile::battle_map::tile_map::{TileMap, TileMapCell};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (Some(terrain), Some(map), Some(want)) = (args.first(), args.get(1), args.get(2)) else {
        eprintln!("tile_map_check <kit raw_data/terrain> <map> <folder with tile_map.index / .tiles>");
        std::process::exit(2);
    };

    let terrain = PathBuf::from(terrain);
    let want = PathBuf::from(want);
    let fail = |e: String| -> ! {
        eprintln!("{e}");
        std::process::exit(1)
    };

    let db = TileDatabase::load(&terrain.join("tiles/battle/_tile_database")).unwrap_or_else(|e| fail(e));
    let read = |n: &str| std::fs::read(want.join(n)).unwrap_or_else(|e| fail(format!("{n}: {e}")));
    let want = TileMap::parse(&read("tile_map.index"), &read("tile_map.tiles")).unwrap_or_else(|e| fail(e));
    let (got, messages) = fill_kit_tile_map(&db, &terrain.join("battles").join(map)).unwrap_or_else(|e| fail(e));
    if let Some(dir) = args.iter().position(|a| a == "--write").and_then(|i| args.get(i + 1)) {
        let (index, tiles) = got.write();
        let dir = PathBuf::from(dir);
        let _ = std::fs::create_dir_all(&dir);
        std::fs::write(dir.join("tile_map.index"), index).unwrap_or_else(|e| fail(e.to_string()));
        std::fs::write(dir.join("tile_map.tiles"), tiles).unwrap_or_else(|e| fail(e.to_string()));
    }

    println!("size got {} x {}, want {} x {}; names got {}, want {}; {} messages", got.width, got.height, want.width, want.height, got.names.len(), want.names.len(), messages.len());
    if (got.width, got.height) != (want.width, want.height) {
        return;
    }

    let name = |m: &TileMap, c: &TileMapCell| if c.index > 0 { m.names.get(c.index as usize - 1).cloned().unwrap_or_default() } else { String::new() };
    let w = got.width as usize;
    for layer in 0..2 {
        let (mut same, mut first, mut kinds) = (0usize, None, BTreeMap::<&str, usize>::new());
        for (i, (a, b)) in got.layers[layer].iter().zip(&want.layers[layer]).enumerate() {
            let kind = if a.index > 0 || b.index > 0 {
                if name(&got, a) != name(&want, b) { "origin: tile" } else if a.flags != b.flags { "origin: flags" } else if a.climate != b.climate { "origin: climate" }
                else if a.lf != b.lf { "origin: lf" } else if a.origin != b.origin || a.flipped != b.flipped { "origin: other" } else { "" }
            } else if a.index != b.index { "cover: origin" } else if a.flags != b.flags { "cover: flags" } else if a.climate != b.climate { "cover: climate" }
            else if a.origin != b.origin || a.lf != b.lf || a.flipped != b.flipped { "cover: other" } else { "" };

            if kind.is_empty() {
                same += 1;
                continue;
            }

            *kinds.entry(kind).or_default() += 1;
            first.get_or_insert(i);
        }

        println!("layer {layer}: {same} / {} cells agree; {kinds:?}", got.layers[layer].len());
        if let Some(i) = first {
            let show = |m: &TileMap, c: &TileMapCell| format!("{} {} {:#06x} {:?} c{} f{} lf {:?}", c.index, name(m, c), c.flags, c.origin, c.climate, c.flipped, c.lf);
            println!("  first at ({}, {}):", i % w, i / w);
            for j in i..(i + 4).min(got.layers[layer].len()) {
                println!("   ({}, {}) got  {}", j % w, j / w, show(&got, &got.layers[layer][j]));
                println!("   ({}, {}) want {}", j % w, j / w, show(&want, &want.layers[layer][j]));
            }
        }
    }

    for m in messages.iter().take(5) {
        println!("  {m}");
    }
}

