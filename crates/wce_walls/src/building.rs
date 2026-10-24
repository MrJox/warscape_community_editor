//! A building's folder, `rigidmodels/buildings/<key>/`: its piece models, collapses, effects metadata, logic file and zone file
//! (docs/re/buildings.md, "The files of a building folder").

use std::collections::BTreeMap;

use wce_vfs::Vfs;

/// The files of a building folder by file name.
#[derive(Clone, Debug)]
pub struct BuildingFiles {
    pub key: String,
    pub files: BTreeMap<String, Vec<u8>>,
}

impl BuildingFiles {

    /// The folder of `key` as the packs have it (each file from the pack with the highest priority).
    ///
    /// # Errors
    ///
    /// No pack has the folder, or a file of it doesn't read.
    pub fn read(vfs: &Vfs, key: &str) -> Result<Self, String> {
        let prefix = folder(key);
        let mut files = BTreeMap::new();
        for path in vfs.list(&prefix, "") {
            let name = &path[prefix.len()..];
            if name.contains('/') {
                continue;
            }

            files.insert(name.to_owned(), vfs.read(path).map_err(|e| e.to_string())?);
        }

        if files.is_empty() {
            return Err(format!("no pack has {prefix}"));
        }

        Ok(Self { key: key.to_owned(), files })
    }

    /// The same files under another key: a file named after the key (`<key>_piece01_destruct01.rigid_model_v2`, `<key>_tech.cs2.parsed`)
    /// takes the new one. The contents are not changed: what they name (pieces, attached buildings) does not depend on the key.
    pub fn renamed(self, key: &str) -> Self {
        let files = self.files.into_iter().map(|(name, bytes)| match name.strip_prefix(self.key.as_str()) {
            Some(rest) => (format!("{key}{rest}"), bytes),
            None => (name, bytes),
        }).collect();
        Self { key: key.to_owned(), files }
    }

    /// The building's logic file name.
    pub fn logic_name(&self) -> String {
        format!("{}_tech.cs2.parsed", self.key)
    }

    /// The building's zone file name.
    pub fn zones_name(&self) -> String {
        format!("{}_zone_tech.xml", self.key)
    }

    /// Leaves out the collapses (`<piece>_destructNN_anim.*`, `_destructNN_destruction_weighted_anim.*`): a level then swaps to the next
    /// with no animation.
    pub fn without_collapses(mut self) -> Self {
        self.files.retain(|name, _| !is_collapse(name));
        self
    }

    /// The files by their paths in a pack.
    pub fn into_pack_files(self) -> Vec<(String, Vec<u8>)> {
        let prefix = folder(&self.key);
        self.files.into_iter().map(|(name, bytes)| (format!("{prefix}{name}"), bytes)).collect()
    }
}

/// The folder of a building in the packs.
pub fn folder(key: &str) -> String {
    format!("rigidmodels/buildings/{key}/")
}

/// If a file of a building folder is a collapse (from one destruction level to the next).
pub fn is_collapse(name: &str) -> bool {
    let Some(at) = name.find("_destruct") else { return false };
    let rest = name[at + 9..].trim_start_matches(|c: char| c.is_ascii_digit());
    let stem = rest.split('.').next().unwrap_or_default();
    stem == "_anim" || stem == "_destruction_weighted_anim"
}

/// If a file of a building folder is a piece's mesh at a level (the standing piece, not a collapse).
pub fn is_piece_mesh(name: &str) -> bool {
    name.ends_with(".rigid_model_v2") && !is_collapse(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collapses() {
        assert!(is_collapse("roman_fort_straight_piece01_destruct01_anim.rigid_model_v2"));
        assert!(is_collapse("roman_fort_straight_piece01_destruct02_anim.anim"));
        assert!(is_collapse("roman_fort_straight_piece01_destruct02_anim.meta"));
        assert!(is_collapse("battering_ram_piece01_destruct01_destruction_weighted_anim.rigid_model_v2"));
        assert!(!is_collapse("roman_fort_straight_piece01_destruct01.rigid_model_v2"));
        assert!(!is_collapse("roman_fort_straight_piece01_destruct01.rigid_model_v2.meta"));
        assert!(!is_collapse("roman_fort_gateway_piece01_destruct01_gate_opening_anim.anim"));
        assert!(!is_collapse("roman_fort_straight_tech.cs2.parsed"));
    }
}
