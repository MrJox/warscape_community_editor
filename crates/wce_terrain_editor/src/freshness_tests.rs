// Included as a child of publication_tests to reuse its accepted synthetic fixtures.
#[test]
fn save_as_copy_cannot_authorize_the_unrelated_active_destination() {
    let root = Scratch::new();
    let active = root.0.join("active");
    fixture().write_to(&active).unwrap();
    let mut d = doc(built::read("test", &fixture()).unwrap());
    d.save_dir = Some(active.clone());
    d.observe_save_destination().unwrap();
    changed(&mut d);
    let before = state(&d);
    let original = disk(&active);
    let copy = SaveTarget { path: root.0.join("copy") };
    d.save(&copy).unwrap();
    assert_eq!(state(&d), before);
    assert_eq!(disk(&active), original);
    assert!(d.save_changes(&SaveTarget::default()).unwrap().is_some());
    assert!(d.io_changes.lock().unwrap().begin_export(&root.0.join("export")).is_err());
    assert!(d.save(&SaveTarget::default()).is_err());
    assert_eq!(state(&d), before);
    assert_eq!(disk(&active), original);
}

#[test]
fn enumeration_stamp_survives_changes_before_document_creation() {
    let root = Scratch::new();
    let source = root.0.join("source");
    fixture().write_to(&source).unwrap();
    let input = MapFiles::from_dir(&source).unwrap();
    std::fs::write(source.join("normals/normal_0.dds"), b"external replacement with another size").unwrap();
    let mut d = doc(built::read("test", &input).unwrap());
    d.save_dir = Some(source.clone());
    changed(&mut d);
    let before = state(&d);
    let target = SaveTarget { path: root.0.join("out") };
    assert!(d.save_changes(&target).unwrap().is_some());
    assert!(d.save(&target).is_err());
    assert_eq!(state(&d), before);
    assert!(!target.path.join("lf_height_map.dds").exists());
    d.save_overwrite(&target).unwrap();
    assert_eq!(state(&d), before);
    d.save_overwrite(&SaveTarget::default()).unwrap();
    assert!(!d.modified());
}

#[test]
fn external_change_cancel_overwrite_and_own_publication_use_actual_save_folder() {
    let root = Scratch::new();
    let source = root.0.join("source");
    fixture().write_to(&source).unwrap();
    let mut d = doc(built::read("test", &MapFiles::from_dir(&source).unwrap()).unwrap());
    d.save_dir = Some(source.clone());
    changed(&mut d);
    let mut docs = DocumentManager::default();
    let mut scene = wce_scene::render_scene::RenderScene::default();
    let id = docs.add(&mut scene, Box::new(d), "terrain").unwrap();
    std::fs::write(source.join("normals/normal_0.dds"), b"external opaque normals, resized").unwrap();
    let before = state(docs.find_as::<MapDocument>(id).unwrap());
    let on_disk = disk(&source);
    assert!(docs.save_one(id).is_err());
    assert_eq!(docs.pending_changed_save().unwrap().id, id);
    docs.answer_changed_save(false).unwrap();
    assert_eq!(state(docs.find_as::<MapDocument>(id).unwrap()), before);
    assert_eq!(disk(&source), on_disk);
    assert!(docs.save_one(id).is_err());
    docs.answer_changed_save(true).unwrap();
    assert!(!docs.find(id).unwrap().modified());
    assert_eq!(docs.find(id).unwrap().save_destination(), Some(source.clone()));
    let saved = disk(&source);
    docs.save_one(id).unwrap();
    assert!(docs.pending_changed_save().is_none());
    assert_eq!(disk(&source), saved);
    let copy = root.0.join("save-as");
    docs.save_to(id, &SaveTarget { path: copy.clone() }).unwrap();
    assert_eq!(docs.find(id).unwrap().save_destination(), Some(source.clone()));
    std::fs::write(copy.join("normals/normal_0.dds"), b"external change to the separate copy").unwrap();
    docs.save_one(id).unwrap();
    assert!(docs.pending_changed_save().is_none());
    assert!(docs.save_to(id, &SaveTarget { path: copy.clone() }).is_err());
    assert_eq!(docs.pending_changed_save().unwrap().target.path, copy);
    docs.answer_changed_save(false).unwrap();
    assert_eq!(disk(&source), saved);
}

#[test]
fn observing_populated_destination_does_not_authorize_overwrite_or_export() {
    let root = Scratch::new();
    fixture().write_to(&root.0).unwrap();
    let mut d = doc(built::read("test", &fixture()).unwrap());
    d.save_dir = Some(root.0.clone());
    d.observe_save_destination().unwrap();
    assert!(d.save_changes(&SaveTarget::default()).unwrap().is_some());
    assert!(d.io_changes.lock().unwrap().begin_export(&root.0.join("export")).is_err());
    let before = state(&d);
    let on_disk = disk(&root.0);
    assert!(d.save(&SaveTarget::default()).is_err());
    assert_eq!(state(&d), before);
    assert_eq!(disk(&root.0), on_disk);
    d.save_overwrite(&SaveTarget::default()).unwrap();
    assert!(d.save_changes(&SaveTarget::default()).unwrap().is_none());
    d.save(&SaveTarget::default()).unwrap();
}

#[test]
fn normalized_cross_resource_file_sources_cannot_be_overwritten() {
    let root = Scratch::new();
    fixture().write_to(&root.0).unwrap();
    std::fs::create_dir(root.0.join("alias")).unwrap();
    std::fs::write(root.0.join("a.bin"), b"AA").unwrap();
    std::fs::write(root.0.join("b.bin"), b"BB").unwrap();
    let input = MapFiles::from_dir(&root.0).unwrap();
    let mut model = built::read("test", &input).unwrap();
    model.carried.insert("a.bin".into(), Blob::File(root.0.join("alias/../b.bin")));
    model.carried.insert("b.bin".into(), Blob::File(root.0.join("alias/../a.bin")));
    let mut d = doc(model);
    changed(&mut d);
    let before = state(&d);
    let on_disk = disk(&root.0);
    assert!(d.save_overwrite(&SaveTarget { path: root.0.clone() }).unwrap_err().contains("another resource"));
    assert_eq!(state(&d), before);
    assert_eq!(disk(&root.0), on_disk);
}

#[test]
fn destination_only_keys_and_spelling_changes_refuse_even_with_overwrite() {
    for spelling in [false, true] {
        let root = Scratch::new();
        fixture().write_to(&root.0).unwrap();
        if spelling {
            std::fs::rename(root.0.join("terrain.tile_list"), root.0.join("TERRAIN.TILE_LIST")).unwrap();
        } else {
            std::fs::write(root.0.join("unknown.bin"), b"retain").unwrap();
        }
        let mut d = doc(built::read("test", &fixture()).unwrap());
        changed(&mut d);
        let before = state(&d);
        let on_disk = disk(&root.0);
        assert!(d.save_overwrite(&SaveTarget { path: root.0.clone() }).is_err());
        assert_eq!(state(&d), before);
        assert_eq!(disk(&root.0), on_disk);
    }
}

#[test]
fn complete_input_keys_reject_aliases_directory_conflicts_and_decoded_duplicates() {
    for names in [["x", "X"], ["directory", "DIRECTORY/file"], ["Dir/a", "dir/b"], ["lf_height_map.dds", "opaque"]] {
        let root = Scratch::new();
        let mut model = built::read("test", &fixture()).unwrap();
        for name in names {
            model.carried.insert(name.into(), Blob::Bytes(Arc::from(&b"opaque"[..])));
        }
        let mut d = doc(model);
        changed(&mut d);
        let before = state(&d);
        let target = root.0.join("out");
        assert!(d.save_overwrite(&SaveTarget { path: target.clone() }).is_err());
        assert_eq!(state(&d), before);
        assert!(!target.exists());
    }
}

fn blm_files() -> MapFiles {
    use wce_tile::battle_map::locations::{BattleLocations, Location};
    let native = BattleLocations {
        version: 7,
        width: 16,
        height: 16,
        types: vec![("land_normal".into(), vec![Location { rect: [3, 4, 5, 6], centre: [4, 5], id: -1, extra: [0, 0] }])],
        tail: b"opaque diagnostic tail\0\xff".to_vec(),
    };
    MapFiles::from_bytes([
        ("locations.blm".into(), native.write().unwrap()),
        ("locations.blm.mask1".into(), vec![0xa5, 0x5a]),
        ("opaque.bin".into(), vec![1, 255, 0, 7]),
    ])
}
fn widen(d: &mut MapDocument) {
    let p = d.project().unwrap();
    let e = p.entities.iter().find(|e| matches!(p.kind(e), wce_terrain::terry::EntityKind::CatchmentRegion)).unwrap();
    let id = e.id;
    let elem = p.component(e, "ECRectangle").unwrap();
    let before = p.doc.get(elem, "width").unwrap();
    let after = (p.rectangle(e).unwrap()[0] + 512.0).to_string();
    d.push_terry(Box::new(crate::terry::TerryAttrCommand {
        id,
        component: "ECRectangle".into(),
        attr: "width".into(),
        before,
        after,
        key: 0,
        revision: d.terry_revision.clone(),
    }));
}

// Accepted R05 repeat/undo/redo control, adapted only to the current built::read API.
#[test]
fn p004_r05_repeat_save_undo_redo_after_save() {
    let root = Scratch::new();
    let original = blm_files();
    original.write_to(&root.0).unwrap();
    let baseline = disk(&root.0);
    let mut d = doc(built::read("test", &MapFiles::from_dir(&root.0).unwrap()).unwrap());
    d.save_dir = Some(root.0.clone());
    widen(&mut d);
    let target = SaveTarget { path: root.0.clone() };
    d.save(&target).unwrap();
    assert!(!d.modified());
    let edited = disk(&root.0);
    assert_ne!(edited, baseline);
    d.save(&target).unwrap();
    assert_eq!(disk(&root.0), edited);
    assert!(d.history_mut().undo());
    assert!(d.modified());
    d.save(&target).unwrap();
    assert_eq!(disk(&root.0), baseline);
    assert!(!d.modified());
    assert!(d.history_mut().redo());
    assert!(d.modified());
    d.save(&target).unwrap();
    assert_eq!(disk(&root.0), edited);
    assert!(!d.modified());
}

#[test]
fn file_backed_undo_and_independent_save_recover_without_false_staleness() {
    let root = Scratch::new();
    let source = root.0.join("source");
    blm_files().write_to(&source).unwrap();
    let baseline = disk(&source);
    let mut d = doc(built::read("test", &MapFiles::from_dir(&source).unwrap()).unwrap());
    d.save_dir = Some(source.clone());
    widen(&mut d);
    let target = SaveTarget { path: source.clone() };
    d.save(&target).unwrap();
    let edited = disk(&source);
    assert!(d.history_mut().undo());
    d.save(&target).unwrap();
    assert_eq!(disk(&source), baseline);
    let copy = root.0.join("copy");
    d.save(&SaveTarget { path: copy.clone() }).unwrap();
    assert_eq!(disk(&copy), baseline);
    assert!(d.history_mut().redo());
    d.save(&target).unwrap();
    assert_eq!(disk(&source), edited);
    assert!(!d.modified());
}
