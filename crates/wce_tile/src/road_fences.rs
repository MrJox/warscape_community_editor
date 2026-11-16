//! The fences of the generated road types (`road_dirt`), from a file the user can edit (`road_fences.toml` in the settings folder; user
//! 2026-11-16: buildings added without a rebuild): each fence's building, which way its model runs and its joiners. The file shipped is
//! [`DEFAULT_TEXT`]; it is written where missing and read on each scan of the prefab library.

use std::path::Path;

use serde::{Deserialize, Serialize};

/// The file's name, in the settings folder.
pub const FILE_NAME: &str = "road_fences.toml";

/// The file as shipped (and written where there is none).
pub const DEFAULT_TEXT: &str = include_str!("road_fences.toml");

/// Which way a fence's model runs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Along {
    /// Along its x, from its pivot (the kit's `lowfence_*`).
    #[default]
    X,
    /// Along its z, about its pivot (the cultures' fences).
    Z,
}

/// A fence: its piece's name (`fence_<name>`; none for the type's own `fence`), its building, its model's way and its joiners.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Fence {
    #[serde(default)]
    pub name: String,
    pub key: String,
    #[serde(default)]
    pub along: Along,
    #[serde(default)]
    pub joiners: Vec<String>,
}

impl Fence {
    /// Its piece's name: `fence`, or `fence_<name>`.
    #[must_use]
    pub fn piece(&self) -> String {
        if self.name.is_empty() { "fence".to_owned() } else { format!("fence_{}", self.name) }
    }
}

/// The file: the road types' own fence and the named ones.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Fences {
    pub fence: Fence,
    #[serde(default)]
    pub named: Vec<Fence>,
}

impl Default for Fences {
    fn default() -> Self {
        parse(DEFAULT_TEXT).unwrap_or_else(|e| panic!("the shipped {FILE_NAME} reads: {e}"))
    }
}

/// Reads a fences file.
///
/// # Errors
///
/// It is not TOML of this shape, or a name is not one a piece can have (empty, a number alone, other than `a-z 0-9 _`, twice), or a key
/// is empty.
pub fn parse(text: &str) -> Result<Fences, String> {
    let f: Fences = toml::from_str(text).map_err(|e| e.to_string())?;
    let mut seen = std::collections::HashSet::new();
    for n in &f.named {
        let ok_chars = n.name.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
        if n.name.is_empty() || !ok_chars || n.name.parse::<u32>().is_ok() || n.name.starts_with('_') || n.name.ends_with('_') {
            return Err(format!("the fence name '{}' (lowercase letters, digits and _, not a number alone)", n.name));
        }

        if !seen.insert(n.name.clone()) {
            return Err(format!("the fence name '{}' twice", n.name));
        }
    }

    if let Some(k) = std::iter::once(&f.fence).chain(&f.named).find(|k| k.key.trim().is_empty()) {
        return Err(format!("the fence '{}' has no key", k.piece()));
    }

    Ok(f)
}

/// The fences file in `dir` (the settings folder): written as shipped where missing; one that does not read is logged and the shipped
/// one used (the file is left as it is, for the user to mend).
#[must_use]
pub fn load(dir: &Path) -> Fences {
    let path = dir.join(FILE_NAME);
    match std::fs::read_to_string(&path) {
        Ok(text) => parse(&text).unwrap_or_else(|e| {
            wce_foundation::log::warn(format!("{}: {e}: the shipped fences are used", path.display()));
            Fences::default()
        }),
        Err(_) => {
            if let Err(e) = std::fs::create_dir_all(dir).and_then(|()| std::fs::write(&path, DEFAULT_TEXT)) {
                wce_foundation::log::warn(format!("{}: not written: {e}", path.display()));
            }

            Fences::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shipped_file_reads_and_bad_names_do_not() {
        let f = Fences::default();
        assert_eq!(f.fence.piece(), "fence");
        assert_eq!(f.fence.key, "lowfence_3");
        assert!(f.named.iter().any(|n| n.piece() == "fence_greek" && n.along == Along::Z && n.joiners == ["greek_lowfence_pillar"]));
        assert!(parse("[fence]\nkey = \"a\"\n[[named]]\nname = \"2\"\nkey = \"b\"\n").is_err(), "a number alone reads as a variant");
        assert!(parse("[fence]\nkey = \"a\"\n[[named]]\nname = \"Big\"\nkey = \"b\"\n").is_err());
        assert!(parse("[fence]\nkey = \"a\"\n[[named]]\nname = \"x\"\nkey = \"b\"\n[[named]]\nname = \"x\"\nkey = \"c\"\n").is_err());
        assert!(parse("[fence]\nkey = \"a\"\n").is_ok_and(|f| f.named.is_empty() && f.fence.along == Along::X));
    }

    #[test]
    fn written_where_missing_kept_once_edited() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(load(dir.path()), Fences::default());
        assert_eq!(std::fs::read_to_string(dir.path().join(FILE_NAME)).unwrap(), DEFAULT_TEXT);
        std::fs::write(dir.path().join(FILE_NAME), "[fence]\nkey = \"mine\"\n").unwrap();
        assert_eq!(load(dir.path()).fence.key, "mine");
        std::fs::write(dir.path().join(FILE_NAME), "not toml [").unwrap();
        assert_eq!(load(dir.path()), Fences::default());
        assert_eq!(std::fs::read_to_string(dir.path().join(FILE_NAME)).unwrap(), "not toml [", "left for the user to mend");
    }
}
