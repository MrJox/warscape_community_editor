//! Tile > Launch in game on the tool (`launch.rs` makes the pack, the battle and the command line): the checks before the game starts, the
//! question when stray movie-type packs are in `data\`, and the wait for the game to exit, after which the launch pack is removed.

use wce_editor::editor::Editor;
use wce_editor::{gui, theme};
use wce_editor_core::document::Document;
use wce_foundation::log;
use wce_tile_editor::tile_document::TileDocument;
use wce_vfs::GameProfile;

use crate::context_panel::GAME_MAP;
use crate::launch::{self, Battle, BattleMap, BattleWeather, LAUNCH_PACK, Running};
use crate::settings::{LaunchMap, SETTINGS_ID, TileSettings};
use crate::tool::{TileTool, document_id};

/// The staging folder under the output folder: the exported tile and a copy of the battle's XML (a dry run's pack too).
const STAGING: &str = "launch";

impl TileTool {
    /// Launch in game: exports the open tile, writes the launch pack and starts the game into a battle on it (docs/plans/launch_in_game.md).
    /// `dry_run`: everything but the game (the pack goes into the staging folder, the game's folder is not touched). `ask`: stray
    /// movie-type packs open the question instead of failing.
    ///
    /// # Errors
    ///
    /// No tile or game folder, a campaign tile, the game already running or Steam not running, stray movie-type packs (unless
    /// `ignore_movie_packs`), a launch pack the editor did not write, or the export, the pack or the start fails.
    pub fn launch_in_game(&mut self, e: &mut Editor, dry_run: bool, ignore_movie_packs: bool, ask: bool) -> Result<String, String> {
        let profile = GameProfile::ATTILA;
        let game_dir = e.settings_ref().game_dir().ok_or("no game folder (setup <game folder>)")?;
        let data = game_dir.join(profile.data_dir);
        let id = document_id(e).ok_or("no tile is open")?;
        let d = e.docs.find_as::<TileDocument>(id).ok_or("no tile is open")?;
        let info = d.project().info.clone();
        if self.weather.tile() != d.source_key() {
            // A tile opened this frame (a script): its weather is taken up now, not on the next update.
            self.weather.tile_changed(e, id);
        }

        let location = info.location.replace('\\', "/").to_lowercase();
        if !location.contains("terrain/tiles/battle/") {
            return Err("only a battle tile can be launched in a battle".to_owned());
        }

        if !dry_run {
            if self.launched.is_some() {
                return Err("the game launched before is still running".to_owned());
            }

            if launch::process_running(profile.exe) == Some(true) {
                return Err(format!("{} is running already: close it first (the game refuses a second instance)", profile.exe));
            }

            if launch::process_running("steam.exe") == Some(false) {
                return Err("Steam is not running: start it first".to_owned());
            }

            let stray = launch::stray_movie_packs(&data, &profile.release_pack_names(&game_dir));
            if !stray.is_empty() && !ignore_movie_packs {
                if ask {
                    self.launch_prompt = Some(stray.clone());
                    self.launch_prompt_open = true;
                }

                return Err(format!(
                    "movie-type packs in data\\ load whatever the mod list says and may shadow the mods' files: {}; move them out, or launch anyway \
                     (tile_launch_game --ignore-movie-packs)",
                    stray.join(", ")
                ));
            }

            // A pack left by a launch the editor did not see end.
            if let Some(m) = launch::remove_launch_pack(&data)? {
                log::info(format!("launch in game: {m} (left by an earlier launch)"));
            }
        }

        // The tile, exported into a fresh staging folder.
        let staging = e.output_dir().join(STAGING);
        let _ = std::fs::remove_dir_all(&staging);
        std::fs::create_dir_all(&staging).map_err(|err| format!("{}: {err}", staging.display()))?;
        let exported = self.export_to(e, &staging)?;
        let folder = wce_tile_io::attila::workflow::tile_folder(&exported.dir).ok_or_else(|| format!("{} is not under terrain/tiles/", exported.dir.display()))?;
        let files = match &exported.pack {
            Some(p) => wce_walls::pack::read_pack_files(p, profile.key)?,
            None => wce_walls::export::exported_pack_files(&staging, &folder)?,
        };

        // The battle.
        let settings = e.tool_settings::<TileSettings>(SETTINGS_ID).launch;
        let (map, note) = self.launch_map(&folder, &location, (info.cells_x, info.cells_y), settings.map);
        let battle = Battle { map, cells: (info.cells_x, info.cells_y), weather: BattleWeather::from_tile(self.weather.choice()), settings: &settings };
        let xml = launch::battle_xml(&battle);
        let xml_copy = staging.join("wce_launch.xml");
        std::fs::write(&xml_copy, &xml).map_err(|err| format!("{}: {err}", xml_copy.display()))?;

        let pack = if dry_run { staging.join(LAUNCH_PACK) } else { data.join(LAUNCH_PACK) };
        launch::write_launch_pack(&pack, files, &xml)?;

        let mods: Vec<String> = e.selected_mod_packs().into_iter().filter(|m| data.join(m).is_file()).collect();
        let command = launch::command_line(&mods);
        log::info(format!("launch in game: {} {command}", profile.exe));
        if !note.is_empty() {
            log::info(format!("launch in game: {note}"));
        }

        if dry_run {
            return Ok(format!("dry run: {} and {} written, the command line is in the log{note}", pack.display(), xml_copy.display()));
        }

        let child = match launch::start(&game_dir, profile.exe, &command) {
            Ok(c) => c,
            Err(err) => {
                let _ = launch::remove_launch_pack(&data);
                return Err(err);
            }
        };

        self.launched = Some(Running { child, data, staging });
        Ok(format!("launched {} into a battle on {folder} with {} mod pack(s); the launch pack is removed when the game exits{note}", profile.exe, mods.len()))
    }

    /// The battle's map: the tile alone, or (`LaunchMap::MapContext`) the Map context's placement in the game's campaign map; with a note
    /// when the placement can't be used (the kit's maps are not in the game's packs).
    fn launch_map(&self, folder: &str, location: &str, cells: (u32, u32), mode: LaunchMap) -> (BattleMap, String) {
        let tile = BattleMap::Tile(folder.to_owned());
        if mode == LaunchMap::Tile {
            return (tile, String::new());
        }

        let Some((name, map, _)) = &self.context.loaded else { return (tile, " (no map in the Map context: the tile alone)".to_owned()) };
        if name != GAME_MAP {
            return (tile, format!(" ({name} is a kit map, not in the game's packs: the tile alone)"));
        }

        let Some(spot) = self.context.spot() else { return (tile, " (no placement in the Map context: the tile alone)".to_owned()) };
        let (w, h) = if matches!(spot.rotation, 0x20 | 0x80) { (cells.1, cells.0) } else { cells };
        let x = (f64::from(spot.x) + f64::from(w) / 2.0) / f64::from(map.width.max(1));
        let y = (f64::from(spot.y) + f64::from(h) / 2.0) / f64::from(map.height.max(1));
        let set = location.split('/').nth(3).unwrap_or_default();
        (BattleMap::Position { map: "terrain/battles/main_attila_map/".to_owned(), x, y, settlement: set.starts_with("settlement") }, String::new())
    }

    /// Each frame: once, a launch pack left by an editor closed while the game ran is removed (when the game is not running); a launched
    /// game that exited has its launch pack and staging folder removed.
    pub(crate) fn update_launch(&mut self, e: &mut Editor) {
        if !self.launch_swept {
            self.launch_swept = true;
            if let Some(dir) = e.settings_ref().game_dir() {
                let data = dir.join(GameProfile::ATTILA.data_dir);
                if data.join(LAUNCH_PACK).is_file() && launch::process_running(GameProfile::ATTILA.exe) != Some(true) {
                    match launch::remove_launch_pack(&data) {
                        Ok(Some(m)) => log::info(format!("launch in game: {m} (left by an earlier launch)")),
                        Ok(None) => {}
                        Err(err) => log::warn(format!("launch in game: {err}")),
                    }
                }
            }
        }

        if let Some(msg) = self.launched.as_mut().and_then(Running::poll) {
            self.launched = None;
            log::info(format!("launch in game: {msg}"));
            e.status(&format!("Launch in game: {msg}"), false);
        }
    }

    /// The question when stray movie-type packs are in `data\`: launch anyway, or not.
    pub(crate) fn draw_launch_prompt(&mut self, e: &mut Editor) {
        const TITLE: &str = "Launch in game";
        if std::mem::take(&mut self.launch_prompt_open) {
            gui::open_popup(TITLE);
        }

        if !gui::begin_popup_modal(TITLE, None, gui::WINDOW_ALWAYS_AUTO_RESIZE) {
            return;
        }

        let Some(packs) = self.launch_prompt.clone() else {
            gui::close_current_popup();
            gui::end_popup();
            return;
        };

        gui::text("These movie-type packs are in the game's data folder:");
        for p in &packs {
            gui::text_disabled(&format!("  {p}"));
        }

        gui::text_colored(theme::warning(), "The game loads them whatever the mod list says; a stale one shadows the mods' files and can crash the battle.");
        gui::text("Move them out of data\\ to test the tile as the mods have it.");
        gui::separator();
        let mut close = false;
        if gui::button("Launch anyway") {
            let msg = self.launch_in_game(e, false, true, false).unwrap_or_else(|m| format!("launch failed: {m}"));
            e.status(&msg, false);
            close = true;
        }

        gui::same_line();
        if gui::button("Cancel") {
            close = true;
        }

        if close {
            self.launch_prompt = None;
            gui::close_current_popup();
        }

        gui::end_popup();
    }
}
