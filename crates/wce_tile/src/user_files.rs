//! The editor's tables in files (user 2026-11-16: buildings added without a rebuild; distributed with the app, read from its folder): the
//! road types (`road_dirt`, `road_types.toml`), the road fences (`road_fences.toml`), the wall bases (`wce_walls::profile`, `walls.toml`)
//! and the bridge bases (`wce_walls::bridge`, `bridges.toml`), one set per game (user 2026-11-16: a game's buildings are not another's):
//! in the app's `tables/<game key>` folder ([`tables_dir`]; `attila`, `rome_2`, `thrones_of_britannia`). The user's files can also be in
//! their own `<data folder>/tables/<game key>` (`wce_foundation::settings::user_tables_dir`; the Flatpak's app folder can't be written).
//!
//! Each table is its game's file (the vanilla one, `walls.toml`) and every file beside it whose name starts with its name's stem and `_`
//! (`walls_tdd.toml`, `walls_mine.toml`; user 2026-11-16: a table per mod): each has a top-level `mod_key`, the start of a mod pack's
//! name (`tdd_pack1_main_` for The Dawnless Days), and is read only while a pack whose name starts with it is loaded in the editor (an
//! empty `mod_key`: always, for the user's own changes). They are read in their names' order, each one's entries replacing those of the
//! same key or name before it, the rest added. The app never writes one; an update replaces only the files it ships. All are read again on
//! each load; a set that does not read is logged once and the table before (or the built-in one) kept. A missing vanilla file is logged
//! and the copy built into the editor used.

use std::path::{Path, PathBuf};
use std::sync::{OnceLock, RwLock};

/// The tables' folder in the app's folder (and in the source tree).
pub const TABLES_DIR: &str = "tables";

/// The tables' folder: the app's (`<folder of wce.exe>/tables`, as a release has it; the Flatpak's `/app/share/wce/tables`, beside its
/// `/app/bin`) where it is there, else the source tree's (a build run from `target`, on the machine it was built on).
#[must_use]
pub fn tables_dir() -> PathBuf {
    let bin = std::env::current_exe().ok().and_then(|exe| exe.parent().map(Path::to_path_buf));
    let app = bin.as_ref().map(|b| b.join(TABLES_DIR));
    let share = bin.as_ref().map(|b| b.join("..").join("share").join("wce").join(TABLES_DIR)).filter(|s| s.is_dir());
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..").join(TABLES_DIR);
    match (app, share) {
        (Some(a), _) if a.is_dir() => a,
        (_, Some(s)) => s,
        (Some(a), None) if !source.is_dir() => a,
        _ => source,
    }
}

/// A table's file read over the vanilla one: its name and its text.
pub type Extra = (String, String);

/// How a table is read: the vanilla text and the files over it, in order.
pub type Parse<T> = fn(&str, &[Extra]) -> Result<T, String>;

/// A table's vanilla files as built into the editor, by game key (the first: the table before any load, as tests and tools see it).
pub type BuiltIn = &'static [(&'static str, &'static str)];

/// A file's `mod_key` (none: it has none, or does not read).
#[must_use]
pub fn mod_key(text: &str) -> Option<String> {
    #[derive(serde::Deserialize)]
    struct Head {
        mod_key: Option<String>,
    }

    toml::from_str::<Head>(text).ok()?.mod_key
}

/// The files over a table's vanilla file `name` in `dir` (`<stem>_*.toml`), in their names' order, those whose `mod_key` starts a name of
/// `mods` (the mod packs loaded; an empty key: always). One without a `mod_key` is logged and left out.
#[must_use]
pub fn extra_files(dir: &Path, name: &str, mods: &[String]) -> Vec<Extra> {
    let (stem, ext) = name.rsplit_once('.').unwrap_or((name, ""));
    let prefix = format!("{stem}_");
    let mut names = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| e.file_name().to_str().map(str::to_owned))
        .filter(|n| n.starts_with(&prefix) && n.rsplit_once('.').is_some_and(|(_, x)| x == ext))
        .collect::<Vec<_>>();
    names.sort();
    names
        .into_iter()
        .filter_map(|n| {
            let text = std::fs::read_to_string(dir.join(&n)).ok()?;
            let Some(key) = mod_key(&text) else {
                wce_foundation::log::warn(format!(
                    "{}: no mod_key (the start of a mod pack's name; empty: always): not read",
                    dir.join(&n).display()
                ));
                return None;
            };

            let wanted = key.is_empty() || mods.iter().any(|m| m.to_ascii_lowercase().starts_with(&key.to_ascii_lowercase()));
            wanted.then_some((n, text))
        })
        .collect()
}

/// A table read from its files: the built-in one until [`Self::load`] reads them, then theirs (each set of texts read once, kept: the
/// editor's rows and the walls made hold on to what it gives).
pub struct Registry<T: 'static> {
    name: &'static str,
    built_in: BuiltIn,
    parse: Parse<T>,
    shipped: OnceLock<&'static T>,
    loaded: RwLock<Option<((String, Option<String>, Vec<Extra>), &'static T)>>,
}

impl<T: Send + Sync + 'static> Registry<T> {
    #[must_use]
    pub const fn new(name: &'static str, built_in: BuiltIn, parse: Parse<T>) -> Self {
        Self { name, built_in, parse, shipped: OnceLock::new(), loaded: RwLock::new(None) }
    }

    /// A game's vanilla file as built into the editor.
    #[must_use]
    pub fn built_in(&self, game: &str) -> Option<&'static str> {
        self.built_in.iter().find(|(g, _)| *g == game).map(|(_, t)| *t)
    }

    /// The vanilla file's name.
    #[must_use]
    pub fn file_name(&self) -> &'static str {
        self.name
    }

    /// The first game's table built into the editor (its vanilla file as it was built, nothing over it): the table before any load.
    ///
    /// # Panics
    ///
    /// There is no built-in file or it does not read (a test sees it first).
    pub fn shipped(&self) -> &'static T {
        self.shipped.get_or_init(|| {
            let (game, text) = self.built_in.first().unwrap_or_else(|| panic!("{}: no built-in file", self.name));
            Box::leak(Box::new((self.parse)(text, &[]).unwrap_or_else(|e| panic!("the built-in {game} {} reads: {e}", self.name))))
        })
    }

    /// The table: its files' last read, else the built-in one.
    pub fn get(&self) -> &'static T {
        let loaded = self.loaded.read().unwrap_or_else(std::sync::PoisonError::into_inner);
        loaded.as_ref().map_or_else(|| self.shipped(), |(_, t)| *t)
    }

    /// Reads a game's table in `tables` (the tables' folder: [`tables_dir`]), in its `<game>` folder: its vanilla file and those over it
    /// for `mods` (the mod packs loaded: [`extra_files`]). Texts read before are not read again (nor leaked again, nor logged again). A
    /// game whose vanilla file is missing gets the copy built into the editor; one without either keeps the table before (logged).
    pub fn load(&self, tables: &Path, game: &str, mods: &[String]) {
        let dir = tables.join(game);
        let vanilla = std::fs::read_to_string(dir.join(self.name)).ok();
        let mut extras = extra_files(&dir, self.name, mods);
        // The user's own files, in their tables folder (the app's may not be writable), in the names' order with the app's.
        if let Some(user) = wce_foundation::settings::user_tables_dir().map(|u| u.join(game)).filter(|u| *u != dir) {
            extras.extend(extra_files(&user, self.name, mods));
            extras.sort_by(|a, b| a.0.cmp(&b.0));
        }

        let texts = (game.to_owned(), vanilla, extras);
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

        if !texts.2.is_empty() {
            let names = texts.2.iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>();
            wce_foundation::log::info(format!("{} over {}: {}", dir.display(), self.name, names.join(", ")));
        }

        let table = match (self.parse)(base, &texts.2) {
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

/// Reads each of `extras` as a `F` (its errors named after it), in order.
///
/// # Errors
///
/// One is not TOML of `F`'s shape.
pub fn read_extras<F: serde::de::DeserializeOwned>(extras: &[Extra]) -> Result<Vec<F>, String> {
    extras.iter().map(|(name, text)| toml::from_str::<F>(text).map_err(|e| format!("{name}: {e}"))).collect()
}

/// Puts each of `over` in `list` in place of the one with the same name (`same`), or after the rest.
pub fn replace_or_add<E>(list: &mut Vec<E>, over: Vec<E>, same: impl Fn(&E, &E) -> bool) {
    for c in over {
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

    /// Lines, those of the files over it after (but their mod_key); "bad" anywhere does not read.
    fn parse(vanilla: &str, extras: &[Extra]) -> Result<Vec<String>, String> {
        let text = std::iter::once(vanilla).chain(extras.iter().map(|(_, t)| t.as_str())).collect::<Vec<_>>().join("\n");
        if text.contains("bad") {
            Err("bad".to_owned())
        } else {
            Ok(text.lines().filter(|l| !l.is_empty() && !l.starts_with("mod_key")).map(str::to_owned).collect())
        }
    }

    #[test]
    fn the_vanilla_file_and_those_of_the_mods_loaded_and_bad_ones_keep_the_last() {
        static R: Registry<Vec<String>> = Registry::new("t.toml", &[("one", "built\nin"), ("two", "other")], parse);
        assert_eq!(R.get(), &["built", "in"]);
        let tables = tempfile::tempdir().unwrap();
        R.load(tables.path(), "two", &[]);
        assert_eq!(R.get(), &["other"], "no file: the game's built-in copy");
        R.load(tables.path(), "three", &[]);
        assert_eq!(R.get(), &["other"], "neither: the table before");
        let dir = tables.path().join("one");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("t.toml"), "a").unwrap();
        R.load(tables.path(), "one", &[]);
        assert_eq!(R.get(), &["a"]);
        assert!(!dir.join("t_x.toml").exists(), "nothing written");

        // A mod's file, read while its pack is loaded; the user's own (an empty key) always; one without a key never.
        std::fs::write(dir.join("t_mod.toml"), "mod_key = \"tdd_pack1_main_\"\nfrom_mod = 1").unwrap();
        std::fs::write(dir.join("t_mine.toml"), "mod_key = \"\"\nmine = 1").unwrap();
        std::fs::write(dir.join("t_nokey.toml"), "x = 1").unwrap();
        std::fs::write(dir.join("other_mod.toml"), "mod_key = \"\"\nnot_this_table = 1").unwrap();
        R.load(tables.path(), "one", &[]);
        assert_eq!(R.get(), &["a", "mine = 1"]);
        R.load(tables.path(), "one", &["TDD_pack1_main_v1.1.0.pack".to_owned()]);
        assert_eq!(R.get(), &["a", "mine = 1", "from_mod = 1"], "in their names' order");
        let first = std::ptr::from_ref(R.get());
        R.load(tables.path(), "one", &["tdd_pack1_main_v1.1.0.pack".to_owned()]);
        assert!(std::ptr::eq(first, R.get()), "the same texts: not read again");
        std::fs::write(dir.join("t_mine.toml"), "mod_key = \"\"\nbad = 1").unwrap();
        R.load(tables.path(), "one", &[]);
        assert_eq!(R.get(), &["a", "mine = 1", "from_mod = 1"], "a bad file keeps the table before");
    }

    #[test]
    fn entries_over_replace_by_name_or_are_added() {
        let mut list = vec![("a", 1), ("b", 2)];
        replace_or_add(&mut list, vec![("b", 20), ("c", 3)], |x, y| x.0 == y.0);
        assert_eq!(list, [("a", 1), ("b", 20), ("c", 3)]);
        assert_eq!(mod_key("mod_key = \"x_\"\n[[a]]\nb = 1\n").as_deref(), Some("x_"));
        assert_eq!(mod_key("[[a]]\nb = 1\n"), None);
    }

    #[test]
    fn the_source_trees_tables_are_found_from_a_build() {
        for game in ["attila", "rome_2", "thrones_of_britannia"] {
            assert!(tables_dir().join(game).join("walls.toml").is_file(), "{}", tables_dir().display());
        }
    }
}
