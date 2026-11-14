//! Local stage-then-rename publication shared by native writers. No map transaction or recovery journal.
use std::{
    collections::BTreeSet,
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    path::{Path, PathBuf},
    sync::Arc,
};

#[derive(Clone, Debug)]
pub enum Source {
    File(PathBuf),
    Bytes(Arc<[u8]>),
    Remove,
}
#[derive(Clone, Debug)]
pub struct Output {
    pub path: PathBuf,
    pub source: Source,
    pub priority: u8,
}
impl Output {
    /// Equal priorities publish in path order. Format-specific ordering belongs to the caller.
    pub fn new(path: PathBuf, source: Source) -> Self {
        Self {
            path,
            source,
            priority: 0,
        }
    }
}

#[derive(Debug, Default)]
pub struct Report {
    pub published: Vec<PathBuf>,
    pub retained: Vec<PathBuf>,
}
#[derive(Debug)]
pub struct Failure {
    pub message: String,
    pub published: Vec<PathBuf>,
}
impl std::fmt::Display for Failure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}; {} file(s) published{}",
            self.message,
            self.published.len(),
            if self.published.is_empty() {
                String::new()
            } else {
                format!(
                    ": {}",
                    self.published
                        .iter()
                        .take(5)
                        .map(|p| p.display().to_string())
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            }
        )
    }
}
fn fail(e: impl ToString) -> Failure {
    Failure {
        message: e.to_string(),
        published: vec![],
    }
}

pub fn reserved(name: &str) -> bool {
    name.split(['/', '\\'])
        .any(|n| n.to_ascii_lowercase().ends_with(".wce_tmp"))
}

/// Join a reader-visible resource key without allowing it to escape its output folder.
pub fn resource_path(root: &Path, key: &str) -> Result<PathBuf, String> {
    if reserved(key)
        || key.contains('\\')
        || key.split('/').any(|p| {
            p.is_empty()
                || p == "."
                || p == ".."
                || p.ends_with(['.', ' '])
                || p.chars()
                    .any(|c| c.is_control() || matches!(c, ':' | '<' | '>' | '"' | '|' | '?' | '*'))
        })
    {
        return Err(format!("{key}: unsupported or reserved resource key"));
    }
    for part in key.split('/') {
        let base = part.split('.').next().unwrap_or("").to_ascii_uppercase();
        if matches!(base.as_str(), "CON" | "PRN" | "AUX" | "NUL")
            || ["COM", "LPT"].iter().any(|p| {
                base.strip_prefix(p).is_some_and(|n| {
                    matches!(n, "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9")
                })
            })
        {
            return Err(format!("{key}: ambiguous device resource key"));
        }
    }
    Ok(root.join(key))
}

fn identical(source: &Source, to: &Path) -> io::Result<bool> {
    if matches!(source, Source::Remove) {
        return match fs::metadata(to) {
            Ok(_) => Ok(false),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(true),
            Err(e) => Err(e),
        };
    }
    if let Source::File(path) = source {
        // A retained file already at its destination needs no content reads. Resolve aliases
        // too, but keep distinct files on the exact-content comparison path below.
        let metadata = fs::metadata(path)?;
        if !metadata.is_file() {
            return Err(io::Error::other("source is not a regular file"));
        }
        if path == to
            || fs::canonicalize(path)?
                == match fs::canonicalize(to) {
                    Ok(path) => path,
                    Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(false),
                    Err(e) => return Err(e),
                }
        {
            return Ok(true);
        }
    }
    let mut target = match File::open(to) {
        Ok(f) => f,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(e),
    };
    if !target.metadata()?.is_file() {
        return Err(io::Error::other("destination is not a regular file"));
    }
    let mut buffer = [0u8; 65536];
    match source {
        Source::Remove => unreachable!(),
        Source::Bytes(bytes) => {
            if target.metadata()?.len() != bytes.len() as u64 {
                return Ok(false);
            }
            for part in bytes.chunks(buffer.len()) {
                target.read_exact(&mut buffer[..part.len()])?;
                if &buffer[..part.len()] != part {
                    return Ok(false);
                }
            }
        }
        Source::File(path) => {
            let mut src = File::open(path)?;
            if !src.metadata()?.is_file() {
                return Err(io::Error::other("source is not a regular file"));
            }
            let mut remaining = src.metadata()?.len();
            if target.metadata()?.len() != remaining {
                return Ok(false);
            }
            let mut other = [0u8; 65536];
            while remaining != 0 {
                let n = remaining.min(buffer.len() as u64) as usize;
                src.read_exact(&mut buffer[..n])?;
                target.read_exact(&mut other[..n])?;
                if buffer[..n] != other[..n] {
                    return Ok(false);
                }
                remaining -= n as u64;
            }
            if src.read(&mut buffer[..1])? != 0 {
                return Ok(false);
            }
        }
    }
    Ok(target.read(&mut buffer[..1])? == 0)
}

fn clear_temp(path: &Path) -> io::Result<()> {
    match fs::symlink_metadata(path) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
        Ok(m) => {
            if m.is_dir() {
                return Err(io::Error::other("reserved temporary path is a directory"));
            }
            if !m.file_type().is_symlink() && m.permissions().readonly() {
                let mut p = m.permissions();
                p.set_readonly(false);
                fs::set_permissions(path, p)?;
            }
            fs::remove_file(path)
        }
    }
}

pub struct Staged {
    files: Vec<(Output, Option<PathBuf>)>,
    retained: Vec<PathBuf>,
}
impl Drop for Staged {
    fn drop(&mut self) {
        for (_, temp) in &self.files {
            if let Some(temp) = temp {
                let _ = fs::remove_file(temp);
            }
        }
    }
}
impl Staged {
    /// All source reads and writes to reserved temporaries finish here, before any destination rename.
    pub fn prepare(outputs: Vec<Output>) -> Result<Self, Failure> {
        let mut staged = Self {
            files: vec![],
            retained: vec![],
        };
        let mut keys = BTreeSet::new();
        for o in &outputs {
            if reserved(&o.path.to_string_lossy()) {
                return Err(fail(format!(
                    "{}: reserved .wce_tmp resource",
                    o.path.display()
                )));
            }
            let key = crate::save_path::key(&o.path).map_err(fail)?;
            if !keys.insert(key) {
                return Err(fail("duplicate publication destination"));
            }
        }
        for o in outputs {
            let result = (|| -> io::Result<Option<PathBuf>> {
                if identical(&o.source, &o.path)? {
                    return Ok(None);
                }
                let permissions = match fs::metadata(&o.path) {
                    Ok(m) => {
                        if m.permissions().readonly() {
                            return Err(io::Error::other("destination is read-only"));
                        }
                        Some(m.permissions())
                    }
                    Err(e) if e.kind() == io::ErrorKind::NotFound => None,
                    Err(e) => return Err(e),
                };
                if matches!(o.source, Source::Remove) {
                    return Ok(None);
                }
                if let Some(parent) = o.path.parent() {
                    fs::create_dir_all(parent)?;
                }
                let temp = o.path.with_added_extension("wce_tmp");
                clear_temp(&temp)?;
                // Never copy source permissions: new files use creation defaults, existing files retain theirs.
                let mut file = OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&temp)?;
                let written = match &o.source {
                    Source::File(p) => {
                        File::open(p).and_then(|mut src| io::copy(&mut src, &mut file).map(|_| ()))
                    }
                    Source::Bytes(b) => file.write_all(b),
                    Source::Remove => unreachable!(),
                };
                let result = written.and_then(|()| file.flush());
                drop(file);
                if let Err(e) = result {
                    let _ = fs::remove_file(&temp);
                    return Err(e);
                }
                if let Some(p) = permissions {
                    if let Err(e) = fs::set_permissions(&temp, p) {
                        let _ = fs::remove_file(&temp);
                        return Err(e);
                    }
                }
                Ok(Some(temp))
            })();
            match result {
                Ok(Some(temp)) => staged.files.push((o, Some(temp))),
                Ok(None) if matches!(o.source, Source::Remove) && o.path.exists() => {
                    staged.files.push((o, None))
                }
                Ok(None) => staged.retained.push(o.path),
                Err(e) => return Err(fail(format!("stage {}: {e}", o.path.display()))),
            }
        }
        Ok(staged)
    }

    pub fn publish(self) -> Result<Report, Failure> {
        self.publish_checked(|_| Ok(()), |_| Ok(()))
    }

    /// Hooks let the owning document recheck freshness and remember each successful own publication.
    pub fn publish_checked(
        mut self,
        mut before: impl FnMut(&Path) -> Result<(), String>,
        mut after: impl FnMut(&Path) -> Result<(), String>,
    ) -> Result<Report, Failure> {
        self.files.sort_by(|(a, _), (b, _)| {
            a.priority
                .cmp(&b.priority)
                .then_with(|| a.path.cmp(&b.path))
        });
        let mut report = Report {
            published: vec![],
            retained: self.retained.clone(),
        };
        for (o, temp) in &self.files {
            let result = before(&o.path).and_then(|()| {
                match temp {
                    Some(temp) => fs::rename(temp, &o.path),
                    None => fs::remove_file(&o.path),
                }
                .map_err(|e| format!("publish {}: {e}", o.path.display()))
            });
            if let Err(message) = result {
                return Err(Failure {
                    message,
                    published: report.published,
                });
            }
            report.published.push(o.path.clone());
            if let Err(message) = after(&o.path) {
                return Err(Failure {
                    message,
                    published: report.published,
                });
            }
        }
        self.files.clear();
        Ok(report)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn root(label: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!(
            "wce-stage-{label}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&p).unwrap();
        p
    }
    fn cleanup(p: &Path) {
        let p = p.canonicalize().unwrap();
        assert!(
            p.starts_with(std::env::temp_dir().canonicalize().unwrap())
                && p.file_name()
                    .unwrap()
                    .to_string_lossy()
                    .starts_with("wce-stage-")
        );
        fs::remove_dir_all(p).unwrap();
    }
    #[test]
    fn every_staging_failure_leaves_destinations_unchanged() {
        for at in 0..3 {
            let dir = root("failure");
            let mut out = vec![];
            for i in 0..3 {
                let p = dir.join(format!("{i}.bin"));
                fs::write(&p, b"old").unwrap();
                out.push(Output::new(
                    p,
                    if i == at {
                        Source::File(dir.join("missing"))
                    } else {
                        Source::Bytes(Arc::from(&b"new"[..]))
                    },
                ));
            }
            assert!(Staged::prepare(out).is_err());
            for i in 0..3 {
                assert_eq!(fs::read(dir.join(format!("{i}.bin"))).unwrap(), b"old");
            }
            assert!(
                !fs::read_dir(&dir)
                    .unwrap()
                    .flatten()
                    .any(|e| reserved(&e.file_name().to_string_lossy()))
            );
            cleanup(&dir);
        }
    }
    #[test]
    fn all_outputs_are_staged_and_leftover_temporary_files_are_replaced() {
        let dir = root("all");
        let a = dir.join("a");
        let b = dir.join("b");
        fs::write(&a, b"old").unwrap();
        fs::write(a.with_added_extension("wce_tmp"), b"leftover").unwrap();
        let plan = Staged::prepare(vec![
            Output::new(a.clone(), Source::Bytes(Arc::from(&b"new"[..]))),
            Output::new(b.clone(), Source::Bytes(Arc::from(&b"other"[..]))),
        ])
        .unwrap();
        assert_eq!(fs::read(&a).unwrap(), b"old");
        assert!(!b.exists());
        assert!(a.with_added_extension("wce_tmp").is_file());
        assert!(b.with_added_extension("wce_tmp").is_file());
        assert_eq!(plan.publish().unwrap().published.len(), 2);
        assert_eq!(fs::read(a).unwrap(), b"new");
        cleanup(&dir);
    }
    #[test]
    fn reserved_resource_outputs_are_refused() {
        let dir = root("reserved");
        for key in [
            "/absolute",
            "../escape",
            "dir/../escape",
            "x\\escape",
            "NUL",
            "a/COM1.bin",
            "a.wce_tmp/file",
        ] {
            assert!(resource_path(&dir, key).is_err());
        }
        assert!(
            Staged::prepare(vec![Output::new(
                dir.join("bad.WCE_TMP"),
                Source::Bytes(Arc::from(&b"bad"[..]))
            )])
            .is_err()
        );
        cleanup(&dir);
    }
    #[test]
    fn caller_priorities_report_progress_and_ordinary_retry_completes() {
        let names = [
            ("baseline", 3),
            ("support", 0),
            ("data", 1),
            ("normal", 0),
            ("preview", 2),
            ("metadata", 4),
        ];
        for fail_at in 0..names.len() {
            let dir = root("order");
            for (n, _) in names {
                fs::write(dir.join(n), b"old").unwrap();
            }
            let make = || {
                names
                    .iter()
                    .map(|(n, priority)| Output {
                        path: dir.join(n),
                        source: Source::Bytes(Arc::from(&b"new"[..])),
                        priority: *priority,
                    })
                    .collect()
            };
            let mut order = vec![];
            let failure = Staged::prepare(make())
                .unwrap()
                .publish_checked(
                    |path| {
                        if order.len() == fail_at {
                            return Err("controlled rename failure".into());
                        }
                        order.push(path.file_name().unwrap().to_string_lossy().into_owned());
                        Ok(())
                    },
                    |_| Ok(()),
                )
                .unwrap_err();
            assert_eq!(failure.published.len(), fail_at);
            assert_eq!(
                order,
                vec![
                    "normal", "support", "data", "preview", "baseline", "metadata"
                ][..fail_at]
            );
            if fail_at <= 4 {
                assert_eq!(fs::read(dir.join("baseline")).unwrap(), b"old");
            }
            Staged::prepare(make()).unwrap().publish().unwrap();
            for (n, _) in names {
                assert_eq!(fs::read(dir.join(n)).unwrap(), b"new");
            }
            cleanup(&dir);
        }
    }
    #[test]
    fn readonly_sources_use_creation_defaults_and_existing_targets_keep_permissions() {
        let dir = root("permissions");
        let source = dir.join("source");
        fs::write(&source, b"source bytes").unwrap();
        let original = fs::metadata(&source).unwrap().permissions();
        let mut readonly = original.clone();
        readonly.set_readonly(true);
        fs::set_permissions(&source, readonly).unwrap();
        let target = dir.join("new");
        Staged::prepare(vec![Output::new(
            target.clone(),
            Source::File(source.clone()),
        )])
        .unwrap()
        .publish()
        .unwrap();
        assert!(!fs::metadata(&target).unwrap().permissions().readonly());
        assert_eq!(fs::read(&target).unwrap(), b"source bytes");
        let before = fs::metadata(&target).unwrap().permissions();
        Staged::prepare(vec![Output::new(
            target.clone(),
            Source::Bytes(Arc::from(&b"rewritten"[..])),
        )])
        .unwrap()
        .publish()
        .unwrap();
        assert_eq!(fs::metadata(&target).unwrap().permissions(), before);
        let mut p = before.clone();
        p.set_readonly(true);
        fs::set_permissions(&target, p).unwrap();
        assert!(
            Staged::prepare(vec![Output::new(
                target.clone(),
                Source::Bytes(Arc::from(&b"different"[..]))
            )])
            .is_err()
        );
        let modified = fs::metadata(&target).unwrap().modified().unwrap();
        let report = Staged::prepare(vec![Output::new(
            target.clone(),
            Source::Bytes(Arc::from(&b"rewritten"[..])),
        )])
        .unwrap()
        .publish()
        .unwrap();
        assert!(report.published.is_empty());
        assert_eq!(fs::metadata(&target).unwrap().modified().unwrap(), modified);
        fs::set_permissions(target, before).unwrap();
        fs::set_permissions(source, original).unwrap();
        cleanup(&dir);
    }
    #[test]
    fn removals_wait_for_all_staging_and_output_collisions_fail_before_publication() {
        let dir = root("removal");
        let old = dir.join("old");
        fs::write(&old, b"keep until all ready").unwrap();
        assert!(
            Staged::prepare(vec![
                Output::new(old.clone(), Source::Remove),
                Output::new(dir.join("late"), Source::File(dir.join("missing")))
            ])
            .is_err()
        );
        assert!(old.is_file());
        assert!(
            Staged::prepare(vec![
                Output::new(old.clone(), Source::Remove),
                Output::new(old.clone(), Source::Bytes(Arc::from(&b"collision"[..])))
            ])
            .is_err()
        );
        assert!(old.is_file());
        Staged::prepare(vec![Output::new(old.clone(), Source::Remove)])
            .unwrap()
            .publish()
            .unwrap();
        assert!(!old.exists());
        cleanup(&dir);
    }
}
