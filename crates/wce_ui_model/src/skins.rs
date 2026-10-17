//! UI skins: the folders under `ui/skins/`. A picture named `ui/skins/default/x.png` is taken from `ui/skins/<skin>/x.png` when that exists,
//! and from the default skin otherwise.

use std::collections::BTreeMap;

use wce_vfs::Vfs;

use crate::images;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Skin {
    pub name: String,
    /// Pictures in the skin's folder.
    pub files: usize,
}

/// The skins of the mounted packs: every folder under `ui/skins/`, `default` first.
pub fn list(vfs: &Vfs) -> Vec<Skin> {
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for p in vfs.files().keys() {
        if let Some((name, _)) = p.strip_prefix("ui/skins/").and_then(|r| r.split_once('/')) {
            *counts.entry(name.to_string()).or_default() += 1;
        }
    }

    let mut v: Vec<Skin> = counts.into_iter().map(|(name, files)| Skin { name, files }).collect();
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
