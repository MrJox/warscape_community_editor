//! TEd's prefabs: saved instance groups in `<kit raw_data>/art/prefabs` (and its folders), each file an `INSTANCE_SELECTION` holding one
//! `UserInstanceGroup`. TEd copies a prefab into a tile as a plain group; the editor takes a group whose name is a prefab's key (its file
//! name, case aside) for a copy of that prefab.
//!
//! The editor's prefabs are `<key>.wce` in the projects (`<projects>/assembly_kit/raw_data/art/prefabs`): the same file (a superset, its root
//! marked, `project_files`), where a prefab placed inside keeps its reference (`wce_prefab`) without its contents. An edit is saved there,
//! never in the kit; a `.wce` takes the place of the kit's `.xml` of the same key. The editor places a prefab as a reference
//! (`project_files::PREFAB_ATTR`), and "Save for Assembly Kit" writes a prefab TEd reads (the references in it baked). A `.wce` can also
//! hold a height layer (`prefab_terrain`), which TEd's prefabs can't: the kit's file goes without it.
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

use crate::prefab_terrain::PrefabTerrain;
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

    /// The editor's `.wce` (else TEd's `.xml`).
    pub wce: bool,
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
        Self::scan_folders(&raw_data_roots.iter().map(|r| prefab_root(r)).collect::<Vec<_>>())
    }

    /// Every `.xml` below these prefab folders (each one like a kit's `art/prefabs`), the first ones first.
    pub fn scan_folders(folders: &[PathBuf]) -> Self {
        let mut lib = Self::default();
        for root in folders {
            let root = root.clone();
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
                    } else if let Some((key, wce)) = name
                        .strip_suffix(".wce")
                        .map(|k| (k, true))
                        .or_else(|| name.strip_suffix(".xml").or_else(|| name.strip_suffix(".XML")).map(|k| (k, false)))
                    {
                        // The editor's .wce sorts before the .xml of the same key in a folder, and takes its place.
                        let folder = path
                            .parent()
                            .and_then(|p| p.strip_prefix(&root).ok())
                            .map(|p| p.to_string_lossy().replace('\\', "/"))
                            .unwrap_or_default();
                        lib.entries.entry(key.to_lowercase()).or_insert(PrefabEntry {
                            key: key.to_owned(),
                            path: path.clone(),
                            folder,
                            root: root.clone(),
                            wce,
                        });
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

    /// A prefab's group contents (at column 0) with the prefabs placed in it filled in (up to 8 deep), as a placed copy holds them.
    pub fn contents(&self, key: &str) -> Option<Vec<u8>> {
        self.contents_at(key, 0)
    }

    fn contents_at(&self, key: &str, depth: u32) -> Option<Vec<u8>> {
        if depth > 8 {
            return None;
        }

        let entry = self.get(key)?;
        let text = std::fs::read(&entry.path).ok()?;
        let (text, _) = crate::project_files::expand_prefabs(text, &|k| self.contents_at(k, depth + 1)).ok()?;
        let scene = TedScene::parse_any(text).ok()?;
        let group = *scene.find_all("UserInstanceGroup").first()?;
        Some(scene.children_text(group))
    }

    /// A prefab's file as the editor reads it: its group's text with the prefabs placed in it filled in.
    ///
    /// # Errors
    ///
    /// No such prefab, or its file does not read.
    pub fn load(&self, key: &str) -> Result<Prefab, String> {
        let entry = self.get(key).ok_or_else(|| format!("no prefab '{key}'"))?;
        let text = std::fs::read(&entry.path).map_err(|e| format!("{}: {e}", entry.path.display()))?;
        let (text, missing) = crate::project_files::expand_prefabs(text, &|k| self.contents(k))?;
        if !missing.is_empty() {
            wce_foundation::log::warn(format!("{}: no prefab {} (placed in it)", entry.path.display(), missing.join(", ")));
        }

        let scene = TedScene::parse_any(text).map_err(|e| format!("{}: {e}", entry.path.display()))?;
        let group =
            *scene.find_all("UserInstanceGroup").first().ok_or_else(|| format!("{}: no UserInstanceGroup", entry.path.display()))?;
        Ok(Prefab { group_text: scene.element_text(group), children_text: scene.children_text(group) })
    }

    /// Saves an edit of a prefab's contents (`children_text`, the editor's: references filled in) into the projects' prefab folder
    /// `projects_root`: its `.wce` there, made from the kit's file the first time. Returns the file.
    ///
    /// # Errors
    ///
    /// No such prefab, or a file can't be read or written.
    pub fn save_edit(&self, key: &str, projects_root: &Path, children_text: &[u8]) -> Result<PathBuf, String> {
        self.save_with(key, projects_root, children_text, None)
    }

    /// A prefab's height layer (`prefab_terrain`), none if it has none (a kit's `.xml` never has).
    ///
    /// # Errors
    ///
    /// No such prefab, or its file or its layer does not read.
    pub fn terrain(&self, key: &str) -> Result<Option<PrefabTerrain>, String> {
        let entry = self.get(key).ok_or_else(|| format!("no prefab '{key}'"))?;
        let text = std::fs::read(&entry.path).map_err(|e| format!("{}: {e}", entry.path.display()))?;
        crate::prefab_terrain::read(&text).map_err(|e| format!("{}: {e}", entry.path.display()))
    }

    /// As [`Self::save_edit`], and the prefab's height layer becomes `terrain` (none: it has none).
    ///
    /// # Errors
    ///
    /// No such prefab, or a file can't be read or written.
    pub fn save_with_terrain(
        &self,
        key: &str,
        projects_root: &Path,
        children_text: &[u8],
        terrain: Option<&PrefabTerrain>,
    ) -> Result<PathBuf, String> {
        self.save_with(key, projects_root, children_text, Some(terrain))
    }

    /// `terrain`: none keeps the file's layer.
    fn save_with(
        &self,
        key: &str,
        projects_root: &Path,
        children_text: &[u8],
        terrain: Option<Option<&PrefabTerrain>>,
    ) -> Result<PathBuf, String> {
        let entry = self.get(key).ok_or_else(|| format!("no prefab '{key}'"))?;
        let target = if entry.wce && entry.path.starts_with(projects_root) {
            entry.path.clone()
        } else {
            let folder = if entry.folder.is_empty() { projects_root.to_path_buf() } else { projects_root.join(&entry.folder) };
            folder.join(format!("{}.wce", entry.key))
        };

        let mut scene = TedScene::parse_any(std::fs::read(&entry.path).map_err(|e| format!("{}: {e}", entry.path.display()))?)
            .map_err(|e| format!("{}: {e}", entry.path.display()))?;
        let group =
            *scene.find_all("UserInstanceGroup").first().ok_or_else(|| format!("{}: no UserInstanceGroup", entry.path.display()))?;
        scene.set_children_text(group, children_text)?;
        if let Some(dir) = target.parent() {
            std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        }

        let mut text = wce_file_text(scene.save(), Some(&|k: &str| self.contents(k)));
        if let Some(terrain) = terrain {
            text = crate::prefab_terrain::with_terrain(text, terrain).map_err(|e| format!("{}: {e}", target.display()))?;
        }

        std::fs::write(&target, text).map_err(|e| format!("{}: {e}", target.display()))?;
        Ok(target)
    }

    /// A copy of a prefab (the kit's or the projects') as a new one of the projects: `<projects_root>/<folder>/<new_key>.wce`, its group named
    /// after it, its height layer and the references in it kept. Returns the file.
    ///
    /// # Errors
    ///
    /// No such prefab, a bad or taken key, or a file can't be read or written.
    pub fn copy_to(&self, key: &str, projects_root: &Path, new_key: &str, folder: &str) -> Result<PathBuf, String> {
        let new_key = check_key(new_key)?;
        if self.get(&new_key).is_some() {
            return Err(format!("a prefab '{new_key}' exists"));
        }

        let entry = self.get(key).ok_or_else(|| format!("no prefab '{key}'"))?;
        let folder = folder.trim().trim_matches(['/', '\\']);
        let target = if folder.is_empty() {
            projects_root.join(format!("{new_key}.wce"))
        } else {
            projects_root.join(folder).join(format!("{new_key}.wce"))
        };
        let mut scene = TedScene::parse_any(std::fs::read(&entry.path).map_err(|e| format!("{}: {e}", entry.path.display()))?)
            .map_err(|e| format!("{}: {e}", entry.path.display()))?;
        let group =
            *scene.find_all("UserInstanceGroup").first().ok_or_else(|| format!("{}: no UserInstanceGroup", entry.path.display()))?;
        scene.set_attr(group, "name", &new_key);
        if let Some(dir) = target.parent() {
            std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        }

        std::fs::write(&target, crate::project_files::mark_wce_text(scene.save())).map_err(|e| format!("{}: {e}", target.display()))?;
        Ok(target)
    }

    /// Renames a prefab of the projects (its file and its group; in the same folder). The tiles' references to the old key are not changed.
    /// Returns the new file.
    ///
    /// # Errors
    ///
    /// No such prefab, not one of the projects', a bad or taken key, or a file can't be written or removed.
    pub fn rename(&self, key: &str, projects_root: &Path, new_key: &str) -> Result<PathBuf, String> {
        let entry = self.project_entry(key, projects_root)?;
        let target = self.copy_to(key, projects_root, new_key, &entry.folder)?;
        std::fs::remove_file(&entry.path).map_err(|e| format!("{}: {e}", entry.path.display()))?;
        Ok(target)
    }

    /// Deletes a prefab of the projects (its `.wce`; a kit's prefab of the same key is the one found again). Returns the file removed.
    ///
    /// # Errors
    ///
    /// No such prefab, not one of the projects', or the file can't be removed.
    pub fn delete(&self, key: &str, projects_root: &Path) -> Result<PathBuf, String> {
        let entry = self.project_entry(key, projects_root)?;
        std::fs::remove_file(&entry.path).map_err(|e| format!("{}: {e}", entry.path.display()))?;
        Ok(entry.path.clone())
    }

    fn project_entry(&self, key: &str, projects_root: &Path) -> Result<&PrefabEntry, String> {
        let entry = self.get(key).ok_or_else(|| format!("no prefab '{key}'"))?;
        if !entry.wce || !entry.path.starts_with(projects_root) {
            return Err(format!("{key} is the Assembly Kit's: only the projects' prefabs are renamed or deleted (duplicate it first)"));
        }

        Ok(entry)
    }

    /// "Save for Assembly Kit" of a prefab: its file as TEd reads it (the prefabs placed in it baked) in the kit's prefab folder `kit_root`,
    /// at the same folder, as `<key>.xml`, its buildings marked Export as prop as their models (`export_as_prop`; `models_in`: the game's
    /// files). A file it replaces with other bytes keeps a `.bak`. Returns the file and the conversion's report.
    ///
    /// # Errors
    ///
    /// No such prefab, or a file can't be read, converted or written.
    pub fn save_for_kit(
        &self,
        key: &str,
        kit_root: &Path,
        models_in: Option<crate::export_as_prop::ModelsIn>,
    ) -> Result<(PathBuf, crate::project_files::TedReport), String> {
        let entry = self.get(key).ok_or_else(|| format!("no prefab '{key}'"))?;
        let text = std::fs::read(&entry.path).map_err(|e| format!("{}: {e}", entry.path.display()))?;
        let (text, _) = crate::project_files::expand_prefabs(text, &|k| self.contents(k))?;
        let (text, props) = crate::export_as_prop::apply(text, models_in)?;
        let (ted, mut report) = crate::project_files::to_ted(text)?;
        props.add_to(&mut report);
        let folder = if entry.folder.is_empty() { kit_root.to_path_buf() } else { kit_root.join(&entry.folder) };
        std::fs::create_dir_all(&folder).map_err(|e| format!("{}: {e}", folder.display()))?;
        let target = folder.join(format!("{}.xml", entry.key));
        match std::fs::read(&target) {
            Ok(old) if old == ted => return Ok((target, report)),
            Ok(_) => {
                std::fs::copy(&target, target.with_extension("xml.bak")).map_err(|e| e.to_string())?;
            }

            Err(_) => {}
        }

        std::fs::write(&target, ted).map_err(|e| format!("{}: {e}", target.display()))?;
        Ok((target, report))
    }
}

/// A prefab's key as a file name takes it: trimmed, not empty, no slashes, dots or characters files can't have.
///
/// # Errors
///
/// It is empty or has such a character.
pub fn check_key(key: &str) -> Result<String, String> {
    let key = key.trim();
    if key.is_empty() || key.contains(['/', '\\', '.', ':', '*', '?', '"', '<', '>', '|']) {
        return Err(format!("'{key}' can't be a prefab's name: no slashes, dots or : * ? \" < > |"));
    }

    Ok(key.to_owned())
}

/// A prefab file's text as the editor keeps it: the references' contents left out (what they differ by from `base`, their prefabs'
/// contents, kept as their overrides), the root marked.
fn wce_file_text(text: Vec<u8>, base: Option<crate::project_files::PrefabContents<'_>>) -> Vec<u8> {
    crate::project_files::mark_wce_text(crate::project_files::strip_prefab_contents_with(text, base).0)
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
    let scene = TedScene::parse_any(std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?)
        .map_err(|e| format!("{}: {e}", path.display()))?;
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

/// Writes a new prefab file (its folders too): a `.wce` as the editor keeps it (the references in it keep what they differ by from `base`,
/// their prefabs' contents), an `.xml` as TEd's.
///
/// # Errors
///
/// The file can't be written.
pub fn write(path: &Path, group_text: &[u8], base: Option<crate::project_files::PrefabContents<'_>>) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }

    let text = file_text(group_text);
    let text = if path.extension().is_some_and(|x| x == "wce") { wce_file_text(text, base) } else { text };
    std::fs::write(path, text).map_err(|e| format!("{}: {e}", path.display()))
}

/// Replaces the contents of a prefab file's group, keeping the rest of the file as it is.
///
/// # Errors
///
/// The file can't be read or written, or holds no `UserInstanceGroup`.
pub fn update_children(path: &Path, children_text: &[u8]) -> Result<(), String> {
    let mut scene = TedScene::parse_any(std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?)
        .map_err(|e| format!("{}: {e}", path.display()))?;
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

    #[test]
    fn an_edit_goes_to_the_projects_wce_which_takes_the_kits_place() {
        let kit = tempfile::tempdir().unwrap();
        let projects = tempfile::tempdir().unwrap();
        let path = prefab_root(kit.path()).join("global").join("rocks.xml");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, FILE).unwrap();
        let lib = PrefabLibrary::scan(&[projects.path().to_path_buf(), kit.path().to_path_buf()]);
        let saved = lib
            .save_edit(
                "rocks",
                &prefab_root(projects.path()),
                b"<BUILDING_INSTANCE position=\"vector(2,0,0)\" key=\"rock\" name=\"rock\"/>\n",
            )
            .unwrap();
        assert_eq!(saved, prefab_root(projects.path()).join("global").join("rocks.wce"));
        assert_eq!(std::fs::read(&path).unwrap(), FILE.as_bytes(), "the kit's file is not touched");
        let text = String::from_utf8(std::fs::read(&saved).unwrap()).unwrap();
        assert!(text.starts_with("<INSTANCE_SELECTION version=\"17\" wce_version=\"1\">") && text.contains("vector(2,0,0)"), "{text}");

        let lib = PrefabLibrary::scan(&[projects.path().to_path_buf(), kit.path().to_path_buf()]);
        let e = lib.get("rocks").unwrap();
        assert!(e.wce && e.path == saved, "{e:?}");
        assert_eq!(lib.contents("rocks").unwrap(), b"<BUILDING_INSTANCE position=\"vector(2,0,0)\" key=\"rock\" name=\"rock\"/>\n");

        // For the kit: TEd's file again, at the kit's place.
        let (back, _) = lib.save_for_kit("rocks", &prefab_root(kit.path()), None).unwrap();
        assert_eq!(back, path);
        let ted = String::from_utf8(std::fs::read(&path).unwrap()).unwrap();
        assert!(!ted.contains("wce_") && ted.contains("vector(2,0,0)"), "{ted}");
    }

    #[test]
    fn a_height_layer_stays_through_edits_and_does_not_go_to_the_kit() {
        let (kit, projects) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let path = prefab_root(kit.path()).join("rocks.xml");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, FILE).unwrap();
        let lib = PrefabLibrary::scan(&[projects.path().to_path_buf(), kit.path().to_path_buf()]);
        assert_eq!(lib.terrain("rocks").unwrap(), None);

        let mut t = PrefabTerrain::empty([-1.0, -1.0], 1.0, 3, 3);
        (t.heights[4], t.coverage[4], t.normalize) = (2.0, 1.0, true);
        let children = b"<BUILDING_INSTANCE position=\"vector(1,0,0)\" key=\"rock\" name=\"rock\"/>\n";
        let saved = lib.save_with_terrain("rocks", &prefab_root(projects.path()), children, Some(&t)).unwrap();
        let lib = PrefabLibrary::scan(&[projects.path().to_path_buf(), kit.path().to_path_buf()]);
        let kept = t.cropped().unwrap();
        assert_eq!(lib.terrain("rocks").unwrap().as_ref(), Some(&kept));
        assert_eq!(lib.contents("rocks").unwrap(), children, "the layer is not part of the contents");

        // An edit of the contents keeps the layer; one that sets none takes it away.
        lib.save_edit(
            "rocks",
            &prefab_root(projects.path()),
            b"<BUILDING_INSTANCE position=\"vector(3,0,0)\" key=\"rock\" name=\"rock\"/>\n",
        )
        .unwrap();
        assert_eq!(lib.terrain("rocks").unwrap().as_ref(), Some(&kept));

        let (ted, report) = lib.save_for_kit("rocks", &prefab_root(kit.path()), None).unwrap();
        let ted = std::fs::read_to_string(ted).unwrap();
        assert!(!ted.contains("Wce") && ted.contains("vector(3,0,0)"), "{ted}");
        assert!(report.dropped.iter().any(|l| l.contains("height layer")), "{report:?}");

        lib.save_with_terrain("rocks", &prefab_root(projects.path()), children, None).unwrap();
        assert_eq!(lib.terrain("rocks").unwrap(), None);
        assert!(!std::fs::read_to_string(saved).unwrap().contains("Wce"));
    }

    #[test]
    fn a_prefab_is_duplicated_renamed_and_deleted_in_the_projects_only() {
        let (kit, projects) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let kit_file = prefab_root(kit.path()).join("rocks.xml");
        std::fs::create_dir_all(kit_file.parent().unwrap()).unwrap();
        std::fs::write(&kit_file, FILE).unwrap();
        let root = prefab_root(projects.path());
        let scan = || PrefabLibrary::scan(&[projects.path().to_path_buf(), kit.path().to_path_buf()]);

        // A kit's prefab is copied, not renamed or deleted.
        let lib = scan();
        assert!(lib.rename("rocks", &root, "stones").is_err() && lib.delete("rocks", &root).is_err());
        assert!(lib.copy_to("rocks", &root, "a/b", "").is_err() && lib.copy_to("rocks", &root, "ROCKS", "").is_err());
        let copy = lib.copy_to("rocks", &root, "stones", "global").unwrap();
        assert_eq!(copy, root.join("global").join("stones.wce"));
        let text = std::fs::read_to_string(&copy).unwrap();
        assert!(text.contains("wce_version=\"1\"") && text.contains("name=\"stones\"") && text.contains("key=\"rock\""), "{text}");

        let lib = scan();
        assert_eq!(lib.contents("stones"), lib.contents("rocks"));
        let renamed = lib.rename("stones", &root, "pebbles").unwrap();
        assert!(!copy.exists() && renamed == root.join("global").join("pebbles.wce"));
        let lib = scan();
        assert!(lib.get("stones").is_none() && lib.get("pebbles").is_some());
        lib.delete("pebbles", &root).unwrap();
        assert!(scan().get("pebbles").is_none() && kit_file.exists());
    }

    #[test]
    fn a_prefab_for_the_kit_writes_its_props_as_models() {
        let (kit, projects) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let root = prefab_root(projects.path());
        std::fs::create_dir_all(&root).unwrap();
        let text = "<INSTANCE_SELECTION version=\"17\" wce_version=\"1\">\n    <BUILDING_INSTANCE position=\"vector(2,0,0)\" key=\"house\" name=\"house\" wce_export_as_prop=\"true\"/>\n    <BUILDING_INSTANCE position=\"vector(3,0,0)\" key=\"tower\" name=\"tower\" capture_location=\"cl\" wce_export_as_prop=\"true\"/>\n</INSTANCE_SELECTION>\n";
        std::fs::write(root.join("pair.wce"), text).unwrap();
        let lib = PrefabLibrary::scan(&[projects.path().to_path_buf()]);
        let models_in = |folder: &str| {
            vec![format!("{folder}house_piece01_destruct01.rigid_model_v2"), format!("{folder}house_piece02_destruct01.rigid_model_v2")]
        };
        let (path, report) = lib.save_for_kit("pair", &prefab_root(kit.path()), Some(&models_in)).unwrap();
        let ted = String::from_utf8(std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(ted.matches("<SIMPLE_MESH_INSTANCE").count(), 2, "{ted}");
        assert!(
            ted.contains("key=\"rigidmodels/buildings/house/house_piece02_destruct01.rigid_model_v2\"") && ted.contains("key=\"tower\""),
            "{ted}"
        );
        assert!(!ted.contains("wce_export_as_prop"), "{ted}");
        assert!(
            report.polyfilled.iter().any(|l| l.contains("1 building(s) written as 2 model(s)"))
                && report.polyfilled.iter().any(|l| l.contains("water planes")),
            "{report:?}"
        );
        assert!(report.dropped.iter().any(|l| l.contains("'tower'") && l.contains("capture location")), "{report:?}");

        // Without the game's files both stay buildings, and the report says so.
        let (path, report) = lib.save_for_kit("pair", &prefab_root(kit.path()), None).unwrap();
        assert_eq!(String::from_utf8(std::fs::read(&path).unwrap()).unwrap().matches("<BUILDING_INSTANCE").count(), 2);
        assert_eq!(report.dropped.len(), 2, "{report:?}");
    }
}
