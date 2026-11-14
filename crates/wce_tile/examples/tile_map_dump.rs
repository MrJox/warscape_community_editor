//! Prints a built tile map's origin cells (layer 0) in file order within a range of rows: position, origin, flags, name.
//!
//!   cargo run --release -p wce_tile --example tile_map_dump -- <folder with tile_map.index / .tiles> <first row> <last row>

use wce_tile::battle_map::tile_map::TileMap;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let (Some(dir), Some(r0), Some(r1)) =
        (a.first(), a.get(1).and_then(|v| v.parse::<u32>().ok()), a.get(2).and_then(|v| v.parse::<u32>().ok()))
    else {
        eprintln!("tile_map_dump <folder> <first row> <last row>");
        std::process::exit(2);
    };

    let dir = std::path::Path::new(dir);
    let m = TileMap::parse(
        &std::fs::read(dir.join("tile_map.index")).expect("index"),
        &std::fs::read(dir.join("tile_map.tiles")).expect("tiles"),
    )
    .expect("tile map");
    println!("version {}, {} x {}, {} names", m.version, m.width, m.height, m.names.len());
    for (i, c) in m.layers[0].iter().enumerate() {
        let (x, y) = (i as u32 % m.width, i as u32 / m.width);
        if y < r0 || y > r1 || c.index <= 0 {
            continue;
        }

        println!(
            "  ({x}, {y}) origin {:?} flags {:#06x} {}",
            c.origin,
            c.flags,
            m.names.get(c.index as usize - 1).cloned().unwrap_or_default()
        );
    }
}
