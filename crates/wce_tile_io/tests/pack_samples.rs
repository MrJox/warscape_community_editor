//! Exported tiles of Attila's packs through `attila.tile_binary` and back, read from the packs: a vanilla settlement, a mountain, a farm with
//! the older lists and `tile.ted_binary`, a campaign tile, a version 4 height map, TDD's Hornburg and a TDD campaign tile whose height map
//! has a short block table, an assembly kit map tile.
//!
//! Each is also decompiled (`attila::decompile`), written as a TEd project and read back into the same model.
//!
//! Needs `WCE_ATTILA_GAME=<Attila>` (the folder with `data/`); without it the test says so and passes. Every tile of the packs: the
//! `tile_binary_roundtrip_check` and `tile_decompile_check` examples.

use std::collections::BTreeMap;
use std::path::PathBuf;

use rpfm_lib::files::Decodeable;
use rpfm_lib::files::pack::Pack;
use rpfm_lib::files::tile_database::TileDatabase;
use rpfm_lib::games::supported_games::SupportedGames;
use wce_tile_io::attila::decompile::{Context, decompile};
use wce_tile_io::attila::{TedProject, TileBinary};
use wce_tile_io::{PartRead, PartWritten, TileFiles, TileFormat};
use wce_tile_model::Blob;

const SAMPLES: &[(&str, &str)] = &[
    ("tiles4.pack", "terrain/tiles/battle/settlement_western_cities/western_city_b/small"),
    ("tiles2.pack", "terrain/tiles/battle/atlantic_mountains/custom_mont_blanc/aa"),
    ("tiles.pack", "terrain/tiles/battle/farm_eastern/2x1/aa"),
    ("tiles.pack", "terrain/tiles/campaign/roads/2x2_junction_br_tl_tr/aa"),
    ("tiles.pack", "terrain/tiles/battle/roads/2x2_straight_br_lb/bb"),
    ("tdd_pack2_battles_1.1.0-rc1.pack", "terrain/tiles/battle/tdd_settlement_middle_men_cities/middle_men_city_hornburg/medium"),
    ("tdd_pack3_campaign_1.0.0.10.pack", "terrain/tiles/campaign/lakes/12x28_long_lake/aa"),
    ("castle_hill.pack", "terrain/tiles/battle/assembly_kit/castle_hill/battlefield"),
];

#[test]
fn pack_samples_round_trip() {
    let Some(game) = std::env::var_os("WCE_ATTILA_GAME").map(PathBuf::from) else {
        eprintln!("WCE_ATTILA_GAME is not set: pack samples skipped");
        return;
    };

    let games = SupportedGames::default();
    let attila = games.game("attila").expect("attila");
    let mut failures = vec![];
    let mut by_pack: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for (pack, dir) in SAMPLES {
        by_pack.entry(pack).or_default().push(dir);
    }

    // The game's tile databases, a mod's own before them.
    let read = |name: &str| Pack::read_and_merge(&[game.join("data").join(name)], attila, true, false, false).unwrap_or_else(|e| panic!("{name}: {e}"));
    let game_dbs = databases(&mut read("tiles.pack"));
    for (pack_name, dirs) in by_pack {
        let mut pack = read(pack_name);
        let mut dbs = if pack_name.starts_with("tiles") { vec![] } else { databases(&mut pack) };
        dbs.extend(game_dbs.iter().cloned());
        for dir in dirs {
            let prefix = format!("{dir}/");
            let names = pack.files().keys().filter(|n| n.starts_with(&prefix) && !n[prefix.len()..].contains('/')).cloned().collect::<Vec<_>>();
            if names.is_empty() {
                failures.push(format!("{pack_name}:{dir}: not in the pack"));
                continue;
            }

            let mut files = TileFiles::default();
            for n in names {
                let bytes = pack.files_mut().get_mut(&n).unwrap().encode(&None, false, false, true).unwrap().unwrap();
                files.folder.insert(n[prefix.len()..].to_owned(), Blob::Bytes(bytes.into()));
            }

            let (tile, report) = TileBinary.read(&files).unwrap();
            for (part, how) in &report.parts {
                if let PartRead::Failed(e) = how {
                    failures.push(format!("{dir}/{part}: {e}"));
                }
            }

            let (out, wreport) = TileBinary.write(&tile).unwrap_or_else(|e| panic!("{dir}: {e}"));
            for (part, how) in &wreport.parts {
                if *how != PartWritten::Copied && out.folder[part].bytes().unwrap() != files.folder[part].bytes().unwrap() {
                    failures.push(format!("{dir}/{part}: differs"));
                }
            }

            let (back, _) = TileBinary.read(&out).unwrap();
            if back.semantic() != tile.semantic() {
                failures.push(format!("{dir}: another model read back"));
            }

            // Decompiled, written as a TEd project, read back.
            match decompile(&tile, dir, &Context { databases: &dbs, ..Default::default() }) {
                Ok((raw, _)) => {
                    let (written, _) = TedProject.write(&raw).unwrap_or_else(|e| panic!("{dir}: {e}"));
                    let (back, _) = TedProject.read(&written).unwrap_or_else(|e| panic!("{dir}: {e}"));
                    if back.semantic() != raw.semantic() {
                        failures.push(format!("{dir}: the decompiled project reads back into another model"));
                    }

                    if raw.scene.is_none() || raw.entry.is_none() {
                        failures.push(format!("{dir}: decompiled without a scene or an entry"));
                    }
                }
                Err(e) => failures.push(format!("{dir}: decompile: {e}")),
            }
        }
    }

    assert!(failures.is_empty(), "{failures:#?}");
}

/// The tile databases of a pack (`terrain/tiles/*/tile_database.bin`).
fn databases(pack: &mut Pack) -> Vec<TileDatabase> {
    let names = pack.files().keys().filter(|n| n.starts_with("terrain/tiles/") && n.ends_with("/tile_database.bin")).cloned().collect::<Vec<_>>();
    names
        .iter()
        .filter_map(|n| {
            let bytes = pack.files_mut().get_mut(n)?.encode(&None, false, false, true).ok()??;
            TileDatabase::decode(&mut std::io::Cursor::new(&bytes), &None).ok()
        })
        .collect()
}
