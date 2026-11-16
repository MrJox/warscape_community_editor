//! The editor's tables in files (user 2026-11-16: buildings added without a rebuild; distributed with the app, read from its folder): the
//! road fences (`road_fences.toml`), the wall bases (`wce_walls::profile`, `walls.toml`) and the bridge bases (`wce_walls::bridge`,
//! `bridges.toml`), one set per game (user 2026-11-16: a game's buildings are not another's): in the app's `tables/<game key>` folder
//! ([`tables_dir`]; `attila`, `rome_2`, `thrones_of_britannia`). Each has a custom variant beside it (`walls_custom.toml`, ...) the
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

/// A table's shipped files as built into the editor, by game key (the first: the table before any load, as tests and tools see it).
pub type BuiltIn = &'static [(&'static str, &'static str)];

/// A table read from its files: the built-in one until [`Self::load`] reads them, then theirs (each pair of texts read once, kept: the
/// editor's rows and the walls made hold on to what it gives).
pub struct Registry<T: 'static> {
    name: &'static str,
    built_in: BuiltIn,
    parse: Parse<T>,
    shipped: OnceLock<&'static T>,
    loaded: RwLock<Option<((String, Option<String>, Option<String>), &'static T)>>,
}

impl<T: Send + Sync + 'static> Registry<T> {
    #[must_use]
    pub const fn new(name: &'static str, built_in: BuiltIn, parse: Parse<T>) -> Self {
        Self { name, built_in, parse, shipped: OnceLock::new(), loaded: RwLock::new(None) }
    }

    /// A game's shipped file as built into the editor.
    #[must_use]
    pub fn built_in(&self, game: &str) -> Option<&'static str> {
        self.built_in.iter().find(|(g, _)| *g == game).map(|(_, t)| *t)
    }

    /// The shipped file's name.
    #[must_use]
    pub fn file_name(&self) -> &'static str {
        self.name
    }

    /// The first game's table built into the editor (its shipped file as it was built, no custom one): the table before any load.
    ///
    /// # Panics
    ///
    /// There is no built-in file or it does not read (a test sees it first).
    pub fn shipped(&self) -> &'static T {
        self.shipped.get_or_init(|| {
            let (game, text) = self.built_in.first().unwrap_or_else(|| panic!("{}: no built-in file", self.name));
            Box::leak(Box::new((self.parse)(text, None).unwrap_or_else(|e| panic!("the built-in {game} {} reads: {e}", self.name))))
        })
    }

    /// The table: its files' last read, else the built-in one.
    pub fn get(&self) -> &'static T {
        let loaded = self.loaded.read().unwrap_or_else(std::sync::PoisonError::into_inner);
        loaded.as_ref().map_or_else(|| self.shipped(), |(_, t)| *t)
    }

    /// Reads a game's shipped file and its custom variant in `tables` (the tables' folder: [`tables_dir`]), in its `<game>` folder. Texts
    /// read before are not read again (nor leaked again, nor logged again). A game whose file is missing gets the copy built into the
    /// editor; one without either keeps the table before (logged).
    pub fn load(&self, tables: &Path, game: &str) {
        let dir = tables.join(game);
        let shipped = std::fs::read_to_string(dir.join(self.name)).ok();
        let custom = std::fs::read_to_string(dir.join(custom_name(self.name))).ok();
        let texts = (game.to_owned(), shipped, custom);
        let mut loaded = self.loaded.write().unwrap_or_else(std::sync::PoisonError::into_inner);
        if loaded.as_ref().is_some_and(|(t, _)| *t == texts) {
            return;
        }

        let kept = loaded.as_ref().map_or_else(|| self.shipped(), |(_, t)| *t);
        let Some(base) = texts.1.as_deref().or_else(|| self.built_in(game)) else {
            wce_foundation::log::warn(format!(
                "{}: missing, and none built in for {game}: the table before is kept",
                dir.join(self.name).display()
            ));
            *loaded = Some((texts, kept));
            return;
        };

        if texts.1.is_none() {
            wce_foundation::log::warn(format!("{}: missing: the editor's built-in copy is used", dir.join(self.name).display()));
        }

        let table = match (self.parse)(base, texts.2.as_deref()) {
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
        static R: Registry<Vec<String>> = Registry::new("t.toml", &[("one", "built\nin"), ("two", "other")], parse);
        assert_eq!(R.get(), &["built", "in"]);
        let tables = tempfile::tempdir().unwrap();
        R.load(tables.path(), "two");
        assert_eq!(R.get(), &["other"], "no file: the game's built-in copy");
        R.load(tables.path(), "three");
        assert_eq!(R.get(), &["other"], "neither: the table before");
        let dir = tables.path().join("one");
        std::fs::create_dir_all(&dir).unwrap();
        assert!(!dir.join("t.toml").exists(), "nothing written");
        std::fs::write(dir.join("t.toml"), "a").unwrap();
        R.load(tables.path(), "one");
        assert_eq!(R.get(), &["a"]);
        std::fs::write(dir.join("t_custom.toml"), "mine").unwrap();
        R.load(tables.path(), "one");
        assert_eq!(R.get(), &["a", "mine"]);
        let first = std::ptr::from_ref(R.get());
        R.load(tables.path(), "one");
        assert!(std::ptr::eq(first, R.get()), "the same texts: not read again");
        R.load(tables.path(), "two");
        assert_eq!(R.get(), &["other"], "another game: its own");
        std::fs::write(dir.join("t_custom.toml"), "bad").unwrap();
        R.load(tables.path(), "one");
        assert_eq!(R.get(), &["other"], "a bad custom file keeps the table before");
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
        for game in ["attila", "rome_2", "thrones_of_britannia"] {
            assert!(tables_dir().join(game).join("walls.toml").is_file(), "{}", tables_dir().display());
        }
    }
}
