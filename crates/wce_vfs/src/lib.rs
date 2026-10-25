//! The game's file system: one view of the files of a game's packs, in their priority order, plus loose overlay folders above them.
//!
//! Pack priority is the editor's own ([`GameProfile::pack_order`]):
//!
//! 0. loose overlay folders (an Assembly Kit's `raw_data`, for instance), in the order given;
//! 1. the enabled mods, in the order of the launcher's `used_mods.txt` (first listed wins);
//! 2. the vanilla packs: the patch packs, then the rest (see [`GameProfile::pack_order_with`]).
//!
//! An empty file in a pack removes the file from the packs below it (the patch packs remove files that way): the path is not listed
//! and can't be read, unless a pack above has it again.
//!
//! Packs are read with rpfm_lib, lazily: their file data stays on disk until a file is read. Paths are compared lower-case and with
//! forward slashes; each pack keeps its own spelling for reading.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use rpfm_lib::files::pack::Pack as RpfmPack;
use rpfm_lib::games::supported_games::SupportedGames;

pub use wce_foundation::game::{GameFeatures, GameProfile, PackOrder, PackRef, VanillaPacks};

pub mod files;
pub mod generated;
pub mod mod_pack;

/// Errors of the file system.
#[derive(Debug, thiserror::Error)]
pub enum VfsError {
    #[error("the game folder is not set or does not exist: {0}")]
    NoGameFolder(PathBuf),

    #[error("rpfm doesn't know the game '{0}'")]
    UnknownGame(String),

    #[error("cannot read the pack {0}: {1}")]
    Pack(PathBuf, String),

    #[error("not found in any pack: {0}")]
    NotFound(String),

    #[error("cannot read {0} from {1}: {2}")]
    Read(String, String, String),
}

pub type Result<T> = std::result::Result<T, VfsError>;

/// What to mount.
#[derive(Clone, Debug)]
pub struct Config {
    /// Folder of the game.
    pub game_dir: PathBuf,

    /// The game.
    pub profile: GameProfile,

    /// If the enabled mods are mounted.
    pub use_mods: bool,

    /// The mods to mount, by file name in the data folder (first wins). `None`: the launcher's list if `use_mods`.
    pub mod_packs: Option<Vec<String>>,

    /// Loose folders mounted above every pack, highest priority first.
    pub overlay_folders: Vec<PathBuf>,
}

impl Config {

    /// The packs of a game in its folder, with its enabled mods and no overlay folders.
    pub fn new(game_dir: impl Into<PathBuf>, profile: GameProfile) -> Self {
        Self { game_dir: game_dir.into(), profile, use_mods: true, mod_packs: None, overlay_folders: vec![] }
    }
}

/// A mounted pack or overlay folder.
#[derive(Clone, Debug)]
pub struct Pack {
    /// File name of the pack, or of the folder.
    pub name: String,

    /// Path of the pack, or of the folder.
    pub path: PathBuf,

    /// If the pack is a mod.
    pub is_mod: bool,

    /// If it's a folder on disk instead of a pack.
    pub loose: bool,
}

/// The pack index of a path an empty file removed, while mounting.
const REMOVED: usize = usize::MAX;

/// How a mounted pack is read.
enum Source {
    Folder,
    Pack(Box<Mutex<RpfmPack>>),
}

/// The game's file system.
pub struct Vfs {
    profile: GameProfile,
    packs: Vec<Pack>,
    sources: Vec<Source>,
    missing_mods: Vec<String>,

    /// Path -> index of the pack it's read from.
    files: BTreeMap<String, usize>,

    /// Path -> every pack that has it, highest priority first.
    owners: HashMap<String, Vec<usize>>,

    /// Per pack, path -> the path as the pack spells it, where that's not the normalized path.
    spelling: Vec<HashMap<String, String>>,
}

impl Vfs {

    /// Mounts the packs and overlay folders of a configuration.
    ///
    /// # Errors
    ///
    /// Returns an error if the game folder doesn't exist, rpfm doesn't know the game, or a pack can't be read.
    pub fn mount(config: &Config) -> Result<Self> {
        if !config.game_dir.is_dir() {
            return Err(VfsError::NoGameFolder(config.game_dir.clone()));
        }

        let games = SupportedGames::default();
        let game = games.game(config.profile.key).ok_or_else(|| VfsError::UnknownGame(config.profile.key.to_owned()))?;

        let mut vfs = Self {
            profile: config.profile.clone(),
            packs: vec![],
            sources: vec![],
            missing_mods: vec![],
            files: BTreeMap::new(),
            owners: HashMap::new(),
            spelling: vec![],
        };

        for folder in &config.overlay_folders {
            let name = folder.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
            let paths = list_folder(folder);
            vfs.add(Pack { name, path: folder.clone(), is_mod: false, loose: true }, Source::Folder, paths, vec![]);
        }

        let order = match &config.mod_packs {
            Some(mods) => config.profile.pack_order_with(&config.game_dir, mods),
            None => config.profile.pack_order(&config.game_dir, config.use_mods),
        };
        vfs.missing_mods = order.missing_mods;
        for pack_ref in order.packs {
            let pack = RpfmPack::read_and_merge(std::slice::from_ref(&pack_ref.path), game, true, false, false)
                .map_err(|error| VfsError::Pack(pack_ref.path.clone(), error.to_string()))?;
            let (paths, removed) = pack.files().iter().map(|(path, file)| (path.clone(), file.stored_size() == Some(0))).partition::<Vec<_>, _>(|(_, empty)| !empty);
            let paths = paths.into_iter().map(|(path, _)| path).collect();
            let removed = removed.into_iter().map(|(path, _)| path).collect();
            vfs.add(Pack { name: pack_ref.name, path: pack_ref.path, is_mod: pack_ref.is_mod, loose: false }, Source::Pack(Box::new(Mutex::new(pack))), paths, removed);
        }

        vfs.files.retain(|_, pack| *pack != REMOVED);
        Ok(vfs)
    }

    /// Indexes a mounted pack or folder, with lower priority than the ones before it. `removed` are its empty files: the paths it removes
    /// from the packs below it.
    fn add(&mut self, pack: Pack, source: Source, paths: Vec<String>, removed: Vec<String>) {
        let index = self.packs.len();
        for path in removed {
            self.files.entry(normalize(&path)).or_insert(REMOVED);
        }

        let mut spelling = HashMap::new();
        for path in paths {
            let normalized = normalize(&path);
            self.files.entry(normalized.clone()).or_insert(index);
            self.owners.entry(normalized.clone()).or_default().push(index);
            if normalized != path {
                spelling.insert(normalized, path);
            }
        }

        self.packs.push(pack);
        self.sources.push(source);
        self.spelling.push(spelling);
    }

    /// The game the packs are of.
    pub fn profile(&self) -> &GameProfile {
        &self.profile
    }

    /// The mounted packs and folders, highest priority first.
    pub fn packs(&self) -> &[Pack] {
        &self.packs
    }

    /// Enabled mods that are not in the data folder.
    pub fn missing_mods(&self) -> &[String] {
        &self.missing_mods
    }

    /// Every path, normalized, with the index of the pack it's read from.
    pub fn files(&self) -> &BTreeMap<String, usize> {
        &self.files
    }

    /// The paths starting with `prefix` and, if not empty, ending with `extension`. Both are normalized first.
    pub fn list(&self, prefix: &str, extension: &str) -> Vec<&str> {
        let prefix = normalize(prefix);
        let extension = normalize(extension);
        self.files.range(prefix.clone()..)
            .take_while(|(path, _)| path.starts_with(&prefix))
            .filter(|(path, _)| path.ends_with(&extension))
            .map(|(path, _)| path.as_str())
            .collect()
    }

    /// The pack a path is read from, if any pack has it.
    pub fn owner(&self, path: &str) -> Option<usize> {
        self.files.get(&normalize(path)).copied()
    }

    /// Every pack that has a path, highest priority first.
    pub fn owners(&self, path: &str) -> &[usize] {
        self.owners.get(&normalize(path)).map_or(&[], |owners| owners.as_slice())
    }

    /// Reads a file from the pack with the highest priority that has it.
    ///
    /// # Errors
    ///
    /// Returns [`VfsError::NotFound`] if no pack has it, or an error if it can't be read.
    pub fn read(&self, path: &str) -> Result<Vec<u8>> {
        let pack = self.owner(path).ok_or_else(|| VfsError::NotFound(path.to_owned()))?;
        self.read_from(pack, path)
    }

    /// Reads a file from one specific pack.
    ///
    /// # Errors
    ///
    /// Returns [`VfsError::NotFound`] if the pack doesn't have it, or an error if it can't be read.
    pub fn read_from(&self, pack: usize, path: &str) -> Result<Vec<u8>> {
        let normalized = normalize(path);
        if !self.owners(&normalized).contains(&pack) {
            return Err(VfsError::NotFound(path.to_owned()));
        }

        let stored = self.spelling[pack].get(&normalized).unwrap_or(&normalized);
        let read_error = |error: String| VfsError::Read(path.to_owned(), self.packs[pack].name.clone(), error);
        match &self.sources[pack] {
            Source::Folder => std::fs::read(self.packs[pack].path.join(stored)).map_err(|error| read_error(error.to_string())),
            Source::Pack(rpfm_pack) => {
                let mut rpfm_pack = rpfm_pack.lock().map_err(|_| read_error("another read panicked".to_owned()))?;
                let file = rpfm_pack.files_mut().get_mut(stored).ok_or_else(|| VfsError::NotFound(path.to_owned()))?;
                file.encode(&None, false, false, true)
                    .map_err(|error| read_error(error.to_string()))?
                    .ok_or_else(|| read_error("no data".to_owned()))
            }
        }
    }
}

/// Normalizes a path: lower-case ASCII and forward slashes.
pub fn normalize(path: &str) -> String {
    path.chars().map(|char| if char == '\\' { '/' } else { char.to_ascii_lowercase() }).collect()
}

/// Every file under a folder, as paths relative to it with forward slashes.
pub(crate) fn list_folder(folder: &Path) -> Vec<String> {
    let mut paths = vec![];
    let mut pending = vec![folder.to_path_buf()];
    while let Some(dir) = pending.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for entry in entries.flatten() {
            let path = entry.path();
            match entry.file_type() {
                Ok(kind) if kind.is_dir() => pending.push(path),
                Ok(kind) if kind.is_file() => {
                    if let Ok(relative) = path.strip_prefix(folder) {
                        paths.push(relative.to_string_lossy().replace('\\', "/"));
                    }
                }
                _ => {}
            }
        }
    }

    paths
}
