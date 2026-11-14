//! Prints a `locations.blm`: its header, its types and counts, the locations with a deployment id (0xffff0000 + n) and the first
//! records of each type.
//!
//!   cargo run --release -p wce_tile --example blm_dump -- <locations.blm> [records a type, 20]

use wce_tile::battle_map::locations::BattleLocations;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let b = BattleLocations::parse(&std::fs::read(&a[0]).expect("file")).expect("blm");
    let n: usize = a.get(1).and_then(|v| v.parse().ok()).unwrap_or(20);
    println!("version {} {} x {} cells, {} types, tail {} bytes", b.version, b.width, b.height, b.types.len(), b.tail.len());
    let mut ids = vec![];
    for (t, l) in &b.types {
        let with: Vec<u32> = l.iter().filter(|x| x.id != -1).map(|x| (x.id as u32).wrapping_sub(0xffff_0000)).collect();
        println!("  {t}: {} locations, {} with an id (min {:?} max {:?})", l.len(), with.len(), with.iter().min(), with.iter().max());
        for x in l.iter().take(n) {
            println!("    rect {:?} centre {:?} id {:#x} extra {:?}", x.rect, x.centre, x.id, x.extra);
        }

        ids.extend(with);
    }

    ids.sort_unstable();
    let distinct = {
        let mut d = ids.clone();
        d.dedup();
        d.len()
    };
    println!("ids: {} in all, {distinct} distinct, from {:?} to {:?}", ids.len(), ids.first(), ids.last());
}
