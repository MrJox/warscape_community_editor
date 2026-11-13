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

    /// The folder of `key` through any of the editor's file sources.
    ///
    /// # Errors
    ///
    /// No file of the folder, or one doesn't read.
    pub fn read_files(files: &dyn wce_vfs::files::Files, key: &str) -> Result<Self, String> {
        let prefix = folder(key);
        let mut out = BTreeMap::new();
        for path in files.list(&prefix, "") {
            let name = &path[prefix.len()..];
            if !name.contains('/') {
                out.insert(name.to_owned(), files.read(&path)?);
            }
        }

        if out.is_empty() {
            return Err(format!("no file under {prefix}"));
        }

        // The Dawnless Days' numbered copies (`..._destruct01_0.rigid_model_v2` beside `..._destruct01.rigid_model_v2`, docs/re/buildings.md)
        // are never the building's: warped along with their originals, the walls drew every piece twice.
        let copies = out.keys().filter(|n| numbered_copy_of(n).is_some_and(|o| out.contains_key(&o))).cloned().collect::<Vec<_>>();
        for c in copies {
            out.remove(&c);
        }

        Ok(Self { key: key.to_owned(), files: out })
    }

    /// The same files under another key: a file named after the key (`<key>_piece01_destruct01.rigid_model_v2`, `<key>_tech.cs2.parsed`)
    /// takes the new one. The contents are not changed: what they name (pieces, attached buildings) does not depend on the key.
    pub fn renamed(self, key: &str) -> Self {
        let files = self
            .files
            .into_iter()
            .map(|(name, bytes)| match name.strip_prefix(self.key.as_str()) {
                Some(rest) => (format!("{key}{rest}"), bytes),
                None => (name, bytes),
            })
            .collect();
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
/// The file a numbered copy copies: the name less a trailing `_<digits>` before its extension (`a_destruct01_0.meta` -> `a_destruct01.meta`,
/// `key_tech_0.cs2.parsed` -> `key_tech.cs2.parsed`), whether or not it is there.
pub fn numbered_copy_of(name: &str) -> Option<String> {
    let dot = name.find('.')?;
    let (stem, ext) = name.split_at(dot);
    let (base, number) = stem.rsplit_once('_')?;
    (!number.is_empty() && number.bytes().all(|b| b.is_ascii_digit()) && !base.is_empty()).then(|| format!("{base}{ext}"))
}

pub fn is_piece_mesh(name: &str) -> bool {
    name.ends_with(".rigid_model_v2") && !is_collapse(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbered_copies() {
        assert_eq!(
            numbered_copy_of("gondor_fort_straight_piece01_destruct01_0.rigid_model_v2").as_deref(),
            Some("gondor_fort_straight_piece01_destruct01.rigid_model_v2")
        );
        assert_eq!(
            numbered_copy_of("gondor_fort_straight_piece01_destruct02_anim_0.anim").as_deref(),
            Some("gondor_fort_straight_piece01_destruct02_anim.anim")
        );
        assert_eq!(numbered_copy_of("gondor_fort_straight_tech_0.cs2.parsed").as_deref(), Some("gondor_fort_straight_tech.cs2.parsed"));
        assert_eq!(numbered_copy_of("roman_fort_straight_piece01_destruct01.rigid_model_v2"), None);
    }

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
