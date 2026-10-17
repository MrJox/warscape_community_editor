//! CPU decoding of the DDS files the terrain bake reads (DXT1 / DXT3 / DXT5 and uncompressed 32-bit BGRA / RGBA): one mip level to RGBA8. The
//! renderer uploads DDS files as they are; this is for code that has to read the pixels (the terrain colour bake).

/// An RGBA8 image, rows top to bottom.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Rgba8Image {
    pub w: u32,
    pub h: u32,

    /// w * h * 4 bytes: r, g, b, a.
    pub px: Vec<u8>,
}

impl Rgba8Image {
    pub fn at(&self, x: u32, y: u32) -> &[u8] {
        let i = (y as usize * self.w as usize + x as usize) * 4;
        &self.px[i..i + 4]
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DdsInfo {
    pub width: u32,
    pub height: u32,
    pub mips: u32,

    /// "DXT1", "DXT3", "DXT5", "BGRA8", "RGBA8" or "" when not supported.
    pub format: &'static str,
}

impl Default for DdsInfo {
    fn default() -> Self {
        Self { width: 0, height: 0, mips: 1, format: "" }
    }
}

fn u32_at(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]])
}

fn rgb565(c: u16) -> [u8; 3] {
    let (r, g, b) = ((c >> 11) & 31, (c >> 5) & 63, c & 31);
    [((r << 3) | (r >> 2)) as u8, ((g << 2) | (g >> 4)) as u8, ((b << 3) | (b >> 2)) as u8]
}

/// A BC1 block (8 bytes) to 16 RGBA pixels. Without `allow_alpha` (the colour half of DXT3 / DXT5) the four-colour palette is always used.
pub fn decode_bc1_block(blk: &[u8], allow_alpha: bool) -> [[u8; 4]; 16] {
    let (c0, c1) = (u16::from_le_bytes([blk[0], blk[1]]), u16::from_le_bytes([blk[2], blk[3]]));
    let (p0, p1) = (rgb565(c0), rgb565(c1));
    let mut pal = [[0u8, 0, 0, 255]; 4];
    pal[0][..3].copy_from_slice(&p0);
    pal[1][..3].copy_from_slice(&p1);
    if c0 > c1 || !allow_alpha {
        for k in 0..3 {
            pal[2][k] = ((2 * u32::from(p0[k]) + u32::from(p1[k])) / 3) as u8;
            pal[3][k] = ((u32::from(p0[k]) + 2 * u32::from(p1[k])) / 3) as u8;
        }
    } else {
        for k in 0..3 {
            pal[2][k] = ((u32::from(p0[k]) + u32::from(p1[k])) / 2) as u8;
            pal[3][k] = 0;
        }

        pal[3][3] = 0;
    }

    let bits = u32_at(blk, 4);
    std::array::from_fn(|i| pal[((bits >> (2 * i)) & 3) as usize])
}

/// A BC3 alpha block (8 bytes) to 16 alpha values.
pub fn decode_bc3_alpha_block(blk: &[u8]) -> [u8; 16] {
    let mut a = [0u32; 8];
    a[0] = u32::from(blk[0]);
    a[1] = u32::from(blk[1]);
    if a[0] > a[1] {
        for k in 1..7 {
            a[k + 1] = ((7 - k as u32) * a[0] + k as u32 * a[1]) / 7;
        }
    } else {
        for k in 1..5 {
            a[k + 1] = ((5 - k as u32) * a[0] + k as u32 * a[1]) / 5;
        }

        a[6] = 0;
        a[7] = 255;
    }

    let bits = (0..6).fold(0u64, |bits, i| bits | (u64::from(blk[2 + i]) << (8 * i)));
    std::array::from_fn(|i| a[((bits >> (3 * i)) & 7) as usize] as u8)
}

/// Reads the header only (needs the first 128 bytes). `format` is empty for anything the decoder does not handle.
pub fn dds_info(f: &[u8]) -> DdsInfo {
    let mut i = DdsInfo::default();
    if f.len() < 128 || &f[..4] != b"DDS " {
        return i;
    }

    i.height = u32_at(f, 12);
    i.width = u32_at(f, 16);
    i.mips = u32_at(f, 28).max(1);
    let pf_flags = u32_at(f, 80);
    if pf_flags & 4 != 0 {
        // DDPF_FOURCC.
        i.format = match &f[84..88] {
            b"DXT1" => "DXT1",
            b"DXT3" => "DXT3",
            b"DXT5" => "DXT5",
            _ => "",
        };
    } else if pf_flags & 0x40 != 0 && u32_at(f, 88) == 32 {
        // DDPF_RGB, 32 bits.
        let (rm, bm) = (u32_at(f, 92), u32_at(f, 100));
        if rm == 0x00ff_0000 && bm == 0x0000_00ff {
            i.format = "BGRA8";
        } else if rm == 0x0000_00ff && bm == 0x00ff_0000 {
            i.format = "RGBA8";
        }
    }

    i
}

/// Decodes mip level `mip` (0 = the largest; clamped to the last one).
///
/// # Errors
///
/// The file is not a supported DDS, or shorter than its header says.
pub fn dds_decode_mip(f: &[u8], mip: u32) -> Result<Rgba8Image, &'static str> {
    let info = dds_info(f);
    if info.format.is_empty() {
        return Err("not a DDS file, or a format the decoder does not read (DXT1/3/5, 32-bit RGB)");
    }

    let mip = mip.min(info.mips - 1);
    let block = info.format.starts_with('D');
    let blk_bytes = if info.format == "DXT1" { 8 } else { 16 };
    let size = |w: u32, h: u32| if block { w.div_ceil(4).max(1) as usize * h.div_ceil(4).max(1) as usize * blk_bytes } else { w as usize * h as usize * 4 };
    let mut off = 128;
    let (mut w, mut h) = (info.width, info.height);
    for _ in 0..mip {
        off += size(w, h);
        w = (w / 2).max(1);
        h = (h / 2).max(1);
    }

    if off + size(w, h) > f.len() {
        return Err("the file is shorter than its header says");
    }

    let mut out = Rgba8Image { w, h, px: vec![255; w as usize * h as usize * 4] };
    if !block {
        for (d, s) in out.px.as_chunks_mut::<4>().0.iter_mut().zip(f[off..].as_chunks::<4>().0) {
            *d = if info.format == "BGRA8" { [s[2], s[1], s[0], s[3]] } else { *s };
        }

        return Ok(out);
    }

    let (bw, bh) = (w.div_ceil(4).max(1), h.div_ceil(4).max(1));
    for by in 0..bh {
        for bx in 0..bw {
            let at = off + (by as usize * bw as usize + bx as usize) * blk_bytes;
            let blk = &f[at..at + blk_bytes];
            let mut px = if info.format == "DXT1" { decode_bc1_block(blk, true) } else { decode_bc1_block(&blk[8..], false) };
            if info.format == "DXT5" {
                for (p, a) in px.iter_mut().zip(decode_bc3_alpha_block(blk)) {
                    p[3] = a;
                }
            } else if info.format == "DXT3" {
                // 4-bit alpha.
                for (i, p) in px.iter_mut().enumerate() {
                    p[3] = ((blk[i / 2] >> (4 * (i & 1))) & 15) * 17;
                }
            }

            for (i, p) in px.iter().enumerate() {
                let (x, y) = (bx * 4 + (i & 3) as u32, by * 4 + (i >> 2) as u32);
                if x < w && y < h {
                    let o = (y as usize * w as usize + x as usize) * 4;
                    out.px[o..o + 4].copy_from_slice(p);
                }
            }
        }
    }

    Ok(out)
}
