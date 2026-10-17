//! UI skins: the folders under `ui/skins/`. A picture named `ui/skins/default/x.png` is taken from `ui/skins/<skin>/x.png` when that exists,
//! and from the default skin otherwise.

use std::collections::BTreeMap;
use std::path::Path;

use wce_vfs::Vfs;

use crate::images;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Skin {
    pub name: String,
    /// Pictures in the skin: those of the packs plus those of the loose folder.
    pub files: usize,
    /// There is a folder for it on disk (the user's own, read before the packs).
    pub loose: bool,
    /// The mounted packs have pictures for it.
    pub packed: bool,
}

/// The names of the skin folders under `<project>/ui/skins`.
pub fn loose_names(project: &Path) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(project.join("ui").join("skins"))
        .map(|d| d.filter_map(|e| e.ok()).filter(|e| e.path().is_dir()).map(|e| e.file_name().to_string_lossy().to_lowercase()).collect())
        .unwrap_or_default();
    v.sort();
    v
}

fn count_files(dir: &Path) -> usize {
    std::fs::read_dir(dir).map(|d| d.filter_map(|e| e.ok()).map(|e| if e.path().is_dir() { count_files(&e.path()) } else { 1 }).sum()).unwrap_or(0)
}

fn entry<'a>(map: &'a mut BTreeMap<String, Skin>, name: &str) -> &'a mut Skin {
    map.entry(name.to_string()).or_insert_with(|| Skin { name: name.to_string(), files: 0, loose: false, packed: false })
}

/// The skins: every folder under `ui/skins/` of the mounted packs and of the project's loose folder, `default` first. A skin in both is
/// listed once, as loose and packed: the loose pictures are read first, the pack's fill in the rest.
pub fn list(vfs: &Vfs, project: Option<&Path>) -> Vec<Skin> {
    let mut map: BTreeMap<String, Skin> = BTreeMap::new();
    for p in vfs.files().keys() {
        if let Some((name, _)) = p.strip_prefix("ui/skins/").and_then(|r| r.split_once('/')) {
            let s = entry(&mut map, name);
            for &i in vfs.owners(p) {
                if vfs.packs().get(i).is_some_and(|pk| pk.loose) {
                    s.loose = true;
                } else {
                    s.packed = true;
                    s.files += 1;
                    break;
                }
            }
        }
    }

    if let Some(pd) = project {
        for name in loose_names(pd) {
            let n = count_files(&pd.join("ui").join("skins").join(&name));
            let s = entry(&mut map, &name);
            s.loose = true;
            s.files += n;
        }
    }

    let mut v: Vec<Skin> = map.into_values().collect();
    v.sort_by(|a, b| (b.name == "default").cmp(&(a.name == "default")).then(a.name.cmp(&b.name)));
    v
}

/// The paths to try for a picture drawn with `skin`: the skin's version of a `ui/skins/default/` picture first, then the picture itself,
/// each with the other picture extensions.
pub fn candidates(path: &str, skin: Option<&str>) -> Vec<String> {
    let mut v = Vec::new();
    if let Some(s) = skin.filter(|s| !s.is_empty() && *s != "default") {
        if let Some(rest) = path.strip_prefix("ui/skins/default/") {
            v.extend(images::candidates(&format!("ui/skins/{s}/{rest}")));
        }
    }

    for c in images::candidates(path) {
        if !v.contains(&c) {
            v.push(c);
        }
    }

    v
}
