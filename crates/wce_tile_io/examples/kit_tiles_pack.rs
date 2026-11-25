//! Exports kit tiles into a mod pack as their folders alone (no tile database: the one the game reads already lists them), for kit tiles the
//! packs lack (Attila's river crossings: 2 of the kit's 29 ship, docs/plans/river_bridges.md). For each tile: its export's files, the height
//! range its export's entry has against the database the game reads (a pack of folders alone is right only where they are the same), and
//! with `--compare` each file against the packs' own (the tiles that ship, to check the export against the game's).
//!
//!   cargo run --release -p wce_tile_io --example kit_tiles_pack -- <game folder> <out .pack|--dry> [--compare] <project folder> [...]
//!
//! A project folder is a kit's variation folder (`.../raw_data/terrain/tiles/battle/river_crossing/2x2_riv_lb_rt_road_bl_tl/aa`).

use std::path::PathBuf;

use wce_tile_io::attila::workflow::{Game, exported_pack_files, read_database};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() < 3 {
        eprintln!("usage: kit_tiles_pack <game folder> <out .pack|--dry> [--compare] <project folder> [...]");
        std::process::exit(2);
    }

    let compare = args.iter().any(|a| a == "--compare");
    let projects = args[2..].iter().filter(|a| *a != "--compare").collect::<Vec<_>>();
    let game = Game::load(wce_foundation::game::GameProfile::from_env(), &PathBuf::from(&args[0])).unwrap_or_else(|e| panic!("game: {e}"));
    let in_effect = game.data.database("battle").cloned();
    let mut folders = vec![];
    let mut files = vec![];
    let mut failed = false;
    for project in projects {
        let (folder, out, notes) = match exported_pack_files(&game, &PathBuf::from(project), None, &|_| None) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("{project}: {e}");
                failed = true;
                continue;
            }
        };

        // The height range of the export's entry against the database the game reads.
        let range = |db: &rpfm_lib::files::tile_database::TileDatabase| {
            let (location, variation) = folder.rsplit_once('/')?;
            let location = location.replace('/', "\\").to_lowercase();
            let tile = db.tiles().iter().find(|t| t.location().to_lowercase() == location)?;
            let v = tile.variations().iter().find(|v| v.location().eq_ignore_ascii_case(variation))?;
            Some((*v.min_height(), *v.scale(), *v.raw_data_tri_density()))
        };
        let exported = out.iter().find(|(p, _)| p.ends_with("tile_database.bin")).and_then(|(_, b)| read_database(b).ok());
        let (mine, theirs) = (exported.as_ref().and_then(range), in_effect.as_ref().and_then(range));
        let same = mine.is_some() && mine == theirs;
        if !same {
            failed = true;
        }

        let own = out.into_iter().filter(|(p, _)| p.starts_with(&format!("{folder}/"))).collect::<Vec<_>>();
        println!(
            "{folder}: {} files, height range {} (export {mine:?}, game {theirs:?}){}",
            own.len(),
            if same { "same" } else { "DIFFERS" },
            if notes.notes.is_empty() { String::new() } else { format!(", {} note(s)", notes.notes.len()) }
        );
        for n in &notes.notes {
            println!("    note: {n}");
        }

        if compare {
            for (path, bytes) in &own {
                match game.files.read(path) {
                    Ok(theirs) if theirs[..] == bytes[..] => println!("    same     {path}"),
                    Ok(theirs) => println!("    differs  {path} ({} against {} bytes)", bytes.len(), theirs.len()),
                    Err(_) => println!("    new      {path} ({} bytes)", bytes.len()),
                }
            }

            for p in game.files.list(&format!("{folder}/"), "") {
                if !own.iter().any(|(q, _)| q.eq_ignore_ascii_case(p)) {
                    println!("    missing  {p} (in the packs, not in the export)");
                }
            }
        }

        folders.push(folder);
        files.extend(own);
    }

    if args[1] != "--dry" {
        let folder_refs = folders.iter().map(String::as_str).collect::<Vec<_>>();
        if let Err(e) = wce_vfs::mod_pack::write_files(&game.profile, &PathBuf::from(&args[1]), &folder_refs, &files) {
            eprintln!("{}: {e}", args[1]);
            std::process::exit(1);
        }

        println!("{} tiles, {} files into {}", folders.len(), files.len(), args[1]);
    }

    if failed {
        std::process::exit(1);
    }
}
