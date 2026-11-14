//! Exploration: the tiles over some cells of a kit map as the fill places them (both layers: name, origin, flags, climate).
//!
//!   cargo run --release -p wce_terrain_io --example cell_tiles -- <kit raw_data> <map | campaigns/<map>> <x> <y> [<x> <y> ...]

use std::path::PathBuf;

use wce_terrain::{LayerRole, LfRole, MapKind};
use wce_terrain_io::{MapFiles, attila};
use wce_tile::battle_map::fill::MapImages;
use wce_tile::battle_map::tile_db::TileDatabase;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let raw = PathBuf::from(&a[0]);
    let (kind, folder, name, base) = match a[1].strip_prefix("campaigns/") {
        Some(n) => (MapKind::Campaign, "terrain/campaigns", n.to_owned(), "campaign"),
        None => (MapKind::Battle, "terrain/battles", a[1].clone(), "battle"),
    };

    let m = attila::read(&name, kind, &MapFiles::from_dir(&raw.join(folder).join(&name)).expect("folder")).expect("map");
    let db = TileDatabase::load(&raw.join(format!("terrain/tiles/{base}/_tile_database"))).expect("database");
    let rgb = |l: &wce_terrain::cells::CellLayer| -> Vec<[u8; 3]> {
        l.cells()
            .iter()
            .map(|&i| {
                let c = l.palette()[usize::from(i)];
                [c[0], c[1], c[2]]
            })
            .collect()
    };
    let tiles = m.layer(&LayerRole::TileMap).expect("tile map").get();
    let climate = m.layer(&LayerRole::Climate).map(|l| rgb(l.get()));
    let lf = m
        .lf(LfRole::Land)
        .map(|f| f.get())
        .map(|f| (f.to_le_rows().as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes(*c)).collect::<Vec<u16>>(), f.width(), f.height()));
    let t = rgb(tiles);
    let images = MapImages {
        width: tiles.width(),
        height: tiles.height(),
        tile_map: &t,
        climate_map: climate.as_deref(),
        explicit_tiles: None,
        lf: None,
        lf_u16: lf.as_ref().map(|(v, w, h)| (v.as_slice(), *w, *h)),
        campaign: kind == MapKind::Campaign,
    };

    let (map, _, _) = wce_tile::battle_map::build_images(&db, &images);
    let w = map.width;
    for xy in a[2..].chunks(2) {
        let (x, y): (u32, u32) = (xy[0].parse().expect("x"), xy[1].parse().expect("y"));
        println!("cell {x}, {y}: tile map colour {:?}", t[(y * w + x) as usize]);
        for (k, layer) in map.layers.iter().enumerate() {
            let c = layer[(y * w + x) as usize];
            let o = if c.index < 0 { layer[(-1 - c.index) as usize] } else { c };
            let n = if o.index > 0 { map.names[o.index as usize - 1].as_str() } else { "-" };
            println!("  layer {k}: {n} origin {:?} flags {:#06x} climate {}", c.origin, c.flags, c.climate);
        }
    }
}
