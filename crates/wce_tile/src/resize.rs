//! Resizing a tile's maps (Phase 6b of docs/plans/tile_ted_parity.md; TEd's Resize Tile, `TileResizingEditor`): the old tile placed in the
//! new size at an offset of whole cells (centred, or anywhere when growing; a window of it when shrinking), every map of the tile's size
//! (`height_map_0.png`, `blend_map.tif`, `ground_type_map.png` and any other image of their size) copied there, border included. What the
//! new size adds is, as TEd leaves it, what a new terrain has (`Fill::Flat`: the variation's base height, the first texture group, no
//! ground type), or the old edge carried out (`Fill::Extend`, no cliff at the old border). The scene is moved by the document.

use std::path::{Path, PathBuf};

use wce_formats::image_containers::TiffFile;
use wce_formats::png_file::PngFile;

/// What the new area gets.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fill {
    Flat,
    Extend,
}

/// A resize: the new size in cells and where the old tile's cell (0, 0) lands in the new one (cells along x and along z, the maps' rows).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Resize {
    pub cells: (u32, u32),
    pub offset: (i32, i32),
    pub fill: Fill,
}

impl Resize {
    /// The offset that centres the old tile along the axes asked for (whole cells, rounded down), at 0 along the others.
    pub fn centred(old: (u32, u32), new: (u32, u32), centre: (bool, bool), fill: Fill) -> Self {
        let c = |o: u32, n: u32, on: bool| if on { (n as i32 - o as i32).div_euclid(2) } else { 0 };
        Self { cells: new, offset: (c(old.0, new.0, centre.0), c(old.1, new.1, centre.1)), fill }
    }
}

/// Copies a map of `bpp` bytes a pixel and `old` pixels into a `new` one, the old pixel (x, y) going to (x + dx, y + dy); the rest is
/// `fill` (a pixel's bytes), or the nearest old pixel with `extend`.
fn place(pixels: &[u8], bpp: usize, old: (u32, u32), new: (u32, u32), (dx, dy): (i64, i64), fill: &[u8], extend: bool) -> Vec<u8> {
    let (ow, oh) = (i64::from(old.0), i64::from(old.1));
    let mut out = Vec::with_capacity(new.0 as usize * new.1 as usize * bpp);
    for y in 0..i64::from(new.1) {
        for x in 0..i64::from(new.0) {
            let (sx, sy) = (x - dx, y - dy);
            let inside = (0..ow).contains(&sx) && (0..oh).contains(&sy);
            if inside || extend {
                let (sx, sy) = (sx.clamp(0, ow - 1), sy.clamp(0, oh - 1));
                let at = ((sy * ow + sx) as usize) * bpp;
                out.extend_from_slice(&pixels[at..at + bpp]);
            } else {
                out.extend_from_slice(fill);
            }
        }
    }

    out
}

/// Resizes the maps of a saved tile folder (`density` pixels a cell; the maps are `(cells + 2) * density` a side). Each file replaced gets
/// a `.resize.bak` beside it. Returns what was done, and the images of other sizes left as they were.
///
/// # Errors
///
/// A map can't be read or written.
pub fn resize_maps(dir: &Path, old_cells: (u32, u32), density: u32, r: &Resize) -> Result<String, String> {
    let size = |c: (u32, u32)| ((c.0 + 2) * density, (c.1 + 2) * density);
    let (old, new) = (size(old_cells), size(r.cells));
    let shift = (i64::from(r.offset.0) * i64::from(density), i64::from(r.offset.1) * i64::from(density));
    let extend = r.fill == Fill::Extend;
    let mut done = vec![];
    let mut left = vec![];
    let backup = |p: &Path| -> Result<(), String> {
        let bak = PathBuf::from(format!("{}.resize.bak", p.display()));
        std::fs::copy(p, &bak).map(|_| ()).map_err(|e| format!("{}: {e}", bak.display()))
    };

    let mut files = std::fs::read_dir(dir).map_err(|e| e.to_string())?.flatten().map(|f| f.path()).filter(|p| p.is_file()).collect::<Vec<_>>();
    files.sort();
    for path in files {
        let name = path.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
        let bytes_of = || std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()));
        if name.ends_with(".png") {
            let png = PngFile::parse(&bytes_of()?).map_err(|e| format!("{name}: {e}"))?;
            if (png.width, png.height) != old {
                left.push(name);
                continue;
            }

            let bpp = png.bytes_per_pixel();
            // A new terrain: height 0 (the base), no ground type (black, opaque), else zero.
            let mut fill = vec![0u8; bpp];
            if name == "ground_type_map.png" && bpp == 4 {
                fill[3] = 255;
            }

            let mut out = png.clone();
            out.width = new.0;
            out.height = new.1;
            out.pixels = place(&png.pixels, bpp, old, new, shift, &fill, extend);
            out.pixels_edited = true;
            let bytes = if out.has_ancillary { out.encode() } else { out.write() }.map_err(|e| format!("{name}: {e}"))?;
            backup(&path)?;
            std::fs::write(&path, bytes).map_err(|e| format!("{}: {e}", path.display()))?;
            done.push(name);
        } else if name == "blend_map.tif" {
            let tif = TiffFile::parse(&bytes_of()?).map_err(|e| format!("{name}: {e}"))?;
            if (tif.width, tif.height) != old {
                left.push(name);
                continue;
            }

            // 16 samples of 16 bits; a new terrain is all the first texture group.
            let bpp = (tif.samples_per_pixel * tif.bits_per_sample / 8) as usize;
            let mut fill = vec![0u8; bpp];
            fill[..2].copy_from_slice(&u16::MAX.to_le_bytes());
            let mut bytes = TiffFile::ted_header(new.0, new.1);
            bytes.extend(place(&tif.pixels, bpp, old, new, shift, &fill, extend));
            backup(&path)?;
            std::fs::write(&path, bytes).map_err(|e| format!("{}: {e}", path.display()))?;
            done.push(name);
        } else if [".dds", ".tga", ".tif", ".data"].iter().any(|x| name.ends_with(x)) {
            left.push(name);
        }
    }

    let mut msg = format!("resized to {} x {} cells: {}", r.cells.0, r.cells.1, done.join(", "));
    if !left.is_empty() {
        msg += &format!("; left as they were (other sizes, or the export's: made again at export): {}", left.join(", "));
    }

    Ok(msg)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn placing_copies_shifts_and_fills() {
        // A 2 x 2 map of one byte a pixel into 4 x 3, one pixel right and one down.
        let old = [1u8, 2, 3, 4];
        let flat = place(&old, 1, (2, 2), (4, 3), (1, 1), &[9], false);
        assert_eq!(flat, vec![9, 9, 9, 9, 9, 1, 2, 9, 9, 3, 4, 9]);
        let ext = place(&old, 1, (2, 2), (4, 3), (1, 1), &[9], true);
        assert_eq!(ext, vec![1, 1, 2, 2, 1, 1, 2, 2, 3, 3, 4, 4]);

        // Shrinking: a window of the old map.
        let small = place(&old, 1, (2, 2), (1, 1), (-1, -1), &[9], false);
        assert_eq!(small, vec![4]);
    }

    #[test]
    fn centring_rounds_down() {
        let r = Resize::centred((2, 2), (5, 2), (true, true), Fill::Flat);
        assert_eq!(r.offset, (1, 0));
        let r = Resize::centred((4, 4), (2, 4), (true, false), Fill::Flat);
        assert_eq!(r.offset, (-1, 0));
    }
}
