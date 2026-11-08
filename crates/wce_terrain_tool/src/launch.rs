//! Launch in game (docs/plans/terrain_editor.md, Phase 7): the shown kit map's export (`crate::export`) in the launch pack, with a battle
//! at a battle catchment area the user picks: the Tile tool's launch (`wce_tile_tool::launch`: the battle XML, the launch pack, the
//! command line, the start, the clean-up when the game exits) with the battle map `terrain/battles/<map>/` and the BCA's centre as the
//! tile map position. One unit a side, the game's vanilla keys, day, dry, summer.

use std::path::PathBuf;

use wce_editor::editor::Editor;
use wce_scene::render_scene::DocumentId;
use wce_terrain::terry::EntityKind;
use wce_terrain_editor::MapDocument;
use wce_tile_tool::launch::{self, Battle, BattleMap, BattleWeather, Delivery, LAUNCH_PACK, Running};
use wce_tile_tool::settings::LaunchArmy;

/// Where a launch's battle is: a BCA's centre cell, its type and its size in cells (its `Catchment Region`'s).
#[derive(Clone, Debug, PartialEq)]
pub struct Spot {
    pub cell: (f64, f64),
    pub kind: String,
    pub cells: (u32, u32),
    pub name: String,
}

/// The BCA to launch at: the selected one (or the BCA of a selected region or zone), else the one given by id, else the first.
///
/// # Errors
///
/// No map, no `.terry`, no BCA.
pub fn spot(e: &Editor, doc: DocumentId, id: Option<u64>) -> Result<Spot, String> {
    let d = e.docs.find_as::<MapDocument>(doc).ok_or("no map shown")?;
    let p = d.project().ok_or("this map has no .terry project")?;
    let is_bca = |i: u64| p.entity(i).is_some_and(|x| matches!(p.kind(x), EntityKind::Bca(_)));
    let up = |mut i: u64| {
        for _ in 0..4 {
            if is_bca(i) {
                return Some(i);
            }

            i = p.parent_of(i)?;
        }

        None
    };

    let selected = e.selection.items().iter().filter(|s| s.doc == doc).find_map(|s| up(s.item));
    let first = p.entities.iter().find(|x| matches!(p.kind(x), EntityKind::Bca(_))).map(|x| x.id);
    let bca = id.and_then(up).or(selected).or(first).ok_or("the map has no battle catchment area to launch at")?;
    let x = p.entity(bca).ok_or("no such entity")?;
    let EntityKind::Bca(kind) = p.kind(x) else { return Err("not a battle catchment area".to_owned()) };
    let pos = p.world_position(bca);
    let (w, h) = (f64::from(d.placed.width / 2), f64::from(d.placed.height / 2));
    let region = x.children.iter().filter_map(|&c| p.entity(c)).find(|c| matches!(p.kind(c), EntityKind::CatchmentRegion));
    let size = region.and_then(|r| p.rectangle(r)).or_else(|| p.rectangle(x)).unwrap_or([2048.0, 2048.0]);
    Ok(Spot {
        cell: (pos[0] / 256.0 + w, h - pos[2] / 256.0),
        kind,
        cells: ((size[0] / 256.0).round().max(1.0) as u32, (size[1] / 256.0).round().max(1.0) as u32),
        name: x.name.clone(),
    })
}

/// The battle's XML at a spot of a map `w` x `h` cells.
pub fn battle_xml(e: &mut Editor, map: &str, w: u32, h: u32, s: &Spot) -> String {
    let profile = e.game();
    let army = LaunchArmy::default().resolved(&profile.launch);
    let sea_surface = e.renderer().and_then(|r| {
        let wind = r.settings.sea_state.clone();
        r.sea_surface_of(&wind)
    });
    let weather = BattleWeather { environment: String::new(), season: "Summer", precipitation: "dry", time_of_day: "day", climate: String::new() };
    launch::battle_xml(&Battle {
        map: BattleMap::Position { map: format!("terrain/battles/{map}/"), x: s.cell.0 / f64::from(w.max(1)), y: s.cell.1 / f64::from(h.max(1)), settlement: s.kind.starts_with("settlement") },
        cells: s.cells, weather, sea_surface: sea_surface.unwrap_or_default(), army: &army, facts: &profile.launch, zones: &[],
    })
}

/// A launch made ready: the pack in `data\` and the command line; a dry run stops here (the pack written into the project's build folder).
pub struct Prepared {
    pub command: String,
    pub pack: PathBuf,
    pub xml: String,
}

/// Writes the launch pack (the export's files under `terrain/battles/<map>/`, the battle XML) and the command line.
///
/// # Errors
///
/// No game folder, the game running, Steam not running, stray movie-type packs (unless `ignore_movie_packs`), a launch pack the editor
/// did not write, the pack not written.
pub fn prepare(e: &mut Editor, x: &crate::export::Exported, w: u32, h: u32, s: &Spot, dry_run: bool, ignore_movie_packs: bool) -> Result<Prepared, String> {
    let profile = e.game();
    let game_dir = e.settings_ref().game_dir().ok_or("no game folder (setup <game folder>)")?;
    let data = game_dir.join(profile.data_dir);
    if !dry_run {
        let running = launch::processes_running(&[profile.exe, "steam.exe"]);
        if running.as_ref().is_some_and(|r| r[0]) {
            return Err(format!("{} is running already: close it first (the game refuses a second instance)", profile.exe));
        }

        if running.as_ref().is_some_and(|r| !r[1]) {
            return Err("Steam is not running: start it first".to_owned());
        }

        let stray = launch::stray_movie_packs(&data, &profile.release_pack_names(&game_dir));
        if !stray.is_empty() && !ignore_movie_packs {
            return Err(format!("movie-type packs in data\\ may shadow the mods' files: {}; move them out, or launch anyway (--ignore-movie-packs)", stray.join(", ")));
        }

        if let Some(m) = launch::remove_launch_pack(&data, profile.key)? {
            wce_foundation::log::info(format!("launch in game: {m} (left by an earlier launch)"));
        }
    }

    let xml = battle_xml(e, &x.map, w, h, s);
    let pack = if dry_run { e.build_dir().join(format!("dry_{LAUNCH_PACK}")) } else { data.join(LAUNCH_PACK) };
    if let Some(p) = pack.parent() {
        std::fs::create_dir_all(p).map_err(|err| format!("{}: {err}", p.display()))?;
    }

    launch::write_launch_pack(&pack, profile.key, crate::export::pack_files(&x.map, &x.files), &xml)?;
    let mods: Vec<String> = e.selected_mod_packs().into_iter().filter(|m| data.join(m).is_file()).collect();
    Ok(Prepared { command: launch::command_line(&Delivery::Pack, &mods), pack, xml })
}

/// Starts the game with a prepared launch.
///
/// # Errors
///
/// The game could not be started (the launch pack is removed then).
pub fn start(e: &Editor, p: &Prepared) -> Result<(Running, String), String> {
    let profile = e.game();
    let game_dir = e.settings_ref().game_dir().ok_or("no game folder")?;
    let data = game_dir.join(profile.data_dir);
    let staging = e.build_dir().join("launch").join("terrain");
    let _ = std::fs::create_dir_all(&staging);
    wce_foundation::log::info(format!("launch in game: {} {}", profile.exe, p.command));
    match launch::start(&game_dir, profile, &p.command, None, &staging) {
        Ok((child, started)) => Ok((Running { child, game: profile.key, data, staging }, started)),
        Err(err) => {
            let _ = launch::remove_launch_pack(&data, profile.key);
            Err(err)
        }
    }
}
