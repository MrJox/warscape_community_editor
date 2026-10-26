//! Where the editor reads the game's files from: the packs, or (tools and comparisons) a folder with an extracted copy of them; files made in
//! memory ([`crate::generated`]) first.

use std::path::PathBuf;

use crate::{GameProfile, Vfs, generated, list_folder, normalize};

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

    /// The normalized paths starting with `prefix` and, if not empty, ending with `extension`, sorted.
    fn list(&self, prefix: &str, extension: &str) -> Vec<String>;

    /// The game the files are of, when they know it (the mounted packs do; an extracted copy doesn't: Attila's, by default).
    fn game(&self) -> Option<&GameProfile> {
        None
    }
}

impl Files for Vfs {
    fn read(&self, path: &str) -> Result<Vec<u8>, String> {
        if let Some(made) = generated::get(path) {
            return Ok(made.as_ref().clone());
        }

        Vfs::read(self, path).map_err(|error| error.to_string())
    }

    fn exists(&self, path: &str) -> bool {
        generated::get(path).is_some() || self.owner(path).is_some()
    }

    fn list(&self, prefix: &str, extension: &str) -> Vec<String> {
        Vfs::list(self, prefix, extension).into_iter().map(str::to_owned).collect()
    }

    fn game(&self) -> Option<&GameProfile> {
        Some(self.profile())
    }
}

/// Files under a folder, by their normalized path (an extracted copy of the packs).
#[derive(Clone, Debug)]
pub struct FolderFiles(pub PathBuf);

impl Files for FolderFiles {
    fn read(&self, path: &str) -> Result<Vec<u8>, String> {
        if let Some(made) = generated::get(path) {
            return Ok(made.as_ref().clone());
        }

        let p = self.0.join(normalize(path));
        if p.exists() { std::fs::read(&p).map_err(|error| error.to_string()) } else { Err(format!("missing {path}")) }
    }

    fn exists(&self, path: &str) -> bool {
        generated::get(path).is_some() || self.0.join(normalize(path)).exists()
    }

    fn list(&self, prefix: &str, extension: &str) -> Vec<String> {
        let (prefix, extension) = (normalize(prefix), normalize(extension));
        let mut paths = list_folder(&self.0).into_iter().map(|p| normalize(&p)).filter(|p| p.starts_with(&prefix) && p.ends_with(&extension)).collect::<Vec<_>>();
        paths.sort();
        paths
    }
}
