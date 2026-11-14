//! Rome 2's version 6 tile maps made again from their placements (`TileMap::placements`, then `TileMap::with_placements` with the tile
//! database): the shipped files byte for byte, so an edit of a campaign battle map's placements writes the rest of it as the game's.
//! The packs' maps with a tile map; the database in the kit's format (the Terrain tool's packs tree: `<cache>/neighbours/rome_2_*/
//! raw_data/terrain/tiles/battle/_tile_database`). Exit 1 otherwise.
//!
//!   WCE_GAME_KEY=rome_2 cargo run --release -p wce_tile --example tile_map_rebuild_check -- <game dir> <tile database folder>

use wce_tile::battle_map::tile_db::TileDatabase;
use wce_tile::battle_map::tile_map::TileMap;
use wce_vfs::{Config, GameProfile, Vfs};

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let mut cfg = Config::new(&a[0], GameProfile::from_env().clone());
    cfg.mod_packs = Some(vec![]);
    let vfs = Vfs::mount(&cfg).expect("vfs");
    let db = TileDatabase::load(std::path::Path::new(&a[1])).expect("database");
    let mut bad = 0;
    for idx in vfs.list("terrain/battles/", "/tile_map.index") {
        let folder = idx.trim_end_matches("/tile_map.index");
        let (index, tiles) = (vfs.read(&idx).expect("index"), vfs.read(&format!("{folder}/tile_map.tiles")).expect("tiles"));
        let m = TileMap::parse(&index, &tiles).expect("tile map");
        if m.version >= 8 {
            continue;
        }

        let again = m.with_placements(&m.placements(), &db);
        let (i2, t2) = again.write();
        let same = i2 == index && t2 == tiles;
        let cells = m.layers[0].iter().zip(&again.layers[0]).filter(|(x, y)| x != y).count();
        let first = m.layers[0].iter().zip(&again.layers[0]).position(|(x, y)| x != y).map(|i| {
            format!(
                " first cell {} {}: shipped {:?} / made {:?}",
                i as u32 % m.width,
                i as u32 / m.width,
                m.layers[0][i],
                again.layers[0][i]
            )
        });
        println!(
            "{folder}: {} placements; {}{}",
            m.placements().placements.len(),
            if same { "the same".to_owned() } else { format!("DIFFERS: {cells} cells") },
            first.unwrap_or_default()
        );
        if !same {
            bad += 1;
        }
    }

    if bad > 0 {
        std::process::exit(1);
    }
}
