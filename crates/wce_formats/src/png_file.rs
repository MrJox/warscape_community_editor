//! PNG as the kit's tools write it (TEd / BOB, libpng defaults): chunks IHDR, IDAT..., IEND, no ancillary data.
//!
//! [`PngFile::parse`] decodes the pixels (non-interlaced, 8 or 16 bit grey / RGB / grey+alpha / RGBA). [`PngFile::encode`]
//! reproduces the kit's encoder: per row libpng's "minimum sum of absolute differences" filter choice (None, Sub, Up, Average,
//! Paeth; a later filter only wins when strictly smaller), classic zlib at level 6 with memLevel 8 and the filtered strategy, and
//! IDAT chunks of `idat_chunk_size` bytes. The deflate stream must come from classic zlib (zlib-ng compresses differently), so
//! this links libz-sys' bundled zlib statically. Files with ancillary chunks (Photoshop output) or from another encoder are not
//! reproduced by `encode`; [`PngFile::write`] gives their original bytes unless the pixels were edited.

use std::mem::MaybeUninit;
use std::os::raw::c_int;

use libz_sys as z;

/// Errors of reading or encoding a PNG.
#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct PngError(pub String);

fn fail<T>(what: &str) -> Result<T, PngError> {
    Err(PngError(what.to_owned()))
}

const SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', b'\r', b'\n', 0x1A, b'\n'];

/// A PNG image.
#[derive(Clone, Debug, Default)]
pub struct PngFile {
    pub width: u32,
    pub height: u32,
    pub bit_depth: u8,
    pub color_type: u8,

    /// Size of the first IDAT chunk of the source (0: the default, 8192).
    pub idat_chunk_size: u32,

    /// Any chunk besides IHDR, IDAT and IEND.
    pub has_ancillary: bool,

    /// Unfiltered rows, `width * bytes_per_pixel` each, big-endian samples as stored.
    pub pixels: Vec<u8>,

    /// The source bytes.
    pub original: Vec<u8>,

    /// If the pixels changed since parsing: [`Self::write`] encodes them then.
    pub pixels_edited: bool,
}

fn be32(b: &[u8]) -> u32 {
    u32::from_be_bytes([b[0], b[1], b[2], b[3]])
}

fn crc32(data: &[u8]) -> u32 {
    // SAFETY: zlib reads `len` bytes from a valid slice.
    unsafe { z::crc32(0, data.as_ptr(), data.len() as z::uInt) as u32 }
}

fn put_chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    let start = out.len();
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    let crc = crc32(&out[start..]);
    out.extend_from_slice(&crc.to_be_bytes());
}

fn paeth(a: i32, b: i32, c: i32) -> u8 {
    let p = a + b - c;
    let (pa, pb, pc) = ((p - a).abs(), (p - b).abs(), (p - c).abs());
    (if pa <= pb && pa <= pc { a } else if pb <= pc { b } else { c }) as u8
}

/// The predictor of filter type `ft` for byte `i` of `row`, against `prev` (zeros for the first row).
fn predict(ft: u8, row: &[u8], prev: &[u8], i: usize, bpp: usize) -> u8 {
    let a = if i >= bpp { i32::from(row[i - bpp]) } else { 0 };
    let b = i32::from(prev[i]);
    let c = if i >= bpp { i32::from(prev[i - bpp]) } else { 0 };
    match ft {
        0 => 0,
        1 => a as u8,
        2 => b as u8,
        3 => ((a + b) >> 1) as u8,
        _ => paeth(a, b, c),
    }
}

fn filter_row(ft: u8, row: &[u8], prev: &[u8], bpp: usize, out: &mut [u8]) {
    for i in 0..row.len() {
        out[i] = row[i].wrapping_sub(predict(ft, row, prev, i, bpp));
    }
}

/// Inflates a zlib stream into exactly `out.len()` bytes.
fn inflate_exact(src: &[u8], out: &mut [u8]) -> Result<(), PngError> {
    // SAFETY: zlib expects a zeroed z_stream before inflateInit (null zalloc / zfree select its own allocator); it's only reached
    // through the raw pointer, as Rust's type doesn't allow null function pointers. The in / out pointers cover valid slices.
    unsafe {
        let mut stream = MaybeUninit::<z::z_stream>::zeroed();
        let zs = stream.as_mut_ptr();
        if z::inflateInit_(zs, z::zlibVersion(), size_of::<z::z_stream>() as c_int) != z::Z_OK {
            return fail("inflateInit");
        }

        (*zs).next_in = src.as_ptr().cast_mut();
        (*zs).avail_in = src.len() as z::uInt;
        (*zs).next_out = out.as_mut_ptr();
        (*zs).avail_out = out.len() as z::uInt;
        let rc = z::inflate(zs, z::Z_FINISH);
        let short = (*zs).avail_out != 0;
        z::inflateEnd(zs);
        if rc != z::Z_STREAM_END || short {
            return fail("bad or short IDAT stream");
        }
    }

    Ok(())
}

/// Deflates with the kit's settings: level 6, window 15, memLevel 8, filtered strategy.
fn deflate_kit(src: &[u8]) -> Result<Vec<u8>, PngError> {
    // SAFETY: as in inflate_exact; the output buffer has compressBound's size, enough for one Z_FINISH call.
    unsafe {
        let mut out = vec![0u8; z::compressBound(src.len() as z::uLong) as usize];
        let mut stream = MaybeUninit::<z::z_stream>::zeroed();
        let zs = stream.as_mut_ptr();
        if z::deflateInit2_(zs, 6, z::Z_DEFLATED, 15, 8, z::Z_FILTERED, z::zlibVersion(), size_of::<z::z_stream>() as c_int) != z::Z_OK {
            return fail("deflateInit2");
        }

        (*zs).next_in = src.as_ptr().cast_mut();
        (*zs).avail_in = src.len() as z::uInt;
        (*zs).next_out = out.as_mut_ptr();
        (*zs).avail_out = out.len() as z::uInt;
        let rc = z::deflate(zs, z::Z_FINISH);
        let total = (*zs).total_out as usize;
        z::deflateEnd(zs);
        if rc != z::Z_STREAM_END {
            return fail("deflate failed");
        }

        out.truncate(total);
        Ok(out)
    }
}

impl PngFile {

    /// Bytes per pixel of the colour type and depth (0 for unsupported ones).
    pub fn bytes_per_pixel(&self) -> usize {
        let channels = match self.color_type {
            0 => 1,
            2 => 3,
            4 => 2,
            6 => 4,
            _ => 0,
        };

        channels * if self.bit_depth == 16 { 2 } else { 1 }
    }

    /// Reads a PNG.
    ///
    /// # Errors
    ///
    /// Fails for a file that isn't a PNG, is truncated or interlaced, has an unsupported format, or a bad pixel stream.
    pub fn parse(bytes: &[u8]) -> Result<Self, PngError> {
        if bytes.len() < 8 || bytes[..8] != SIGNATURE {
            return fail("not a PNG");
        }

        let mut f = Self { original: bytes.to_vec(), ..Default::default() };
        let mut idat = vec![];
        let (mut have_ihdr, mut first_idat) = (false, true);
        let mut i = 8;
        while i + 12 <= bytes.len() {
            let n = be32(&bytes[i..]) as usize;
            if i + 12 + n > bytes.len() {
                return fail("truncated chunk");
            }

            let data = &bytes[i + 8..i + 8 + n];
            match &bytes[i + 4..i + 8] {
                b"IHDR" => {
                    if data.len() < 13 {
                        return fail("truncated chunk");
                    }

                    f.width = be32(data);
                    f.height = be32(&data[4..]);
                    f.bit_depth = data[8];
                    f.color_type = data[9];
                    if data[10..13] != [0, 0, 0] {
                        return fail("interlaced or unusual PNG");
                    }

                    have_ihdr = true;
                }

                b"IDAT" => {
                    if first_idat {
                        f.idat_chunk_size = n as u32;
                        first_idat = false;
                    }

                    idat.extend_from_slice(data);
                }

                b"IEND" => {}
                _ => f.has_ancillary = true,
            }

            i += 12 + n;
        }

        if !have_ihdr {
            return fail("no IHDR");
        }

        let bpp = f.bytes_per_pixel();
        if bpp == 0 || (f.bit_depth != 8 && f.bit_depth != 16) {
            return fail("unsupported PNG format");
        }

        let stride = f.width as usize * bpp;
        let height = f.height as usize;
        let mut raw = vec![0u8; (stride + 1) * height];
        inflate_exact(&idat, &mut raw)?;

        f.pixels = vec![0u8; stride * height];
        let zero = vec![0u8; stride];
        for y in 0..height {
            let ft = raw[y * (stride + 1)];
            if ft > 4 {
                return fail("bad filter type");
            }

            let src = &raw[y * (stride + 1) + 1..(y + 1) * (stride + 1)];
            let (done, rest) = f.pixels.split_at_mut(y * stride);
            let prev = if y > 0 { &done[(y - 1) * stride..] } else { &zero[..] };
            let cur = &mut rest[..stride];
            for x in 0..stride {
                cur[x] = src[x].wrapping_add(predict(ft, cur, prev, x, bpp));
            }
        }

        Ok(f)
    }

    /// The pixels encoded as the kit's tools encode them.
    ///
    /// # Errors
    ///
    /// Fails if zlib fails.
    pub fn encode(&self) -> Result<Vec<u8>, PngError> {
        let bpp = self.bytes_per_pixel();
        let stride = self.width as usize * bpp;
        let height = self.height as usize;
        let mut filtered = vec![0u8; (stride + 1) * height];
        let zero = vec![0u8; stride];
        let mut tmp = vec![0u8; stride];
        for y in 0..height {
            let row = &self.pixels[y * stride..(y + 1) * stride];
            let prev = if y > 0 { &self.pixels[(y - 1) * stride..y * stride] } else { &zero[..] };
            let mut best = (0u8, u64::MAX);
            for ft in 0..5u8 {
                filter_row(ft, row, prev, bpp, &mut tmp);
                let sum = tmp.iter().map(|&b| u64::from((b as i8).unsigned_abs())).sum::<u64>();
                if sum < best.1 {
                    best = (ft, sum);
                }
            }

            let out = &mut filtered[y * (stride + 1)..(y + 1) * (stride + 1)];
            out[0] = best.0;
            filter_row(best.0, row, prev, bpp, &mut out[1..]);
        }

        let compressed = deflate_kit(&filtered)?;
        let mut out = SIGNATURE.to_vec();
        let mut ihdr = Vec::with_capacity(13);
        ihdr.extend_from_slice(&self.width.to_be_bytes());
        ihdr.extend_from_slice(&self.height.to_be_bytes());
        ihdr.extend_from_slice(&[self.bit_depth, self.color_type, 0, 0, 0]);
        put_chunk(&mut out, b"IHDR", &ihdr);
        let chunk = if self.idat_chunk_size > 0 { self.idat_chunk_size as usize } else { 8192 };
        for data in compressed.chunks(chunk) {
            put_chunk(&mut out, b"IDAT", data);
        }

        put_chunk(&mut out, b"IEND", &[]);
        Ok(out)
    }

    /// The original bytes while the pixels are unchanged, else [`Self::encode`].
    ///
    /// # Errors
    ///
    /// Fails if zlib fails.
    pub fn write(&self) -> Result<Vec<u8>, PngError> {
        if self.pixels_edited { self.encode() } else { Ok(self.original.clone()) }
    }
}
