//! Exploration: a campaign build's `terrain.tile_list` and `tile_map.tiles` (ours, e.g. `build_check`'s `WCE_OUT`) against BOB's: the
//! headers, the placements by tile folder and layer, the tile map cells that differ.
//!
//!   cargo run --release -p wce_terrain_io --example campaign_fill_diff -- <our folder> <BOB's folder>

use std::collections::BTreeMap;
use std::path::PathBuf;

use wce_tile::battle_map::tile_list::TileList;
use wce_tile::battle_map::tile_map::TileMap;

fn folder(name: &str) -> String {
    let t = name.split('\\').next().unwrap_or(name);
    t.rsplit_once('/').map_or(t.to_owned(), |x| x.0.to_owned())
}

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let (ours, theirs) = (PathBuf::from(&a[0]), PathBuf::from(&a[1]));
    let x = TileList::parse(&std::fs::read(ours.join("terrain.tile_list")).expect("ours")).expect("ours");
    let y = TileList::parse(&std::fs::read(theirs.join("terrain.tile_list")).expect("BOB's")).expect("BOB's");
    println!("ours:  {} names {} placements lf {:?} {:?} {:?} strings {:?} size {:?} centre {:?} offset {:?} bounds {:?} campaign {}",
        x.names.len(), x.placements.len(), x.lf_position, x.lf_scale, x.unknown, x.strings, x.battle_size, x.battle_centre, x.origin_offset, x.bounds, x.is_campaign);
    println!("BOB's: {} names {} placements lf {:?} {:?} {:?} strings {:?} size {:?} centre {:?} offset {:?} bounds {:?} campaign {}",
        y.names.len(), y.placements.len(), y.lf_position, y.lf_scale, y.unknown, y.strings, y.battle_size, y.battle_centre, y.origin_offset, y.bounds, y.is_campaign);
    // Placements by (folder, layer bits): ours, BOB's.
    let mut by: BTreeMap<(String, u16), [usize; 2]> = BTreeMap::new();
    for (k, l) in [&x, &y].iter().enumerate() {
        for p in &l.placements {
            by.entry((folder(&l.names[p.name]), p.flags & 0x700)).or_default()[k] += 1;
        }
    }

    for ((f, layer), [o, t]) in &by {
        if o != t {
            println!("  {f:60} layer {layer:#05x}: ours {o:7} BOB's {t:7}");
        }
    }

    for k in 0..x.placements.len().min(y.placements.len()) {
        let (p, q) = (&x.placements[k], &y.placements[k]);
        if (&x.names[p.name], p.x, p.y, p.flags, p.climate) != (&y.names[q.name], q.x, q.y, q.flags, q.climate) {
            println!("first differing placement #{k}: ours {} {:?}\n                                BOB's {} {:?}", x.names[p.name], p, y.names[q.name], q);
            break;
        }
    }

    let m = TileMap::parse(&std::fs::read(ours.join("tile_map.index")).unwrap_or_else(|_| std::fs::read(theirs.join("tile_map.index")).expect("index")), &std::fs::read(ours.join("tile_map.tiles")).expect("ours")).expect("ours");
    let n = TileMap::parse(&std::fs::read(theirs.join("tile_map.index")).expect("index"), &std::fs::read(theirs.join("tile_map.tiles")).expect("BOB's")).expect("BOB's");
    println!("tile maps {} x {} / {} x {}, names {} / {}", m.width, m.height, n.width, n.height, m.names.len(), n.names.len());
    for l in 0..2 {
        let diff: Vec<usize> = (0..m.layers[l].len()).filter(|&i| {
            let (mut c, mut d) = (m.layers[l][i], n.layers[l][i]);
            if c.index <= 0 {
                (c.lf, d.lf) = ([0.0; 2], [0.0; 2]);
            }
            c != d
        }).collect();
        let empty_ours = diff.iter().filter(|&&i| m.layers[l][i].index < 0).count();
        let empty_theirs = diff.iter().filter(|&&i| n.layers[l][i].index < 0).count();
        println!("layer {l}: {} cells differ ({empty_ours} empty in ours, {empty_theirs} in BOB's)", diff.len());
        for &i in diff.iter().take(3) {
            println!("  cell ({}, {}): ours {:?} BOB's {:?}", i as u32 % m.width, i as u32 / m.width, m.layers[l][i], n.layers[l][i]);
        }
    }
}
