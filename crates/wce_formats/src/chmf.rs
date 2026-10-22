//! `CHMF`: the compressed height maps BOB writes (`hf_height_map.data` and `hf_water_map.data` of a tile, `lf_height_map.data` of a map).
//! The engine's class is `WARSCAPE::COMPRESSED_HEIGHT_MAP` (`Warscape.AssemblyKit.dll`: `load`, `value` 0x104a9150, the block decoder
//! 0x104a9790; the game's reader is `empire.retail.dll` fn 0x11857a00). docs/formats/tile_binary.md, "CHMF".
//!
//! Layout (little endian): `"CHMF"`, u32 version (4 or 5); u32 width, height (samples), u32 `cells_x`, `cells_y` (the unit of the region
//! rectangles, in samples), f32 min, f32 range; version 5: u32 spacing (metres per sample); u32 region count, per region `x0, y0, x1, y1`
//! (in cells, end exclusive); then per region: u16 x 2, u32 block count, u32 data size, the block offsets, the block data.
//!
//! A region is cut into blocks of 16 x 16 samples, row by row (the last block of a row or column holds what is left); the table can stop
//! early (TDD's campaign tiles): the game reads a sample of a missing block as 0 (`value`). A block starts with a
//! mode byte `k`: 0 = one value (u16); 1 ..= 127 = a palette of `k + 1` u16 values, then one index per sample of 1 to 7 bits
//! (`ceil(log2(k + 1))`); 128 ..= 142 = a u16 base, then `k - 127` bits per sample added to it; 143 = one u16 per sample. Bits are packed
//! from the low bit of each byte. A sample is `value / 65535`, the height `min + range * sample`.

#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct ChmfError(pub String);

fn fail<T>(what: impl Into<String>) -> Result<T, ChmfError> {
    Err(ChmfError(what.into()))
}

/// The compressed samples of one region.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ChmfRegion {
    /// `x0, y0, x1, y1` in cells, end exclusive.
    pub rect: [u32; 4],

    /// The two u16 before the block table (0 and 0xFFFF in the battle tiles; meaning unknown).
    pub tag: [u16; 2],
    pub offsets: Vec<u32>,
    pub data: Vec<u8>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ChmfFile {
    pub version: u32,
    pub width: u32,
    pub height: u32,
    pub cells_x: u32,
    pub cells_y: u32,
    pub min: f32,
    pub range: f32,

    /// Metres per sample (version 5; 1 for version 4).
    pub spacing: u32,
    pub regions: Vec<ChmfRegion>,
}

struct Reader<'a> {
    b: &'a [u8],
    p: usize,
}

impl Reader<'_> {
    fn take(&mut self, n: usize) -> Result<&[u8], ChmfError> {
        let r = self.b.get(self.p..self.p + n).ok_or_else(|| ChmfError(format!("truncated at {}", self.p)))?;
        self.p += n;
        Ok(r)
    }

    fn u32(&mut self) -> Result<u32, ChmfError> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().expect("4 bytes")))
    }

    fn u16(&mut self) -> Result<u16, ChmfError> {
        Ok(u16::from_le_bytes(self.take(2)?.try_into().expect("2 bytes")))
    }
}

/// The bits per sample of a block mode (`None`: one value, or raw u16), and whether it is a palette.
fn mode(k: u8) -> Result<(u32, bool), ChmfError> {
    Ok(match k {
        0 => (0, true),
        1..=127 => (u32::BITS - u32::from(k).leading_zeros(), true),
        128..=142 => (u32::from(k) - 127, false),
        143 => (16, false),
        _ => return fail(format!("block mode {k}")),
    })
}

/// `n` bits at bit `pos` of `data`, from the low bit of each byte.
fn bits(data: &[u8], pos: usize, n: u32) -> Result<u32, ChmfError> {
    let mut v = 0u32;
    for i in 0..n as usize {
        let bit = pos + i;
        let Some(&byte) = data.get(bit >> 3) else { return fail("block too short") };
        v |= u32::from((byte >> (bit & 7)) & 1) << i;
    }

    Ok(v)
}

/// The `count` samples of a block.
///
/// # Errors
///
/// An unknown mode, or a block shorter than its samples need.
pub fn decode_block(block: &[u8], count: usize, out: &mut Vec<u16>) -> Result<(), ChmfError> {
    let Some(&k) = block.first() else { return fail("empty block") };
    let rd16 = |o: usize| block.get(o..o + 2).map(|b| u16::from_le_bytes([b[0], b[1]])).ok_or_else(|| ChmfError("block too short".to_owned()));
    let (n, palette) = mode(k)?;
    if k == 143 {
        for i in 0..count {
            out.push(rd16(1 + 2 * i)?);
        }
    } else if k == 0 {
        let v = rd16(1)?;
        out.extend(std::iter::repeat_n(v, count));
    } else if palette {
        let entries = usize::from(k) + 1;
        let packed = block.get(1 + 2 * entries..).unwrap_or_default();
        for i in 0..count {
            let index = bits(packed, i * n as usize, n)? as usize;
            if index >= entries {
                return fail("palette index out of range");
            }

            out.push(rd16(1 + 2 * index)?);
        }
    } else {
        let base = rd16(1)?;
        let packed = &block[3..];
        for i in 0..count {
            out.push(base.wrapping_add(bits(packed, i * n as usize, n)? as u16));
        }
    }

    Ok(())
}

impl ChmfFile {

    /// # Errors
    ///
    /// Not a version 4 or 5 CHMF file, or a truncated one.
    pub fn parse(b: &[u8]) -> Result<Self, ChmfError> {
        let mut r = Reader { b, p: 0 };
        if r.take(4)? != b"CHMF" {
            return fail("not CHMF");
        }

        let version = r.u32()?;
        if !(4..=5).contains(&version) {
            return fail(format!("unsupported version {version}"));
        }

        let (width, height, cells_x, cells_y) = (r.u32()?, r.u32()?, r.u32()?, r.u32()?);
        let (min, range) = (f32::from_bits(r.u32()?), f32::from_bits(r.u32()?));
        let spacing = if version > 4 { r.u32()? } else { 1 };
        let n = r.u32()? as usize;
        if n > b.len() / 16 {
            return fail("region count");
        }

        let mut regions = (0..n).map(|_| Ok(ChmfRegion { rect: [r.u32()?, r.u32()?, r.u32()?, r.u32()?], ..Default::default() })).collect::<Result<Vec<_>, ChmfError>>()?;
        for region in &mut regions {
            region.tag = [r.u16()?, r.u16()?];
            let (blocks, size) = (r.u32()? as usize, r.u32()? as usize);
            if blocks > b.len() / 4 {
                return fail("block count");
            }

            region.offsets = (0..blocks).map(|_| r.u32()).collect::<Result<_, _>>()?;
            region.data = r.take(size)?.to_vec();
        }

        if r.p != b.len() {
            return fail(format!("{} bytes after the last region", b.len() - r.p));
        }

        Ok(Self { version, width, height, cells_x, cells_y, min, range, spacing, regions })
    }

    pub fn write(&self) -> Vec<u8> {
        let mut o = b"CHMF".to_vec();
        let put = |o: &mut Vec<u8>, v: u32| o.extend_from_slice(&v.to_le_bytes());
        for v in [self.version, self.width, self.height, self.cells_x, self.cells_y, self.min.to_bits(), self.range.to_bits()] {
            put(&mut o, v);
        }

        if self.version > 4 {
            put(&mut o, self.spacing);
        }

        put(&mut o, self.regions.len() as u32);
        for r in &self.regions {
            for v in r.rect {
                put(&mut o, v);
            }
        }

        for r in &self.regions {
            o.extend_from_slice(&r.tag[0].to_le_bytes());
            o.extend_from_slice(&r.tag[1].to_le_bytes());
            put(&mut o, r.offsets.len() as u32);
            put(&mut o, r.data.len() as u32);
            for &v in &r.offsets {
                put(&mut o, v);
            }

            o.extend_from_slice(&r.data);
        }

        o
    }

    /// The samples of a region (row-major, `(x1 - x0) * cells_x` by `(y1 - y0) * cells_y`), decoded; the samples of blocks the table does
    /// not have are 0, as the game reads them.
    ///
    /// # Errors
    ///
    /// A block that does not decode, or more blocks than the region has.
    pub fn region_samples(&self, region: usize) -> Result<(u32, u32, Vec<u16>), ChmfError> {
        let r = &self.regions[region];
        let w = (r.rect[2] - r.rect[0]) * self.cells_x;
        let h = (r.rect[3] - r.rect[1]) * self.cells_y;
        let (bw, bh) = (w.div_ceil(16), h.div_ceil(16));
        if r.offsets.len() > (bw * bh) as usize {
            return fail(format!("{} blocks for {w} x {h} samples", r.offsets.len()));
        }

        let mut out = vec![0u16; (w * h) as usize];
        let mut block = vec![];
        for by in 0..bh {
            for bx in 0..bw {
                let i = (by * bw + bx) as usize;
                if i >= r.offsets.len() {
                    continue;
                }

                let start = r.offsets[i] as usize;
                let end = r.offsets.get(i + 1).map_or(r.data.len(), |&e| e as usize);
                let Some(bytes) = r.data.get(start..end.max(start)) else { return fail("block offset out of range") };
                let (cw, ch) = ((w - bx * 16).min(16), (h - by * 16).min(16));
                block.clear();
                decode_block(bytes, (cw * ch) as usize, &mut block)?;
                for y in 0..ch {
                    for x in 0..cw {
                        out[((by * 16 + y) * w + bx * 16 + x) as usize] = block[(y * cw + x) as usize];
                    }
                }
            }
        }

        Ok((w, h, out))
    }

    /// A sample as metres.
    pub fn metres(&self, sample: u16) -> f32 {
        self.min + self.range * (f32::from(sample) / 65535.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn block_modes() {
        let mut out = vec![];
        decode_block(&[0, 0x34, 0x12], 3, &mut out).unwrap();
        assert_eq!(out, [0x1234; 3]);

        // Palette of 3 (2 bits): indices 0, 2, 1, 2.
        out.clear();
        decode_block(&[2, 10, 0, 20, 0, 30, 0, 0b10_01_10_00], 4, &mut out).unwrap();
        assert_eq!(out, [10, 30, 20, 30]);

        // Delta of 3 bits (k = 130) from 1000: 1, 7, 0.
        out.clear();
        decode_block(&[130, 0xE8, 0x03, 0b00_111_001, 0], 3, &mut out).unwrap();
        assert_eq!(out, [1001, 1007, 1000]);

        out.clear();
        decode_block(&[143, 1, 0, 2, 0], 2, &mut out).unwrap();
        assert_eq!(out, [1, 2]);
    }
}
