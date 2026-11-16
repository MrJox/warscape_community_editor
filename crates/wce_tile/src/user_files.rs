//! Files of the editor's tables the user can edit (user 2026-11-16: buildings added without a rebuild): the road fences
//! (`road_fences.toml`), the wall bases (`wce_walls::profile`, `walls.toml`) and the bridge bases (`wce_walls::bridge`, `bridges.toml`),
//! in the settings folder. Each ships as text built into the editor, is written where missing, and is read again on each load; one that
//! does not read is logged and the last good one (or the shipped one) kept, the file left as it is for the user to mend.

use std::path::Path;
use std::sync::{OnceLock, RwLock};

/// A file's text in `dir`, written there as `default` where it is missing (then `default`).
#[must_use]
pub fn read_or_write(dir: &Path, name: &str, default: &str) -> String {
    let path = dir.join(name);
    match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(_) => {
            if let Err(e) = std::fs::create_dir_all(dir).and_then(|()| std::fs::write(&path, default)) {
                wce_foundation::log::warn(format!("{}: not written: {e}", path.display()));
            }

            default.to_owned()
        }
    }
}

/// A table read from a user file: the shipped one until [`Self::load`] reads the file, then the file's (each text read once, kept: the
/// editor's rows and the walls made hold on to what it gives).
pub struct Registry<T: 'static> {
    name: &'static str,
    default: &'static str,
    parse: fn(&str) -> Result<T, String>,
    shipped: OnceLock<&'static T>,
    loaded: RwLock<Option<(String, &'static T)>>,
}

impl<T: Send + Sync + 'static> Registry<T> {
    #[must_use]
    pub const fn new(name: &'static str, default: &'static str, parse: fn(&str) -> Result<T, String>) -> Self {
        Self { name, default, parse, shipped: OnceLock::new(), loaded: RwLock::new(None) }
    }

    /// The file's name.
    #[must_use]
    pub fn file_name(&self) -> &'static str {
        self.name
    }

    /// The shipped table.
    ///
    /// # Panics
    ///
    /// The shipped text does not read (a test sees it first).
    pub fn shipped(&self) -> &'static T {
        self.shipped.get_or_init(|| {
            Box::leak(Box::new((self.parse)(self.default).unwrap_or_else(|e| panic!("the shipped {} reads: {e}", self.name))))
        })
    }

    /// The table: the file's last read, else the shipped one.
    pub fn get(&self) -> &'static T {
        let loaded = self.loaded.read().unwrap_or_else(std::sync::PoisonError::into_inner);
        loaded.as_ref().map_or_else(|| self.shipped(), |(_, t)| *t)
    }

    /// Reads the file in `dir` (written as shipped where missing). A text read before is not read again (nor leaked again); one that does
    /// not read is logged and the table kept.
    pub fn load(&self, dir: &Path) {
        let text = read_or_write(dir, self.name, self.default);
        let mut loaded = self.loaded.write().unwrap_or_else(std::sync::PoisonError::into_inner);
        if loaded.as_ref().is_some_and(|(t, _)| *t == text) {
            return;
        }

        match (self.parse)(&text) {
            Ok(table) => *loaded = Some((text, Box::leak(Box::new(table)))),
            Err(e) => {
                // Said once: the bad text is remembered with the table kept.
                wce_foundation::log::warn(format!("{}: {e}: the table before is kept", dir.join(self.name).display()));
                let kept = loaded.as_ref().map_or_else(|| self.shipped(), |(_, t)| *t);
                *loaded = Some((text, kept));
            }
        }
    }
}

/// A string kept for the editor's run (a user file's key or label, as the tables' `&'static str`s hold them).
#[must_use]
pub fn keep(s: &str) -> &'static str {
    Box::leak(s.to_owned().into_boxed_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str) -> Result<Vec<String>, String> {
        if text.starts_with("bad") { Err("bad".to_owned()) } else { Ok(text.lines().map(str::to_owned).collect()) }
    }

    #[test]
    fn shipped_then_the_file_and_a_bad_file_keeps_the_last() {
        static R: Registry<Vec<String>> = Registry::new("t.txt", "a\nb", parse);
        assert_eq!(R.get(), &["a", "b"]);
        let dir = tempfile::tempdir().unwrap();
        R.load(dir.path());
        assert_eq!(std::fs::read_to_string(dir.path().join("t.txt")).unwrap(), "a\nb", "written where missing");
        std::fs::write(dir.path().join("t.txt"), "c").unwrap();
        R.load(dir.path());
        assert_eq!(R.get(), &["c"]);
        let first = std::ptr::from_ref(R.get());
        R.load(dir.path());
        assert!(std::ptr::eq(first, R.get()), "the same text: not read again");
        std::fs::write(dir.path().join("t.txt"), "bad").unwrap();
        R.load(dir.path());
        assert_eq!(R.get(), &["c"], "a bad file keeps the last good one");
    }
}
