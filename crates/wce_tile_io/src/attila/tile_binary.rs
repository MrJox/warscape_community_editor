//! Attila's exported (binary) tile: a variation folder as BOB writes it into `working_data` and the packs ship it
//! (`terrain/tiles/<kind>/<set>/.../<variation>/`). docs/formats/tile_binary.md lists the files.
//!
//! Into the model:
//! - `hf_height_map.data` / `hf_water_map.data` (CHMF): a document (`CHMF`: the header, a `region` child per region with its rectangle and
//!   its two u16) and one raster per region (`Height` / `Water`, indexed by region). Written with the engine's encoder; a file whose block
//!   table is shorter than its regions (not the engine's encoder) is written from its original bytes while its samples are unchanged.
//! - `*.markers`: a document per file (the class as the root, a `record` child per record with the layout's named fields, a `groups` child).
//! - building lists (`building_list*`, `far_building_list`): a `BATTLEFIELD_BUILDING_LIST` document, a `building` child per building.
//! - tree and grass lists (FASTBIN `.bin` and the older format): documents with one child per type and the records as columns.
//! - the BMD binaries, the civilian lists, the tile database and other ESF files: documents through [`crate::serde_node`] (rpfm_lib's
//!   structs).
//! - `blend.dds`, `index.dds`, `normal.dds` (DXT5): rasters (`BlendTexture`, `IndexTexture`, `NormalTexture`) decoded to RGBA8 and written
//!   from their original bytes while unchanged (the compressor is not reproduced yet); `ground_types.dds` (uncompressed): a `GroundType`
//!   raster, written from its pixels.
//! - every XML file: a document through the XML codec (values as text).
//! - everything else (terrain meshes, the loading screen, `tile.ted_binary`, a file that did not read): carried along untouched.

use std::io::Cursor;
use std::sync::Arc;

use rpfm_lib::files::bmd::Bmd;
use rpfm_lib::files::esf::{Coordinates3DNode, ESF, ESFSignature, F32Node, NodeType, RecordNode, RecordNodeFlags, BoolNode, U32Node};
use rpfm_lib::files::fastbin_lists::{CivilianDeploymentFile, CivilianShelterFile, GrassListFile, TreeListFile};
use rpfm_lib::files::tile_database::TileDatabase;
use rpfm_lib::files::{Decodeable, Encodeable};
use serde::Serialize;
use serde::de::DeserializeOwned;
use wce_formats::chmf::{ChmfFile, ChmfRegion, encode_region};
use wce_formats::dds_decode::dds_decode_mip;
use wce_formats::image_containers::DdsFile;
use wce_formats::markers::{self, KeyPart, MarkerGroup, MarkersFile, record_from_fields};
use wce_formats::old_lists::{OldList, OldListKind};
use wce_tile_model::{Blob, Document, MapKind, Node, OpaqueFile, Raster, Samples, Sym, Tile, TileMap, Value};

use super::building_list::{self, BattlefieldBuilding};
use crate::xml::values::{Kind, Schema};
use crate::xml::{self, DocStyle};
use crate::{PartRead, PartWritten, ReadReport, Result, TileFiles, TileFormat, TileIoError, WriteReport, serde_node};

pub struct TileBinary;

pub const ID: &str = "attila.tile_binary";

/// What a file of the folder is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Part {
    Chmf(MapKind),
    Markers,
    BuildingList,
    Esf,
    TreeListBin,
    GrassListBin,
    OldList(OldListKind),
    Bmd,
    CivilianDeployment,
    CivilianShelter,
    TileDatabase,
    Dds(MapKind),
    Xml,
}

fn classify(name: &str, head: &[u8]) -> Option<Part> {
    let lower = name.to_lowercase();
    let fastbin = head.starts_with(b"FASTBIN0");
    Some(match () {
        _ if head.starts_with(b"CHMF") && lower == "hf_height_map.data" => Part::Chmf(MapKind::Height),
        _ if head.starts_with(b"CHMF") && lower == "hf_water_map.data" => Part::Chmf(MapKind::Water),
        _ if lower.ends_with(".markers") => Part::Markers,
        _ if head.starts_with(&[0xCA, 0xAB, 0, 0]) && (lower.starts_with("building_list") || lower == "far_building_list") => Part::BuildingList,
        _ if head.starts_with(&[0xCA, 0xAB, 0, 0]) => Part::Esf,
        _ if fastbin && lower.ends_with(".tree_list.bin") => Part::TreeListBin,
        _ if fastbin && lower.ends_with("grass_list.bin") => Part::GrassListBin,
        _ if fastbin && lower.ends_with("civilian_deployment.bin") => Part::CivilianDeployment,
        _ if fastbin && lower.ends_with("civilian_shelter.bin") => Part::CivilianShelter,
        _ if fastbin && lower.ends_with("tile_database.bin") => Part::TileDatabase,
        _ if fastbin && lower.ends_with(".bin") && lower.contains("bmd_data") => Part::Bmd,
        _ if lower.ends_with(".tree_list") => Part::OldList(OldListKind::Tree),
        _ if lower.ends_with(".grass_list") => Part::OldList(OldListKind::Grass),
        _ if head.starts_with(b"DDS ") && lower == "blend.dds" => Part::Dds(MapKind::BlendTexture),
        _ if head.starts_with(b"DDS ") && lower == "index.dds" => Part::Dds(MapKind::IndexTexture),
        _ if head.starts_with(b"DDS ") && lower == "normal.dds" => Part::Dds(MapKind::NormalTexture),
        _ if head.starts_with(b"DDS ") && lower == "ground_types.dds" => Part::Dds(MapKind::GroundType),
        _ if (lower.ends_with(".xml") || lower.ends_with(".outlines"))
            && head.iter().find(|b| !b.is_ascii_whitespace() && **b != 0xEF && **b != 0xBB && **b != 0xBF) == Some(&b'<') =>
        {
            Part::Xml
        }

        _ => return None,
    })
}

fn dds_name(kind: MapKind) -> Option<&'static str> {
    Some(match kind {
        MapKind::BlendTexture => "blend.dds",
        MapKind::IndexTexture => "index.dds",
        MapKind::NormalTexture => "normal.dds",
        MapKind::GroundType => "ground_types.dds",
        _ => return None,
    })
}

// ---- small value helpers

fn int(n: &Node, name: &str) -> std::result::Result<i64, String> {
    n.get(name).and_then(Value::as_int).ok_or_else(|| format!("{}: no int {name}", n.class))
}

fn float(n: &Node, name: &str) -> std::result::Result<f64, String> {
    n.get(name).and_then(Value::as_f64).ok_or_else(|| format!("{}: no number {name}", n.class))
}

fn floats<'a>(n: &'a Node, name: &str) -> std::result::Result<&'a [f64], String> {
    n.get(name).and_then(Value::as_floats).ok_or_else(|| format!("{}: no numbers {name}", n.class))
}

fn bytes<'a>(n: &'a Node, name: &str) -> std::result::Result<&'a [u8], String> {
    n.get(name).and_then(Value::as_bytes).ok_or_else(|| format!("{}: no bytes {name}", n.class))
}

fn boolean(n: &Node, name: &str) -> std::result::Result<bool, String> {
    n.get(name).and_then(Value::as_bool).ok_or_else(|| format!("{}: no bool {name}", n.class))
}

/// Text as a value: `Str` when it is UTF-8, else `Bytes`.
fn text(b: &[u8]) -> Value {
    match std::str::from_utf8(b) {
        Ok(s) => Value::Str(s.to_owned()),
        Err(_) => Value::Bytes(b.to_vec()),
    }
}

fn text_bytes(v: &Value) -> std::result::Result<Vec<u8>, String> {
    match v {
        Value::Str(s) => Ok(s.as_bytes().to_vec()),
        Value::Bytes(b) => Ok(b.clone()),
        other => Err(format!("{other:?} where text was expected")),
    }
}

// ---- CHMF

/// What a CHMF file needs besides its samples: whether its encoding is the engine's, else its original bytes.
#[derive(Debug)]
struct ChmfTrivia {
    /// Original bytes and the fingerprints of the regions' samples, for a file the encoder does not give back.
    kept: Option<(Arc<[u8]>, Vec<u64>)>,
}

fn read_chmf(name: &str, kind: MapKind, b: &[u8], tile: &mut Tile) -> std::result::Result<(), String> {
    let f = ChmfFile::parse(b).map_err(|e| e.0)?;
    let mut root = Node::new("CHMF");
    root.set("version", Value::Int(i64::from(f.version)));
    root.set("width", Value::Int(i64::from(f.width)));
    root.set("height", Value::Int(i64::from(f.height)));
    root.set("cells_x", Value::Int(i64::from(f.cells_x)));
    root.set("cells_y", Value::Int(i64::from(f.cells_y)));
    root.set("min", Value::Float(f64::from(f.min)));
    root.set("max", Value::Float(f64::from(f.max)));
    root.set("spacing", Value::Int(i64::from(f.spacing)));
    let mut maps = vec![];
    let mut exact = true;
    let mut prints = vec![];
    for (i, r) in f.regions.iter().enumerate() {
        let (w, h, samples) = f.region_samples(i).map_err(|e| e.0)?;
        let mut region = Node::new("region");
        region.set("rect", Value::Ints(r.rect.iter().map(|&v| i64::from(v)).collect()));
        region.set("tag", Value::Ints(r.tag.iter().map(|&v| i64::from(v)).collect()));
        root.children.push(region);
        let (_, offsets, data) = encode_region(w, h, &samples);
        exact &= offsets == r.offsets && data == r.data;
        let raster = Raster { width: w, height: h, channels: 1, samples: Samples::U16(samples) };
        prints.push(raster.samples.fingerprint());
        maps.push(TileMap { kind, index: i as u32, raster });
    }

    tile.maps.extend(maps);
    tile.documents.push(Document { name: name.to_owned(), root });
    if !exact {
        tile.source.insert(name.to_owned(), Arc::new(ChmfTrivia { kept: Some((Arc::from(b), prints)) }));
    }

    Ok(())
}

fn write_chmf(doc: &Document, tile: &Tile, own: bool) -> std::result::Result<(Vec<u8>, PartWritten), String> {
    let kind = if doc.name.eq_ignore_ascii_case("hf_water_map.data") { MapKind::Water } else { MapKind::Height };
    let root = &doc.root;
    let regions = root.children.iter().filter(|c| c.class == "region").collect::<Vec<_>>();
    let maps = (0..regions.len()).map(|i| tile.map(kind, i as u32).ok_or_else(|| format!("{}: no raster for region {i}", doc.name))).collect::<std::result::Result<Vec<_>, _>>()?;
    if own
        && let Some(ChmfTrivia { kept: Some((bytes, prints)) }) = tile.trivia::<ChmfTrivia>(&doc.name)
        && maps.iter().map(|m| m.raster.samples.fingerprint()).eq(prints.iter().copied())
    {
        return Ok((bytes.to_vec(), PartWritten::Kept));
    }

    let mut f = ChmfFile {
        version: int(root, "version")? as u32,
        width: int(root, "width")? as u32,
        height: int(root, "height")? as u32,
        cells_x: int(root, "cells_x")? as u32,
        cells_y: int(root, "cells_y")? as u32,
        min: float(root, "min")? as f32,
        max: float(root, "max")? as f32,
        spacing: int(root, "spacing")? as u32,
        regions: vec![],
    };

    for (r, m) in regions.iter().zip(&maps) {
        let Samples::U16(samples) = &m.raster.samples else { return Err(format!("{}: the height raster is not 16 bit", doc.name)) };
        let rect = r.get("rect").and_then(Value::as_ints).ok_or("region without rect")?;
        let tag = r.get("tag").and_then(Value::as_ints).ok_or("region without tag")?;
        let (_, offsets, data) = encode_region(m.raster.width, m.raster.height, samples);
        f.regions.push(ChmfRegion {
            rect: [rect[0] as u32, rect[1] as u32, rect[2] as u32, rect[3] as u32],
            tag: [tag[0] as u16, tag[1] as u16],
            offsets,
            data,
        });
    }

    Ok((f.write(), PartWritten::Encoded))
}

// ---- markers

fn marker_value(v: markers::Value) -> (Option<Value>, Option<Node>) {
    use markers::Value as M;
    let pts = |p: &[[f32; 2]]| Value::Floats(p.iter().flat_map(|[x, y]| [f64::from(*x), f64::from(*y)]).collect());
    match v {
        M::Matrix(m) => (Some(Value::Floats(m.iter().map(|x| f64::from(*x)).collect())), None),
        M::F32(x) => (Some(Value::Float(f64::from(x))), None),
        M::U32(x) => (Some(Value::Int(i64::from(x))), None),
        M::U8(x) => (Some(Value::Int(i64::from(x))), None),
        M::Str(s) => (Some(text(&s)), None),
        M::Points(p) => (Some(pts(&p)), None),
        M::PointLists(l) => {
            let mut n = Node::new("polylines");
            n.children = l
                .iter()
                .map(|p| {
                    let mut c = Node::new("polyline");
                    c.set("points", pts(p));
                    c
                })
                .collect();
            (None, Some(n))
        }

        M::Bytes(b) => (Some(Value::Bytes(b)), None),
    }
}

fn read_markers(name: &str, b: &[u8], tile: &mut Tile) -> std::result::Result<(), String> {
    let f = MarkersFile::parse(b).map_err(|e| e.0)?;
    let mut root = Node::new(f.class_name.as_str());
    root.set("repository", Value::Str(f.repository.clone()));
    root.set("repository_id", Value::Int(i64::from(f.repository_id)));
    root.set("repository_u32", Value::Int(i64::from(f.repository_u32)));
    root.set("repository_u16", Value::Int(i64::from(f.repository_u16)));
    root.set("class_id", Value::Int(i64::from(f.class_id)));
    root.set("version", Value::Int(i64::from(f.version)));
    for i in 0..f.records.len() {
        let mut rec = Node::new("record");
        for (field, v) in f.fields(i).map_err(|e| e.0)? {
            match marker_value(v) {
                (Some(v), _) => rec.set(field, v),
                (None, Some(child)) => rec.children.push(child),
                _ => unreachable!(),
            }
        }

        root.children.push(rec);
    }

    if let Some(groups) = &f.groups {
        let mut gs = Node::new("groups");
        for g in groups {
            let mut n = Node::new("group");
            let key = g
                .key
                .iter()
                .map(|k| match k {
                    KeyPart::Wide(w) => match String::from_utf16(w) {
                        Ok(s) => Value::Str(s),
                        Err(_) => Value::Ints(w.iter().map(|&u| i64::from(u)).collect()),
                    },
                    KeyPart::Narrow(s) => text(s),
                    KeyPart::U32(v) => Value::Int(i64::from(*v)),
                })
                .collect::<Vec<_>>();

            n.children = key
                .into_iter()
                .map(|v| {
                    let mut k = Node::new("key");
                    k.set("value", v);
                    k
                })
                .collect();
            n.set("indices", Value::Ints(g.indices.iter().map(|&i| i64::from(i)).collect()));
            gs.children.push(n);
        }

        root.children.push(gs);
    }

    tile.documents.push(Document { name: name.to_owned(), root });
    Ok(())
}

fn write_markers(doc: &Document) -> std::result::Result<Vec<u8>, String> {
    let root = &doc.root;
    let class = root.class.as_str();
    let version = int(root, "version")? as u32;
    let (layout, key_kind) = match markers::layout(class, version) {
        Some(l) => l,
        None => (&[][..], markers::KeyKind::Wide),
    };

    let mut f = MarkersFile {
        repository: root.get("repository").and_then(Value::as_str).unwrap_or("BASE_MARKER_REPOSITORY").to_owned(),
        repository_id: int(root, "repository_id")? as u32,
        repository_u32: int(root, "repository_u32")? as u32,
        repository_u16: int(root, "repository_u16")? as u16,
        class_id: int(root, "class_id")? as u32,
        class_name: class.to_owned(),
        version,
        records: vec![],
        groups: None,
    };

    let pts = |v: &[f64]| v.as_chunks::<2>().0.iter().map(|c| [c[0] as f32, c[1] as f32]).collect::<Vec<_>>();
    for rec in root.children.iter().filter(|c| c.class == "record") {
        let mut fields: Vec<(&str, markers::Value)> = vec![];
        for field in layout {
            use markers::Field as F;
            use markers::Value as M;
            let (name, v) = match *field {
                F::IfByte(_) => continue,
                F::Matrix(n) => (n, rec.get(n).and_then(Value::as_floats).map(|m| M::Matrix(std::array::from_fn(|i| m[i] as f32)))),
                F::F32(n) => (n, rec.get(n).and_then(Value::as_f64).map(|x| M::F32(x as f32))),
                F::U32(n) => (n, rec.get(n).and_then(Value::as_int).map(|x| M::U32(x as u32))),
                F::U8(n) => (n, rec.get(n).and_then(Value::as_int).map(|x| M::U8(x as u8))),
                F::Str(n) => (n, rec.get(n).map(text_bytes).transpose()?.map(M::Str)),
                F::Points(n) => (n, rec.get(n).and_then(Value::as_floats).map(|p| M::Points(pts(p)))),
                F::PointLists(n) => (
                    n,
                    rec.child("polylines").map(|l| {
                        M::PointLists(l.children.iter().map(|c| c.get("points").and_then(Value::as_floats).map(pts).unwrap_or_default()).collect())
                    }),
                ),

                F::Bytes(n, _) => (n, rec.get(n).and_then(Value::as_bytes).map(|b| M::Bytes(b.to_vec()))),
            };

            if let Some(v) = v {
                fields.push((name, v));
            }
        }

        f.records.push(record_from_fields(class, version, &fields).map_err(|e| e.0)?);
    }

    if let Some(gs) = root.child("groups") {
        let mut groups = vec![];
        for g in &gs.children {
            let key = g
                .children
                .iter()
                .map(|k| {
                    let v = k.get("value").ok_or("key without value")?;
                    Ok(match (key_kind, v) {
                        (markers::KeyKind::Wide, Value::Str(s)) => KeyPart::Wide(s.encode_utf16().collect()),
                        (markers::KeyKind::Wide, Value::Ints(u)) => KeyPart::Wide(u.iter().map(|&x| x as u16).collect()),
                        (markers::KeyKind::Narrow, v) => KeyPart::Narrow(text_bytes(v)?),
                        (markers::KeyKind::U32, Value::Int(x)) => KeyPart::U32(*x as u32),
                        (_, v) => return Err(format!("group key {v:?}")),
                    })
                })
                .collect::<std::result::Result<Vec<_>, String>>()?;

            let indices = g.get("indices").and_then(Value::as_ints).ok_or("group without indices")?.iter().map(|&i| i as u32).collect();
            groups.push(MarkerGroup { key, indices });
        }

        f.groups = Some(groups);
    }

    Ok(f.write())
}

// ---- building lists

fn read_building_list(name: &str, b: &[u8], tile: &mut Tile) -> std::result::Result<(), String> {
    let esf = ESF::decode(&mut Cursor::new(b), &None).map_err(|e| e.to_string())?;
    let list = building_list::read(&esf)?;
    let NodeType::Record(r) = esf.root_node() else { unreachable!() };
    let mut root = Node::new("BATTLEFIELD_BUILDING_LIST");
    root.set("version", Value::Int(i64::from(*r.version())));
    // A few of Rome 2's lists have their string table in index order (its tools' writer).
    if *esf.strings_in_index_order() {
        root.set("strings_in_index_order", Value::Bool(true));
    }

    for b in list {
        let mut n = Node::new("building");
        let v3 = |v: [f32; 3]| Value::Vec3(v.map(f64::from));
        n.set("key", Value::Str(b.key));
        n.set("position", v3(b.position));
        n.set("orientation", Value::Floats(b.orientation.iter().map(|&x| f64::from(x)).collect()));
        n.set("scale", v3(b.scale));
        n.set("starting_damage", Value::Float(f64::from(b.starting_damage)));
        for (k, v) in [
            ("on_fire", b.on_fire), ("start_disabled", b.start_disabled), ("weak_point", b.weak_point), ("ai_breachable", b.ai_breachable),
            ("indestructible", b.indestructible), ("dockable", b.dockable),
        ] {
            n.set(k, Value::Bool(v));
        }

        if let Some(t) = b.position_type {
            n.set("position_type", Value::Int(i64::from(t)));
        }

        if let Some(t) = b.toggleable {
            n.set("toggleable", Value::Bool(t));
        }

        if let Some(l) = b.lite {
            n.set("lite", Value::Bool(l));
        }

        if let Some(a) = b.burn_amount {
            n.set("burn_amount", Value::Float(f64::from(a)));
        }

        root.children.push(n);
    }

    // The building list's encoding is fixed (checked by the round trip): a CAAB file with a zero header.
    if *esf.signature() != ESFSignature::CAAB || *esf.unknown_1() != 0 || *esf.creation_date() != 0 {
        return Err("a building list with another ESF header".to_owned());
    }

    tile.documents.push(Document { name: name.to_owned(), root });
    Ok(())
}

fn write_building_list(doc: &Document) -> std::result::Result<Vec<u8>, String> {
    let root = &doc.root;
    let version = int(root, "version")? as u8;
    let f32n = |v: f32| {
        let mut n = F32Node::default();
        n.set_value(v);
        n.set_optimized(v == 0.0);
        NodeType::F32(n)
    };

    let booln = |v: bool| {
        let mut n = BoolNode::default();
        n.set_value(v);
        n.set_optimized(true);
        NodeType::Bool(n)
    };

    let v3n = |v: &[f64; 3]| {
        let mut c = Coordinates3DNode::default();
        c.set_x(v[0] as f32);
        c.set_y(v[1] as f32);
        c.set_z(v[2] as f32);
        NodeType::Coord3d(c)
    };

    let mut blocks = vec![];
    for b in root.children.iter().filter(|c| c.class == "building") {
        let pos = b.get("position").and_then(Value::as_vec3).ok_or("building without position")?;
        let scale = b.get("scale").and_then(Value::as_vec3).ok_or("building without scale")?;
        let o = floats(b, "orientation")?;
        let mut values = vec![
            NodeType::Ascii(b.get("key").and_then(Value::as_str).ok_or("building without key")?.to_owned()),
            v3n(&pos),
            f32n(o[0] as f32),
            f32n(o[1] as f32),
            f32n(o[2] as f32),
            f32n(o[3] as f32),
            v3n(&scale),
            f32n(float(b, "starting_damage")? as f32),
        ];

        for k in ["on_fire", "start_disabled", "weak_point", "ai_breachable", "indestructible", "dockable"] {
            values.push(booln(boolean(b, k)?));
        }

        // The fields past the flags are the ones the building has (by game and version, `building_list`).
        if b.get("position_type").is_some() {
            let mut ptype = U32Node::default();
            ptype.set_value(int(b, "position_type")? as u32);
            ptype.set_optimized(true);
            values.push(NodeType::U32(ptype));
        }

        if b.get("toggleable").is_some() || b.get("lite").is_some() {
            values.push(booln(boolean(b, "toggleable")?));
            values.push(booln(boolean(b, "lite")?));
        }

        if b.get("burn_amount").is_some() {
            values.push(f32n(float(b, "burn_amount")? as f32));
        }

        blocks.push(values);
    }

    let mut block = RecordNode::default();
    block.set_record_flags(RecordNodeFlags::IS_RECORD_NODE | RecordNodeFlags::HAS_NESTED_BLOCKS);
    block.set_name("BATTLEFIELD_BUILDING_LIST_BLOCK".to_owned());
    block.set_children(blocks);
    let mut list = RecordNode::default();
    list.set_record_flags(RecordNodeFlags::IS_RECORD_NODE);
    list.set_version(version);
    list.set_name("BATTLEFIELD_BUILDING_LIST".to_owned());
    list.set_children(vec![vec![NodeType::Record(Box::new(block))]]);
    let mut esf = ESF::default();
    esf.set_signature(ESFSignature::CAAB);
    esf.set_strings_in_index_order(root.get("strings_in_index_order").and_then(Value::as_bool).unwrap_or(false));
    esf.set_root_node(NodeType::Record(Box::new(list)));
    let mut out = vec![];
    esf.encode(&mut out, &None).map_err(|e| e.to_string())?;
    Ok(out)
}

/// The typed buildings of a building list document (for the tools).
///
/// # Errors
///
/// The document is not a building list.
pub fn buildings(doc: &Document) -> std::result::Result<Vec<BattlefieldBuilding>, String> {
    let bytes = write_building_list(doc)?;
    building_list::read(&ESF::decode(&mut Cursor::new(&bytes), &None).map_err(|e| e.to_string())?)
}

// ---- tree and grass lists

fn half_bits(v: f32) -> u16 {
    // Round to nearest even; exact for every value a half holds.
    let x = v.to_bits();
    let sign = ((x >> 16) & 0x8000) as u16;
    let exp = ((x >> 23) & 0xFF) as i32;
    let man = x & 0x7F_FFFF;
    if exp == 0xFF {
        return sign | 0x7C00 | if man != 0 { 0x200 } else { 0 };
    }

    let e = exp - 127 + 15;
    if e >= 0x1F {
        return sign | 0x7C00;
    }

    if e <= 0 {
        if e < -10 {
            return sign;
        }

        let m = man | 0x80_0000;
        let shift = (14 - e) as u32;
        let half = m >> shift;
        let rem = m & ((1 << shift) - 1);
        let mid = 1 << (shift - 1);
        let r = if rem > mid || (rem == mid && half & 1 == 1) { half + 1 } else { half };
        return sign | r as u16;
    }

    let half = ((e as u32) << 10) | (man >> 13);
    let rem = man & 0x1FFF;
    let r = if rem > 0x1000 || (rem == 0x1000 && half & 1 == 1) { half + 1 } else { half };
    sign | r as u16
}

fn read_old_list(name: &str, kind: OldListKind, b: &[u8], tile: &mut Tile) -> std::result::Result<(), String> {
    let l = OldList::parse(kind, b).map_err(|e| e.0)?;
    let mut root = Node::new(if kind == OldListKind::Tree { "TREE_LIST" } else { "GRASS_LIST" });
    root.set("version", Value::Int(i64::from(l.version)));
    for (name, first) in &l.types {
        let mut t = Node::new("type");
        t.set("name", text(name));
        t.set("first", Value::Int(i64::from(*first)));
        root.children.push(t);
    }

    let n = l.len();
    let mut recs = Node::new("records");
    recs.set("position", Value::Floats((0..n).flat_map(|i| l.position(i)).map(f64::from).collect()));
    match (kind, l.version) {
        (OldListKind::Tree, 2) => {
            recs.set("byte_6", Value::Bytes((0..n).map(|i| l.record(i)[6]).collect()));
            recs.set("scale", Value::Bytes((0..n).map(|i| l.record(i)[7]).collect()));
            recs.set("bytes_8", Value::Bytes((0..n).flat_map(|i| l.record(i)[8..10].to_vec()).collect()));
        }

        (OldListKind::Tree, 0) => {
            recs.set("up", Value::Floats((0..n).flat_map(|i| (0..3).map(move |k| (i, k))).map(|(i, k)| f64::from(f32::from_le_bytes(l.record(i)[12 + 4 * k..16 + 4 * k].try_into().expect("4")))).collect()));
            recs.set("bytes_24", Value::Bytes((0..n).flat_map(|i| l.record(i)[24..26].to_vec()).collect()));
            recs.set("scale", Value::Bytes((0..n).map(|i| l.record(i)[26]).collect()));
            recs.set("byte_27", Value::Bytes((0..n).map(|i| l.record(i)[27]).collect()));
        }

        _ => {}
    }

    root.children.push(recs);
    tile.documents.push(Document { name: name.to_owned(), root });
    Ok(())
}

fn write_old_list(doc: &Document) -> std::result::Result<Vec<u8>, String> {
    let root = &doc.root;
    let kind = if root.class == "TREE_LIST" { OldListKind::Tree } else { OldListKind::Grass };
    let version = int(root, "version")? as u32;
    let types = root
        .children
        .iter()
        .filter(|c| c.class == "type")
        .map(|t| Ok((text_bytes(t.get("name").ok_or("type without name")?)?, int(t, "first")? as u32)))
        .collect::<std::result::Result<Vec<_>, String>>()?;

    let recs = root.child("records").ok_or("no records")?;
    let pos = floats(recs, "position")?;
    let n = pos.len() / 3;
    let mut records = vec![];
    for i in 0..n {
        let p = &pos[3 * i..3 * i + 3];
        match (kind, version) {
            (_, 0) => {
                for v in p {
                    records.extend_from_slice(&(*v as f32).to_le_bytes());
                }

                if kind == OldListKind::Tree {
                    for v in &floats(recs, "up")?[3 * i..3 * i + 3] {
                        records.extend_from_slice(&(*v as f32).to_le_bytes());
                    }

                    records.extend_from_slice(&bytes(recs, "bytes_24")?[2 * i..2 * i + 2]);
                    records.push(bytes(recs, "scale")?[i]);
                    records.push(bytes(recs, "byte_27")?[i]);
                }
            }

            _ => {
                for v in p {
                    records.extend_from_slice(&half_bits(*v as f32).to_le_bytes());
                }

                if kind == OldListKind::Tree {
                    records.push(bytes(recs, "byte_6")?[i]);
                    records.push(bytes(recs, "scale")?[i]);
                    records.extend_from_slice(&bytes(recs, "bytes_8")?[2 * i..2 * i + 2]);
                }
            }
        }
    }

    Ok(OldList { kind, version, types, records }.write())
}

/// FASTBIN tree and grass lists as columns: `serde_node` would make a node per tree.
fn read_fastbin_list(name: &str, part: Part, b: &[u8], tile: &mut Tile) -> std::result::Result<(), String> {
    let mut root;
    if part == Part::TreeListBin {
        let f = TreeListFile::decode(&mut Cursor::new(b), &None).map_err(|e| e.to_string())?;
        let l = f.tree_list();
        root = Node::new("TREE_LIST_BIN");
        root.set("version", Value::Int(i64::from(*l.serialise_version())));
        for t in l.tree_list() {
            let mut n = Node::new("type");
            n.set("key", Value::Str(t.key().clone()));
            let v = t.value();
            n.set("position", Value::Floats(v.iter().flat_map(|x| [*x.x(), *x.y(), *x.z()]).map(f64::from).collect()));
            n.set("rotation", Value::Ints(v.iter().map(|x| i64::from(*x.rotation())).collect()));
            n.set("scale", Value::Floats(v.iter().map(|x| f64::from(*x.scale())).collect()));
            n.set("flags", Value::Ints(v.iter().map(|x| i64::from(*x.flags())).collect()));
            n.set("is_burnable", Value::Ints(v.iter().map(|x| i64::from(*x.is_burnable())).collect()));
            root.children.push(n);
        }
    } else {
        let f = GrassListFile::decode(&mut Cursor::new(b), &None).map_err(|e| e.to_string())?;
        let l = f.grass_list();
        root = Node::new("GRASS_LIST_BIN");
        root.set("version", Value::Int(i64::from(*l.serialise_version())));
        for t in l.grass_list() {
            let mut n = Node::new("type");
            n.set("key", Value::Str(t.key().clone()));
            n.set("position", Value::Floats(t.value().iter().flat_map(|x| [*x.x(), *x.y(), *x.z()]).map(f64::from).collect()));
            root.children.push(n);
        }
    }

    tile.documents.push(Document { name: name.to_owned(), root });
    Ok(())
}

fn write_fastbin_list(doc: &Document) -> std::result::Result<Vec<u8>, String> {
    use rpfm_lib::files::bmd_vegetation::grass_list::{GrassItem, GrassItemVector, GrassList};
    use rpfm_lib::files::bmd_vegetation::tree_list::{BattleTreeItem, BattleTreeItemVector, TreeList};
    let root = &doc.root;
    let version = int(root, "version")? as u16;
    let mut out = vec![];
    if root.class == "TREE_LIST_BIN" {
        let mut types = vec![];
        for t in root.children.iter().filter(|c| c.class == "type") {
            let ints = |name: &str| t.get(name).and_then(Value::as_ints).ok_or_else(|| format!("type without {name}"));
            let (rot, flags, burn) = (ints("rotation")?, ints("flags")?, ints("is_burnable")?);
            let (pos, scale) = (floats(t, "position")?, floats(t, "scale")?);
            // rpfm_lib's `id` is not in the file (it makes one up when it reads); the default writes the same bytes.
            let items = (0..rot.len())
                .map(|i| {
                    let mut x = BattleTreeItem::default();
                    x.set_x(pos[3 * i] as f32);
                    x.set_y(pos[3 * i + 1] as f32);
                    x.set_z(pos[3 * i + 2] as f32);
                    x.set_rotation(rot[i] as u8);
                    x.set_scale(scale[i] as f32);
                    x.set_flags(flags[i] as u8);
                    x.set_is_burnable(burn[i] as u8);
                    x
                })
                .collect();

            let mut v = BattleTreeItemVector::default();
            v.set_key(t.get("key").and_then(Value::as_str).unwrap_or_default().to_owned());
            v.set_value(items);
            types.push(v);
        }

        let mut l = TreeList::default();
        l.set_serialise_version(version);
        l.set_tree_list(types);
        let mut f = TreeListFile::default();
        f.set_tree_list(l);
        f.encode(&mut out, &None).map_err(|e| e.to_string())?;
    } else {
        let mut types = vec![];
        for t in root.children.iter().filter(|c| c.class == "type") {
            let pos = floats(t, "position")?;
            let items = pos
                .as_chunks::<3>()
                .0
                .iter()
                .map(|p| {
                    let mut x = GrassItem::default();
                    x.set_x(p[0] as f32);
                    x.set_y(p[1] as f32);
                    x.set_z(p[2] as f32);
                    x
                })
                .collect();

            let mut v = GrassItemVector::default();
            v.set_key(t.get("key").and_then(Value::as_str).unwrap_or_default().to_owned());
            v.set_value(items);
            types.push(v);
        }

        let mut l = GrassList::default();
        l.set_serialise_version(version);
        l.set_grass_list(types);
        let mut f = GrassListFile::default();
        f.set_grass_list(l);
        f.encode(&mut out, &None).map_err(|e| e.to_string())?;
    }

    Ok(out)
}

// ---- rpfm structs through serde

/// rpfm_lib gives some structs an `id` that is not in the file (a new random u64 at every read: the capture locations of the BMD files,
/// the trees of the tree lists). They are set to 0, so a re-read is the same model; the round trip checks that no `id` is file data.
fn zero_ids(n: &mut Node) {
    if n.get("id").is_some() {
        n.set("id", Value::Int(0));
    }

    for c in &mut n.children {
        zero_ids(c);
    }
}

fn read_serde<T: Decodeable + Serialize>(name: &str, class: &str, b: &[u8], tile: &mut Tile) -> std::result::Result<(), String> {
    let v = T::decode(&mut Cursor::new(b), &None).map_err(|e| e.to_string())?;
    let mut root = serde_node::to_node(class, &v)?;
    zero_ids(&mut root);
    tile.documents.push(Document { name: name.to_owned(), root });
    Ok(())
}

fn write_serde<T: Encodeable + DeserializeOwned>(doc: &Document) -> std::result::Result<Vec<u8>, String> {
    let mut v: T = serde_node::from_node(&doc.root)?;
    let mut out = vec![];
    v.encode(&mut out, &None).map_err(|e| e.to_string())?;
    Ok(out)
}

// ---- DDS

#[derive(Debug)]
struct DdsTrivia {
    /// The bytes before the pixels (uncompressed files, written from the pixels).
    header: Option<Vec<u8>>,

    /// The original bytes and the fingerprint of the decoded pixels (compressed files).
    kept: Option<(Arc<[u8]>, u64)>,
}

fn read_dds(name: &str, kind: MapKind, b: &[u8], tile: &mut Tile) -> std::result::Result<(), String> {
    let d = DdsFile::parse(b).map_err(|e| e.0)?;
    let (w, h, bpp) = (d.width(), d.height(), d.bit_count() / 8);
    let raster;
    let trivia;
    if d.fourcc() == 0 && d.mip_count() <= 1 && !d.has_dx10 && bpp > 0 && d.payload.len() == (w * h * bpp) as usize {
        raster = Raster { width: w, height: h, channels: bpp, samples: Samples::U8(d.payload.clone()) };
        trivia = DdsTrivia { header: Some(b[..b.len() - d.payload.len()].to_vec()), kept: None };
    } else {
        let img = dds_decode_mip(b, 0).map_err(str::to_owned)?;
        raster = Raster { width: img.w, height: img.h, channels: 4, samples: Samples::U8(img.px) };
        trivia = DdsTrivia { header: None, kept: Some((Arc::from(b), raster.samples.fingerprint())) };
    }

    tile.maps.push(TileMap { kind, index: 0, raster });
    tile.source.insert(name.to_owned(), Arc::new(trivia));
    Ok(())
}

fn write_dds(name: &str, m: &TileMap, tile: &Tile, own: bool) -> std::result::Result<(Vec<u8>, PartWritten), String> {
    let t = if own { tile.trivia::<DdsTrivia>(name) } else { None };
    if let Some(DdsTrivia { kept: Some((bytes, print)), .. }) = t {
        if m.raster.samples.fingerprint() == *print {
            return Ok((bytes.to_vec(), PartWritten::Kept));
        }

        return Err(format!("{name}: the texture changed and its compressor is not reproduced yet"));
    }

    let Some(DdsTrivia { header: Some(header), .. }) = t else { return Err(format!("{name}: no DDS layout to write it in")) };
    let Samples::U8(px) = &m.raster.samples else { return Err(format!("{name}: not 8 bit")) };
    Ok(([&header[..], px].concat(), PartWritten::Encoded))
}

// ---- XML

/// Every attribute as text: the XML files of an exported tile are carried by structure, not typed yet.
pub(crate) struct Untyped;

impl Schema for Untyped {
    fn kind(&self, _: Sym, _: Sym) -> Option<Kind> {
        Some(Kind::Str)
    }
}

fn read_xml(name: &str, b: &[u8], tile: &mut Tile) -> std::result::Result<(), String> {
    let (root, style, _) = xml::read(b, &Untyped).map_err(|e| e.to_string())?;
    tile.documents.push(Document { name: name.to_owned(), root });
    tile.source.insert(name.to_owned(), Arc::new(style));
    Ok(())
}

// ---- the format

impl TileFormat for TileBinary {
    fn id(&self) -> &'static str {
        ID
    }

    fn claims(&self, files: &TileFiles) -> bool {
        files.folder.keys().any(|k| k == "hf_height_map.data" || k == "mesh.rigid_model_v2") && !files.folder.contains_key("tile.ted")
    }

    fn read(&self, files: &TileFiles) -> Result<(Tile, ReadReport)> {
        let mut tile = Tile { source_format: Some(ID), variation: files.variation.clone(), ..Default::default() };
        let mut report = ReadReport::default();
        for (path, blob) in &files.folder {
            let bytes = blob.bytes().map_err(|e| TileIoError::File { file: path.clone(), what: e.to_string() })?;
            let part = if path.contains('/') { None } else { classify(path, &bytes[..bytes.len().min(64)]) };
            let Some(part) = part else {
                tile.files.push(OpaqueFile { path: path.clone(), data: blob.clone() });
                continue;
            };

            let (n_docs, n_maps) = (tile.documents.len(), tile.maps.len());
            let read = match part {
                Part::Chmf(kind) => read_chmf(path, kind, &bytes, &mut tile),
                Part::Markers => read_markers(path, &bytes, &mut tile),
                Part::BuildingList => read_building_list(path, &bytes, &mut tile),
                Part::Esf => read_serde::<ESF>(path, "ESF", &bytes, &mut tile),
                Part::TreeListBin | Part::GrassListBin => read_fastbin_list(path, part, &bytes, &mut tile),
                Part::OldList(kind) => read_old_list(path, kind, &bytes, &mut tile),
                Part::Bmd => read_serde::<Bmd>(path, "BMD", &bytes, &mut tile),
                Part::CivilianDeployment => read_serde::<CivilianDeploymentFile>(path, "CIVILIAN_DEPLOYMENT", &bytes, &mut tile),
                Part::CivilianShelter => read_serde::<CivilianShelterFile>(path, "CIVILIAN_SHELTER", &bytes, &mut tile),
                Part::TileDatabase => read_serde::<TileDatabase>(path, "TILE_DATABASE", &bytes, &mut tile),
                Part::Dds(kind) => read_dds(path, kind, &bytes, &mut tile),
                Part::Xml => read_xml(path, &bytes, &mut tile),
            };

            match read {
                Ok(()) => report.parts.push((path.clone(), PartRead::Decoded)),
                Err(e) => {
                    tile.documents.truncate(n_docs);
                    tile.maps.truncate(n_maps);
                    tile.source.remove(path);
                    tile.files.push(OpaqueFile { path: path.clone(), data: Blob::Bytes(bytes) });
                    report.parts.push((path.clone(), PartRead::Failed(e)));
                }
            }
        }

        tile.files.sort_by(|a, b| a.path.cmp(&b.path));
        Ok((tile, report))
    }

    fn write(&self, tile: &Tile) -> Result<(TileFiles, WriteReport)> {
        let mut out = TileFiles { variation: tile.variation.clone(), ..Default::default() };
        let mut report = WriteReport::default();
        let own = tile.source_format == Some(ID);
        let mut put = |name: &str, r: std::result::Result<(Vec<u8>, PartWritten), String>, out: &mut TileFiles| -> Result<()> {
            let (bytes, how) = r.map_err(|what| TileIoError::File { file: name.to_owned(), what })?;
            report.parts.push((name.to_owned(), how));
            out.folder.insert(name.to_owned(), Blob::Bytes(bytes.into()));
            Ok(())
        };

        let enc = |r: std::result::Result<Vec<u8>, String>| r.map(|b| (b, PartWritten::Encoded));
        for doc in &tile.documents {
            // XML by its name first: an XML file can have the root of a binary one (`building_list.xml`).
            let lower = doc.name.to_lowercase();
            let head = if lower.ends_with(".xml") || lower.ends_with(".outlines") { "#xml" } else { doc.root.class.as_str() };
            let r = match head {
                "CHMF" => write_chmf(doc, tile, own),
                "BATTLEFIELD_BUILDING_LIST" => enc(write_building_list(doc)),
                "TREE_LIST" | "GRASS_LIST" => enc(write_old_list(doc)),
                "TREE_LIST_BIN" | "GRASS_LIST_BIN" => enc(write_fastbin_list(doc)),
                "ESF" => enc(write_serde::<ESF>(doc)),
                "BMD" => enc(write_serde::<Bmd>(doc)),
                "CIVILIAN_DEPLOYMENT" => enc(write_serde::<CivilianDeploymentFile>(doc)),
                "CIVILIAN_SHELTER" => enc(write_serde::<CivilianShelterFile>(doc)),
                "TILE_DATABASE" => enc(write_serde::<TileDatabase>(doc)),
                _ if lower.ends_with(".markers") => enc(write_markers(doc)),
                _ => {
                    let style = own.then(|| tile.trivia::<DocStyle>(&doc.name).cloned()).flatten().unwrap_or_else(DocStyle::ted);
                    enc(Ok(xml::write(&doc.root, &style, &Untyped)))
                }
            };

            put(&doc.name, r, &mut out)?;
        }

        for m in &tile.maps {
            if let Some(name) = dds_name(m.kind) {
                put(name, write_dds(name, m, tile, own), &mut out)?;
            } else if !matches!(m.kind, MapKind::Height | MapKind::Water) {
                return Err(TileIoError::File { file: format!("{:?} map", m.kind), what: "an exported tile has no such map".to_owned() });
            }
        }

        for f in &tile.files {
            report.parts.push((f.path.clone(), PartWritten::Copied));
            out.folder.insert(f.path.clone(), f.data.clone());
        }

        Ok((out, report))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn halves() {
        for bits in [0u16, 0x3C00, 0xC000, 0x5507, 0xA05A, 0x0001, 0x03FF, 0x7BFF, 0x8001] {
            let v = {
                let sign = if bits & 0x8000 != 0 { -1.0f32 } else { 1.0 };
                let e = i32::from((bits >> 10) & 0x1F);
                let m = f32::from(bits & 0x3FF);
                sign * if e == 0 { m * 2f32.powi(-24) } else { (1.0 + m / 1024.0) * 2f32.powi(e - 15) }
            };

            assert_eq!(half_bits(v), bits, "{bits:#06x}");
        }
    }
}
