//! Launch in game (docs/plans/cinematic.md): the scene's historical battle pack (scripts, tables, loc) as the launch pack, the game
//! started straight into the battle from its own XML (`game_startup_mode battle script/<key>/<key>_battle.xml`: its start script
//! is named relative to it) with the mods chosen in the editor; the pack removed when the game exits. The Tile and Terrain tools'
//! launch (`wce_launch::launch`): the checks (the game not running, Steam running, stray movie-type packs), the command line, the
//! start, the clean-up.

use std::path::PathBuf;

use wce_editor::editor::Editor;
use wce_launch::launch::{self, BATTLE_XML, Delivery, LAUNCH_PACK, Running};

/// A launch made ready: the pack written and the command line; a dry run stops here (the pack in the project's build folder).
pub struct Prepared {
    pub command: String,
    pub pack: PathBuf,
    pub xml_path: String,
}

/// The battle's XML in the pack's files (`script/<key>/<key>_battle.xml`).
fn battle_xml_path(files: &[(String, Vec<u8>)]) -> Option<(String, String)> {
    files
        .iter()
        .find(|(p, _)| p.starts_with("script/") && p.ends_with("_battle.xml"))
        .map(|(p, b)| (p.clone(), String::from_utf8_lossy(b).into_owned()))
}

/// Writes the launch pack (the battle's files) and the command line.
///
/// # Errors
///
/// No game folder, the game running, Steam not running, stray movie-type packs (unless `ignore_movie_packs`), a launch pack the editor
/// did not write, no battle XML, the pack not written.
pub fn prepare(e: &mut Editor, files: Vec<(String, Vec<u8>)>, dry_run: bool, ignore_movie_packs: bool) -> Result<Prepared, String> {
    let profile = e.game();
    let game_dir = e.settings_ref().game_dir().ok_or("no game folder (Settings > Games)")?;
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
            return Err(format!(
                "movie-type packs in data\\ may shadow the mods' files: {}; move them out, or launch anyway",
                stray.join(", ")
            ));
        }
        if let Some(m) = launch::remove_launch_pack(&data, profile.key)? {
            wce_foundation::log::info(format!("launch in game: {m} (left by an earlier launch)"));
        }
    }
    let (xml_path, xml) = battle_xml_path(&files).ok_or("the battle has no XML")?;
    let pack = if dry_run { e.build_dir().join(format!("dry_cinematic_{LAUNCH_PACK}")) } else { data.join(LAUNCH_PACK) };
    if let Some(p) = pack.parent() {
        std::fs::create_dir_all(p).map_err(|err| format!("{}: {err}", p.display()))?;
    }
    launch::write_launch_pack(&pack, profile.key, files, &xml)?;
    let mods: Vec<String> = e.selected_mod_packs().into_iter().filter(|m| data.join(m).is_file()).collect();
    // From the battle's own XML: its start script is named relative to its folder.
    let command = launch::command_line(&Delivery::Pack, &mods).replace(BATTLE_XML, &xml_path);
    Ok(Prepared { command, pack, xml_path })
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
    let staging = e.build_dir().join("launch").join("cinematic");
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

#[cfg(test)]
mod tests {
    #[test]
    fn the_battle_starts_from_its_own_xml() {
        let files = vec![("script/x/x_start.lua".to_owned(), b"--".to_vec()), ("script/x/x_battle.xml".to_owned(), b"<battle/>".to_vec())];
        let (path, xml) = super::battle_xml_path(&files).unwrap();
        assert_eq!(path, "script/x/x_battle.xml");
        assert_eq!(xml, "<battle/>");
        let c = wce_launch::launch::command_line(&wce_launch::launch::Delivery::Pack, &["my_mod.pack".into()])
            .replace(wce_launch::launch::BATTLE_XML, &path);
        assert!(c.contains("game_startup_mode battle script/x/x_battle.xml;"), "{c}");
        assert!(c.starts_with("mod \"wce_launch.pack\"; mod \"my_mod.pack\";"), "{c}");
    }
}
