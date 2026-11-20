use std::path::PathBuf;
use std::sync::Arc;

use wce_editor_core::document::{Document, SaveTarget};
use wce_formats::lf_region_png::{decode, encode};
use wce_scene::render_scene::LfSamples;
use wce_terrain::lf_region::LfRegion;
use wce_terrain::{LfRole, MapKind};
use wce_terrain_editor::lf::LfMirror;
use wce_terrain_editor::{MapDocument, MapSource};
use wce_terrain_io::{MapFiles, attila};

const W: u32 = 130;
const H: u32 = 67;

fn tiff(w: u32, h: u32, values: &[u16]) -> Vec<u8> {
    let mut b = b"II*\0".to_vec();
    b.extend(8u32.to_le_bytes());
    let entries: [(u16, u16, u32, u32); 6] =
        [(256, 3, 1, w), (257, 3, 1, h), (258, 3, 1, 16), (259, 3, 1, 1), (273, 4, 1, 86), (277, 3, 1, 1)];
    b.extend(6u16.to_le_bytes());
    for (tag, kind, count, value) in entries {
        b.extend(tag.to_le_bytes());
        b.extend(kind.to_le_bytes());
        b.extend(count.to_le_bytes());
        b.extend(value.to_le_bytes());
    }
    b.extend(0u32.to_le_bytes());
    b.extend(values.iter().flat_map(|s| s.to_le_bytes()));
    b.extend(b"unknown tail retained");
    b
}

fn fixture() -> MapFiles {
    MapFiles::from_bytes([
        ("lf_heights.tif".into(), tiff(W, H, &(0..W * H).map(|i| (i as u16).wrapping_mul(13)).collect::<Vec<_>>())),
        ("lf_sea_heights.tif".into(), tiff(2, 2, &[1, 2, 3, 4])),
        ("config.xml".into(), b"<ROOT unknown=\"kept\"/>\r\n".to_vec()),
        ("m.terry".into(), b"<project version=\"3\"><scene version=\"10\"/></project>".to_vec()),
        ("normal.dds".into(), b"opaque normal".to_vec()),
        ("deployment/unknown.bin".into(), b"opaque resource".to_vec()),
    ])
}

fn document(kind: MapKind, native: bool) -> MapDocument {
    let mut m = attila::read("m", kind, &fixture()).unwrap();
    m.built = native;
    let placed =
        wce_tile::battle_map::context::PlacedMap { width: 2, height: 2, lf_height: 712., vertical_offset: 38.28, ..Default::default() };
    let mut d = MapDocument::new("m".into(), MapSource::Kit("unused".into()), Some(m), Arc::new(placed), None, Arc::default());
    d.history_mut().mark_saved();
    d
}

fn region() -> LfRegion {
    LfRegion { x: 62, y: 62, width: 68, height: 5 }
}
fn pixels() -> Vec<u16> {
    (0..340).map(|i| (i as u16).wrapping_mul(7919)).collect()
}
fn field(d: &MapDocument) -> Vec<u16> {
    d.lf_field(LfRole::Land).unwrap().to_vec()
}
fn state(d: &MapDocument) -> (usize, usize, u64, bool, bool, u64) {
    (
        d.history().size(),
        d.history().position(),
        d.history().version(),
        d.history().modified(),
        d.model().unwrap().lf(LfRole::Land).unwrap().edited(),
        d.terry_revision.load(std::sync::atomic::Ordering::Relaxed),
    )
}
struct Scratch(PathBuf);
impl std::ops::Deref for Scratch {
    type Target = std::path::Path;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let root = self.0.canonicalize().unwrap();
        assert!(
            root.starts_with(std::env::temp_dir().canonicalize().unwrap())
                && root.file_name().unwrap().to_string_lossy().starts_with("wce_lf_region_")
        );
        std::fs::remove_dir_all(root).unwrap();
    }
}
fn scratch() -> Scratch {
    let path = std::env::temp_dir().join(format!(
        "wce_lf_region_{}_{}",
        std::process::id(),
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
    ));
    std::fs::create_dir(&path).unwrap();
    Scratch(path)
}

#[test]
fn page_crossing_import_is_one_undo_step_and_mirror_refresh() {
    let mut d = document(MapKind::Campaign, false);
    let before = field(&d);
    let placed = d.placed.clone();
    let samples = LfSamples::new(before.chunks(W as usize).rev().flatten().copied().collect());
    let mirror = Arc::new(LfMirror::of(d.lf_field(LfRole::Land).unwrap(), samples.clone(), W as i32, H as i32).unwrap());
    let png = encode(68, 5, &pixels()).unwrap();
    assert!(d.import_lf_region_png(region(), &png, Some(mirror.clone())).unwrap());
    assert_eq!((d.history().size(), d.history().position()), (1, 1));
    assert_eq!(d.history().name(0), "Import LF region PNG");
    assert!(d.history().modified());
    assert!(Arc::ptr_eq(&d.placed, &placed));
    assert_eq!((d.placed.lf_height, d.placed.vertical_offset), (712., 38.28));
    assert_eq!(mirror.take_touched(), Some([62, 62, 130, 67]));
    assert_eq!(samples.take_dirty(), Some([62, 0, 130, 5]));
    assert_eq!(samples.revision(), 1);
    let after = field(&d);
    for y in 0..H {
        for x in 0..W {
            let i = (y * W + x) as usize;
            let want = if x >= 62 && y >= 62 { pixels()[((y - 62) * 68 + x - 62) as usize] } else { before[i] };
            assert_eq!(after[i], want, "sample {x}, {y}");
            assert_eq!(samples.read()[((H - 1 - y) * W + x) as usize], want);
        }
    }
    let written = attila::write(d.model().unwrap()).unwrap();
    let source = fixture();
    for (name, blob) in &source.folder {
        let actual = written.folder[name].bytes().unwrap();
        let original = blob.bytes().unwrap();
        if name == "lf_heights.tif" {
            for (i, (&a, &b)) in actual.iter().zip(original.iter()).enumerate() {
                if a != b {
                    assert!((86..86 + W as usize * H as usize * 2).contains(&i));
                    let sample = (i - 86) / 2;
                    assert!(sample % W as usize >= 62 && sample / W as usize >= 62);
                }
            }
        } else {
            assert_eq!(actual, original, "{name}");
        }
    }
    assert!(d.history_mut().undo());
    assert_eq!(field(&d), before);
    assert!(!d.history().modified());
    assert!(!d.history().can_undo());
    assert!(d.history_mut().redo());
    assert_eq!(field(&d), after);
    assert_eq!(samples.revision(), 3);
    assert_eq!(state(&d).5, 0, "no per-pixel Terry scene invalidation");
    // A shared view uses the field directly and still receives one dirty rectangle per command.
    let shared = LfSamples::shared(d.lf_field(LfRole::Land).unwrap().grid().clone(), true);
    let shared_mirror = Arc::new(LfMirror::of(d.lf_field(LfRole::Land).unwrap(), shared.clone(), W as i32, H as i32).unwrap());
    let next = encode(68, 5, &vec![65535; 340]).unwrap();
    assert!(d.import_lf_region_png(region(), &next, Some(shared_mirror)).unwrap());
    assert_eq!(shared.revision(), 1);
    assert_eq!(shared.read()[62], 65535);
}

#[test]
fn export_noop_errors_and_cancellation_preserve_document_and_redo() {
    let mut d = document(MapKind::Battle, false);
    let before = field(&d);
    let initial = state(&d);
    let png = d.export_lf_region_png(region()).unwrap();
    assert_eq!(decode(&png).unwrap().samples, region().read(d.lf_field(LfRole::Land).unwrap()).unwrap());
    assert_eq!(state(&d), initial);
    assert!(!d.import_lf_region_png(region(), &png, None).unwrap());
    assert_eq!(state(&d), initial);
    assert!(d.import_lf_region_png(region(), &encode(68, 5, &pixels()).unwrap(), None).unwrap());
    d.history_mut().undo();
    let stable = state(&d);
    for bytes in [b"not png".to_vec(), encode(1, 1, &[10]).unwrap()] {
        assert!(d.import_lf_region_png(region(), &bytes, None).is_err());
        assert_eq!(state(&d), stable);
    }
    for r in [LfRegion { x: u32::MAX, ..region() }, LfRegion { width: 0, ..region() }, LfRegion { width: 1025, height: 1024, ..region() }] {
        assert!(d.import_lf_region_png(r, &png, None).is_err());
        assert!(d.export_lf_region_png(r).is_err());
        assert_eq!(state(&d), stable);
    }
    let invalid_mirror = Arc::new(LfMirror::of(&wce_terrain::paged::PagedU16::filled(1, 1, 0), LfSamples::new(vec![0]), 1, 1).unwrap());
    assert!(d.import_lf_region_png(region(), &png, Some(invalid_mirror)).is_err());
    assert!(d.import_lf_region_file(region(), Some(&scratch().join("missing.png")), None).is_err());
    assert!(!d.import_lf_region_file(region(), None, None).unwrap());
    assert!(!d.export_lf_region_file(region(), None).unwrap());
    assert!(!d.import_lf_region_png(region(), &png, None).unwrap());
    let dir = scratch();
    let path = dir.join("region.png");
    assert!(d.export_lf_region_file(region(), Some(&path)).unwrap());
    assert!(d.export_lf_region_file(region(), Some(&path)).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), png);
    assert!(d.export_lf_region_file(region(), Some(&dir.join("map.tif"))).is_err());
    assert!(d.export_lf_region_file(region(), Some(&dir.join("missing/region.png"))).is_err());
    assert_eq!(state(&d), stable);
    assert_eq!(field(&d), before);
    assert!(d.history().can_redo());
}

#[test]
fn native_battle_and_campaign_export_readonly_and_import_refuses_before_file_access() {
    for kind in [MapKind::Battle, MapKind::Campaign] {
        let mut d = document(kind, true);
        let before = field(&d);
        let stable = state(&d);
        assert_eq!(decode(&d.export_lf_region_png(region()).unwrap()).unwrap().samples.len(), 340);
        let err = d.import_lf_region_file(region(), Some(&PathBuf::from("does-not-exist.png")), None).unwrap_err();
        assert!(err.starts_with("Native LF region import"));
        assert!(d.import_lf_region_png(region(), b"invalid png", None).unwrap_err().starts_with("Native LF region import"));
        assert_eq!(state(&d), stable);
        assert_eq!(field(&d), before);
    }
}

#[test]
fn pack_only_maps_export_shared_file_rows_without_creating_an_editable_model() {
    use wce_tile::battle_map::context::{LfField, PlacedMap};
    for name in ["pack_battle", "campaigns/pack_campaign"] {
        let lf = LfField::new(4, 3, (0..12).collect());
        let grid = lf.values.clone();
        let placed = Arc::new(PlacedMap { width: 4, height: 3, lf: Some(lf), ..Default::default() });
        let mut d = MapDocument::new(name.into(), MapSource::Packs(name.into()), None, placed.clone(), None, Arc::default());
        assert!(Arc::ptr_eq(d.land_region_field().unwrap().grid(), &grid));
        let region = LfRegion { x: 1, y: 1, width: 2, height: 2 };
        assert_eq!(decode(&d.export_lf_region_png(region).unwrap()).unwrap().samples, [5, 6, 9, 10]);
        assert!(!d.lf_region_import_supported());
        let error = d.import_lf_region_file(region, Some(std::path::Path::new("missing-input.png")), None).unwrap_err();
        assert!(!error.contains("missing-input"), "native eligibility must be checked before file access");
        assert!(d.model().is_none());
        assert_eq!(d.history().size(), 0);
        assert!(!d.modified());
        assert!(Arc::ptr_eq(&d.placed, &placed));
        assert_eq!(&*grid.read(), &(0..12).collect::<Vec<u16>>());
    }
}

#[test]
fn extensionless_exports_append_png_and_still_refuse_existing_files() {
    let d = document(MapKind::Battle, false);
    let stable = state(&d);
    let dir = scratch();
    let path = dir.join("region1");
    assert!(d.export_lf_region_file(region(), Some(&path)).unwrap());
    let png = dir.join("region1.png");
    let saved = std::fs::read(&png).unwrap();
    assert_eq!(decode(&saved).unwrap().samples.len(), 340);
    assert!(!path.exists());
    assert!(d.export_lf_region_file(region(), Some(&path)).is_err());
    assert_eq!(std::fs::read(&png).unwrap(), saved);
    assert!(d.export_lf_region_file(region(), Some(&dir.join("region.txt"))).is_err());
    assert_eq!(state(&d), stable);
}

#[test]
fn raw_battle_and_campaign_save_reopen_preserve_all_other_resources() {
    for kind in [MapKind::Battle, MapKind::Campaign] {
        let mut d = document(kind, false);
        let dir = scratch();
        let target = SaveTarget { path: dir.join("saved") };
        // Exercise owned project Save; an arbitrary target is a copy and must not clean history.
        d.save_dir = Some(target.path.clone());
        d.save(&target).unwrap();
        assert!(!d.history().modified());
        let png = encode(68, 5, &pixels()).unwrap();
        assert!(d.import_lf_region_png(region(), &png, None).unwrap());
        d.save(&target).unwrap();
        d.save(&target).unwrap();
        assert!(!d.history().modified());
        let saved = MapFiles::from_dir(&target.path).unwrap();
        let reopened = attila::read("m", kind, &saved).unwrap();
        assert_eq!(region().read(reopened.lf(LfRole::Land).unwrap().get()).unwrap(), pixels());
        assert_eq!(saved.folder.keys().collect::<Vec<_>>(), fixture().folder.keys().collect::<Vec<_>>());
        for (name, blob) in &fixture().folder {
            if name != "lf_heights.tif" {
                assert_eq!(saved.folder[name].bytes().unwrap(), blob.bytes().unwrap());
            }
        }
        d.history_mut().undo();
        d.save(&target).unwrap();
        assert_eq!(
            std::fs::read(target.path.join("lf_heights.tif")).unwrap(),
            fixture().folder["lf_heights.tif"].bytes().unwrap().as_ref()
        );
    }
}

#[test]
#[ignore = "requires readonly WCE_ATTILA_MAP and WCE_ROME2_MAP witness folders"]
fn real_witnesses_export_small_regions_without_mutation() {
    use std::io::{Read, Seek, SeekFrom};
    use wce_terrain::paged::PagedU16;
    use wce_terrain::{Item, MapModel};
    use wce_tile_model::Blob;
    for native in [false, true] {
        let folder = PathBuf::from(std::env::var_os(if native { "WCE_ROME2_MAP" } else { "WCE_ATTILA_MAP" }).unwrap());
        let path = folder.join(if native { "lf_height_map.dds" } else { "lf_heights.tif" });
        let before = std::fs::metadata(&path).unwrap();
        let model = if native {
            let mut file = std::fs::File::open(&path).unwrap();
            let mut header = [0; 128];
            file.read_exact(&mut header).unwrap();
            assert_eq!(&header[..4], b"DDS ");
            let w = u32::from_le_bytes(header[16..20].try_into().unwrap());
            let h = u32::from_le_bytes(header[12..16].try_into().unwrap());
            let field = PagedU16::from_grid(Arc::new(wce_foundation::samples::SampleGrid::map_file(&path, 128, w, h).unwrap()));
            MapModel {
                built: true,
                kind: MapKind::Campaign,
                lf: vec![(LfRole::Land, Item::new("lf_height_map.dds", field, Blob::File(path.clone())))],
                ..Default::default()
            }
        } else {
            let files =
                MapFiles { folder: [("lf_heights.tif".into(), Blob::File(path.clone()))].into_iter().collect(), ..Default::default() };
            attila::read("witness", MapKind::Campaign, &files).unwrap()
        };
        let region = LfRegion { x: 62, y: 62, width: 5, height: 5 };
        let mut doc = MapDocument::new("witness".into(), MapSource::Kit(folder), Some(model), Arc::default(), None, Arc::default());
        doc.history_mut().mark_saved();
        let stable = state(&doc);
        let field = doc.lf_field(LfRole::Land).unwrap();
        assert!(field.grid().is_mapped(), "do not copy the real field");
        let values = region.read(field).unwrap();
        // Independent small file-row reads verify orientation and exact PNG endianness.
        let offset = if native {
            128
        } else {
            let mut file = std::fs::File::open(&path).unwrap();
            let mut header = [0; 8];
            file.read_exact(&mut header).unwrap();
            file.seek(SeekFrom::Start(u32::from_le_bytes(header[4..8].try_into().unwrap()) as u64)).unwrap();
            let mut count = [0; 2];
            file.read_exact(&mut count).unwrap();
            let mut strip = None;
            for _ in 0..u16::from_le_bytes(count) {
                let mut entry = [0; 12];
                file.read_exact(&mut entry).unwrap();
                if u16::from_le_bytes(entry[..2].try_into().unwrap()) == 273 {
                    strip = Some(u32::from_le_bytes(entry[8..12].try_into().unwrap()) as u64);
                }
            }
            strip.unwrap()
        };
        let mut file = std::fs::File::open(&path).unwrap();
        let mut expected = Vec::new();
        for y in region.y..region.y + region.height {
            file.seek(SeekFrom::Start(offset + (u64::from(y) * u64::from(field.width()) + u64::from(region.x)) * 2)).unwrap();
            let mut row = [0; 10];
            file.read_exact(&mut row).unwrap();
            expected.extend(row.chunks_exact(2).map(|s| u16::from_le_bytes([s[0], s[1]])));
        }
        assert_eq!(values, expected);
        let png = doc.export_lf_region_png(region).unwrap();
        assert_eq!(decode(&png).unwrap().samples, expected);
        if native {
            assert!(doc.import_lf_region_png(region, &png, None).unwrap_err().starts_with("Native LF region import"));
        }
        assert_eq!(state(&doc), stable);
        let after = std::fs::metadata(&path).unwrap();
        assert_eq!((before.len(), before.modified().unwrap()), (after.len(), after.modified().unwrap()));
        println!("{}: exact 5 x 5 readonly region at (62, 62): {:?}", path.display(), expected);
    }
}
