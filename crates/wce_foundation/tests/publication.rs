use std::{fs, sync::Arc};
use wce_foundation::{
    publication::{Output, Source, Staged, resource_path},
    save_path,
};

#[test]
fn destination_keys_normalize_segments_and_missing_parents_without_creating_them() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("future/native.bin");
    let alias = dir.path().join("unused/../future/./native.bin");
    let canonical_parent = dir.path().canonicalize().unwrap().join("future/native.bin");
    assert_eq!(
        save_path::key(&target).unwrap(),
        save_path::key(&alias).unwrap()
    );
    assert_eq!(
        save_path::key(&target).unwrap(),
        save_path::key(&canonical_parent).unwrap()
    );
    assert!(!dir.path().join("future").exists());
    assert!(!dir.path().join("unused").exists());
}

#[cfg(windows)]
#[test]
fn windows_destination_keys_ignore_case() {
    let dir = tempfile::tempdir().unwrap();
    assert_eq!(
        save_path::key(&dir.path().join("Future/Native.bin")).unwrap(),
        save_path::key(&dir.path().join("future/native.BIN")).unwrap()
    );
}

#[test]
fn normalized_duplicate_outputs_fail_before_creating_temporaries() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("native.bin");
    fs::write(&target, b"original").unwrap();
    let error = Staged::prepare(vec![
        Output::new(target.clone(), Source::Bytes(Arc::from(&b"first"[..]))),
        Output::new(
            dir.path().join("unused/../native.bin"),
            Source::Bytes(Arc::from(&b"second"[..])),
        ),
    ])
    .err()
    .expect("duplicate destination must be refused");
    assert!(error.message.contains("duplicate publication destination"));
    assert!(error.published.is_empty());
    assert_eq!(fs::read(&target).unwrap(), b"original");
    assert!(!target.with_added_extension("wce_tmp").exists());
    assert!(!dir.path().join("unused").exists());
}

#[test]
fn file_sources_compare_bytes_instead_of_length_and_retain_exact_matches() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("source.bin");
    let target = dir.path().join("native.bin");
    fs::write(&source, b"new!").unwrap();
    fs::write(&target, b"old!").unwrap();
    let outputs = || vec![Output::new(target.clone(), Source::File(source.clone()))];
    let report = Staged::prepare(outputs()).unwrap().publish().unwrap();
    assert_eq!(report.published, vec![target.clone()]);
    assert_eq!(fs::read(&target).unwrap(), b"new!");
    let modified = fs::metadata(&target).unwrap().modified().unwrap();
    let report = Staged::prepare(outputs()).unwrap().publish().unwrap();
    assert!(report.published.is_empty());
    assert_eq!(report.retained, vec![target.clone()]);
    assert_eq!(fs::metadata(&target).unwrap().modified().unwrap(), modified);
}

#[test]
fn dropping_a_staged_plan_removes_temporaries_and_preserves_the_destination() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("native.bin");
    fs::write(&target, b"original").unwrap();
    let staged = Staged::prepare(vec![Output::new(
        target.clone(),
        Source::Bytes(Arc::from(&b"new"[..])),
    )])
    .unwrap();
    assert!(target.with_added_extension("wce_tmp").is_file());
    assert_eq!(fs::read(&target).unwrap(), b"original");
    drop(staged);
    assert!(!target.with_added_extension("wce_tmp").exists());
    assert_eq!(fs::read(&target).unwrap(), b"original");
}

#[test]
fn resource_keys_reject_ambiguous_names_and_reserved_components() {
    let dir = tempfile::tempdir().unwrap();
    for key in [
        "",
        ".",
        "./file",
        "a//b",
        "a/..",
        "a.",
        "a ",
        "a:stream",
        "a<",
        "a\n",
        "sub.WCE_TMP/file",
        "sub/file.wce_tmp",
        "a\\b",
        "/absolute",
        "C:/absolute",
        "a/LPT9.bin",
    ] {
        assert!(resource_path(dir.path(), key).is_err(), "accepted {key:?}");
    }
    assert_eq!(
        resource_path(dir.path(), "nested/native.bin").unwrap(),
        dir.path().join("nested/native.bin")
    );
}

#[test]
fn publication_priorities_are_supplied_by_the_consumer() {
    for name in [
        "tile.wce",
        "lf_height_map.data",
        "lf_height_map.dds",
        "metadata.data",
    ] {
        let output = Output::new(name.into(), Source::Bytes(Arc::from(&b"data"[..])));
        assert_eq!(output.priority, 0);
    }
}

#[test]
fn same_file_and_its_path_alias_are_retained_without_replacement() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("native.bin");
    fs::write(&path, b"original").unwrap();
    let before = fs::metadata(&path).unwrap().modified().unwrap();
    for source in [path.clone(), dir.path().join("./native.bin")] {
        let report = Staged::prepare(vec![Output::new(path.clone(), Source::File(source))])
            .unwrap()
            .publish()
            .unwrap();
        assert_eq!(report.retained, [path.clone()]);
        assert!(report.published.is_empty());
        assert!(!path.with_added_extension("wce_tmp").exists());
        assert_eq!(fs::metadata(&path).unwrap().modified().unwrap(), before);
    }
    assert!(
        Staged::prepare(vec![Output::new(
            dir.path().join("missing"),
            Source::File(dir.path().join("missing"))
        )])
        .is_err()
    );
    assert!(
        Staged::prepare(vec![Output::new(
            dir.path().to_path_buf(),
            Source::File(dir.path().to_path_buf())
        )])
        .is_err()
    );
}

#[cfg(windows)]
#[test]
fn same_file_retention_never_opens_contents_but_other_outputs_still_do() {
    use std::os::windows::fs::OpenOptionsExt;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("native.bin");
    fs::write(&path, b"original").unwrap();
    let lock = fs::OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(&path)
        .unwrap();
    for source in [path.clone(), dir.path().join("./native.bin")] {
        let report = Staged::prepare(vec![Output::new(path.clone(), Source::File(source))])
            .unwrap()
            .publish()
            .unwrap();
        assert_eq!(report.retained, [path.clone()]);
    }
    // A distinct source and generated bytes must compare the actual destination contents.
    let other = dir.path().join("other.bin");
    fs::write(&other, b"original").unwrap();
    for source in [
        Source::File(other),
        Source::Bytes(Arc::from(&b"original"[..])),
    ] {
        assert!(Staged::prepare(vec![Output::new(path.clone(), source)]).is_err());
    }
    drop(lock);
    assert_eq!(fs::read(&path).unwrap(), b"original");
}
