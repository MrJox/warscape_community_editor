//! Where the renderer reads the game's files from: the packs, or (tools and comparisons) a folder.

use std::path::PathBuf;
use std::sync::Arc;

use wce_vfs::{Vfs, normalize};

use crate::model_cache::SharedReader;

/// The game's files, readable from any thread.
pub trait Files: Send + Sync {

    /// The bytes of a file.
    ///
    /// # Errors
    ///
    /// Fails, with a message, if the file is missing or can't be read.
    fn read(&self, path: &str) -> Result<Vec<u8>, String>;

    /// If the file exists.
    fn exists(&self, path: &str) -> bool;
}

impl Files for Vfs {
    fn read(&self, path: &str) -> Result<Vec<u8>, String> {
        Vfs::read(self, path).map_err(|error| error.to_string())
    }

    fn exists(&self, path: &str) -> bool {
        self.owner(path).is_some()
    }
}

/// Files under a folder, by their normalized path (an extracted copy of the packs).
#[derive(Clone, Debug)]
pub struct FolderFiles(pub PathBuf);

impl Files for FolderFiles {
    fn read(&self, path: &str) -> Result<Vec<u8>, String> {
        let p = self.0.join(normalize(path));
        if p.exists() { std::fs::read(&p).map_err(|error| error.to_string()) } else { Err(format!("missing {path}")) }
    }

    fn exists(&self, path: &str) -> bool {
        self.0.join(normalize(path)).exists()
    }
}

/// A reader for the caches.
pub fn shared_reader(files: &Arc<dyn Files>) -> SharedReader {
    let files = files.clone();
    Arc::new(move |path: &str| files.read(path))
}
