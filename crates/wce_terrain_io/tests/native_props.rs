use std::sync::Arc;
use wce_formats::markers::{KeyPart, MarkerGroup, MarkersFile};
use wce_terrain::campaign_props::CampaignProps;
use wce_terrain_io::{MapFiles, built};
use wce_tile::battle_map::tile_list::TileList;
use wce_tile_model::Blob;

fn files() -> MapFiles {
    let mut record = vec![0; 69];
    for i in [0, 5, 10, 15] {
        record[i * 4..i * 4 + 4].copy_from_slice(&1f32.to_le_bytes());
    }
    let f = MarkersFile {
        repository: "BASE_MARKER_REPOSITORY".into(),
        repository_id: 645563,
        repository_u32: 1,
        repository_u16: 1,
        class_id: 645566,
        class_name: "PROP_MARKER".into(),
        version: 4,
        records: vec![record],
        groups: Some(vec![MarkerGroup {
            key: vec![KeyPart::Wide("model".encode_utf16().collect())],
            indices: vec![0],
        }]),
    };
    MapFiles::from_bytes([
        ("prop_marker.markers".into(), f.write()),
        (
            "terrain.tile_list".into(),
            TileList {
                version: 9,
                is_campaign: true,
                ..Default::default()
            }
            .write(),
        ),
        ("lf_normal.dds".into(), b"opaque normal".to_vec()),
        ("unrelated.bin".into(), vec![0, 255, 13, 10]),
    ])
}

#[test]
fn native_props_only_resource_changes_and_reopen_undo_are_exact() {
    let f = files();
    let mut m = built::read("campaign", &f).unwrap();
    assert!(built::changed(&f, &built::write(&m, None).unwrap()).is_empty());
    m.native_props
        .as_mut()
        .unwrap()
        .edit()
        .translate(0, [1., 0., 2.])
        .unwrap();
    let written = built::write(&m, None).unwrap();
    let changes = built::changed(&f, &written);
    assert_eq!(changes.len(), 1);
    assert_eq!(changes[0].0, "prop_marker.markers");
    let reopened = built::read("campaign", &written).unwrap();
    assert_eq!(
        reopened.native_props.unwrap().get().position(0).unwrap(),
        [1., 0., 2.]
    );
    m.native_props
        .as_mut()
        .unwrap()
        .edit()
        .translate(0, [0.; 3])
        .unwrap();
    assert!(built::changed(&f, &built::write(&m, None).unwrap()).is_empty());
    let bytes = f.folder["prop_marker.markers"].bytes().unwrap();
    assert_eq!(
        CampaignProps::parse(Arc::from(bytes.as_ref()))
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn source_association_rejection_keeps_the_captured_model() {
    for case in 0..4 {
        let f = files();
        let mut m = built::read("campaign", &f).unwrap();
        let captured = m.native_props.as_ref().unwrap().get().write();
        match case {
            0 => m.built = false,
            1 => m.kind = wce_terrain::MapKind::Battle,
            2 => {
                m.carried.remove("prop_marker.markers");
            }
            _ => {
                m.native_props.as_mut().unwrap().origin =
                    Blob::Bytes(Arc::from(&b"replaced source"[..]))
            }
        }
        assert!(built::write(&m, None).is_err());
        assert_eq!(m.native_props.as_ref().unwrap().get().write(), captured);
    }
}

#[test]
fn unsupported_mapping_and_ambiguous_resource_keys_remain_opaque() {
    for duplicate_key in [false, true] {
        let mut f = files();
        if duplicate_key {
            f.folder.insert(
                "PROP_MARKER.MARKERS".into(),
                f.folder["prop_marker.markers"].clone(),
            );
        } else {
            let mut marker =
                MarkersFile::parse(&f.folder["prop_marker.markers"].bytes().unwrap()).unwrap();
            marker.groups = None;
            f.folder.insert(
                "prop_marker.markers".into(),
                Blob::Bytes(Arc::from(marker.write())),
            );
        }
        let m = built::read("campaign", &f).unwrap();
        assert!(m.native_props.is_none());
        assert!(built::changed(&f, &built::write(&m, None).unwrap()).is_empty());
    }
}
