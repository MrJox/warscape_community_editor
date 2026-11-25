//! Where a campaign map's river crossings fall on its battle map (docs/plans/river_bridges.md): for each campaign placement of a
//! `river_crossing` tile, what the battle map has there, how many of the battle tiles round it have a road link into it (a road reaching
//! the crossing), and, the battle crossing of the campaign tile's name put there at its rotation, how many of its own road and river
//! links land on a tile of that set.
//!
//!   cargo run --release -p wce_tile --example crossing_spots -- <kit _tile_database folder> <campaign terrain.tile_list> <battle terrain.tile_list> [list]

use std::collections::{BTreeMap, HashMap};
use std::path::Path;

use wce_tile::battle_map::fill::rotate_in_tile_space;
use wce_tile::battle_map::tile_db::{LinkTest, TileDatabase};
use wce_tile::battle_map::tile_list::TileList;

/// A placement's name as the database's location: `terrain\tiles\battle\<set>\<size>` (its variation folder off), in lower case.
fn location(name: &str) -> String {
    name.rsplit_once('\\').map_or(name, |(l, _)| l).to_lowercase()
}

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let db = TileDatabase::load(Path::new(&a[0])).expect("tile database");
    let campaign = TileList::parse(&std::fs::read(&a[1]).expect("campaign list")).expect("campaign list");
    let battle = TileList::parse(&std::fs::read(&a[2]).expect("battle list")).expect("battle list");
    let list = a.get(3).is_some_and(|v| v == "list");
    let by_location = db.tiles.iter().enumerate().map(|(i, t)| (t.location.to_lowercase(), i)).collect::<HashMap<_, _>>();
    let link_name = |s: Option<usize>| s.map(|s| db.sets[s].link_name().to_owned()).unwrap_or_default();

    // The battle map's cells: the placement covering each (its index), the geometry-only ones left out.
    let (w, h) = (battle.battle_size[0] as i32, battle.battle_size[1] as i32);
    let mut cells: HashMap<(i32, i32), usize> = HashMap::new();
    let mut unknown = 0;
    for (k, p) in battle.placements.iter().enumerate() {
        if p.flags & 0x700 == 0x100 {
            continue;
        }

        let Some(&t) = battle.names.get(p.name).and_then(|n| by_location.get(&location(n))) else {
            unknown += 1;
            continue;
        };
        let tile = &db.tiles[t];
        for sy in 0..tile.height {
            for sx in 0..tile.width {
                if !tile.subtile_masked_valid(sx, sy) {
                    continue;
                }

                let (dx, dy) = rotate_in_tile_space(tile.width, tile.height, p.flags & 0xf0, sx, sy);
                cells.insert((i32::from(p.x) + dx, i32::from(p.y) + dy), k);
            }
        }
    }

    // The set a battle cell is of.
    let set_at = |x: i32, y: i32| -> Option<String> {
        let &k = cells.get(&(x, y))?;
        let t = by_location.get(&location(&battle.names[battle.placements[k].name]))?;
        Some(link_name(db.tiles[*t].set))
    };
    let is_road = |s: &str| s.starts_with("road");

    let mut summary: BTreeMap<String, usize> = BTreeMap::new();
    let mut spots = 0;
    for p in &campaign.placements {
        let name = &campaign.names[p.name];
        if !name.to_lowercase().contains("\\river_crossing\\") {
            continue;
        }

        spots += 1;
        let (ox, oy) = (i32::from(p.x), i32::from(p.y));
        let here = cells
            .get(&(ox, oy))
            .map(|&k| {
                let n = &battle.names[battle.placements[k].name];
                let l = location(n);
                l.split('\\').skip(3).collect::<Vec<_>>().join("\\")
            })
            .unwrap_or_else(|| "nothing".to_owned());

        // The battle tiles round it whose road links land on the spot's 2 x 2.
        let inside = |x: i32, y: i32| (ox..ox + 2).contains(&x) && (oy..oy + 2).contains(&y);
        let mut roads_in = 0;
        let mut seen = vec![];
        for y in oy - 2..oy + 4 {
            for x in ox - 2..ox + 4 {
                let Some(&k) = cells.get(&(x, y)) else { continue };
                if inside(x, y) || seen.contains(&k) {
                    continue;
                }

                seen.push(k);
                let q = &battle.placements[k];
                let Some(&t) = by_location.get(&location(&battle.names[q.name])) else { continue };
                let tile = &db.tiles[t];
                for link in tile.links.iter().filter(|l| l.test == LinkTest::Equals && is_road(&link_name(l.set))) {
                    let (dx, dy) = rotate_in_tile_space(tile.width, tile.height, q.flags & 0xf0, link.x, link.y);
                    if inside(i32::from(q.x) + dx, i32::from(q.y) + dy) {
                        roads_in += 1;
                    }
                }
            }
        }

        // The battle crossing of the campaign tile's name there, at its rotation: its road and river links met or not.
        let battle_location = location(name).replace("\\campaign\\", "\\battle\\");
        let crossing = by_location.get(&battle_location).map(|&t| &db.tiles[t]);
        let (mut road_ok, mut road_all, mut river_ok, mut river_all) = (0, 0, 0, 0);
        if let Some(tile) = crossing {
            for link in tile.links.iter().filter(|l| l.test == LinkTest::Equals) {
                let want = link_name(link.set);
                let (dx, dy) = rotate_in_tile_space(tile.width, tile.height, p.flags & 0xf0, link.x, link.y);
                let (x, y) = (ox + dx, oy + dy);
                if x < 0 || y < 0 || x >= w || y >= h {
                    continue;
                }

                let got = set_at(x, y).unwrap_or_default();
                if is_road(&want) {
                    road_all += 1;
                    road_ok += usize::from(is_road(&got));
                } else if want.starts_with("river") {
                    river_all += 1;
                    river_ok += usize::from(got.starts_with("river"));
                }
            }
        }

        let verdict = match crossing {
            None => "no battle crossing of that name".to_owned(),
            Some(_) => format!("crossing's road links met {road_ok}/{road_all}, river links met {river_ok}/{river_all}"),
        };
        *summary.entry(format!("road links into it {roads_in}; {verdict}")).or_default() += 1;
        if list {
            println!(
                "{} at {ox},{oy} rot {:#x}: battle has {here}; {roads_in} road link(s) into it; {verdict}",
                location(name),
                p.flags & 0xf0
            );
        }
    }

    println!("{spots} campaign crossings ({unknown} battle placements of tiles the database lacks)");
    for (k, v) in &summary {
        println!("{v:4}  {k}");
    }
}
