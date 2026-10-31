//! Machine settings: `settings.toml` in the system's folder for program settings, with environment overrides, and the folders for caches and
//! logs ([`Paths`]). Nothing about the machine lives in code; the first-run dialog and the Settings window write the file.
//!
//! The file has the editor's own values at the top level, a `[games.<id>]` table per game (its folder, mods and loose folders) and a
//! `[tools.<id>]` table per tool, which the tool reads and writes with its own type ([`Settings::tool`]). An environment variable wins over
//! the file and is never written back.

mod legacy;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::game::GameProfile;
use crate::log;

/// The settings file's name in the config folder.
pub const FILE_NAME: &str = "settings.toml";

/// The frame cap of a new install.
pub const DEFAULT_FRAME_CAP: u32 = 60;

/// The version of the file's layout, for migrations.
const VERSION: u32 = 1;

/// An environment variable, if set and not empty.
pub fn env_value(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|value| !value.is_empty())
}

/// rpfm's schemas folder: its config folder (`%APPDATA%\FrodoWazEre\rpfm\config` on Windows, `~/.config/rpfm` on Linux,
/// `~/Library/Application Support/com.FrodoWazEre.rpfm` on macOS) or the older `<config>/rpfm` layout, then `schemas`: the first of them
/// that is there.
pub fn default_rpfm_schemas_dir() -> Option<PathBuf> {
    let var = |name: &str| std::env::var_os(name).map(PathBuf::from);
    let home = var("HOME").or_else(|| var("USERPROFILE"));
    let config = var("XDG_CONFIG_HOME").or_else(|| home.as_ref().map(|h| h.join(".config")));
    let mut folders = vec![];
    if let Some(appdata) = var("APPDATA") {
        folders.push(appdata.join("FrodoWazEre/rpfm/config"));
        folders.push(appdata.join("rpfm"));
    }

    folders.extend(config.map(|c| c.join("rpfm")));
    folders.extend(home.map(|h| h.join("Library/Application Support/com.FrodoWazEre.rpfm")));
    folders.into_iter().map(|f| f.join("schemas")).find(|p| p.is_dir())
}

/// rpfm's schema of a game where rpfm keeps it (`<rpfm's schemas folder>/<the game's schema>`, like `schema_att.ron`), if it is there.
pub fn default_rpfm_schema_path(game: &crate::game::GameProfile) -> Option<PathBuf> {
    default_rpfm_schemas_dir().map(|dir| dir.join(game.rpfm_schema)).filter(|p| p.is_file())
}

/// The DB schemas folder of the settings ([`Settings::db_schemas`]), for [`rpfm_schema_path`]: [`Settings::load`] and the editor's
/// Settings window put it here, so that the readers of db tables need no settings.
static DB_SCHEMAS: std::sync::RwLock<Option<PathBuf>> = std::sync::RwLock::new(None);

/// The DB schemas folder: `$WCE_DB_SCHEMAS`, else the settings' (the last [`Settings::load`] or [`Settings::publish_db_schemas`]).
pub fn db_schemas_dir() -> Option<PathBuf> {
    env_value("WCE_DB_SCHEMAS").map(PathBuf::from).or_else(|| DB_SCHEMAS.read().ok().and_then(|d| d.clone()))
}

/// rpfm's schema of a game's db tables: `$WCE_RPFM_SCHEMA` (a file), else the game's schema (like `schema_att.ron`) in the DB schemas
/// folder ([`db_schemas_dir`]), else in rpfm's own ([`default_rpfm_schema_path`]). None when none of them has it.
pub fn rpfm_schema_path(game: &crate::game::GameProfile) -> Option<PathBuf> {
    rpfm_schema_path_in(game, db_schemas_dir().as_deref())
}

/// Like [`rpfm_schema_path`], with `schemas` as the DB schemas folder.
pub fn rpfm_schema_path_in(game: &crate::game::GameProfile, schemas: Option<&Path>) -> Option<PathBuf> {
    env_value("WCE_RPFM_SCHEMA")
        .map(PathBuf::from)
        .or_else(|| schemas.map(|dir| dir.join(game.rpfm_schema)).filter(|p| p.is_file()))
        .or_else(|| default_rpfm_schema_path(game))
}

/// A `;` separated list from the environment.
fn env_list(name: &str) -> Option<Vec<String>> {
    env_value(name).map(|v| v.split(';').map(str::trim).filter(|s| !s.is_empty()).map(str::to_owned).collect())
}

/// Where the editor keeps its files.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Paths {
    config: PathBuf,
    data: PathBuf,
    cache: PathBuf,
    logs: PathBuf,
}

impl Paths {
    /// The system's folders for an app called "wce" (Linux: `~/.config/wce`, `~/.local/share/wce`, `~/.cache/wce`, `~/.local/state/wce`;
    /// Windows: `%LOCALAPPDATA%\wce\config`, `...\data`, `...\cache`; macOS: `~/Library/Application Support/wce`, `~/Library/Caches/wce`), or
    /// everything under `$WCE_SETTINGS_DIR` (a portable or throw-away setup, see [`Self::in_dir`]). `$WCE_CACHE_DIR` moves the caches.
    pub fn standard() -> Self {
        let mut paths = match env_value("WCE_SETTINGS_DIR") {
            Some(dir) => Self::in_dir(Path::new(&dir)),
            None => match directories::ProjectDirs::from("", "", "wce") {
                Some(p) => Self {
                    config: p.config_local_dir().to_path_buf(),
                    data: p.data_local_dir().to_path_buf(),
                    cache: p.cache_dir().to_path_buf(),
                    logs: p.state_dir().unwrap_or_else(|| p.data_local_dir()).to_path_buf(),
                },

                // No home folder.
                None => Self::in_dir(&std::env::temp_dir().join("wce")),
            },
        };

        if let Some(dir) = env_value("WCE_CACHE_DIR") {
            paths.cache = PathBuf::from(dir);
        }

        paths
    }

    /// Everything in one folder: the settings, the user's files and the logs in it, the caches in its `cache` folder.
    pub fn in_dir(dir: &Path) -> Self {
        Self { config: dir.to_path_buf(), data: dir.to_path_buf(), cache: dir.join("cache"), logs: dir.to_path_buf() }
    }

    /// The folder of settings.toml and the tools' layouts.
    pub fn config_dir(&self) -> &Path {
        &self.config
    }

    /// The user's files the editor makes (the Tile tool's workspace of decompiled tiles): not to be deleted like a cache.
    pub fn data_dir(&self) -> &Path {
        &self.data
    }

    /// Files the editor can rebuild (indices, the map context's terrain).
    pub fn cache_dir(&self) -> &Path {
        &self.cache
    }

    /// The last run's log.
    pub fn log_dir(&self) -> &Path {
        &self.logs
    }

    /// The dock layout of each tool (`<tool id>.ini`).
    pub fn layouts_dir(&self) -> PathBuf {
        self.config.join("layouts")
    }

    pub fn settings_file(&self) -> PathBuf {
        self.config.join(FILE_NAME)
    }
}

/// What a game needs: where it is installed and what is read on top of its own packs.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GameSettings {
    /// The installation folder (the one with `data`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dir: Option<PathBuf>,

    /// Mod pack file names in the data folder, read on top of the game's release packs.
    pub mod_packs: Vec<String>,

    /// The Assembly Kits (each the folder that holds `raw_data`): files kept as TEd and BOB read them, read by the tools and written only by
    /// "Save for Assembly Kit"; the first one wins where a tile or a prefab is in several. None: the kit found in the game folder.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub assembly_kits: Vec<PathBuf>,

    /// TODO(pre-release): the old single Assembly Kit, read only to move it into `assembly_kits` (`Settings::load`).
    #[serde(skip_serializing)]
    pub assembly_kit: Option<PathBuf>,

    /// The game's projects: `assembly_kit/` (the editor's own files in the kit's layout, like the tiles' `tile.wce`) and `loose/` (loose files
    /// read before every pack: the UI tool's skins, layouts and pictures); none: `<data folder>/projects/<game>`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub projects: Option<PathBuf>,

    /// TODO(pre-release): the old project folder, read only to move it into `overlay_folders` (`Settings::load`); `loose` replaces it.
    #[serde(skip_serializing)]
    pub project_dir: Option<PathBuf>,

    /// Loose folders read before every pack, after the project folder, highest priority first.
    pub overlay_folders: Vec<PathBuf>,

    /// Where "Save" writes loose files (under their in-pack path); none: the editor's default for the game. Per game: the files saved there
    /// are read back as the game's own (a weather file of one game must not show in another).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_dir: Option<PathBuf>,

    /// The renderer's graphics options for this game: the game's own (its preferences file) or the editor's, and the editor's render
    /// scale.
    #[serde(skip_serializing_if = "GraphicsSettings::is_default")]
    pub graphics: GraphicsSettings,

    /// The tools' fields that are about this game's files (recent tiles, a tile's weather...): `[games.<id>.tools.<tool>]`. The rest of a
    /// tool's table is the editor's, in `[tools.<tool>]`; see [`Settings::set_game_fields`].
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub tools: BTreeMap<String, toml::Table>,
}

/// A game's graphics options in the editor (`crate::graphics`): the game's own, or a copy of them the editor keeps, and what only the
/// editor has.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GraphicsSettings {
    /// The game's options (its `preferences.script.txt`), as the game draws; off: [`Self::options`].
    pub use_game: bool,

    /// The editor's options (`gfx_*` keys, values as the preferences file writes them), when `use_game` is off: made from the game's file
    /// when it is turned off. An option missing here takes the game's value.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub options: BTreeMap<String, String>,

    /// Editor only (no game has it): the scene is drawn at this fraction of the viewport's size (0.5 .. 1) and enlarged with NVIDIA Image
    /// Scaling; 1 draws it at full size.
    pub render_scale: f32,

    /// NVIDIA Image Scaling's sharpening, 0 .. 1, when the render scale is below 1.
    pub sharpness: f32,
}

impl Default for GraphicsSettings {
    fn default() -> Self {
        Self { use_game: true, options: BTreeMap::new(), render_scale: 1.0, sharpness: 0.5 }
    }
}

impl GraphicsSettings {
    fn is_default(&self) -> bool {
        *self == Self::default()
    }

    /// The render scale in its range.
    pub fn render_scale(&self) -> f32 {
        if self.render_scale.is_finite() { self.render_scale.clamp(0.5, 1.0) } else { 1.0 }
    }
}

/// How the editor looks.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Appearance {
    /// A theme's id (the editor lists them).
    pub theme: String,

    /// The text size in points, before the display's scale.
    pub font_size: f32,

    /// The viewport gizmo's handles, in points before the display's scale: the same on screen whatever the viewport's size, the camera's
    /// distance or the object's size.
    pub gizmo_size: f32,
}

/// [`Appearance::gizmo_size`]'s default: about what the gizmo had as a fixed part (0.16 of the clip space) of a 715-point viewport.
pub const DEFAULT_GIZMO_SIZE: f32 = 60.0;

impl Default for Appearance {
    fn default() -> Self {
        Self { theme: "dark".to_owned(), font_size: 15.0, gizmo_size: DEFAULT_GIZMO_SIZE }
    }
}

/// The viewport's camera.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CameraSettings {
    /// The camera last used: `orbit`, `total_war`, `top_down`, `walk` or `fly`.
    pub mode: String,

    /// 0 (off) .. 1.
    pub smoothing: f32,

    /// Moving the mouse to the viewport's edge moves the camera.
    pub edge_scroll: bool,

    /// The game's battle camera height limits.
    pub game_limits: bool,

    /// The eye stays `ground_clearance` metres above the terrain.
    pub respect_ground: bool,
    pub ground_clearance: f32,

    /// The eye stays out of the models' bounding boxes.
    pub respect_collisions: bool,
}

impl Default for CameraSettings {
    fn default() -> Self {
        Self {
            mode: "orbit".to_owned(), smoothing: 0.25, edge_scroll: false, game_limits: false, respect_ground: true, ground_clearance: 1.0,
            respect_collisions: false,
        }
    }
}

/// The self-updater (Help > Check for updates).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct UpdateSettings {
    /// An interactive session looks for a newer release when it starts.
    pub check_on_start: bool,

    /// Pre-releases (betas) count as updates.
    pub prereleases: bool,
}

impl Default for UpdateSettings {
    fn default() -> Self {
        Self { check_on_start: true, prereleases: false }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// The file layout's version.
    pub version: u32,

    /// The game being edited: a key of `games`.
    pub game: String,

    /// Frames per second an interactive session is held to (0: no limit).
    pub frame_cap: u32,

    /// The tool an interactive session was last in (its id): the next one starts in it.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub last_tool: String,

    /// TODO(pre-release): the old output folder shared by every game, read only to move it to Attila's table (`Settings::load`).
    #[serde(skip_serializing)]
    pub output_dir: Option<PathBuf>,

    /// The folder of the db tables' schemas (rpfm's files: `schema_att.ron`, `schema_tob.ron`...), every game's; none, or a game's file
    /// missing from it: rpfm's own schemas folder ([`rpfm_schema_path`]).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub db_schemas: Option<PathBuf>,

    pub appearance: Appearance,
    pub camera: CameraSettings,
    pub updates: UpdateSettings,
    pub games: BTreeMap<String, GameSettings>,

    /// Each tool's own table, read and written with the tool's type (see [`Self::tool`]).
    pub tools: BTreeMap<String, toml::Table>,

    #[serde(skip)]
    paths: Paths,

    /// Per tool, the fields of its table kept per game (the tools declare them when added).
    #[serde(skip)]
    game_fields: BTreeMap<String, &'static [&'static str]>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            version: VERSION, game: "attila".to_owned(), frame_cap: DEFAULT_FRAME_CAP, last_tool: String::new(), output_dir: None, db_schemas: None, appearance: Appearance::default(),
            camera: CameraSettings::default(), updates: UpdateSettings::default(), games: BTreeMap::new(), tools: BTreeMap::new(), paths: Paths::default(), game_fields: BTreeMap::new(),
        }
    }
}

impl Settings {

    /// The settings in the system's folders ([`Paths::standard`]).
    pub fn load_default() -> Self {
        Self::load(Paths::standard())
    }

    /// Reads settings.toml. No file gives the defaults (or what the old wce.ini had); a file that can't be read is kept aside as
    /// `settings.toml.bad` and gives the defaults.
    pub fn load(paths: Paths) -> Self {
        let file = paths.settings_file();
        let mut settings = match std::fs::read_to_string(&file) {
            Ok(text) => match toml::from_str::<Self>(&text) {
                Ok(s) => s,
                Err(e) => {
                    let bad = file.with_extension("toml.bad");
                    log::error(format!("{} can't be read, using the defaults (the file is kept as {}): {e}", file.display(), bad.display()));
                    let _ = std::fs::rename(&file, &bad);
                    Self::default()
                }
            },

            Err(_) => {
                // TODO(pre-release): remove with the `legacy` module.
                match legacy::migrate(&paths) {
                    Some(s) => {
                        let mut s = s;
                        s.paths = paths.clone();
                        match s.save() {
                            Ok(()) => log::info(format!("Settings moved from the old wce.ini to {}", file.display())),
                            Err(e) => log::error(format!("Cannot write {}: {e}", file.display())),
                        }

                        s
                    }

                    None => Self::default(),
                }
            }
        };

        // TODO(pre-release): the old shared output folder was Attila's (the only game then); the other games get their own default.
        if let Some(old) = settings.output_dir.take() {
            let attila = settings.games.entry(GameProfile::ATTILA.key.to_owned()).or_default();
            if attila.output_dir.is_none() {
                log::info(format!("the output folder {} is Attila's now; every game has its own", old.display()));
                attila.output_dir = Some(old);
            }
        }

        // TODO(pre-release): the old per-game project folder becomes the first overlay folder (its files stay read); `loose` replaces it.
        for (game, g) in &mut settings.games {
            if let Some(old) = g.project_dir.take()
                && !g.overlay_folders.contains(&old)
            {
                log::info(format!("{game}: the old project folder {} is read as an overlay folder now; the projects folder's loose/ replaces it", old.display()));
                g.overlay_folders.insert(0, old);
            }
        }

        // TODO(pre-release): the old single Assembly Kit becomes the first of the list.
        for g in settings.games.values_mut() {
            if let Some(old) = g.assembly_kit.take()
                && !g.assembly_kits.contains(&old)
            {
                g.assembly_kits.insert(0, old);
            }
        }

        // TODO(pre-release): the Tile tool's Attila schema file becomes the DB schemas folder (where it is, if it has rpfm's file name).
        if let Some(old) = settings.tools.get_mut("tile").and_then(|t| t.remove("rpfm_schema")).and_then(|v| v.as_str().map(PathBuf::from))
            && settings.db_schemas.is_none()
        {
            if old.file_name().is_some_and(|n| n == GameProfile::ATTILA.rpfm_schema) {
                log::info(format!("the Tile settings' rpfm schema {} is read from the DB schemas folder now", old.display()));
                settings.db_schemas = old.parent().map(Path::to_path_buf);
            } else {
                log::warn(format!("the Tile settings' rpfm schema {} is not used any more: set the DB schemas folder (Settings > General)", old.display()));
            }
        }

        settings.paths = paths;
        settings.publish_db_schemas();
        settings
    }

    /// Makes the settings' DB schemas folder the one the db tables are read with ([`rpfm_schema_path`]).
    pub fn publish_db_schemas(&self) {
        if let Ok(mut dir) = DB_SCHEMAS.write() {
            dir.clone_from(&self.db_schemas);
        }
    }

    /// Writes settings.toml: to a temporary file first, then over the old one, so a crash never leaves half a file.
    ///
    /// # Errors
    ///
    /// Fails if the folder or the file can't be written.
    pub fn save(&self) -> std::io::Result<()> {
        let file = self.paths.settings_file();
        std::fs::create_dir_all(self.paths.config_dir())?;
        let text = toml::to_string(self).map_err(std::io::Error::other)?;
        let tmp = file.with_extension("toml.tmp");
        std::fs::write(&tmp, text)?;
        std::fs::rename(&tmp, &file)
    }

    pub fn paths(&self) -> &Paths {
        &self.paths
    }

    pub fn file(&self) -> PathBuf {
        self.paths.settings_file()
    }

    // ---- the current game, with the environment's overrides

    /// The current game's profile (Attila for a key the editor doesn't support).
    pub fn game_profile(&self) -> &'static GameProfile {
        GameProfile::by_key_or_default(&self.game)
    }

    /// The current game's table, created if missing.
    pub fn game_mut(&mut self) -> &mut GameSettings {
        self.games.entry(self.game.clone()).or_default()
    }

    pub fn game_settings(&self) -> GameSettings {
        self.games.get(&self.game).cloned().unwrap_or_default()
    }

    /// The game folder: `$WCE_GAME`, else the file's.
    pub fn game_dir(&self) -> Option<PathBuf> {
        env_value("WCE_GAME").map(PathBuf::from).or_else(|| self.games.get(&self.game)?.dir.clone())
    }

    /// The mod packs: `$WCE_MOD_PACKS` (`;` separated), else the file's.
    pub fn mod_packs(&self) -> Vec<String> {
        env_list("WCE_MOD_PACKS").unwrap_or_else(|| self.game_settings().mod_packs)
    }

    /// The loose folders: `$WCE_OVERLAY` (`;` separated), else the file's.
    pub fn overlay_folders(&self) -> Vec<PathBuf> {
        env_list("WCE_OVERLAY").map_or_else(|| self.game_settings().overlay_folders, |v| v.into_iter().map(PathBuf::from).collect())
    }

    /// The kit in the game folder (`assembly_kit`, or the battle tile map kit of a development install), if there is one.
    pub fn detected_assembly_kit(&self) -> Option<PathBuf> {
        let game = self.game_dir()?;
        ["assembly_kit", "assembly_kit_battle_tilemap"].iter().map(|k| game.join(k)).find(|k| k.join("raw_data").is_dir())
    }

    /// The Assembly Kits, the first one wins: `$WCE_ASSEMBLY_KIT` (`;` separated), else the file's, else the kit in the game folder
    /// ([`Self::detected_assembly_kit`]). Empty: no kit.
    pub fn assembly_kit_dirs(&self) -> Vec<PathBuf> {
        let listed = env_list("WCE_ASSEMBLY_KIT").map_or_else(|| self.game_settings().assembly_kits, |v| v.into_iter().map(PathBuf::from).collect());
        if listed.is_empty() { self.detected_assembly_kit().into_iter().collect() } else { listed }
    }

    /// The `raw_data` folders of the Assembly Kits ([`Self::assembly_kit_dirs`]) that have one, in their order. A development install's
    /// battle tile map kit brings the campaign tile map kit beside it.
    pub fn assembly_kit_raw_dirs(&self) -> Vec<PathBuf> {
        let mut kits = vec![];
        for kit in self.assembly_kit_dirs() {
            let campaign = kit.file_name().is_some_and(|n| n == "assembly_kit_battle_tilemap").then(|| kit.with_file_name("assembly_kit_campaign_tilemap"));
            kits.push(kit);
            kits.extend(campaign);
        }

        let mut raws: Vec<PathBuf> = vec![];
        for raw in kits.into_iter().map(|k| k.join("raw_data")).filter(|raw| raw.is_dir()) {
            if !raws.contains(&raw) {
                raws.push(raw);
            }
        }

        raws
    }

    /// The game's projects folder: `$WCE_PROJECTS`, else the file's, else `<data folder>/projects/<game>`.
    pub fn projects_dir(&self) -> PathBuf {
        env_value("WCE_PROJECTS").map(PathBuf::from).or_else(|| self.game_settings().projects).unwrap_or_else(|| self.paths.data_dir().join("projects").join(&self.game))
    }

    /// The projects' loose files, read before every pack.
    pub fn loose_dir(&self) -> PathBuf {
        self.projects_dir().join("loose")
    }

    /// The projects in the Assembly Kit's layout (`<this>/raw_data/terrain/tiles/...`).
    pub fn kit_projects_dir(&self) -> PathBuf {
        self.projects_dir().join("assembly_kit")
    }

    /// The current game's output folder: `$WCE_OUTPUT_DIR`, else the file's.
    pub fn output_dir(&self) -> Option<PathBuf> {
        env_value("WCE_OUTPUT_DIR").map(PathBuf::from).or_else(|| self.game_settings().output_dir)
    }

    /// The DB schemas folder: `$WCE_DB_SCHEMAS`, else the file's.
    pub fn db_schemas_dir(&self) -> Option<PathBuf> {
        env_value("WCE_DB_SCHEMAS").map(PathBuf::from).or_else(|| self.db_schemas.clone())
    }

    /// `$WCE_FRAME_CAP`, else the file's.
    pub fn frame_cap(&self) -> u32 {
        env_value("WCE_FRAME_CAP").and_then(|v| v.trim().parse().ok()).unwrap_or(self.frame_cap)
    }

    // ---- the tools' tables

    /// Declares which fields of a tool's table are about the current game's files: they are read from and written to the game's table
    /// (`[games.<id>.tools.<tool>]`), so that each game keeps its own.
    pub fn set_game_fields(&mut self, id: &str, fields: &'static [&'static str]) {
        self.game_fields.insert(id.to_owned(), fields);
    }

    fn game_fields_of(&self, id: &str) -> &'static [&'static str] {
        self.game_fields.get(id).copied().unwrap_or(&[])
    }

    /// A tool's settings: its table with the current game's fields over it, read as `T`. A field the game has no value for yet takes the
    /// one from before the fields were per game (the shared table). A missing table, or one that does not read as `T`, gives `T`'s defaults.
    pub fn tool<T: DeserializeOwned + Default>(&self, id: &str) -> T {
        let mut table = self.tools.get(id).cloned().unwrap_or_default();
        if let Some(own) = self.games.get(&self.game).and_then(|g| g.tools.get(id)) {
            for (k, v) in own {
                table.insert(k.clone(), v.clone());
            }
        }

        table.try_into().unwrap_or_else(|e| {
            log::warn(format!("settings: [tools.{id}] does not read ({e}), using its defaults"));
            T::default()
        })
    }

    /// Replaces a tool's table with `value`: its game fields go to the current game's table, the rest to the shared one.
    pub fn set_tool<T: Serialize>(&mut self, id: &str, value: &T) {
        let mut table = match toml::Table::try_from(value) {
            Ok(table) => table,
            Err(e) => {
                log::error(format!("settings: [tools.{id}] can't be written: {e}"));
                return;
            }
        };

        let fields = self.game_fields_of(id);
        if !fields.is_empty() {
            // The values from before the fields were per game: every game that has none yet keeps them, once.
            if let Some(shared) = self.tools.get_mut(id) {
                let old: Vec<(String, toml::Value)> = fields.iter().filter_map(|f| shared.remove(*f).map(|v| ((*f).to_owned(), v))).collect();
                if !old.is_empty() {
                    let current = self.game.clone();
                    for (key, g) in &mut self.games {
                        if *key == current {
                            continue;
                        }

                        let own = g.tools.entry(id.to_owned()).or_default();
                        for (k, v) in &old {
                            own.entry(k.clone()).or_insert_with(|| v.clone());
                        }
                    }
                }
            }

            let own = self.game_mut().tools.entry(id.to_owned()).or_default();
            for f in fields {
                if let Some(v) = table.remove(*f) {
                    own.insert((*f).to_owned(), v);
                }
            }
        }

        self.tools.insert(id.to_owned(), table);
    }
}
