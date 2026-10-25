//! Tiles in and out of the game-neutral model (`wce_tile_model`), one module per game format (docs/formats/tile_model.md).
//!
//! A format reads a [`TileFiles`] (the tile's folder and its tile database entry, as named blobs) into a [`Tile`] and writes a tile back
//! into one. Reading and writing never touch the disk themselves: [`TileFiles::from_dir`] / [`TileFiles::write_to`] do, so tests and the
//! corpus gate run in memory.
//!
//! Formats: [`attila::TedProject`] (`attila.ted_project`).

pub mod attila;
pub mod rome2;
pub mod serde_node;
pub mod xml;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use thiserror::Error;
use wce_formats::tile_index::TileIndex;
use wce_tile_model::{Blob, Tile};

#[derive(Debug, Error)]
pub enum TileIoError {
    #[error("{file}: {what}")]
    File { file: String, what: String },

    #[error(transparent)]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, TileIoError>;

/// The files of a tile: its folder and its tile database entry.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TileFiles {
    /// Every file of the tile's folder by relative path (`/` separated), sub folders included.
    pub folder: BTreeMap<String, Blob>,

    /// The tile database entry: its file name and bytes.
    pub entry: Option<(String, Blob)>,

    /// The variation's name in the entry.
    pub variation: Option<String>,
}

fn collect(dir: &Path, prefix: &str, out: &mut BTreeMap<String, Blob>) -> std::io::Result<()> {
    for e in std::fs::read_dir(dir)? {
        let e = e?;
        let name = e.file_name().to_string_lossy().into_owned();
        let rel = if prefix.is_empty() { name } else { format!("{prefix}/{name}") };
        if e.file_type()?.is_dir() {
            collect(&e.path(), &rel, out)?;
        } else {
            out.insert(rel, Blob::File(e.path()));
        }
    }

    Ok(())
}

impl TileFiles {

    /// The files of a folder (not read yet: blobs that point at the files), without an entry.
    ///
    /// # Errors
    ///
    /// The folder could not be listed.
    pub fn from_dir(dir: &Path) -> Result<Self> {
        let mut folder = BTreeMap::new();
        collect(dir, "", &mut folder)?;
        Ok(Self { folder, entry: None, variation: None })
    }

    /// [`Self::from_dir`] plus the folder's tile database entry and variation, found through the kit's index when the folder is in a
    /// kit (`<kit>/raw_data/terrain/tiles/...`).
    ///
    /// # Errors
    ///
    /// The folder could not be listed.
    pub fn from_kit_dir(dir: &Path) -> Result<Self> {
        let mut files = Self::from_dir(dir)?;

        // The editor's tile.wce is read as the tile.ted TEd would get ("Save for Assembly Kit").
        if let Some(blob) = files.folder.remove(wce_tile::project_files::WCE_FILE) {
            let ted = wce_tile::project_files::to_ted(blob.bytes()?.to_vec()).map_err(std::io::Error::other)?.0;
            files.folder.insert(wce_tile::project_files::TED_FILE.to_owned(), Blob::Bytes(ted.into()));
        }

        if let Some(root) = kit_raw_root(dir) {
            let index = TileIndex::shared(&root);
            let r = index.resolve(dir);
            if let Some(e) = r.entry {
                let name = e.xml.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                files.entry = Some((name, Blob::File(e.xml.clone())));
                files.variation = r.variation.and_then(|v| e.variations.get(v).cloned());
            }
        }

        Ok(files)
    }

    /// Writes the folder's files into `dir` and the entry into `entry_dir` (skipped when `None`).
    ///
    /// # Errors
    ///
    /// A file could not be read or written.
    pub fn write_to(&self, dir: &Path, entry_dir: Option<&Path>) -> Result<()> {
        for (rel, blob) in &self.folder {
            let target = dir.join(rel);
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent)?;
            }

            match blob {
                Blob::File(src) if src == &target => {}
                Blob::File(src) => {
                    std::fs::copy(src, &target)?;
                }

                Blob::Bytes(b) => std::fs::write(&target, b)?,
            }
        }

        if let (Some((name, blob)), Some(entry_dir)) = (&self.entry, entry_dir) {
            std::fs::create_dir_all(entry_dir)?;
            std::fs::write(entry_dir.join(name), blob.bytes()?)?;
        }

        Ok(())
    }
}

/// The raw_data folder a tile path lives under (the part before `terrain/tiles`), if there is one.
pub fn kit_raw_root(path: &Path) -> Option<PathBuf> {
    let g = path.to_string_lossy().replace('\\', "/");
    let at = g.to_lowercase().find("/terrain/tiles/")?;
    Some(PathBuf::from(&g[..at]))
}

/// How a part of a tile was read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PartRead {
    /// Into the model.
    Decoded,

    /// A file of a kind the format reads did not read: carried along as an opaque file.
    Failed(String),
}

/// How a part of a tile was written.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PartWritten {
    /// From the model.
    Encoded,

    /// The original bytes of an image whose encoding the writer does not reproduce, the pixels unchanged.
    Kept,

    /// An opaque file, copied.
    Copied,
}

#[derive(Clone, Debug, Default)]
pub struct ReadReport {
    /// The parts the format reads (by file name) and how they read.
    pub parts: Vec<(String, PartRead)>,

    /// What the XML reader counted, over every XML part.
    pub xml: xml::ReadStats,
}

#[derive(Clone, Debug, Default)]
pub struct WriteReport {
    pub parts: Vec<(String, PartWritten)>,
}

/// One game's tile format.
pub trait TileFormat: Sync {
    /// `<game>.<format>`, e.g. `attila.ted_project`.
    fn id(&self) -> &'static str;

    /// Whether the files look like this format's.
    fn claims(&self, files: &TileFiles) -> bool;

    /// # Errors
    ///
    /// A file the model needs could not be read.
    fn read(&self, files: &TileFiles) -> Result<(Tile, ReadReport)>;

    /// # Errors
    ///
    /// A part of the tile cannot be written in this format.
    fn write(&self, tile: &Tile) -> Result<(TileFiles, WriteReport)>;
}

/// Every format, in the order they are tried.
pub fn formats() -> Vec<Box<dyn TileFormat>> {
    vec![Box::new(attila::TedProject), Box::new(attila::TileBinary)]
}

/// The format of a format id.
pub fn format(id: &str) -> Option<Box<dyn TileFormat>> {
    formats().into_iter().find(|f| f.id() == id)
}
