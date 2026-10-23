//! Attila's exported (binary) tile through the model and back, on a tile made here: CHMF heights, markers, a building list, tree and grass
//! lists (FASTBIN and the older format), XML with text and comments, an uncompressed DDS, files carried along. The packs' own tiles:
//! `pack_samples.rs` and the corpus gate `tile_binary_roundtrip_check`.

use std::collections::BTreeMap;
use std::sync::Arc;

use wce_formats::chmf::{ChmfFile, ChmfRegion, encode_region};
use wce_formats::markers::{KeyPart, MarkerGroup, MarkersFile};
use wce_tile_io::attila::TileBinary;
use wce_tile_io::attila::tile_binary::buildings;
use wce_tile_io::{PartRead, PartWritten, TileFiles, TileFormat, formats};
use wce_tile_model::{Blob, MapKind, Samples, Value};

fn chmf(w: u32, h: u32) -> Vec<u8> {
    let samples = (0..w * h).map(|i| ((i * 37) % 900 + (i / w) * 13) as u16).collect::<Vec<_>>();
    let (tag, offsets, data) = encode_region(w, h, &samples);
    ChmfFile { version: 5, width: w, height: h, cells_x: 1, cells_y: 1, min: -2.5, max: 27.5, spacing: 1, regions: vec![ChmfRegion { rect: [0, 0, w, h], tag, offsets, data }] }
        .write()
}

fn point_lights() -> Vec<u8> {
    let mut rec = vec![];
    for v in [10.0f32, 2.0, 30.0, 4.0, 1.0, 0.5, 0.25, 1.5] {
        rec.extend_from_slice(&v.to_le_bytes());
    }

    rec.push(1);
    rec.extend_from_slice(&8u16.to_le_bytes());
    rec.extend_from_slice(b"LAT_NONE");
    rec.extend_from_slice(&[0; 16]);
    rec.extend_from_slice(&12u16.to_le_bytes());
    rec.extend_from_slice(b"WPLFT_SMOOTH");
    MarkersFile {
        repository: "BASE_MARKER_REPOSITORY".into(), repository_id: 0x9D9BB, repository_u32: 1, repository_u16: 1, class_id: 0x00B7_E42E,
        class_name: "POINT_LIGHT_MARKER".into(), version: 3, records: vec![rec.clone(), rec],
        groups: Some(vec![MarkerGroup { key: vec![KeyPart::Wide("Point light".encode_utf16().collect())], indices: vec![0, 1] }]),
    }
    .write()
}

/// A building list written by the format itself from a document (the binary is checked against the kit's in the corpus gate).
fn building_list() -> Vec<u8> {
    use rpfm_lib::files::Encodeable;
    use rpfm_lib::files::esf::{BoolNode, Coordinates3DNode, ESF, ESFSignature, F32Node, NodeType, RecordNode, RecordNodeFlags, U32Node};
    let f = |v: f32| {
        let mut n = F32Node::default();
        n.set_value(v);
        n.set_optimized(v == 0.0);
        NodeType::F32(n)
    };

    let b = |v: bool| {
        let mut n = BoolNode::default();
        n.set_value(v);
        n.set_optimized(true);
        NodeType::Bool(n)
    };

    let v3 = |x: f32, y: f32, z: f32| {
        let mut c = Coordinates3DNode::default();
        c.set_x(x);
        c.set_y(y);
        c.set_z(z);
        NodeType::Coord3d(c)
    };

    let mut pt = U32Node::default();
    pt.set_value(2);
    pt.set_optimized(true);
    let building = vec![
        NodeType::Ascii("western_ahouse_1".into()), v3(741.2, 5.2, 428.9), f(0.0), f(0.2588), f(0.0), f(0.9659), v3(1.0, 1.0, 1.0), f(1.0),
        b(false), b(false), b(false), b(true), b(false), b(true), NodeType::U32(pt), b(false), b(false),
    ];

    let mut block = RecordNode::default();
    block.set_record_flags(RecordNodeFlags::IS_RECORD_NODE | RecordNodeFlags::HAS_NESTED_BLOCKS);
    block.set_name("BATTLEFIELD_BUILDING_LIST_BLOCK".into());
    block.set_children(vec![building.clone(), building]);
    let mut list = RecordNode::default();
    list.set_record_flags(RecordNodeFlags::IS_RECORD_NODE);
    list.set_version(12);
    list.set_name("BATTLEFIELD_BUILDING_LIST".into());
    list.set_children(vec![vec![NodeType::Record(Box::new(block))]]);
    let mut esf = ESF::default();
    esf.set_signature(ESFSignature::CAAB);
    esf.set_root_node(NodeType::Record(Box::new(list)));
    let mut out = vec![];
    esf.encode(&mut out, &None).unwrap();
    out
}

fn old_tree_list() -> Vec<u8> {
    let mut b = vec![];
    for v in [2u32, 1, 2] {
        b.extend_from_slice(&v.to_le_bytes());
    }

    b.extend_from_slice(&5u16.to_le_bytes());
    b.extend_from_slice(b"birch");
    b.extend_from_slice(&0u32.to_le_bytes());
    b.extend_from_slice(&[0x07, 0x55, 0x5a, 0xa0, 0x22, 0x5c, 0, 127, 0, 0x6b]);
    b.extend_from_slice(&[0x00, 0x3c, 0, 0, 0, 0x40, 0, 254, 1, 0]);
    b
}

fn ground_types() -> Vec<u8> {
    let mut header = vec![0u8; 128];
    header[..4].copy_from_slice(b"DDS ");
    let put = |h: &mut Vec<u8>, at: usize, v: u32| h[at..at + 4].copy_from_slice(&v.to_le_bytes());
    put(&mut header, 4, 124);
    put(&mut header, 12, 2);
    put(&mut header, 16, 4);
    put(&mut header, 76, 32);
    put(&mut header, 80, 0x20000);
    put(&mut header, 88, 8);
    [header, (0..8).collect()].concat()
}

const ZONES: &str = "<BATTLEFIELD_ZONE_TEMPLATE_LIST>\r\n<BATTLEFIELD_ZONE_TEMPLATE>\r\n<zone_name>ZONE_POLYGON_INSTANCE</zone_name>\r\n<!-- a comment -->\r\n<position x='282.091217' y='618.072388'/><position x='282.5' y='547'/>\r\n</BATTLEFIELD_ZONE_TEMPLATE>\r\n</BATTLEFIELD_ZONE_TEMPLATE_LIST>\r\n";

fn files() -> TileFiles {
    let mut folder = BTreeMap::new();
    let mut put = |name: &str, bytes: Vec<u8>| {
        folder.insert(name.to_owned(), Blob::Bytes(bytes.into()));
    };

    put("hf_height_map.data", chmf(33, 17));
    put("point_light_marker.markers", point_lights());
    put("building_list", building_list());
    put("rome_alpine.tree_list", old_tree_list());
    put("ground_types.dds", ground_types());
    put("zones.xml", ZONES.as_bytes().to_vec());
    put("mesh.rigid_model_v2", b"RMV2 not read".to_vec());
    put("tile.ted_binary", vec![0; 76]);
    TileFiles { folder, entry: None, variation: None }
}

fn round_trip(input: &TileFiles) -> TileFiles {
    let (tile, report) = TileBinary.read(input).unwrap();
    assert!(report.parts.iter().all(|p| p.1 == PartRead::Decoded), "{:?}", report.parts);
    let (out, _) = TileBinary.write(&tile).unwrap();
    assert_eq!(out.folder.keys().collect::<Vec<_>>(), input.folder.keys().collect::<Vec<_>>());
    for (name, blob) in &input.folder {
        assert_eq!(out.folder[name].bytes().unwrap(), blob.bytes().unwrap(), "{name}");
    }

    let (back, _) = TileBinary.read(&out).unwrap();
    assert!(back.semantic() == tile.semantic());
    out
}

#[test]
fn every_part_round_trips() {
    round_trip(&files());
}

#[test]
fn what_the_model_holds() {
    let (tile, _) = TileBinary.read(&files()).unwrap();
    assert_eq!(tile.source_format, Some("attila.tile_binary"));
    let h = tile.map(MapKind::Height, 0).unwrap();
    assert_eq!((h.raster.width, h.raster.height), (33, 17));
    let chmf = tile.document("hf_height_map.data").unwrap();
    assert_eq!(chmf.root.get("min"), Some(&Value::Float(-2.5)));
    let lights = tile.document("point_light_marker.markers").unwrap();
    assert_eq!(lights.root.class, "POINT_LIGHT_MARKER");
    assert_eq!(lights.root.children[0].get("radius"), Some(&Value::Float(4.0)));
    assert_eq!(lights.root.children[0].get("animation_type"), Some(&Value::Str("LAT_NONE".into())));
    let list = buildings(tile.document("building_list").unwrap()).unwrap();
    assert_eq!(list.len(), 2);
    assert_eq!(list[0].key, "western_ahouse_1");
    let trees = &tile.document("rome_alpine.tree_list").unwrap().root;
    assert_eq!(trees.child("records").unwrap().get("position").and_then(Value::as_floats).unwrap()[3..6], [1.0, 0.0, 2.0]);
    assert_eq!(tile.map(MapKind::GroundType, 0).unwrap().raster.samples, Samples::U8((0..8).collect()));
    let zones = &tile.document("zones.xml").unwrap().root;
    assert_eq!(zones.children[0].children[0].children[0].get("text"), Some(&Value::Str("ZONE_POLYGON_INSTANCE".into())));
    assert_eq!(tile.files.iter().map(|f| f.path.as_str()).collect::<Vec<_>>(), ["mesh.rigid_model_v2", "tile.ted_binary"]);
}

#[test]
fn edits_are_encoded() {
    let (mut tile, _) = TileBinary.read(&files()).unwrap();
    let Samples::U16(s) = &mut tile.map_mut(MapKind::Height, 0).unwrap().raster.samples else { panic!() };
    s[40] = 4321;
    let b = &mut tile.document_mut("building_list").unwrap().root.children[1];
    b.set("position", Value::Vec3([10.0, 2.0, -3.5]));
    let (out, report) = TileBinary.write(&tile).unwrap();
    assert!(report.parts.iter().all(|p| p.1 != PartWritten::Kept));
    let (back, _) = TileBinary.read(&out).unwrap();
    let Samples::U16(s) = &back.map(MapKind::Height, 0).unwrap().raster.samples else { panic!() };
    assert_eq!(s[40], 4321);
    assert_eq!(buildings(back.document("building_list").unwrap()).unwrap()[1].position, [10.0, 2.0, -3.5]);
}

#[test]
fn a_short_block_table_is_kept_until_edited() {
    // A file with fewer blocks than its region (as TDD's campaign tiles): not the engine's encoder.
    let mut f = ChmfFile::parse(&chmf(40, 40)).unwrap();
    f.regions[0].offsets.truncate(2);
    let first_two = f.regions[0].offsets[1] as usize;
    let end = {
        let mut probe = vec![];
        wce_formats::chmf::decode_block(&f.regions[0].data[first_two..], 256, &mut probe).unwrap();
        // The second block's length: re-encode it.
        let mut block = [0u16; 256];
        block.copy_from_slice(&probe);
        let mut enc = vec![];
        wce_formats::chmf::encode_block(&block, &mut enc);
        first_two + enc.len()
    };

    f.regions[0].data.truncate(end);
    let bytes = f.write();
    let mut folder = BTreeMap::new();
    folder.insert("hf_height_map.data".to_owned(), Blob::Bytes(Arc::from(bytes.as_slice())));
    let input = TileFiles { folder, ..Default::default() };
    let (mut tile, _) = TileBinary.read(&input).unwrap();
    let (out, report) = TileBinary.write(&tile).unwrap();
    assert_eq!(report.parts[0].1, PartWritten::Kept);
    assert_eq!(out.folder["hf_height_map.data"].bytes().unwrap(), Arc::from(bytes.as_slice()));
    let Samples::U16(s) = &mut tile.map_mut(MapKind::Height, 0).unwrap().raster.samples else { panic!() };
    s[0] = 7;
    let (out, report) = TileBinary.write(&tile).unwrap();
    assert_eq!(report.parts[0].1, PartWritten::Encoded);
    assert_eq!(ChmfFile::parse(&out.folder["hf_height_map.data"].bytes().unwrap()).unwrap().regions[0].offsets.len(), 9);
}

#[test]
fn registry() {
    assert!(formats().iter().any(|f| f.id() == "attila.tile_binary"));
    let mut files = files();
    files.folder.insert("hf_height_map.data".into(), Blob::Bytes(Arc::from(&b"CHMF"[..])));
    assert!(TileBinary.claims(&files));
    files.folder.insert("tile.ted".into(), Blob::Bytes(Arc::from(&b"<project/>"[..])));
    assert!(!TileBinary.claims(&files));
}
