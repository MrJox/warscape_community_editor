use super::*;
use wce_tile::battle_map::tile_list::{TileList, TileListPlacement};

fn files(version: u32, tail: Vec<u8>) -> MapFiles {
    let list = TileList { version, names: vec!["tile".to_owned()], placements: (0..3).map(|x| TileListPlacement { x, ..Default::default() }).collect(), tail, ..Default::default() };
    MapFiles::from_bytes([(LIST.to_owned(), list.write()), ("unknown.bin".to_owned(), vec![7, 8, 9])])
}

fn paired_tail() -> Vec<u8> {
    let mut tail = vec![4, 0, 0, 0, 1, 0, 0, 0, 0, 0];
    tail.extend(3u32.to_le_bytes());
    for value in [0x11, 0x22, 0x33] {
        tail.extend([value; 32]);
    }
    tail
}

#[test]
fn originals_including_unknown_and_malformed_v9_layouts_are_exact() {
    for tail in [paired_tail(), vec![7, 8, 9], vec![4, 0, 0, 0, 1, 0, 0, 0]] {
        let files = files(9, tail);
        let mut model = read("campaigns/test", &files).unwrap();
        assert!(changed(&files, &write(&model, None).unwrap()).is_empty());
        // An edit flag alone is not a changed opaque resource (for example undo-to-original).
        model.tile_list.as_mut().unwrap().edit();
        assert!(changed(&files, &write(&model, None).unwrap()).is_empty());
    }
}

#[test]
fn writer_independently_rejects_bypassed_insertions_and_all_removal_positions() {
    let files = files(9, paired_tail());
    for i in 0..3 {
        let mut model = read("campaigns/test", &files).unwrap();
        model.tile_list.as_mut().unwrap().edit().placements.remove(i);
        let before = model.tile_list.as_ref().unwrap().get().clone();
        let err = write(&model, None).unwrap_err().to_string();
        assert!(err.contains(LIST) && err.contains("count"));
        assert_eq!(model.tile_list.as_ref().unwrap().get(), &before);
    }

    let mut model = read("campaigns/test", &files).unwrap();
    let list = model.tile_list.as_mut().unwrap().edit();
    list.placements.push(list.placements[0].clone());
    assert!(write(&model, None).is_err());
    let list = model.tile_list.as_mut().unwrap().edit();
    list.version = 8;
    assert!(write(&model, None).is_err(), "a downgraded version cannot bypass original v9 validation");
}

#[test]
fn writer_rejects_changed_unknown_or_malformed_metadata() {
    for tail in [vec![7, 8, 9], vec![4, 0, 0, 0, 1, 0, 0, 0]] {
        let files = files(9, tail);
        let mut model = read("campaigns/test", &files).unwrap();
        model.tile_list.as_mut().unwrap().edit().placements[0].x += 1;
        assert!(write(&model, None).is_err());
    }

    let files = files(9, paired_tail());
    let mut model = read("campaigns/test", &files).unwrap();
    model.tile_list.as_mut().unwrap().edit().tail[14] ^= 1;
    assert!(write(&model, None).is_err());
}

#[test]
fn writer_keeps_recognized_same_count_edits_and_other_versions() {
    let files = files(9, paired_tail());
    let mut model = read("campaigns/test", &files).unwrap();
    let original_tail = model.tile_list.as_ref().unwrap().get().tail.clone();
    model.tile_list.as_mut().unwrap().edit().placements[1].x += 1;
    let out = write(&model, None).unwrap();
    let back = TileList::parse(&out.folder[LIST].bytes().unwrap()).unwrap();
    assert_eq!(back.tail, original_tail);
    assert_eq!(changed(&files, &out).len(), 1);

    for version in [7, 8, 11] {
        let files = self::files(version, vec![]);
        let mut model = read("test", &files).unwrap();
        model.tile_list.as_mut().unwrap().edit().placements.pop();
        let out = write(&model, None).unwrap();
        assert_eq!(TileList::parse(&out.folder[LIST].bytes().unwrap()).unwrap().placements.len(), 2);
    }
}

#[test]
fn review_unavailable_non_v9_original_does_not_prevent_edited_write() {
    for version in [7, 8, 11] {
        let source = files(version, vec![]).folder[LIST].bytes().unwrap();
        let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let dir = std::env::temp_dir().join(format!("wce-list-source-{}-{version}-{stamp}", std::process::id()));
        std::fs::create_dir(&dir).unwrap();
        let path = dir.join(LIST);
        std::fs::write(&path, &source).unwrap();
        let input = MapFiles::from_dir(&dir).unwrap();
        let mut model = read("test", &input).unwrap();
        assert_eq!(model.tile_list.as_ref().unwrap().original_version(), Some(version));
        model.tile_list.as_mut().unwrap().edit().placements.pop();
        std::fs::remove_file(&path).unwrap();
        let result = write(&model, None);
        std::fs::remove_dir(&dir).unwrap();
        let output = result.unwrap();
        assert_eq!(TileList::parse(&output.folder[LIST].bytes().unwrap()).unwrap().placements.len(), 2);
    }
}

#[test]
fn review_edited_flag_only_nan_original_is_byte_exact_and_real_changes_are_rejected() {
    for tail in [vec![9, 8, 7], vec![4, 0, 0, 0, 1, 0, 0, 0]] {
        let input = files(9, tail);
        let mut list = TileList::parse(&input.folder[LIST].bytes().unwrap()).unwrap();
        list.unknown = [f32::from_bits(0x7fc01234), -0.0];
        let mut bytes = list.write();
        // Preserve a source byte that the existing string codec would normalize on reserialization.
        let name = bytes.windows(4).position(|s| s == b"tile").unwrap();
        bytes[name] = 0xe9;
        let input = MapFiles::from_bytes([(LIST.to_owned(), bytes.clone())]);
        let mut model = read("test", &input).unwrap();
        model.tile_list.as_mut().unwrap().edit();
        let written = write(&model, None).unwrap();
        assert_eq!(&*written.folder[LIST].bytes().unwrap(), &bytes);
        assert_eq!(TileList::parse(&written.folder[LIST].bytes().unwrap()).unwrap().unknown[0].to_bits(), 0x7fc01234);
        model.tile_list.as_mut().unwrap().edit().unknown[1] = 0.0;
        assert!(write(&model, None).is_err());
        model.tile_list.as_mut().unwrap().edit().unknown[1] = -0.0;
        model.tile_list.as_mut().unwrap().edit().unknown[0] = f32::from_bits(0x7fc01235);
        assert!(write(&model, None).is_err());
    }
}

#[test]
fn review_original_v9_requires_evidence_even_after_downgrade_and_clone() {
    let source = files(9, paired_tail()).folder[LIST].bytes().unwrap();
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
    let dir = std::env::temp_dir().join(format!("wce-v9-source-{}-{stamp}", std::process::id()));
    std::fs::create_dir(&dir).unwrap();
    let path = dir.join(LIST);
    std::fs::write(&path, &source).unwrap();
    let mut model = read("test", &MapFiles::from_dir(&dir).unwrap()).unwrap();
    model.tile_list.as_mut().unwrap().edit().placements.pop();
    std::fs::remove_file(&path).unwrap();
    for version in [9, 7, 8, 11] {
        model.tile_list.as_mut().unwrap().edit().version = version;
        let cloned = model.clone();
        assert_eq!(cloned.tile_list.as_ref().unwrap().original_version(), Some(9));
        let before = cloned.tile_list.as_ref().unwrap().get().write();
        assert!(write(&cloned, None).is_err());
        assert_eq!(cloned.tile_list.as_ref().unwrap().get().write(), before);
    }
    // A changed source header also cannot erase the loaded v9 provenance.
    let replacement = files(8, vec![]).folder[LIST].bytes().unwrap();
    std::fs::write(&path, &replacement).unwrap();
    model.tile_list.as_mut().unwrap().edit().version = 8;
    assert!(write(&model, None).unwrap_err().to_string().contains("loaded provenance"));
    std::fs::remove_file(&path).unwrap();
    std::fs::remove_dir(&dir).unwrap();
}

#[test]
fn review_missing_provenance_does_not_trust_a_mutable_version() {
    let input = files(9, paired_tail());
    let mut model = read("test", &input).unwrap();
    let original = model.tile_list.take().unwrap();
    let mut edited = original.get().clone();
    edited.version = 8;
    // Older/manual constructors without captured provenance retain a conservative evidence check.
    let mut item = Item::new(LIST, edited, original.origin);
    item.edit();
    model.tile_list = Some(item);
    assert!(write(&model, None).is_err());
}
