//! TEd's prefabs: saved instance groups in `<kit raw_data>/art/prefabs` (and its folders), each file an `INSTANCE_SELECTION` holding one
//! `UserInstanceGroup`. TEd copies a prefab into a tile as a plain group; the editor takes a group whose name is a prefab's key (its file
//! name, case aside) for a copy of that prefab.
//!
//! A file as TEd writes it (`\n` line ends, four-space indentation):
//!
//! ```text
//! <INSTANCE_SELECTION version="17">
//!     <children>
//!         <UserInstanceGroup ... name="rock_formation_with_stairs" ...>
//!             <children>
//!                 ...
//!             </children>
//!         </UserInstanceGroup>
//!     </children>
//! </INSTANCE_SELECTION>
//! ```

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::ted_scene::TedScene;

/// The prefab files' version.
const VERSION: &str = "17";

/// A prefab of the library.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PrefabEntry {
    /// The file name without `.xml`: what a copy's group is named.
    pub key: String,
    pub path: PathBuf,

    /// Its folder below `art/prefabs` (`/` separated; empty at the top).
    pub folder: String,

    /// The `art/prefabs` folder it is in.
    pub root: PathBuf,
}

/// The prefabs of some kits, by key (lower case). A key in several folders or kits is the first one found.
#[derive(Clone, Debug, Default)]
pub struct PrefabLibrary {
    entries: BTreeMap<String, PrefabEntry>,
    roots: Vec<PathBuf>,
}

/// The prefab folder of a kit's raw_data folder.
pub fn prefab_root(raw_data: &Path) -> PathBuf {
    raw_data.join("art").join("prefabs")
}

impl PrefabLibrary {
    /// Every `.xml` below the prefab folders of these raw_data folders (folders starting with a dot, like a git checkout's, are skipped).
    pub fn scan(raw_data_roots: &[PathBuf]) -> Self {
        let mut lib = Self::default();
        for raw in raw_data_roots {
            let root = prefab_root(raw);
            if lib.roots.contains(&root) {
                continue;
            }

            let mut stack = vec![root.clone()];
            while let Some(dir) = stack.pop() {
                let Ok(read) = std::fs::read_dir(&dir) else { continue };
                let mut entries = read.flatten().map(|e| e.path()).collect::<Vec<_>>();
                entries.sort();
                for path in entries {
                    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                    if name.starts_with('.') {
                        continue;
                    }

                    if path.is_dir() {
                        stack.push(path);
                    } else if let Some(key) = name.strip_suffix(".xml").or_else(|| name.strip_suffix(".XML")) {
                        let folder = path.parent().and_then(|p| p.strip_prefix(&root).ok()).map(|p| p.to_string_lossy().replace('\\', "/")).unwrap_or_default();
                        lib.entries.entry(key.to_lowercase()).or_insert(PrefabEntry { key: key.to_owned(), path: path.clone(), folder, root: root.clone() });
                    }
                }
            }

            lib.roots.push(root);
        }

        lib
    }

    pub fn get(&self, key: &str) -> Option<&PrefabEntry> {
        self.entries.get(&key.to_lowercase())
    }

    pub fn entries(&self) -> impl Iterator<Item = &PrefabEntry> {
        self.entries.values()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The prefab folders scanned (where a new prefab can go).
    pub fn roots(&self) -> &[PathBuf] {
        &self.roots
    }
}

/// A prefab file's group: its whole text and the text of its contents (both at column 0, `\n` line ends, see [`TedScene::element_text`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Prefab {
    pub group_text: Vec<u8>,
    pub children_text: Vec<u8>,
}

/// Reads a prefab file.
///
/// # Errors
///
/// The file can't be read, is not XML, or holds no `UserInstanceGroup`.
pub fn load(path: &Path) -> Result<Prefab, String> {
    let scene = TedScene::parse_any(std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?).map_err(|e| format!("{}: {e}", path.display()))?;
    let group = *scene.find_all("UserInstanceGroup").first().ok_or_else(|| format!("{}: no UserInstanceGroup", path.display()))?;
    Ok(Prefab { group_text: scene.element_text(group), children_text: scene.children_text(group) })
}

/// A prefab file's text holding a group (its text at column 0).
pub fn file_text(group_text: &[u8]) -> Vec<u8> {
    let mut out = format!("<INSTANCE_SELECTION version=\"{VERSION}\">\n    <children>\n").into_bytes();
    for line in group_text.split_inclusive(|&c| c == b'\n') {
        out.extend_from_slice(b"        ");
        out.extend_from_slice(line);
    }

    if !out.ends_with(b"\n") {
        out.push(b'\n');
    }

    out.extend_from_slice(b"    </children>\n</INSTANCE_SELECTION>\n");
    out
}

/// Writes a new prefab file (its folders too).
///
/// # Errors
///
/// The file can't be written.
pub fn write(path: &Path, group_text: &[u8]) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }

    std::fs::write(path, file_text(group_text)).map_err(|e| format!("{}: {e}", path.display()))
}

/// Replaces the contents of a prefab file's group, keeping the rest of the file as it is.
///
/// # Errors
///
/// The file can't be read or written, or holds no `UserInstanceGroup`.
pub fn update_children(path: &Path, children_text: &[u8]) -> Result<(), String> {
    let mut scene = TedScene::parse_any(std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?).map_err(|e| format!("{}: {e}", path.display()))?;
    let group = *scene.find_all("UserInstanceGroup").first().ok_or_else(|| format!("{}: no UserInstanceGroup", path.display()))?;
    scene.set_children_text(group, children_text)?;
    std::fs::write(path, scene.save()).map_err(|e| format!("{}: {e}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    const FILE: &str = "<INSTANCE_SELECTION version=\"17\">\n    <children>\n        <UserInstanceGroup flags=\" OIF_VISIBLE\" position=\"vector(1,2,3)\" name=\"rocks\" orientation=\"euler(0,0,0)\">\n            <children>\n                <BUILDING_INSTANCE position=\"vector(1,0,0)\" key=\"rock\" name=\"rock\"/>\n                <BUILDING_INSTANCE position=\"vector(-1,0,0)\" key=\"rock\" name=\"rock\"/>\n            </children>\n        </UserInstanceGroup>\n    </children>\n</INSTANCE_SELECTION>\n";

    #[test]
    fn a_prefab_reads_writes_and_scans() {
        let dir = tempfile::tempdir().unwrap();
        let root = prefab_root(dir.path());
        let path = root.join("global").join("rocks.xml");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, FILE).unwrap();
        std::fs::create_dir_all(root.join(".git")).unwrap();
        std::fs::write(root.join(".git").join("x.xml"), FILE).unwrap();

        let lib = PrefabLibrary::scan(&[dir.path().to_path_buf()]);
        assert_eq!(lib.len(), 1, "a dot folder is skipped");
        let e = lib.get("ROCKS").unwrap();
        assert_eq!((e.key.as_str(), e.folder.as_str()), ("rocks", "global"));

        let p = load(&path).unwrap();
        assert!(p.group_text.starts_with(b"<UserInstanceGroup ") && p.group_text.ends_with(b"</UserInstanceGroup>\n"));
        assert_eq!(p.children_text, b"<BUILDING_INSTANCE position=\"vector(1,0,0)\" key=\"rock\" name=\"rock\"/>\n<BUILDING_INSTANCE position=\"vector(-1,0,0)\" key=\"rock\" name=\"rock\"/>\n");

        // TEd's layout, byte for byte.
        assert_eq!(file_text(&p.group_text), FILE.as_bytes());

        // New contents keep the rest of the file.
        update_children(&path, b"<BUILDING_INSTANCE position=\"vector(0,0,0)\" key=\"tree\" name=\"tree\"/>\n").unwrap();
        let now = std::fs::read_to_string(&path).unwrap();
        assert_eq!(now, FILE.replace("                <BUILDING_INSTANCE position=\"vector(1,0,0)\" key=\"rock\" name=\"rock\"/>\n                <BUILDING_INSTANCE position=\"vector(-1,0,0)\" key=\"rock\" name=\"rock\"/>\n", "                <BUILDING_INSTANCE position=\"vector(0,0,0)\" key=\"tree\" name=\"tree\"/>\n"));
    }
}
