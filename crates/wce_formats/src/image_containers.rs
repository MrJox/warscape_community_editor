//! Pixel containers of the kit's images: DDS, TGA, TIFF (PNG is in [`crate::png_file`]).
//!
//! Each keeps the header as parsed fields plus the raw payload, so writing reproduces the file byte for byte and edits of the
//! payload keep the header consistent. TEd and BOB write two fixed layouts that can be generated from the size alone: the R32F
//! DDS of single-channel float textures, and the 242-byte TIFF header of blend maps.

/// Errors of reading an image container.
#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct ImageError(pub String);

fn fail<T>(what: &str) -> Result<T, ImageError> {
    Err(ImageError(what.to_owned()))
}

fn rd32(b: &[u8], o: usize) -> Result<u32, ImageError> {
    match b.get(o..o + 4) {
        Some(v) => Ok(u32::from_le_bytes([v[0], v[1], v[2], v[3]])),
        None => fail("truncated"),
    }
}

fn rd16(b: &[u8], o: usize) -> Result<u16, ImageError> {
    match b.get(o..o + 2) {
        Some(v) => Ok(u16::from_le_bytes([v[0], v[1]])),
        None => fail("truncated"),
    }
}

/// `DX10`: the fourcc of DDS files with the extended header.
const FOURCC_DX10: u32 = 0x3031_5844;

/// A DDS file.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DdsFile {
    /// The 124 bytes after the magic, as dwords (size, flags, height, width, pitch, depth, mips, 11 reserved, pixel format x8, caps x4, reserved).
    pub header: [u32; 31],
    pub has_dx10: bool,
    pub dx10: [u32; 5],

    /// Everything after the header(s).
    pub payload: Vec<u8>,
}

impl DdsFile {

    pub fn flags(&self) -> u32 {
        self.header[1]
    }

    pub fn height(&self) -> u32 {
        self.header[2]
    }

    pub fn width(&self) -> u32 {
        self.header[3]
    }

    pub fn mip_count(&self) -> u32 {
        self.header[6]
    }

    pub fn pixel_format_flags(&self) -> u32 {
        self.header[19]
    }

    pub fn fourcc(&self) -> u32 {
        self.header[20]
    }

    pub fn bit_count(&self) -> u32 {
        self.header[21]
    }

    /// Reads a DDS file.
    ///
    /// # Errors
    ///
    /// Fails for a file that isn't a DDS, or is truncated.
    pub fn parse(b: &[u8]) -> Result<Self, ImageError> {
        if b.len() < 128 || &b[..4] != b"DDS " {
            return fail("not a DDS file");
        }

        let mut d = Self::default();
        for (i, dword) in d.header.iter_mut().enumerate() {
            *dword = rd32(b, 4 + 4 * i)?;
        }

        if d.header[0] != 124 {
            return fail("unexpected header size");
        }

        let mut off = 128;
        if d.header[20] == FOURCC_DX10 {
            if b.len() < 148 {
                return fail("truncated DX10 header");
            }

            d.has_dx10 = true;
            for (i, dword) in d.dx10.iter_mut().enumerate() {
                *dword = rd32(b, 128 + 4 * i)?;
            }

            off = 148;
        }

        d.payload = b[off..].to_vec();
        Ok(d)
    }

    pub fn write(&self) -> Vec<u8> {
        let mut out = b"DDS ".to_vec();
        out.extend(self.header.iter().flat_map(|v| v.to_le_bytes()));
        if self.has_dx10 {
            out.extend(self.dx10.iter().flat_map(|v| v.to_le_bytes()));
        }

        out.extend_from_slice(&self.payload);
        out
    }

    /// The header TEd and BOB write for single-channel float textures (final_*.dds, protection_map.dds): flags 0, no mips, no caps,
    /// pixel format fourcc 114 (D3DFMT_R32F), 32 bits. 5,099 shipped files are this plus their payload.
    pub fn make_r32f(width: u32, height: u32, float_payload: Vec<u8>) -> Self {
        let mut header = [0; 31];
        header[0] = 124;
        header[2] = height;
        header[3] = width;
        header[18] = 32;
        header[19] = 4;
        header[20] = 114;
        header[21] = 32;
        Self { header, has_dx10: false, dx10: [0; 5], payload: float_payload }
    }

    /// If the header is [`Self::make_r32f`]'s for its size, and the payload has width * height floats.
    pub fn is_r32f_template(&self) -> bool {
        !self.has_dx10 && self.header == Self::make_r32f(self.width(), self.height(), vec![]).header
            && self.payload.len() == self.width() as usize * self.height() as usize * 4
    }
}

/// A TGA file.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TgaFile {
    pub header: [u8; 18],

    /// Id field, colour map, pixels and footer, verbatim.
    pub rest: Vec<u8>,
}

impl TgaFile {

    pub fn image_type(&self) -> u8 {
        self.header[2]
    }

    pub fn width(&self) -> u16 {
        u16::from_le_bytes([self.header[12], self.header[13]])
    }

    pub fn height(&self) -> u16 {
        u16::from_le_bytes([self.header[14], self.header[15]])
    }

    pub fn bits_per_pixel(&self) -> u8 {
        self.header[16]
    }

    /// Reads a TGA file.
    ///
    /// # Errors
    ///
    /// Fails for a truncated file, or an image type other than colour-mapped, true colour, grey or RLE true colour.
    pub fn parse(b: &[u8]) -> Result<Self, ImageError> {
        let Some(header) = b.get(..18) else { return fail("truncated TGA") };
        let mut t = Self { header: [0; 18], rest: b[18..].to_vec() };
        t.header.copy_from_slice(header);
        if ![1, 2, 3, 10].contains(&t.header[2]) {
            return fail("unsupported TGA type");
        }

        Ok(t)
    }

    pub fn write(&self) -> Vec<u8> {
        [&self.header[..], &self.rest].concat()
    }
}

/// An entry of a TIFF directory.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TiffEntry {
    pub tag: u16,
    pub kind: u16,
    pub count: u32,
    pub value: u32,
}

/// A little-endian TIFF file.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TiffFile {
    /// The bytes before the first strip (the directory, out-of-line values), verbatim.
    pub header: Vec<u8>,

    /// The first directory, decoded (a read-only view of `header`).
    pub entries: Vec<TiffEntry>,

    /// The strip(s).
    pub pixels: Vec<u8>,
    pub width: u32,
    pub height: u32,
    pub samples_per_pixel: u32,
    pub bits_per_sample: u32,
    pub strip_offset: u32,

    /// 15 entries, strip at 242: regenerable from width and height.
    pub ted_layout: bool,
}

impl TiffFile {

    /// Reads a TIFF file.
    ///
    /// # Errors
    ///
    /// Fails for a file that isn't a little-endian TIFF, is truncated, or has no valid strip offset.
    pub fn parse(b: &[u8]) -> Result<Self, ImageError> {
        if b.len() < 16 || &b[..4] != b"II*\0" {
            return fail("not a little-endian TIFF");
        }

        let mut t = Self::default();
        let ifd = rd32(b, 4)? as usize;
        let n = rd16(b, ifd)?;
        let mut strip_off = 0;
        for i in 0..usize::from(n) {
            let o = ifd + 2 + 12 * i;
            let e = TiffEntry { tag: rd16(b, o)?, kind: rd16(b, o + 2)?, count: rd32(b, o + 4)?, value: rd32(b, o + 8)? };
            t.entries.push(e);

            // SHORT values live in the low half of the value field.
            let v = if e.kind == 3 && e.count == 1 { e.value & 0xFFFF } else { e.value };
            match e.tag {
                256 => t.width = v,
                257 => t.height = v,
                277 => t.samples_per_pixel = v,
                258 => t.bits_per_sample = if e.count == 1 { v } else { u32::from(rd16(b, e.value as usize)?) },
                273 => strip_off = if e.count == 1 { v } else { rd32(b, e.value as usize)? },

                // The strip length isn't kept, but reading it fails on a truncated file as the C++ code does.
                279 if e.count != 1 => {
                    rd32(b, e.value as usize)?;
                }

                _ => {}
            }
        }

        if strip_off == 0 || strip_off as usize > b.len() {
            return fail("bad strip offset");
        }

        t.strip_offset = strip_off;
        t.header = b[..strip_off as usize].to_vec();
        t.pixels = b[strip_off as usize..].to_vec();
        t.ted_layout = n == 15 && strip_off == 242;
        Ok(t)
    }

    pub fn write(&self) -> Vec<u8> {
        [&self.header[..], &self.pixels].concat()
    }

    /// The 242-byte header TEd and BOB write for a blend map of 16 16-bit samples per pixel.
    pub fn ted_header(width: u32, height: u32) -> Vec<u8> {
        let mut o = vec![b'I', b'I', 42, 0, 8, 0, 0, 0];
        o.extend_from_slice(&15u16.to_le_bytes());
        for (tag, kind, count, value) in [
            (254u16, 4u16, 1u32, 0u32), (256, 3, 1, width), (257, 3, 1, height), (258, 3, 16, 194), (259, 3, 1, 1), (262, 3, 1, 1), (273, 4, 1, 242),
            (274, 3, 1, 1), (277, 3, 1, 16), (278, 3, 1, height), (279, 4, 1, width.wrapping_mul(height).wrapping_mul(32)), (282, 5, 1, 226), (283, 5, 1, 234),
            (284, 3, 1, 1), (296, 3, 1, 1),
        ] {
            o.extend_from_slice(&tag.to_le_bytes());
            o.extend_from_slice(&kind.to_le_bytes());
            o.extend_from_slice(&count.to_le_bytes());
            o.extend_from_slice(&value.to_le_bytes());
        }

        o.extend_from_slice(&0u32.to_le_bytes());
        o.extend((0..16).flat_map(|_| 16u16.to_le_bytes()));
        for v in [width, 1, height, 1] {
            o.extend_from_slice(&v.to_le_bytes());
        }

        o
    }

    /// If this is a TEd-layout file whose header is [`Self::ted_header`]'s, so it can be rebuilt from the pixels alone.
    pub fn header_is_regenerable(&self) -> bool {
        self.ted_layout && self.header == Self::ted_header(self.width, self.height) && self.pixels.len() == self.width as usize * self.height as usize * 32
    }
}
