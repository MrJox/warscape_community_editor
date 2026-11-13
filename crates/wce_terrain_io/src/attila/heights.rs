//! BOB_Terrain's `Heights & Normals` outputs from a map's LF fields (docs/re/terry.md, "BOB_Terrain's outputs"):
//!
//! - `lf_height_map.dds`: L16, no mips, each sample the TIFF's through `trunc(f32(f32(a * f32(1 / 65535)) * f32(65535)))` (one step lower
//!   now and then);
//! - `lf_sea_height_map.dds`: the same of the sea field, with a mip chain down to a side of 2 (`floor(log2(min side))` levels): each level
//!   from the float level above, `f32(((a + b) + c) + d) * 0.25` over the 2 x 2 block (floor halving), truncated as level 0;
//! - `lf_height_map.data`: CHMF version 5 of the samples, the header's min the lowest sample and max 65535, spacing 1, regions of 16 x 16
//!   cells (`cells` the map's size in cells, the samples a cell from the field's size; whole regions, the last ones reaching past the
//!   map with its edge samples), each region encoded by `chmf::encode_region` with BOB's tag `[region min, 65535]`; the samples converted as the DDS's;
//! - a campaign map's: `lf_height_map.dds` and `lf_height_map_lq.dds` the LF's 2 x 2 and 4 x 4 box averages (`campaign_level`), its
//!   `lf_height_map.data` a sample a cell (`campaign_lf_data`).

use wce_formats::chmf::{ChmfFile, ChmfRegion, encode_region};
use wce_terrain::paged::PagedU16;

/// Map cells a side of a CHMF region.
const REGION_CELLS: u32 = 16;

/// The level-0 value of a sample (the converter's float round trip, truncated).
pub fn convert(a: u16) -> u16 {
    let inv = 1.0f32 / 65535.0;
    ((f32::from(a) * inv) * 65535.0) as u16
}

/// The 128-byte DDS header of an L16 texture: none or a mip chain.
fn l16_header(w: u32, h: u32, mips: Option<u32>) -> Vec<u8> {
    let mut v = [0u32; 31];
    v[0] = 0x7c;
    v[2] = h;
    v[3] = w;
    v[18] = 0x20;
    v[19] = 0x2_0000;
    v[21] = 0x10;
    v[22] = 0xffff;
    if let Some(m) = mips {
        v[1] = 0x2_1007;
        v[6] = m;
        v[26] = 0x40_1008;
    }

    let mut out = b"DDS ".to_vec();
    out.extend(v.iter().flat_map(|x| x.to_le_bytes()));
    out
}

/// The field's samples row by row (row 0 the file's first).
fn rows(field: &PagedU16) -> Vec<u16> {
    field.to_le_rows().as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes(*c)).collect()
}

/// `lf_height_map.dds`.
pub fn lf_height_dds(field: &PagedU16) -> Vec<u8> {
    let mut out = l16_header(field.width(), field.height(), None);
    out.extend(rows(field).into_iter().flat_map(|a| convert(a).to_le_bytes()));
    out
}

/// `lf_sea_height_map.dds`.
pub fn lf_sea_height_dds(field: &PagedU16) -> Vec<u8> {
    let (mut w, mut h) = (field.width(), field.height());
    let levels = w.min(h).max(1).ilog2().max(1);
    let mut out = l16_header(w, h, Some(levels));
    let inv = 1.0f32 / 65535.0;
    let mut level: Vec<f32> = rows(field).into_iter().map(|a| f32::from(a) * inv).collect();
    let put = |out: &mut Vec<u8>, level: &[f32]| out.extend(level.iter().flat_map(|&v| ((v * 65535.0) as u16).to_le_bytes()));
    put(&mut out, &level);
    for _ in 1..levels {
        let (nw, nh) = ((w / 2).max(1), (h / 2).max(1));
        let at = |x: u32, y: u32| level[(y.min(h - 1) * w + x.min(w - 1)) as usize];
        let next: Vec<f32> = (0..nw * nh)
            .map(|i| {
                let (x, y) = (2 * (i % nw), 2 * (i / nw));
                (((at(x, y) + at(x + 1, y)) + at(x, y + 1)) + at(x + 1, y + 1)) * 0.25
            })
            .collect();

        put(&mut out, &next);
        (level, w, h) = (next, nw, nh);
    }

    out
}

/// `lf_height_map.data` for a map of `cells` (width, height in cells): the samples as the DDS has them (converted).
pub fn lf_height_data(field: &PagedU16, cells: (u32, u32)) -> Vec<u8> {
    let all: Vec<u16> = rows(field).into_iter().map(convert).collect();
    lf_height_data_of(&all, field.width(), field.height(), cells)
}

/// Level `k` of the campaign's LF: each sample the `2^k` x `2^k` block of the field's samples as 0 .. 1 (`a * f32(1 / 65535)`), summed
/// row by row from 0 in f32, divided by the block's size, times 65535, truncated (BOB's `lf_height_map_lq.dds`, `k` 2, exactly).
fn campaign_level(field: &PagedU16, k: u32) -> (Vec<u16>, u32, u32) {
    let (w, h) = (field.width(), field.height());
    let n = 1u32 << k;
    let (nw, nh) = ((w / n).max(1), (h / n).max(1));
    let inv = 1.0f32 / 65535.0;
    let level: Vec<f32> = rows(field).into_iter().map(|a| f32::from(a) * inv).collect();
    let area = (n * n) as f32;
    let v = (0..nw * nh)
        .map(|i| {
            let (x, y) = (n * (i % nw), n * (i / nw));
            let mut sum = 0f32;
            for dy in 0..n {
                for dx in 0..n {
                    sum += level[((y + dy).min(h - 1) * w + (x + dx).min(w - 1)) as usize];
                }
            }

            (sum / area * 65535.0) as u16
        })
        .collect();

    (v, nw, nh)
}

/// A campaign map's `lf_height_map.dds` (`k` 1) and `lf_height_map_lq.dds` (`k` 2): `campaign_level` `k`, no mips.
pub fn campaign_lf_dds(field: &PagedU16, k: u32) -> Vec<u8> {
    let (v, w, h) = campaign_level(field, k);
    let mut out = l16_header(w, h, None);
    out.extend(v.iter().flat_map(|a| a.to_le_bytes()));
    out
}

/// A campaign map's `lf_height_map.data`: the CHMF of a sample a cell, the LF's sample at each cell's corner (every `n`-th, `n` the LF's
/// samples a cell; converted as the DDS's); its header's two counts 1 and `n` (BOB's: regions of 16 x 16 samples all the same; ASSUMPTION
/// the samples a cell and the LF's samples a cell).
pub fn campaign_lf_data(field: &PagedU16, cells: (u32, u32)) -> Vec<u8> {
    let n = (field.width() / cells.0.max(1)).max(1);
    let (fw, fh) = (field.width(), field.height());
    let (w, h) = ((fw / n).max(1), (fh / n).max(1));
    // The samples of whole regions: those past the map the LF's edge samples.
    let (ew, eh) = (w.div_ceil(REGION_CELLS) * REGION_CELLS, h.div_ceil(REGION_CELLS) * REGION_CELLS);
    let all = rows(field);
    let v: Vec<u16> = (0..ew * eh).map(|i| convert(all[(((i / ew) * n).min(fh - 1) * fw + ((i % ew) * n).min(fw - 1)) as usize])).collect();
    let mut f = ChmfFile::parse(&lf_height_data_of(&v, ew, eh, (ew, eh))).unwrap_or_else(|_| ChmfFile {
        version: 5,
        width: w,
        height: h,
        cells_x: 1,
        cells_y: 1,
        min: 0.0,
        max: 65535.0,
        spacing: 1,
        regions: vec![],
    });
    (f.width, f.height, f.cells_x, f.cells_y) = (w, h, 1, n);
    f.write()
}

/// `lf_height_map.data` of samples (`w` x `h`, converted already), as BOB writes it (the header's max and each region tag's second
/// value 65535).
pub fn lf_height_data_of(all: &[u16], w: u32, h: u32, cells: (u32, u32)) -> Vec<u8> {
    chmf_of(all, w, h, cells, true)
}

/// The same with the header's max the highest sample and the regions' tags as encoded (Rome 2's packs).
pub fn lf_height_data_rome2(all: &[u16], w: u32, h: u32, cells: (u32, u32)) -> Vec<u8> {
    chmf_of(all, w, h, cells, false)
}

fn chmf_of(all: &[u16], w: u32, h: u32, cells: (u32, u32), bob: bool) -> Vec<u8> {
    let (cx, cy) = ((w / cells.0.max(1)).max(1), (h / cells.1.max(1)).max(1));
    let min = all.iter().copied().min().unwrap_or(0);
    let max = if bob { 65535.0 } else { f32::from(all.iter().copied().max().unwrap_or(0)) };
    let mut f =
        ChmfFile { version: 5, width: w, height: h, cells_x: cx, cells_y: cy, min: f32::from(min), max, spacing: 1, regions: vec![] };
    let (rw, rh) = (cells.0.div_ceil(REGION_CELLS), cells.1.div_ceil(REGION_CELLS));
    for ry in 0..rh {
        for rx in 0..rw {
            // Always a whole region: the last row and column reach past the map (BOB's campaign map: rows 1424 .. 1440 of 1437), their
            // samples past it the edge's.
            let rect = [rx * REGION_CELLS, ry * REGION_CELLS, (rx + 1) * REGION_CELLS, (ry + 1) * REGION_CELLS];
            let (sw, sh) = ((rect[2] - rect[0]) * cx, (rect[3] - rect[1]) * cy);
            let (sx, sy) = (rect[0] * cx, rect[1] * cy);
            let samples: Vec<u16> = (0..sh)
                .flat_map(|y| (0..sw).map(move |x| (x, y)))
                .map(|(x, y)| all[((sy + y).min(h - 1) * w + (sx + x).min(w - 1)) as usize])
                .collect();
            let (tag, offsets, data) = encode_region(sw, sh, &samples);
            f.regions.push(ChmfRegion { rect, tag: if bob { [tag[0], 65535] } else { tag }, offsets, data });
        }
    }

    f.write()
}
