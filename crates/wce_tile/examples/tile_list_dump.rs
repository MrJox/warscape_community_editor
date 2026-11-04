//! Exploration: a `terrain.tile_list`'s header, names and placements (with Rome 2's unknown bytes).
//!
//!   cargo run --release -p wce_tile --example tile_list_dump -- <terrain.tile_list> [placements]

use wce_tile::battle_map::tile_list::TileList;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let t = TileList::parse(&std::fs::read(&a[0]).expect("file")).expect("tile list");
    let n: usize = a.get(1).and_then(|v| v.parse().ok()).unwrap_or(20);
    println!("version {} size {:?} centre {:?} offset {:?} bounds {:?} campaign {} rects {:?} tail {} bytes", t.version, t.battle_size, t.battle_centre, t.origin_offset, t.bounds, t.is_campaign, t.rects, t.tail.len());
    for (i, name) in t.names.iter().enumerate() {
        println!("  name {i}: {name}");
    }

    for p in t.placements.iter().take(n) {
        println!("  {} at {}, {} flags {:#06x} climate {} old {:?} lf {:?}", p.name, p.x, p.y, p.flags, p.climate, p.old, p.lf);
    }
}
