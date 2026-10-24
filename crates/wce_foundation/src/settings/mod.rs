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

/// A `;` separated list from the environment.
fn env_list(name: &str) -> Option<Vec<String>> {
    env_value(name).map(|v| v.split(';').map(str::trim).filter(|s| !s.is_empty()).map(str::to_owned).collect())
}

/// Where the editor keeps its files.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Paths {
    config: PathBuf,
    cache: PathBuf,
    logs: PathBuf,
}

impl Paths {
    /// The system's folders for an app called "wce" (Linux: `~/.config/wce`, `~/.cache/wce`, `~/.local/state/wce`; Windows:
    /// `%LOCALAPPDATA%\wce\config`, `...\cache`, `...\data`; macOS: `~/Library/Application Support/wce`, `~/Library/Caches/wce`), or
    /// everything under `$WCE_SETTINGS_DIR` (a portable or throw-away setup, see [`Self::in_dir`]). `$WCE_CACHE_DIR` moves the caches.
    pub fn standard() -> Self {
        let mut paths = match env_value("WCE_SETTINGS_DIR") {
            Some(dir) => Self::in_dir(Path::new(&dir)),
            None => match directories::ProjectDirs::from("", "", "wce") {
                Some(p) => Self {
                    config: p.config_local_dir().to_path_buf(),
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

    /// Everything in one folder: the settings and the logs in it, the caches in its `cache` folder.
    pub fn in_dir(dir: &Path) -> Self {
        Self { config: dir.to_path_buf(), cache: dir.join("cache"), logs: dir.to_path_buf() }
    }

    /// The folder of settings.toml and the tools' layouts.
    pub fn config_dir(&self) -> &Path {
        &self.config
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
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct GameSettings {
    /// The installation folder (the one with `data`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dir: Option<PathBuf>,

    /// Mod pack file names in the data folder, read on top of the game's release packs.
    pub mod_packs: Vec<String>,

    /// The project folder (the UI tool's saved layouts and the user's pictures), read before every pack.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project_dir: Option<PathBuf>,

    /// Loose folders read before every pack, after the project folder, highest priority first.
    pub overlay_folders: Vec<PathBuf>,
}

/// How the editor looks.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Appearance {
    /// A theme's id (the editor lists them).
    pub theme: String,

    /// The text size in points, before the display's scale.
    pub font_size: f32,
}

impl Default for Appearance {
    fn default() -> Self {
        Self { theme: "dark".to_owned(), font_size: 15.0 }
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

    /// Where "Save" writes loose files (under their in-pack path); none: the editor's default.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_dir: Option<PathBuf>,

    pub appearance: Appearance,
    pub games: BTreeMap<String, GameSettings>,

    /// Each tool's own table, read and written with the tool's type (see [`Self::tool`]).
    pub tools: BTreeMap<String, toml::Table>,

    #[serde(skip)]
    paths: Paths,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            version: VERSION, game: "attila".to_owned(), frame_cap: DEFAULT_FRAME_CAP, output_dir: None, appearance: Appearance::default(),
            games: BTreeMap::new(), tools: BTreeMap::new(), paths: Paths::default(),
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

        settings.paths = paths;
        settings
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

    pub fn project_dir(&self) -> Option<PathBuf> {
        self.game_settings().project_dir
    }

    /// `$WCE_OUTPUT_DIR`, else the file's.
    pub fn output_dir(&self) -> Option<PathBuf> {
        env_value("WCE_OUTPUT_DIR").map(PathBuf::from).or_else(|| self.output_dir.clone())
    }

    /// `$WCE_FRAME_CAP`, else the file's.
    pub fn frame_cap(&self) -> u32 {
        env_value("WCE_FRAME_CAP").and_then(|v| v.trim().parse().ok()).unwrap_or(self.frame_cap)
    }

    // ---- the tools' tables

    /// A tool's settings: its table read as `T`. A missing table, or one that does not read as `T`, gives `T`'s defaults.
    pub fn tool<T: DeserializeOwned + Default>(&self, id: &str) -> T {
        let Some(table) = self.tools.get(id) else { return T::default() };
        table.clone().try_into().unwrap_or_else(|e| {
            log::warn(format!("settings: [tools.{id}] does not read ({e}), using its defaults"));
            T::default()
        })
    }

    /// Replaces a tool's table with `value`.
    pub fn set_tool<T: Serialize>(&mut self, id: &str, value: &T) {
        match toml::Table::try_from(value) {
            Ok(table) => {
                self.tools.insert(id.to_owned(), table);
            }

            Err(e) => log::error(format!("settings: [tools.{id}] can't be written: {e}")),
        }
    }
}
