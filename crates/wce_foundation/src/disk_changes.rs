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
impl Folder {
    pub fn empty(root: &Path) -> Self {
        Self { root: root.to_owned(), files: BTreeMap::new() }
    }
    pub fn record(&mut self, path: &Path) -> Result<(), String> {
        self.files.insert(crate::save_path::key(path)?, Stamp::read(path)?);
        Ok(())
    }
    pub fn capture(root: &Path) -> Result<Self, String> {
        let mut out = Self::empty(root);
        let mut todo = vec![root.to_owned()];
        while let Some(dir) = todo.pop() {
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
                if entry.file_type().map_err(|e| e.to_string())?.is_dir() {
                    todo.push(entry.path())
                } else {
                    out.record(&entry.path())?;
                }
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
}
pub struct Attempt {
    sources: Vec<Folder>,
    pub target: Folder,
}
impl Changes {
    pub fn new(source: Option<&Folder>) -> Self {
        Self { sources: source.into_iter().cloned().collect(), ..Self::default() }
    }
    pub fn set_destination(&mut self, path: &Path) -> Result<(), String> {
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
        let sources = self.sources.iter().map(|f| Folder::capture(&f.root)).collect::<Result<Vec<_>, _>>()?;
        let target = Folder::capture(path)?;
        let unapproved =
            self.destination_needs_choice && self.destination.as_ref().map(|f| f.same_root(path)).transpose()?.unwrap_or(false);
        let changed = unapproved
            || sources.iter().zip(&self.sources).any(|(a, b)| a.files != b.files)
            || match self.observed_target(path)? {
                Some(before) => before.files != target.files,
                None => !target.files.is_empty(),
            };
        Ok((Attempt { sources, target }, changed))
    }
    pub fn reason(&self, path: &Path) -> Result<Option<String>, String> {
        let (_, changed) = self.current(path)?;
        Ok(changed.then(|| {
            "Files changed on disk, or this folder already contains files from another source. Overwrite with this document, or cancel?"
                .into()
        }))
    }
    pub fn begin(&self, path: &Path, overwrite: bool) -> Result<Attempt, String> {
        let (attempt, changed) = self.current(path)?;
        if changed && !overwrite {
            return Err("Source or destination files changed; choose Overwrite or Cancel".into());
        }
        Ok(attempt)
    }
    pub fn published(&mut self, attempt: &Attempt, path: &Path) -> Result<(), String> {
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
        Ok(Attempt { sources, target: Folder::capture(path)? })
    }
    pub fn note_owned(&mut self, path: &Path) -> Result<(), String> {
        for observed in self.sources.iter_mut().chain(self.destination.iter_mut()).chain(self.partial.iter_mut()) {
            observed.note(path)?;
        }
        Ok(())
    }
    /// Remember a completed copy without changing the active Save destination or its
    /// outstanding overwrite choice. Source stamps include only our own publications.
    pub fn finish_copy(&mut self, attempt: Attempt) {
        self.sources = attempt.sources;
        self.partial = Some(attempt.target);
    }

    pub fn finish(&mut self, attempt: Attempt) {
        self.sources = attempt.sources;
        self.destination = Some(attempt.target);
        self.destination_needs_choice = false;
        self.partial = None;
    }
}
impl Attempt {
    pub fn recheck(&self) -> Result<(), String> {
        for folder in self.sources.iter().chain(std::iter::once(&self.target)) {
            if Folder::capture(&folder.root)?.files != folder.files {
                return Err("Files changed while staging; nothing has been published. Save again to review the change".into());
            }
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
