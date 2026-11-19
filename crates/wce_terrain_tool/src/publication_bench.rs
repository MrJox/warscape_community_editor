//! Opt-in synthetic publication measurement; no game assets, window or user settings.
use super::*;
use wce_editor_core::{document::SaveTarget, document_manager::DocumentManager};
use wce_terrain::{LfRole, MapKind};
use wce_terrain_editor::MapSource;
use wce_terrain_io::MapFiles;

struct Scratch(PathBuf);
impl Drop for Scratch {
    fn drop(&mut self) {
        let path = self.0.canonicalize().unwrap();
        assert!(path.starts_with(std::env::temp_dir().canonicalize().unwrap()));
        assert!(path.file_name().unwrap().to_string_lossy().starts_with("wce-publication-bench-"));
        std::fs::remove_dir_all(path).unwrap();
    }
}

#[cfg(windows)]
fn io_bytes() -> (u64, u64) {
    #[repr(C)]
    #[derive(Default)]
    struct Counters {
        read_ops: u64,
        write_ops: u64,
        other_ops: u64,
        read_bytes: u64,
        write_bytes: u64,
        other_bytes: u64,
    }
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetCurrentProcess() -> *mut std::ffi::c_void;
        fn GetProcessIoCounters(process: *mut std::ffi::c_void, counters: *mut Counters) -> i32;
    }
    let mut counters = Counters::default();
    // SAFETY: the pseudo-handle identifies this process; the writable output has the documented IO_COUNTERS layout.
    assert_ne!(unsafe { GetProcessIoCounters(GetCurrentProcess(), &mut counters) }, 0);
    (counters.read_bytes, counters.write_bytes)
}
#[cfg(not(windows))]
fn io_bytes() -> (u64, u64) {
    let io = std::fs::read_to_string("/proc/self/io").unwrap_or_default();
    let value = |key| io.lines().find_map(|line| line.strip_prefix(key)).and_then(|v| v.trim().parse().ok()).unwrap_or(0);
    (value("rchar:"), value("wchar:"))
}
fn measure(label: &str, mut operation: impl FnMut()) {
    for run in 0..3 {
        let before = io_bytes();
        let start = Instant::now();
        operation();
        let elapsed = start.elapsed().as_secs_f64();
        let after = io_bytes();
        println!(
            "WCE_PUBLICATION_BENCH {label} run={run} seconds={elapsed:.6} logical_read_bytes={} logical_write_bytes={}",
            after.0 - before.0,
            after.1 - before.1
        );
    }
}
fn tiff() -> Vec<u8> {
    const SIDE: u32 = 4096;
    let mut bytes = b"II*\0".to_vec();
    bytes.extend(8u32.to_le_bytes());
    bytes.extend(6u16.to_le_bytes());
    for (tag, kind, value) in [(256u16, 3u16, SIDE), (257, 3, SIDE), (258, 3, 16), (259, 3, 1), (273, 4, 86), (277, 3, 1)] {
        bytes.extend(tag.to_le_bytes());
        bytes.extend(kind.to_le_bytes());
        bytes.extend(1u32.to_le_bytes());
        bytes.extend(value.to_le_bytes());
    }
    bytes.extend(0u32.to_le_bytes());
    for index in 0..SIDE * SIDE {
        bytes.extend((index as u16).wrapping_mul(13).to_le_bytes());
    }
    bytes.extend(b"preserved unknown TIFF tail");
    bytes
}

#[test]
#[ignore = "explicit 4096-square LF / 160 MiB synthetic publication measurement; run alone with --nocapture --test-threads=1"]
fn large_synthetic_save_export_measurement() {
    use wce_foundation::publication::{Output, Source, Staged};
    let root = Scratch(std::env::temp_dir().join(format!(
        "wce-publication-bench-{}-{}",
        std::process::id(),
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
    )));
    let source = root.0.join("source");
    let mut files = vec![("lf_heights.tif".into(), tiff()), ("carried/large.bin".into(), vec![73; 128 * 1024 * 1024])];
    files.extend((0..1024).map(|i| (format!("carried/small-{i:04}.bin"), vec![(i % 251) as u8; 1024])));
    Staged::prepare(files.iter().map(|(name, bytes)| Output::new(source.join(name), Source::Bytes(Arc::from(bytes.as_slice())))).collect())
        .unwrap()
        .publish()
        .unwrap();
    let model = wce_terrain_io::attila::read("benchmark", MapKind::Battle, &MapFiles::from_dir(&source).unwrap()).unwrap();
    let mut document =
        MapDocument::new("benchmark".into(), MapSource::Kit(source.clone()), Some(model), Arc::default(), None, Arc::default());
    document.save_dir = Some(source.clone());
    document.observe_save_destination().unwrap();
    let mut manager = DocumentManager::default();
    let id = manager.add(&mut Default::default(), Box::new(document), "terrain").unwrap();
    measure("owned_changed", || {
        let document = manager.find_as_mut::<MapDocument>(id).unwrap();
        let mut stroke = wce_terrain::lf_edit::LfStroke::default();
        let previous = document.lf_field(LfRole::Land).unwrap().at(4095, 4095);
        stroke.set(document.lf_field_mut(LfRole::Land).unwrap(), 4095, 4095, previous.wrapping_add(1));
        let record = stroke.finish(document.lf_field(LfRole::Land).unwrap()).unwrap();
        document.push_stroke(wce_terrain_editor::lf::LfStrokeCommand { role: LfRole::Land, record: Arc::new(record), mirror: None });
        manager.save_one(id).unwrap();
        assert!(!manager.find(id).unwrap().modified());
    });
    measure("owned_noop", || {
        manager.save_one(id).unwrap();
    });
    let copy = SaveTarget { path: root.0.join("copy") };
    manager.save_to(id, &copy).unwrap();
    measure("copy_noop", || {
        manager.save_to(id, &copy).unwrap();
    });
    assert!(std::fs::read(source.join("carried/large.bin")).unwrap() == files[1].1);
    assert!(std::fs::read(copy.path.join("lf_heights.tif")).unwrap() == std::fs::read(source.join("lf_heights.tif")).unwrap());
    drop(manager); // Release mapped LF before removing the scratch tree on Windows.

    let changes = std::sync::Mutex::new(wce_foundation::disk_changes::Changes::default());
    let normals = std::sync::Mutex::new(Default::default());
    let model = wce_terrain::MapModel::default();
    let out = root.0.join("export");
    measure("export_changed", || {
        files[1].1[128 * 1024 * 1024 - 1] ^= 1; // A late difference exercises exact comparisons over the large resource.
        let attempt = changes.lock().unwrap().begin_export_scoped(&out, &["terrain/battles/benchmark", "terrain_benchmark.pack"]).unwrap();
        write(&out, "benchmark", &files, &wce_vfs::GameProfile::ROME_2, &model, &normals, &changes, attempt).unwrap();
    });
    measure("export_noop", || {
        let attempt = changes.lock().unwrap().begin_export_scoped(&out, &["terrain/battles/benchmark", "terrain_benchmark.pack"]).unwrap();
        write(&out, "benchmark", &files, &wce_vfs::GameProfile::ROME_2, &model, &normals, &changes, attempt).unwrap();
    });
    assert!(std::fs::read(out.join("terrain/battles/benchmark/carried/large.bin")).unwrap() == files[1].1);
    // Newly generated pack headers may contain fresh metadata. Read every resource
    // back through RPFM and require exact payloads rather than comparing new containers.
    use rpfm_lib::files::pack::Pack;
    let games = rpfm_lib::games::supported_games::SupportedGames::default();
    let info = games.game(wce_vfs::GameProfile::ROME_2.key).unwrap();
    let mut pack = Pack::read_and_merge(&[out.join("terrain_benchmark.pack")], info, false, false, false).unwrap();
    assert_eq!(pack.files().len(), files.len());
    for (name, bytes) in pack_files("benchmark", &files) {
        let actual = pack.files_mut().get_mut(&name).unwrap().cached().unwrap();
        assert!(actual == bytes, "pack resource {name}");
    }
}
