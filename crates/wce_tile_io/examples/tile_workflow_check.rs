//! The editor's round trip over the game's packs: each tile folder is opened as the editor opens an exported tile (decompiled into a
//! workspace, `attila::workflow::open_exported`), then saved as the editor saves it (`save_exported`), and the saved folder is compared
//! with the packs' file by file, and the saved tile database with the one in effect.
//!
//!   cargo run --release -p wce_tile_io --example tile_workflow_check -- <game folder> <scratch dir> <tile folder> [<tile folder> ...]
//!
//! `$WCE_GAME_KEY` chooses the game (`rome_2`; Attila by default).
//!
//! A tile folder is the packs' path, `terrain/tiles/battle/farm_western/1x1/aa`. The scratch dir gets `workspace/` and `out/`.

use std::path::PathBuf;

use wce_tile_io::attila::TedProject;
use wce_tile_io::attila::workflow::{Game, open_exported, read_database, save_exported};
use wce_tile_io::{TileFiles, TileFormat};
use wce_tile_model::{MapKind, Node, Samples, Value};

/// Moves the first VEGETATION_INSTANCE 10 scene units in x and raises a 20 x 20 patch of height_map_0 by 1000 steps, then writes the
/// project back.
fn edit(dir: &std::path::Path) -> Result<(), String> {
    fn first_tree(n: &mut Node) -> Option<&mut Node> {
        if n.class == "VEGETATION_INSTANCE" {
            return Some(n);
        }

        n.children.iter_mut().find_map(first_tree)
    }

    let files = TileFiles::from_kit_dir(dir).map_err(|e| e.to_string())?;
    let (mut p, _) = TedProject.read(&files).map_err(|e| e.to_string())?;
    if let Some(t) = p.scene.as_mut().and_then(first_tree)
        && let Some(Value::Vec3(v)) = t.get("position").cloned()
    {
        t.set("position", Value::Vec3([v[0] + 10.0, v[1], v[2]]));
    }

    if let Some(m) = p.maps.iter_mut().find(|m| m.kind == MapKind::Height)
        && let Samples::U16(h) = &mut m.raster.samples
    {
        let w = m.raster.width as usize;
        for y in 40..60 {
            for x in 40..60 {
                h[y * w + x] = h[y * w + x].saturating_add(1000);
            }
        }
    }

    let (written, _) = TedProject.write(&p).map_err(|e| e.to_string())?;
    written.write_to(dir, None).map_err(|e| e.to_string())
}

fn main() -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() < 3 {
        return Err("usage: tile_workflow_check <game folder> <scratch dir> <tile folder> ...".to_owned());
    }

    let game = Game::load(wce_foundation::game::GameProfile::from_env(), &PathBuf::from(&args[0]))?;
    for w in &game.warnings {
        println!("warning: {w}");
    }

    let scratch = PathBuf::from(&args[1]);
    for folder in &args[2..] {
        let files = game.exported_from_packs(folder)?;
        let (project, losses) = open_exported(&game, &files, folder, &scratch.join("workspace"))?;
        println!("{folder}: opened as {} ({} notes)", project.display(), losses.notes.len());
        // WCE_WORKFLOW_EDIT=1: an edit as the editor would save it: the first tree moved 5 m and a patch of the height map raised.
        if std::env::var_os("WCE_WORKFLOW_EDIT").is_some() {
            edit(&project)?;
        }

        let out = scratch.join("out");
        let (dir, notes) = save_exported(&game, &project, &out)?;
        for n in &notes.notes {
            println!("  note: {n}");
        }

        // The packs' names are lower case; BOB's keep the categories' case.
        let saved: std::collections::BTreeMap<String, PathBuf> =
            std::fs::read_dir(&dir).map_err(|e| e.to_string())?.flatten().map(|e| (e.file_name().to_string_lossy().to_lowercase(), e.path())).collect();
        let (mut same, mut differ, mut missing) = (vec![], vec![], vec![]);
        for (name, blob) in &files.folder {
            let theirs = blob.bytes().map_err(|e| e.to_string())?;
            match saved.get(&name.to_lowercase()).map(std::fs::read).unwrap_or_else(|| Err(std::io::ErrorKind::NotFound.into())) {
                Ok(ours) if ours == theirs[..] => same.push(name.clone()),
                Ok(_) => differ.push(name.clone()),
                Err(_) => missing.push(name.clone()),
            }
        }

        let extra = saved.keys().filter(|n| !files.folder.keys().any(|k| k.to_lowercase() == **n)).cloned().collect::<Vec<_>>();
        println!("  files: {} the same, {} differ {:?}, {} missing {:?}, {} extra {:?}", same.len(), differ.len(), differ, missing.len(), missing, extra.len(), extra);

        let kind = folder.split('/').nth(2).unwrap_or("battle");
        let saved = std::fs::read(out.join("terrain/tiles").join(kind).join("tile_database.bin")).map_err(|e| e.to_string())?;
        let saved = read_database(&saved)?;
        let base = game.data.database(kind).ok_or("no database")?;
        let location = folder.trim_start_matches("terrain/tiles/").rsplit_once('/').map(|(l, _)| l.replace('/', "\\")).unwrap_or_default();
        let find = |db: &rpfm_lib::files::tile_database::TileDatabase| db.tiles().iter().position(|t| t.location().to_lowercase().ends_with(&location.to_lowercase()));
        let (a, b) = (find(base), find(&saved));
        println!(
            "  tile database: {} tiles (in effect {}), the entry at {:?} (in effect {:?}), {}",
            saved.tiles().len(),
            base.tiles().len(),
            b,
            a,
            if saved == *base { "the same database" } else { "differs" }
        );
        if saved != *base
            && let (Some(a), Some(b)) = (a, b)
        {
            println!("    entry the same: {}", base.tiles()[a] == saved.tiles()[b]);
            if base.tiles()[a] != saved.tiles()[b] {
                println!("    in effect {:?}\n    saved     {:?}", base.tiles()[a], saved.tiles()[b]);
            }
        }
    }

    Ok(())
}
