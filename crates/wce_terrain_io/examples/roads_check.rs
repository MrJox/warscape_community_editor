//! A campaign's roads at its start (`wce_terrain_io::attila::roads`): the regions at each road level, the regions whose key has a text
//! with their value and level, and whether the settlements' cells lie in their own region (`HEX_MAP_DATA`'s region byte).
//!
//!   cargo run --release -p wce_terrain_io --example roads_check -- <game dir> <campaign, e.g. main_attila> <map, e.g. main_attila_map> [region filter]

use wce_terrain_io::attila::cities::{DisplayTables, read as read_cities};
use wce_terrain_io::attila::roads::{RoadTables, read};
use wce_vfs::{Config, GameProfile, Vfs};

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let game = GameProfile::from_env().clone();
    let mut cfg = Config::new(&a[0], game.clone());
    cfg.mod_packs = Some(vec![]);
    let vfs = Vfs::mount(&cfg).expect("vfs");
    let (campaign, map) = (&a[1], &a[2]);
    let filter = a.get(3).cloned().unwrap_or_default();
    let schema = rpfm_lib::schema::Schema::load(&wce_foundation::settings::rpfm_schema_path(&game).expect("an rpfm schema"), None).expect("schema");
    let list = |p: &str| vfs.list(p, "").iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
    let read_vfs = |p: &str| vfs.read(p).ok().map(|b| b.to_vec());
    let tables = RoadTables::read(&schema, campaign, &list, &read_vfs);
    println!("thresholds {:?}; {} buildings with a road value", tables.thresholds, tables.values.len());
    let map_data = vfs.read(&format!("campaign_maps/{map}/map_data.esf")).expect("map_data.esf");
    let startpos = std::fs::read(std::path::Path::new(&a[0]).join(format!("data/campaigns/{campaign}/startpos.esf"))).expect("startpos.esf");
    let upgrades = vfs.read("terrain/tiles/campaign/tile_upgrades.xml").expect("tile_upgrades.xml");
    let dynamic = vfs.read(&format!("campaign_maps/{map}/dynamic_resources.esf")).ok();
    let roads = read(&map_data, &startpos, &upgrades, &tables, dynamic.as_deref()).expect("roads");
    println!("farmland: {} areas reached, {} not, {} hex cells", roads.areas_on, roads.areas_off, roads.farms.len());

    // The regions' highest level in each superchain the areas name, and how many areas each rule would reach.
    let buildings = wce_terrain_io::attila::roads::region_buildings(&startpos).expect("buildings");
    let areas = dynamic.as_deref().map(|d| wce_terrain_io::attila::roads::dynamic_resources(d).expect("areas")).unwrap_or_default();
    let mut tiers = std::collections::BTreeMap::<String, usize>::new();
    let (mut level_rule, mut tier_rule) = (0, 0);
    for a in &areas {
        let best = roads.hex.regions.get(a.region).and_then(|r| buildings.get(r)).into_iter().flatten()
            .filter_map(|b| tables.chain_levels.get(b).filter(|(sc, _)| *sc == a.superchain).map(|(_, l)| (*l, b.clone())))
            .max();
        if a.level == 1 {
            *tiers.entry(best.as_ref().map_or("none".to_owned(), |(l, b)| format!("{l} {b}"))).or_default() += 1;
        }

        level_rule += usize::from(best.as_ref().is_some_and(|(l, _)| *l >= a.level));
        tier_rule += usize::from(best.as_ref().is_some_and(|(l, _)| *l + 1 >= a.level));
    }

    println!("areas reached: chain level >= area level {level_rule}, chain level + 1 >= area level {tier_rule}");
    let mut shown = std::collections::BTreeSet::new();
    for a in &areas {
        let best = roads.hex.regions.get(a.region).and_then(|r| buildings.get(r)).into_iter().flatten()
            .filter_map(|b| tables.chain_levels.get(b).filter(|(sc, _)| *sc == a.superchain).map(|(_, l)| *l)).max();
        if best.is_some_and(|l| l >= a.level) && shown.insert(a.region) && shown.len() <= 8 {
            println!("  farmland: {} level {} {} cells, first {:?}", roads.hex.regions[a.region], a.level, a.cells.len(), a.cells.first());
        }
    }
    println!("regions' best agriculture building (per level-1 area): {tiers:?}");
    println!("hex {} x {}, {} regions; regions a level {:?}; upgrade groups {:?}", roads.hex.w, roads.hex.h, roads.hex.regions.len(), roads.level_counts(),
        roads.upgrades.groups.iter().map(|(k, v)| format!("{k} {}", v.len())).collect::<Vec<_>>());

    // Road cells a level.
    let mut cells = vec![0usize; roads.level_counts().len()];
    for r in 0..roads.hex.h {
        for c in 0..roads.hex.w {
            if roads.hex.road((c, r)) != 0 {
                cells[roads.levels.get(usize::from(roads.hex.region((c, r)))).copied().unwrap_or(0)] += 1;
            }
        }
    }

    println!("road cells a level {cells:?}");
    for (i, key) in roads.hex.regions.iter().enumerate() {
        if !filter.is_empty() && key.contains(&filter) {
            println!("  {i} {key}: value {} level {}", roads.values[i], roads.levels[i]);
        }
    }

    // The settlements' main slots: in their own region?
    let tables = DisplayTables::read(&schema, &list, &read_vfs);
    let cities = read_cities(&map_data, &startpos, &tables).expect("cities");
    let (mut same, mut other) = (0, vec![]);
    for s in cities.slots.iter().filter(|s| s.index == 0) {
        let got = roads.hex.cell_at(s.world.0, s.world.1).map(|c| roads.hex.regions[usize::from(roads.hex.region(c))].clone());
        if got.as_deref() == Some(s.region.as_str()) { same += 1 } else { other.push(format!("{} in {got:?}", s.region)) }
    }

    println!("main slots in their own region {same}, elsewhere {}: {:?}", other.len(), other.iter().take(6).collect::<Vec<_>>());
}
