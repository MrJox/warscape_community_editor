use std::sync::Arc;
use wce_formats::markers::{KeyPart, MarkerGroup, MarkersFile};
use wce_terrain::campaign_props::CampaignProps;
use wce_terrain_io::{MapFiles, built};
use wce_tile::battle_map::tile_list::TileList;

fn files() -> MapFiles {
    let mut record = vec![0;69];
    for i in [0,5,10,15] { record[i*4..i*4+4].copy_from_slice(&1f32.to_le_bytes()); }
    let f = MarkersFile { repository:"BASE_MARKER_REPOSITORY".into(),repository_id:645563,
        repository_u32:1,repository_u16:1,class_id:645566,class_name:"PROP_MARKER".into(),version:4,
        records:vec![record],groups:Some(vec![MarkerGroup {key:vec![KeyPart::Wide("model".encode_utf16().collect())],indices:vec![0]}]) };
    MapFiles::from_bytes([
        ("prop_marker.markers".into(), f.write()),
        ("terrain.tile_list".into(),TileList {version:9,is_campaign:true,..Default::default()}.write()),
        ("lf_normal.dds".into(),b"opaque normal".to_vec()),
        ("unrelated.bin".into(),vec![0,255,13,10]),
    ])
}

#[test]
fn native_props_only_resource_changes_and_reopen_undo_are_exact() {
    let f=files(); let mut m=built::read("campaign",&f).unwrap();
    assert!(built::changed(&f,&built::write(&m,None).unwrap()).is_empty());
    m.native_props.as_mut().unwrap().edit().translate(0,[1.,0.,2.]).unwrap();
    let written=built::write(&m,None).unwrap();
    let changes=built::changed(&f,&written);
    assert_eq!(changes.len(),1); assert_eq!(changes[0].0,"prop_marker.markers");
    let reopened=built::read("campaign",&written).unwrap();
    assert_eq!(reopened.native_props.unwrap().get().position(0).unwrap(),[1.,0.,2.]);
    m.native_props.as_mut().unwrap().edit().translate(0,[0.;3]).unwrap();
    assert!(built::changed(&f,&built::write(&m,None).unwrap()).is_empty());
    let bytes=f.folder["prop_marker.markers"].bytes().unwrap();
    assert_eq!(CampaignProps::parse(Arc::from(bytes.as_ref())).unwrap().len(),1);
}
