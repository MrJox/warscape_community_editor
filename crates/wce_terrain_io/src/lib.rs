//! Maps in and out of the Terrain tool's model (`wce_terrain`), one module per game format. A format reads a [`MapFiles`] (the map's
//! folder as named blobs) into a [`wce_terrain::MapModel`] and writes a model back into one; reading and writing never touch the disk
//! themselves (`MapFiles::from_dir` enumerates sources), so the gates run in memory.

pub mod attila;
pub mod built;
mod campaign_props;

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

/// Native dependency ordering for the Terrain consumer: supporting resources first,
/// then CHMF, preview, full LF baseline and authoring metadata. Packs publish last.
pub fn publication_priority(name: &str) -> u8 {
    match Path::new(name).file_name().unwrap_or_default().to_string_lossy().to_ascii_lowercase().as_str() {
        "terrain_layers.wcl" => 4,
        "lf_height_map.dds" | "lf_sea_height_map.dds" | "lf_heights.tif" | "lf_sea_heights.tif" | "height_map_0.png" => 3,
        "lf_height_map_lq.dds" => 2,
        "lf_height_map.data" => 1,
        _ => 0,
    }
}

pub(crate) fn file_error(file: &str, what: impl std::fmt::Display) -> MapIoError {
    MapIoError::File { file: file.to_owned(), what: what.to_string() }
}

/// The files of a map's folder by relative path (`/` separated), sub folders included.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MapFiles {
    pub folder: BTreeMap<String, Blob>,
    pub disk_source: Option<std::sync::Arc<wce_foundation::disk_changes::Folder>>,
}

fn collect(
    dir: &Path,
    prefix: &str,
    out: &mut BTreeMap<String, Blob>,
    disk: &mut wce_foundation::disk_changes::Folder,
) -> std::io::Result<()> {
    for e in std::fs::read_dir(dir)? {
        let e = e?;
        let name = e.file_name().to_string_lossy().into_owned();
        if wce_foundation::publication::reserved(&name) {
            continue;
        }
        let rel = if prefix.is_empty() { name } else { format!("{prefix}/{name}") };
        if e.file_type()?.is_dir() {
            collect(&e.path(), &rel, out, disk)?;
        } else {
            disk.record(&e.path()).map_err(std::io::Error::other)?;
            out.insert(rel, Blob::File(e.path()));
        }
    }

    Ok(())
}

impl MapFiles {
    /// Files read already (a folder of the game's packs).
    pub fn from_bytes(files: impl IntoIterator<Item = (String, Vec<u8>)>) -> Self {
        Self { folder: files.into_iter().map(|(p, b)| (p, Blob::Bytes(std::sync::Arc::from(b)))).collect(), disk_source: None }
    }

    /// The files of a folder (not read yet: blobs that point at the files).
    ///
    /// # Errors
    ///
    /// The folder could not be listed.
    pub fn from_dir(dir: &Path) -> Result<Self> {
        let mut folder = BTreeMap::new();
        let mut disk = wce_foundation::disk_changes::Folder::empty(dir);
        collect(dir, "", &mut folder, &mut disk)?;
        Ok(Self { folder, disk_source: Some(std::sync::Arc::new(disk)) })
    }
}
