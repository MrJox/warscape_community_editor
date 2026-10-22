//! The files of Attila's exported (binary) tiles: which codec reads each, and their round trip (decode, encode). The codecs are
//! rpfm_lib's (ESF, FASTBIN lists, BMD, rigid models, the tile database) and wce_formats' (CHMF, markers, DDS, lossless XML).
//! docs/formats/tile_binary.md lists the files and what each holds.

use std::io::Cursor;

use rpfm_lib::files::bmd::Bmd;
use rpfm_lib::files::esf::ESF;
use rpfm_lib::files::fastbin_lists::{CivilianDeploymentFile, CivilianShelterFile, GrassListFile, TreeListFile};
use rpfm_lib::files::rigidmodel::RigidModel;
use rpfm_lib::files::tile_database::TileDatabase;
use rpfm_lib::files::{Decodeable, DecodeableExtraData, Encodeable};
use wce_formats::chmf::ChmfFile;
use wce_formats::image_containers::DdsFile;
use wce_formats::markers::MarkersFile;
use wce_formats::xml_doc::XmlDoc;

/// How a file of an exported tile is read.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Codec {
    /// CAAB ESF (`building_list`, `far_building_list`, `bmd.grass_list`, ...).
    Esf,
    TreeListBin,
    GrassListBin,
    CivilianDeployment,
    CivilianShelter,

    /// BATTLE_MAP_DEFINITION binaries (`*_procedural_bmd_data.bin`, `bmd_data_escalation<n>.bin`).
    Bmd,
    TileDatabase,
    Chmf,
    Markers,
    Dds,
    RigidModel,
    Xml,

    /// The older tree and grass lists (no `.bin`, not FASTBIN).
    OldList,

    /// No codec: carried as bytes.
    Opaque,
}

const FASTBIN: &[u8] = b"FASTBIN0";
const CAAB: &[u8] = &[0xCA, 0xAB, 0, 0];

/// The codec of a file, from its name and first bytes.
pub fn codec_of(name: &str, head: &[u8]) -> Codec {
    let lower = name.to_lowercase();
    let fastbin = head.starts_with(FASTBIN);
    if head.starts_with(CAAB) {
        Codec::Esf
    } else if head.starts_with(b"CHMF") {
        Codec::Chmf
    } else if head.starts_with(b"RMV2") {
        Codec::RigidModel
    } else if head.starts_with(b"DDS ") {
        Codec::Dds
    } else if lower.ends_with(".markers") {
        Codec::Markers
    } else if fastbin && lower.ends_with(".tree_list.bin") {
        Codec::TreeListBin
    } else if fastbin && lower.ends_with("grass_list.bin") {
        Codec::GrassListBin
    } else if fastbin && lower.ends_with("civilian_deployment.bin") {
        Codec::CivilianDeployment
    } else if fastbin && lower.ends_with("civilian_shelter.bin") {
        Codec::CivilianShelter
    } else if fastbin && lower.ends_with("tile_database.bin") {
        Codec::TileDatabase
    } else if fastbin && lower.ends_with(".bin") && (lower.contains("_bmd_data") || lower.contains("bmd_data_escalation")) {
        Codec::Bmd
    } else if lower.ends_with(".xml") && head.iter().find(|b| !b.is_ascii_whitespace() && **b != 0xEF && **b != 0xBB && **b != 0xBF) == Some(&b'<') {
        Codec::Xml
    } else if lower.ends_with(".tree_list") || lower.ends_with(".grass_list") {
        Codec::OldList
    } else {
        Codec::Opaque
    }
}

fn rpfm<T: Decodeable + Encodeable>(bytes: &[u8], extra: &Option<DecodeableExtraData>) -> Result<Vec<u8>, String> {
    let mut v = T::decode(&mut Cursor::new(bytes), extra).map_err(|e| e.to_string())?;
    let mut out = vec![];
    v.encode(&mut out, &None).map_err(|e| e.to_string())?;
    Ok(out)
}

/// Decodes and encodes a file with its codec; `Ok` holds the encoded bytes (the caller compares them). `Opaque` and `OldList` give the
/// bytes back as they are.
///
/// # Errors
///
/// The codec could not read the file.
pub fn round_trip(codec: Codec, bytes: &[u8]) -> Result<Vec<u8>, String> {
    match codec {
        Codec::Esf => rpfm::<ESF>(bytes, &None),
        Codec::TreeListBin => rpfm::<TreeListFile>(bytes, &None),
        Codec::GrassListBin => rpfm::<GrassListFile>(bytes, &None),
        Codec::CivilianDeployment => rpfm::<CivilianDeploymentFile>(bytes, &None),
        Codec::CivilianShelter => rpfm::<CivilianShelterFile>(bytes, &None),
        Codec::Bmd => rpfm::<Bmd>(bytes, &None),
        Codec::TileDatabase => rpfm::<TileDatabase>(bytes, &None),
        Codec::RigidModel => rpfm::<RigidModel>(bytes, &Some(DecodeableExtraData::default())),
        Codec::Chmf => ChmfFile::parse(bytes).map(|f| f.write()).map_err(|e| e.0),
        Codec::Markers => MarkersFile::parse(bytes).map(|f| f.write()).map_err(|e| e.0),
        Codec::Dds => DdsFile::parse(bytes).map(|f| f.write()).map_err(|e| e.0),
        Codec::Xml => XmlDoc::parse(bytes.to_vec()).map(|d| d.save()).map_err(|e| e.to_string()),
        Codec::OldList | Codec::Opaque => Ok(bytes.to_vec()),
    }
}
