//! The art resources' git work (`wce_updater::art`) against local repositories: a bare one as the team's remote, a clone as a teammate.
//! Skipped (passes) where git is not installed.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use wce_updater::art::{self, Choice, Error};

fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git").args(args).current_dir(dir).output().expect("git runs");
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn write(dir: &Path, path: &str, text: &str) {
    let p = dir.join(path);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, text).unwrap();
}

fn read(dir: &Path, path: &str) -> String {
    std::fs::read_to_string(dir.join(path)).unwrap()
}

/// A bare remote with `files` on `main`, pushed by a teammate's clone (returned too, to push more).
fn team(root: &Path, files: &[(&str, &str)]) -> (PathBuf, PathBuf) {
    let remote = root.join("remote.git");
    std::fs::create_dir_all(&remote).unwrap();
    git(&remote, &["init", "-q", "--bare", "-b", "main"]);
    let mate = root.join("mate");
    std::fs::create_dir_all(&mate).unwrap();
    git(&mate, &["init", "-q", "-b", "main"]);
    art::set_identity(&mate, "Mate", "mate@example.com").unwrap();
    git(&mate, &["remote", "add", "origin", remote.to_str().unwrap()]);
    if !files.is_empty() {
        for (p, t) in files {
            write(&mate, p, t);
        }

        git(&mate, &["add", "-A"]);
        git(&mate, &["commit", "-q", "-m", "first"]);
        git(&mate, &["push", "-q", "-u", "origin", "main"]);
    }

    (remote, mate)
}

fn mate_push(mate: &Path, files: &[(&str, &str)]) {
    git(mate, &["pull", "-q", "--ff-only", "origin", "main"]);
    for (p, t) in files {
        write(mate, p, t);
    }

    git(mate, &["add", "-A"]);
    git(mate, &["commit", "-q", "-m", "more"]);
    git(mate, &["push", "-q", "origin", "main"]);
}

fn no_progress(_: &str) {}

fn have_git() -> bool {
    art::git_version().is_ok()
}

#[test]
fn setup_keeps_the_files_there_and_backs_up_the_differing_ones() {
    if !have_git() {
        return;
    }

    let tmp = tempfile::tempdir().unwrap();
    let (remote, _) = team(tmp.path(), &[("prefabs/a.wce", "team a"), ("prefabs/b.wce", "same")]);
    let art_dir = tmp.path().join("art");
    write(&art_dir, "prefabs/a.wce", "my a");
    write(&art_dir, "prefabs/b.wce", "same");
    write(&art_dir, "prefabs/mine.wce", "only mine");
    let backup = tmp.path().join("backup");

    let msg = art::setup(&art_dir, remote.to_str().unwrap(), None, &backup, &no_progress).unwrap();
    assert!(msg.contains("1 file(s) the repository has differently"), "{msg}");
    assert_eq!(read(&art_dir, "prefabs/a.wce"), "team a");
    assert_eq!(read(&art_dir, "prefabs/mine.wce"), "only mine");
    let stamped = std::fs::read_dir(&backup).unwrap().next().unwrap().unwrap().path();
    assert_eq!(read(&stamped, "prefabs/a.wce"), "my a");

    let info = art::info(&art_dir, false, &no_progress).unwrap();
    assert_eq!(info.branch, "main");
    assert!(info.on_remote);
    assert_eq!((info.ahead, info.behind), (0, 0));
    assert_eq!(info.changes.iter().map(|c| c.path.as_str()).collect::<Vec<_>>(), ["prefabs/mine.wce"]);

    // Set up again: nothing changes.
    art::setup(&art_dir, remote.to_str().unwrap(), None, &backup, &no_progress).unwrap();
    assert_eq!(read(&art_dir, "prefabs/mine.wce"), "only mine");
}

#[test]
fn update_keeps_local_changes_and_asks_about_files_changed_on_both_sides() {
    if !have_git() {
        return;
    }

    let tmp = tempfile::tempdir().unwrap();
    let (remote, mate) = team(tmp.path(), &[("a.txt", "1"), ("b.txt", "1"), ("c.txt", "1")]);
    let art_dir = tmp.path().join("art");
    art::setup(&art_dir, remote.to_str().unwrap(), None, &tmp.path().join("backup"), &no_progress).unwrap();

    mate_push(&mate, &[("a.txt", "team 2"), ("b.txt", "team 2"), ("new.txt", "team new")]);
    write(&art_dir, "a.txt", "mine 2");
    write(&art_dir, "b.txt", "mine 2");
    write(&art_dir, "c.txt", "mine 2");

    let info = art::info(&art_dir, true, &no_progress).unwrap();
    assert_eq!(info.behind, 1);
    assert_eq!(info.overlaps(), ["a.txt", "b.txt"]);
    assert_eq!(art::update(&art_dir, &BTreeMap::new()), Err(Error::Conflicts(vec!["a.txt".to_owned(), "b.txt".to_owned()])));
    assert_eq!(read(&art_dir, "a.txt"), "mine 2", "nothing was done");

    let choices = BTreeMap::from([("a.txt".to_owned(), Choice::Mine), ("b.txt".to_owned(), Choice::Theirs)]);
    art::update(&art_dir, &choices).unwrap();
    assert_eq!(read(&art_dir, "a.txt"), "mine 2");
    assert_eq!(read(&art_dir, "b.txt"), "team 2");
    assert_eq!(read(&art_dir, "c.txt"), "mine 2");
    assert_eq!(read(&art_dir, "new.txt"), "team new");
    let info = art::info(&art_dir, false, &no_progress).unwrap();
    assert_eq!(info.behind, 0);
    let mut changed = info.changes.iter().map(|c| c.path.as_str()).collect::<Vec<_>>();
    changed.sort_unstable();
    assert_eq!(changed, ["a.txt", "c.txt"]);
}

#[test]
fn publish_brings_the_team_in_then_pushes_only_the_files_picked() {
    if !have_git() {
        return;
    }

    let tmp = tempfile::tempdir().unwrap();
    let (remote, mate) = team(tmp.path(), &[("a.txt", "1")]);
    let art_dir = tmp.path().join("art");
    art::setup(&art_dir, remote.to_str().unwrap(), None, &tmp.path().join("backup"), &no_progress).unwrap();
    art::set_identity(&art_dir, "Me", "me@example.com").unwrap();

    mate_push(&mate, &[("team.txt", "t")]);
    write(&art_dir, "share.txt", "s");
    write(&art_dir, "keep.txt", "k");
    assert!(art::publish(&art_dir, &["share.txt".to_owned()], "  ", &BTreeMap::new(), &no_progress).is_err(), "a message is needed");
    let msg = art::publish(&art_dir, &["share.txt".to_owned()], "Share a file", &BTreeMap::new(), &no_progress).unwrap();
    assert!(msg.starts_with("1 file(s) published"), "{msg}");

    git(&mate, &["pull", "-q", "--ff-only", "origin", "main"]);
    assert_eq!(read(&mate, "share.txt"), "s");
    assert!(!mate.join("keep.txt").exists());
    assert_eq!(git(&mate, &["log", "-1", "--format=%s %an"]).trim(), "Share a file Me");

    let info = art::info(&art_dir, true, &no_progress).unwrap();
    assert_eq!((info.ahead, info.behind), (0, 0));
    assert_eq!(info.changes.iter().map(|c| c.path.as_str()).collect::<Vec<_>>(), ["keep.txt"]);
}

#[test]
fn an_empty_repository_gets_its_branch_from_the_first_publish() {
    if !have_git() {
        return;
    }

    let tmp = tempfile::tempdir().unwrap();
    let (remote, _) = team(tmp.path(), &[]);
    let art_dir = tmp.path().join("art");
    write(&art_dir, "prefabs/p.wce", "p");
    let msg = art::setup(&art_dir, remote.to_str().unwrap(), Some("art"), &tmp.path().join("backup"), &no_progress).unwrap();
    assert!(msg.contains("no branch art yet"), "{msg}");
    let info = art::info(&art_dir, false, &no_progress).unwrap();
    assert_eq!((info.branch.as_str(), info.on_remote, info.changes.len()), ("art", false, 1));

    art::set_identity(&art_dir, "Me", "me@example.com").unwrap();
    art::publish(&art_dir, &["prefabs/p.wce".to_owned()], "First", &BTreeMap::new(), &no_progress).unwrap();
    let info = art::info(&art_dir, true, &no_progress).unwrap();
    assert!(info.on_remote);
    assert_eq!((info.ahead, info.behind, info.changes.len()), (0, 0, 0));
    assert!(git(&remote, &["branch", "--list", "art"]).contains("art"));
}

#[test]
fn a_rejected_push_leaves_the_files_as_local_changes() {
    if !have_git() {
        return;
    }

    let tmp = tempfile::tempdir().unwrap();
    let (remote, _) = team(tmp.path(), &[("a.txt", "1")]);
    let art_dir = tmp.path().join("art");
    art::setup(&art_dir, remote.to_str().unwrap(), None, &tmp.path().join("backup"), &no_progress).unwrap();
    art::set_identity(&art_dir, "Me", "me@example.com").unwrap();

    // The remote refuses every push.
    let hook = remote.join("hooks").join("pre-receive");
    std::fs::write(&hook, "#!/bin/sh\necho refused >&2\nexit 1\n").unwrap();
    write(&art_dir, "b.txt", "b");
    assert!(art::publish(&art_dir, &["b.txt".to_owned()], "Add b", &BTreeMap::new(), &no_progress).is_err());
    let info = art::info(&art_dir, false, &no_progress).unwrap();
    assert_eq!(info.ahead, 0, "the commit was taken back");
    assert_eq!(info.changes.iter().map(|c| c.path.as_str()).collect::<Vec<_>>(), ["b.txt"]);
}
