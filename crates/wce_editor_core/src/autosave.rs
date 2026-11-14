//! Autosave's folder (docs/plans/autosave.md): copies of the modified documents in `<projects>/autosave/`, never over the project's own
//! files, offered back when the editor next starts after a session that did not end cleanly.
//!
//! ```text
//! <projects>/autosave/
//!   <session>/                one per editor process (its start time and pid), locked while it runs
//!     session.lock            held locked by the running editor: a session whose lock can be taken ended without cleaning up
//!     session.toml            the editor's version, the start time
//!     <slot>/                 a document's: its key (where its Save writes) made file-safe, and a hash of it
//!       meta.toml             what the document is and its copies, oldest first
//!       1/ 2/ 3/              the copies, each written into `tmp_<n>` first and renamed into place
//!     restored/<n>/           copies taken back into documents of this session (they read their files from there until closed)
//! ```
//!
//! What a copy holds is the document's business (`Document::autosave`); this module only gives it a folder.

use std::fs::File;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

/// The autosave folder's name in the projects folder.
pub const FOLDER: &str = "autosave";

const META: &str = "meta.toml";
const SESSION: &str = "session.toml";
const LOCK: &str = "session.lock";
const RESTORED: &str = "restored";

/// What a document's copies are of, and the copies themselves (`meta.toml`).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AutosaveMeta {
    /// The tool the document belongs to (the editor fills it in): the one that takes the copy back.
    pub tool: String,

    /// `Document::kind`.
    pub kind: String,
    pub title: String,

    /// Where the document's Save writes (a file or a folder): what names its slot, and what recovery compares a copy's time with.
    pub target: PathBuf,

    /// What the tool opens again to restore it: a path in the packs (`from_packs`), a file on disk, a map's name.
    pub open: String,
    pub from_packs: bool,

    /// The document's own file or folder inside a copy (relative, `/` separated; empty: the copy's folder itself).
    pub inner: String,

    /// The copies, oldest first.
    pub copies: Vec<CopyRecord>,
}

/// One copy of a document: its folder in the slot and when it was written (seconds since 1970).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct CopyRecord {
    pub folder: String,
    pub time: u64,
}

impl AutosaveMeta {
    /// What tells documents apart across sessions: their Save's target, spelled the same however the path was joined.
    pub fn key(&self) -> String {
        let s = self.target.to_string_lossy().replace('\\', "/");
        let s = s.trim_end_matches('/');
        if cfg!(windows) { s.to_lowercase() } else { s.to_owned() }
    }

    /// The newest copy.
    pub fn newest(&self) -> Option<&CopyRecord> {
        self.copies.last()
    }
}

/// Seconds since 1970, now.
pub fn now_secs() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

/// When `path` was last written (seconds since 1970): a file's time, or a folder's newest file directly inside it (a tile's or a map's
/// files); `None` when there is nothing there.
pub fn written_secs(path: &Path) -> Option<u64> {
    let secs = |m: &std::fs::Metadata| m.modified().ok()?.duration_since(UNIX_EPOCH).ok().map(|d| d.as_secs());
    let meta = std::fs::metadata(path).ok()?;
    if meta.is_file() {
        return secs(&meta);
    }

    std::fs::read_dir(path)
        .ok()?
        .flatten()
        .filter_map(|e| e.metadata().ok())
        .filter(std::fs::Metadata::is_file)
        .filter_map(|m| secs(&m))
        .max()
}

/// A slot's folder name: the key's last two parts, file-safe, and a hash of the whole key (two documents of the same name differ).
pub fn slot_name(key: &str) -> String {
    let tail = key.rsplit('/').take(2).collect::<Vec<_>>().into_iter().rev().collect::<Vec<_>>().join("_");
    let safe: String =
        tail.chars().map(|c| if c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_' { c } else { '_' }).take(48).collect();
    // FNV-1a: the same in every run.
    let hash = key.bytes().fold(0xcbf2_9ce4_8422_2325_u64, |h, b| (h ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01b3));
    format!("{safe}_{hash:016x}")
}

fn read_meta(slot: &Path) -> Option<AutosaveMeta> {
    let text = std::fs::read_to_string(slot.join(META)).ok()?;
    toml::from_str(&text).ok()
}

fn write_meta(slot: &Path, meta: &AutosaveMeta) -> Result<(), String> {
    let text = toml::to_string(meta).map_err(|e| e.to_string())?;
    wce_foundation::files::write_replacing(slot.join(META), text).map_err(|e| format!("{}: {e}", slot.join(META).display()))
}

/// Removes a folder and what is in it; one that is not there is fine.
fn remove_dir(dir: &Path) -> io::Result<()> {
    match std::fs::remove_dir_all(dir) {
        Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
        _ => Ok(()),
    }
}

/// Whether a session's editor is gone: its lock file can be locked (or it has none). A lock held by a running editor fails.
fn session_ended(dir: &Path) -> bool {
    let Ok(f) = File::options().read(true).write(true).open(dir.join(LOCK)) else { return !dir.join(LOCK).exists() };
    let ended = f.try_lock().is_ok();
    if ended {
        let _ = f.unlock();
    }

    ended
}

/// This editor's session folder, locked while it lives.
pub struct Session {
    dir: PathBuf,
    lock: Option<File>,
    restored: u32,
}

impl Session {
    /// A new session folder under `root` (`<projects>/autosave`), its lock taken.
    ///
    /// # Errors
    ///
    /// The folder can't be made, or its lock taken.
    pub fn start(root: &Path) -> Result<Self, String> {
        let pid = std::process::id();
        let started = now_secs();
        // Another session of this process started in the same second (a game switched): a folder of its own.
        let dir = (0..)
            .map(|n| root.join(if n == 0 { format!("{started}_{pid}") } else { format!("{started}_{pid}_{n}") }))
            .find(|d| !d.exists())
            .expect("a free name");
        std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        let lock = File::options()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(dir.join(LOCK))
            .map_err(|e| format!("{}: {e}", dir.join(LOCK).display()))?;
        lock.try_lock().map_err(|e| format!("{}: {e}", dir.join(LOCK).display()))?;
        let about = format!("version = \"{}\"\nstarted = {}\npid = {pid}\n", env!("CARGO_PKG_VERSION"), now_secs());
        let _ = std::fs::write(dir.join(SESSION), about);
        Ok(Self { dir, lock: Some(lock), restored: 0 })
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// The autosave folder the session is in.
    pub fn root(&self) -> &Path {
        self.dir.parent().unwrap_or(&self.dir)
    }

    fn slot(&self, key: &str) -> PathBuf {
        self.dir.join(slot_name(key))
    }

    /// A new copy of a document: `write` fills a fresh folder, which then becomes the slot's newest copy; the oldest go past `keep`.
    /// Returns the slot's meta as written.
    ///
    /// # Errors
    ///
    /// `write`'s, or the folders can't be made or renamed.
    pub fn write(&self, meta: &AutosaveMeta, keep: usize, write: impl FnOnce(&Path) -> Result<(), String>) -> Result<AutosaveMeta, String> {
        let slot = self.slot(&meta.key());
        std::fs::create_dir_all(&slot).map_err(|e| format!("{}: {e}", slot.display()))?;
        let mut copies = read_meta(&slot).map(|m| m.copies).unwrap_or_default();
        let n = copies.iter().filter_map(|c| c.folder.parse::<u64>().ok()).max().unwrap_or(0) + 1;
        let temp = slot.join(format!("tmp_{n}"));
        remove_dir(&temp).map_err(|e| format!("{}: {e}", temp.display()))?;
        std::fs::create_dir_all(&temp).map_err(|e| format!("{}: {e}", temp.display()))?;
        if let Err(e) = write(&temp) {
            let _ = remove_dir(&temp);
            return Err(e);
        }

        let folder = n.to_string();
        std::fs::rename(&temp, slot.join(&folder)).map_err(|e| {
            let _ = remove_dir(&temp);
            format!("{}: {e}", slot.join(&folder).display())
        })?;
        copies.push(CopyRecord { folder, time: now_secs() });
        while copies.len() > keep.max(1) {
            let old = copies.remove(0);
            let _ = remove_dir(&slot.join(&old.folder));
        }

        let meta = AutosaveMeta { copies, ..meta.clone() };
        write_meta(&slot, &meta)?;
        Ok(meta)
    }

    /// Drops a document's copies (saved, closed, or its key changed).
    pub fn remove(&self, key: &str) {
        let _ = remove_dir(&self.slot(key));
    }

    /// The document slots the session has.
    pub fn slots(&self) -> Vec<AutosaveMeta> {
        slots_of(&self.dir).into_iter().map(|(_, m)| m).collect()
    }

    /// Takes a copy of another session into this one (`restored/<n>`), where the restored document reads its files from until the
    /// session ends. The caller then drops the other session's slot ([`discard`]) once the document is back, or puts the copy back
    /// ([`give_back`]) when it could not be.
    ///
    /// # Errors
    ///
    /// The copy is gone, or can't be moved or copied.
    pub fn take(&mut self, found: &Found, copy: usize) -> Result<PathBuf, String> {
        let record = found.meta.copies.get(copy).ok_or("no such copy")?;
        let from = found.slot.join(&record.folder);
        if !from.is_dir() {
            return Err(format!("{} is gone", from.display()));
        }

        self.restored += 1;
        let to = self.dir.join(RESTORED).join(format!("{}_{}", self.restored, slot_name(&found.meta.key())));
        let _ = remove_dir(&to);
        std::fs::create_dir_all(to.parent().unwrap_or(&self.dir)).map_err(|e| e.to_string())?;
        if std::fs::rename(&from, &to).is_err() {
            copy_tree(&from, &to).map_err(|e| format!("{} to {}: {e}", from.display(), to.display()))?;
        }

        Ok(to)
    }

    /// A clean end: the session's folder goes (the documents were saved or their changes discarded).
    pub fn end(mut self) {
        if let Some(lock) = self.lock.take() {
            let _ = lock.unlock();
        }

        if let Err(e) = remove_dir(&self.dir) {
            wce_foundation::log::warn(format!("autosave: {} stays: {e}", self.dir.display()));
        }
    }
}

/// Copies a folder and what is in it.
///
/// # Errors
///
/// A file or folder can't be read or written.
pub fn copy_tree(from: &Path, to: &Path) -> io::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_tree(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), &target)?;
        }
    }

    Ok(())
}

fn slots_of(session: &Path) -> Vec<(PathBuf, AutosaveMeta)> {
    let Ok(read) = std::fs::read_dir(session) else { return vec![] };
    let mut out = read
        .flatten()
        .filter(|e| e.path().is_dir() && e.file_name() != RESTORED)
        .filter_map(|e| read_meta(&e.path()).map(|m| (e.path(), m)))
        .filter(|(_, m)| !m.copies.is_empty())
        .collect::<Vec<_>>();
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

/// A document's copies left by a session that did not end cleanly.
#[derive(Clone, Debug)]
pub struct Found {
    pub session: PathBuf,
    pub slot: PathBuf,
    pub meta: AutosaveMeta,
}

impl Found {
    /// A copy's folder.
    pub fn copy_dir(&self, copy: usize) -> Option<PathBuf> {
        self.meta.copies.get(copy).map(|c| self.slot.join(&c.folder))
    }

    /// The document's own file was written after its newest copy (saved since, maybe by another editor).
    pub fn saved_after(&self) -> bool {
        let newest = self.meta.newest().map_or(0, |c| c.time);
        written_secs(&self.meta.target).is_some_and(|t| t > newest)
    }
}

/// The copies left in `root` (`<projects>/autosave`) by sessions whose editor is gone, newest first; a running editor's are not
/// listed. Sessions left with nothing to offer are removed on the way.
pub fn stale(root: &Path) -> Vec<Found> {
    let Ok(read) = std::fs::read_dir(root) else { return vec![] };
    let mut out = vec![];
    for session in read.flatten().map(|e| e.path()).filter(|p| p.is_dir()) {
        if !session_ended(&session) {
            continue;
        }

        let slots = slots_of(&session);
        if slots.is_empty() {
            let _ = remove_dir(&session);
            continue;
        }

        out.extend(slots.into_iter().map(|(slot, meta)| Found { session: session.clone(), slot, meta }));
    }

    out.sort_by_key(|f| std::cmp::Reverse(f.meta.newest().map_or(0, |c| c.time)));
    out
}

/// A copy [`Session::take`] took that no document came of: back into its slot (copied there when it can't be moved), so it is offered
/// again.
pub fn give_back(found: &Found, copy: usize, taken: &Path) {
    let Some(back) = found.copy_dir(copy) else { return };
    // Already back (copied, not moved), moved back, or copied back: what is left in this session goes.
    let back_there = back.exists() || std::fs::rename(taken, &back).is_ok() || copy_tree(taken, &back).is_ok();
    if back_there {
        let _ = remove_dir(taken);
    }
}

/// Drops a document's copies of a gone session (and the session when nothing is left in it).
pub fn discard(found: &Found) {
    let _ = remove_dir(&found.slot);
    if slots_of(&found.session).is_empty() && session_ended(&found.session) {
        let _ = remove_dir(&found.session);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("wce_autosave_{name}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    fn meta(target: &str) -> AutosaveMeta {
        AutosaveMeta { tool: "t".into(), kind: "k".into(), title: "doc".into(), target: PathBuf::from(target), ..Default::default() }
    }

    #[test]
    fn copies_rotate_and_a_clean_end_leaves_nothing() {
        let root = scratch("rotate");
        let s = Session::start(&root).unwrap();
        let m = meta("/projects/a/doc.xml");
        for i in 0..5 {
            let written = s.write(&m, 3, |dir| std::fs::write(dir.join("doc.xml"), format!("{i}")).map_err(|e| e.to_string())).unwrap();
            assert_eq!(written.copies.len(), (i + 1).min(3));
        }

        let slots = s.slots();
        assert_eq!(slots.len(), 1);
        let folders = slots[0].copies.iter().map(|c| c.folder.as_str()).collect::<Vec<_>>();
        assert_eq!(folders, ["3", "4", "5"]);
        let slot = s.dir().join(slot_name(&m.key()));
        assert_eq!(std::fs::read_to_string(slot.join("5/doc.xml")).unwrap(), "4");
        assert!(!slot.join("1").exists() && !slot.join("2").exists());

        // A failed copy leaves the others as they were.
        assert!(s.write(&m, 3, |_| Err("no".into())).is_err());
        assert_eq!(s.slots()[0].copies.len(), 3);
        assert!(!slot.join("tmp_6").exists());

        // A running session is not offered.
        assert!(stale(&root).is_empty());
        s.end();
        assert!(std::fs::read_dir(&root).unwrap().next().is_none());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_session_that_did_not_end_is_offered_and_taken_back() {
        let root = scratch("stale");
        let crashed = Session::start(&root).unwrap();
        let m = meta("/projects/a/tile");
        crashed.write(&m, 3, |dir| std::fs::write(dir.join("tile.wce"), "x").map_err(|e| e.to_string())).unwrap();
        crashed.write(&meta("/projects/b.xml"), 3, |_| Ok(())).unwrap();

        // Gone without `end`: its lock is let go, its folder stays.
        drop(crashed);
        let found = stale(&root);
        assert_eq!(found.len(), 2);
        let tile = found.iter().find(|f| f.meta.key() == m.key()).unwrap();
        assert!(!tile.saved_after(), "the target does not exist");

        let mut now = Session::start(&root).unwrap();
        let taken = now.take(tile, 0).unwrap();
        assert_eq!(std::fs::read_to_string(taken.join("tile.wce")).unwrap(), "x");
        assert!(taken.starts_with(now.dir()));

        // Not taken back: offered again.
        give_back(tile, 0, &taken);
        assert_eq!(stale(&root).len(), 2);
        let taken = now.take(tile, 0).unwrap();
        discard(tile);
        assert!(taken.join("tile.wce").is_file());
        assert_eq!(stale(&root).len(), 1, "the other document is still offered");

        let other = stale(&root).remove(0);
        discard(&other);
        assert!(stale(&root).is_empty());
        assert!(!other.session.exists(), "the old session went with its last slot");
        now.end();
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn slot_names_are_file_safe_and_tell_keys_apart() {
        let a = slot_name("c:/projects/raw_data/terrain/tiles/battle/x/y/aa");
        assert!(a.starts_with("y_aa_"));
        assert_ne!(a, slot_name("c:/other/raw_data/terrain/tiles/battle/x/y/aa"));
        assert!(slot_name("weird:name?*").chars().all(|c| c.is_ascii_alphanumeric() || "._-".contains(c)));
    }
}
