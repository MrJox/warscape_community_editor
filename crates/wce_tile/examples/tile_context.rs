//! Where a tile is placed in a kit battle map or a saved tile map, and what surrounds it.
//!
//!   cargo run --release -p wce_tile --example tile_context -- <kit raw_data> <map name | folder with tile_map.index/.tiles/lf_height_map.dds> <tile location> <variation> [radius]

use std::path::PathBuf;

use wce_tile::battle_map::context::Spot;
use wce_tile::battle_map::tile_db::TileDatabase;
use wce_tile::battle_map::{load_game_map, load_kit_map};

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let (Some(raw), Some(map), Some(loc), Some(var)) = (a.first(), a.get(1), a.get(2), a.get(3)) else {
        eprintln!("tile_context <kit raw_data> <map> <tile location> <variation> [radius]");
        std::process::exit(2);
    };

    let radius = a.get(4).and_then(|r| r.parse().ok()).unwrap_or(8);
    let raw = PathBuf::from(raw);
    let db = TileDatabase::load(&raw.join("terrain/tiles/battle/_tile_database")).expect("database");
    let t0 = std::time::Instant::now();
    let dir = PathBuf::from(map);
    let m = if dir.join("tile_map.tiles").is_file() {
        let r = |n: &str| std::fs::read(dir.join(n)).ok();
        load_game_map("game", &db, &r("tile_map.index").expect("index"), &r("tile_map.tiles").expect("tiles"), r("lf_height_map.dds").as_deref())
    } else {
        load_kit_map(&db, &raw.join("terrain/battles").join(map))
    }
    .expect("map");

    println!("{}: {} x {}, {} placements, lf {:?}, unknown {:?} ({:.1} s)", m.name, m.width, m.height, m.placements.len(), m.lf.as_ref().map(|f| (f.w, f.h)), m.unknown, t0.elapsed().as_secs_f64());
    let found = m.find(loc, var);
    println!("{} placements of {loc}\\{var}; {} of the tile", found.len(), m.find_tile(loc).len());
    for &i in found.iter().take(8) {
        let p = &m.placements[i];
        let c = m.context(&Spot { x: p.x, y: p.y, rotation: p.rotation }, p.w, p.h, radius);
        println!("  ({}, {}) rot {:#x}: {} neighbours in {:?}, lf mean {:.1} m range {:.1} m", p.x, p.y, p.rotation, c.neighbours.len(), c.window, c.lf_mean, c.lf_range);
        if a.iter().any(|v| v == "--names") {
            for (n, _) in &c.neighbours {
                let q = &m.placements[*n];
                println!("      {}\\{} at ({}, {})", q.location, q.variation, q.x, q.y);
            }
        }
    }
}
