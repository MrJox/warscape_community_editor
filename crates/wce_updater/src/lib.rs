//! The self-updater. The release workflow (.github/workflows/release.yml) publishes a GitHub release per `vX.Y.Z` tag with the Windows build
//! as `wce-X.Y.Z-windows-x64.zip` (the files at the zip's root); a tag with a pre-release part (`v0.2.0-beta.1`) is a pre-release, offered only
//! when the settings ask for them. [`check`] finds the newest release above the running version, [`download`] fetches its zip with progress,
//! and [`install`] puts the zip's files over the running build: each file in use is renamed aside (Windows lets a running exe or a loaded DLL
//! be renamed, not overwritten) and [`remove_leftovers`] deletes those on the next start (only the files listed, so a development build's
//! folder is not searched). Nothing here draws: the editor runs these on a thread.

use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;

pub use semver::Version;
use serde::Deserialize;

/// The GitHub repository the releases come from.
pub const REPO: &str = "MrJox/warscape_community_editor";

/// What a file replaced by an update is renamed to, until the next start removes it: `<name>.wce-old`.
const LEFTOVER_SUFFIX: &str = ".wce-old";

/// The folder an update is unpacked into before its files are put in place, in the build's folder.
const STAGING: &str = ".wce-update";

/// The list of the files an update renamed aside (one path per line, relative to the build's folder), for [`remove_leftovers`].
const LEFTOVERS: &str = ".wce-update-leftovers";

/// The running build's version.
pub fn current_version() -> Version {
    Version::parse(env!("CARGO_PKG_VERSION")).expect("the workspace version is semver")
}

/// The release asset with this platform's build of `version`; none where no build is published (the editor is built from source there).
pub fn asset_name(version: &Version) -> Option<String> {
    (cfg!(windows) && cfg!(target_arch = "x86_64")).then(|| format!("wce-{version}-windows-x64.zip"))
}

/// A release newer than the running build.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Release {
    pub version: Version,
    pub tag: String,

    /// The release's title (its tag when it has none).
    pub name: String,

    /// The release notes (markdown).
    pub notes: String,

    /// The release's page on GitHub.
    pub page: String,

    /// This platform's build; none on a platform without one.
    pub asset: Option<Asset>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Asset {
    pub name: String,
    pub url: String,
    pub size: u64,
}

#[derive(Deserialize)]
struct GhRelease {
    tag_name: String,
    name: Option<String>,
    body: Option<String>,
    html_url: String,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
    #[serde(default)]
    assets: Vec<GhAsset>,
}

#[derive(Deserialize)]
struct GhAsset {
    name: String,
    browser_download_url: String,
    size: u64,
}

/// Download progress, shared between the downloading thread and the window showing it.
#[derive(Debug, Default)]
pub struct Progress {
    /// Bytes received.
    pub done: AtomicU64,

    /// Bytes expected (0: not known yet).
    pub total: AtomicU64,

    /// Set to stop the download.
    pub cancel: AtomicBool,
}

impl Progress {
    /// 0..1, or none while the size is not known.
    pub fn fraction(&self) -> Option<f32> {
        let total = self.total.load(Ordering::Relaxed);
        (total > 0).then(|| (self.done.load(Ordering::Relaxed) as f64 / total as f64).min(1.0) as f32)
    }
}

/// An HTTP client; `whole` bounds the whole request (the release list), else only connecting is (a download takes what it takes).
fn agent(whole: Option<Duration>) -> ureq::Agent {
    ureq::Agent::config_builder().user_agent(format!("wce/{}", env!("CARGO_PKG_VERSION"))).timeout_connect(Some(Duration::from_secs(15)))
        .timeout_global(whole).build().into()
}

/// Asks GitHub for the releases and returns the newest one above `current`, if any. Pre-releases count only with `prereleases`.
///
/// # Errors
///
/// GitHub can't be reached or answers with something that isn't its release list.
pub fn check(current: &Version, prereleases: bool) -> Result<Option<Release>, String> {
    let url = format!("https://api.github.com/repos/{REPO}/releases?per_page=30");
    let text = agent(Some(Duration::from_secs(30))).get(&url).header("Accept", "application/vnd.github+json").call()
        .and_then(|mut r| r.body_mut().read_to_string())
        .map_err(|e| format!("cannot reach GitHub: {e}"))?;
    newest(&text, current, prereleases, asset_name)
}

/// The newest release of GitHub's release list (`json`) above `current`. Drafts, tags that aren't `v<semver>` and, without `prereleases`,
/// pre-releases are left out; so are releases without this platform's asset where `asset_for` names one.
///
/// # Errors
///
/// `json` isn't a release list.
pub fn newest(json: &str, current: &Version, prereleases: bool, asset_for: fn(&Version) -> Option<String>) -> Result<Option<Release>, String> {
    let list: Vec<GhRelease> = serde_json::from_str(json).map_err(|e| format!("GitHub's release list can't be read: {e}"))?;
    let mut best: Option<Release> = None;
    for r in list {
        let Some(version) = r.tag_name.strip_prefix('v').and_then(|v| Version::parse(v).ok()) else { continue };
        if r.draft || version <= *current || (!prereleases && (r.prerelease || !version.pre.is_empty())) {
            continue;
        }

        let asset = match asset_for(&version) {
            Some(name) => match r.assets.into_iter().find(|a| a.name == name) {
                Some(a) => Some(Asset { name: a.name, url: a.browser_download_url, size: a.size }),
                None => continue,
            },

            None => None,
        };

        if best.as_ref().is_none_or(|b| version > b.version) {
            let name = r.name.filter(|n| !n.trim().is_empty()).unwrap_or_else(|| r.tag_name.clone());
            best = Some(Release { version, tag: r.tag_name, name, notes: r.body.unwrap_or_default(), page: r.html_url, asset });
        }
    }

    Ok(best)
}

/// Downloads `asset` to `dest`, counting into `progress`; stops (and removes the part written) when `progress.cancel` is set.
///
/// # Errors
///
/// The download fails, is cancelled, or doesn't have the asset's size.
pub fn download(asset: &Asset, dest: &Path, progress: &Progress) -> Result<(), String> {
    let result = download_to(asset, dest, progress);
    if result.is_err() {
        let _ = std::fs::remove_file(dest);
    }

    result
}

fn download_to(asset: &Asset, dest: &Path, progress: &Progress) -> Result<(), String> {
    let response = agent(None).get(&asset.url).call().map_err(|e| format!("cannot download {}: {e}", asset.name))?;
    let length = response.headers().get("content-length").and_then(|v| v.to_str().ok()?.parse::<u64>().ok());
    progress.done.store(0, Ordering::Relaxed);
    progress.total.store(length.unwrap_or(asset.size), Ordering::Relaxed);

    let mut reader = response.into_body().into_reader();
    let mut file = File::create(dest).map_err(|e| format!("cannot write {}: {e}", dest.display()))?;
    let mut buf = vec![0; 64 * 1024];
    let mut done = 0u64;
    loop {
        if progress.cancel.load(Ordering::Relaxed) {
            return Err("download cancelled".to_owned());
        }

        let n = reader.read(&mut buf).map_err(|e| format!("cannot download {}: {e}", asset.name))?;
        if n == 0 {
            break;
        }

        file.write_all(&buf[..n]).map_err(|e| format!("cannot write {}: {e}", dest.display()))?;
        done += n as u64;
        progress.done.store(done, Ordering::Relaxed);
    }

    file.flush().map_err(|e| format!("cannot write {}: {e}", dest.display()))?;
    if asset.size > 0 && done != asset.size {
        return Err(format!("{} is incomplete: {done} of {} bytes", asset.name, asset.size));
    }

    Ok(())
}

/// The folder of the running build: where [`install`] puts an update.
///
/// # Errors
///
/// The executable's path isn't known.
pub fn install_dir() -> Result<PathBuf, String> {
    let exe = std::env::current_exe().map_err(|e| format!("cannot find the editor's executable: {e}"))?;
    exe.parent().map(Path::to_path_buf).ok_or_else(|| format!("{} has no folder", exe.display()))
}

/// Puts the files of the update `zip` into `dir`. The zip is unpacked whole into a staging folder first (a broken zip changes nothing); then
/// each file already in `dir` is renamed to `<name>.wce-old` and the new one moved in its place. A failure puts back the files moved so far.
/// Returns the files installed, relative to `dir`.
///
/// # Errors
///
/// The zip can't be read, has a path outside its root, or a file can't be moved.
pub fn install(zip: &Path, dir: &Path) -> Result<Vec<PathBuf>, String> {
    let staging = dir.join(STAGING);
    let _ = std::fs::remove_dir_all(&staging);
    let files = unpack(zip, &staging);
    let result = files.and_then(|files| {
        let mut moved: Vec<(PathBuf, Option<PathBuf>)> = Vec::new();
        for rel in &files {
            match replace(&staging.join(rel), &dir.join(rel)) {
                Ok(aside) => moved.push((dir.join(rel), aside)),
                Err(e) => {
                    for (target, aside) in moved.into_iter().rev() {
                        let _ = std::fs::remove_file(&target);
                        if let Some(aside) = aside {
                            let _ = std::fs::rename(&aside, &target);
                        }
                    }

                    return Err(e);
                }
            }
        }

        let asides = moved.iter().filter_map(|(_, aside)| aside.as_ref()?.strip_prefix(dir).ok().map(Path::to_path_buf));
        add_leftovers(dir, asides);
        Ok(files)
    });

    let _ = std::fs::remove_dir_all(&staging);
    result
}

/// The files of [`LEFTOVERS`] in `dir`.
fn leftovers(dir: &Path) -> Vec<PathBuf> {
    std::fs::read_to_string(dir.join(LEFTOVERS)).unwrap_or_default().lines().filter(|l| !l.is_empty()).map(PathBuf::from).collect()
}

/// Writes `files` as [`LEFTOVERS`] in `dir` (none: removes it).
fn write_leftovers(dir: &Path, files: &[PathBuf]) {
    let path = dir.join(LEFTOVERS);
    if files.is_empty() {
        let _ = std::fs::remove_file(path);
    } else {
        let text = files.iter().map(|f| format!("{}\n", f.display())).collect::<String>();
        let _ = std::fs::write(path, text);
    }
}

fn add_leftovers(dir: &Path, new: impl Iterator<Item = PathBuf>) {
    let mut files = leftovers(dir);
    for f in new {
        if !files.contains(&f) {
            files.push(f);
        }
    }

    write_leftovers(dir, &files);
}

/// Unpacks `zip` into `into`; the files' paths, relative to it.
fn unpack(zip: &Path, into: &Path) -> Result<Vec<PathBuf>, String> {
    let file = File::open(zip).map_err(|e| format!("cannot open {}: {e}", zip.display()))?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| format!("{} is not a zip: {e}", zip.display()))?;
    let mut files = Vec::new();
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).map_err(|e| format!("cannot read {}: {e}", zip.display()))?;
        let Some(rel) = entry.enclosed_name() else {
            return Err(format!("{} has a file outside its folder: {}", zip.display(), entry.name_raw().escape_ascii()));
        };

        let out = into.join(&rel);
        if entry.is_dir() {
            std::fs::create_dir_all(&out).map_err(|e| format!("cannot create {}: {e}", out.display()))?;
            continue;
        }

        if let Some(parent) = out.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("cannot create {}: {e}", parent.display()))?;
        }

        let mut dest = File::create(&out).map_err(|e| format!("cannot write {}: {e}", out.display()))?;
        std::io::copy(&mut entry, &mut dest).map_err(|e| format!("cannot unpack {}: {e}", rel.display()))?;
        files.push(rel);
    }

    if files.is_empty() {
        return Err(format!("{} is empty", zip.display()));
    }

    Ok(files)
}

/// Moves `new` to `target`, renaming a file already there aside first; that file's new path.
fn replace(new: &Path, target: &Path) -> Result<Option<PathBuf>, String> {
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("cannot create {}: {e}", parent.display()))?;
    }

    let aside = if target.exists() {
        // An update installed without restarting leaves the previous one aside, still in use: the next free name then.
        let mut aside = leftover_name(target, 0);
        let mut n = 0;
        while aside.exists() && std::fs::remove_file(&aside).is_err() {
            n += 1;
            aside = leftover_name(target, n);
        }

        std::fs::rename(target, &aside).map_err(|e| format!("cannot move {} aside: {e}", target.display()))?;
        Some(aside)
    } else {
        None
    };

    if let Err(e) = std::fs::rename(new, target) {
        if let Some(aside) = &aside {
            let _ = std::fs::rename(aside, target);
        }

        return Err(format!("cannot put {} in place: {e}", target.display()));
    }

    Ok(aside)
}

fn leftover_name(target: &Path, n: u32) -> PathBuf {
    let mut name = target.as_os_str().to_owned();
    if n > 0 {
        name.push(format!(".{n}"));
    }

    name.push(LEFTOVER_SUFFIX);
    PathBuf::from(name)
}

/// Deletes what an update left in `dir`: the files it renamed aside, an interrupted staging folder. Files still in use stay listed for the
/// next time.
pub fn remove_leftovers(dir: &Path) {
    let _ = std::fs::remove_dir_all(dir.join(STAGING));
    let files = leftovers(dir);
    if files.is_empty() {
        return;
    }

    let kept: Vec<PathBuf> = files.into_iter().filter(|f| {
        let path = dir.join(f);
        path.exists() && std::fs::remove_file(&path).is_err()
    }).collect();

    write_leftovers(dir, &kept);
}

#[cfg(test)]
mod tests {
    use super::*;

    const LIST: &str = r#"[
        {"tag_name": "v0.3.0-beta.1", "name": "", "body": "beta", "html_url": "https://x/b", "prerelease": true,
         "assets": [{"name": "wce-0.3.0-beta.1-windows-x64.zip", "browser_download_url": "https://x/b.zip", "size": 10}]},
        {"tag_name": "v0.2.1", "name": "Two one", "body": "notes", "html_url": "https://x/a", "draft": false, "prerelease": false,
         "assets": [{"name": "wce-0.2.1-windows-x64.zip", "browser_download_url": "https://x/a.zip", "size": 5}]},
        {"tag_name": "v0.2.2", "html_url": "https://x/c", "prerelease": false, "assets": []},
        {"tag_name": "v9.0.0", "html_url": "https://x/d", "draft": true,
         "assets": [{"name": "wce-9.0.0-windows-x64.zip", "browser_download_url": "https://x/d.zip", "size": 1}]},
        {"tag_name": "nightly", "html_url": "https://x/e", "assets": []}
    ]"#;

    fn windows(v: &Version) -> Option<String> {
        Some(format!("wce-{v}-windows-x64.zip"))
    }

    fn none(_: &Version) -> Option<String> {
        None
    }

    fn v(s: &str) -> Version {
        Version::parse(s).unwrap()
    }

    #[test]
    fn newest_stable_with_the_asset() {
        let r = newest(LIST, &v("0.1.0"), false, windows).unwrap().unwrap();
        assert_eq!(r.version, v("0.2.1"));
        assert_eq!(r.name, "Two one");
        assert_eq!(r.asset, Some(Asset { name: "wce-0.2.1-windows-x64.zip".into(), url: "https://x/a.zip".into(), size: 5 }));
    }

    #[test]
    fn prereleases_only_when_asked() {
        let r = newest(LIST, &v("0.1.0"), true, windows).unwrap().unwrap();
        assert_eq!(r.version, v("0.3.0-beta.1"));
        assert_eq!(r.name, "v0.3.0-beta.1");
    }

    #[test]
    fn without_a_platform_asset_any_release_counts() {
        assert_eq!(newest(LIST, &v("0.1.0"), false, none).unwrap().unwrap().version, v("0.2.2"));
    }

    #[test]
    fn nothing_newer() {
        assert_eq!(newest(LIST, &v("0.2.1"), false, windows).unwrap(), None);
        assert!(newest("{}", &v("0.1.0"), false, windows).is_err());
    }

    /// Asks the real GitHub (`cargo test -p wce_updater -- --ignored`).
    #[test]
    #[ignore = "needs the network"]
    fn live_check() {
        let r = check(&v("0.0.1"), true);
        println!("{r:?}");
        assert!(r.is_ok());
    }

    fn write_zip(path: &Path, files: &[(&str, &[u8])]) {
        let mut zip = zip::ZipWriter::new(File::create(path).unwrap());
        for (name, data) in files {
            zip.start_file(*name, zip::write::SimpleFileOptions::default()).unwrap();
            zip.write_all(data).unwrap();
        }

        zip.finish().unwrap();
    }

    #[test]
    fn install_replaces_and_keeps_the_old_aside() {
        let dir = tempfile::tempdir().unwrap();
        let app = dir.path().join("app");
        std::fs::create_dir_all(&app).unwrap();
        std::fs::write(app.join("wce.exe"), b"old").unwrap();
        std::fs::write(app.join("keep.txt"), b"mine").unwrap();
        let zip = dir.path().join("u.zip");
        write_zip(&zip, &[("wce.exe", b"new"), ("SDL3.dll", b"sdl"), ("docs/README.md", b"doc")]);

        let files = install(&zip, &app).unwrap();
        assert_eq!(files.len(), 3);
        assert_eq!(std::fs::read(app.join("wce.exe")).unwrap(), b"new");
        assert_eq!(std::fs::read(app.join("wce.exe.wce-old")).unwrap(), b"old");
        assert_eq!(std::fs::read(app.join("docs/README.md")).unwrap(), b"doc");
        assert_eq!(std::fs::read(app.join("keep.txt")).unwrap(), b"mine");
        assert!(!app.join(STAGING).exists());

        // A second update before a restart: the first one's leftover is replaced.
        install(&zip, &app).unwrap();
        assert_eq!(std::fs::read(app.join("wce.exe.wce-old")).unwrap(), b"new");

        remove_leftovers(&app);
        assert!(!app.join("wce.exe.wce-old").exists() && !app.join("docs/README.md.wce-old").exists());
        assert!(app.join("wce.exe").exists() && app.join("keep.txt").exists() && !app.join(LEFTOVERS).exists());
    }

    #[test]
    fn a_broken_zip_changes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("wce.exe"), b"old").unwrap();
        let zip = dir.path().join("u.zip");
        std::fs::write(&zip, b"not a zip").unwrap();
        assert!(install(&zip, dir.path()).is_err());
        assert_eq!(std::fs::read(dir.path().join("wce.exe")).unwrap(), b"old");
        assert!(!dir.path().join("wce.exe.wce-old").exists());
    }
}
