//! rpfm's schemas (the db tables' definitions, `schema_att.ron`...), which some features need to read the games' tables. They come from
//! rpfm's schema repository: [`latest`] asks GitHub for its newest commit, [`download`] fetches the games' schema files of that commit into a
//! folder (the editor's own, `Paths::schemas_dir`) and records the commit there, and [`installed`] tells the commit a folder has (the
//! editor's record, or the git checkout rpfm keeps in its own schemas folder).

use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::time::Duration;

use crate::{Progress, agent};

/// rpfm's schema repository on GitHub, and its branch.
pub const REPO: &str = "Frodo45127/rpfm-schemas";
pub const BRANCH: &str = "master";

/// The file of a schemas folder the editor downloaded into, with the commit its files are from.
const VERSION_FILE: &str = ".wce-schemas-version";

/// The newest commit of the schema repository (its full hash).
///
/// # Errors
///
/// GitHub can't be reached or doesn't answer with a commit hash.
pub fn latest() -> Result<String, String> {
    let url = format!("https://api.github.com/repos/{REPO}/commits/{BRANCH}");
    let text = agent(Some(Duration::from_secs(30))).get(&url).header("Accept", "application/vnd.github.sha").call()
        .and_then(|mut r| r.body_mut().read_to_string())
        .map_err(|e| format!("cannot reach GitHub: {e}"))?;
    let sha = text.trim();
    if sha.len() == 40 && sha.bytes().all(|b| b.is_ascii_hexdigit()) {
        Ok(sha.to_lowercase())
    } else {
        Err("GitHub's answer is not a commit".to_owned())
    }
}

/// The commit the schemas of `dir` are from: the editor's record of a download, else (rpfm's own folder, a git checkout) its checked out
/// commit. None when neither tells.
pub fn installed(dir: &Path) -> Option<String> {
    if let Ok(text) = std::fs::read_to_string(dir.join(VERSION_FILE)) {
        let sha = text.trim();
        if !sha.is_empty() {
            return Some(sha.to_lowercase());
        }
    }

    git_head(dir)
}

/// The commit checked out in the git checkout `dir`: `.git/HEAD`, a hash or a branch's ref (a loose ref file, else `packed-refs`).
fn git_head(dir: &Path) -> Option<String> {
    let git = dir.join(".git");
    let head = std::fs::read_to_string(git.join("HEAD")).ok()?;
    let head = head.trim();
    let Some(name) = head.strip_prefix("ref:").map(str::trim) else { return Some(head.to_lowercase()) };
    if let Ok(sha) = std::fs::read_to_string(git.join(name)) {
        return Some(sha.trim().to_lowercase());
    }

    let packed = std::fs::read_to_string(git.join("packed-refs")).ok()?;
    packed.lines().find_map(|l| l.strip_suffix(name).map(|sha| sha.trim().to_lowercase())).filter(|sha| !sha.is_empty())
}

/// Downloads the schema `files` (like `schema_att.ron`) of `commit` into `dir`, counting the files done into `progress` (`total`: the
/// number of files); each file is written aside and moved in place once whole, and the commit is recorded last. Stops when
/// `progress.cancel` is set. Returns the files written.
///
/// # Errors
///
/// A file can't be downloaded or written, or the download was cancelled (the files moved in by then stay: each is whole).
pub fn download(dir: &Path, commit: &str, files: &[&str], progress: &Progress) -> Result<Vec<PathBuf>, String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    progress.done.store(0, Ordering::Relaxed);
    progress.total.store(files.len() as u64, Ordering::Relaxed);
    let mut written = vec![];
    for (i, name) in files.iter().enumerate() {
        if progress.cancel.load(Ordering::Relaxed) {
            return Err("download cancelled".to_owned());
        }

        let url = format!("https://raw.githubusercontent.com/{REPO}/{commit}/{name}");
        let mut bytes = vec![];
        agent(None).get(&url).call().map_err(|e| format!("cannot download {name}: {e}"))?
            .into_body().into_reader().read_to_end(&mut bytes).map_err(|e| format!("cannot download {name}: {e}"))?;

        let target = dir.join(name);
        let part = dir.join(format!("{name}.part"));
        std::fs::write(&part, &bytes).map_err(|e| format!("cannot write {}: {e}", part.display()))?;
        if let Err(e) = std::fs::rename(&part, &target) {
            let _ = std::fs::remove_file(&part);
            return Err(format!("cannot write {}: {e}", target.display()));
        }

        written.push(target);
        progress.done.store(i as u64 + 1, Ordering::Relaxed);
    }

    std::fs::write(dir.join(VERSION_FILE), format!("{commit}\n")).map_err(|e| format!("cannot write {}: {e}", dir.join(VERSION_FILE).display()))?;
    Ok(written)
}

/// A commit's short form, for people.
pub fn short(commit: &str) -> &str {
    commit.get(..7).unwrap_or(commit)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn installed_reads_the_record_then_a_git_checkout() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(installed(dir.path()), None);

        let git = dir.path().join(".git");
        std::fs::create_dir_all(git.join("refs/heads")).unwrap();
        std::fs::write(git.join("HEAD"), "ref: refs/heads/master\n").unwrap();
        std::fs::write(git.join("packed-refs"), "# pack-refs with: peeled\nABCDEF0123 refs/heads/master\n").unwrap();
        assert_eq!(installed(dir.path()).as_deref(), Some("abcdef0123"));

        std::fs::write(git.join("refs/heads/master"), "1234567890\n").unwrap();
        assert_eq!(installed(dir.path()).as_deref(), Some("1234567890"));

        std::fs::write(dir.path().join(VERSION_FILE), "FEDCBA\n").unwrap();
        assert_eq!(installed(dir.path()).as_deref(), Some("fedcba"));
        assert_eq!(short("0123456789abcdef"), "0123456");
    }
}
