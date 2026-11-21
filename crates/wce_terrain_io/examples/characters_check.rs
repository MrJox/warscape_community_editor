//! A campaign's characters and forces at its start (`wce_terrain_io::attila::characters`): how many of each kind, on the map or not,
//! their forces and units.
//!
//!   cargo run --release -p wce_terrain_io --example characters_check -- <startpos.esf> [rows to print]

use std::collections::BTreeMap;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let rows: usize = a.get(1).and_then(|n| n.parse().ok()).unwrap_or(10);
    let bytes = std::fs::read(&a[0]).expect("startpos.esf");
    let start = wce_terrain_io::attila::characters::read(&bytes).expect("characters");
    let mut kinds: BTreeMap<&str, (usize, usize)> = BTreeMap::new();
    let shown: Vec<u32> = start.on_map().map(|c| c.id).collect();
    for c in &start.characters {
        let k = kinds.entry(c.kind.as_str()).or_default();
        k.0 += 1;
        k.1 += usize::from(shown.contains(&c.id));
    }
    println!(
        "{} characters, {} forces ({} navies, {} in a residence)",
        start.characters.len(),
        start.forces.len(),
        start.forces.values().filter(|f| f.navy).count(),
        start.forces.values().filter(|f| f.residence.is_some()).count()
    );
    for (k, (n, on)) in &kinds {
        println!("  {k}: {n}, on the map {on}");
    }
    let no_art = start.on_map().filter(|c| c.art_set.is_empty()).count();
    println!("on the map without an art set: {no_art}");
    // `WCE_ALL`: every character, on the map or not.
    let all: Vec<&wce_terrain_io::attila::characters::StartCharacter> =
        if std::env::var("WCE_ALL").is_ok() { start.characters.iter().collect() } else { start.on_map().collect() };
    for c in all.into_iter().take(rows) {
        let f = start.force_of(c);
        println!(
            "  {} {} {} art {} at ({:.2}, {:.2}) cell {:?} facing {:.3}; force {:?} navy {:?} units {}",
            c.id,
            c.faction,
            c.kind,
            c.art_set,
            c.world.0,
            c.world.1,
            c.cell,
            c.facing,
            c.force,
            f.map(|f| f.navy),
            f.map_or(0, |f| f.units.len())
        );
    }
}
