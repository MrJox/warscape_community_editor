use super::*;
use wce_editor_core::document::Document;
use wce_terrain_editor::document::MapSource;
use wce_tile::battle_map::context::PlacedMap;

// Use the built reader to create an Item with an actual original resource; no game fixtures or window/device are needed.
fn document(version: u32, unknown: bool) -> MapDocument {
    let mut tail = vec![4, 0, 0, 0, 1, 0, 0, 0, 0, 0];
    tail.extend(3u32.to_le_bytes());
    for value in [0x11, 0x22, 0x33] {
        tail.extend([value; 32]);
    }
    if unknown {
        tail = vec![9, 8, 7]
    }
    let list = TileList {
        version,
        battle_size: [64, 64],
        names: vec!["tile".to_owned()],
        placements: (0..3)
            .map(|x| TileListPlacement {
                x,
                ..Default::default()
            })
            .collect(),
        tail,
        ..Default::default()
    };
    let files =
        wce_terrain_io::MapFiles::from_bytes([("terrain.tile_list".to_owned(), list.write())]);
    let model = wce_terrain_io::built::read("campaigns/test", &files).unwrap();
    let mut d = MapDocument::new(
        "campaigns/test".to_owned(),
        MapSource::Packs("test".to_owned()),
        Some(model),
        Arc::new(PlacedMap::default()),
        None,
        Arc::new(TileDatabase::default()),
    );
    d.history_mut().mark_saved();
    d
}

#[test]
fn zero_rows_allow_count_commands_save_and_exact_undo_with_multiple_names() {
    for names in 0u32..=3 {
        let mut list = PlacementsPanel::list(&document(9, false)).unwrap().clone();
        list.tail = 4u32.to_le_bytes().to_vec();
        list.tail.extend(names.to_le_bytes());
        for _ in 0..names {
            list.tail.extend([1, 0, b'x']);
        }
        list.tail.extend(0u32.to_le_bytes());
        let files =
            wce_terrain_io::MapFiles::from_bytes([("terrain.tile_list".into(), list.write())]);
        let model = wce_terrain_io::built::read("zero rows", &files).unwrap();
        let mut d = MapDocument::new(
            "zero rows".into(),
            MapSource::Packs("test".into()),
            Some(model),
            Arc::default(),
            None,
            Arc::default(),
        );
        d.history_mut().mark_saved();
        let before = state(&d);
        PlacementsPanel::edit_document(&mut d, "remove placement", true, |l, _| {
            l.placements.pop();
            Ok(String::new())
        })
        .unwrap();
        let written = wce_terrain_io::built::write(d.model().unwrap(), None).unwrap();
        let loaded = wce_terrain_io::built::read("saved", &written).unwrap();
        let loaded = loaded.tile_list.as_ref().unwrap().get();
        assert_eq!(loaded.placements.len(), 2);
        assert_eq!(loaded.tail, list.tail);
        assert!(d.history_mut().undo());
        assert_eq!(state(&d).0, before.0);
        assert!(!d.history().modified());
        assert!(d.history_mut().redo());
        assert_eq!(PlacementsPanel::list(&d).unwrap().placements.len(), 2);
    }
}

#[test]
fn missing_built_placements_keep_the_informative_error_before_guard_checks() {
    let mut d = MapDocument::new(
        "no native model".into(),
        MapSource::Packs("test".into()),
        None,
        Arc::default(),
        None,
        Arc::default(),
    );
    let before = (
        d.history().size(),
        d.history().position(),
        d.history().version(),
        d.history().modified(),
    );
    let err =
        PlacementsPanel::edit_document(&mut d, "move", false, |_, _| panic!("must not mutate"))
            .unwrap_err();
    assert_eq!(
        err,
        "a built map with a terrain.tile_list (Rome 2's pack maps) has placements to edit"
    );
    assert_eq!(
        (
            d.history().size(),
            d.history().position(),
            d.history().version(),
            d.history().modified()
        ),
        before
    );
}

fn state(d: &MapDocument) -> (Vec<u8>, usize, usize, bool, u64, bool, bool) {
    let h = d.history();
    (
        PlacementsPanel::list(d).unwrap().write(),
        h.size(),
        h.position(),
        h.modified(),
        h.version(),
        h.can_undo(),
        h.can_redo(),
    )
}

#[test]
fn rejected_count_actions_never_invoke_mutation_or_touch_document_history() {
    for i in 0..3 {
        for with_redo in [false, true] {
            let mut d = document(9, false);
            if with_redo {
                PlacementsPanel::edit_document(&mut d, "move a placement", false, |l, _| {
                    l.placements[0].x += 5;
                    Ok(String::new())
                })
                .unwrap();
                d.history_mut().undo();
            }
            let before = state(&d);
            let edited = d.model().unwrap().tile_list.as_ref().unwrap().edited();
            let _ = d.cells.take();
            let mut called = false;
            let r = PlacementsPanel::edit_document(&mut d, "remove a placement", true, |l, _| {
                called = true;
                l.placements.remove(i);
                Ok(String::new())
            });
            assert!(r.unwrap_err().contains("remove a placement"));
            assert!(!called);
            assert_eq!(state(&d), before);
            assert_eq!(
                d.model().unwrap().tile_list.as_ref().unwrap().edited(),
                edited
            );
            assert!(d.cells.take().is_empty());

            let r = PlacementsPanel::edit_document(&mut d, "place a tile", true, |l, _| {
                called = true;
                l.placements.insert(i, l.placements[0].clone());
                Ok(String::new())
            });
            assert!(r.unwrap_err().contains("place a tile"));
            assert!(!called);
            assert_eq!(state(&d), before);
        }
    }
}

#[test]
fn supported_same_count_command_and_undo_redo_still_work() {
    let mut d = document(9, false);
    let before = state(&d).0;
    PlacementsPanel::edit_document(&mut d, "move a placement", false, |l, _| {
        l.placements[1].x += 5;
        Ok("moved".to_owned())
    })
    .unwrap();
    let after = state(&d).0;
    assert_ne!(after, before);
    assert!(d.history().modified());
    assert!(d.history_mut().undo());
    assert_eq!(state(&d).0, before);
    assert!(!d.history().modified());
    assert!(d.history_mut().redo());
    assert_eq!(state(&d).0, after);
}

#[test]
fn unknown_layout_is_refused_and_unannounced_count_change_cannot_record_a_command() {
    let mut d = document(9, true);
    let before = state(&d);
    assert!(
        PlacementsPanel::edit_document(&mut d, "move a placement", false, |_, _| {
            panic!("unknown layout")
        })
        .is_err()
    );
    assert_eq!(state(&d), before);
    let mut d = document(9, false);
    let before = state(&d);
    assert!(
        PlacementsPanel::edit_document(&mut d, "bypassed remove", false, |l, _| {
            l.placements.pop();
            Ok(String::new())
        })
        .is_err()
    );
    assert_eq!(state(&d), before);
}

#[test]
fn v7_v8_actions_and_v11_panel_exclusion_are_unchanged() {
    for version in [7, 8] {
        let mut d = document(version, false);
        PlacementsPanel::edit_document(&mut d, "remove a placement", true, |l, _| {
            l.placements.pop();
            Ok(String::new())
        })
        .unwrap();
        assert_eq!(PlacementsPanel::list(&d).unwrap().placements.len(), 2);
        assert!(d.history_mut().undo());
        assert_eq!(PlacementsPanel::list(&d).unwrap().placements.len(), 3);
    }
    let mut d = document(11, false);
    let before = state(&d);
    assert!(
        PlacementsPanel::edit_document(&mut d, "remove a placement", true, |_, _| {
            panic!("v11 is not edited by this panel")
        })
        .unwrap_err()
        .contains("versions 7 to 9")
    );
    assert_eq!(state(&d), before);
}

#[test]
fn document_save_refuses_an_independently_recorded_inconsistent_list_without_writing() {
    let mut d = document(9, false);
    let before = PlacementsPanel::list(&d).unwrap().clone();
    let mut after = before.clone();
    after.placements.remove(1);
    d.push_tile_list(TileListCommand {
        name: "bypassed removal".to_owned(),
        before,
        after,
        watch: d.cells.clone(),
    });
    let state_before_save = state(&d);
    assert!(d.history().modified());
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!("wce-v9-save-{}-{stamp}", std::process::id()));
    std::fs::create_dir(&path).unwrap();
    let r = d.save(&wce_editor_core::document::SaveTarget { path: path.clone() });
    assert!(r.unwrap_err().contains("paired metadata count"));
    assert_eq!(state(&d), state_before_save);
    assert!(std::fs::read_dir(&path).unwrap().next().is_none());
    std::fs::remove_dir(&path).unwrap();
}

#[test]
fn review_document_nan_no_op_saves_exactly_and_rejected_edit_keeps_saved_history() {
    let mut list = PlacementsPanel::list(&document(9, true)).unwrap().clone();
    list.unknown = [f32::from_bits(0x7fc01234), -0.0];
    let bytes = list.write();
    let files =
        wce_terrain_io::MapFiles::from_bytes([("terrain.tile_list".to_owned(), bytes.clone())]);
    let model = wce_terrain_io::built::read("test", &files).unwrap();
    let mut d = MapDocument::new(
        "test".to_owned(),
        MapSource::Packs("test".to_owned()),
        Some(model),
        Arc::new(PlacedMap::default()),
        None,
        Arc::new(TileDatabase::default()),
    );
    d.history_mut().mark_saved();
    d.push_tile_list(TileListCommand {
        name: "unchanged NaN".to_owned(),
        before: list.clone(),
        after: list,
        watch: d.cells.clone(),
    });
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!("wce-nan-save-{}-{stamp}", std::process::id()));
    std::fs::create_dir(&path).unwrap();
    d.save(&wce_editor_core::document::SaveTarget { path: path.clone() })
        .unwrap();
    assert_eq!(
        std::fs::read(path.join("terrain.tile_list")).unwrap(),
        bytes
    );
    assert!(!d.history().modified());
    let before = state(&d);
    assert!(
        PlacementsPanel::edit_document(&mut d, "move a placement", false, |_, _| {
            panic!("unknown metadata")
        })
        .is_err()
    );
    assert_eq!(state(&d), before);
    std::fs::remove_file(path.join("terrain.tile_list")).unwrap();
    std::fs::remove_dir(&path).unwrap();
}

#[test]
fn review_missing_original_v9_save_failure_preserves_document_and_history() {
    let list = PlacementsPanel::list(&document(9, false)).unwrap().clone();
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!("wce-missing-v9-{}-{stamp}", std::process::id()));
    std::fs::create_dir(&path).unwrap();
    let source = path.join("terrain.tile_list");
    std::fs::write(&source, list.write()).unwrap();
    let model =
        wce_terrain_io::built::read("test", &wce_terrain_io::MapFiles::from_dir(&path).unwrap())
            .unwrap();
    let mut d = MapDocument::new(
        "test".to_owned(),
        MapSource::Packs("test".to_owned()),
        Some(model),
        Arc::new(PlacedMap::default()),
        None,
        Arc::new(TileDatabase::default()),
    );
    d.history_mut().mark_saved();
    let mut after = list.clone();
    after.version = 8;
    after.placements.pop();
    d.push_tile_list(TileListCommand {
        name: "bypassed downgrade".to_owned(),
        before: list,
        after,
        watch: d.cells.clone(),
    });
    std::fs::remove_file(&source).unwrap();
    let before = state(&d);
    let output = path.join("output");
    assert!(
        d.save(&wce_editor_core::document::SaveTarget {
            path: output.clone()
        })
        .is_err()
    );
    assert_eq!(state(&d), before);
    assert!(d.history().modified());
    assert_eq!(
        d.model()
            .unwrap()
            .tile_list
            .as_ref()
            .unwrap()
            .original_version(),
        Some(9)
    );
    assert!(!output.exists());
    std::fs::remove_dir(&path).unwrap();
}
