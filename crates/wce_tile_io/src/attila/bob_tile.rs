//! Stage B: BOB's Tile processor (BOB_Tile, ToolDataBuilderDll), from a project's maps to the exported tile's textures. The blend and
//! index maps (`TOOLDATABUILDER::convert_blend_map`, `TerrainBlendMap.cpp`), the ground types, and BOB's DDS writer
//! (`TOOLDATABUILDER::write_dds_ATI_compressor`: mips by a 2 x 2 box, ATI's DXT5 compressor per level). docs/formats/tile_compile.md.

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
    let at = |x: isize, y: isize| metres[(y.clamp(0, h as isize - 1) as usize) * w + x.clamp(0, w as isize - 1) as usize];
    const K: [[f32; 3]; 3] = [[1.0, 0.0, -1.0], [2.0, 0.0, -2.0], [1.0, 0.0, -1.0]];
    let z = 1.0f32 / strength;
    let mut out = vec![0u8; w * h * 4];
    for y in 0..h {
        for x in 0..w {
            let (mut gx, mut gy) = (0.0f32, 0.0f32);
            for dy in 0..3 {
                for dx in 0..3 {
                    let v = at(x as isize + dx as isize - 1, y as isize + dy as isize - 1);
                    gx += K[dy][dx] * v;
                    gy += K[dx][dy] * v;
                }
            }

            let (gx, gy) = (gx / 9.0, gy / 9.0);
            let len = gy * gy + gx * gx + z * z;
            let inv = 1.0f32 / len.sqrt();
            let byte = |c: f32| ((c + 1.0) * 0.5 * 255.0) as i32 as u8;
            out[(y * w + x) * 4..(y * w + x) * 4 + 4].copy_from_slice(&[byte(inv * z), byte(gy * inv), byte(gx * inv), 255]);
        }
    }

    out
}

/// `normal.dds` from the normal map (ToolDataBuilderDll `FUN_10087420`): each pixel to (B 0, G y, R 255, A x), DXT5; none when the map is
/// flat (every pixel 127, 127, 255: BOB points the variation at `terrain/textures/common/flat_normal.dds` instead).
pub fn normal_dds(normals: &[u8], w: usize, h: usize) -> Option<Vec<u8>> {
    if normals.chunks_exact(4).all(|p| p == [255, 127, 127, 255]) {
        return None;
    }

    let swizzled = normals.chunks_exact(4).flat_map(|p| [0, p[1], 255, p[2]]).collect::<Vec<_>>();
    Some(dds_dxt5(&swizzled, w, h))
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

/// The stage-B textures of a project (BOB's Tile processor), as files: `blend.dds`, `index.dds` (DXT5) and `ground_types.dds` (battle
/// tiles), from `blend_map.tif`, the entry's texture groups and `ground_type_map.png`.
pub fn textures(raw: &Tile) -> Result<Vec<(String, Vec<u8>)>, String> {
    let blend_map = raw.map(MapKind::Blend, 0).ok_or("the project has no blend map")?;
    let Samples::U16(weights) = &blend_map.raster.samples else { return Err("the blend map is not 16 bit".into()) };
    let (w, h) = (blend_map.raster.width as usize, blend_map.raster.height as usize);
    let (blend, index) = blend_and_index(weights, w, h);
    let mut out = vec![("blend.dds".to_owned(), dds_dxt5(&blend, w, h)), ("index.dds".to_owned(), dds_dxt5(&index, w, h))];
    let entry = raw.entry.as_ref().map(|e| &e.root);
    // The variation's height range and normal strength.
    let variation = entry.and_then(|e| e.child("VARIATIONS")).and_then(|vs| {
        vs.children.iter().find(|v| v.get("location").and_then(|l| l.as_str()) == raw.variation.as_deref())
    });
    let num = |k: &str, d: f32| variation.and_then(|v| v.get(k)).and_then(|v| v.as_f64().or_else(|| v.as_str().and_then(|t| t.parse().ok()))).map_or(d, |v| v as f32);
    if let Some(hm) = raw.map(MapKind::Height, 0)
        && let Samples::U16(hs) = &hm.raster.samples
    {
        let (hw, hh) = (hm.raster.width as usize, hm.raster.height as usize);
        let normals = normal_map(hs, hw, hh, num("scale", 1.0), num("min_height", 0.0), num("normal_strength", 1.0));
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
