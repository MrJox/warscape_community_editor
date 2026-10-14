//! Where the renderer reads the game's files from: the packs, or (tools and comparisons) a folder.

use std::sync::Arc;

pub use wce_vfs::files::{Files, FolderFiles};

use crate::model_cache::SharedReader;

/// A reader for the caches.
pub fn shared_reader(files: &Arc<dyn Files>) -> SharedReader {
    let files = files.clone();
    Arc::new(move |path: &str| files.read(path))
}
