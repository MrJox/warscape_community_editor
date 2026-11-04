//! A battle map in its built form, the only form a game without kit maps has (Rome 2: `terrain/battles/<map>` in its packs; docs/plans/
//! terrain_editor.md, Phase 9): the files a map's folder holds there, into the model and back.
//!
//! - `lf_height_map.dds` (L16, a 128-byte header): the land LF, editable ([`MapModel::lf`], `LfRole::Land`);
//! - `terrain.tile_list`: the placements, editable ([`MapModel::tile_list`]; written back byte for byte unedited, every version);
//! - every other file carried as it is (the tile list or tile map, `locations.blm`, the deployment files, the sea LF, the normals...).
//!
//! Written back: the files as they were, but for an edited LF its DDS (the header kept, the samples the model's) and its CHMF
//! (`lf_height_map.data`): the samples encoded as `heights::lf_height_data_rome2` does, the shipped file's version kept and each
//! region's tag maximum the shipped one or the region's new maximum, the higher (Rome 2's tags hold the tiles' relief, which the LF
//! alone does not give: with them kept, 110 of its 111 maps' CHMFs come out byte for byte from their DDS, `built_lf_check`). A shipped
//! CHMF of another region layout is kept as it was (`settlement_preset`). ASSUMPTION: a lowered region keeps its old tag maximum.

use std::collections::BTreeMap;
use std::sync::Arc;

use wce_formats::chmf::ChmfFile;
use wce_terrain::paged::PagedU16;
use wce_terrain::{Item, LfRole, MapKind, MapModel};
use wce_tile_model::Blob;

use crate::{MapFiles, Result, file_error};

const LF: &str = "lf_height_map.dds";
const CHMF: &str = "lf_height_map.data";
const LIST: &str = "terrain.tile_list";

/// The samples of an L16 DDS (a 128-byte header, then the rows): its size and field.
fn l16(bytes: &[u8]) -> Option<PagedU16> {
    let u = |o: usize| bytes.get(o..o + 4).map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]));
    if bytes.get(..4)? != b"DDS " {
        return None;
    }

    let (h, w) = (u(12)?, u(16)?);
    let rows = bytes.get(128..128 + w as usize * h as usize * 2)?;
    Some(PagedU16::from_le_rows(w, h, rows))
}

/// A built map's folder into the model.
///
/// # Errors
///
/// A file could not be read.
pub fn read(name: &str, files: &MapFiles) -> Result<MapModel> {
    let mut m = MapModel { name: name.to_owned(), kind: MapKind::Battle, built: true, ..Default::default() };
    for (path, blob) in &files.folder {
        if path.eq_ignore_ascii_case(LF)
            && let Some(f) = l16(&blob.bytes()?)
        {
            m.lf.push((LfRole::Land, Item::new(path.clone(), f, blob.clone())));
        } else if path.eq_ignore_ascii_case(LIST)
            && let Ok(t) = wce_tile::battle_map::tile_list::TileList::parse(&blob.bytes()?)
        {
            m.tile_list = Some(Item::new(path.clone(), t, blob.clone()));
        } else {
            m.carried.insert(path.clone(), blob.clone());
        }
    }

    Ok(m)
}

/// The map's size in cells: its tile list's battle size, else its tile map's.
fn cells(m: &MapModel) -> Option<(u32, u32)> {
    if let Some(t) = &m.tile_list {
        return Some((t.get().battle_size[0], t.get().battle_size[1]));
    }

    let read = |n: &str| m.carried.iter().find(|(p, _)| p.eq_ignore_ascii_case(n)).and_then(|(_, b)| b.bytes().ok());
    let i = read("tile_map.index").filter(|i| i.len() >= 12)?;
    Some((u32::from_le_bytes(i[4..8].try_into().ok()?), u32::from_le_bytes(i[8..12].try_into().ok()?)))
}

/// The CHMF of an edited LF (see the module's notes); none to keep the shipped one.
pub fn chmf(field: &PagedU16, cells: (u32, u32), shipped: Option<&[u8]>) -> Option<Vec<u8>> {
    let samples: Vec<u16> = field.to_le_rows().as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes(*c)).collect();
    let made = crate::attila::heights::lf_height_data_rome2(&samples, field.width(), field.height(), cells);
    let Some(shipped) = shipped else { return Some(made) };
    let (mut m, s) = (ChmfFile::parse(&made).ok()?, ChmfFile::parse(shipped).ok()?);
    if m.regions.len() != s.regions.len() || m.regions.iter().zip(&s.regions).any(|(a, b)| a.rect != b.rect) {
        return None;
    }

    for (r, q) in m.regions.iter_mut().zip(&s.regions) {
        r.tag[1] = r.tag[1].max(q.tag[1]);
    }

    m.max = m.max.max(s.max);
    m.version = s.version;
    Some(m.write())
}

/// The model's files: the carried ones as they were, an edited LF's DDS and CHMF made again.
///
/// # Errors
///
/// The LF's original file can't be read.
pub fn write(m: &MapModel) -> Result<MapFiles> {
    let mut folder: BTreeMap<String, Blob> = m.carried.clone();
    if let Some(t) = &m.tile_list {
        let blob = if t.edited() { Blob::Bytes(Arc::from(t.get().write())) } else { t.origin.clone() };
        folder.insert(t.path.clone(), blob);
    }
    for (_, item) in &m.lf {
        if !item.edited() {
            folder.insert(item.path.clone(), item.origin.clone());
            continue;
        }

        let f = item.get();
        let origin = item.origin.bytes()?;
        let mut dds = origin.get(..128).ok_or_else(|| file_error(&item.path, "not a DDS"))?.to_vec();
        dds.extend(f.to_le_rows());
        dds.extend_from_slice(origin.get(128 + f.width() as usize * f.height() as usize * 2..).unwrap_or_default());
        folder.insert(item.path.clone(), Blob::Bytes(Arc::from(dds)));

        let key = folder.keys().find(|p| p.eq_ignore_ascii_case(CHMF)).cloned().unwrap_or_else(|| CHMF.to_owned());
        let shipped = folder.get(&key).and_then(|b| b.bytes().ok());
        if let Some(cells) = cells(m)
            && let Some(data) = chmf(f, cells, shipped.as_deref())
        {
            folder.insert(key, Blob::Bytes(Arc::from(data)));
        }
    }

    Ok(MapFiles { folder })
}

/// The files of `written` that differ from `read` (or are new): what an export of an edited built map needs.
pub fn changed(read: &MapFiles, written: &MapFiles) -> Vec<(String, Vec<u8>)> {
    written.folder.iter().filter_map(|(p, b)| {
        let new = b.bytes().ok()?;
        let old = read.folder.get(p).and_then(|o| o.bytes().ok());
        (old.as_deref() != Some(&new[..])).then(|| (p.clone(), new.to_vec()))
    }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dds(w: u32, h: u32, samples: &[u16]) -> Vec<u8> {
        let mut v = b"DDS ".to_vec();
        let mut head = [0u32; 31];
        (head[0], head[2], head[3]) = (0x7c, h, w);
        v.extend(head.iter().flat_map(|x| x.to_le_bytes()));
        v.extend(samples.iter().flat_map(|s| s.to_le_bytes()));
        v
    }

    #[test]
    fn unedited_writes_back_and_an_edit_changes_the_dds() {
        let samples: Vec<u16> = (0..64u16).map(|i| 400 + i).collect();
        let files = MapFiles { folder: BTreeMap::from([
            (LF.to_owned(), Blob::Bytes(Arc::from(dds(8, 8, &samples)))),
            ("terrain.tile_list".to_owned(), Blob::Bytes(Arc::from(vec![1u8, 2, 3]))),
        ]) };

        let mut m = read("x", &files).unwrap();
        assert!(m.built && m.lf.len() == 1 && m.carried.len() == 1);
        assert!(changed(&files, &write(&m).unwrap()).is_empty());

        m.lf[0].1.edit().set(3, 3, 9000);
        let out = write(&m).unwrap();
        let diff = changed(&files, &out);
        assert_eq!(diff.len(), 1, "the DDS only (no size for a CHMF: the tile list does not read)");
        let back = l16(&out.folder[LF].bytes().unwrap()).unwrap();
        assert_eq!(back.at(3, 3), 9000);
        assert_eq!(back.at(2, 3), samples[3 * 8 + 2]);
    }
}
