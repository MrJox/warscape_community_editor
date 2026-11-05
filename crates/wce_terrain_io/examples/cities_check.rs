//! A campaign's settlements at its start (`wce_terrain_io::attila::cities`): its slots, their buildings and models, how many models the
//! packs have, and how many slots stand on land (the LF above the sea at their cell, from BOB's `lf_height_map_lq.dds` over the map's
//! cells) by the tree list's frame (x east, y north over the world).
//!
//!   cargo run --release -p wce_terrain_io --example cities_check -- <game dir> <campaign, e.g. main_attila> <map, e.g. main_attila_map> <lf_height> <vertical_offset> [region filter]

use wce_terrain_io::attila::cities::{DisplayTables, read};
use wce_vfs::{Config, GameProfile, Vfs};

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let game = GameProfile::from_env().clone();
    let mut cfg = Config::new(&a[0], game.clone());
    cfg.mod_packs = Some(vec![]);
    let vfs = Vfs::mount(&cfg).expect("vfs");
    let (campaign, map) = (&a[1], &a[2]);
    let (lf_height, offset): (f64, f64) = (a[3].parse().expect("lf_height"), a[4].parse().expect("offset"));
    let filter = a.get(5).cloned().unwrap_or_default();
    let schema = rpfm_lib::schema::Schema::load(&wce_foundation::settings::rpfm_schema_path(&game).expect("an rpfm schema"), None).expect("schema");
    let list = |p: &str| vfs.list(p, "").iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
    let read_vfs = |p: &str| vfs.read(p).ok().map(|b| b.to_vec());
    let tables = DisplayTables::read(&schema, &list, &read_vfs);
    println!("tables: {} factions, {} subcultures, {} building levels, {} models", tables.subculture.len(), tables.culture.len(), tables.ids.len(), tables.models.len());
    let map_data = vfs.read(&format!("campaign_maps/{map}/map_data.esf")).expect("map_data.esf");
    let startpos = std::fs::read(std::path::Path::new(&a[0]).join(format!("data/campaigns/{campaign}/startpos.esf"))).expect("startpos.esf");
    let cities = read(&map_data, &startpos, &tables).expect("cities");
    println!("world {:?}, grid {:?}, {} slots", cities.world, cities.grid, cities.slots.len());
    for n in &cities.notes {
        println!("  {n}");
    }

    let lf = vfs.read(&format!("terrain/campaigns/{map}/lf_height_map_lq.dds")).expect("lf");
    let u = |o: usize| u32::from_le_bytes(lf[o..o + 4].try_into().unwrap());
    let (lh, lw) = (u(12) as usize, u(16) as usize);
    let (mut land, mut sea, mut have, mut missing) = (0, 0, 0, 0);
    let mut kinds = std::collections::BTreeMap::<String, usize>::new();
    for s in &cities.slots {
        *kinds.entry(s.kind.clone()).or_default() += 1;
        let (fx, fy) = (s.world.0 / cities.world.0, 1.0 - s.world.1 / cities.world.1);
        let (px, py) = (((fx * lw as f64) as usize).min(lw - 1), ((fy * lh as f64) as usize).min(lh - 1));
        let o = 128 + 2 * (py * lw + px);
        let m = f64::from(u16::from_le_bytes([lf[o], lf[o + 1]])) / 65535.0 * lf_height - offset;
        if m > 0.0 {
            land += 1;
        } else {
            sea += 1;
            if s.model.is_some() {
                println!("  in the sea: {} #{} {} cell {:?} world ({:.2}, {:.2}) {m:.0} m {:?}", s.region, s.index, s.kind, s.cell, s.world.0, s.world.1, s.model);
            }
        }
        if let Some(model) = &s.model {
            if vfs.read(model).is_ok() { have += 1 } else { missing += 1 }
        }

        if !filter.is_empty() && s.region.contains(&filter) {
            println!("  {} #{} {} cell {:?} world ({:.2}, {:.2}) {:.0} m: {:?} -> {:?}", s.region, s.index, s.kind, s.cell, s.world.0, s.world.1, m, s.building, s.model);
        }
    }

    println!("slot kinds {kinds:?}");
    let mut pieces = std::collections::BTreeMap::<String, (usize, usize)>::new();
    for sp in &cities.sprawl {
        let e = pieces.entry(format!("{} lvl{}", sp.piece, sp.level)).or_default();
        e.0 += 1;
        e.1 += usize::from(sp.model.is_some());
    }
    println!("sprawl pieces (count, with a model): {pieces:?}");
    if !filter.is_empty() {
        let walls: Vec<(u16, u16)> = cities.sprawl.iter().filter(|p| p.region.contains(&filter) && (p.piece.starts_with("wall_") || p.piece.contains("gatehouse"))).map(|p| p.cell).collect();
        let span = |f: fn(&(u16, u16)) -> u16| (walls.iter().map(f).min(), walls.iter().map(f).max());
        println!("walls of {filter}: {} cells, columns {:?}, rows {:?}", walls.len(), span(|c| c.0), span(|c| c.1));
    }
    println!("on land {land}, in the sea {sea}; models in the packs {have}, missing {missing}");
}
