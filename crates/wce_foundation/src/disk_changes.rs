//! Per-document size/mtime observations. These are not native semantic authority or a recovery record.
use std::{
    collections::BTreeMap,
    fs, io,
    path::{Path, PathBuf},
    time::SystemTime,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Stamp {
    pub size: u64,
    pub modified: SystemTime,
}
impl Stamp {
    pub fn read(path: &Path) -> Result<Self, String> {
        let m = fs::metadata(path).map_err(|e| format!("{}: {e}", path.display()))?;
        if !m.is_file() {
            return Err(format!("{} is not a regular resource file", path.display()));
        }
        Ok(Self { size: m.len(), modified: m.modified().map_err(|e| e.to_string())? })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Folder {
    pub root: PathBuf,
    pub files: BTreeMap<String, Stamp>,
}

pub enum ResourceEntry {
    File,
    Directory,
    Ignore,
}

/// Follow file links for observations, but never descend through linked folders or
/// treat a dangling link as a map resource. Readers and freshness scans share this rule.
pub fn resource_entry(entry: &fs::DirEntry) -> io::Result<ResourceEntry> {
    let kind = entry.file_type()?;
    if kind.is_symlink() {
        return match fs::metadata(entry.path()) {
            Ok(metadata) if metadata.is_file() => Ok(ResourceEntry::File),
            Ok(_) => Ok(ResourceEntry::Ignore),
            Err(error) if matches!(error.kind(), io::ErrorKind::NotFound | io::ErrorKind::NotADirectory) => Ok(ResourceEntry::Ignore),
            Err(error) => Err(error),
        };
    }
    Ok(if kind.is_dir() {
        ResourceEntry::Directory
    } else if kind.is_file() {
        ResourceEntry::File
    } else {
        ResourceEntry::Ignore
    })
}
impl Folder {
    pub fn empty(root: &Path) -> Self {
        Self { root: root.to_owned(), files: BTreeMap::new() }
    }
    pub fn record(&mut self, path: &Path) -> Result<(), String> {
        self.files.insert(crate::save_path::key(path)?, Stamp::read(path)?);
        Ok(())
    }
    pub fn capture(root: &Path) -> Result<Self, String> {
        Self::capture_named(root).map(|(folder, _)| folder)
    }
    fn capture_named(root: &Path) -> Result<(Self, Vec<String>), String> {
        #[cfg(test)]
        tests::CAPTURES.with(|count| count.set(count.get() + 1));
        let mut out = Self::empty(root);
        let mut names = vec![];
        let mut todo = vec![(root.to_owned(), String::new())];
        while let Some((dir, prefix)) = todo.pop() {
            let entries = match fs::read_dir(&dir) {
                Ok(e) => e,
                Err(e) if e.kind() == io::ErrorKind::NotFound && dir == root => continue,
                Err(e) => return Err(format!("{}: {e}", dir.display())),
            };
            for entry in entries {
                let entry = entry.map_err(|e| e.to_string())?;
                if crate::publication::reserved(&entry.file_name().to_string_lossy()) {
                    continue;
                }
                let name = entry.file_name().to_string_lossy().into_owned();
                let name = if prefix.is_empty() { name } else { format!("{prefix}/{name}") };
                match resource_entry(&entry).map_err(|e| e.to_string())? {
                    ResourceEntry::Directory => todo.push((entry.path(), name)),
                    ResourceEntry::File => {
                        out.record(&entry.path())?;
                        names.push(name);
                    }
                    ResourceEntry::Ignore => {}
                }
            }
        }
        Ok((out, names))
    }
    fn capture_paths(root: &Path, paths: &[PathBuf]) -> Result<Self, String> {
        let mut out = Self::empty(root);
        for path in paths {
            match fs::metadata(path) {
                Ok(metadata) if metadata.is_dir() => out.files.extend(Self::capture(path)?.files),
                Ok(metadata) if metadata.is_file() => out.record(path)?,
                Ok(_) => return Err(format!("{} is not a resource file or folder", path.display())),
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(format!("{}: {error}", path.display())),
            }
        }
        Ok(out)
    }
    fn same_root(&self, path: &Path) -> Result<bool, String> {
        Ok(crate::save_path::key(&self.root)? == crate::save_path::key(path)?)
    }
    fn contains(&self, path: &Path) -> Result<bool, String> {
        let root = crate::save_path::key(&self.root)?;
        let path = crate::save_path::key(path)?;
        Ok(path.strip_prefix(&root).is_some_and(|rest| rest.starts_with(std::path::MAIN_SEPARATOR)))
    }
    fn note(&mut self, path: &Path) -> Result<(), String> {
        if self.contains(path)? {
            if path.exists() {
                self.record(path)?;
            } else {
                self.files.remove(&crate::save_path::key(path)?);
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Default)]
pub struct Changes {
    pub sources: Vec<Folder>,
    destination: Option<Folder>,
    destination_needs_choice: bool,
    partial: Option<Folder>,
    // A successful prompt precheck can be consumed by the immediately following Save.
    // It grants no publication authority: Attempt::recheck still runs after staging.
    prepared: Option<Attempt>,
}
#[derive(Clone, Debug)]
pub struct Attempt {
    sources: Vec<Folder>,
    pub target: Folder,
    target_paths: Option<Vec<PathBuf>>,
    target_names: Vec<String>,
}
impl Changes {
    pub fn new(source: Option<&Folder>) -> Self {
        Self { sources: source.into_iter().cloned().collect(), ..Self::default() }
    }
    pub fn set_destination(&mut self, path: &Path) -> Result<(), String> {
        self.prepared = None;
        let folder = Folder::capture(path)?;
        let source = self.sources.iter().map(|f| f.same_root(path)).collect::<Result<Vec<_>, _>>()?.into_iter().any(|same| same);
        self.destination_needs_choice = !folder.files.is_empty() && !source;
        self.destination = Some(folder);
        Ok(())
    }
    fn observed_target(&self, path: &Path) -> Result<Option<&Folder>, String> {
        for folder in [&self.destination, &self.partial].into_iter().flatten() {
            if folder.same_root(path)? {
                return Ok(Some(folder));
            }
        }
        for folder in &self.sources {
            if folder.same_root(path)? {
                return Ok(Some(folder));
            }
        }
        Ok(None)
    }
    fn current(&self, path: &Path) -> Result<(Attempt, bool), String> {
        let mut captured: BTreeMap<String, (Folder, Vec<String>)> = BTreeMap::new();
        let mut capture = |root: &Path| -> Result<(Folder, Vec<String>), String> {
            let key = crate::save_path::key(root)?;
            if let Some((folder, names)) = captured.get(&key) {
                let mut folder: Folder = folder.clone();
                folder.root = root.to_owned();
                return Ok((folder, names.clone()));
            }
            let observed = Folder::capture_named(root)?;
            captured.insert(key, observed.clone());
            Ok(observed)
        };
        let sources = self.sources.iter().map(|f| capture(&f.root).map(|(folder, _)| folder)).collect::<Result<Vec<_>, _>>()?;
        let (target, target_names) = capture(path)?;
        let unapproved =
            self.destination_needs_choice && self.destination.as_ref().map(|f| f.same_root(path)).transpose()?.unwrap_or(false);
        let changed = unapproved
            || sources.iter().zip(&self.sources).any(|(a, b)| a.files != b.files)
            || match self.observed_target(path)? {
                Some(before) => before.files != target.files,
                None => !target.files.is_empty(),
            };
        Ok((Attempt { sources, target, target_paths: None, target_names }, changed))
    }
    pub fn reason(&mut self, path: &Path) -> Result<Option<String>, String> {
        self.prepared = None;
        let (attempt, changed) = self.current(path)?;
        if !changed {
            self.prepared = Some(attempt);
        }
        Ok(changed.then(|| {
            "Files changed on disk, or this folder already contains files from another source. Overwrite with this document, or cancel?"
                .into()
        }))
    }
    pub fn begin(&mut self, path: &Path, overwrite: bool) -> Result<Attempt, String> {
        if let Some(attempt) = self.prepared.take()
            && !overwrite
            && attempt.target.same_root(path)?
            && attempt.sources == self.sources
            && !(self.destination_needs_choice && self.destination.as_ref().map(|f| f.same_root(path)).transpose()?.unwrap_or(false))
            && self.observed_target(path)?.map_or(attempt.target.files.is_empty(), |before| before.files == attempt.target.files)
        {
            return Ok(attempt);
        }
        let (attempt, changed) = self.current(path)?;
        if changed && !overwrite {
            return Err("Source or destination files changed; choose Overwrite or Cancel".into());
        }
        Ok(attempt)
    }
    pub fn published(&mut self, attempt: &Attempt, path: &Path) -> Result<(), String> {
        self.prepared = None;
        for source in &mut self.sources {
            source.note(path)?;
        }
        if self.destination.as_ref().map(|f| f.same_root(&attempt.target.root)).transpose()?.unwrap_or(false) {
            self.destination.as_mut().unwrap().note(path)?;
            self.destination_needs_choice = false;
        } else {
            if !self.partial.as_ref().map(|f| f.same_root(&attempt.target.root)).transpose()?.unwrap_or(false) {
                self.partial = Some(attempt.target.clone());
            }
            self.partial.as_mut().unwrap().note(path)?;
        }
        Ok(())
    }
    /// Export has its own destination; it must not retarget the document's Save folder.
    pub fn begin_export(&self, path: &Path) -> Result<Attempt, String> {
        self.export_attempt(path, None)
    }
    /// Observe only this map's loose subtree and pack, not other projects sharing the output root.
    pub fn begin_export_scoped(&self, path: &Path, resources: &[&str]) -> Result<Attempt, String> {
        let paths = resources.iter().map(|rel| crate::publication::resource_path(path, rel)).collect::<Result<Vec<_>, _>>()?;
        self.export_attempt(path, Some(paths))
    }
    fn export_attempt(&self, path: &Path, target_paths: Option<Vec<PathBuf>>) -> Result<Attempt, String> {
        if self.destination_needs_choice {
            return Err("Review the populated Save folder with Save (Overwrite or Cancel) before exporting".into());
        }
        let mut sources = vec![];
        for observed in self.sources.iter().chain(self.destination.iter()) {
            let current = Folder::capture(&observed.root)?;
            if current.files != observed.files {
                return Err("Source or Save files changed; use Save to review Overwrite or Cancel before exporting".into());
            }
            sources.push(current);
        }
        let (target, target_names) = match &target_paths {
            Some(paths) => (Folder::capture_paths(path, paths)?, vec![]),
            None => Folder::capture_named(path)?,
        };
        Ok(Attempt { sources, target, target_paths, target_names })
    }
    pub fn note_owned(&mut self, path: &Path) -> Result<(), String> {
        self.prepared = None;
        for observed in self.sources.iter_mut().chain(self.destination.iter_mut()).chain(self.partial.iter_mut()) {
            observed.note(path)?;
        }
        Ok(())
    }
    /// Remember a completed copy without changing the active Save destination or its
    /// outstanding overwrite choice. Source stamps include only our own publications.
    pub fn finish_copy(&mut self, attempt: Attempt) {
        self.prepared = None;
        self.partial = Some(attempt.target);
    }

    pub fn finish(&mut self, attempt: Attempt) {
        self.prepared = None;
        self.sources = attempt.sources;
        self.destination = Some(attempt.target);
        self.destination_needs_choice = false;
        self.partial = None;
    }
}
impl Attempt {
    /// Exact reader-visible spellings from the same initial capture as the target stamps.
    pub fn target_names(&self) -> &[String] {
        &self.target_names
    }
    pub fn recheck(&self) -> Result<(), String> {
        let mut checked = BTreeMap::new();
        for folder in &self.sources {
            let key = crate::save_path::key(&folder.root)?;
            if let std::collections::btree_map::Entry::Vacant(entry) = checked.entry(key.clone()) {
                entry.insert(Folder::capture_named(&folder.root)?);
            }
            if checked[&key].0.files != folder.files {
                return Err("Files changed while staging; nothing has been published. Save again to review the change".into());
            }
        }
        let (target, target_names) = match &self.target_paths {
            Some(paths) => (Folder::capture_paths(&self.target.root, paths)?, vec![]),
            None => {
                let key = crate::save_path::key(&self.target.root)?;
                match checked.remove(&key) {
                    Some(folder) => folder,
                    None => Folder::capture_named(&self.target.root)?,
                }
            }
        };
        if target.files != self.target.files
            || target_names.iter().collect::<std::collections::BTreeSet<_>>() != self.target_names.iter().collect()
        {
            return Err("Destination changed while staging; nothing has been published".into());
        }
        Ok(())
    }
    pub fn check_target(&self, path: &Path) -> Result<(), String> {
        let key = crate::save_path::key(path)?;
        let current = match fs::metadata(path) {
            Ok(_) => Some(Stamp::read(path)?),
            Err(e) if e.kind() == io::ErrorKind::NotFound => None,
            Err(e) => return Err(e.to_string()),
        };
        if current.as_ref() != self.target.files.get(&key) {
            return Err(format!("{} changed before publication", path.display()));
        }
        Ok(())
    }
    pub fn published(&mut self, path: &Path) -> Result<(), String> {
        self.target.note(path)?;
        for source in &mut self.sources {
            source.note(path)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    thread_local! { pub(super) static CAPTURES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) }; }

    #[test]
    fn owned_save_captures_each_shared_root_once_before_and_once_after_staging() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("native.bin"), b"original").unwrap();
        let mut changes = Changes::new(Some(&Folder::capture(root.path()).unwrap()));
        changes.set_destination(root.path()).unwrap();
        CAPTURES.set(0);
        assert!(changes.reason(root.path()).unwrap().is_none());
        let attempt = changes.begin(root.path(), false).unwrap();
        assert_eq!(attempt.target_names(), &["native.bin"]);
        attempt.recheck().unwrap();
        assert_eq!(CAPTURES.get(), 2, "source, destination and prompt precheck share a capture; the final check remains independent");
    }

    #[test]
    fn a_cached_precheck_never_approves_later_outside_changes() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("native.bin"), b"original").unwrap();
        let mut changes = Changes::new(Some(&Folder::capture(root.path()).unwrap()));
        assert!(changes.reason(root.path()).unwrap().is_none());
        fs::write(root.path().join("native.bin"), b"TEd changed the file after the precheck").unwrap();
        let attempt = changes.begin(root.path(), false).unwrap();
        assert!(attempt.recheck().is_err());
        assert!(changes.reason(root.path()).unwrap().is_some());
        assert!(changes.begin(root.path(), false).is_err());
        changes.begin(root.path(), true).unwrap().recheck().unwrap();
    }

    #[test]
    fn changed_observations_invalidate_the_single_use_precheck() {
        let root = tempfile::tempdir().unwrap();
        let extra = tempfile::tempdir().unwrap();
        fs::write(extra.path().join("other.bin"), b"original").unwrap();
        let observed = Folder::capture(extra.path()).unwrap();
        let mut changes = Changes::default();
        assert!(changes.reason(root.path()).unwrap().is_none());
        changes.sources.push(observed);
        fs::write(extra.path().join("other.bin"), b"outside edit to the newly observed source").unwrap();
        assert!(changes.begin(root.path(), false).is_err());
    }

    #[test]
    fn cached_destination_spellings_are_checked_again_before_publication() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("native.bin"), b"original").unwrap();
        let mut changes = Changes::new(Some(&Folder::capture(root.path()).unwrap()));
        assert!(changes.reason(root.path()).unwrap().is_none());
        fs::rename(root.path().join("native.bin"), root.path().join("NATIVE.bin")).unwrap();
        assert!(changes.begin(root.path(), false).unwrap().recheck().is_err());
    }
}
