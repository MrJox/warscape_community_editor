//! Rome 2's tile databases (`rome2::tile_database`) from the game's packs: every tile written back into its set's file must give the
//! file byte for byte, and the file read again the same tile.
//!
//!   cargo run --release -p wce_tile_io --example rome2_tile_database_check -- <Rome 2 folder>

use std::path::PathBuf;

use wce_tile_io::rome2::tile_database::Rome2TileDatabase;
use wce_vfs::{Config, GameProfile, Vfs};

fn main() {
    let game = PathBuf::from(std::env::args().nth(1).expect("usage: rome2_tile_database_check <Rome 2 folder>"));
    let vfs = Vfs::mount(&Config::new(game, GameProfile::ROME_2)).expect("the game's packs");
    let mut failed = 0;
    for kind in ["battle", "campaign"] {
        let db = Rome2TileDatabase::from_files(&vfs, kind).expect("the database");
        let (mut same, mut differ) = (0, 0);
        for tile in db.database.tiles() {
            let written = db.with_tile(tile).expect("written");
            let file = db.set_file(tile.tile_set()).unwrap_or_default();
            let ok = written.len() == 1 && written[0].0 == file && written[0].1 == db.files[file];
            if ok {
                same += 1;
            } else {
                differ += 1;
                if differ <= 5 {
                    println!("  {kind} {}: {} files written ({})", tile.location(), written.len(), written.iter().map(|w| w.0.as_str()).collect::<Vec<_>>().join(", "));
                    if let Some((_, bytes)) = written.iter().find(|w| w.0 == file) {
                        let (a, b) = (&db.files[file], bytes);
                        let at = a.iter().zip(b.iter()).position(|(x, y)| x != y).unwrap_or(a.len().min(b.len()));
                        let show = |v: &[u8]| String::from_utf8_lossy(&v[at.saturating_sub(60)..(at + 120).min(v.len())]).into_owned();
                        println!("    file: {}\n    ours: {}", show(a), show(b));
                    }
                }
            }
        }

        println!(
            "{kind}: {} files, {} tile sets, {} tiles, {} texture groups; {same} tiles write their file back byte for byte, {differ} do not",
            db.files.len(), db.database.tile_sets().len(), db.database.tiles().len(), db.texture_groups.len()
        );
        failed += differ;
    }

    if failed > 0 {
        println!("FAILED: {failed} tiles");
        std::process::exit(1);
    }
}
