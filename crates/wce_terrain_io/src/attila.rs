//! Attila's map folders (`raw_data/terrain/battles/<map>`, `raw_data/terrain/campaigns/<map>`; docs/re/terry.md, "The maps' files"):
//!
//! - the cell layers: `tile_map.png`, `climate_map.png`, `culture_map.png`, `snow_map[_<season>].png`, `season_mask_<season>_01.png`,
//!   `layout_map.png` (8-bit RGB or RGBA; most were saved by Photoshop, with `pHYs` / `iCCP` / `cHRM` chunks, which an edit keeps);
//! - the LF fields: `lf_heights.tif`, `lf_sea_heights.tif` (16-bit grey, uncompressed, one strip; TEd's 242-byte layout or Photoshop's:
//!   an edit rewrites the samples in place, the header and any bytes after the strip stay);
//! - `explicit_tiles.txt`, the `.terry` project, the folder's XML files (lossless);
//! - everything else carried (`normal.dds`, `nogo.dds`, `.psd`, `rules.bob`, ...).
//!
//! A file that does not read as its kind is carried as it is.

pub mod build;
pub mod campaign;
pub mod heights;
pub mod normals;

use std::collections::BTreeMap;
use std::sync::Arc;

use wce_formats::image_containers::TiffFile;
use wce_formats::png_file::PngFile;
use wce_formats::xml_doc::XmlDoc;
use wce_terrain::cells::CellLayer;
use wce_terrain::explicit::ExplicitTiles;
use wce_terrain::paged::PagedU16;
use wce_terrain::terry::TerryProject;
use wce_terrain::{Item, LayerRole, LfRole, MapKind, MapModel};
use wce_tile_model::Blob;

use crate::{MapFiles, Result, file_error};

/// The role of a top-level PNG, by its name (lower case).
fn layer_role(name: &str) -> Option<LayerRole> {
    let stem = name.strip_suffix(".png")?;
    Some(match stem {
        "tile_map" => LayerRole::TileMap,
        "climate_map" => LayerRole::Climate,
        "culture_map" => LayerRole::Culture,
        "layout_map" => LayerRole::Layout,
        "snow_map" => LayerRole::Snow(String::new()),
        _ => {
            if let Some(season) = stem.strip_prefix("snow_map_") {
                LayerRole::Snow(season.to_owned())
            } else if let Some(season) = stem.strip_prefix("season_mask_").and_then(|s| s.strip_suffix("_01")) {
                LayerRole::SeasonMask(season.to_owned())
            } else {
                return None;
            }
        }
    })
}

fn lf_role(name: &str) -> Option<LfRole> {
    match name {
        "lf_heights.tif" => Some(LfRole::Land),
        "lf_sea_heights.tif" => Some(LfRole::Sea),
        _ => None,
    }
}

fn read_layer(bytes: &[u8]) -> std::result::Result<CellLayer, String> {
    let p = PngFile::parse(bytes).map_err(|e| e.0)?;
    let channels = match (p.color_type, p.bit_depth) {
        (2, 8) => 3,
        (6, 8) => 4,
        _ => return Err(format!("png: colour type {} depth {}", p.color_type, p.bit_depth)),
    };

    CellLayer::from_pixels(p.width, p.height, channels, &p.pixels)
}

/// An LF file's samples as a copy-on-write view of the file (`SampleGrid::map_file`): its header and directory read for the one strip of
/// 16-bit grey samples. None for any other layout (it is then read whole, `read_lf`).
fn map_lf(path: &std::path::Path) -> Option<PagedU16> {
    use std::io::{Read, Seek, SeekFrom};
    let mut f = std::fs::File::open(path).ok()?;
    let mut head = [0u8; 8];
    f.read_exact(&mut head).ok()?;
    if &head[..4] != b"II*\0" {
        return None;
    }

    let ifd = u32::from_le_bytes([head[4], head[5], head[6], head[7]]);
    f.seek(SeekFrom::Start(u64::from(ifd))).ok()?;
    let mut count = [0u8; 2];
    f.read_exact(&mut count).ok()?;
    let mut entries = vec![0u8; usize::from(u16::from_le_bytes(count)) * 12];
    f.read_exact(&mut entries).ok()?;
    let mut tag = std::collections::HashMap::new();
    for e in entries.chunks_exact(12) {
        let (id, kind, n) = (u16::from_le_bytes([e[0], e[1]]), u16::from_le_bytes([e[2], e[3]]), u32::from_le_bytes([e[4], e[5], e[6], e[7]]));
        // One value in the entry (a short in its first two bytes).
        let value = if kind == 3 { u32::from(u16::from_le_bytes([e[8], e[9]])) } else { u32::from_le_bytes([e[8], e[9], e[10], e[11]]) };
        if n == 1 {
            tag.insert(id, value);
        }
    }

    let get = |t: u16| tag.get(&t).copied();
    let (w, h) = (get(256)?, get(257)?);
    let grey16 = get(258) == Some(16) && get(277).unwrap_or(1) == 1 && get(259).unwrap_or(1) == 1;
    let offset = get(273)?;
    if !grey16 || get(279).is_some_and(|n| u64::from(n) < u64::from(w) * u64::from(h) * 2) {
        return None;
    }

    let grid = wce_foundation::samples::SampleGrid::map_file(path, u64::from(offset), w, h).ok()?;
    Some(PagedU16::from_grid(std::sync::Arc::new(grid)))
}

fn read_lf(bytes: &[u8]) -> std::result::Result<PagedU16, String> {
    let t = TiffFile::parse(bytes).map_err(|e| e.0)?;
    if t.samples_per_pixel != 1 || t.bits_per_sample != 16 || t.entries.iter().any(|e| e.tag == 259 && e.value & 0xFFFF != 1) {
        return Err(format!("tiff: {} x {} bits, compressed or not grey", t.samples_per_pixel, t.bits_per_sample));
    }

    if t.pixels.len() < t.width as usize * t.height as usize * 2 {
        return Err("tiff: short strip".to_owned());
    }

    Ok(PagedU16::from_le_rows(t.width, t.height, &t.pixels))
}

/// Reads a map folder. `name` is the map's (its folder's name), `kind` where it lives.
///
/// # Errors
///
/// A file could not be read.
pub fn read(name: &str, kind: MapKind, files: &MapFiles) -> Result<MapModel> {
    let mut m = MapModel { name: name.to_owned(), kind, ..Default::default() };
    for (path, blob) in &files.folder {
        let lower = path.to_ascii_lowercase();
        let top = !path.contains('/');
        let carried = |m: &mut MapModel| {
            m.carried.insert(path.clone(), blob.clone());
        };

        if !top {
            carried(&mut m);
            continue;
        }

        if let Some(role) = layer_role(&lower) {
            let bytes = blob.bytes()?;
            match read_layer(&bytes) {
                Ok(l) => m.layers.push((role, Item::new(path.clone(), l, blob.clone()))),
                Err(_) => carried(&mut m),
            }
        } else if let Some(role) = lf_role(&lower) {
            // A file on disk: its samples a view of it (`map_lf`); else (or when that fails) read.
            if let Blob::File(file) = blob
                && let Some(f) = map_lf(file)
            {
                m.lf.push((role, Item::new(path.clone(), f, blob.clone())));
                continue;
            }

            let bytes = blob.bytes()?;
            match read_lf(&bytes) {
                Ok(f) => m.lf.push((role, Item::new(path.clone(), f, blob.clone()))),
                Err(_) => carried(&mut m),
            }
        } else if lower == "explicit_tiles.txt" {
            m.explicit = Some(Item::new(path.clone(), ExplicitTiles::parse(&blob.bytes()?), blob.clone()));
        } else if lower.ends_with(".terry") && m.project.is_none() {
            match TerryProject::parse(blob.bytes()?.to_vec()) {
                Ok(p) => m.project = Some(Item::new(path.clone(), p, blob.clone())),
                Err(_) => carried(&mut m),
            }
        } else if lower.ends_with(".xml") {
            match XmlDoc::parse(blob.bytes()?.to_vec()) {
                Ok(d) => m.xml.push(Item::new(path.clone(), d, blob.clone())),
                Err(_) => carried(&mut m),
            }
        } else {
            carried(&mut m);
        }
    }

    Ok(m)
}

fn write_layer(item: &Item<CellLayer>) -> Result<Vec<u8>> {
    let l = item.get();
    let mut p = item.origin.bytes().ok().and_then(|b| PngFile::parse(&b).ok()).filter(|p| p.width == l.width() && p.height == l.height()).unwrap_or_default();
    let keep = !p.original.is_empty() && p.bit_depth == 8 && p.color_type == if l.alpha { 6 } else { 2 };
    if !keep {
        p = PngFile { width: l.width(), height: l.height(), bit_depth: 8, color_type: if l.alpha { 6 } else { 2 }, ..Default::default() };
    }

    p.pixels = l.to_pixels();
    p.pixels_edited = true;
    let out = if keep && p.has_ancillary { p.encode_keeping_chunks() } else { p.encode() };
    out.map_err(|e| file_error(&item.path, e.0))
}

fn write_lf(item: &Item<PagedU16>) -> Result<Vec<u8>> {
    let f = item.get();
    let bytes = item.origin.bytes()?;
    let mut t = TiffFile::parse(&bytes).map_err(|e| file_error(&item.path, e.0))?;
    if t.width != f.width() || t.height != f.height() {
        return Err(file_error(&item.path, "the field's size changed (a new header is not written yet)"));
    }

    let rows = f.to_le_rows();
    t.pixels[..rows.len()].copy_from_slice(&rows);
    Ok(t.write())
}

fn bytes_blob(b: Vec<u8>) -> Blob {
    Blob::Bytes(Arc::from(b))
}

/// Writes a map: the files not edited as they were read, the edited ones made again.
///
/// # Errors
///
/// An edited file could not be made (its original no longer reads).
pub fn write(m: &MapModel) -> Result<MapFiles> {
    let mut folder: BTreeMap<String, Blob> = m.carried.clone();
    for (_, l) in &m.layers {
        let blob = if l.edited() { bytes_blob(write_layer(l)?) } else { l.origin.clone() };
        folder.insert(l.path.clone(), blob);
    }

    for (_, f) in &m.lf {
        let blob = if f.edited() { bytes_blob(write_lf(f)?) } else { f.origin.clone() };
        folder.insert(f.path.clone(), blob);
    }

    if let Some(e) = &m.explicit {
        folder.insert(e.path.clone(), if e.edited() { bytes_blob(e.get().write()) } else { e.origin.clone() });
    }

    if let Some(p) = &m.project {
        folder.insert(p.path.clone(), if p.edited() { bytes_blob(p.get().save()) } else { p.origin.clone() });
    }

    for x in &m.xml {
        folder.insert(x.path.clone(), if x.edited() { bytes_blob(x.get().save()) } else { x.origin.clone() });
    }

    Ok(MapFiles { folder })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png(w: u32, h: u32, ct: u8, px: Vec<u8>) -> Vec<u8> {
        PngFile { width: w, height: h, bit_depth: 8, color_type: ct, pixels: px, ..Default::default() }.encode().expect("png")
    }

    fn tiff(w: u32, h: u32, samples: &[u16]) -> Vec<u8> {
        // TEd's layout for one 16-bit sample: the blend map header with spp 1 and the strip's size patched is not needed here; a
        // minimal little-endian TIFF does.
        let mut o = b"II*\0".to_vec();
        o.extend_from_slice(&8u32.to_le_bytes());
        let entries: [(u16, u16, u32, u32); 6] = [(256, 3, 1, w), (257, 3, 1, h), (258, 3, 1, 16), (259, 3, 1, 1), (273, 4, 1, 8 + 2 + 6 * 12 + 4), (277, 3, 1, 1)];
        o.extend_from_slice(&(entries.len() as u16).to_le_bytes());
        for (t, k, c, v) in entries {
            o.extend_from_slice(&t.to_le_bytes());
            o.extend_from_slice(&k.to_le_bytes());
            o.extend_from_slice(&c.to_le_bytes());
            o.extend_from_slice(&v.to_le_bytes());
        }

        o.extend_from_slice(&0u32.to_le_bytes());
        o.extend(samples.iter().flat_map(|s| s.to_le_bytes()));
        o.extend_from_slice(b"trailing");
        o
    }

    fn files() -> MapFiles {
        let mut folder = BTreeMap::new();
        folder.insert("tile_map.png".to_owned(), bytes_blob(png(2, 2, 2, vec![90, 118, 71, 150, 75, 50, 90, 118, 71, 0, 0, 0])));
        folder.insert("lf_heights.tif".to_owned(), bytes_blob(tiff(3, 2, &[436, 437, 438, 439, 440, 441])));
        folder.insert("explicit_tiles.txt".to_owned(), bytes_blob(b"1,1,terrain\\tiles\\battle\\a\\b,90\r\n".to_vec()));
        folder.insert("m.terry".to_owned(), bytes_blob(b"<project version=\"3\">\n    <scene version=\"10\"/>\n</project>".to_vec()));
        folder.insert("normal.dds".to_owned(), bytes_blob(b"DDS anything".to_vec()));
        MapFiles { folder }
    }

    #[test]
    fn an_lf_on_disk_is_a_view_written_back_after_detaching() {
        let dir = std::env::temp_dir().join(format!("wce_lf_view_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        for (name, blob) in &files().folder {
            std::fs::write(dir.join(name), blob.bytes().unwrap()).unwrap();
        }

        {
            let mut m = read("m", MapKind::Battle, &MapFiles::from_dir(&dir).unwrap()).unwrap();
            let item = m.lf_mut(LfRole::Land).unwrap();
            assert!(item.get().grid().is_mapped(), "a view of the file");
            assert_eq!(item.get().at(2, 1), 441);
            item.edit().set(2, 1, 7);
            let out = write(&m).unwrap();
            // As the document's save: the samples into memory, then the file written over.
            m.lf(LfRole::Land).unwrap().get().grid().detach();
            std::fs::write(dir.join("lf_heights.tif"), out.folder["lf_heights.tif"].bytes().unwrap()).unwrap();
            assert_eq!(m.lf(LfRole::Land).unwrap().get().at(2, 1), 7);
        }

        let again = read("m", MapKind::Battle, &MapFiles::from_dir(&dir).unwrap()).unwrap();
        assert_eq!(again.lf(LfRole::Land).unwrap().get().at(2, 1), 7);
        assert_eq!(again.lf(LfRole::Land).unwrap().get().at(1, 1), 440);
        drop(again);
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn same(a: &MapFiles, b: &MapFiles) -> bool {
        a.folder.len() == b.folder.len() && a.folder.iter().all(|(k, v)| b.folder.get(k).is_some_and(|w| v.bytes().ok() == w.bytes().ok()))
    }

    #[test]
    fn unedited_and_reencoded_round_trip() {
        let f = files();
        let mut m = read("m", MapKind::Battle, &f).expect("read");
        assert_eq!((m.layers.len(), m.lf.len(), m.carried.len()), (1, 1, 1));
        assert!(m.project.is_some() && m.explicit.is_some());
        assert!(same(&write(&m).expect("write"), &f));

        // Made again from the model (nothing changed): the kit's encoders give the same bytes.
        m.layers[0].1.edit();
        m.lf[0].1.edit();
        m.explicit.as_mut().expect("explicit").edit();
        m.project.as_mut().expect("project").edit();
        assert!(same(&write(&m).expect("write"), &f));
    }

    #[test]
    fn an_lf_edit_changes_two_bytes() {
        let f = files();
        let mut m = read("m", MapKind::Battle, &f).expect("read");
        m.lf_mut(LfRole::Land).expect("lf").edit().set(1, 1, 0x1234);
        let out = write(&m).expect("write");
        let (a, b) = (f.folder["lf_heights.tif"].bytes().expect("a"), out.folder["lf_heights.tif"].bytes().expect("b"));
        let diff: Vec<usize> = a.iter().zip(b.iter()).enumerate().filter(|(_, (x, y))| x != y).map(|(i, _)| i).collect();
        assert_eq!(a.len(), b.len());
        assert_eq!(diff.len(), 2);
        assert!(b.ends_with(b"trailing"));
    }
}
