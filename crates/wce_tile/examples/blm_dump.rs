//! Prints a `locations.blm`: its size, then each battle type's locations (rectangle and centre in cells).
//!
//!   cargo run --release -p wce_tile --example blm_dump -- <locations.blm>

use wce_tile::battle_map::locations::BattleLocations;

fn main() {
    let Some(path) = std::env::args().nth(1) else {
        eprintln!("blm_dump <locations.blm>");
        std::process::exit(2);
    };

    let bytes = std::fs::read(&path).unwrap_or_else(|e| {
        eprintln!("{path}: {e}");
        std::process::exit(1)
    });

    let b = BattleLocations::parse(&bytes).unwrap_or_else(|e| {
        eprintln!("{path}: {e:?}");
        std::process::exit(1)
    });

    println!("version {}, {} x {} cells", b.version, b.width, b.height);
    for (t, locs) in &b.types {
        println!("{t}: {}", locs.len());
        for l in locs.iter().take(20) {
            println!("  rect {:?} centre {:?} id {}", l.rect, l.centre, l.id);
        }
    }
}
