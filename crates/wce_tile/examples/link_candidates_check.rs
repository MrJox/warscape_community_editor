//! `battle_map::link_candidates` against BOB's own maps: on every kit settlement map (BOB's fill, exact), each neighbour that touches an
//! unturned tile and carries one of its joins on must be among that tile's candidates, at its place and rotation.
//!
//!   cargo run --release -p wce_tile --example link_candidates_check -- <kit raw_data>

use std::collections::HashMap;
use std::path::Path;

use wce_tile::battle_map::link_candidates::candidates;
use wce_tile::battle_map::tile_db::TileDatabase;

fn main() {
    let raw = std::env::args().nth(1).expect("usage: link_candidates_check <kit raw_data>");
    let raw = Path::new(&raw);
    let db = TileDatabase::load(&raw.join("terrain/tiles/battle/_tile_database")).expect("database");
    let by_location: HashMap<String, usize> = db.tiles.iter().enumerate().map(|(i, t)| (t.location.to_lowercase(), i)).collect();
    let mut cache: HashMap<usize, Vec<wce_tile::battle_map::link_candidates::Candidate>> = HashMap::new();
    let (mut pairs, mut found) = (0, 0);
    let mut maps = std::fs::read_dir(raw.join("terrain/battles")).expect("battles").flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect::<Vec<_>>();
    maps.sort();
    for m in maps.iter().filter(|m| m.file_name().is_some_and(|n| n.to_string_lossy().starts_with("settlement"))) {
        let Ok(map) = wce_tile::battle_map::load_kit_map(&db, m) else { continue };
        for a in map.placements.iter().filter(|p| p.rotation == 0x10) {
            let Some(&ai) = by_location.get(&a.location.to_lowercase()) else { continue };
            let at = &db.tiles[ai];
            if at.link_targets.is_empty() {
                continue;
            }

            let ar = a.rect();
            for b in &map.placements {
                let Some(&bi) = by_location.get(&b.location.to_lowercase()) else { continue };
                let br = b.rect();
                let touch = (br[2] == ar[0] || br[0] == ar[2]) && br[1] < ar[3] && br[3] > ar[1] || (br[3] == ar[1] || br[1] == ar[3]) && br[0] < ar[2] && br[2] > ar[0];
                if !touch {
                    continue;
                }

                // Does it carry a join on (one of its must-be rules on one of a's targets, same set)?
                let bt = &db.tiles[bi];
                let rel = (b.x - a.x, b.y - a.y);
                let lands = |lx: i32, ly: i32| {
                    let (dx, dy) = wce_tile::battle_map::fill::rotate_in_tile_space(bt.width, bt.height, b.rotation, lx, ly);
                    (rel.0 + dx, rel.1 + dy)
                };
                let name = |s: Option<usize>| s.map(|s| db.sets[s].link_name().to_owned());
                let joins = at.link_targets.iter().any(|tg| bt.links.iter().any(|l| l.test == wce_tile::battle_map::tile_db::LinkTest::Equals && lands(l.x, l.y) == (tg.x, tg.y) && name(l.set) == name(tg.set)));
                if !joins {
                    continue;
                }

                pairs += 1;
                let c = cache.entry(ai).or_insert_with(|| candidates(&db, at));
                if c.iter().any(|c| c.tile == bi && c.rotation == b.rotation && (c.x, c.y) == rel) {
                    found += 1;
                } else {
                    println!("{}: {} next to {} at {rel:?} rot {:#x} not a candidate", m.file_name().unwrap_or_default().to_string_lossy(), b.location, a.location, b.rotation);
                }
            }
        }
    }

    println!("{pairs} joined neighbours on the maps, {found} among the candidates");
}
