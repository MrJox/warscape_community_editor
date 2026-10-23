//! Stage B: BOB's Tile processor (BOB_Tile, ToolDataBuilderDll), from a project's maps to the exported tile's textures. The blend and
//! index maps (`TOOLDATABUILDER::convert_blend_map`, `TerrainBlendMap.cpp`), the ground types, and BOB's DDS writer
//! (`TOOLDATABUILDER::write_dds_ATI_compressor`: mips by a 2 x 2 box, ATI's DXT5 compressor per level). docs/formats/tile_compile.md.

use wce_formats::chmf::{ChmfFile, ChmfRegion, encode_region};
use wce_tile_model::{MapKind, Node, Samples, Tile};

use super::ati_dxt::compress_dxt5;
use super::decompile::{GROUND_COLOURS, ground_type_of_group};

/// BOB's reduction of `blend_map.tif` (16 weights per pixel, 16 bit, channel 4 g + v) to the blend and index maps (BGRA bytes, the
/// same size), `WARSCAPE::terrain_build_blend_pixel` (ToolDataBuilderDll `FUN_10027430`): a group's weight is the sum of its four
/// variations (the weights as `IMAGE::convert` gives them, / 65535), times 255, truncated; its index the variations weighted 0,
/// 0.33333, 0.66666, 1 over that sum, times 255, truncated. Group red goes to R, green to G, blue to B, alpha to A.
pub fn blend_and_index(weights: &[u16], w: usize, h: usize) -> (Vec<u8>, Vec<u8>) {
    const W1: f32 = 0.333_33;
    const W2: f32 = 0.666_66;
    let mut blend = vec![0u8; w * h * 4];
    let mut index = vec![0u8; w * h * 4];
    for p in 0..w * h {
        // `IMAGE::convert` scales by the f32 of 1 / 65535 (1.5259022e-05, a hair below it: 514 / 65535 * 255 truncates to 1).
        let f = |c: usize| f32::from(weights[p * 16 + c]) * (1.0f32 / 65535.0);
        // BGRA byte of each group (red, green, blue, alpha).
        for (g, byte) in [(0usize, 2usize), (1, 1), (2, 0), (3, 3)] {
            let (v0, v1, v2, v3) = (f(4 * g), f(4 * g + 1), f(4 * g + 2), f(4 * g + 3));
            let sum = v1 + v0 + v2 + v3;
            blend[p * 4 + byte] = (sum * 255.0) as i32 as u8;
            let s = if sum == 0.0 { 1.0 } else { sum };
            // The last group's first variation: its weight times 0.0 (`fVar4 * 0.0`), like the others' (0.0 * w0).
            let weighted = v1 * W1 + v0 * 0.0 + v2 * W2 + v3;
            index[p * 4 + byte] = ((weighted / s) * 255.0) as i32 as u8;
        }
    }

    (blend, index)
}

/// `ground_types.dds`'s pixels (the tile without its 128-pixel border): per pixel the strongest group of the blend map (ties to red,
/// green, blue, alpha) as the entry's ground types of the four groups, then the painted ground types (`ground_type_map.png`, any colour
/// but black: `ground_type_of` gives its type) over them.
pub fn ground_types(blend: &[u8], w: usize, h: usize, group_types: [u8; 4], painted: Option<&dyn Fn(usize, usize) -> Option<u8>>) -> Vec<u8> {
    let (ow, oh) = (w - 256, h - 256);
    let mut out = vec![0u8; ow * oh];
    for y in 0..oh {
        for x in 0..ow {
            let o = ((y + 128) * w + x + 128) * 4;
            let (b, g, r, a) = (blend[o], blend[o + 1], blend[o + 2], blend[o + 3]);
            let t = if r >= b && r >= g && r >= a {
                group_types[0]
            } else if g >= b && g >= r && g >= a {
                group_types[1]
            } else if b >= r && b >= g && b >= a {
                group_types[2]
            } else {
                group_types[3]
            };
            out[y * ow + x] = painted.and_then(|f| f(x + 128, y + 128)).unwrap_or(t);
        }
    }

    out
}

/// The next mip level of a BGRA image (ToolDataBuilderDll `FUN_102d1330`): each pixel the rounded mean of a 2 x 2 box, (sum + 2) >> 2
/// per channel, the box clamped to the last row and column.
pub fn half(bgra: &[u8], w: usize, h: usize) -> (Vec<u8>, usize, usize) {
    let (nw, nh) = ((w / 2).max(1), (h / 2).max(1));
    let mut out = vec![0u8; nw * nh * 4];
    for y in 0..nh {
        for x in 0..nw {
            let (x0, y0) = (2 * x, 2 * y);
            let (x1, y1) = ((x0 + 1).min(w - 1), (y0 + 1).min(h - 1));
            for c in 0..4 {
                let px = |xx: usize, yy: usize| u32::from(bgra[(yy * w + xx) * 4 + c]);
                out[(y * nw + x) * 4 + c] = ((px(x0, y0) + px(x1, y0) + px(x0, y1) + px(x1, y1) + 2) >> 2) as u8;
            }
        }
    }

    (out, nw, nh)
}

/// A DXT5 DDS as BOB writes it: the header (flags 0xa1007, the first level's size as the linear size, caps 0x401008) and the levels
/// down to a side of 1 pixel short of the logarithm (log2 of the larger side, truncated: 8 levels for 384), each compressed by ATI's
/// codec from the box-filtered level above.
pub fn dds_dxt5(bgra: &[u8], w: usize, h: usize) -> Vec<u8> {
    let levels = (w.max(h) as f64).log2() as u32;
    let size_of = |w: usize, h: usize| w.div_ceil(4).max(1) * h.div_ceil(4).max(1) * 16;
    let mut out = Vec::new();
    out.extend(b"DDS ");
    let mut header = [0u32; 31];
    header[0] = 0x7c;
    header[1] = 0xa1007;
    header[2] = h as u32;
    header[3] = w as u32;
    header[4] = size_of(w, h) as u32;
    header[6] = levels;
    header[18] = 0x20;
    header[19] = 4;
    header[20] = u32::from_le_bytes(*b"DXT5");
    header[26] = 0x401008;
    for v in header {
        out.extend(v.to_le_bytes());
    }

    let (mut img, mut lw, mut lh) = (bgra.to_vec(), w, h);
    for level in 0..levels {
        out.extend(compress_dxt5(&img, lw, lh));
        if level + 1 < levels {
            (img, lw, lh) = half(&img, lw, lh);
        }
    }

    out
}

/// BOB's normal map of the heights (ToolDataBuilderDll `FUN_10021300`, `TileRawData.cpp`'s `calculate_normal`; BGRA, the height map's
/// size, written as `tile_normal.tga`): the heights in metres (the 16-bit samples x the f32 of 1 / 65535, x scale + min height), per
/// pixel the Sobel gradients (x: 1 0 -1 / 2 0 -2 / 1 0 -1, y its transpose, the samples clamped at the edges) / 9, the vector
/// (gx, gy, 1 / normal strength) normalised, each component (n + 1) x 0.5 x 255 truncated: R = x, G = y, B = z, A 255.
///
/// All in single precision: BOB's x87 runs at 24-bit precision here (a Direct3D device's FPU setting, most likely), every operation
/// rounded to f32 (terrain_rolling_hills_med/2x2/ae's pixel (325, 437): 114 so, 113 in double).
pub fn normal_map(heights: &[u16], w: usize, h: usize, scale: f32, min_height: f32, strength: f32) -> Vec<u8> {
    let metres = heights.iter().map(|&v| f32::from(v) * (1.0f32 / 65535.0) * scale + min_height).collect::<Vec<_>>();
    let mut out = vec![0u8; w * h * 4];
    for y in 0..h {
        for x in 0..w {
            let [nx, ny, nz] = calculate_normal(&metres, w, h, x, y, strength);
            let byte = |c: f32| ((c + 1.0) * 0.5 * 255.0) as i32 as u8;
            out[(y * w + x) * 4..(y * w + x) * 4 + 4].copy_from_slice(&[byte(nz), byte(ny), byte(nx), 255]);
        }
    }

    out
}

/// TileRawData.cpp's `calculate_normal` (ToolDataBuilderDll `FUN_10022750`) at a sample of a field of heights: the Sobel gradients
/// (x: 1 0 -1 / 2 0 -2 / 1 0 -1, y its transpose, samples clamped at the edges) / 9, the vector (gx, gy, 1 / strength) normalised, in
/// f32 (see [`normal_map`]).
pub fn calculate_normal(field: &[f32], w: usize, h: usize, x: usize, y: usize, strength: f32) -> [f32; 3] {
    const K: [[f32; 3]; 3] = [[1.0, 0.0, -1.0], [2.0, 0.0, -2.0], [1.0, 0.0, -1.0]];
    let at = |x: isize, y: isize| field[(y.clamp(0, h as isize - 1) as usize) * w + x.clamp(0, w as isize - 1) as usize];
    let z = 1.0f32 / strength;
    let (mut gx, mut gy) = (0.0f32, 0.0f32);
    for (dy, row) in K.iter().enumerate() {
        for (dx, k) in row.iter().enumerate() {
            let v = at(x as isize + dx as isize - 1, y as isize + dy as isize - 1);
            gx += k * v;
            gy += K[dx][dy] * v;
        }
    }

    let (gx, gy) = (gx / 9.0, gy / 9.0);
    let len = gy * gy + gx * gx + z * z;
    let inv = 1.0f32 / len.sqrt();
    [gx * inv, gy * inv, inv * z]
}

/// `normal.dds` from the normal map (ToolDataBuilderDll `FUN_10087420`): each pixel to (B 0, G y, R 255, A x), DXT5; none when the map is
/// flat (every pixel 127, 127, 255: BOB points the variation at `terrain/textures/common/flat_normal.dds` instead).
pub fn normal_dds(normals: &[u8], w: usize, h: usize) -> Option<Vec<u8>> {
    if normals.as_chunks::<4>().0.iter().all(|p| *p == [255, 127, 127, 255]) {
        return None;
    }

    let swizzled = normals.as_chunks::<4>().0.iter().flat_map(|p| [0, p[1], 255, p[2]]).collect::<Vec<_>>();
    Some(dds_dxt5(&swizzled, w, h))
}

/// Samples per cell of a battle tile's height map.
pub const CELL: usize = 128;

/// TEd's height influence of a tile alone (`WS_SCENE_NODE_TERRAIN_EDITOR_V2::build_hf_influence_map`, Warscape.AssemblyKit.dll, no
/// tile links): `cw * k + 1` by `ch * k + 1` values, 0 at the tile's edges, 1 inside. Per cell, each side and corner is 1 when a cell
/// of the tile lies past it (a corner also when its two sides differ); per sample, `lerp(i)` runs from a side's value to 1 over the
/// first half of the cell (`t = clamp(i - 2, 0, 5000) / ((k + 0.5) / 2)`) and back to the far side's value over the second half; the
/// value is `lerp_y x lerp_x x lerp_y(lerp_x of the corners)` x 2.1, squared, and the influence `(1 - 1 / e^v) / 0.98784` (e as an f32).
pub fn influence(k: usize, cw: usize, ch: usize) -> Vec<f32> {
    let half = (k as f32 + 0.5) * 0.5;
    let lerp = |s: usize, a: f32, b: f32| {
        if ((k + 1) as f32) * 0.5 > s as f32 {
            let t = (s as f32 - 2.0).clamp(0.0, 5000.0) / half;
            (1.0 - a) * t + a
        } else {
            let t = ((k - s) as f32 - 2.0).clamp(0.0, 5000.0) / half;
            (1.0 - b) * t + b
        }
    };
    let w = cw * k + 1;
    let mut out = vec![0.0f32; w * (ch * k + 1)];
    for y in 0..ch {
        for x in 0..cw {
            let flag = |inside: bool| if inside { 1.0f32 } else { 0.0 };
            let (xm, xp, ym, yp) = (flag(x > 0), flag(x + 1 < cw), flag(y > 0), flag(y + 1 < ch));
            let corner = |c: f32, a: f32, b: f32| if c == 0.0 && a != b { 1.0 } else { c };
            let c00 = corner(flag(x > 0 && y > 0), xm, ym);
            let c10 = corner(flag(x + 1 < cw && y > 0), xp, ym);
            let c01 = corner(flag(x > 0 && y + 1 < ch), xm, yp);
            let c11 = corner(flag(x + 1 < cw && y + 1 < ch), xp, yp);
            for j in 0..=k {
                let ly = lerp(j, ym, yp);
                for i in 0..=k {
                    let lx = lerp(i, xm, xp);
                    let c = lerp(j, lerp(i, c00, c10), lerp(i, c01, c11));
                    let v = ly * lx * c * 2.1;
                    let e = f64::from(f32::from_bits(0x402d_f854)).powf(f64::from(v * v)) as f32;
                    out[(y * k + j) * w + x * k + i] = (1.0 - 1.0 / e) / f32::from_bits(0x3f7c_e366);
                }
            }
        }
    }

    out
}

/// BOB's final heights (`final_heights.dds`): the height map without its one-cell border, in metres (as [`normal_map`] reads them),
/// times the tile's [`influence`]. With the size, `(w / 128 - 2) * 128 + 1` by the same of `h`.
pub fn final_heights(heights: &[u16], w: usize, h: usize, scale: f32, min_height: f32) -> (Vec<f32>, usize, usize) {
    let (cw, ch) = (w / CELL - 2, h / CELL - 2);
    let (fw, fh) = (cw * CELL + 1, ch * CELL + 1);
    let alpha = influence(CELL, cw, ch);
    let mut out = Vec::with_capacity(fw * fh);
    for y in 0..fh {
        for x in 0..fw {
            let v = f32::from(heights[(y + CELL) * w + x + CELL]) * (1.0f32 / 65535.0) * scale + min_height;
            out.push(v * alpha[y * fw + x]);
        }
    }

    (out, fw, fh)
}

/// `hf_height_map.data` from heights in metres (`COMPRESSED_HEIGHT_MAP::convert`, Warscape.AssemblyKit.dll 0x104ac240, with one metre
/// per sample and one sample per cell): CHMF version 5, the heights' min and max, each sample `(h - min) / (max - min) x 65535`
/// truncated (0 when flat), one region over the whole map. The converter pads the samples to a multiple of 16 plus one; battle
/// tiles never need it.
///
/// BOB's heights are not [`final_heights`]: it rasterises the terrain mesh it decimated from them (ToolDataBuilderDll `FUN_1001bf20`),
/// then raises the buildings' height meshes into it (`rasterise_max_heights`).
pub fn chmf(heights: &[f32], w: usize, h: usize) -> Vec<u8> {
    assert!((w - 1).is_multiple_of(16) && (h - 1).is_multiple_of(16), "the CHMF converter would pad {w} x {h}");
    let min = heights.iter().copied().fold(f32::MAX, f32::min);
    let max = heights.iter().copied().fold(f32::MIN, f32::max);
    let range = max - min;
    let samples = heights.iter().map(|&v| if range == 0.0 { 0 } else { ((v - min) / range * 65535.0) as i32 as u16 }).collect::<Vec<_>>();
    let (tag, offsets, data) = encode_region(w as u32, h as u32, &samples);
    let file = ChmfFile {
        version: 5,
        width: w as u32,
        height: h as u32,
        cells_x: 1,
        cells_y: 1,
        min,
        max,
        spacing: 1,
        regions: vec![ChmfRegion { rect: [0, 0, w as u32, h as u32], tag, offsets, data }],
    };
    file.write()
}

/// A DDS of 8-bit luminance without mips, as `IMAGE::save_raw_to_file` writes `ground_types.dds`.
pub fn dds_l8(px: &[u8], w: usize, h: usize) -> Vec<u8> {
    let mut header = [0u32; 31];
    header[0] = 0x7c;
    header[2] = h as u32;
    header[3] = w as u32;
    header[18] = 0x20;
    header[19] = 0x20000;
    header[21] = 8;
    header[22] = 0xff;
    let mut out = b"DDS ".to_vec();
    for v in header {
        out.extend(v.to_le_bytes());
    }

    out.extend_from_slice(px);
    out
}

/// The variation's numbers BOB reads the height map with.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HeightNumbers {
    pub scale: f32,
    pub min_height: f32,
    pub normal_strength: f32,
}

impl HeightNumbers {
    /// The project's variation's (its tile database entry), TEd's defaults when missing.
    pub fn of(raw: &Tile) -> Self {
        let entry = raw.entry.as_ref().map(|e| &e.root);
        let variation = entry.and_then(|e| e.child("VARIATIONS")).and_then(|vs| {
            vs.children.iter().find(|v| v.get("location").and_then(|l| l.as_str()) == raw.variation.as_deref())
        });
        let num = |k: &str, d: f32| variation.and_then(|v| v.get(k)).and_then(|v| v.as_f64().or_else(|| v.as_str().and_then(|t| t.parse().ok()))).map_or(d, |v| v as f32);
        Self { scale: num("scale", 1.0), min_height: num("min_height", 0.0), normal_strength: num("normal_strength", 1.0) }
    }
}

/// The stage-B textures of a project (BOB's Tile processor), as files: `blend.dds`, `index.dds` (DXT5) and `ground_types.dds` (battle
/// tiles), from `blend_map.tif`, the entry's texture groups and `ground_type_map.png`.
pub fn textures(raw: &Tile) -> Result<Vec<(String, Vec<u8>)>, String> {
    let blend_map = raw.map(MapKind::Blend, 0).ok_or("the project has no blend map")?;
    let Samples::U16(weights) = &blend_map.raster.samples else { return Err("the blend map is not 16 bit".into()) };
    let (w, h) = (blend_map.raster.width as usize, blend_map.raster.height as usize);
    let (blend, index) = blend_and_index(weights, w, h);
    let mut out = vec![("blend.dds".to_owned(), dds_dxt5(&blend, w, h)), ("index.dds".to_owned(), dds_dxt5(&index, w, h))];
    let entry = raw.entry.as_ref().map(|e| &e.root);
    if let Some(hm) = raw.map(MapKind::Height, 0)
        && let Samples::U16(hs) = &hm.raster.samples
    {
        let (hw, hh) = (hm.raster.width as usize, hm.raster.height as usize);
        let v = HeightNumbers::of(raw);
        let normals = normal_map(hs, hw, hh, v.scale, v.min_height, v.normal_strength);
        out.extend(normal_dds(&normals, hw, hh).map(|d| ("normal.dds".to_owned(), d)));
    }

    let battle = entry.and_then(|e| e.get("location")).and_then(|v| v.as_str()).is_some_and(|l| l.to_lowercase().contains("battle"));
    if battle {
        let groups = entry.and_then(|e| e.child("TEXTURE_GROUP"));
        let name = |g: &Option<&Node>, k: &str| g.and_then(|g| g.get(k)).and_then(|v| v.as_str()).unwrap_or_default().to_owned();
        let types = ["red", "green", "blue", "alpha"].map(|k| ground_type_of_group(&name(&groups, k)));
        // The painted ground types (`ground_type_map.png`, RGBA, the blend map's size): a colour of the palette is its type.
        // BOB opens it whatever its case (Windows): a `ground_type_map.PNG` the project carries as a file counts too.
        let carried = || {
            let f = raw.files.iter().find(|f| f.path.eq_ignore_ascii_case("ground_type_map.png"))?;
            super::images::read_png(&f.data.bytes().ok()?).ok().map(|(r, _)| r)
        };
        let raster = raw.map(MapKind::GroundType, 0).map(|m| m.raster.clone()).or_else(carried);
        let painted = raster.and_then(|r| match r.samples {
            Samples::U8(px) if r.width as usize == w => Some((px, r.channels as usize)),
            _ => None,
        });
        let lookup = painted.as_ref().map(|(px, ch)| {
            move |x: usize, y: usize| -> Option<u8> {
                let o = (y * w + x) * ch;
                let rgb = [px[o], px[o + 1], px[o + 2]];
                GROUND_COLOURS.iter().position(|c| *c == rgb).map(|t| t as u8)
            }
        });
        let gt = ground_types(&blend, w, h, types, lookup.as_ref().map(|f| f as &dyn Fn(usize, usize) -> Option<u8>));
        out.push(("ground_types.dds".to_owned(), dds_l8(&gt, w - 256, h - 256)));
    }

    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_block_as_bob_compresses_it() {
        // farm_western/1x1/aa's blend.dds, block (176, 0): red 255 but one pixel (252, green 2); BOB's bytes.
        let mut px = [0u8, 0, 255, 0].repeat(16);
        px[3 * 4..3 * 4 + 4].copy_from_slice(&[0, 2, 252, 0]);
        let block = compress_dxt5(&px, 4, 4);
        assert_eq!(block, [0x01, 0x00, 0x49, 0x92, 0x24, 0x49, 0x92, 0x24, 0x00, 0xf8, 0x20, 0xf0, 0x80, 0x00, 0x00, 0x00]);
    }

    #[test]
    fn influence_as_ted_builds_it() {
        // terrain_rolling_hills_med/2x2/ae's final_alpha.dds (BOB's), row 128 every 8 samples.
        let alpha = influence(CELL, 2, 2);
        let row = (0..9).map(|i| alpha[128 * 257 + i * 8]).collect::<Vec<_>>();
        assert_eq!(row, [0.0, 0.038_192_786, 0.191_242_77, 0.408_694_4, 0.625_269, 0.795_853_9, 0.906_724_93, 0.967_387_14, 0.995_637_4]);
    }

    #[test]
    fn levels_and_header() {
        // 384 x 384: 8 levels (384 .. 3), the first level's size as the linear size.
        let dds = dds_dxt5(&vec![0u8; 384 * 384 * 4], 384, 384);
        assert_eq!(dds.len(), 196_768);
        assert_eq!(u32::from_le_bytes(dds[28..32].try_into().unwrap()), 8);
        assert_eq!(u32::from_le_bytes(dds[20..24].try_into().unwrap()), 147_456);
        // The box filter rounds ((sum + 2) >> 2) and clamps a single row: pixels 1 and 2 twice each give (6 + 2) >> 2 = 2, 0 and 1
        // twice each (2 + 2) >> 2 = 1.
        let (h, w, hh) = half(&[1, 0, 0, 0, 2, 1, 1, 1, 9, 9, 9, 9], 3, 1);
        assert_eq!((w, hh), (1, 1));
        assert_eq!(h, [2, 1, 1, 1]);
    }
}
