//! The editor's tables in files (user 2026-11-16: buildings added without a rebuild; distributed with the app, read from its folder): the
//! road fences (`road_fences.toml`), the wall bases (`wce_walls::profile`, `walls.toml`) and the bridge bases (`wce_walls::bridge`,
//! `bridges.toml`), in the app's `tables` folder ([`tables_dir`]). Each has a custom variant beside it (`walls_custom.toml`, ...) the
//! user writes and the app never does, nor an update (it is not in the release): its entries replace the shipped ones of the same name or
//! key, the rest are added. Both are read again on each load; a pair that does not read is logged once and the table before (or the
//! built-in one) kept. A missing shipped file is logged and the copy built into the editor used.

use std::path::{Path, PathBuf};
use std::sync::{OnceLock, RwLock};

/// The tables' folder in the app's folder (and in the source tree).
pub const TABLES_DIR: &str = "tables";

/// The tables' folder: the app's (`<folder of wce.exe>/tables`, as a release has it) where it is there, else the source tree's (a build
/// run from `target`, on the machine it was built on).
#[must_use]
pub fn tables_dir() -> PathBuf {
    let app = std::env::current_exe().ok().and_then(|exe| exe.parent().map(|p| p.join(TABLES_DIR)));
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..").join(TABLES_DIR);
    match app {
        Some(a) if a.is_dir() || !source.is_dir() => a,
        _ => source,
    }
}

/// The custom variant's name of a table's file: `walls.toml`, `walls_custom.toml`.
#[must_use]
pub fn custom_name(name: &str) -> String {
    match name.rsplit_once('.') {
        Some((stem, ext)) => format!("{stem}_custom.{ext}"),
        None => format!("{name}_custom"),
    }
}

/// How a table is read: the shipped text and the custom one (none: no custom file).
pub type Parse<T> = fn(&str, Option<&str>) -> Result<T, String>;

/// A table read from its files: the built-in one until [`Self::load`] reads them, then theirs (each pair of texts read once, kept: the
/// editor's rows and the walls made hold on to what it gives).
pub struct Registry<T: 'static> {
    name: &'static str,
    default: &'static str,
    parse: Parse<T>,
    shipped: OnceLock<&'static T>,
    loaded: RwLock<Option<((Option<String>, Option<String>), &'static T)>>,
}

impl<T: Send + Sync + 'static> Registry<T> {
    #[must_use]
    pub const fn new(name: &'static str, default: &'static str, parse: Parse<T>) -> Self {
        Self { name, default, parse, shipped: OnceLock::new(), loaded: RwLock::new(None) }
    }

    /// The shipped file's name.
    #[must_use]
    pub fn file_name(&self) -> &'static str {
        self.name
    }

    /// The table built into the editor (the shipped file as it was built, no custom one).
    ///
    /// # Panics
    ///
    /// The built-in text does not read (a test sees it first).
    pub fn shipped(&self) -> &'static T {
        self.shipped.get_or_init(|| {
            Box::leak(Box::new((self.parse)(self.default, None).unwrap_or_else(|e| panic!("the built-in {} reads: {e}", self.name))))
        })
    }

    /// The table: its files' last read, else the built-in one.
    pub fn get(&self) -> &'static T {
        let loaded = self.loaded.read().unwrap_or_else(std::sync::PoisonError::into_inner);
        loaded.as_ref().map_or_else(|| self.shipped(), |(_, t)| *t)
    }

    /// Reads the shipped file and its custom variant in `dir` (the tables' folder). Texts read before are not read again (nor leaked
    /// again, nor logged again).
    pub fn load(&self, dir: &Path) {
        let shipped = std::fs::read_to_string(dir.join(self.name)).ok();
        let custom = std::fs::read_to_string(dir.join(custom_name(self.name))).ok();
        let texts = (shipped, custom);
        let mut loaded = self.loaded.write().unwrap_or_else(std::sync::PoisonError::into_inner);
        if loaded.as_ref().is_some_and(|(t, _)| *t == texts) {
            return;
        }

        if texts.0.is_none() {
            wce_foundation::log::warn(format!("{}: missing: the editor's built-in copy is used", dir.join(self.name).display()));
        }

        let kept = loaded.as_ref().map_or_else(|| self.shipped(), |(_, t)| *t);
        let table = match (self.parse)(texts.0.as_deref().unwrap_or(self.default), texts.1.as_deref()) {
            Ok(table) => Box::leak(Box::new(table)),
            Err(e) => {
                // Said once: the texts are remembered with the table kept.
                wce_foundation::log::warn(format!("{}: {e}: the table before is kept", dir.display()));
                kept
            }
        };
        *loaded = Some((texts, table));
    }
}

/// Puts each of `custom` in `list` in place of the one with the same name (`same`), or after the rest.
pub fn replace_or_add<E>(list: &mut Vec<E>, custom: Vec<E>, same: impl Fn(&E, &E) -> bool) {
    for c in custom {
        match list.iter().position(|e| same(e, &c)) {
            Some(i) => list[i] = c,
            None => list.push(c),
        }
    }
}

/// A string kept for the editor's run (a file's key or label, as the tables' `&'static str`s hold them).
#[must_use]
pub fn keep(s: &str) -> &'static str {
    Box::leak(s.to_owned().into_boxed_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Lines, the custom ones after; "bad" anywhere does not read.
    fn parse(shipped: &str, custom: Option<&str>) -> Result<Vec<String>, String> {
        let text = format!("{shipped}\n{}", custom.unwrap_or_default());
        if text.contains("bad") { Err("bad".to_owned()) } else { Ok(text.lines().filter(|l| !l.is_empty()).map(str::to_owned).collect()) }
    }

    #[test]
    fn the_shipped_file_and_its_custom_one_and_bad_ones_keep_the_last() {
        static R: Registry<Vec<String>> = Registry::new("t.toml", "built\nin", parse);
        assert_eq!(R.get(), &["built", "in"]);
        let dir = tempfile::tempdir().unwrap();
        R.load(dir.path());
        assert_eq!(R.get(), &["built", "in"], "no file: the built-in copy");
        assert!(!dir.path().join("t.toml").exists(), "nothing written");
        std::fs::write(dir.path().join("t.toml"), "a").unwrap();
        R.load(dir.path());
        assert_eq!(R.get(), &["a"]);
        std::fs::write(dir.path().join("t_custom.toml"), "mine").unwrap();
        R.load(dir.path());
        assert_eq!(R.get(), &["a", "mine"]);
        let first = std::ptr::from_ref(R.get());
        R.load(dir.path());
        assert!(std::ptr::eq(first, R.get()), "the same texts: not read again");
        std::fs::write(dir.path().join("t_custom.toml"), "bad").unwrap();
        R.load(dir.path());
        assert_eq!(R.get(), &["a", "mine"], "a bad custom file keeps the last good table");
    }

    #[test]
    fn custom_entries_replace_by_name_or_are_added() {
        let mut list = vec![("a", 1), ("b", 2)];
        replace_or_add(&mut list, vec![("b", 20), ("c", 3)], |x, y| x.0 == y.0);
        assert_eq!(list, [("a", 1), ("b", 20), ("c", 3)]);
        assert_eq!(custom_name("walls.toml"), "walls_custom.toml");
    }

    #[test]
    fn the_source_trees_tables_are_found_from_a_build() {
        assert!(tables_dir().join("walls.toml").is_file(), "{}", tables_dir().display());
    }
}
