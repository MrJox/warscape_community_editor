//! Accepted P0-04 immutable BLM/deployment and truthful project Save contracts.
#[path = "common/map_fixture.rs"]
mod map_fixture;
use map_fixture::FixtureWrite;
#[test]
#[ignore = "private native witness"]
fn step_b_native_gaul_project_save_reopen_repeat() {
    let source = witness().join("rome2/terrain_battles_main_gaul/noop");
    let original = std::fs::read(source.join("locations.blm")).unwrap();
    let input = MapFiles::from_dir(&source).unwrap();
    let mut d = doc(model(&input));
    let noop = built::write_project(d.model().unwrap(), None).unwrap();
    assert_eq!(materialize(&noop), materialize(&input));
    widen(&mut d);
    let expected = built::write(d.model().unwrap(), None).unwrap();
    let changed = bytes(&expected, "locations.blm");
    assert_eq!(
        original.iter().zip(&changed).enumerate().filter_map(|(i, (a, b))| (a != b).then_some(i)).collect::<Vec<_>>(),
        vec![16548, 16556]
    );
    let out = scratch("step-b-native-gaul");
    save(&mut d, &out).unwrap();
    assert!(!d.modified());
    assert_eq!(tree(&out), materialize(&expected));
    save(&mut d, &out).unwrap();
    assert!(d.history_mut().undo());
    assert!(d.modified());
    assert!(d.history_mut().redo());
    assert!(!d.modified());
    save(&mut d, &out).unwrap();
    assert_eq!(tree(&out), materialize(&expected));
    let reopened = model(&MapFiles::from_dir(&out).unwrap());
    assert_eq!(materialize(&built::write_project(&reopened, None).unwrap()), materialize(&expected));
    assert_eq!(std::fs::read(source.join("locations.blm")).unwrap(), original);
}
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::Arc,
};
use wce_editor_core::document::{Document, SaveTarget};
use wce_terrain::{LfRole, MapKind, MapModel};
use wce_terrain_editor::{MapDocument, MapSource, terry::TerryAttrCommand};
use wce_terrain_io::{MapFiles, attila, built};
use wce_tile::battle_map::{
    context::PlacedMap,
    locations::{BattleLocations, Location},
    tile_db::TileDatabase,
    tile_list::TileList,
};
use wce_tile_model::Blob;

fn scratch(id: &str) -> PathBuf {
    let p = PathBuf::from(std::env::var_os("P004_SCRATCH").expect("set P004_SCRATCH to isolated output"))
        .join(format!("{}-{id}", std::process::id()));
    std::fs::create_dir_all(&p).unwrap();
    p
}
fn record(extra: [i32; 2]) -> Location {
    Location { rect: [3, 4, 5, 6], centre: [4, 5], id: -1, extra }
}
fn fixture(extra: [i32; 2]) -> BattleLocations {
    BattleLocations {
        version: 7,
        width: 16,
        height: 16,
        types: vec![("land_normal".into(), vec![record(extra)])],
        tail: b"opaque diagnostic tail\0\xff".to_vec(),
    }
}
fn blob(b: Vec<u8>) -> Blob {
    Blob::Bytes(Arc::from(b))
}
fn files(b: BattleLocations) -> MapFiles {
    MapFiles::from_bytes([
        ("locations.blm".into(), b.write().expect("P004 checked native serialization")),
        ("locations.blm.mask1".into(), vec![0xa5, 0x5a]),
        ("opaque.bin".into(), vec![1, 255, 0, 7]),
    ])
}
fn bytes(f: &MapFiles, k: &str) -> Vec<u8> {
    f.folder[k].bytes().unwrap().to_vec()
}
fn materialize(f: &MapFiles) -> BTreeMap<String, Vec<u8>> {
    f.folder.iter().map(|(k, v)| (k.clone(), v.bytes().unwrap().to_vec())).collect()
}
fn tree(p: &Path) -> BTreeMap<String, Vec<u8>> {
    materialize(&MapFiles::from_dir(p).unwrap())
}
fn model(f: &MapFiles) -> MapModel {
    built::read("p004", f).unwrap()
}
fn doc(m: MapModel) -> MapDocument {
    let db = Arc::new(TileDatabase::default());
    let list = TileList { version: 9, battle_size: [16, 16], ..Default::default() };
    let placed = PlacedMap::from_tile_list("p004", &list, &db, None);
    MapDocument::new("p004".into(), MapSource::Kit(PathBuf::new()), Some(m), Arc::new(placed), None, db)
}
fn attr(d: &mut MapDocument, id: u64, component: &str, name: &str, after: String) {
    let p = d.project().unwrap();
    let e = p.entity(id).unwrap();
    let elem = if component.is_empty() { e.elem } else { p.component(e, component).unwrap() };
    let before = p.doc.get(elem, name).unwrap();
    d.push_terry(Box::new(TerryAttrCommand {
        id,
        component: component.into(),
        attr: name.into(),
        before,
        after,
        key: 0,
        revision: d.terry_revision.clone(),
    }));
}
fn rename(d: &mut MapDocument) {
    let id = d.project().unwrap().entities[0].id;
    attr(d, id, "", "name", "P004 unsaved label".into());
}
fn widen(d: &mut MapDocument) {
    let p = d.project().unwrap();
    let e = p.entities.iter().find(|e| matches!(p.kind(e), wce_terrain::terry::EntityKind::CatchmentRegion)).unwrap();
    let id = e.id;
    let width = p.rectangle(e).unwrap()[0];
    attr(d, id, "ECRectangle", "width", (width + 512.0).to_string());
}
fn state(d: &MapDocument) -> (bool, usize, usize, u64, Vec<u8>) {
    (d.modified(), d.history().size(), d.history().position(), d.history().version(), d.project().unwrap().save())
}
fn save(d: &mut MapDocument, p: &Path) -> Result<(), String> {
    d.save_dir = Some(p.to_owned());
    d.save(&SaveTarget { path: p.to_owned() })
}
fn reject_save(d: &mut MapDocument, id: &str) {
    let p = scratch(id).join("destination");
    std::fs::create_dir_all(&p).unwrap();
    std::fs::write(p.join("sentinel.bin"), b"existing generation").unwrap();
    let before = state(d);
    let original = tree(&p);
    let result = save(d, &p);
    println!(
        "save_ok={} before_dirty={} after_dirty={} destination_unchanged={} error={:?}",
        result.is_ok(),
        before.0,
        d.modified(),
        tree(&p) == original,
        result.as_ref().err()
    );
    if result.is_ok() {
        let saved = MapFiles::from_dir(&p).unwrap();
        if saved.folder.contains_key("locations.blm") {
            if let Ok(b) = BattleLocations::parse(&bytes(&saved, "locations.blm")) {
                let reopened = built::read("reopened", &saved);
                println!(
                    "saved_version={} saved_first_rect={:?} reopened_has_scene={}",
                    b.version,
                    b.types.first().and_then(|t| t.1.first()).map(|r| r.rect),
                    reopened.as_ref().is_ok_and(|m| m.project.is_some())
                );
            }
        } else {
            println!("Save succeeded with no locations.blm in the output");
        }
    }
    assert!(result.is_err(), "unsupported serialization must fail rather than report Save success");
    assert_eq!(state(d), before, "rejection must retain model/history/dirty state");
    assert_eq!(tree(&p), original, "IO preflight must finish before publication");
    assert!(!result.unwrap_err().trim().is_empty(), "report a usable error");
}
fn unchanged_or_error(f: &MapFiles, w: Result<MapFiles, wce_terrain_io::MapIoError>) {
    match w {
        Err(e) => assert!(!e.to_string().is_empty()),
        Ok(w) => assert_eq!(materialize(&w), materialize(f), "unrelated edit must preserve every native byte/key"),
    }
}

#[test]
fn p004_n01_opaque_noop() {
    let f = files(fixture([123, -7]));
    let m = model(&f);
    assert_eq!(materialize(&built::write(&m, None).unwrap()), materialize(&f));
}
#[test]
fn p004_n02_unique_groups_safe_edit() {
    let f = files(fixture([0, 0]));
    let mut d = doc(model(&f));
    widen(&mut d);
    let w = built::write(d.model().unwrap(), None).unwrap();
    let b = BattleLocations::parse(&bytes(&w, "locations.blm")).unwrap();
    let mut expected = fixture([0, 0]);
    expected.types[0].1[0].rect = [2, 4, 6, 6];
    assert_eq!(b, expected, "supported rectangle edit must remain possible and change only its endpoints");
    assert_eq!(bytes(&f, "opaque.bin"), bytes(&w, "opaque.bin"));
    assert_eq!(bytes(&f, "locations.blm.mask1"), bytes(&w, "locations.blm.mask1"));
}
#[test]
fn p004_n03_unrelated_lf() {
    let mut f = files(fixture([123, -7]));
    let mut dds = vec![0u8; 128];
    dds[..4].copy_from_slice(b"DDS ");
    dds[12..16].copy_from_slice(&2u32.to_le_bytes());
    dds[16..20].copy_from_slice(&2u32.to_le_bytes());
    dds.extend([10u16, 20, 30, 40].into_iter().flat_map(u16::to_le_bytes));
    f.folder.insert("lf_height_map.dds".into(), blob(dds));
    let mut m = model(&f);
    m.lf_mut(LfRole::Land).unwrap().edit().set(0, 0, 11);
    let w = built::write(&m, None).unwrap();
    assert_eq!(bytes(&f, "locations.blm"), bytes(&w, "locations.blm"));
    assert_ne!(bytes(&f, "lf_height_map.dds"), bytes(&w, "lf_height_map.dds"));
}
#[test]
fn p004_u01_rename_undo_save() {
    let f = files(fixture([123, -7]));
    let mut d = doc(model(&f));
    let original = d.project().unwrap().save();
    rename(&mut d);
    assert!(d.history_mut().undo());
    assert_eq!(d.project().unwrap().save(), original, "complete undo restores the complete scene bytes");
    assert!(!d.modified());
    println!("observed persistent Item flag after undo={}", d.model().unwrap().project.as_ref().unwrap().edited());
    let out = scratch("u01").join("save");
    save(&mut d, &out).unwrap();
    let saved = MapFiles::from_dir(&out).unwrap();
    println!("clean={} BLM_equal={}", !d.modified(), bytes(&f, "locations.blm") == bytes(&saved, "locations.blm"));
    assert_eq!(materialize(&saved), materialize(&f));
    let reopened = model(&saved);
    assert_eq!(materialize(&built::write(&reopened, None).unwrap()), materialize(&f));
}
#[test]
fn p004_u02_native_edit_undo_redo_undo() {
    let f = files(fixture([123, -7]));
    let mut d = doc(model(&f));
    widen(&mut d);
    assert!(d.modified());
    assert!(d.history_mut().undo());
    assert!(d.history_mut().redo());
    assert!(d.modified());
    assert!(d.history_mut().undo());
    assert!(!d.modified());
    let w = built::write(d.model().unwrap(), None).expect("complete undo is a required preservation no-op");
    assert_eq!(materialize(&w), materialize(&f));
}
#[test]
fn p004_u03_metadata_save_truthful() {
    let mut d = doc(model(&files(fixture([0, 0]))));
    rename(&mut d);
    let before = state(&d);
    let out = scratch("u03").join("save");
    let result = save(&mut d, &out);
    if let Err(e) = result {
        assert!(!e.is_empty());
        assert_eq!(state(&d), before);
    } else {
        let reopened = model(&MapFiles::from_dir(&out).unwrap());
        let retained = reopened.project.as_ref().unwrap().get().entities.iter().any(|e| e.name == "P004 unsaved label");
        println!("Save succeeded; rename retained after reopening={retained}; dirty={}", d.modified());
        assert!(retained, "successful project Save must retain the label, or explicitly reject this unsupported metadata operation");
    }
}
#[test]
fn p004_u04_rename_native_passthrough() {
    let f = files(fixture([123, -7]));
    let mut d = doc(model(&f));
    rename(&mut d);
    unchanged_or_error(&f, built::write(d.model().unwrap(), None));
}
#[test]
fn p004_u05_existing_xml_origin_survives_baking() {
    let mut d = doc(model(&files(fixture([0, 0]))));
    let origin = d.project().unwrap().doc.origin_bytes().to_vec();
    rename(&mut d);
    d.project_mut().unwrap().doc.bake_pending().unwrap();
    assert_eq!(d.project().unwrap().doc.origin_bytes(), origin);
    assert_ne!(d.project().unwrap().doc.original_text(), origin, "baked working text is not an immutable baseline");
    assert!(d.history_mut().undo());
    assert_eq!(d.project().unwrap().save(), origin);
    assert_eq!(d.project().unwrap().doc.origin_bytes(), origin);
}
#[test]
fn p004_o01_opaque_safe_or_reject() {
    let f = files(fixture([123, -7]));
    let mut d = doc(model(&f));
    widen(&mut d);
    if let Ok(w) = built::write(d.model().unwrap(), None) {
        let b = BattleLocations::parse(&bytes(&w, "locations.blm")).unwrap();
        assert_eq!(b.types[0].1[0].extra, [123, -7]);
        assert_eq!(b.tail, fixture([123, -7]).tail);
    }
}
#[test]
fn p004_o02_opaque_rejection_history() {
    let mut d = doc(model(&files(fixture([123, -7]))));
    widen(&mut d);
    reject_save(&mut d, "o02");
}

fn unusual(name: &str) -> MapFiles {
    let mut b = fixture([0, 0]);
    if name == "empty" {
        b.types.push(("future_empty".into(), vec![]));
    } else {
        b.types.push(b.types[0].clone());
    }
    files(b)
}
#[test]
fn p004_g01_empty_unrelated() {
    let f = unusual("empty");
    let mut d = doc(model(&f));
    rename(&mut d);
    unchanged_or_error(&f, built::write(d.model().unwrap(), None));
}
#[test]
fn p004_g02_repeated_unrelated() {
    let f = unusual("repeated");
    let mut d = doc(model(&f));
    rename(&mut d);
    unchanged_or_error(&f, built::write(d.model().unwrap(), None));
}
#[test]
fn p004_g03_empty_undo() {
    let f = unusual("empty");
    let mut d = doc(model(&f));
    rename(&mut d);
    assert!(d.history_mut().undo());
    assert_eq!(materialize(&built::write(d.model().unwrap(), None).unwrap()), materialize(&f));
}
#[test]
fn p004_g04_repeated_undo() {
    let f = unusual("repeated");
    let mut d = doc(model(&f));
    rename(&mut d);
    assert!(d.history_mut().undo());
    assert_eq!(materialize(&built::write(d.model().unwrap(), None).unwrap()), materialize(&f));
}
#[test]
fn p004_g05_ordinary_unknown_ascii_type() {
    let mut b = fixture([0, 0]);
    b.types[0].0 = "future_kind".into();
    let f = files(b);
    let mut d = doc(model(&f));
    rename(&mut d);
    assert_eq!(materialize(&built::write(d.model().unwrap(), None).unwrap()), materialize(&f));
}

const DEP:&[u8]=b"<BATTLE_DEPLOYMENT_AREA_HASH_TABLE>\r\n\t<BATTLE_DEPLOYMENT_AREAS>\r\n\t\t<ALLIANCE id='0'>\r\n\t\t</ALLIANCE>\r\n\t\t<ALLIANCE id='1'>\r\n\t\t</ALLIANCE>\r\n\t</BATTLE_DEPLOYMENT_AREAS>\r\n</BATTLE_DEPLOYMENT_AREA_HASH_TABLE>\r\n";
fn deployed(extension: bool, missing: bool) -> MapFiles {
    let mut b = fixture([0, 0]);
    b.types[0].1[0].id = 0xffff0000u32 as i32;
    let mut f = files(b);
    if !missing {
        let data = if extension {
            String::from_utf8(DEP.to_vec())
                .unwrap()
                .replace(
                    "<BATTLE_DEPLOYMENT_AREA_HASH_TABLE>",
                    "<BATTLE_DEPLOYMENT_AREA_HASH_TABLE diagnostic_opaque='keep'><opaque_extension value='7'/>",
                )
                .into_bytes()
        } else {
            DEP.to_vec()
        };
        f.folder.insert("deployment/deployment_0000.xml".into(), blob(data));
    }
    f.folder.insert("deployment/deployment_0099.xml".into(), blob(b"private orphan; must remain unchanged".to_vec()));
    f
}
#[test]
fn p004_d01_extension_unrelated() {
    let f = deployed(true, false);
    let mut d = doc(model(&f));
    rename(&mut d);
    unchanged_or_error(&f, built::write(d.model().unwrap(), None));
}
#[test]
fn p004_d02_extension_noop() {
    let f = deployed(true, false);
    assert_eq!(materialize(&built::write(&model(&f), None).unwrap()), materialize(&f));
}
#[test]
fn p004_d03_missing_dependency_unsafe_edit() {
    let f = deployed(false, true);
    let mut d = doc(model(&f));
    widen(&mut d);
    let result = built::write(d.model().unwrap(), None);
    println!(
        "serialize_ok={} synthesized_dependency={}",
        result.is_ok(),
        result.as_ref().is_ok_and(|w| w.folder.contains_key("deployment/deployment_0000.xml"))
    );
    assert!(result.is_err(), "missing referenced data must not be replaced by an invented empty deployment during an edit");
}
#[test]
fn p004_d04_missing_dependency_noop() {
    let f = deployed(false, true);
    assert_eq!(materialize(&built::write(&model(&f), None).unwrap()), materialize(&f));
}
#[test]
fn p004_d05_orphan_preserved_safe_edit() {
    let f = deployed(false, false);
    let mut d = doc(model(&f));
    widen(&mut d);
    let w = built::write(d.model().unwrap(), None).unwrap();
    assert_eq!(bytes(&w, "deployment/deployment_0099.xml"), bytes(&f, "deployment/deployment_0099.xml"));
    assert_eq!(bytes(&w, "deployment/deployment_0000.xml"), bytes(&f, "deployment/deployment_0000.xml"));
}
#[test]
fn p004_d06_extension_undo() {
    let f = deployed(true, false);
    let mut d = doc(model(&f));
    rename(&mut d);
    assert!(d.history_mut().undo());
    assert_eq!(materialize(&built::write(d.model().unwrap(), None).unwrap()), materialize(&f));
}
#[test]
fn p004_d07_extension_safe_rectangle_edit() {
    let f = deployed(true, false);
    let mut d = doc(model(&f));
    widen(&mut d);
    let w = built::write(d.model().unwrap(), None)
        .expect("an unrelated XML extension must not block this independently supported BLM rectangle edit");
    assert_eq!(BattleLocations::parse(&bytes(&w, "locations.blm")).unwrap().types[0].1[0].rect, [2, 4, 6, 6]);
    for k in f.folder.keys().filter(|k| k.as_str() != "locations.blm") {
        assert_eq!(bytes(&w, k), bytes(&f, k), "unchanged resources must use original raw bytes, not scene reconstruction");
    }
}

fn changed_baseline(case: &str) {
    let source = scratch(case).join("source");
    let f = deployed(false, false);
    f.write_to(&source).unwrap();
    let mut d = doc(model(&MapFiles::from_dir(&source).unwrap()));
    widen(&mut d);
    let path = source.join("locations.blm");
    let mut b = BattleLocations::parse(&std::fs::read(&path).unwrap()).unwrap();
    match case {
        "version" => {
            b.version = 8;
            std::fs::write(&path, b.write().expect("P004 checked native serialization")).unwrap();
        }
        "same-version" => {
            b.tail.push(0x7f);
            std::fs::write(&path, b.write().expect("P004 checked native serialization")).unwrap();
        }
        "missing" => std::fs::remove_file(&path).unwrap(),
        "deployment" => std::fs::write(source.join("deployment/deployment_0000.xml"), b"<changed/>").unwrap(),
        _ => panic!("case"),
    }
    // A frozen, load-time byte authority is a valid alternative to refusal for physical source changes.
    // This is distinct from replacing/removing the semantic baseline in MapModel (V07--V10).
    let before = state(&d);
    let output = scratch(&format!("{case}-save")).join("destination");
    let result = save(&mut d, &output);
    if let Err(e) = result {
        assert!(!e.is_empty());
        assert_eq!(state(&d), before);
    } else {
        let saved = MapFiles::from_dir(&output).unwrap();
        let mut expected = materialize(&f);
        let mut b = BattleLocations::parse(&expected["locations.blm"]).unwrap();
        b.types[0].1[0].rect = [2, 4, 6, 6];
        expected.insert("locations.blm".into(), b.write().expect("P004 checked native serialization"));
        let actual = materialize(&saved);
        let reopened = built::read("reopen", &saved);
        println!(
            "physical-source Save succeeded; snapshot_output_equal={} dirty={} reopened_has_scene={}",
            actual == expected,
            d.modified(),
            reopened.as_ref().is_ok_and(|m| m.project.is_some())
        );
        assert_eq!(
            actual, expected,
            "successful output must use the complete original byte authority and retain the intended supported native edit"
        );
        assert!(!d.modified());
        assert!(reopened.unwrap().project.is_some());
    }
}
#[test]
fn p004_v01_changed_version_save() {
    changed_baseline("version");
}
#[test]
fn p004_v02_same_version_baseline_save() {
    changed_baseline("same-version");
}
#[test]
fn p004_v03_missing_baseline_preflight() {
    let path = scratch("v03").join("source");
    files(fixture([0, 0])).write_to(&path).unwrap();
    let mut d = doc(model(&MapFiles::from_dir(&path).unwrap()));
    widen(&mut d);
    std::fs::remove_file(path.join("locations.blm")).unwrap();
    if let Ok(w) = built::write(d.model().unwrap(), None) {
        let original = files(fixture([0, 0]));
        let mut expected = materialize(&original);
        let mut b = fixture([0, 0]);
        b.types[0].1[0].rect = [2, 4, 6, 6];
        expected.insert("locations.blm".into(), b.write().expect("P004 checked native serialization"));
        assert_eq!(
            materialize(&w),
            expected,
            "successful preflight must hand publication immutable, readable authoritative output; not a missing File blob"
        );
    }
}
#[test]
fn p004_v04_changed_dependency_save() {
    changed_baseline("deployment");
}
#[test]
fn p004_v05_opaque_versions() {
    for v in [6, 8, 9, 99] {
        let mut b = fixture([123, -7]);
        b.version = v;
        let f = files(b);
        let m = model(&f);
        assert!(m.project.is_none());
        assert_eq!(materialize(&built::write(&m, None).unwrap()), materialize(&f));
    }
}
#[test]
fn p004_v06_same_bytes_replacement_content() {
    let path = scratch("v06").join("source");
    let f = files(fixture([0, 0]));
    f.write_to(&path).unwrap();
    let mut d = doc(model(&MapFiles::from_dir(&path).unwrap()));
    widen(&mut d);
    std::fs::write(path.join("locations.blm"), bytes(&f, "locations.blm")).unwrap();
    let w = built::write(d.model().unwrap(), None).unwrap();
    assert_eq!(BattleLocations::parse(&bytes(&w, "locations.blm")).unwrap().types[0].1[0].rect, [2, 4, 6, 6]);
}
fn replaced_model_baseline(case: &str) {
    let f = deployed(false, false);
    let mut m = model(&f);
    match case {
        "model-version" => {
            let mut b = BattleLocations::parse(&bytes(&f, "locations.blm")).unwrap();
            b.version = 8;
            m.carried.insert("locations.blm".into(), blob(b.write().expect("P004 checked native serialization")));
        }
        "model-missing" => {
            m.carried.remove("locations.blm");
        }
        "model-tail" => {
            let mut b = BattleLocations::parse(&bytes(&f, "locations.blm")).unwrap();
            b.tail.push(0x7f);
            m.carried.insert("locations.blm".into(), blob(b.write().expect("P004 checked native serialization")));
        }
        "model-dependency" => {
            m.carried.insert("deployment/deployment_0000.xml".into(), blob(b"<changed/>".to_vec()));
        }
        _ => panic!("case"),
    }
    let mut d = doc(m);
    widen(&mut d);
    reject_save(&mut d, case);
}
#[test]
fn p004_v07_incompatible_model_baseline() {
    replaced_model_baseline("model-version");
}
#[test]
fn p004_v08_missing_model_baseline() {
    replaced_model_baseline("model-missing");
}
#[test]
fn p004_v09_same_version_model_replaced() {
    replaced_model_baseline("model-tail");
}
#[test]
fn p004_v10_model_dependency_replaced() {
    replaced_model_baseline("model-dependency");
}

fn single_byte_name(byte: u8) -> Vec<u8> {
    let mut b = b"BLMF".to_vec();
    for n in [7u32, 1, 16, 16] {
        b.extend(n.to_le_bytes());
    }
    b.extend(1u16.to_le_bytes());
    b.push(byte);
    b.extend(0u32.to_le_bytes());
    b.extend([0u8; 8]);
    b
}
#[test]
fn p004_e01_non_ascii_name_inverse() {
    let input = single_byte_name(0xe9);
    let b = BattleLocations::parse(&input).unwrap();
    let out = b.write().expect("P004 checked native serialization");
    println!(
        "prefix_before={} prefix_after={}",
        u16::from_le_bytes(input[20..22].try_into().unwrap()),
        u16::from_le_bytes(out[20..22].try_into().unwrap())
    );
    assert_eq!(out, input, "single-byte preservation does not imply UTF-8 is the native encoding");
}
#[test]
fn p004_e02_ascii_name_inverse() {
    for c in b'a'..=b'z' {
        let b = single_byte_name(c);
        assert_eq!(BattleLocations::parse(&b).unwrap().write().expect("P004 checked native serialization"), b);
    }
}
#[test]
fn p004_e03_unrepresentable_edit_rejected() {
    let mut d = doc(model(&files(fixture([0, 0]))));
    let id = d.project().unwrap().entities[0].id;
    attr(&mut d, id, "ECBattleCatchmentArea", "catchment_type", "\u{100}".into());
    assert!(
        built::write(d.model().unwrap(), None).is_err(),
        "new names outside the existing single-byte representation must not be truncated"
    );
}
#[test]
fn p004_e04_oversized_name_rejected() {
    let mut d = doc(model(&files(fixture([0, 0]))));
    let id = d.project().unwrap().entities[0].id;
    attr(&mut d, id, "ECBattleCatchmentArea", "catchment_type", "a".repeat(65536));
    assert!(built::write(d.model().unwrap(), None).is_err(), "u16 length overflow must not silently wrap");
}
#[test]
fn p004_r01_opaque_edit_rejection_then_undo_save() {
    let f = files(fixture([123, -7]));
    let mut d = doc(model(&f));
    widen(&mut d);
    reject_save(&mut d, "r01-reject");
    assert!(d.history_mut().undo());
    assert!(!d.modified());
    let path = scratch("r01-retry").join("save");
    save(&mut d, &path).unwrap();
    assert_eq!(tree(&path), materialize(&f));
}
#[test]
fn p004_r02_bad_id_explicit_failure() {
    let mut b = fixture([0, 0]);
    b.types[0].1[0].id = 0xffff0001u32 as i32;
    assert!(built::read("bad-id", &files(b)).is_err());
}
#[test]
fn p004_r04_unrepresented_inspector_change() {
    let mut d = doc(model(&files(fixture([0, 0]))));
    let id = d.project().unwrap().entities[0].id;
    attr(&mut d, id, "ECLandscapeAbove", "active", "false".into());
    reject_save(&mut d, "r04");
}
#[test]
fn p004_r05_repeat_save_undo_redo_after_save() {
    let f = files(fixture([0, 0]));
    let path = scratch("r05").join("project");
    f.write_to(&path).unwrap();
    let mut d = doc(model(&MapFiles::from_dir(&path).unwrap()));
    widen(&mut d);
    save(&mut d, &path).unwrap();
    assert!(!d.modified());
    let edited = tree(&path);
    assert_ne!(edited, materialize(&f));
    save(&mut d, &path).unwrap();
    assert_eq!(tree(&path), edited);
    assert!(d.history_mut().undo());
    assert!(d.modified(), "undo after Save is dirty relative to the saved generation");
    save(&mut d, &path).unwrap();
    assert_eq!(tree(&path), materialize(&f));
    assert!(!d.modified());
    assert!(d.history_mut().redo());
    assert!(d.modified());
    save(&mut d, &path).unwrap();
    assert_eq!(tree(&path), edited);
    assert!(!d.modified());
}
#[test]
fn p004_c01_raw_authoring_passthrough() {
    for v in [6, 7, 8] {
        let mut b = fixture([123, -7]);
        b.version = v;
        let f = files(b);
        let m = attila::read("raw", MapKind::Battle, &f).unwrap();
        assert_eq!(materialize(&attila::write(&m).unwrap()), materialize(&f));
    }
}

fn witness() -> PathBuf {
    PathBuf::from(std::env::var_os("P004_D1_WITNESS_ROOT").expect("private D1 witness root required"))
}
fn native_folders(game: &str) -> Vec<PathBuf> {
    let mut p: Vec<_> =
        std::fs::read_dir(witness().join(game)).unwrap().map(|x| x.unwrap().path().join("noop")).filter(|p| p.is_dir()).collect();
    p.sort();
    p
}
#[test]
#[ignore = "private native witness"]
fn p004_n10_nine_native_noops() {
    let mut n = 0;
    let mut records = 0;
    for game in ["rome2", "attila"] {
        for p in native_folders(game) {
            let f = MapFiles::from_dir(&p).unwrap();
            let b = BattleLocations::parse(&bytes(&f, "locations.blm")).unwrap();
            assert_eq!(b.write().expect("P004 checked native serialization"), bytes(&f, "locations.blm"));
            records += b.types.iter().map(|t| t.1.len()).sum::<usize>();
            let w = built::write_project(&model(&f), None).unwrap();
            assert_eq!(materialize(&w), materialize(&f));
            n += 1;
        }
    }
    assert_eq!((n, records), (9, 99962));
    println!("native_resources={n} native_records={records}");
}
#[test]
#[ignore = "private native witness"]
fn p004_n11_native_rename_and_reopen() {
    let mut n = 0;
    let mut deps = 0;
    let mut orphans = 0;
    for p in native_folders("rome2") {
        let f = MapFiles::from_dir(&p).unwrap();
        let b = BattleLocations::parse(&bytes(&f, "locations.blm")).unwrap();
        if b.version != 7 {
            continue;
        }
        let m = model(&f);
        let (rewritten, generated) = built::scene::write(m.project.as_ref().unwrap().get(), &b).expect("P004 checked native serialization");
        assert_eq!(rewritten, bytes(&f, "locations.blm"));
        let all = f.folder.keys().filter(|k| k.starts_with("deployment/")).count();
        deps += all;
        orphans += all - generated.len();
        let mut d = doc(m);
        rename(&mut d);
        let w = built::write(d.model().unwrap(), None).expect("native-only rename must retain native bytes");
        assert_eq!(materialize(&w), materialize(&f));
        let out = scratch(&format!("n11-{n}"));
        w.write_to(&out).unwrap();
        let reopened = model(&MapFiles::from_dir(&out).unwrap());
        assert_eq!(materialize(&built::write(&reopened, None).unwrap()), materialize(&f));
        n += 1;
    }
    assert_eq!((n, deps, orphans), (6, 8039, 902));
    println!("v7_maps={n} deployment_files={deps} orphans={orphans}");
}
#[test]
#[ignore = "private native witness"]
fn p004_n12_native_gaul_catchment_edit() {
    let p = witness().join("rome2/terrain_battles_main_gaul/noop");
    let f = MapFiles::from_dir(&p).unwrap();
    let input = bytes(&f, "locations.blm");
    let original = BattleLocations::parse(&input).unwrap();
    let mut d = doc(model(&f));
    widen(&mut d);
    let w = built::write(d.model().unwrap(), None).unwrap();
    let output = bytes(&w, "locations.blm");
    let edited = BattleLocations::parse(&output).unwrap();
    assert_eq!(input.len(), output.len());
    assert_eq!(input.iter().zip(&output).filter(|(x, y)| x != y).count(), 2);
    assert_eq!(original.tail, edited.tail);
    assert_eq!((original.version, original.width, original.height), (edited.version, edited.width, edited.height));
    let mut changes = 0;
    for ((a, x), (b, y)) in original.types.iter().zip(&edited.types) {
        assert_eq!(a, b);
        assert_eq!(x.len(), y.len());
        for (x, y) in x.iter().zip(y) {
            assert_eq!((x.centre, x.id, x.extra), (y.centre, y.id, y.extra));
            changes += usize::from(x.rect != y.rect);
        }
    }
    assert_eq!(changes, 1);
    for k in f.folder.keys().filter(|k| k.as_str() != "locations.blm") {
        assert_eq!(bytes(&f, k), bytes(&w, k));
    }
    let out = scratch("n12");
    w.write_to(&out).unwrap();
    let re = model(&MapFiles::from_dir(&out).unwrap());
    assert_eq!(materialize(&built::write(&re, None).unwrap()), materialize(&w));
}
#[test]
#[ignore = "private native witness"]
fn p004_c10_native_v6_v8_passthrough() {
    let mut seen = vec![];
    for game in ["rome2", "attila"] {
        for p in native_folders(game) {
            let f = MapFiles::from_dir(&p).unwrap();
            let b = BattleLocations::parse(&bytes(&f, "locations.blm")).unwrap();
            if b.version == 7 {
                continue;
            }
            let m = model(&f);
            assert!(m.project.is_none());
            assert_eq!(materialize(&built::write(&m, None).unwrap()), materialize(&f));
            seen.push(b.version);
        }
    }
    seen.sort();
    assert_eq!(seen, vec![6, 8, 8]);
}
