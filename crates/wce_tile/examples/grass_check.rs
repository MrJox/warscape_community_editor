//! The grass generator against the game's packed `default.grass_list.bin` of every Assembly Kit tile that has one: per tile, the list's
//! patches, ours, and how many are the same (key, x, z), in parallel.
//!
//!   cargo run --release -p wce_tile --example grass_check -- <game> [filter] [--diff] [--order] [--oracle-dir <working_data dir>]
//!
//! `--oracle-dir` compares against BOB's fresh output in a kit's working_data instead of the packs.
//!
//! `order` is whether every key's patches are in the list's order (their yaws in the game).

use std::collections::HashSet;
use std::io::Cursor;
use std::path::PathBuf;

use rayon::prelude::*;
use rpfm_lib::files::Decodeable;
use rpfm_lib::files::fastbin_lists::GrassListFile;
use wce_tile::tile_project::TileProject;
use wce_tile::vegetation::{self, grass};
use wce_vfs::{Config, GameProfile, Vfs};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let game = PathBuf::from(&args[0]);
    let filter = args.get(1).filter(|a| !a.starts_with("--")).cloned().unwrap_or_default();
    let diff = args.iter().any(|a| a == "--diff");
    let oracle_dir: Option<PathBuf> = args.iter().position(|a| a == "--oracle-dir").and_then(|i| args.get(i + 1)).map(PathBuf::from);
    let raw = game.join("assembly_kit").join("raw_data");
    let vfs = Vfs::mount(&Config::new(game.clone(), GameProfile::ATTILA)).expect("vfs");
    let params = grass::GrassParams::load(&game.join("assembly_kit/working_data/terrain/vegetation/battle/grass/max_grass.xml")).expect("max_grass.xml");
    let mut folders: Vec<String> = vfs
        .list("terrain/tiles/battle/", ".grass_list.bin")
        .into_iter()
        .filter_map(|l| l.strip_suffix("/default.grass_list.bin").map(str::to_owned))
        .filter(|f| f.contains(&filter) && raw.join(f).join("tile.ted").is_file())
        .collect();
    folders.sort();

    let results: Vec<String> = folders
        .par_iter()
        .map(|folder| {
            let project = match TileProject::open(&raw.join(folder)) {
                Ok(p) => p,
                Err(e) => return format!("{folder}\topen failed: {e}"),
            };

            let inputs = match vegetation::tile_inputs(&project) {
                Ok(i) => i,
                Err(e) => return format!("{folder}\tinputs: {e}"),
            };

            let Some(seed) = vegetation::grass_seed(&project.scene) else { return format!("{folder}\tno grass seed") };
            let bytes = match oracle_dir.as_ref() {
                Some(dir) => std::fs::read(dir.join(folder).join("default.grass_list.bin")).ok(),
                None => vfs.read(&format!("{folder}/default.grass_list.bin")).ok(),
            };
            let Some(bytes) = bytes else { return format!("{folder}\tno list") };
            let Ok(list) = GrassListFile::decode(&mut Cursor::new(bytes), &None) else { return format!("{folder}\tlist does not decode") };
            let key = |k: &str, x: f32, z: f32| (k.to_owned(), (x * 2.0).round() as i64, (z * 2.0).round() as i64);
            let theirs: HashSet<(String, i64, i64)> =
                list.grass_list().grass_list().iter().flat_map(|v| v.value().iter().map(move |i| key(v.key(), *i.x(), *i.z()))).collect();
            let mine_list = grass::generate_grass(&inputs, &params, seed);
            let mine: HashSet<(String, i64, i64)> = mine_list.iter().map(|g| key(&g.key, g.x, g.z)).collect();
            let same = mine.intersection(&theirs).count();
            // The order of each key's patches (their yaws in the game).
            let ordered = list.grass_list().grass_list().iter().all(|v| {
                let theirs = v.value().iter().map(|i| key(v.key(), *i.x(), *i.z()));
                let ours = mine_list.iter().filter(|g| g.key == *v.key()).map(|g| key(&g.key, g.x, g.z));
                theirs.eq(ours)
            });
            let mut line = format!("{folder}\ttheirs {}\tmine {}\tsame {same}\torder {}", theirs.len(), mine.len(), if ordered { "same" } else { "differs" });
            if args.iter().any(|a| a == "--order") {
                for v in list.grass_list().grass_list() {
                    let theirs: Vec<(i64, i64)> = v.value().iter().map(|i| ((*i.x() * 2.0).round() as i64, (*i.z() * 2.0).round() as i64)).collect();
                    let ours: Vec<(i64, i64)> = mine_list.iter().filter(|g| g.key == *v.key()).map(|g| ((g.x * 2.0).round() as i64, (g.z * 2.0).round() as i64)).collect();
                    let idx: Vec<String> = theirs.iter().take(12).map(|p| ours.iter().position(|q| q == p).map_or("-".into(), |i| i.to_string())).collect();
                    line += &format!("\n  {} {} ours {} order {}", v.key(), theirs.len(), ours.len(), idx.join(" "));
                }
            }

            if diff {
                let pos = |s: &HashSet<(String, i64, i64)>| s.iter().map(|(_, x, z)| (*x, *z)).collect::<HashSet<_>>();
                let (tp, mp) = (pos(&theirs), pos(&mine));
                line += &format!("\n  same positions {}; only theirs {:?}\n  only mine {:?}", tp.intersection(&mp).count(),
                    theirs.difference(&mine).take(6).collect::<Vec<_>>(), mine.difference(&theirs).take(6).collect::<Vec<_>>());
            }

            line
        })
        .collect();

    for r in results {
        println!("{r}");
    }
}
