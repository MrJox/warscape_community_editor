//! Practical destination normalization; no file identity/ownership matcher.
use std::path::{Component, Path};

pub fn key(path: &Path) -> Result<String, String> {
    let absolute = std::path::absolute(path).map_err(|e| e.to_string())?;
    let mut clean = std::path::PathBuf::new();
    for c in absolute.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                clean.pop();
            }
            _ => clean.push(c.as_os_str()),
        }
    }
    // Existing project/kit roots may be reached through the migration junction. Resolve the
    // nearest existing parent too, without requiring a new Save As folder to exist.
    for parent in clean.ancestors() {
        if let Ok(mut resolved) = std::fs::canonicalize(parent) {
            let tail = clean.strip_prefix(parent).map_err(|e| e.to_string())?;
            if !tail.as_os_str().is_empty() {
                resolved.push(tail);
            }
            let text = resolved.to_string_lossy().into_owned();
            #[cfg(windows)]
            {
                return Ok(text.to_uppercase());
            }
            #[cfg(not(windows))]
            {
                return Ok(text);
            }
        }
    }
    Err(format!("cannot resolve destination {}", path.display()))
}
