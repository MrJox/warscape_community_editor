//! The packs' battle maps: each `lf_height_map.data` (CHMF) made again from its `lf_height_map.dds` by the built maps' rule
//! (`wce_terrain_io::built::chmf`: the DDS's samples encoded, the shipped file's version and region tag maxima kept) against the shipped
//! file. A game without kit maps (Rome 2) has its LF edited in this form: this is what its export writes after an LF edit. A shipped CHMF
//! of another region layout is kept (counted apart). Exit 1 on a difference.
//!
//!   WCE_GAME_KEY=rome_2 cargo run --release -p wce_terrain_io --example built_lf_check -- <game dir> [--vanilla] [map filter]

use wce_terrain::paged::PagedU16;
use wce_tile::battle_map::tile_list::TileList;
use wce_vfs::{Config, GameProfile, Vfs};

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let mut cfg = Config::new(&a[0], GameProfile::from_env().clone());
    if a.iter().any(|x| x == "--vanilla") {
        cfg.mod_packs = Some(vec![]);
    }

    let filter = a.iter().skip(1).find(|x| !x.starts_with("--")).cloned().unwrap_or_default();
    let vfs = Vfs::mount(&cfg).expect("vfs");
    let (mut same, mut bad, mut kept, mut skipped) = (0, 0, 0, 0);
    for data in vfs.list("terrain/battles/", "/lf_height_map.data") {
        let folder = data.trim_end_matches("/lf_height_map.data");
        if !folder.contains(&filter) {
            continue;
        }

        let read = |n: &str| vfs.read(&format!("{folder}/{n}")).ok();
        let (Some(want), Some(dds)) = (read("lf_height_map.data"), read("lf_height_map.dds")) else { skipped += 1; continue };
        let cells = read("terrain.tile_list").and_then(|b| TileList::parse(&b).ok()).map(|t| (t.battle_size[0], t.battle_size[1]))
            .or_else(|| read("tile_map.index").filter(|i| i.len() >= 12).map(|i| (u32::from_le_bytes(i[4..8].try_into().unwrap()), u32::from_le_bytes(i[8..12].try_into().unwrap()))));
        let Some(cells) = cells else { skipped += 1; continue };
        let u = |o: usize| u32::from_le_bytes(dds[o..o + 4].try_into().unwrap());
        let (h, w) = (u(12), u(16));
        let field = PagedU16::from_le_rows(w, h, &dds[128..]);
        match wce_terrain_io::built::chmf(&field, cells, Some(&want)) {
            None => kept += 1,
            Some(mine) if mine == want => same += 1,
            Some(mine) => {
                bad += 1;
                println!("  {folder}: DIFFERS (sizes {} / {}, first at {:?})", mine.len(), want.len(), mine.iter().zip(&want).position(|(x, y)| x != y));
            }
        }
    }

    println!("{same} the same, {bad} differ, {kept} kept (another region layout), {skipped} without a DDS or a size");
    if bad > 0 {
        std::process::exit(1);
    }
}
