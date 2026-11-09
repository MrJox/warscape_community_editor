//! Independent byte fixtures exercise the public codec and both direct production callers.
// E01--E04 retain the accepted encoding controls at the current native writer boundary.
#[test] fn p004_e01_non_ascii_name_inverse() { inverse(&fixture(7, &[(b"\xe9", 1)], b"tail")); }
#[test] fn p004_e02_ascii_name_inverse() { for c in b'a'..=b'z' { inverse(&fixture(7, &[(&[c], 1)], b"tail")); } }
fn built_name_error(name: &str) {
    let (_, b) = bad_scene("land_normal");
    let f = wce_terrain_io::MapFiles::from_bytes([("locations.blm".into(), b.write().unwrap())]);
    let mut m = wce_terrain_io::built::read("codec", &f).unwrap();
    let item = m.project.as_mut().unwrap();
    let id = item.get().entities[0].id;
    item.edit().set_attr(id, "ECBattleCatchmentArea", "catchment_type", name).unwrap();
    let before = item.get().save();
    assert!(wce_terrain_io::built::write(&m, None).is_err());
    assert_eq!(m.project.as_ref().unwrap().get().save(), before);
}
#[test] fn p004_e03_unrepresentable_edit_rejected() { built_name_error("\u{100}"); }
#[test] fn p004_e04_oversized_name_rejected() { built_name_error(&"a".repeat(65536)); }
use std::collections::BTreeMap;
use wce_tile::battle_map::{locations::{BattleLocations, Location}, tile_db::TileDatabase};
use wce_terrain::{Item, LayerRole, MapKind, MapModel, cells::CellLayer};
use wce_terrain_io::{MapIoError, attila::build::{build, BuildInputs}, built::scene};
use wce_tile_model::Blob;

const RECORD: [i32; 9] = [i32::MIN, -2, 3, i32::MAX, 1, -1, -65536, 0x12345678, -314159];
fn fixture(version: u32, groups: &[(&[u8], usize)], tail: &[u8]) -> Vec<u8> {
    let mut b = b"BLMF".to_vec();
    for v in [version, u32::try_from(groups.len()).unwrap(), 123, 456] { b.extend(v.to_le_bytes()); }
    for (name, records) in groups {
        b.extend(u16::try_from(name.len()).unwrap().to_le_bytes()); b.extend_from_slice(name);
        b.extend(u32::try_from(*records).unwrap().to_le_bytes());
        for _ in 0..*records { for v in RECORD { b.extend(v.to_le_bytes()); } }
    }
    b.extend_from_slice(tail); b
}
fn inverse(bytes: &[u8]) -> BattleLocations {
    let parsed = BattleLocations::parse(bytes).unwrap();
    assert_eq!(parsed.write().unwrap(), bytes);
    assert_eq!(BattleLocations::parse(&parsed.write().unwrap()).unwrap(), parsed);
    parsed
}
#[test] fn all_256_native_bytes_are_exact_inverses() {
    let name: Vec<u8> = (0..=255).collect();
    let b = fixture(7, &[(&name, 1)], b"\0\xfftail"); let parsed = inverse(&b);
    assert_eq!(parsed.types[0].0.chars().map(u32::from).collect::<Vec<_>>(), (0..=255).collect::<Vec<_>>());
    assert_eq!(u16::from_le_bytes(parsed.write().unwrap()[20..22].try_into().unwrap()), 256);
}
#[test] fn mixed_names_do_not_shift_following_groups_or_records() {
    let groups: &[(&[u8], usize)] = &[(b"ASCII", 2), (b"a\x7f\x80\xe9\xffZ", 3), (b"following", 1)];
    let parsed = inverse(&fixture(7, groups, b"opaque table\0\xff"));
    assert_eq!(parsed.types.iter().map(|(_, r)| r.len()).collect::<Vec<_>>(), vec![2, 3, 1]);
}
#[test] fn versions_have_identical_name_rules_without_scene_assumptions() {
    for version in [0, 6, 7, 8, 999, u32::MAX] { inverse(&fixture(version, &[(b"\xff\xe9", 1)], b"opaque")); }
}
#[test] fn empty_and_repeated_groups_with_records_remain_in_order() {
    let p = inverse(&fixture(7, &[(b"", 0), (b"repeat", 1), (b"", 2), (b"repeat", 0)], &[1, 2, 0, 255]));
    assert_eq!(p.types.len(), 4); assert_eq!(p.types[1].0, p.types[3].0);
    assert_eq!(p.types[2].0, ""); assert_eq!(p.types[2].1.len(), 2);
}
#[test] fn zero_groups_and_zero_length_name_remain_distinct() {
    assert!(inverse(&fixture(7, &[], b"tail")).types.is_empty());
    assert_eq!(inverse(&fixture(7, &[(b"", 0)], b"tail")).types.len(), 1);
}
#[test] fn exact_u16_capacity_accepts_high_bytes_despite_utf8_size() {
    for byte in [b'A', 0xe9] {
        let name = vec![byte; usize::from(u16::MAX)];
        let p = inverse(&fixture(7, &[(&name, 1)], b"tail"));
        assert_eq!(p.types[0].0.chars().count(), 65535);
        if byte == 0xe9 { assert_eq!(p.types[0].0.len(), 131070); }
        assert_eq!(&p.write().unwrap()[20..22], &u16::MAX.to_le_bytes());
    }
}
#[test] fn first_oversized_length_errors_without_panic_or_bytes() {
    for c in ['A', '\u{e9}'] {
        let b = BattleLocations { types: vec![(c.to_string().repeat(65536), vec![])], ..Default::default() };
        let result = std::panic::catch_unwind(|| b.write());
        assert!(matches!(result, Ok(Err(e)) if e.contains("65535")));
    }
}
#[test] fn unsupported_unicode_in_later_group_returns_only_error() {
    for c in ['\u{100}', '\u{20ac}', '\u{1f600}', '\u{10ffff}'] {
        let mut p = inverse(&fixture(7, &[(b"valid", 1), (b"later", 1)], b"tail"));
        p.types[1].0.push(c);
        assert!(matches!(std::panic::catch_unwind(|| p.write()), Ok(Err(e)) if e.contains("type 1") && e.contains("single-byte")));
    }
}
#[test] fn opaque_fields_header_and_tail_remain_byte_identical() {
    let b = fixture(8, &[(b"\x80\xe9\xff", 2)], b"\xff\0opaque native suffix\xfe");
    let p = inverse(&b); let l = &p.types[0].1[0];
    assert_eq!(l, &Location { rect: RECORD[..4].try_into().unwrap(), centre: RECORD[4..6].try_into().unwrap(), id: RECORD[6], extra: RECORD[7..].try_into().unwrap() });
    assert_eq!((p.version, p.width, p.height), (8, 123, 456));
}
#[test] fn malformed_name_prefixes_and_truncations_are_rejected() {
    let b = fixture(7, &[(b"A\xe9", 1)], b"");
    for end in 0..b.len() { assert!(BattleLocations::parse(&b[..end]).is_err(), "prefix length {end}"); }
    let mut bad = b.clone(); bad[20..22].copy_from_slice(&u16::MAX.to_le_bytes());
    assert!(BattleLocations::parse(&bad).is_err());
    let mut missing = fixture(7, &[(b"", 0)], b""); missing.truncate(23);
    assert!(BattleLocations::parse(&missing).is_err());
}
fn bad_scene(name: &str) -> (wce_terrain::terry::TerryProject, BattleLocations) {
    let mut b = BattleLocations::parse(&fixture(7, &[(b"land_normal", 1)], b"tail")).unwrap();
    b.types[0].1[0] = Location { rect: [3, 4, 5, 6], centre: [4, 5], id: -1, extra: [0, 0] };
    let mut p = scene::scene(&b, &BTreeMap::new()).unwrap(); let id = p.entities[0].id;
    p.set_attr(id, "ECBattleCatchmentArea", "catchment_type", name).unwrap(); (p, b)
}
#[test] fn scene_candidate_propagates_checked_codec_errors_as_native_io_errors() {
    for name in ["\u{100}".to_owned(), "A".repeat(65536)] {
        let (p, b) = bad_scene(&name);
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| scene::write(&p, &b)));
        assert!(matches!(result, Ok(Err(MapIoError::File { file, what })) if file == "locations.blm" && what.contains("locations.blm")));
    }
}
#[test] fn attila_builder_propagates_codec_error_without_returning_partial_files() {
    for name in ["\u{100}".to_owned(), "A".repeat(65536)] {
        let (p, _) = bad_scene(&name);
        let empty = || Blob::Bytes(Vec::<u8>::new().into());
        let mut m = MapModel { kind: MapKind::Battle, project: Some(Item::new("test.terry", p, empty())), ..Default::default() };
        m.layers.push((LayerRole::TileMap, Item::new("tile_map.png", CellLayer::filled(16, 16, false, [0, 0, 0, 255]), empty())));
        let db = TileDatabase::default(); let inputs = BuildInputs { db: &db, environment_map: &[], tile_upgrades: &[] };
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| build(&m, &inputs)));
        assert!(matches!(result, Ok(Err(MapIoError::File { file, what })) if file == "locations.blm" && what.contains("locations.blm")));
    }
}
