//! Files made in memory, read before the packs and folders by every [`crate::files::Files`] here: the procedural walls' preview pieces
//! (docs/plans/procedural_walls.md), drawn like any building of the packs. Process-wide, by normalized path.

use std::collections::HashMap;
use std::sync::{Arc, LazyLock, RwLock};

use crate::normalize;

static STORE: LazyLock<RwLock<HashMap<String, Arc<Vec<u8>>>>> = LazyLock::new(RwLock::default);

/// Adds (or replaces) a file.
pub fn put(path: &str, bytes: Vec<u8>) {
    if let Ok(mut s) = STORE.write() {
        s.insert(normalize(path), Arc::new(bytes));
    }
}

/// A file, if one was made at this path.
pub fn get(path: &str) -> Option<Arc<Vec<u8>>> {
    let s = STORE.read().ok()?;
    if s.is_empty() {
        return None;
    }

    s.get(&normalize(path)).cloned()
}

/// The made files below `prefix` ending with `extension` (normalized paths).
pub fn list(prefix: &str, extension: &str) -> Vec<String> {
    let (prefix, extension) = (normalize(prefix), normalize(extension));
    let Ok(s) = STORE.read() else { return vec![] };
    s.keys().filter(|k| k.starts_with(&prefix) && k.ends_with(&extension)).cloned().collect()
}

/// Forgets the files below a folder (`prefix` ends with `/`), or a file.
pub fn remove_prefix(prefix: &str) {
    let prefix = normalize(prefix);
    if let Ok(mut s) = STORE.write() {
        s.retain(|k, _| !k.starts_with(&prefix));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn files_are_found_by_their_normalized_path_and_forgotten_by_folder() {
        put("RigidModels/Buildings/wce_test_gen/a.rigid_model_v2", vec![1, 2]);
        assert_eq!(get(r"rigidmodels\buildings\wce_test_gen\a.rigid_model_v2").as_deref(), Some(&vec![1, 2]));
        remove_prefix("rigidmodels/buildings/wce_test_gen/");
        assert!(get("rigidmodels/buildings/wce_test_gen/a.rigid_model_v2").is_none());
    }
}
