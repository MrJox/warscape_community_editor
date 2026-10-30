//! Tile > Launch in game on the tool (`launch.rs` makes the battle, finds the packs the tile needs, writes the files and the command line):
//! the checks before the game starts, the question when stray movie-type packs are in `data\`, the work on a thread with its progress
//! window (the steps, what each is doing, the time taken and left from the last launch's steps), and the wait for the game to exit, after
//! which the launch's files are removed.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, channel};
use std::time::Instant;

use wce_editor::editor::Editor;
use wce_editor::{gui, icons, theme};
use wce_editor_core::document::Document;
use wce_foundation::{activity, log};
use wce_tile_editor::tile_document::TileDocument;
use wce_tile_io::attila::workflow::Game;
use wce_vfs::{GameProfile, Vfs};

use crate::context_panel::GAME_MAP;
use crate::launch::{self, Battle, BattleMap, BattleWeather, Delivery, LAUNCH_PACK, Running};
use crate::settings::{LaunchArmy, LaunchDelivery, LaunchMap, LaunchSettings, SETTINGS_ID, TileSettings};
use crate::tool::{ExportInputs, TileTool, document_id};

/// The staging folder under the output folder: a subfolder per launch (`d…` for a dry run) with the exported tile, the loose files the
/// game reads (`game/`) or the launch pack, and a copy of the battle's XML. A launch's folder is removed when its game exits; the others
/// only when no game runs (a running game reads its loose files there).
const STAGING: &str = "launch";

/// The working directory of the loose files, in a launch's folder.
const LOOSE: &str = "game";

/// The tables whose rows a building or an army needs: a mod pack holding one of their keys loads with loose files.
const KEY_TABLES: [&str; 4] = ["models_building_tables", "main_units_tables", "land_units_tables", "factions_tables"];

/// The steps of a launch: key (for the timings kept in the settings) and what the window says.
const STEPS: [(&str, &str); 5] = [
    ("game", "Reading the game's tile data"),
    ("compile", "Compiling the tile (stages A, B and C)"),
    ("deps", "Finding the mod packs the tile needs"),
    ("write", "Writing the launch files"),
    ("start", "Starting the game"),
];
const GAME: usize = 0;
const COMPILE: usize = 1;
const DEPS: usize = 2;
const WRITE: usize = 3;
const START: usize = 4;

/// What the work sends back.
enum Msg {
    Step(usize),
    Game(Arc<Game>),
    Done(Result<Prepared, String>),
}

/// Everything the work needs, taken on the main thread.
struct Plan {
    inputs: ExportInputs,
    staging: PathBuf,
    cells: (u32, u32),
    weather: BattleWeather,

    /// The sea surface of the tile's battle wind (`Battle::sea_surface`).
    sea_surface: String,
    settings: LaunchSettings,

    /// The game launched, and its armies (every key given).
    game: &'static GameProfile,
    army: LaunchArmy,

    /// The battle map when it is a position in a map; `None`: the tile alone (its folder is known after the export).
    position: Option<BattleMap>,

    /// The editor's mounted files (the release, its mod packs, its loose folders), and its mod packs that are in `data\`.
    vfs: Option<Arc<Vfs>>,
    mods: Vec<String>,

    /// Where the launch pack goes for the game (`data\wce_launch.pack`; none in a dry run).
    copy_to: Option<PathBuf>,
}

/// What the work made.
struct Prepared {
    delivery: Delivery,

    /// The launch pack in the staging folder (pack delivery).
    pack: Option<PathBuf>,
    mods: Vec<String>,
    xml: PathBuf,
    folder: String,

    /// Why the delivery or the mods are what they are.
    notes: Vec<String>,
}

/// The work: the game's data, the export, the packs the tile needs, the launch's files; all in the staging folder (nothing in the game's
/// folder).
fn prepare(mut plan: Plan, progress: &dyn Fn(Msg)) -> Result<Prepared, String> {
    if plan.inputs.game.is_none() {
        progress(Msg::Step(GAME));
        wce_foundation::wce_prof!("launch.game_data");
        let _a = activity::start("Reading the packs' tile databases, buildings and models, and the kit's vegetation parameters");
        progress(Msg::Game(plan.inputs.game()?));
    }

    progress(Msg::Step(COMPILE));
    let walls = plan.inputs.has_walls();
    let (exported, _) = plan.inputs.run(&plan.staging)?;
    let folder = wce_tile_io::attila::workflow::tile_folder(&exported.dir).ok_or_else(|| format!("{} is not under terrain/tiles/", exported.dir.display()))?;
    let mut files = match &exported.pack {
        Some(p) => wce_walls::pack::read_pack_files(p, plan.game.key)?,
        None => wce_walls::export::exported_pack_files(&plan.staging, &folder)?,
    };

    let map = plan.position.take().unwrap_or_else(|| BattleMap::Tile(folder.clone()));
    let xml = launch::battle_xml(&Battle { map, cells: plan.cells, weather: plan.weather.clone(), sea_surface: plan.sea_surface.clone(), army: &plan.army, facts: &plan.game.launch });
    files.push((launch::BATTLE_XML.to_owned(), xml.as_bytes().to_vec()));

    // Loose files, with the mod packs their assets need; the pack when loose files would lose to one of them.
    let mut notes = vec![];
    let mut loose = plan.settings.delivery == LaunchDelivery::Loose;
    let mut mods = plan.mods.clone();
    let mut overlay = vec![];
    if loose && walls {
        notes.push("the tile's procedural walls carry database rows, which only a pack gives the game: the launch pack".to_owned());
        loose = false;
    }

    if loose && plan.vfs.is_none() {
        notes.push("no game files are mounted to find the packs the tile needs: the launch pack".to_owned());
        loose = false;
    }

    // A pack of the release always loads, and its file wins over a loose one (tiles.pack has the tile database): checked first, it needs
    // no scan of the dependencies.
    if loose
        && let Some(vfs) = plan.vfs.as_ref()
        && let Some((pack, path)) = launch::release_shadowing(vfs, &files)
    {
        notes.push(format!("{pack} also has {path} (a pack's file wins over a loose one): the launch pack"));
        loose = false;
    }

    // The launch pack takes every mod pack chosen in the editor: of the dependencies only the weather file can be an edit of the editor's
    // (its loose folders), carried along.
    if !loose
        && let Some(vfs) = plan.vfs.as_ref()
        && !plan.weather.environment.is_empty()
        && vfs.owner(&plan.weather.environment).is_some_and(|i| vfs.packs()[i].loose)
    {
        overlay.push(wce_vfs::normalize(&plan.weather.environment));
    }

    if loose && let Some(vfs) = plan.vfs.as_ref() {
        progress(Msg::Step(DEPS));
        wce_foundation::wce_prof!("launch.needs");
        let agf = std::fs::read_to_string(plan.staging.join(&folder).join("tile.agf")).unwrap_or_default();
        let buildings: Vec<String> = agf.lines().filter_map(|l| l.trim().strip_prefix("battle/buildings/")).map(str::to_owned).collect();
        let mut prefixes = vec!["terrain/textures/battle/default/".to_owned()];
        if !plan.weather.climate.is_empty() {
            let c = plan.weather.climate.to_lowercase();
            prefixes.push(format!("terrain/textures/battle/{c}/"));
            prefixes.push(format!("terrain/vegetation/battle/grass/{c}/"));
        }

        if !plan.weather.environment.is_empty() {
            prefixes.push(plan.weather.environment.clone());
        }

        let s = &plan.army;
        let mut keys = buildings.clone();
        keys.extend([&s.defender_faction, &s.defender_unit, &s.attacker_faction, &s.attacker_unit].map(|k| k.trim().to_owned()));
        let needs = launch::needs(vfs, &files, &buildings, &prefixes, &KEY_TABLES, &keys);
        log::info(format!(
            "launch in game: {} building(s), {} file(s) followed; mod packs the tile needs: {}",
            buildings.len(),
            needs.followed,
            if needs.mods.is_empty() { "none".to_owned() } else { needs.mods.iter().map(|(m, why)| format!("{m} ({why})")).collect::<Vec<_>>().join(", ") }
        ));

        overlay = needs.overlay.clone();
        if loose {
            if let Some((pack, path)) = needs.shadowing.first() {
                notes.push(format!("{pack} also has {path} (a pack's file wins over a loose one): the launch pack"));
                loose = false;
            } else {
                mods = needs.mods.iter().map(|(m, _)| m.clone()).filter(|m| plan.mods.iter().any(|x| x.eq_ignore_ascii_case(m))).collect();
                notes.push(format!("loose files with {} of the {} mod pack(s) chosen in the editor", mods.len(), plan.mods.len()));
            }
        }
    }

    // The editor's own edits the tile uses (a weather file saved in the output folder): carried along.
    if let Some(vfs) = plan.vfs.as_ref() {
        for path in &overlay {
            if let Ok(bytes) = vfs.read(path) {
                files.push((path.clone(), bytes));
            }
        }

        if !overlay.is_empty() {
            notes.push(format!("{} file(s) of the editor's loose folders carried along", overlay.len()));
        }
    }

    progress(Msg::Step(WRITE));
    wce_foundation::wce_prof!("launch.write");
    let xml_copy = plan.staging.join("wce_launch.xml");
    std::fs::write(&xml_copy, &xml).map_err(|err| format!("{}: {err}", xml_copy.display()))?;
    if loose {
        let _a = activity::start(format!("Writing {} loose file(s)", files.len()));
        let dir = plan.staging.join(LOOSE);
        for (path, bytes) in &files {
            let target = dir.join(path);
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent).map_err(|err| format!("{}: {err}", parent.display()))?;
            }

            std::fs::write(&target, bytes).map_err(|err| format!("{}: {err}", target.display()))?;
        }

        return Ok(Prepared { delivery: Delivery::Loose(dir), pack: None, mods, xml: xml_copy, folder, notes });
    }

    let _a = activity::start(format!("Writing the launch pack ({} file(s))", files.len()));
    let pack = plan.staging.join(LAUNCH_PACK);
    launch::write_launch_pack(&pack, plan.game.key, files, &xml)?;
    if let Some(target) = &plan.copy_to {
        let _a = activity::start("Copying the launch pack into the game's data folder");
        std::fs::copy(&pack, target).map_err(|err| format!("{}: {err}", target.display()))?;
    }

    Ok(Prepared { delivery: Delivery::Pack, pack: Some(pack), mods: plan.mods, xml: xml_copy, folder, notes })
}

/// A launch being prepared on a thread, and its window.
pub(crate) struct LaunchJob {
    rx: Receiver<Msg>,

    /// The game it launches (the editor may be on another one by the time it starts).
    game: &'static GameProfile,
    tile: String,
    dry_run: bool,
    loose: bool,
    game_dir: PathBuf,
    staging: PathBuf,
    note: String,

    /// When it started; the step going on and when it started; how long the steps took (`None`: not run, or skipped).
    started: Instant,
    step: usize,
    step_started: Instant,
    took: [Option<f32>; 5],

    /// The last launch's steps (seconds), for the time left.
    before: [Option<f32>; 5],
}

impl LaunchJob {
    /// Whether a step is part of this launch (reading the game's data only the first time; no game in a dry run).
    fn runs(&self, step: usize) -> bool {
        match step {
            GAME => self.took[GAME].is_some() || self.step == GAME,
            START => !self.dry_run,
            _ => true,
        }
    }

    /// The time left, from the last launch's steps (`None` without them).
    fn left(&self) -> Option<f32> {
        let current = self.step_started.elapsed().as_secs_f32();
        let mut left = 0.0;
        for i in (self.step..STEPS.len()).filter(|&i| self.runs(i)) {
            let before = self.before[i]?;
            left += if i == self.step { (before - current).max(0.0) } else { before };
        }

        Some(left)
    }
}

/// A launch's own folder name: `d` for a dry run, then the time in base 36 (short: the game reads the loose files below it).
fn launch_folder_name(dry_run: bool) -> String {
    let mut n = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis()) % 36u128.pow(7);
    let mut s = String::new();
    while n > 0 || s.is_empty() {
        s.insert(0, char::from_digit((n % 36) as u32, 36).unwrap_or('0'));
        n /= 36;
    }

    if dry_run { format!("d{s}") } else { s }
}

/// Removes the launch folders under `root`, all of them or only the dry runs'.
fn clear_staging(root: &Path, dry_runs_only: bool) {
    let Ok(dir) = std::fs::read_dir(root) else { return };
    for entry in dir.flatten() {
        if dry_runs_only && !entry.file_name().to_string_lossy().starts_with('d') {
            continue;
        }

        let _ = if entry.path().is_dir() { std::fs::remove_dir_all(entry.path()) } else { std::fs::remove_file(entry.path()) };
    }
}

impl TileTool {
    /// Launch in game: the checks, then the export and the launch's files on a thread with a progress window (`ask`: the menu; stray
    /// movie-type packs open the question instead of failing), then the game started into a battle on the tile (docs/plans/launch_in_game.md).
    /// A script's call (`!ask`) does the work at once and returns when the game is started. `dry_run`: everything but the game.
    ///
    /// # Errors
    ///
    /// No tile or game folder, a campaign tile, the game already running or Steam not running, stray movie-type packs (unless
    /// `ignore_movie_packs`), a launch pack the editor did not write, a cancelled launch still finishing, or the work or the start fails.
    pub fn launch_in_game(&mut self, e: &mut Editor, dry_run: bool, ignore_movie_packs: bool, ask: bool) -> Result<String, String> {
        if self.launch_job.is_some() {
            return Err("a launch is being prepared".to_owned());
        }

        if self.launch_worker.as_ref().is_some_and(|w| !w.is_finished()) {
            return Err("the cancelled launch is still finishing its work: try again in a moment".to_owned());
        }

        let profile = e.game();
        let game_dir = e.settings_ref().game_dir().ok_or("no game folder (setup <game folder>)")?;
        let data = game_dir.join(profile.data_dir);
        let id = document_id(e).ok_or("no tile is open")?;
        let d = e.docs.find_as::<TileDocument>(id).ok_or("no tile is open")?;
        let info = d.project().info.clone();
        let climate = d.tile().terrain_look().climate.clone();
        if self.weather.tile() != d.source_key() {
            // A tile opened this frame (a script): its weather is taken up now, not on the next update.
            self.weather.tile_changed(e, id);
        }

        let location = info.location.replace('\\', "/").to_lowercase();
        const BATTLE: &str = "terrain/tiles/battle/";
        let Some(at) = location.find(BATTLE) else { return Err("only a battle tile can be launched in a battle".to_owned()) };
        let tile = format!("{}/{}", &location[at + BATTLE.len()..], info.variation);

        if !dry_run {
            if self.launched.is_some() {
                return Err("the game launched before is still running".to_owned());
            }

            let running = launch::processes_running(&[profile.exe, "steam.exe"]);
            if running.as_ref().is_some_and(|r| r[0]) {
                return Err(format!("{} is running already: close it first (the game refuses a second instance)", profile.exe));
            }

            if running.as_ref().is_some_and(|r| !r[1]) {
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
            if let Some(m) = launch::remove_launch_pack(&data, profile.key)? {
                log::info(format!("launch in game: {m} (left by an earlier launch)"));
            }
        }

        // A folder of its own. The other launches' folders go when no game can be reading them (a real launch: the game is not running).
        let root = e.output_dir().join(STAGING);
        clear_staging(&root, dry_run);
        let staging = root.join(launch_folder_name(dry_run));
        std::fs::create_dir_all(&staging).map_err(|err| format!("{}: {err}", staging.display()))?;

        let tile_settings = e.tool_settings::<TileSettings>(SETTINGS_ID);
        let (settings, army) = (tile_settings.launch, tile_settings.launch_army.resolved(&profile.launch));
        let (position, mut note) = self.launch_position(profile, &location, (info.cells_x, info.cells_y), settings.map);
        if position.is_none() && !climate.is_empty() && !climate.eq_ignore_ascii_case(launch::ALONE_CLIMATE) {
            note.push_str(&launch::climate_alone_note(&climate));
        }

        let sea_surface = e.renderer().and_then(|r| {
            let wind = if tile_settings.sea_state.is_empty() { r.settings.sea_state.clone() } else { tile_settings.sea_state.clone() };
            r.sea_surface_of(&wind)
        });
        let mods: Vec<String> = e.selected_mod_packs().into_iter().filter(|m| data.join(m).is_file()).collect();
        let before = STEPS.map(|(k, _)| settings.step_seconds.get(k).copied());
        let loose = settings.delivery == LaunchDelivery::Loose;
        let plan = Plan {
            inputs: self.export_inputs(e)?, staging: staging.clone(), cells: (info.cells_x, info.cells_y), weather: BattleWeather::from_tile(self.weather.choice()),
            sea_surface: sea_surface.unwrap_or_default(), settings, game: profile, army, position, vfs: e.vfs().cloned(), mods, copy_to: (!dry_run).then(|| data.join(LAUNCH_PACK)),
        };

        activity::clear();
        let now = Instant::now();
        let first = if plan.inputs.game.is_none() { GAME } else { COMPILE };
        let mut job = LaunchJob {
            rx: channel().1, game: profile, tile: tile.clone(), dry_run, loose, game_dir, staging, note, started: now, step: first, step_started: now, took: [None; 5], before,
        };

        if !ask {
            // A script: at once, on this thread.
            let game = std::cell::RefCell::new(None);
            let prepared = prepare(plan, &|m| {
                if let Msg::Game(g) = m {
                    *game.borrow_mut() = Some(g);
                }
            });
            if let Some(g) = game.into_inner() {
                self.exported_game = Some(g);
            }

            return self.finish_launch(&mut job, prepared);
        }

        let (tx, rx) = channel();
        job.rx = rx;
        self.launch_worker = Some(std::thread::spawn(move || {
            let r = prepare(plan, &|m| {
                let _ = tx.send(m);
            });
            let _ = tx.send(Msg::Done(r));
        }));

        self.launch_job = Some(job);
        self.launch_progress_open = true;
        Ok(format!("preparing the launch of {tile}..."))
    }

    /// The prepared launch: a dry run's report, else (the pack copied into `data\`) the game started.
    fn finish_launch(&mut self, job: &mut LaunchJob, prepared: Result<Prepared, String>) -> Result<String, String> {
        let prepared = prepared?;
        let profile = job.game;
        let command = launch::command_line(&prepared.delivery, &prepared.mods);
        log::info(format!("launch in game: {} {command}", profile.exe));
        let mut notes = prepared.notes.join("; ");
        notes.push_str(&job.note);
        if !notes.is_empty() {
            log::info(format!("launch in game: {notes}"));
        }

        let how = match &prepared.delivery {
            Delivery::Loose(dir) => format!("loose files in {}", dir.display()),
            Delivery::Pack => format!("the launch pack {}", prepared.pack.as_ref().map_or(String::new(), |p| p.display().to_string())),
        };

        if job.dry_run {
            return Ok(format!("dry run: {how}, {} mod pack(s), {}; the command line is in the log", prepared.mods.len(), prepared.xml.display()));
        }

        // The launch pack is in data\ already (the work copied it).
        let data = job.game_dir.join(profile.data_dir);
        let child = match launch::start(&job.game_dir, profile.exe, &command) {
            Ok(c) => c,
            Err(err) => {
                let _ = launch::remove_launch_pack(&data, profile.key);
                return Err(err);
            }
        };

        self.launched = Some(Running { child, game: profile.key, data, staging: job.staging.clone() });
        Ok(format!(
            "launched {} into a battle on {} ({how}, {} mod pack(s)) in {:.0} s; its files are removed when the game exits",
            profile.exe,
            prepared.folder,
            prepared.mods.len(),
            job.started.elapsed().as_secs_f32()
        ))
    }

    /// The battle's map when it is a position (`LaunchMap::MapContext`: the Map context's placement in the game's campaign map), `None` for
    /// the tile alone; with a note when the placement can't be used (the kit's maps are not in the game's packs).
    fn launch_position(&self, game: &GameProfile, location: &str, cells: (u32, u32), mode: LaunchMap) -> (Option<BattleMap>, String) {
        let (map, mut note) = self.launch_map_position(game, location, cells, mode);
        if map.is_none() && launch::coastal_set(location.split('/').nth(3).unwrap_or_default()) {
            note.push_str(launch::COAST_ALONE_NOTE);
        }

        (map, note)
    }

    fn launch_map_position(&self, game: &GameProfile, location: &str, cells: (u32, u32), mode: LaunchMap) -> (Option<BattleMap>, String) {
        if mode == LaunchMap::Tile {
            return (None, String::new());
        }

        let Some((name, map, _)) = &self.context.loaded else { return (None, " (no map in the Map context: the tile alone)".to_owned()) };
        if name != GAME_MAP {
            return (None, format!(" ({name} is a kit map, not in the game's packs: the tile alone)"));
        }

        let Some(spot) = self.context.spot() else { return (None, " (no placement in the Map context: the tile alone)".to_owned()) };
        let (w, h) = if matches!(spot.rotation, 0x20 | 0x80) { (cells.1, cells.0) } else { cells };
        let x = (f64::from(spot.x) + f64::from(w) / 2.0) / f64::from(map.width.max(1));
        let y = (f64::from(spot.y) + f64::from(h) / 2.0) / f64::from(map.height.max(1));
        let set = location.split('/').nth(3).unwrap_or_default();
        (Some(BattleMap::Position { map: format!("{}/", game.campaign_battle_map), x, y, settlement: set.starts_with("settlement") }), String::new())
    }

    /// Each frame: once, what an editor closed while its game ran left (the launch pack, the launch folders) is removed when the game is
    /// not running; the launch being prepared moves on; a launched game that exited has its files removed.
    pub(crate) fn update_launch(&mut self, e: &mut Editor) {
        if !self.launch_swept {
            self.launch_swept = true;
            if let Some(dir) = e.settings_ref().game_dir() {
                let game = e.game();
                let data = dir.join(game.data_dir);
                let root = e.output_dir().join(STAGING);
                let left = data.join(LAUNCH_PACK).is_file() || std::fs::read_dir(&root).is_ok_and(|mut d| d.next().is_some());
                if left && launch::process_running(game.exe) != Some(true) {
                    clear_staging(&root, false);
                    match launch::remove_launch_pack(&data, game.key) {
                        Ok(Some(m)) => log::info(format!("launch in game: {m} (left by an earlier launch)")),
                        Ok(None) => {}
                        Err(err) => log::warn(format!("launch in game: {err}")),
                    }
                }
            }
        }

        self.preload_game(e);
        self.poll_launch_job(e);
        if let Some(msg) = self.launched.as_mut().and_then(Running::poll) {
            self.launched = None;
            log::info(format!("launch in game: {msg}"));
            e.status(&format!("Launch in game: {msg}"), false);
        }
    }

    /// Once a tile is open, reads the game's tile data on a thread (once a session: about 0.6 s), so that the first export or launch finds it
    /// read; takes it when it is.
    fn preload_game(&mut self, e: &mut Editor) {
        if !self.game_preload_tried
            && self.exported_game.is_none()
            && self.game_preload.is_none()
            && document_id(e).is_some()
            && let Some(files) = e.vfs().cloned()
        {
            self.game_preload_tried = true;
            let kit = crate::tool::tiles_kit(e);
            let (tx, rx) = channel();
            std::thread::spawn(move || {
                let r = Game::from_vfs(files, kit.as_deref()).map(|g| {
                    for w in &g.warnings {
                        log::warn(format!("exported tiles: {w}"));
                    }

                    Arc::new(g)
                });
                let _ = tx.send(r);
            });
            self.game_preload = Some(rx);
        }

        let Some(rx) = &self.game_preload else { return };
        match rx.try_recv() {
            Ok(Ok(g)) => {
                self.exported_game = Some(g);
                self.game_preload = None;
            }

            Ok(Err(err)) => {
                log::warn(format!("the game's tile data (for exports): {err}"));
                self.game_preload = None;
            }

            Err(std::sync::mpsc::TryRecvError::Disconnected) => self.game_preload = None,
            Err(std::sync::mpsc::TryRecvError::Empty) => {}
        }
    }

    /// Takes the work's messages: the steps (their times), the game's data (kept), the end (the game started, the times kept).
    fn poll_launch_job(&mut self, e: &mut Editor) {
        let Some(mut job) = self.launch_job.take() else { return };
        let mut done = None;
        while let Ok(m) = job.rx.try_recv() {
            match m {
                Msg::Step(i) if i != job.step => {
                    job.took[job.step] = Some(job.step_started.elapsed().as_secs_f32());
                    job.step = i;
                    job.step_started = Instant::now();
                }

                Msg::Step(_) => {}
                Msg::Game(g) => self.exported_game = Some(g),
                Msg::Done(r) => done = Some(r),
            }
        }

        let Some(r) = done else {
            self.launch_job = Some(job);
            return;
        };

        job.took[job.step] = Some(job.step_started.elapsed().as_secs_f32());
        let ok = r.is_ok();
        job.step = START;
        job.step_started = Instant::now();
        let msg = self.finish_launch(&mut job, r);
        if !job.dry_run {
            job.took[START] = Some(job.step_started.elapsed().as_secs_f32());
        }

        if ok && msg.is_ok() {
            let took = job.took;
            e.update_tool_settings(SETTINGS_ID, |s: &mut TileSettings| {
                for (i, (k, _)) in STEPS.iter().enumerate() {
                    if let Some(t) = took[i] {
                        s.launch.step_seconds.insert((*k).to_owned(), t);
                    }
                }
            });
        }

        let steps = STEPS.iter().zip(job.took).filter_map(|((k, _), t)| t.map(|t| format!("{k} {t:.1} s"))).collect::<Vec<_>>();
        log::info(format!("launch in game: steps: {}", steps.join(", ")));
        let msg = msg.unwrap_or_else(|m| format!("launch failed: {m}"));
        log::info(format!("launch in game: {msg}"));
        e.status(&msg, false);
    }

    /// The window while a launch is prepared: each step (done with its time, going on, to come), what the step going on is doing (the pieces
    /// going on, the last ones done), the time taken and left, Cancel.
    pub(crate) fn draw_launch_progress(&mut self, _e: &mut Editor) {
        const TITLE: &str = "Launching in game";
        if std::mem::take(&mut self.launch_progress_open) {
            gui::open_popup(TITLE);
        }

        if !gui::begin_popup_modal(TITLE, None, gui::WINDOW_ALWAYS_AUTO_RESIZE) {
            return;
        }

        let Some(job) = &self.launch_job else {
            gui::close_current_popup();
            gui::end_popup();
            return;
        };

        gui::text(&format!("{} in a battle", job.tile));
        let how = if job.loose { "loose files" } else { "the launch pack" };
        gui::text_disabled(&if job.dry_run { format!("dry run ({how}): the game is not started") } else { format!("{how}; the game starts when they are ready") });
        gui::separator();
        let line = gui::text_line_height();
        for (i, (_, label)) in STEPS.iter().enumerate().filter(|&(i, _)| job.runs(i)) {
            if i == DEPS && !job.loose && job.took[DEPS].is_none() && job.step != DEPS {
                continue;
            }

            if i < job.step {
                let t = job.took[i].map_or(String::new(), |t| format!("  {t:.1} s"));
                gui::text_colored(theme::success(), &format!("{}  {label}{t}", icons::CIRCLE_CHECK));
            } else if i == job.step {
                gui::spinner(line * 0.5, 2.0, theme::text());
                gui::same_line();
                let before = job.before[i].map_or(String::new(), |b| format!(" (about {b:.0} s last time)"));
                gui::text(&format!("{label}...  {:.1} s{before}", job.step_started.elapsed().as_secs_f32()));
                draw_activity(line);
            } else {
                gui::text_disabled(&format!("      {label}"));
            }
        }

        gui::spacing();
        let elapsed = job.started.elapsed().as_secs_f32();
        let (fraction, overlay) = match job.left() {
            Some(left) => (elapsed / (elapsed + left).max(0.001), if left > 0.5 { format!("{elapsed:.0} s, about {left:.0} s left") } else { format!("{elapsed:.0} s, almost done") }),
            None => {
                // No times yet: the steps done, and the current one creeping on (never quite full) so that the bar moves.
                let runs: Vec<usize> = (0..STEPS.len()).filter(|&i| job.runs(i)).collect();
                let done = runs.iter().filter(|&&i| i < job.step).count() as f32;
                let within = 1.0 - 1.0 / (1.0 + job.step_started.elapsed().as_secs_f32() / 4.0);
                ((done + within * 0.9) / runs.len() as f32, format!("{elapsed:.0} s (the first launch: the time is learned for the next ones)"))
            }
        };

        gui::progress_bar(fraction, 520.0, &overlay);
        gui::separator();
        if gui::button("Cancel") {
            // The work ends on its own in its staging folder; nothing is in the game's folder yet.
            log::info("launch in game: cancelled");
            self.launch_job = None;
            gui::close_current_popup();
        }

        gui::end_popup();
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
        let mut launch = false;
        if gui::button("Launch anyway") {
            launch = true;
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
        if launch {
            let msg = self.launch_in_game(e, false, true, true).unwrap_or_else(|m| format!("launch failed: {m}"));
            e.status(&msg, false);
        }
    }
}

/// What the step going on is doing: the pieces going on (several at once on the thread pool), then the last ones done with their times.
fn draw_activity(line: f32) {
    const RECENT: usize = 6;
    let s = activity::snapshot();
    let indent = line * 1.6;
    for (label, t) in &s.active {
        gui::set_cursor_pos_x(indent);
        gui::spinner(line * 0.35, 1.5, theme::dim());
        gui::same_line();
        gui::text_colored(theme::dim(), &format!("{label}  {t:.1} s"));
    }

    for (label, t) in s.done.iter().rev().take(RECENT) {
        gui::set_cursor_pos_x(indent);
        gui::text_disabled(&format!("{}  {label}  {t:.1} s", icons::CHECK));
    }

    if s.done.len() > RECENT {
        gui::set_cursor_pos_x(indent);
        gui::text_disabled(&format!("    and {} more done", s.done.len() - RECENT));
    }
}
