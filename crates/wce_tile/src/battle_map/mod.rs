//! A battle map's tiles: the kit's battle maps (`raw_data/terrain/battles/<map>`) filled with tiles as BOB's Terrain processor does
//! (docs/re/battle_maps.md), for showing a tile in its map.

pub mod context;
pub mod fill;
pub mod link_candidates;
pub mod locations;
pub mod tile_db;
pub mod tile_list;
pub mod tile_map;

use std::path::Path;

use fill::{MapImages, TileMapFill};
use tile_db::TileDatabase;
use tile_list::{TileList, TileListPlacement};
use tile_map::TileMap;
use wce_formats::image_containers::TiffFile;
use wce_formats::png_file::PngFile;

/// RGB pixels of a PNG (8 bit RGB or RGBA), row by row.
fn png_rgb(bytes: &[u8]) -> Result<(u32, u32, Vec<[u8; 3]>), String> {
    let p = PngFile::parse(bytes).map_err(|e| e.0)?;
    let bpp = p.bytes_per_pixel();
    if p.bit_depth != 8 || !(bpp == 3 || bpp == 4) {
        return Err(format!("png: unsupported format (type {}, depth {})", p.color_type, p.bit_depth));
    }

    Ok((p.width, p.height, p.pixels.chunks_exact(bpp).map(|c| [c[0], c[1], c[2]]).collect()))
}

/// The heights of a 16-bit single-channel TIFF, as 0..1 (`* (1 / 65535)`).
fn tiff_heights(bytes: &[u8]) -> Result<(Vec<f32>, u32, u32), String> {
    let t = TiffFile::parse(bytes).map_err(|e| e.0)?;
    if t.bits_per_sample != 16 || t.samples_per_pixel != 1 {
        return Err(format!("tiff: unsupported format ({} x {} bits)", t.samples_per_pixel, t.bits_per_sample));
    }

    let scale = 1.0f32 / 65535.0;
    let v = t.pixels.as_chunks::<2>().0.iter().map(|c| f32::from(u16::from_le_bytes(*c)) * scale).collect();
    Ok((v, t.width, t.height))
}

/// Reads a kit map's images and fills its tile map (`build_tile_map`, then the low-frequency heights).
fn fill<'a>(db: &'a TileDatabase, map: &Path) -> Result<TileMapFill<'a>, String> {
    let read = |name: &str| std::fs::read(map.join(name));
    let (w, h, tiles) = png_rgb(&read("tile_map.png").map_err(|e| format!("tile_map.png: {e}"))?)?;
    let climate = read("climate_map.png").ok().and_then(|b| png_rgb(&b).ok()).filter(|c| c.0 == w && c.1 == h);
    let explicit = read("explicit_tiles.txt").ok().map(|b| b.iter().map(|&c| char::from(c)).collect::<String>());
    let lf = read("lf_heights.tif").ok().and_then(|b| tiff_heights(&b).ok());
    let images = MapImages {
        width: w, height: h, tile_map: &tiles, climate_map: climate.as_ref().map(|c| c.2.as_slice()), explicit_tiles: explicit.as_deref(),
        lf: lf.as_ref().map(|(v, fw, fh)| (v.as_slice(), *fw, *fh)),
    };

    let mut fill = TileMapFill::new(db, &images);
    fill.build(images.explicit_tiles);
    if let Some((field, fw, fh)) = images.lf {
        fill.calculate_lf_min_maxs(field, fw, fh);
    }

    Ok(fill)
}

/// Fills a kit battle map (`map`: the map's folder) and returns its tile map as `TILE_MAP::save` writes it (the campaign map's
/// `tile_map.tiles`), and BOB's messages.
///
/// # Errors
///
/// Fails when the map's tile map can't be read.
pub fn fill_kit_tile_map(db: &TileDatabase, map: &Path) -> Result<(TileMap, Vec<String>), String> {
    let fill = fill(db, map)?;
    Ok((fill.tile_map(), fill.messages))
}

/// Fills a kit battle map (`map`: the map's folder) with the tiles of `db` and returns the battle field as BOB writes it.
///
/// # Errors
///
/// Fails when the map's tile map can't be read.
pub fn fill_kit_map(db: &TileDatabase, map: &Path) -> Result<(TileList, Vec<String>), String> {
    let fill = fill(db, map)?;
    let (w, h) = (fill.w, fill.h);
    let placements = fill.battle_field();
    let mut list = TileList {
        version: 11,
        lf_position: [(f64::from(w / 2) / f64::from(w)) as f32, (f64::from(h / 2) / f64::from(h)) as f32],
        lf_scale: [1.0, 1.0],
        unknown: [500.0, 1.333],
        battle_size: [w, h],
        battle_centre: [w / 2, h / 2],
        origin_offset: [-((w / 2) as f32), -((h / 2) as f32)],
        bounds: [i32::MAX, i32::MAX, i32::MIN, i32::MIN],
        ..Default::default()
    };

    for p in &placements {
        let name = db.tile_location_of(p.tile, p.variation);
        let index = match list.names.iter().position(|n| *n == name) {
            Some(i) => i,
            None => {
                list.names.push(name);
                list.names.len() - 1
            }
        };

        let tile = &db.tiles[p.tile];
        let size = tile.width.max(tile.height);
        let (x, y) = (i32::from(p.x), i32::from(p.y));
        list.bounds = [list.bounds[0].min(x - size), list.bounds[1].min(y - size), list.bounds[2].max(x + size), list.bounds[3].max(y + size)];
        list.placements.push(TileListPlacement { name: index, x: p.x, y: p.y, flags: p.flags, climate: p.climate, lf: p.lf });
    }

    Ok((list, fill.messages))
}

/// The kit's battle maps (folders of `<raw_data>/terrain/battles` with a `tile_map.png`), sorted.
pub fn kit_maps(raw_root: &Path) -> Vec<String> {
    let dir = raw_root.join("terrain").join("battles");
    let mut out: Vec<String> = std::fs::read_dir(&dir)
        .map(|d| d.filter_map(Result::ok).filter(|e| e.path().join("tile_map.png").is_file()).map(|e| e.file_name().to_string_lossy().into_owned()).collect())
        .unwrap_or_default();
    out.sort();
    out
}

/// A kit map's snow in a season (`snow_map_<season>.png`: white cells have snow, black none, blue is sea): its size and a byte per cell
/// (1: snow), row by row; none without the file or without a snowy cell (only the campaign map's winter has them).
pub fn load_snow_map(map: &Path, season: &str) -> Option<(u32, u32, Vec<u8>)> {
    let bytes = std::fs::read(map.join(format!("snow_map_{season}.png"))).ok()?;
    let (w, h, px) = png_rgb(&bytes).ok()?;
    let cells: Vec<u8> = px.iter().map(|p| u8::from(p[0] > 200 && p[1] > 200 && p[2] > 200)).collect();
    cells.contains(&1).then_some((w, h, cells))
}

/// A kit map filled as BOB does, with its `lf_heights.tif`.
///
/// # Errors
///
/// The map's tile map can't be read.
pub fn load_kit_map(db: &TileDatabase, map: &Path) -> Result<context::PlacedMap, String> {
    let (list, _) = fill_kit_map(db, map)?;
    let lf = std::fs::read(map.join("lf_heights.tif")).ok().and_then(|b| TiffFile::parse(&b).ok()).filter(|t| t.bits_per_sample == 16 && t.samples_per_pixel == 1)
        .map(|t| context::LfField { w: t.width, h: t.height, values: t.pixels.as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes(*c)).collect() });
    let name = map.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    Ok(context::PlacedMap::from_tile_list(&name, &list, db, lf))
}

/// A 16-bit single-channel DDS (BOB's `lf_height_map.dds`: a 128-byte header, then the rows).
pub fn l16_dds_heights(b: &[u8]) -> Option<context::LfField> {
    let u32_at = |p: usize| b.get(p..p + 4).map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]));
    if b.get(..4)? != b"DDS " {
        return None;
    }

    let (h, w) = (u32_at(12)?, u32_at(16)?);
    let data = b.get(128..128 + w as usize * h as usize * 2)?;
    Some(context::LfField { w, h, values: data.as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes(*c)).collect() })
}

/// A shipped map (the campaign's `terrain/battles/main_attila_map`): its `tile_map.index` / `.tiles` and `lf_height_map.dds`.
///
/// # Errors
///
/// The tile map doesn't read.
pub fn load_game_map(name: &str, db: &TileDatabase, index: &[u8], tiles: &[u8], lf_dds: Option<&[u8]>) -> Result<context::PlacedMap, String> {
    let map = TileMap::parse(index, tiles)?;
    Ok(context::PlacedMap::from_tile_map(name, &map, db, lf_dds.and_then(l16_dds_heights)))
}
