//! Vegetation sets: named lists of tree keys to place or paint together, each with a weight (how often it is picked) and, if it has one, a
//! scale range of its own. The editor's files, like its prefabs: `<name>.wce` in the projects' `raw_data/art/vegetation_sets` (folders
//! below it are kept), with its root marked (`project_files`):
//!
//! ```text
//! <VEGETATION_SET wce_version="1">
//!     <entry key="oak.oak_a" weight="3"/>
//!     <entry key="birch.birch_b" weight="1" scale_min="0.8" scale_max="1.1"/>
//! </VEGETATION_SET>
//! ```

use std::path::{Path, PathBuf};

use wce_formats::xml_doc::XmlDoc;

/// One tree of a set.
#[derive(Clone, Debug, PartialEq)]
pub struct SetEntry {
    pub key: String,
    pub weight: f32,

    /// Its own scale range (else the brush's).
    pub scale: Option<(f32, f32)>,
}

/// A set: its name (the file's), the folder below `vegetation_sets` it is in, its file and its trees.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct VegetationSet {
    pub name: String,
    pub folder: String,
    pub path: PathBuf,
    pub entries: Vec<SetEntry>,
}

/// The folder of the sets in a raw_data folder (the projects').
pub fn sets_root(raw_data: &Path) -> PathBuf {
    raw_data.join("art").join("vegetation_sets")
}

impl VegetationSet {
    /// A set of these keys, each weighted by how many times it is there (the selected trees).
    pub fn from_keys<'a>(name: &str, keys: impl IntoIterator<Item = &'a str>) -> Self {
        let mut set = Self { name: name.to_owned(), ..Default::default() };
        for k in keys {
            match set.entries.iter_mut().find(|e| e.key == k) {
                Some(e) => e.weight += 1.0,
                None => set.entries.push(SetEntry { key: k.to_owned(), weight: 1.0, scale: None }),
            }
        }

        set
    }

    /// A tree of the set, picked by the weights (`r` in [0, 1)).
    pub fn pick(&self, r: f32) -> Option<&SetEntry> {
        let total = self.entries.iter().map(|e| e.weight.max(0.0)).sum::<f32>();
        if total <= 0.0 {
            return self.entries.first();
        }

        let mut at = r * total;
        for e in &self.entries {
            at -= e.weight.max(0.0);
            if at < 0.0 {
                return Some(e);
            }
        }

        self.entries.last()
    }

    /// Reads a set file.
    ///
    /// # Errors
    ///
    /// The file can't be read or is not a set.
    pub fn load(path: &Path, root: &Path) -> Result<Self, String> {
        let doc = XmlDoc::parse(std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?).map_err(|e| format!("{}: {e}", path.display()))?;
        let els = doc.elements();
        if els.first().is_none_or(|e| e.name != "VEGETATION_SET") {
            return Err(format!("{}: not a vegetation set", path.display()));
        }

        let num = |e: usize, n: &str| doc.get(e, n).and_then(|v| v.trim().parse::<f32>().ok());
        let entries = (0..els.len()).filter(|&e| els[e].name == "entry").filter_map(|e| {
            let key = doc.get(e, "key").filter(|k| !k.is_empty())?;
            let scale = num(e, "scale_min").zip(num(e, "scale_max"));
            Some(SetEntry { key, weight: num(e, "weight").unwrap_or(1.0), scale })
        }).collect();

        Ok(Self { name: name_of(path), folder: folder_of(path, root), path: path.to_path_buf(), entries })
    }

    /// The file's text.
    pub fn text(&self) -> Vec<u8> {
        let mut out = format!("<VEGETATION_SET {}=\"{}\">\n", crate::project_files::WCE_VERSION_ATTR, crate::project_files::WCE_VERSION);
        for e in &self.entries {
            out += &format!("    <entry key=\"{}\" weight=\"{}\"", e.key, crate::ted_scene::format_number(f64::from(e.weight)));
            if let Some((lo, hi)) = e.scale {
                out += &format!(" scale_min=\"{}\" scale_max=\"{}\"", crate::ted_scene::format_number(f64::from(lo)), crate::ted_scene::format_number(f64::from(hi)));
            }

            out += "/>\n";
        }

        out += "</VEGETATION_SET>\n";
        out.into_bytes()
    }

    /// Writes the set to its file (its folders too).
    ///
    /// # Errors
    ///
    /// The file can't be written.
    pub fn save(&self) -> Result<(), String> {
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        }

        std::fs::write(&self.path, self.text()).map_err(|e| format!("{}: {e}", self.path.display()))
    }
}

fn name_of(path: &Path) -> String {
    path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()
}

fn folder_of(path: &Path, root: &Path) -> String {
    path.parent().and_then(|p| p.strip_prefix(root).ok()).map(|p| p.to_string_lossy().replace('\\', "/")).unwrap_or_default()
}

/// Every set below `root` (`.wce` files), by folder and name; ones that don't read are left out.
pub fn scan(root: &Path) -> Vec<VegetationSet> {
    let mut out = vec![];
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(read) = std::fs::read_dir(&dir) else { continue };
        for path in read.flatten().map(|e| e.path()) {
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|x| x == "wce")
                && let Ok(set) = VegetationSet::load(&path, root)
            {
                out.push(set);
            }
        }
    }

    out.sort_by_cached_key(|a| (a.folder.to_lowercase(), a.name.to_lowercase()));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_set_is_made_saved_read_and_picked_from() {
        let dir = tempfile::tempdir().unwrap();
        let root = sets_root(dir.path());
        let mut set = VegetationSet::from_keys("oaks", ["oak.oak_a", "oak.oak_b", "oak.oak_a", "oak.oak_a"]);
        set.path = root.join("forest").join("oaks.wce");
        set.entries[1].scale = Some((0.8, 1.2));
        set.save().unwrap();

        let all = scan(&root);
        assert_eq!(all.len(), 1);
        let back = &all[0];
        assert_eq!((back.name.as_str(), back.folder.as_str()), ("oaks", "forest"));
        assert_eq!(back.entries, set.entries, "weights 3 and 1, the second with its scale");

        // 3 to 1: the first quarter of [0, 1) and more is oak_a.
        assert_eq!(back.pick(0.0).unwrap().key, "oak.oak_a");
        assert_eq!(back.pick(0.74).unwrap().key, "oak.oak_a");
        assert_eq!(back.pick(0.76).unwrap().key, "oak.oak_b");
    }
}
