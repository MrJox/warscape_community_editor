//! The entries the decompiler makes from the packs' tile databases against the kit's `_tile_database/TILES/*.xml`, byte for byte.
//!
//!   cargo run --release -p wce_tile_io --example tile_decompile_entry_check -- <game folder>
//!
//! For each database of `tiles.pack` (battle, campaign) and each of its tiles that the kit has an entry for: `entry_xml` equal to the
//! kit's file. Prints the counts and the first differences.

use std::io::Cursor;
use std::path::PathBuf;

use rpfm_lib::files::Decodeable;
use rpfm_lib::files::pack::Pack;
use rpfm_lib::files::tile_database::TileDatabase;
use rpfm_lib::games::supported_games::SupportedGames;
use wce_formats::xml_doc::XmlDoc;
use wce_tile_io::attila::decompile::{entry_file_name, entry_xml};

fn main() {
    let game = PathBuf::from(std::env::args().nth(1).expect("usage: tile_decompile_entry_check <game folder>"));
    let games = SupportedGames::default();
    let attila = games.game("attila").expect("attila");
    let mut pack = Pack::read_and_merge(&[game.join("data").join("tiles.pack")], attila, true, false, false).expect("tiles.pack");
    let (mut same, mut differ, mut missing, mut values_only) = (0, 0, 0, 0);
    for kind in ["battle", "campaign"] {
        let path = format!("terrain/tiles/{kind}/tile_database.bin");
        let bytes = pack.files_mut().get_mut(&path).expect("database").encode(&None, false, false, true).unwrap().unwrap();
        let db = TileDatabase::decode(&mut Cursor::new(&bytes), &None).expect("database");
        let dir = game.join("assembly_kit/raw_data/terrain/tiles").join(kind).join("_tile_database/TILES");
        for (index, t) in db.tiles().iter().enumerate() {
            let file = dir.join(entry_file_name(t.location()));
            let Ok(kit) = std::fs::read(&file) else {
                missing += 1;
                continue;
            };

            let mine = entry_xml(&db, index);
            if mine == kit {
                same += 1;
            } else {
                differ += 1;
                // Same elements and attribute names, other values: the database and the kit disagree on data, not on layout.
                let shape = |b: &[u8]| {
                    let d = XmlDoc::parse(b.to_vec()).expect("xml");
                    d.elements().iter().map(|e| (e.name.clone(), e.attrs.iter().map(|a| a.name.clone()).collect::<Vec<_>>())).collect::<Vec<_>>()
                };

                if shape(&mine) == shape(&kit) {
                    values_only += 1;
                }

                if differ <= 12 {
                    let i = mine.iter().zip(&kit).take_while(|(a, b)| a == b).count();
                    let show = |x: &[u8]| String::from_utf8_lossy(&x[i.saturating_sub(30)..(i + 50).min(x.len())]).replace(['\r', '\n'], "|");
                    println!("{}: at {i}: mine {:?} / kit {:?}", file.display(), show(&mine), show(&kit));
                }
            }
        }
    }

    println!("entries: {same} identical, {differ} differ ({values_only} only in values), {missing} without a kit file");
}
