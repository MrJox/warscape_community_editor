//! Direct Terrain publication contracts; fixtures are synthetic and contain no game assets.
use crate::{MapDocument, MapSource};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::Arc,
};
use wce_editor_core::{
    document::{Document, SaveTarget},
    document_manager::DocumentManager,
};
use wce_formats::chmf::{ChmfFile, ChmfRegion, encode_region};
use wce_terrain::{LfRole, MapModel, paged::PagedU16};
use wce_terrain_io::{MapFiles, built};
use wce_tile::battle_map::{context::PlacedMap, tile_db::TileDatabase, tile_list::TileList};
use wce_tile_model::Blob;
pub trait FixtureWrite {
    fn write_to(&self, dir: &std::path::Path) -> Result<(), String>;
}
impl FixtureWrite for wce_terrain_io::MapFiles {
    fn write_to(&self, dir: &std::path::Path) -> Result<(), String> {
        use wce_foundation::publication::{Output, Source, Staged};
        let outputs = self
            .folder
            .iter()
            .map(|(key, blob)| {
                let source = match blob {
                    wce_tile_model::Blob::File(p) => Source::File(p.clone()),
                    wce_tile_model::Blob::Bytes(b) => Source::Bytes(b.clone()),
                };
                Ok(Output::new(wce_foundation::publication::resource_path(dir, key)?, source))
            })
            .collect::<Result<Vec<_>, String>>()?;
        Staged::prepare(outputs).and_then(Staged::publish).map(|_| ()).map_err(|e| e.to_string())
    }
}
struct Scratch(PathBuf);
impl Scratch {
    fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let sequence = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let p = std::env::temp_dir().join(format!(
            "wce-pr5-publication-{}-{}-{sequence}",
            std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
        ));
        std::fs::create_dir(&p).unwrap();
        Self(p)
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let target = self.0.canonicalize().unwrap();
        let root = std::env::temp_dir().canonicalize().unwrap();
        assert!(target.starts_with(root) && target.file_name().unwrap().to_string_lossy().starts_with("wce-pr5-publication-"));
        for p in std::fs::read_dir(&target).unwrap().flatten().map(|p| p.path()) {
            if p.is_file() {
                let mut perm = std::fs::metadata(&p).unwrap().permissions();
                if perm.readonly() {
                    perm.set_readonly(false);
                    std::fs::set_permissions(p, perm).unwrap();
                }
            }
        }
        std::fs::remove_dir_all(target).unwrap();
    }
}
fn fixture() -> MapFiles {
    let mut c = ChmfFile { version: 5, width: 48, height: 48, cells_x: 1, cells_y: 8, min: 0.0, max: 65535.0, spacing: 1, regions: vec![] };
    for y in (0..48).step_by(16) {
        for x in (0..48).step_by(16) {
            let (_, offsets, data) = encode_region(16, 16, &[10000; 256]);
            c.regions.push(ChmfRegion { rect: [x, y, x + 16, y + 16], tag: [10000, 65535], offsets, data });
        }
    }
    MapFiles::from_bytes([
        ("lf_height_map.data".into(), c.write()),
        ("lf_height_map.dds".into(), wce_terrain_io::attila::heights::lf_height_dds(&PagedU16::filled(192, 192, 10000))),
        ("lf_height_map_lq.dds".into(), wce_terrain_io::attila::heights::lf_height_dds(&PagedU16::filled(96, 96, 10000))),
        ("terrain.tile_list".into(), TileList { version: 9, is_campaign: false, battle_size: [48, 48], ..Default::default() }.write()),
        ("normals/normal_0.dds".into(), b"opaque unsupported normals stay original".to_vec()),
    ])
}

fn doc(m: MapModel) -> MapDocument {
    let db = Arc::new(TileDatabase::default());
    let list = m.tile_list.as_ref().map(|i| i.get().clone()).unwrap_or_default();
    let placed = PlacedMap::from_tile_list("test", &list, &db, None);
    MapDocument::new("test".into(), MapSource::Kit(PathBuf::new()), Some(m), Arc::new(placed), None, db)
}
fn changed(d: &mut MapDocument) {
    let mut stroke = wce_terrain::lf_edit::LfStroke::default();
    stroke.set(d.lf_field_mut(LfRole::Land).unwrap(), 64, 64, 10200);
    let record = stroke.finish(d.lf_field(LfRole::Land).unwrap()).unwrap();
    d.push_stroke(crate::lf::LfStrokeCommand { role: LfRole::Land, record: Arc::new(record), mirror: None });
}
fn state(d: &MapDocument) -> (bool, usize, usize, u64, Option<PathBuf>, Vec<u8>) {
    (
        d.modified(),
        d.history().size(),
        d.history().position(),
        d.history().version(),
        d.save_dir.clone(),
        d.lf_field(LfRole::Land).unwrap().to_le_rows(),
    )
}
fn disk(p: &Path) -> BTreeMap<String, Vec<u8>> {
    MapFiles::from_dir(p).unwrap().folder.into_iter().map(|(k, b)| (k, b.bytes().unwrap().to_vec())).collect()
}
#[test]
fn staging_failure_preserves_all_existing_outputs_history_and_destination() {
    let root = Scratch::new();
    let out = root.0.join("out");
    fixture().write_to(&out).unwrap();
    let missing = root.0.join("missing.bin");
    let mut model = built::read("test", &fixture()).unwrap();
    model.carried.insert("z-last.bin".into(), Blob::File(missing.clone()));
    let mut d = doc(model);
    d.save_dir = Some(root.0.join("original"));
    changed(&mut d);
    let before = state(&d);
    let original = disk(&out);
    assert!(d.save(&SaveTarget { path: out.clone() }).is_err());
    assert_eq!(state(&d), before);
    assert_eq!(disk(&out), original);
    std::fs::write(missing, b"recovered source").unwrap();
    d.save_overwrite(&SaveTarget { path: out.clone() }).unwrap();
    assert_eq!(state(&d), before);
    assert_eq!(std::fs::read(out.join("z-last.bin")).unwrap(), b"recovered source");
    d.save(&SaveTarget::default()).unwrap();
    assert_eq!(d.save_dir, Some(root.0.join("original")));
    assert!(!d.modified());
}
#[test]
fn shared_export_lease_refuses_save_without_state_changes_and_recovers() {
    let root = Scratch::new();
    let mut d = doc(built::read("test", &fixture()).unwrap());
    d.save_dir = Some(root.0.clone());
    changed(&mut d);
    let before = state(&d);
    let lease = d.begin_io().unwrap();
    assert!(d.begin_io().is_err());
    assert!(d.save(&SaveTarget::default()).unwrap_err().contains("Busy"));
    assert_eq!(state(&d), before);
    assert!(std::fs::read_dir(&root.0).unwrap().next().is_none());
    drop(lease);
    d.save(&SaveTarget::default()).unwrap();
    assert!(!d.modified());
}
#[test]
fn duplicate_destination_keeps_original_document_history_and_one_document_rule() {
    let root = Scratch::new();
    let mut scene = wce_scene::render_scene::RenderScene::default();
    let mut docs = DocumentManager::default();
    let model = built::read("test", &fixture()).unwrap();
    let mut first = doc(model.clone());
    first.save_dir = Some(root.0.clone());
    changed(&mut first);
    let before = state(&first);
    let id = docs.add(&mut scene, Box::new(first), "terrain").unwrap();
    let mut duplicate = doc(model.clone());
    duplicate.save_dir = Some(root.0.join("."));
    assert_eq!(docs.add(&mut scene, Box::new(duplicate), "terrain").unwrap(), id);
    assert_eq!(docs.order().len(), 1);
    assert_eq!(state(docs.find_as::<MapDocument>(id).unwrap()), before);
    let mut different = doc(model);
    different.save_dir = Some(root.0.join("different"));
    assert!(docs.add(&mut scene, Box::new(different), "terrain").is_err());
    assert_eq!(docs.order().len(), 1);
    assert_eq!(state(docs.find_as::<MapDocument>(id).unwrap()), before);
}
#[test]
fn save_as_refuses_owned_destination_then_copies_without_retargeting_or_saving_history() {
    let root = Scratch::new();
    let mut scene = wce_scene::render_scene::RenderScene::default();
    let mut docs = DocumentManager::default();
    let model = built::read("test", &fixture()).unwrap();
    let owned = root.0.join("owned");
    let mut first = doc(model.clone());
    first.save_dir = Some(owned.clone());
    docs.add(&mut scene, Box::new(first), "first").unwrap();
    let old = root.0.join("old");
    let mut second = doc(model);
    second.save_dir = Some(old.clone());
    changed(&mut second);
    let before = state(&second);
    let id = docs.add(&mut scene, Box::new(second), "second").unwrap();
    assert!(docs.save_to(id, &SaveTarget { path: owned.clone() }).is_err());
    assert_eq!(state(docs.find_as::<MapDocument>(id).unwrap()), before);
    assert!(!owned.exists());
    let target = root.0.join("saved-as");
    docs.save_to(id, &SaveTarget { path: target.clone() }).unwrap();
    assert_eq!(state(docs.find_as::<MapDocument>(id).unwrap()), before);
    assert!(!old.exists());
    let original = disk(&target);
    docs.save_one(id).unwrap();
    assert_eq!(disk(&target), original);
    assert_eq!(disk(&old), original);
    assert_eq!(docs.find(id).unwrap().save_destination(), Some(old.clone()));
    assert!(!docs.find(id).unwrap().modified());
}

#[test]
fn restored_document_stays_unsaved_after_copy_or_failure_until_owned_save() {
    for edit in [false, true] {
        let root = Scratch::new();
        let mut d = doc(built::read("test", &fixture()).unwrap());
        let owned = root.0.join("owned");
        let restored = root.0.join("recovery-copy");
        d.save_dir = Some(owned.clone());
        d.restored_from = Some(restored.clone());
        if edit {
            changed(&mut d);
        }
        let before = state(&d);
        assert!(d.modified());
        assert_eq!(d.autosave_meta().unwrap().target, owned);
        assert!(d.revert().is_err());
        assert_eq!(state(&d), before);

        let copy = root.0.join("save-as");
        d.save(&SaveTarget { path: copy.clone() }).unwrap();
        assert_eq!(state(&d), before);
        assert_eq!(d.restored_from.as_ref(), Some(&restored));
        assert!(!owned.exists());
        d.publication_failure_after = Some(0);
        assert!(d.save(&SaveTarget::default()).is_err());
        assert_eq!(state(&d), before);
        assert_eq!(d.restored_from.as_ref(), Some(&restored));

        d.save(&SaveTarget::default()).unwrap();
        assert_eq!(d.restored_from, None);
        assert!(!d.modified());
        assert_eq!(d.save_dir.as_ref(), Some(&owned));
        assert_eq!(disk(&owned), disk(&copy));
    }
}

#[test]
fn autosave_round_trip_retains_owned_target_until_successful_project_save() {
    let root = Scratch::new();
    let owned = root.0.join("owned");
    fixture().write_to(&owned).unwrap();
    let mut d = doc(built::read("test", &MapFiles::from_dir(&owned).unwrap()).unwrap());
    d.save_dir = Some(owned.clone());
    changed(&mut d);
    let before = state(&d);
    let original = disk(&owned);
    let recovery = root.0.join("recovery");
    d.autosave(&recovery).unwrap();
    assert_eq!(state(&d), before);
    assert_eq!(disk(&owned), original);

    let recovered_map = MapDocument::complete_autosave(&recovery).unwrap();
    let mut restored = doc(built::read("test", &MapFiles::from_dir(&recovered_map).unwrap()).unwrap());
    restored.save_dir = Some(owned.clone());
    restored.restored_from = Some(recovery.clone());
    restored.observe_save_destination().unwrap();
    assert_eq!(restored.lf_field(LfRole::Land).unwrap().to_le_rows(), before.5);
    let restored_before = state(&restored);
    restored.save(&SaveTarget { path: root.0.join("copy") }).unwrap();
    assert_eq!(state(&restored), restored_before);
    assert_eq!(restored.restored_from.as_ref(), Some(&recovery));
    assert_eq!(disk(&owned), original);

    restored.save(&SaveTarget::default()).unwrap();
    assert_eq!(restored.restored_from, None);
    assert!(!restored.modified());
    let reopened = built::read("test", &MapFiles::from_dir(&owned).unwrap()).unwrap();
    assert_eq!(reopened.lf(LfRole::Land).unwrap().get().to_le_rows(), before.5);
}
#[test]
fn equal_size_file_is_compared_by_content_and_identical_readonly_output_is_retained() {
    let root = Scratch::new();
    let source = root.0.join("source.bin");
    std::fs::write(&source, b"right").unwrap();
    let mut model = built::read("test", &fixture()).unwrap();
    model.carried.insert("opaque.bin".into(), Blob::File(source));
    let mut d = doc(model);
    let out = root.0.join("out");
    std::fs::create_dir(&out).unwrap();
    std::fs::write(out.join("opaque.bin"), b"wrong").unwrap();
    d.save_dir = Some(out.clone());
    d.save_overwrite(&SaveTarget { path: out.clone() }).unwrap();
    assert_eq!(std::fs::read(out.join("opaque.bin")).unwrap(), b"right");
    let path = out.join("opaque.bin");
    let original = std::fs::metadata(&path).unwrap().permissions();
    let mut readonly = original.clone();
    readonly.set_readonly(true);
    std::fs::set_permissions(&path, readonly).unwrap();
    let result = d.save(&SaveTarget::default());
    std::fs::set_permissions(&path, original).unwrap();
    result.unwrap();
}
#[test]
fn reserved_temporaries_are_excluded_on_read_and_refused_before_save() {
    let root = Scratch::new();
    fixture().write_to(&root.0).unwrap();
    std::fs::write(root.0.join("stray.WCE_TMP"), b"leftover").unwrap();
    std::fs::create_dir(root.0.join("hidden.wce_tmp")).unwrap();
    std::fs::write(root.0.join("hidden.wce_tmp/resource"), b"leftover").unwrap();
    let input = MapFiles::from_dir(&root.0).unwrap();
    assert!(!input.folder.keys().any(|k| k.to_ascii_lowercase().contains("wce_tmp")));
    let mut model = built::read("test", &input).unwrap();
    model.carried.insert("bad.wce_tmp".into(), Blob::Bytes(Arc::from(&b"bad"[..])));
    let mut d = doc(model);
    changed(&mut d);
    let before = state(&d);
    let out = root.0.join("out");
    assert!(d.save(&SaveTarget { path: out.clone() }).is_err());
    assert_eq!(state(&d), before);
    assert!(!out.exists());
}
#[test]
fn partial_publication_keeps_lf_baseline_and_ordinary_retry_regenerates_normals() {
    for via_copy in [false, true] {
        let dir = Scratch::new();
        let mut f = fixture();
        f.folder.remove("lf_height_map_lq.dds");
        let mut t = TileList::parse(&f.folder["terrain.tile_list"].bytes().unwrap()).unwrap();
        t.is_campaign = false;
        f.folder.insert("terrain.tile_list".into(), Blob::Bytes(Arc::from(t.write())));
        let pixels = [0u8, 180, 255, 130].repeat(256 * 256);
        f.folder.insert("normals/normal_0.dds".into(), Blob::Bytes(Arc::from(wce_tile_io_for_test_dds(&pixels))));
        f.write_to(&dir.0).unwrap();
        let source = MapFiles::from_dir(&dir.0).unwrap();
        let m = built::read("battle", &source).unwrap();
        let mut d = doc(m);
        d.save_dir = Some(dir.0.clone());
        d.history_mut().mark_saved();
        let mut stroke = wce_terrain::lf_edit::LfStroke::default();
        stroke.set(d.lf_field_mut(LfRole::Land).unwrap(), 64, 64, 10200);
        let record = stroke.finish(d.model().unwrap().lf(LfRole::Land).unwrap().get()).unwrap();
        d.push_stroke(crate::lf::LfStrokeCommand { role: LfRole::Land, record: Arc::new(record), mirror: None });
        let expected = built::write(d.model().unwrap(), None).unwrap();
        let normal = "normals/normal_0.dds";
        assert_ne!(expected.folder[normal].bytes().unwrap(), f.folder[normal].bytes().unwrap());
        d.publication_failure_after = Some(1);
        let error = d.save(&SaveTarget { path: dir.0.clone() }).unwrap_err();
        assert!(error.contains("1 file(s) published"));
        assert!(Document::modified(&d));
        for n in ["lf_height_map.dds", "lf_height_map.data"] {
            assert_eq!(std::fs::read(dir.0.join(n)).unwrap(), f.folder[n].bytes().unwrap().as_ref());
        }
        if via_copy {
            let copy = Scratch::new();
            d.save(&SaveTarget { path: copy.0.clone() }).unwrap();
            assert!(d.modified());
            assert_eq!(d.save_dir, Some(dir.0.clone()));
            for n in [normal, "lf_height_map.dds", "lf_height_map.data"] {
                assert!(
                    std::fs::read(copy.0.join(n)).unwrap().as_slice() == expected.folder[n].bytes().unwrap().as_ref(),
                    "independent copy: {n}"
                );
            }
        }
        d.save(&SaveTarget { path: dir.0.clone() }).unwrap();
        assert!(!Document::modified(&d));
        for n in [normal, "lf_height_map.dds", "lf_height_map.data"] {
            assert!(
                std::fs::read(dir.0.join(n)).unwrap().as_slice() == expected.folder[n].bytes().unwrap().as_ref(),
                "{n} differs from a clean one-pass Save"
            );
        }
    }
}
fn wce_tile_io_for_test_dds(pixels: &[u8]) -> Vec<u8> {
    let mut b = vec![0u8; 128];
    b[..4].copy_from_slice(b"DDS ");
    for (at, value) in [
        (4, 124u32),
        (8, 0x100f),
        (12, 256),
        (16, 256),
        (20, 1024),
        (76, 32),
        (80, 0x41),
        (88, 32),
        (92, 0x00ff0000),
        (96, 0x0000ff00),
        (100, 0x000000ff),
        (104, 0xff000000),
        (108, 0x1000),
    ] {
        b[at..at + 4].copy_from_slice(&value.to_le_bytes());
    }
    b.extend_from_slice(pixels);
    b
}

mod freshness {
    use super::*;
    include!("freshness_tests.rs");
}
