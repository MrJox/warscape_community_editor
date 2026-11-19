//! Native resource-key, content-retention and provenance checks; disk observations are separate.
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::File,
    io,
    path::Path,
};
use wce_terrain::MapModel;
use wce_terrain_io::{MapFiles, built};
use wce_tile_model::Blob;

fn key(rel: &str) -> Result<String, String> {
    wce_foundation::publication::resource_path(Path::new(""), rel)?;
    Ok(rel.to_uppercase())
}

fn keys<'a>(names: impl IntoIterator<Item = &'a str>) -> Result<BTreeMap<String, &'a str>, String> {
    let mut out = BTreeMap::new();
    let mut directories = BTreeMap::new();
    for name in names {
        let normalized = key(name)?;
        if let Some(other) = out.insert(normalized, name) {
            return Err(format!("Cannot save: resource keys '{other}' and '{name}' collide"));
        }
        let mut parent = name;
        while let Some((p, _)) = parent.rsplit_once('/') {
            if let Some(other) = directories.insert(p.to_uppercase(), p)
                && other != p
            {
                return Err(format!("Cannot save: resource directory spellings '{other}' and '{p}' collide"));
            }
            parent = p;
        }
    }
    for (normalized, name) in &out {
        let mut parent = normalized.as_str();
        while let Some((p, _)) = parent.rsplit_once('/') {
            if let Some(other) = out.get(p) {
                return Err(format!("Cannot save: file '{other}' is also a resource directory for '{name}'"));
            }
            parent = p;
        }
    }
    Ok(out)
}

fn inputs(m: &MapModel) -> Vec<(&str, &Blob)> {
    let mut out: Vec<_> = m.carried.iter().map(|(n, b)| (n.as_str(), b)).collect();
    out.extend(m.layers.iter().map(|(_, i)| (i.path.as_str(), &i.origin)));
    out.extend(m.lf.iter().map(|(_, i)| (i.path.as_str(), &i.origin)));
    out.extend(m.xml.iter().map(|i| (i.path.as_str(), &i.origin)));
    if let Some(i) = &m.explicit {
        out.push((&i.path, &i.origin));
    }
    if let Some(i) = &m.project
        && !m.built
    {
        out.push((&i.path, &i.origin));
    }
    if let Some(i) = &m.tile_list
        && i.path != built::TILE_MAP_VIEW
    {
        out.push((&i.path, &i.origin));
    }
    out
}

fn readable(p: &Path) -> io::Result<File> {
    let f = File::open(p)?;
    if !f.metadata()?.is_file() {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "not a regular file"));
    }
    Ok(f)
}

// Use the same practical path normalization as destination ownership, including existing aliases.
fn location(p: &Path) -> io::Result<String> {
    wce_foundation::save_path::key(p).map_err(io::Error::other)
}

pub(super) fn preflight(m: &MapModel, files: &MapFiles, dir: &Path) -> Result<BTreeSet<String>, String> {
    let originals = inputs(m);
    keys(originals.iter().map(|(n, _)| *n))?;
    let outputs = keys(files.folder.keys().map(String::as_str))?;
    match std::fs::metadata(dir) {
        Ok(_) => {
            let existing = MapFiles::from_dir(dir).map_err(|e| e.to_string())?;
            keys(existing.folder.keys().map(String::as_str))?;
            for name in existing.folder.keys() {
                if let Some(output) = outputs.get(&key(name)?)
                    && *output != name
                {
                    return Err(format!(
                        "Cannot save: destination key '{name}' differs from output key '{output}'; use a destination that preserves the resource spelling"
                    ));
                }
            }
            // Unplanned files stay untouched. Check their combined names only for
            // ambiguity or file/directory conflicts with the outputs we will publish.
            let names: BTreeSet<_> = existing.folder.keys().chain(files.folder.keys()).map(String::as_str).collect();
            keys(names)?;
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        Err(e) => return Err(e.to_string()),
    }
    let mut sources: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for (name, blob) in originals {
        if let Blob::File(p) = blob {
            sources.entry(location(p).map_err(|e| e.to_string())?).or_default().insert(key(name)?);
        }
    }
    let mut kept = BTreeSet::new();
    for (name, blob) in &files.folder {
        let to = dir.join(name);
        let fail = |e: io::Error| format!("{name}: {e}");
        if let Blob::File(p) = blob {
            readable(p).map_err(fail)?;
        }
        let target = match std::fs::metadata(&to) {
            Ok(m) if m.is_file() => Some(m),
            Ok(_) => return Err(format!("{name}: destination is not a regular file")),
            Err(e) if e.kind() == io::ErrorKind::NotFound => None,
            Err(e) => return Err(fail(e)),
        };
        if target.is_some()
            && match blob {
                Blob::File(p) => {
                    wce_foundation::publication::identical(&wce_foundation::publication::Source::File(p.clone()), &to).map_err(fail)?
                }
                Blob::Bytes(b) => {
                    wce_foundation::publication::identical(&wce_foundation::publication::Source::Bytes(b.clone()), &to).map_err(fail)?
                }
            }
        {
            kept.insert(name.clone());
            continue;
        }
        if sources.get(&location(&to).map_err(fail)?).is_some_and(|owners| owners.iter().any(|n| *n != key(name).expect("checked key"))) {
            return Err(format!("Cannot save: '{name}' would overwrite another resource's source; use an independent destination"));
        }
        if target.as_ref().is_some_and(|m| m.permissions().readonly()) {
            return Err(format!("Cannot save: '{name}' is read-only; its native snapshot does not grant write permission"));
        }
    }
    Ok(kept)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reader_visible_keys_refuse_escapes_aliases_and_file_directory_conflicts() {
        for s in ["", "/absolute", "C:/drive", "../escape", "a/../b", "a//b", "a/./b", "a\\b", "x:", "x.", "x ", "NUL", "COM1.bin"] {
            assert!(key(s).is_err(), "{s}");
        }
        for names in [vec!["x", "X"], vec!["directory", "DIRECTORY/file"], vec!["a/b", "a/B/c"], vec!["Dir/a", "dir/b"]] {
            assert!(keys(names).is_err());
        }
        assert!(keys(["a/file", "b/file", "lf_height_map.dds", "雪.bin"]).is_ok());
    }
}
