use super::*;

fn with_metadata(names: &[&[u8]], rows: u32) -> TileList {
    let mut t = paired();
    let mut tail = 4u32.to_le_bytes().to_vec();
    tail.extend((names.len() as u32).to_le_bytes());
    for name in names {
        tail.extend((name.len() as u16).to_le_bytes());
        tail.extend_from_slice(name);
    }
    tail.extend(rows.to_le_bytes());
    for row in 0..rows {
        tail.extend([row as u8; 32]);
    }
    t.tail = tail;
    t
}

#[test]
fn general_name_counts_and_zero_or_paired_rows_preserve_exact_trailers() {
    let names: &[&[u8]] = &[b"first", b"", &[0xff, 0x80, b'x']];
    for k in 0..=3 {
        for rows in [0, 3] {
            let original = with_metadata(&names[..k], rows);
            let bytes = original.write();
            let loaded = TileList::parse(&bytes).unwrap();
            assert_eq!(loaded.write(), bytes);
            assert_eq!(loaded.tail, original.tail);
            assert_eq!(loaded.v9_paired_metadata_count().unwrap(), Some(rows));
            let mut moved = loaded.clone();
            moved.placements[0].x += 1;
            assert!(moved.validate_placement_edit(&loaded).is_ok());
            for insert in [false, true] {
                let mut edited = loaded.clone();
                if insert {
                    edited.placements.push(edited.placements[0].clone());
                } else {
                    edited.placements.remove(1);
                }
                assert_eq!(
                    loaded.check_placement_edit("change count", true).is_ok(),
                    rows == 0
                );
                assert_eq!(edited.validate_placement_edit(&loaded).is_ok(), rows == 0);
                assert_eq!(edited.tail, loaded.tail);
                assert_eq!(TileList::parse(&edited.write()).unwrap().tail, loaded.tail);
            }
        }
    }
}

#[test]
fn general_names_reject_truncation_overflow_extra_bytes_and_inconsistent_rows() {
    for k in 0..=3 {
        let names: &[&[u8]] = &[b"a", b"bc", b"def"];
        let t = with_metadata(&names[..k], 3);
        for size in 4..t.tail.len() {
            let mut bad = t.clone();
            bad.tail.truncate(size);
            assert!(
                bad.v9_paired_metadata_count().is_err(),
                "K={k}, size={size}"
            );
            assert_eq!(TileList::parse(&bad.write()).unwrap().tail, bad.tail);
            assert!(bad.validate_placement_edit(&bad).is_ok());
        }
        let mut bad = t.clone();
        bad.tail.push(0);
        assert!(bad.check_placement_edit("move", false).is_err());
        let mut bad = t.clone();
        bad.tail[4..8].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(bad.v9_paired_metadata_count().is_err());
        let bad = with_metadata(&names[..k], 2);
        assert!(bad.check_placement_edit("move", false).is_err());
        let original = with_metadata(&names[..k], 0);
        let mut forged = original.clone();
        forged.tail = t.tail;
        assert!(forged.validate_placement_edit(&original).is_err());
    }
}

#[test]
#[ignore = "requires existing Rome II native witnesses via WCE_V9_WITNESSES"]
fn representative_native_v9_witnesses_preserve_bytes_and_apply_row_rules() {
    let paths = std::env::var_os("WCE_V9_WITNESSES").expect("set existing native witness paths");
    let mut seen = 0;
    for path in std::env::split_paths(&paths) {
        let bytes = std::fs::read(&path).unwrap();
        let t = TileList::parse(&bytes).unwrap();
        assert_eq!(t.version, 9);
        assert_eq!(t.write(), bytes);
        let rows = t
            .v9_paired_metadata_count()
            .unwrap()
            .expect("recognized native trailer");
        let mut edited = t.clone();
        edited.placements[0].x = edited.placements[0].x.wrapping_add(1);
        assert!(edited.validate_placement_edit(&t).is_ok());
        edited.placements.pop();
        assert_eq!(edited.validate_placement_edit(&t).is_ok(), rows == 0);
        assert_eq!(edited.tail, t.tail);
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
        println!(
            "{}: {} placements, {rows} rows; byte-exact and guard checks passed",
            path.display(),
            t.placements.len()
        );
        seen += 1;
    }
    assert!(seen > 0);
}

fn paired() -> TileList {
    let mut tail = vec![4, 0, 0, 0, 1, 0, 0, 0, 1, 0, b'x'];
    tail.extend(3u32.to_le_bytes());
    for value in [0x11, 0x22, 0x33] {
        tail.extend([value; 32]);
    }

    TileList {
        version: 9,
        names: vec!["tile".to_owned()],
        placements: (0..3)
            .map(|x| TileListPlacement {
                x,
                ..Default::default()
            })
            .collect(),
        tail,
        ..Default::default()
    }
}

#[test]
fn recognized_rows_round_trip_without_decoding_or_changing_them() {
    let t = paired();
    assert_eq!(t.v9_paired_metadata_count().unwrap(), Some(3));
    let b = t.write();
    let back = TileList::parse(&b).unwrap();
    assert_eq!(back.write(), b);
    assert_eq!(&back.tail[15..47], &[0x11; 32]);
    assert_eq!(&back.tail[47..79], &[0x22; 32]);
    assert_eq!(&back.tail[79..111], &[0x33; 32]);
    assert!(back.check_placement_edit("move a placement", false).is_ok());
}

#[test]
fn insertion_and_first_middle_last_removal_are_refused() {
    let original = paired();
    let bytes = original.write();
    for operation in ["place a tile", "remove a placement"] {
        let err = original.check_placement_edit(operation, true).unwrap_err();
        assert!(err.contains(operation) && err.contains("insertion/removal is unsupported"));
        assert_eq!(original.write(), bytes);
    }

    for i in 0..3 {
        let mut edited = original.clone();
        edited.placements.remove(i);
        assert!(edited.validate_placement_edit(&original).is_err());
        assert_eq!(edited.tail, original.tail);
    }

    let mut edited = original.clone();
    edited.placements.push(edited.placements[0].clone());
    assert!(edited.validate_placement_edit(&original).is_err());
}

#[test]
fn malformed_recognized_lengths_and_counts_are_checked_without_allocating_rows() {
    let t = paired();
    for n in [8, 9, 10, 11, 12, 13, 14, 15, 46, 110] {
        let mut bad = t.clone();
        bad.tail.truncate(n);
        assert!(bad.v9_paired_metadata_count().is_err(), "length {n}");
        assert!(bad.check_placement_edit("move a placement", false).is_err());
        // Reading/writing an opaque original remains lossless even if its trailer is malformed.
        let b = bad.write();
        assert_eq!(TileList::parse(&b).unwrap().write(), b);
    }

    for count in [0, 2, 4, u32::MAX] {
        let mut bad = t.clone();
        bad.tail[11..15].copy_from_slice(&count.to_le_bytes());
        assert!(bad.v9_paired_metadata_count().is_err(), "declared {count}");
    }

    let mut bad = t.clone();
    bad.tail[8..10].copy_from_slice(&u16::MAX.to_le_bytes());
    assert!(bad.v9_paired_metadata_count().is_err());
    let mut trailing = t.clone();
    trailing.tail.push(0);
    assert!(trailing.v9_paired_metadata_count().is_err());
}

#[test]
fn unknown_original_layout_is_lossless_but_placement_edits_are_not_assumed_safe() {
    let mut t = paired();
    t.tail = vec![9, 8, 7];
    assert_eq!(t.v9_paired_metadata_count().unwrap(), None);
    let b = t.write();
    assert_eq!(TileList::parse(&b).unwrap().write(), b);
    assert!(t.validate_placement_edit(&t).is_ok());
    assert!(
        t.check_placement_edit("remove a placement", true)
            .unwrap_err()
            .contains("unknown v9 metadata")
    );
    let mut moved = t.clone();
    moved.placements[0].x += 1;
    assert!(moved.validate_placement_edit(&t).is_err());
}

#[test]
fn recognized_same_count_edits_keep_rows_and_other_versions_keep_their_behavior() {
    let original = paired();
    let mut edited = original.clone();
    edited.placements[0].x += 4;
    edited.placements[1].flags = 0x20;
    edited.placements[2].climate = 1;
    assert!(edited.validate_placement_edit(&original).is_ok());
    assert_eq!(edited.tail, original.tail);
    edited.tail[15] ^= 1;
    assert!(edited.validate_placement_edit(&original).is_err());

    for version in [7, 8, 11] {
        let mut original = paired();
        original.version = version;
        let mut edited = original.clone();
        edited.placements.pop();
        assert!(
            original
                .check_placement_edit("remove a placement", true)
                .is_ok()
        );
        assert!(edited.validate_placement_edit(&original).is_ok());
        let b = edited.write();
        assert_eq!(TileList::parse(&b).unwrap().write(), b);
    }
}

#[test]
fn changing_version_or_forging_a_matching_trailer_does_not_bypass_the_guard() {
    let original = paired();
    let mut edited = original.clone();
    edited.version = 8;
    assert!(edited.validate_placement_edit(&original).is_err());
    let mut edited = original.clone();
    edited.placements.pop();
    edited.tail[11..15].copy_from_slice(&2u32.to_le_bytes());
    edited.tail.truncate(15 + 2 * 32);
    assert_eq!(edited.v9_paired_metadata_count().unwrap(), Some(2));
    assert!(edited.validate_placement_edit(&original).is_err());
}

#[test]
fn review_nan_no_op_with_unknown_or_truncated_original_trailer_is_exact() {
    for tail in [vec![9, 8, 7], vec![4, 0, 0, 0, 1, 0, 0, 0]] {
        let mut original = paired();
        original.lf_position = [f32::from_bits(0x7fc01234), -0.0];
        original.lf_scale = [f32::from_bits(0x7fc01234), -0.0];
        original.unknown[0] = f32::from_bits(0x7fc01234);
        original.origin_offset = [f32::from_bits(0x7fc01234), -0.0];
        original.tail = tail;
        let bytes = original.write();
        let loaded = TileList::parse(&bytes).unwrap();
        let unchanged = loaded.clone();
        assert_eq!(unchanged.unknown[0].to_bits(), 0x7fc01234);
        assert!(unchanged.validate_placement_edit(&loaded).is_ok());
        assert_eq!(unchanged.write(), bytes);
    }
}

#[test]
fn review_signed_zero_changes_do_not_use_the_unknown_layout_exemption() {
    for tail in [vec![9, 8, 7], vec![4, 0, 0, 0, 1, 0, 0, 0]] {
        let mut original = paired();
        original.tail = tail;
        for field in 0..4 {
            let mut edited = original.clone();
            match field {
                0 => edited.lf_position[0] = -0.0,
                1 => edited.lf_scale[0] = -0.0,
                2 => edited.unknown[0] = -0.0,
                _ => edited.origin_offset[0] = -0.0,
            }
            assert_ne!(edited.write(), original.write());
            assert!(edited.validate_placement_edit(&original).is_err());
            assert!(original.validate_placement_edit(&edited).is_err());
        }
    }
}

#[test]
fn review_nan_payload_changes_and_other_edits_cannot_hide_in_an_unchanged_nan() {
    let mut original = paired();
    original.unknown[0] = f32::from_bits(0x7fc01234);
    original.tail = vec![9, 8, 7];
    for change in 0..6 {
        let mut edited = original.clone();
        match change {
            0 => edited.unknown[0] = f32::from_bits(0x7fc01235),
            1 => edited.placements[0].old[0] ^= 1,
            2 => edited.tail.push(1),
            3 => {
                edited.placements.pop();
            }
            4 => edited.names[0].push('x'),
            _ => edited.bounds[0] += 1,
        }
        assert_ne!(edited.write(), original.write());
        assert!(edited.validate_placement_edit(&original).is_err());
    }
    let mut same = original.clone();
    same.placements[0].lf = [f32::from_bits(0x7fc01234), -0.0];
    assert!(same.same_bits(&same.clone()));
    let mut different = same.clone();
    different.placements[0].lf[1] = 0.0;
    assert!(!same.same_bits(&different));
}
