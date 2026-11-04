//! Maps in and out of the Terrain tool's model (`wce_terrain`), one module per game format. A format reads a [`MapFiles`] (the map's
//! folder as named blobs) into a [`wce_terrain::MapModel`] and writes a model back into one; reading and writing never touch the disk
//! themselves ([`MapFiles::from_dir`] / [`MapFiles::write_to`] do), so the gates run in memory.

pub mod attila;
pub mod built;

use std::collections::BTreeMap;
use std::path::Path;

use thiserror::Error;
use wce_tile_model::Blob;

#[derive(Debug, Error)]
pub enum MapIoError {
    #[error("{file}: {what}")]
    File { file: String, what: String },

    #[error(transparent)]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, MapIoError>;

pub(crate) fn file_error(file: &str, what: impl std::fmt::Display) -> MapIoError {
    MapIoError::File { file: file.to_owned(), what: what.to_string() }
}

/// The files of a map's folder by relative path (`/` separated), sub folders included.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MapFiles {
    pub folder: BTreeMap<String, Blob>,
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

impl MapFiles {

    /// Files read already (a folder of the game's packs).
    pub fn from_bytes(files: impl IntoIterator<Item = (String, Vec<u8>)>) -> Self {
        Self { folder: files.into_iter().map(|(p, b)| (p, Blob::Bytes(std::sync::Arc::from(b)))).collect() }
    }

    /// The files of a folder (not read yet: blobs that point at the files).
    ///
    /// # Errors
    ///
    /// The folder could not be listed.
    pub fn from_dir(dir: &Path) -> Result<Self> {
        let mut folder = BTreeMap::new();
        collect(dir, "", &mut folder)?;
        Ok(Self { folder })
    }

    /// Writes the files into `dir` (a file that is already its own target is left alone).
    ///
    /// # Errors
    ///
    /// A file could not be read or written.
    pub fn write_to(&self, dir: &Path) -> Result<()> {
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

        Ok(())
    }
}
