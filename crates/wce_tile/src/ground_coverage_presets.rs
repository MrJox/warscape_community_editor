//! The ground coverage's presets (`ground_coverage`): named values for a kind of terrain, its parameters and its rock and plant sets (by
//! name). The editor's files, like the vegetation sets: `<name>.wce` in the projects' `raw_data/art/ground_coverage_presets` (folders below
//! it are kept), with its root marked (`project_files`):
//!
//! ```text
//! <GROUND_COVERAGE_PRESET wce_version="1" kind="coast" rocks="coast rocks" plants="" params="slope_deg=35 min_rise=2 ..."/>
//! ```
//!
//! Each kind also has a built-in preset ([`Preset::builtin`]: its default parameters, no sets), which is no file.

use std::path::{Path, PathBuf};

use wce_formats::xml_doc::XmlDoc;

use crate::ground_coverage::{Kind, Params};

/// The built-in presets' name.
pub const BUILTIN: &str = "Default";

/// A preset: its name (the file's), the folder below `ground_coverage_presets` it is in, its file (none: built in), its kind and values.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Preset {
    pub name: String,
    pub folder: String,
    pub path: Option<PathBuf>,
    pub kind: Kind,
    pub params: Params,

    /// The sets' names (empty: none).
    pub rocks: String,
    pub plants: String,
}

/// The folder of the presets in a raw_data folder (the projects').
pub fn presets_root(raw_data: &Path) -> PathBuf {
    raw_data.join("art").join("ground_coverage_presets")
}

impl Preset {
    /// A kind's built-in preset: its default parameters, no sets.
    pub fn builtin(kind: Kind) -> Self {
        Self { name: BUILTIN.to_owned(), kind, params: Params::for_kind(kind), ..Default::default() }
    }

    pub fn is_builtin(&self) -> bool {
        self.path.is_none()
    }

    /// Whether the values are the preset's.
    pub fn same_values(&self, params: &Params, rocks: &str, plants: &str) -> bool {
        self.params == *params && self.rocks == rocks && self.plants == plants
    }

    /// Reads a preset file.
    ///
    /// # Errors
    ///
    /// The file can't be read, is not a preset, or its kind is unknown.
    pub fn load(path: &Path, root: &Path) -> Result<Self, String> {
        let doc = XmlDoc::parse(std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?).map_err(|e| format!("{}: {e}", path.display()))?;
        if doc.elements().first().is_none_or(|e| e.name != "GROUND_COVERAGE_PRESET") {
            return Err(format!("{}: not a ground coverage preset", path.display()));
        }

        let kind = doc.get(0, "kind").unwrap_or_default();
        let kind = Kind::from_name(&kind).ok_or_else(|| format!("{}: no kind '{kind}'", path.display()))?;
        let params = doc.get(0, "params").map_or_else(|| Params::for_kind(kind), |t| Params::from_text_for(kind, &t));
        Ok(Self {
            name: name_of(path),
            folder: folder_of(path, root),
            path: Some(path.to_path_buf()),
            kind,
            params,
            rocks: doc.get(0, "rocks").unwrap_or_default(),
            plants: doc.get(0, "plants").unwrap_or_default(),
        })
    }

    /// The file's text.
    pub fn text(&self) -> Vec<u8> {
        format!(
            "<GROUND_COVERAGE_PRESET {}=\"{}\" kind=\"{}\" rocks=\"{}\" plants=\"{}\" params=\"{}\"/>\n",
            crate::project_files::WCE_VERSION_ATTR, crate::project_files::WCE_VERSION, self.kind.name(), escape(&self.rocks), escape(&self.plants),
            escape(&self.params.to_text()),
        )
        .into_bytes()
    }

    /// Writes the preset to its file (its folders too).
    ///
    /// # Errors
    ///
    /// A built-in preset (no file), or the file can't be written.
    pub fn save(&self) -> Result<(), String> {
        let path = self.path.as_ref().ok_or("a built-in preset has no file")?;
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        }

        std::fs::write(path, self.text()).map_err(|e| format!("{}: {e}", path.display()))
    }
}

/// An attribute's value as XML text.
fn escape(v: &str) -> String {
    v.replace('&', "&amp;").replace('"', "&quot;").replace('<', "&lt;").replace('>', "&gt;")
}

fn name_of(path: &Path) -> String {
    path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()
}

fn folder_of(path: &Path, root: &Path) -> String {
    path.parent().and_then(|p| p.strip_prefix(root).ok()).map(|p| p.to_string_lossy().replace('\\', "/")).unwrap_or_default()
}

/// Every preset below `root` (`.wce` files), by kind, folder and name; ones that don't read are left out.
pub fn scan(root: &Path) -> Vec<Preset> {
    let mut out = vec![];
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(read) = std::fs::read_dir(&dir) else { continue };
        for path in read.flatten().map(|e| e.path()) {
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|x| x == "wce")
                && let Ok(p) = Preset::load(&path, root)
            {
                out.push(p);
            }
        }
    }

    out.sort_by_cached_key(|p| (p.kind, p.folder.to_lowercase(), p.name.to_lowercase()));
    out
}

/// A kind's presets: its built-in one, then its files'.
pub fn of_kind(all: &[Preset], kind: Kind) -> Vec<Preset> {
    std::iter::once(Preset::builtin(kind)).chain(all.iter().filter(|p| p.kind == kind).cloned()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_preset_is_saved_read_and_listed_by_kind() {
        let dir = tempfile::tempdir().unwrap();
        let root = presets_root(dir.path());
        let mut p = Preset::builtin(Kind::Coast);
        p.name = "North & \"rocky\" coast".to_owned();
        p.path = Some(root.join("north").join("rocky.wce"));
        p.params.min_rise = 4.5;
        p.params.water_below = 1.25;
        (p.rocks, p.plants) = ("rocks & stones".to_owned(), "<reeds>".to_owned());
        p.save().unwrap();
        let mut cliffs = Preset::builtin(Kind::Cliffs);
        cliffs.path = Some(root.join("cliffs.wce"));
        cliffs.save().unwrap();

        let all = scan(&root);
        assert_eq!(all.len(), 2);
        let coast = of_kind(&all, Kind::Coast);
        assert_eq!(coast.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(), [BUILTIN, "rocky"], "the built-in first, the file's name");
        let back = &coast[1];
        assert_eq!((back.kind, back.folder.as_str()), (Kind::Coast, "north"));
        assert_eq!((back.rocks.as_str(), back.plants.as_str()), ("rocks & stones", "<reeds>"), "escaped and read back");
        assert_eq!(back.params, p.params);
        assert!(back.same_values(&p.params, &p.rocks, &p.plants));
        assert!(coast[0].is_builtin() && !back.is_builtin());
        assert_eq!(of_kind(&all, Kind::Riverbank).len(), 1, "only the built-in");
    }

    #[test]
    fn a_built_in_preset_has_no_file() {
        assert!(Preset::builtin(Kind::Cliffs).save().is_err());
        assert_eq!(Preset::builtin(Kind::Riverbank).params, Params::for_kind(Kind::Riverbank));
    }
}
