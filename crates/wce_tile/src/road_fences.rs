//! The fences of the generated road types (`road_dirt`), from the app's `tables/road_fences.toml` and the user's
//! `road_fences_custom.toml` beside it (`user_files`; user 2026-11-16: buildings added without a rebuild): each fence's building, which
//! way its model runs and its joiners. Read on each scan of the prefab library.

use serde::{Deserialize, Serialize};

/// The shipped file's name, in the app's tables folder.
pub const FILE_NAME: &str = "road_fences.toml";

/// The shipped file as built into the editor (used where the app's is missing).
pub const DEFAULT_TEXT: &str = include_str!("../../../tables/road_fences.toml");

/// The fences, from the files ([`load`]) or as built in.
pub static FENCES: crate::user_files::Registry<Fences> = crate::user_files::Registry::new(FILE_NAME, DEFAULT_TEXT, parse_with);

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
    /// The built-in fences.
    fn default() -> Self {
        FENCES.shipped().clone()
    }
}

/// A custom file: the fence in place of the shipped one, named fences in place of the shipped ones of their names or added.
#[derive(Debug, Default, Deserialize)]
struct Custom {
    fence: Option<Fence>,
    #[serde(default)]
    named: Vec<Fence>,
}

/// Reads a fences file (no custom one).
///
/// # Errors
///
/// As [`parse_with`].
pub fn parse(text: &str) -> Result<Fences, String> {
    parse_with(text, None)
}

/// Reads the shipped fences file and the custom one over it.
///
/// # Errors
///
/// Either is not TOML of its shape, or a name is not one a piece can have (empty, a number alone, other than `a-z 0-9 _`, twice), or a
/// key is empty.
pub fn parse_with(shipped: &str, custom: Option<&str>) -> Result<Fences, String> {
    let mut f: Fences = toml::from_str(shipped).map_err(|e| format!("{FILE_NAME}: {e}"))?;
    if let Some(text) = custom {
        let name = crate::user_files::custom_name(FILE_NAME);
        let c: Custom = toml::from_str(text).map_err(|e| format!("{name}: {e}"))?;
        if let Some(fence) = c.fence {
            f.fence = fence;
        }

        crate::user_files::replace_or_add(&mut f.named, c.named, |a, b| a.name == b.name);
    }

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

/// Reads the fences files in the tables folder (`user_files::tables_dir`): the fences from then on.
pub fn load(dir: &std::path::Path) {
    FENCES.load(dir);
}

/// The fences: the files' last read, else the built-in ones.
#[must_use]
pub fn fences() -> &'static Fences {
    FENCES.get()
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
    fn a_custom_file_replaces_by_name_and_adds() {
        let custom = "[fence]\nkey = \"mine\"\n[[named]]\nname = \"greek\"\nkey = \"my_greek\"\nalong = \"z\"\n[[named]]\nname = \"new\"\nkey = \"n\"\n";
        let f = parse_with(DEFAULT_TEXT, Some(custom)).unwrap();
        let shipped = Fences::default();
        assert_eq!(f.fence.key, "mine");
        assert_eq!(f.named.len(), shipped.named.len() + 1);
        let greek = f.named.iter().position(|n| n.name == "greek").unwrap();
        assert_eq!((greek, f.named[greek].key.as_str()), (shipped.named.iter().position(|n| n.name == "greek").unwrap(), "my_greek"));
        assert!(f.named[greek].joiners.is_empty(), "replaced whole");
        assert_eq!(f.named.last().unwrap().name, "new");
        assert!(parse_with(DEFAULT_TEXT, Some("[[named]]\nname = \"Bad\"\nkey = \"k\"\n")).is_err());
        assert!(parse_with(DEFAULT_TEXT, Some("not toml [")).unwrap_err().starts_with("road_fences_custom.toml"));
    }
}
