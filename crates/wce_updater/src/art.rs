//! The art resources: a git repository per game (Settings > Game > Art resources) whose working tree is the projects' `raw_data/art` folder,
//! where a team shares models, CS2 files, prefabs and vegetation sets. Everything goes through the installed `git` (its credential helpers
//! sign in, Git LFS works when installed), never into a merge, rebase or stash state the editor would have to get out of:
//!
//! - [`setup`] makes the folder the repository's working tree; the files already there stay, those the repository has differently are backed
//!   up first, and the others become local changes.
//! - [`info`] fetches and tells the branch, the commits ahead and behind, the local changes and the files the team changed.
//! - [`update`] fast-forwards to the team's commits. A file changed both locally and by the team needs a [`Choice`] first ([`Error::Conflicts`]):
//!   theirs drops the local change, mine puts the local file back over theirs after the update (a local change again, published next time).
//! - [`publish`] updates, commits the files picked and pushes; when someone pushed in between, the commit is taken back and it starts again.

use std::collections::{BTreeMap, BTreeSet};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// What `git` is called as.
const GIT: &str = "git";

/// The remote the repository is fetched from and pushed to.
const REMOTE: &str = "origin";

/// The branch of a repository that has none yet, when the settings name none.
const DEFAULT_BRANCH: &str = "main";

/// Times [`publish`] starts again when the push is rejected because someone pushed first.
const PUBLISH_TRIES: usize = 3;

/// How a file differs from the last commit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChangeKind {
    Added,
    Modified,
    Deleted,
}

/// A local change: a path relative to the folder (with `/`), and how it changed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Change {
    pub path: String,
    pub kind: ChangeKind,
}

/// The repository as [`info`] finds it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RepoInfo {
    /// The branch checked out.
    pub branch: String,

    /// The remote's address.
    pub remote: Option<String>,

    /// The remote has the branch (a new repository has none until the first publish).
    pub on_remote: bool,

    /// Local commits not on the remote, and the remote's not here.
    pub ahead: usize,
    pub behind: usize,

    /// The local changes, by path.
    pub changes: Vec<Change>,

    /// The files the team's commits change (since the last update).
    pub incoming: Vec<String>,
}

impl RepoInfo {
    /// The files changed both locally and by the team's commits: an update needs a [`Choice`] for each.
    pub fn overlaps(&self) -> Vec<String> {
        let incoming = self.incoming.iter().collect::<BTreeSet<_>>();
        self.changes.iter().filter(|c| incoming.contains(&c.path)).map(|c| c.path.clone()).collect()
    }
}

/// Which version a file changed on both sides keeps.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Choice {
    /// The local file, put back over the team's after the update.
    Mine,

    /// The team's: the local change is dropped.
    Theirs,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    /// Files changed both locally and by the team, without a [`Choice`]: nothing was done.
    Conflicts(Vec<String>),
    Failed(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Conflicts(files) => write!(f, "{} file(s) changed both here and by the team: {}", files.len(), files.join(", ")),
            Self::Failed(e) => f.write_str(e),
        }
    }
}

impl From<String> for Error {
    fn from(e: String) -> Self {
        Self::Failed(e)
    }
}

/// Lines git writes while it works (fetch and push progress), for the window.
pub type ProgressFn<'a> = &'a (dyn Fn(&str) + Sync);

/// The installed git's version (`git version 2.43.0...`).
///
/// # Errors
///
/// git is not installed (or not on the PATH).
pub fn git_version() -> Result<String, String> {
    run(None, &["--version"], None)
        .map(|s| s.trim().to_owned())
        .map_err(|_| "git is not installed (or not on the PATH): https://git-scm.com".to_owned())
}

/// The folder is a repository's working tree (its own, not one above it).
pub fn is_repo(dir: &Path) -> bool {
    dir.join(".git").exists()
}

/// Makes `dir` the working tree of the repository at `url` (cloned into it, the files there kept), on `branch` (none: the remote's default,
/// else [`DEFAULT_BRANCH`]). A file the repository has, different here, is copied into `backup/<date and time>/` first; the files the repository
/// lacks stay as local changes. A folder that is a repository already gets `url` as its remote. Returns what was done.
///
/// # Errors
///
/// git failed (not installed, the address cannot be reached or signed in to); the folder is left as it was when the fetch failed.
pub fn setup(dir: &Path, url: &str, branch: Option<&str>, backup: &Path, progress: ProgressFn) -> Result<String, String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    let fresh = !is_repo(dir);
    if fresh {
        git(dir, &["init", "-q"])?;
    }

    match git(dir, &["remote", "get-url", REMOTE]) {
        Ok(u) if u.trim() == url => {}
        Ok(_) => drop(git(dir, &["remote", "set-url", REMOTE, url])?),
        Err(_) => drop(git(dir, &["remote", "add", REMOTE, url])?),
    }

    if let Err(e) = git_progress(dir, &["fetch", "--prune", "--progress", REMOTE], progress) {
        if fresh {
            let _ = std::fs::remove_dir_all(dir.join(".git"));
        }

        return Err(e);
    }

    let branch = match branch.map(str::trim).filter(|b| !b.is_empty()) {
        Some(b) => b.to_owned(),
        None => remote_default_branch(dir).unwrap_or_else(|| DEFAULT_BRANCH.to_owned()),
    };

    let remote_ref = format!("{REMOTE}/{branch}");
    let has_head = verify(dir, "HEAD");
    if !verify(dir, &format!("refs/remotes/{remote_ref}")) {
        // A repository without the branch yet: the first publish makes it.
        if !has_head {
            git(dir, &["symbolic-ref", "HEAD", &format!("refs/heads/{branch}")])?;
        }

        return Ok(format!("{url} has no branch {branch} yet: the files here are published to it as local changes."));
    }

    if has_head {
        // Set up before: the branch is followed, nothing else changes.
        if current_branch(dir).as_deref() != Some(branch.as_str()) {
            git(dir, &["checkout", "-q", &branch]).or_else(|_| git(dir, &["checkout", "-q", "-b", &branch, "--track", &remote_ref]))?;
        }

        return Ok(format!("The art folder follows {url} ({branch})."));
    }

    // The repository's files that are here with other contents are backed up before the checkout writes over them.
    let theirs = tree_blobs(dir, &remote_ref)?;
    let here = theirs.keys().filter(|p| dir.join(p).is_file()).cloned().collect::<Vec<_>>();
    let mine = hash_files(dir, &here)?;
    let differ = here.iter().zip(&mine).filter(|(p, h)| theirs.get(*p) != Some(*h)).map(|(p, _)| p.clone()).collect::<Vec<_>>();
    let mut note = String::new();
    if !differ.is_empty() {
        let to = backup.join(stamp());
        for p in &differ {
            let dst = to.join(p);
            if let Some(parent) = dst.parent() {
                std::fs::create_dir_all(parent).map_err(|e| format!("cannot create {}: {e}", parent.display()))?;
            }

            std::fs::copy(dir.join(p), &dst).map_err(|e| format!("cannot back up {p}: {e}"))?;
        }

        note = format!(" {} file(s) the repository has differently were backed up to {} first.", differ.len(), to.display());
    }

    git(dir, &["checkout", "-q", "-f", "-B", &branch, "--track", &remote_ref])?;
    let local = status(dir)?.len();
    Ok(format!("The art folder is {url} ({branch}) now: {} file(s) from it, {local} local change(s).{note}", theirs.len()))
}

/// The repository's state; `fetch` asks the remote for its commits first.
///
/// # Errors
///
/// Not a repository, or git failed (the fetch: the remote cannot be reached).
pub fn info(dir: &Path, fetch: bool, progress: ProgressFn) -> Result<RepoInfo, String> {
    if !is_repo(dir) {
        return Err(format!("{} is not set up", dir.display()));
    }

    if fetch {
        git_progress(dir, &["fetch", "--prune", "--progress", REMOTE], progress)?;
    }

    let branch = current_branch(dir).ok_or("no branch is checked out")?;
    let remote = git(dir, &["remote", "get-url", REMOTE]).ok().map(|u| u.trim().to_owned());
    let remote_ref = format!("{REMOTE}/{branch}");
    let on_remote = verify(dir, &format!("refs/remotes/{remote_ref}"));
    let has_head = verify(dir, "HEAD");
    let (mut ahead, mut behind, mut incoming) = (0, 0, vec![]);
    match (has_head, on_remote) {
        (true, true) => {
            let counts = git(dir, &["rev-list", "--left-right", "--count", &format!("HEAD...{remote_ref}")])?;
            let mut n = counts.split_whitespace().map(|v| v.parse::<usize>().unwrap_or(0));
            (ahead, behind) = (n.next().unwrap_or(0), n.next().unwrap_or(0));
            if behind > 0 {
                incoming = nul_list(&git(dir, &["diff", "--name-only", "-z", "--no-renames", &format!("HEAD...{remote_ref}")])?);
            }
        }

        (false, true) => {
            behind = git(dir, &["rev-list", "--count", &remote_ref])?.trim().parse().unwrap_or(0);
            incoming = tree_blobs(dir, &remote_ref)?.into_keys().collect();
        }

        (true, false) => ahead = git(dir, &["rev-list", "--count", "HEAD"])?.trim().parse().unwrap_or(0),
        (false, false) => {}
    }

    Ok(RepoInfo { branch, remote, on_remote, ahead, behind, changes: status(dir)?, incoming })
}

/// Brings the team's commits in (after a fetch: [`info`]), the local changes kept; a file changed on both sides goes as `choices` says.
/// Local commits not pushed (a publish that stopped between its commit and its push) become local changes again. Returns what was done.
///
/// # Errors
///
/// [`Error::Conflicts`]: files changed on both sides with no choice (nothing was done); else git failed.
pub fn update(dir: &Path, choices: &BTreeMap<String, Choice>) -> Result<String, Error> {
    let mut now = info(dir, false, &|_| {})?;
    if !now.on_remote || now.behind == 0 {
        return Ok("The art resources are up to date.".to_owned());
    }

    let remote_ref = format!("{REMOTE}/{}", now.branch);
    let overlaps = now.overlaps();
    let missing = overlaps.iter().filter(|p| !choices.contains_key(*p)).cloned().collect::<Vec<_>>();
    if !missing.is_empty() {
        return Err(Error::Conflicts(missing));
    }

    if now.ahead > 0 {
        uncommit_to(dir, git(dir, &["merge-base", "HEAD", &remote_ref])?.trim())?;
        now = info(dir, false, &|_| {})?;
        let missing = now.overlaps().into_iter().filter(|p| !choices.contains_key(p)).collect::<Vec<_>>();
        if !missing.is_empty() {
            return Err(Error::Conflicts(missing));
        }
    }

    // The files changed on both sides: the local ones kept are read, then every one goes back to the last commit for the update.
    let overlaps = now.overlaps();
    let mut kept = vec![];
    for p in &overlaps {
        if choices.get(p) == Some(&Choice::Mine) {
            let content = std::fs::read(dir.join(p)).ok();
            kept.push((p.clone(), content));
        }
    }

    discard(dir, &overlaps)?;
    if verify(dir, "HEAD") {
        git(dir, &["merge", "-q", "--ff-only", &remote_ref])?;
    } else {
        git(dir, &["checkout", "-q", "-B", &now.branch, "--track", &remote_ref])?;
    }

    for (p, content) in &kept {
        let path = dir.join(p);
        match content {
            Some(bytes) => {
                if let Some(parent) = path.parent() {
                    std::fs::create_dir_all(parent).map_err(|e| format!("cannot create {}: {e}", parent.display()))?;
                }

                std::fs::write(&path, bytes).map_err(|e| format!("cannot write {p} back: {e}"))?;
            }

            None => {
                if path.is_file() {
                    std::fs::remove_file(&path).map_err(|e| format!("cannot remove {p}: {e}"))?;
                }
            }
        }
    }

    let mut msg = format!("{} commit(s) from the team, {} file(s) changed.", now.behind, now.incoming.len());
    if !kept.is_empty() {
        msg.push_str(&format!(" {} file(s) of yours kept over theirs (publish them to share them).", kept.len()));
    }

    Ok(msg)
}

/// Shares `files` (local changes, paths as [`Change::path`]): the team's commits are brought in first ([`update`], with `choices`), then the
/// files are committed with `message` and pushed. A push rejected because someone pushed first takes the commit back and starts again.
///
/// # Errors
///
/// As [`update`]; or the commit or the push failed (the commit is taken back: the files stay local changes).
pub fn publish(
    dir: &Path,
    files: &[String],
    message: &str,
    choices: &BTreeMap<String, Choice>,
    progress: ProgressFn,
) -> Result<String, Error> {
    let message = message.trim();
    if message.is_empty() {
        return Err(Error::Failed("a message is needed: what the change is".to_owned()));
    }

    for attempt in 0..PUBLISH_TRIES {
        let before = info(dir, true, progress)?;
        if before.behind > 0 {
            update(dir, choices)?;
        }

        let now = info(dir, false, &|_| {})?;
        let changed = now.changes.iter().map(|c| c.path.as_str()).collect::<BTreeSet<_>>();
        let files = files.iter().filter(|f| changed.contains(f.as_str())).cloned().collect::<Vec<_>>();
        if files.is_empty() && now.ahead == 0 {
            return Ok("Nothing to publish.".to_owned());
        }

        // Only the files picked are committed.
        let base = git(dir, &["rev-parse", "-q", "--verify", "HEAD"]).ok().map(|s| s.trim().to_owned());
        if base.is_some() {
            git(dir, &["reset", "-q"])?;
        } else {
            let _ = git(dir, &["rm", "-q", "-r", "--cached", "--ignore-unmatch", "."]);
        }

        if !files.is_empty() {
            git_input(dir, &["add", "-A", "--pathspec-from-file=-", "--pathspec-file-nul"], &nul_join(&files))?;
            git(dir, &["commit", "-q", "-m", message])?;
        }

        let target = format!("HEAD:refs/heads/{}", now.branch);
        match git_progress(dir, &["push", "--progress", "-u", REMOTE, &target], progress) {
            Ok(()) => {
                return Ok(format!("{} file(s) published to {} ({}).", files.len(), now.remote.unwrap_or_default(), now.branch));
            }

            Err(e) => {
                // The commit goes back to being local changes: the next try (or the user) starts from where they were.
                if !files.is_empty() {
                    match &base {
                        Some(b) => uncommit_to(dir, b)?,
                        None => {
                            git(dir, &["update-ref", "-d", "HEAD"])?;
                            let _ = git(dir, &["rm", "-q", "-r", "--cached", "--ignore-unmatch", "."]);
                        }
                    }
                }

                // Someone pushed first (not a push the server refused: "[remote rejected]").
                let rejected = ["(fetch first)", "(non-fast-forward)"].iter().any(|w| e.contains(w));
                if !rejected || attempt + 1 == PUBLISH_TRIES {
                    return Err(Error::Failed(e));
                }
            }
        }
    }

    Err(Error::Failed("the team kept pushing: try again".to_owned()))
}

/// The name and e-mail commits are signed with, when git has them (the repository's or the user's).
pub fn identity(dir: &Path) -> Option<(String, String)> {
    let name = git(dir, &["config", "user.name"]).ok()?.trim().to_owned();
    let email = git(dir, &["config", "user.email"]).ok()?.trim().to_owned();
    (!name.is_empty() && !email.is_empty()).then_some((name, email))
}

/// Sets the name and e-mail of this repository's commits (its own config, not the user's).
///
/// # Errors
///
/// git failed.
pub fn set_identity(dir: &Path, name: &str, email: &str) -> Result<(), String> {
    git(dir, &["config", "user.name", name.trim()])?;
    git(dir, &["config", "user.email", email.trim()])?;
    Ok(())
}

// ---- the parts

/// The local changes (untracked files one by one).
fn status(dir: &Path) -> Result<Vec<Change>, String> {
    let out = git(dir, &["status", "--porcelain=v1", "-z", "--untracked-files=all", "--no-renames"])?;
    Ok(out
        .split('\0')
        .filter(|e| e.len() > 3)
        .map(|e| {
            let (xy, path) = e.split_at(3);
            let kind = if xy.starts_with("??") || xy.contains('A') {
                ChangeKind::Added
            } else if xy.contains('D') {
                ChangeKind::Deleted
            } else {
                ChangeKind::Modified
            };
            Change { path: path.to_owned(), kind }
        })
        .collect())
}

/// The local changes of `paths` go: the committed files come back, the new ones are deleted.
fn discard(dir: &Path, paths: &[String]) -> Result<(), String> {
    if paths.is_empty() {
        return Ok(());
    }

    // The ones the last commit has (a deleted file too: it is still in the index).
    let tracked = if verify(dir, "HEAD") {
        let all = nul_list(&git(dir, &["ls-files", "-z"])?).into_iter().collect::<BTreeSet<_>>();
        paths.iter().filter(|p| all.contains(*p)).cloned().collect()
    } else {
        vec![]
    };
    let tracked_set = tracked.iter().collect::<BTreeSet<_>>();
    if !tracked.is_empty() {
        git_input(dir, &["checkout", "-q", "HEAD", "--pathspec-from-file=-", "--pathspec-file-nul"], &nul_join(&tracked))?;
    }

    for p in paths.iter().filter(|p| !tracked_set.contains(p)) {
        let path = dir.join(p);
        if path.is_file() {
            std::fs::remove_file(&path).map_err(|e| format!("cannot remove {p}: {e}"))?;
        }
    }

    Ok(())
}

/// The commits after `base` become local changes again (nothing staged).
fn uncommit_to(dir: &Path, base: &str) -> Result<(), String> {
    git(dir, &["reset", "-q", "--soft", base])?;
    git(dir, &["reset", "-q"])?;
    Ok(())
}

fn current_branch(dir: &Path) -> Option<String> {
    git(dir, &["symbolic-ref", "--short", "-q", "HEAD"]).ok().map(|s| s.trim().to_owned()).filter(|s| !s.is_empty())
}

/// The remote's default branch, as it says (`ls-remote --symref`), else the one fetched when it has only one.
fn remote_default_branch(dir: &Path) -> Option<String> {
    let said = git(dir, &["ls-remote", "--symref", REMOTE, "HEAD"])
        .ok()
        .and_then(|out| out.lines().find_map(|l| l.strip_prefix("ref: refs/heads/").and_then(|r| r.split('\t').next()).map(str::to_owned)));
    said.or_else(|| {
        let refs = git(dir, &["for-each-ref", "--format=%(refname:strip=3)", &format!("refs/remotes/{REMOTE}/")]).ok()?;
        let names = refs.lines().filter(|l| *l != "HEAD" && !l.is_empty()).collect::<Vec<_>>();
        (names.len() == 1).then(|| names[0].to_owned())
    })
}

fn verify(dir: &Path, rev: &str) -> bool {
    git(dir, &["rev-parse", "-q", "--verify", rev]).is_ok()
}

/// The files of a commit's tree: path, blob hash.
fn tree_blobs(dir: &Path, rev: &str) -> Result<BTreeMap<String, String>, String> {
    let out = git(dir, &["ls-tree", "-r", "-z", rev])?;
    Ok(out
        .split('\0')
        .filter_map(|e| {
            let (meta, path) = e.split_once('\t')?;
            let mut m = meta.split_whitespace();
            let (_, kind, hash) = (m.next()?, m.next()?, m.next()?);
            (kind == "blob").then(|| (path.to_owned(), hash.to_owned()))
        })
        .collect())
}

/// The blob hashes the files would have (git's filters applied, as a commit of them would).
fn hash_files(dir: &Path, paths: &[String]) -> Result<Vec<String>, String> {
    if paths.is_empty() {
        return Ok(vec![]);
    }

    let mut list = paths.join("\n");
    list.push('\n');
    let out = git_with(dir, &["hash-object", "--stdin-paths"], Some(list.as_bytes()))?;
    Ok(out.lines().map(str::to_owned).collect())
}

fn nul_list(out: &str) -> Vec<String> {
    out.split('\0').filter(|s| !s.is_empty()).map(str::to_owned).collect()
}

fn nul_join(paths: &[String]) -> Vec<u8> {
    let mut out = vec![];
    for p in paths {
        out.extend_from_slice(p.as_bytes());
        out.push(0);
    }

    out
}

/// `YYYY-MM-DD_HH-MM-SS` (UTC) for a backup folder.
fn stamp() -> String {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
    let (days, rest) = (secs / 86_400, secs % 86_400);
    // Days since 1970-01-01 to a civil date (Howard Hinnant's algorithm).
    let z = days as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}_{:02}-{:02}-{:02}", rest / 3600, rest % 3600 / 60, rest % 60)
}

// ---- running git

fn command(dir: Option<&Path>, args: &[&str]) -> Command {
    let mut c = Command::new(GIT);
    // Paths as they are (not quoted), no prompt in a terminal nobody sees (the credential helper's own window still shows), English
    // messages (the rejected push is read from them).
    c.args(["-c", "core.quotepath=off"]).args(args);
    // Paths are paths, never patterns (a file name with `*` or `[`).
    c.env("GIT_TERMINAL_PROMPT", "0").env("LC_ALL", "C").env("LANGUAGE", "C").env("GIT_LITERAL_PATHSPECS", "1");
    if let Some(d) = dir {
        c.current_dir(d);
    }

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        c.creation_flags(CREATE_NO_WINDOW);
    }

    // The Flatpak has no git: the host's runs, with the user's credentials and keys.
    wce_foundation::sandbox::on_host(c)
}

fn run(dir: Option<&Path>, args: &[&str], input: Option<&[u8]>) -> Result<String, String> {
    let mut c = command(dir, args);
    c.stdin(if input.is_some() { Stdio::piped() } else { Stdio::null() }).stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = c.spawn().map_err(|e| format!("cannot run git: {e}"))?;
    if let (Some(bytes), Some(mut stdin)) = (input, child.stdin.take()) {
        stdin.write_all(bytes).map_err(|e| format!("git {}: {e}", args.join(" ")))?;
    }

    let out = child.wait_with_output().map_err(|e| format!("git {}: {e}", args.join(" ")))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        let err = String::from_utf8_lossy(&out.stderr).trim().to_owned();
        Err(if err.is_empty() { format!("git {} failed ({})", args.first().unwrap_or(&""), out.status) } else { err })
    }
}

fn git(dir: &Path, args: &[&str]) -> Result<String, String> {
    run(Some(dir), args, None)
}

fn git_with(dir: &Path, args: &[&str], input: Option<&[u8]>) -> Result<String, String> {
    run(Some(dir), args, input)
}

fn git_input(dir: &Path, args: &[&str], input: &[u8]) -> Result<String, String> {
    run(Some(dir), args, Some(input))
}

/// git with its progress lines (stderr, `\r` between the updates of one line) passed on as they come.
fn git_progress(dir: &Path, args: &[&str], progress: ProgressFn) -> Result<(), String> {
    let mut c = command(Some(dir), args);
    c.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::piped());
    let mut child = c.spawn().map_err(|e| format!("cannot run git: {e}"))?;
    let mut err = child.stderr.take().ok_or("git has no error output")?;
    let mut all = String::new();
    let mut line = vec![];
    let mut buf = [0u8; 1024];
    loop {
        let n = err.read(&mut buf).unwrap_or(0);
        if n == 0 {
            break;
        }

        for &b in &buf[..n] {
            if b == b'\r' || b == b'\n' {
                let text = String::from_utf8_lossy(&line).trim().to_owned();
                if !text.is_empty() {
                    progress(&text);
                    all.push_str(&text);
                    all.push('\n');
                }

                line.clear();
            } else {
                line.push(b);
            }
        }
    }

    let tail = String::from_utf8_lossy(&line).trim().to_owned();
    all.push_str(&tail);
    let status = child.wait().map_err(|e| format!("git {}: {e}", args.join(" ")))?;
    if status.success() {
        Ok(())
    } else {
        // The lines that say what went wrong, not the progress.
        let why = all
            .lines()
            .filter(|l| !l.contains('%') && !l.starts_with("remote: Counting") && !l.starts_with("remote: Compressing"))
            .collect::<Vec<_>>()
            .join("\n");
        Err(if why.trim().is_empty() { format!("git {} failed ({status})", args.first().unwrap_or(&"")) } else { why })
    }
}

/// The folder a game's art resources are backed up into when set up over files that differ ([`setup`]'s `backup`).
pub fn backup_dir(projects: &Path) -> PathBuf {
    projects.join("art_backup")
}
