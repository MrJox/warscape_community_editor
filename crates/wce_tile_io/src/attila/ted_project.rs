//! Attila's TEd project: a tile's variation folder in the Assembly Kit (`raw_data/terrain/tiles/<kind>/<set>/.../<variation>`) and its
//! tile database entry (`_tile_database/TILES/<name>.xml`).
//!
//! Into the model: `tile.ted` (the scene), `height_map_<n>.png`, `blend_map.tif`, `ground_type_map.png`, `nogo.png`,
//! `lf_influence_map.png`, `river_spline[<n>].xml` and the entry. Every other file of the folder (BOB's outputs, the artists' working files,
//! legacy stage-A exports, anything in a sub folder) is carried along untouched, and so is a file of a kind above that does not read (the
//! kit has PNGs whose line ends were converted to CRLF).

use std::sync::Arc;

use wce_tile_model::{Blob, Entry, MapKind, Node, OpaqueFile, RiverSplines, Tile, TileMap};

use super::images::{self, Encoded, ImageTrivia};
use super::schema::{EntrySchema, RiverSchema, TedSchema};
use crate::xml::values::Schema;
use crate::xml::{self, DocStyle, ReadStats};
use crate::{PartRead, PartWritten, ReadReport, Result, TileFiles, TileFormat, TileIoError, WriteReport};

pub struct TedProject;

pub const ID: &str = "attila.ted_project";

/// The trivia key of the entry.
const ENTRY: &str = "entry";

/// A file of the folder the model holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Part {
    Scene,
    River(Option<u32>),
    Map(MapKind, u32),
}

/// A number in a file name, without leading zeros (so the name can be rebuilt from it).
fn index(digits: &str) -> Option<u32> {
    (!digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()) && (digits == "0" || !digits.starts_with('0'))).then(|| digits.parse().ok())?
}

fn classify(name: &str) -> Option<Part> {
    Some(match name {
        "tile.ted" => Part::Scene,
        "blend_map.tif" => Part::Map(MapKind::Blend, 0),
        "ground_type_map.png" => Part::Map(MapKind::GroundType, 0),
        "nogo.png" => Part::Map(MapKind::Nogo, 0),
        "lf_influence_map.png" => Part::Map(MapKind::LfInfluence, 0),
        "river_spline.xml" => Part::River(None),
        _ => {
            if let Some(n) = name.strip_prefix("height_map_").and_then(|r| r.strip_suffix(".png")) {
                Part::Map(MapKind::Height, index(n)?)
            } else {
                Part::River(Some(index(name.strip_prefix("river_spline")?.strip_suffix(".xml")?)?))
            }
        }
    })
}

/// The file of a part; `None` for a map TEd's projects do not have.
fn file_name(part: Part) -> Option<String> {
    Some(match part {
        Part::Scene => "tile.ted".to_owned(),
        Part::River(None) => "river_spline.xml".to_owned(),
        Part::River(Some(n)) => format!("river_spline{n}.xml"),
        Part::Map(MapKind::Height, n) => format!("height_map_{n}.png"),
        Part::Map(MapKind::Blend, _) => "blend_map.tif".to_owned(),
        Part::Map(MapKind::GroundType, _) => "ground_type_map.png".to_owned(),
        Part::Map(MapKind::Nogo, _) => "nogo.png".to_owned(),
        Part::Map(MapKind::LfInfluence, _) => "lf_influence_map.png".to_owned(),
        Part::Map(MapKind::Water | MapKind::BlendTexture | MapKind::IndexTexture | MapKind::NormalTexture, _) => return None,
    })
}

fn merge(into: &mut ReadStats, from: ReadStats) {
    into.elements += from.elements;
    into.props += from.props;
    into.layout_nodes += from.layout_nodes;
    for (k, v) in from.verbatim {
        *into.verbatim.entry(k).or_default() += v;
    }

    for (k, v) in from.unknown {
        *into.unknown.entry(k).or_default() += v;
    }
}

fn read_xml(bytes: &[u8], schema: &dyn Schema, report: &mut ReadReport) -> std::result::Result<(Node, DocStyle), String> {
    let (root, style, stats) = xml::read(bytes, schema).map_err(|e| e.to_string())?;
    merge(&mut report.xml, stats);
    Ok((root, style))
}

/// The style TEd's tools write an entry in, for an entry the tile was not read with.
pub(crate) fn entry_style() -> DocStyle {
    DocStyle { prolog: vec![], epilog: b"\r\n".to_vec(), newline: b"\r\n".to_vec(), indent: b"\t".to_vec(), quote: b'\'' }
}

impl TileFormat for TedProject {
    fn id(&self) -> &'static str {
        ID
    }

    fn claims(&self, files: &TileFiles) -> bool {
        files.folder.contains_key("tile.ted")
    }

    fn read(&self, files: &TileFiles) -> Result<(Tile, ReadReport)> {
        let mut tile = Tile { source_format: Some(ID), variation: files.variation.clone(), ..Default::default() };
        let mut report = ReadReport::default();
        for (path, blob) in &files.folder {
            let part = if path.contains('/') { None } else { classify(path) };
            let Some(part) = part else {
                tile.files.push(OpaqueFile { path: path.clone(), data: blob.clone() });
                continue;
            };

            let bytes = blob.bytes().map_err(|e| TileIoError::File { file: path.clone(), what: e.to_string() })?;
            let read: std::result::Result<(), String> = match part {
                Part::Scene => read_xml(&bytes, &TedSchema, &mut report).map(|(root, style)| {
                    tile.scene = Some(root);
                    tile.source.insert(path.clone(), Arc::new(style));
                }),

                Part::River(index) => read_xml(&bytes, &RiverSchema, &mut report).map(|(root, style)| {
                    tile.rivers.push(RiverSplines { index, root });
                    tile.source.insert(path.clone(), Arc::new(style));
                }),

                Part::Map(kind, index) => {
                    let decoded = if kind == MapKind::Blend { images::read_tiff(&bytes) } else { images::read_png(&bytes) };
                    decoded.map(|(raster, trivia)| {
                        tile.maps.push(TileMap { kind, index, raster });
                        tile.source.insert(path.clone(), Arc::new(trivia));
                    })
                }
            };

            match read {
                Ok(()) => report.parts.push((path.clone(), PartRead::Decoded)),
                Err(e) => {
                    tile.files.push(OpaqueFile { path: path.clone(), data: Blob::Bytes(bytes) });
                    report.parts.push((path.clone(), PartRead::Failed(e)));
                }
            }
        }

        if let Some((name, blob)) = &files.entry {
            let bytes = blob.bytes().map_err(|e| TileIoError::File { file: name.clone(), what: e.to_string() })?;
            let (root, style) = read_xml(&bytes, &EntrySchema, &mut report).map_err(|what| TileIoError::File { file: name.clone(), what })?;
            tile.entry = Some(Entry { file_name: name.clone(), root });
            tile.source.insert(ENTRY.to_owned(), Arc::new(style));
            report.parts.push((name.clone(), PartRead::Decoded));
        }

        tile.maps.sort_by_key(|m| (m.kind, m.index));
        tile.rivers.sort_by_key(|r| r.index);
        tile.files.sort_by(|a, b| a.path.cmp(&b.path));
        Ok((tile, report))
    }

    fn write(&self, tile: &Tile) -> Result<(TileFiles, WriteReport)> {
        let mut out = TileFiles { variation: tile.variation.clone(), ..Default::default() };
        let mut report = WriteReport::default();
        let own = tile.source_format == Some(ID);
        let style = |key: &str, default: fn() -> DocStyle| own.then(|| tile.trivia::<DocStyle>(key).cloned()).flatten().unwrap_or_else(default);
        let mut put = |name: String, bytes: Vec<u8>, how: PartWritten, out: &mut TileFiles| {
            report.parts.push((name.clone(), how));
            out.folder.insert(name, Blob::Bytes(bytes.into()));
        };

        if let Some(scene) = &tile.scene {
            let name = "tile.ted".to_owned();
            let bytes = xml::write(scene, &style(&name, DocStyle::ted), &TedSchema);
            put(name, bytes, PartWritten::Encoded, &mut out);
        }

        for r in &tile.rivers {
            let name = file_name(Part::River(r.index)).expect("a river file");
            let bytes = xml::write(&r.root, &style(&name, DocStyle::ted), &RiverSchema);
            put(name, bytes, PartWritten::Encoded, &mut out);
        }

        for m in &tile.maps {
            let Some(name) = file_name(Part::Map(m.kind, m.index)) else {
                return Err(TileIoError::File { file: format!("{:?} map", m.kind), what: "a TEd project has no such map".to_owned() });
            };
            let trivia = own.then(|| tile.trivia::<ImageTrivia>(&name)).flatten();
            let fail = |what: String| TileIoError::File { file: name.clone(), what };
            let (bytes, how) = if m.kind == MapKind::Blend {
                (images::write_tiff(&m.raster, trivia).map_err(fail)?, PartWritten::Encoded)
            } else {
                let (bytes, how) = images::write_png(&m.raster, trivia).map_err(fail)?;
                (bytes, if how == Encoded::Kept { PartWritten::Kept } else { PartWritten::Encoded })
            };

            put(name, bytes, how, &mut out);
        }

        for f in &tile.files {
            report.parts.push((f.path.clone(), PartWritten::Copied));
            out.folder.insert(f.path.clone(), f.data.clone());
        }

        if let Some(e) = &tile.entry {
            let bytes = xml::write(&e.root, &style(ENTRY, entry_style), &EntrySchema);
            report.parts.push((e.file_name.clone(), PartWritten::Encoded));
            out.entry = Some((e.file_name.clone(), Blob::Bytes(bytes.into())));
        }

        Ok((out, report))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_names_round_trip() {
        for name in ["tile.ted", "height_map_0.png", "height_map_12.png", "river_spline.xml", "river_spline3.xml", "blend_map.tif", "nogo.png"] {
            assert_eq!(file_name(classify(name).unwrap()).unwrap(), name);
        }

        for other in ["height_map_01.png", "river_spline_a.xml", "height_map_.png", "tile.ted.bak", "final_heights.dds"] {
            assert_eq!(classify(other), None, "{other}");
        }
    }
}
