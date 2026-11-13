//! The LF normal chunks of a battle map (`normals/normal_N.dds` in the game's packs; docs/re/terry.md, "BOB_Terrain's outputs"): 576 x
//! 576 DXT5 with mips, the normal map's window from (256 col - 32, 256 row - 32), `N = row * cols + col` over a grid of 256-pixel steps
//! (32 x 23 for `main_attila_map`), each pixel (R 255, G y, B 0, A x) of the normal map (the kit's `normal.dds`, ARGB 32-bit, the LF's
//! grid), (255, 255, 0, 127) past the map's edge.
//!
//! The game's chunks follow the kit's `normal.dds`, which is no function of the LF alone (its correlation with the LF's slopes is 0.2 to
//! 0.3: it carries more than the LF, the placed tiles' relief most likely). So a map whose LF is not edited gets no chunks (the game's
//! stay); an edited LF gets the chunks its edit reaches, from `normal.dds` where the LF is as it was, and over the edited samples (and
//! their neighbours) `normal.dds` moved by the change of the LF's own normal there (the tiles' relief kept; the LF's normal alone without
//! a `normal.dds`). A map without a `normal.dds` (Rome 2's built maps) takes its own shipped chunks as the base
//! ([`edited_shipped_chunks`]). ASSUMPTION: the LF's normals in metres (`LF * 3000 - 20`) at the editor's scale (a cell 128 m,
//! the LF's samples a cell apart in step), x as -dx, y as the south-minus-north slope (the signs the game's chunks show).

use wce_formats::image_containers::TiffFile;
use wce_terrain::paged::PagedU16;

/// A chunk's side, its step, the margin before its step's origin.
pub const SIDE: i64 = 576;
pub const STEP: i64 = 256;
pub const MARGIN: i64 = 32;

/// The value past the map's edge (RGBA).
const OUTSIDE: [u8; 4] = [255, 255, 0, 127];

/// A normal map, RGBA rows (the kit's `normal.dds`).
pub struct NormalImage {
    pub w: u32,
    pub h: u32,
    pub px: Vec<[u8; 4]>,
}

impl NormalImage {
    /// An uncompressed 32-bit DDS (B, G, R, A bytes).
    pub fn from_dds(b: &[u8]) -> Option<Self> {
        let u = |o: usize| b.get(o..o + 4).map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]));
        if b.get(..4) != Some(b"DDS ") || u(88)? != 32 {
            return None;
        }

        let (h, w) = (u(12)?, u(16)?);
        let data = b.get(128..128 + (w * h * 4) as usize)?;
        Some(Self { w, h, px: data.chunks_exact(4).map(|c| [c[2], c[1], c[0], c[3]]).collect() })
    }
}

/// The LF's samples (u16, row by row).
fn samples(f: &PagedU16) -> Vec<u16> {
    f.to_le_rows().as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes(*c)).collect()
}

/// The chunk pixel (R 255, G y, B 0, A x) of the LF's normal at a sample, `step` metres between samples.
fn lf_normal(v: &[u16], w: i64, h: i64, x: i64, y: i64, step: f64) -> [u8; 4] {
    let m = |x: i64, y: i64| f64::from(v[(y.clamp(0, h - 1) * w + x.clamp(0, w - 1)) as usize]) / 65535.0 * 3000.0 - 20.0;
    let dx = (m(x + 1, y) - m(x - 1, y)) / (2.0 * step);
    let ds = (m(x, y + 1) - m(x, y - 1)) / (2.0 * step);
    let l = (dx * dx + ds * ds + 1.0).sqrt();
    let q = |v: f64| ((v * 0.5 + 0.5).clamp(0.0, 1.0) * 255.0).round() as u8;
    [255, q(ds / l), 0, q(-dx / l)]
}

/// The chunks an LF edit reaches: (`normals/normal_N.dds`, bytes). `original`: the LF as the map's file has it; `base`: `normal.dds`;
/// `cells`: the map's size in cells. None when the LF is as it was.
pub fn chunks(lf: &PagedU16, original: &[u16], base: Option<&NormalImage>, cells: (u32, u32)) -> Vec<(String, Vec<u8>)> {
    let pixel = |_: i64, x: i64, y: i64, _: i64, _: i64| -> Option<[u8; 4]> {
        let b = base.filter(|b| x < i64::from(b.w) && y < i64::from(b.h))?;
        let q = b.px[(y * i64::from(b.w) + x) as usize];
        Some([255, q[1], 0, q[0]])
    };

    edited_chunks(lf, original, cells, Layout::ATTILA, &pixel)
}

/// Where a map's chunks lie: `side` pixels each, every `step` LF samples (one normal pixel a sample), from `margin` samples before.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Layout {
    pub side: i64,
    pub step: i64,
    pub margin: i64,
}

impl Layout {
    /// The battle maps' (Attila, Thrones, Rome 2's large maps).
    pub const ATTILA: Self = Self { side: SIDE, step: STEP, margin: MARGIN };

    /// The layout of a map's shipped chunks (`count` of them, `side` pixels, over an LF `w` x `h`): the grid every 256 samples, or one
    /// chunk of the whole LF and a 32-pixel margin. None for the others (Rome 2's small maps whose normal map is not the LF's size: one
    /// chunk of 320, 640 or 1152 pixels).
    pub fn of(count: usize, side: i64, w: i64, h: i64) -> Option<Self> {
        if side == SIDE && count as i64 == ((w + STEP - 1) / STEP) * ((h + STEP - 1) / STEP) {
            return Some(Self::ATTILA);
        }

        (count == 1 && w == h && side == w + 2 * MARGIN).then_some(Self { side, step: w, margin: MARGIN })
    }
}

/// The chunks an LF edit reaches with their pixels from `base` (chunk number, x, y, the pixel's place in the chunk), where the LF is as
/// it was, else the LF's own normals there.
fn edited_chunks(
    lf: &PagedU16,
    original: &[u16],
    cells: (u32, u32),
    layout: Layout,
    base: &dyn Fn(i64, i64, i64, i64, i64) -> Option<[u8; 4]>,
) -> Vec<(String, Vec<u8>)> {
    let (w, h) = (i64::from(lf.width()), i64::from(lf.height()));
    let now = samples(lf);
    if now.len() != original.len() || now == original {
        return vec![];
    }

    // The samples whose normal changes: those edited and their neighbours.
    let mut touched = vec![false; now.len()];
    for y in 0..h {
        for x in 0..w {
            let i = (y * w + x) as usize;
            if now[i] != original[i] {
                for (dx, dy) in [(0, 0), (1, 0), (-1, 0), (0, 1), (0, -1)] {
                    let (nx, ny) = (x + dx, y + dy);
                    if nx >= 0 && ny >= 0 && nx < w && ny < h {
                        touched[(ny * w + nx) as usize] = true;
                    }
                }
            }
        }
    }

    let step = 128.0 / (f64::from(lf.width()) / f64::from(cells.0.max(1)));
    let Layout { side, step: chunk_step, margin } = layout;
    let (cols, rows) = ((w + chunk_step - 1) / chunk_step, (h + chunk_step - 1) / chunk_step);
    let mut out = vec![];
    for row in 0..rows {
        for col in 0..cols {
            let (ox, oy) = (col * chunk_step - margin, row * chunk_step - margin);
            let reached = (oy.max(0)..(oy + side).min(h)).any(|y| (ox.max(0)..(ox + side).min(w)).any(|x| touched[(y * w + x) as usize]));
            if !reached {
                continue;
            }

            let n = row * cols + col;
            let mut bgra = Vec::with_capacity((side * side * 4) as usize);
            for sy in 0..side {
                for sx in 0..side {
                    let (x, y) = (ox + sx, oy + sy);
                    let p = if x < 0 || y < 0 || x >= w || y >= h {
                        base(n, x, y, sx, sy).unwrap_or(OUTSIDE)
                    } else if touched[(y * w + x) as usize] {
                        // The base moved by what the edit changed of the LF's normal there (the base's relief beyond the LF, the
                        // tiles', kept); the LF's normal alone without a base.
                        let new = lf_normal(&now, w, h, x, y, step);
                        match base(n, x, y, sx, sy) {
                            Some(b) => {
                                let old = lf_normal(original, w, h, x, y, step);
                                let moved = |k: usize| (i32::from(b[k]) + i32::from(new[k]) - i32::from(old[k])).clamp(0, 255) as u8;
                                [b[0], moved(1), b[2], moved(3)]
                            }

                            None => new,
                        }
                    } else {
                        base(n, x, y, sx, sy).unwrap_or_else(|| lf_normal(&now, w, h, x, y, step))
                    };

                    bgra.extend([p[2], p[1], p[0], p[3]]);
                }
            }

            out.push((format!("normals/normal_{n}.dds"), wce_tile_io::attila::bob_tile::dds_dxt5(&bgra, side as usize, side as usize)));
        }
    }

    out
}

/// The chunks an LF edit reaches on a map with no `normal.dds` (Rome 2's built maps): each from the map's own shipped chunk (`shipped`
/// gives a chunk's DDS by number), its pixels where the LF is as it was, the LF's normals over the edited samples. None when the shipped
/// chunks' layout is not one this knows ([`Layout::of`]; the game's then stay) or the LF is as it was.
pub fn edited_shipped_chunks(
    lf: &PagedU16,
    original: &[u16],
    cells: (u32, u32),
    count: usize,
    shipped: &dyn Fn(i64) -> Option<Vec<u8>>,
) -> Vec<(String, Vec<u8>)> {
    let Some(side) = shipped(0).and_then(|b| b.get(16..20).map(|s| i64::from(u32::from_le_bytes([s[0], s[1], s[2], s[3]])))) else {
        return vec![];
    };
    let Some(layout) = Layout::of(count, side, i64::from(lf.width()), i64::from(lf.height())) else { return vec![] };
    let decoded = std::cell::RefCell::new(std::collections::HashMap::<i64, Option<wce_formats::dds_decode::Rgba8Image>>::new());
    let pixel = |n: i64, _: i64, _: i64, sx: i64, sy: i64| -> Option<[u8; 4]> {
        let mut d = decoded.borrow_mut();
        let img = d.entry(n).or_insert_with(|| shipped(n).and_then(|b| wce_formats::dds_decode::dds_decode_mip(&b, 0).ok()));
        let img = img.as_ref().filter(|i| i64::from(i.w) == side && i64::from(i.h) == side)?;
        let p = img.at(sx as u32, sy as u32);
        Some([p[0], p[1], p[2], p[3]])
    };

    edited_chunks(lf, original, cells, layout, &pixel)
}

/// The LF's samples as a 16-bit TIFF holds them (the map file's, before the edits).
pub fn tiff_samples(bytes: &[u8]) -> Option<Vec<u16>> {
    let t = TiffFile::parse(bytes).ok()?;
    (t.bits_per_sample == 16 && t.samples_per_pixel == 1)
        .then(|| t.pixels.as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes(*c)).collect())
}
