//! The maps of a TEd tile as rasters, and back: PNG (height, ground type, nogo, LF influence) and TIFF (blend map).
//!
//! PNG: the kit's encoder (`wce_formats::png_file`: libpng's row filters, zlib 6 / filtered) gives every PNG TEd and BOB wrote, so those are
//! re-encoded from the pixels. A PNG with ancillary chunks (Photoshop, World Machine) was written by another encoder: while its pixels are
//! unchanged its original bytes are written ("kept"), once they change it is encoded the kit's way.
//! TIFF: uncompressed, one contiguous run of samples. The bytes before the samples are TEd's 242-byte template (rebuilt from the size) or
//! kept as they are while the size does not change; bytes after the samples are kept.

use std::sync::Arc;

use wce_formats::image_containers::TiffFile;
use wce_formats::png_file::PngFile;
use wce_tile_model::{Raster, Samples};

/// How a map was stored, so it is written the same way.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ImageTrivia {
    Png {
        idat_chunk_size: u32,

        /// The original bytes and the fingerprint of the pixels they hold, for a file the kit's encoder does not reproduce.
        kept: Option<(Arc<[u8]>, u64)>,
    },

    Tiff {
        /// The bytes before the samples, when they are not TEd's template for the size; the size they describe.
        header: Option<(Arc<[u8]>, u32, u32)>,

        /// The bytes after the samples.
        trailer: Arc<[u8]>,
    },
}

/// What writing a map did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Encoded {
    /// Encoded from the pixels.
    Encoded,

    /// The original bytes, the pixels unchanged (an encoding the writer does not reproduce).
    Kept,
}

/// # Errors
///
/// Not a PNG this reader handles (interlaced, palette, other bit depths) or a broken one.
pub fn read_png(bytes: &[u8]) -> Result<(Raster, ImageTrivia), String> {
    let png = PngFile::parse(bytes).map_err(|e| e.to_string())?;
    let channels = match png.color_type {
        0 => 1,
        4 => 2,
        2 => 3,
        6 => 4,
        t => return Err(format!("PNG colour type {t}")),
    };

    let samples = match png.bit_depth {
        8 => Samples::U8(png.pixels),
        16 => Samples::U16(png.pixels.as_chunks::<2>().0.iter().map(|b| u16::from_be_bytes(*b)).collect()),
        d => return Err(format!("PNG bit depth {d}")),
    };

    let kept = png.has_ancillary.then(|| (Arc::<[u8]>::from(bytes), samples.fingerprint()));
    let raster = Raster { width: png.width, height: png.height, channels, samples };
    Ok((raster, ImageTrivia::Png { idat_chunk_size: png.idat_chunk_size, kept }))
}

/// # Errors
///
/// A raster PNG cannot hold (float samples, more than four channels), or zlib failed.
pub fn write_png(raster: &Raster, trivia: Option<&ImageTrivia>) -> Result<(Vec<u8>, Encoded), String> {
    let (chunk, kept) = match trivia {
        Some(ImageTrivia::Png { idat_chunk_size, kept }) => (*idat_chunk_size, kept.as_ref()),
        _ => (0, None),
    };

    if let Some((bytes, fingerprint)) = kept
        && raster.samples.fingerprint() == *fingerprint
    {
        return Ok((bytes.to_vec(), Encoded::Kept));
    }

    let color_type = match raster.channels {
        1 => 0,
        2 => 4,
        3 => 2,
        4 => 6,
        c => return Err(format!("a PNG cannot hold {c} channels")),
    };

    let (bit_depth, pixels) = match &raster.samples {
        Samples::U8(v) => (8, v.clone()),
        Samples::U16(v) => (16, v.iter().flat_map(|s| s.to_be_bytes()).collect()),
        Samples::F32(_) => return Err("a PNG cannot hold float samples".to_owned()),
    };

    let png = PngFile { width: raster.width, height: raster.height, bit_depth, color_type, idat_chunk_size: chunk, pixels, ..Default::default() };
    Ok((png.encode().map_err(|e| e.to_string())?, Encoded::Encoded))
}

fn tag(t: &TiffFile, tag: u16) -> Option<u32> {
    let e = t.entries.iter().find(|e| e.tag == tag)?;
    Some(if e.kind == 3 && e.count == 1 { e.value & 0xFFFF } else { e.value })
}

/// # Errors
///
/// Not an uncompressed, contiguous, 8 or 16 bit TIFF.
pub fn read_tiff(bytes: &[u8]) -> Result<(Raster, ImageTrivia), String> {
    let t = TiffFile::parse(bytes).map_err(|e| e.to_string())?;
    if tag(&t, 259).unwrap_or(1) != 1 {
        return Err("compressed TIFF".to_owned());
    }

    if tag(&t, 284).unwrap_or(1) != 1 {
        return Err("planar TIFF".to_owned());
    }

    // Several strips must follow each other (their offsets are an array when there is more than one).
    if let Some(e) = t.entries.iter().find(|e| e.tag == 273)
        && e.count > 1
    {
        let counts = t.entries.iter().find(|e| e.tag == 279).ok_or("no strip byte counts")?;
        let read = |at: u32, i: u32, kind: u16| -> Option<u32> {
            let o = at as usize + i as usize * if kind == 3 { 2 } else { 4 };
            Some(if kind == 3 { u32::from(u16::from_le_bytes(bytes.get(o..o + 2)?.try_into().ok()?)) } else { u32::from_le_bytes(bytes.get(o..o + 4)?.try_into().ok()?) })
        };

        let mut next = t.strip_offset;
        for i in 0..e.count {
            let (off, len) = (read(e.value, i, e.kind).ok_or("bad strip table")?, read(counts.value, i, counts.kind).ok_or("bad strip table")?);
            if off != next {
                return Err("strips are not contiguous".to_owned());
            }

            next = off + len;
        }
    }

    let bits = t.bits_per_sample;
    let (w, h, spp) = (t.width as usize, t.height as usize, t.samples_per_pixel.max(1) as usize);
    let n = w * h * spp;
    let size = n * (bits as usize / 8);
    if !(bits == 8 || bits == 16) || t.pixels.len() < size {
        return Err(format!("TIFF of {bits} bits, {} bytes of samples for {w} x {h} x {spp}", t.pixels.len()));
    }

    let samples = if bits == 8 {
        Samples::U8(t.pixels[..size].to_vec())
    } else {
        Samples::U16(t.pixels[..size].as_chunks::<2>().0.iter().map(|b| u16::from_le_bytes(*b)).collect())
    };

    let template = TiffFile::ted_header(t.width, t.height);
    let header = (t.header != template || bits != 16 || spp != 16).then(|| (Arc::<[u8]>::from(&t.header[..]), t.width, t.height));
    let trivia = ImageTrivia::Tiff { header, trailer: t.pixels[size..].into() };
    Ok((Raster { width: t.width, height: t.height, channels: spp as u32, samples }, trivia))
}

/// # Errors
///
/// Float samples, or a raster TEd's template cannot describe (not 16 channels of 16 bits) without a header of the same size.
pub fn write_tiff(raster: &Raster, trivia: Option<&ImageTrivia>) -> Result<Vec<u8>, String> {
    let (header, trailer) = match trivia {
        Some(ImageTrivia::Tiff { header, trailer }) => (header.as_ref(), &trailer[..]),
        _ => (None, &[][..]),
    };

    let mut out = match header {
        Some((h, w0, h0)) if *w0 == raster.width && *h0 == raster.height => h.to_vec(),
        _ if raster.channels == 16 && matches!(raster.samples, Samples::U16(_)) => TiffFile::ted_header(raster.width, raster.height),
        _ => return Err(format!("no TIFF header for {} channels of this size", raster.channels)),
    };

    match &raster.samples {
        Samples::U8(v) => out.extend_from_slice(v),
        Samples::U16(v) => out.extend(v.iter().flat_map(|s| s.to_le_bytes())),
        Samples::F32(_) => return Err("float samples in a TIFF".to_owned()),
    }

    out.extend_from_slice(trailer);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn png_round_trip_and_edit() {
        let raster = Raster { width: 3, height: 2, channels: 1, samples: Samples::U16(vec![0, 1, 65535, 300, 4000, 7]) };
        let (bytes, how) = write_png(&raster, None).unwrap();
        assert_eq!(how, Encoded::Encoded);
        let (back, trivia) = read_png(&bytes).unwrap();
        assert_eq!(back, raster);
        assert_eq!(write_png(&back, Some(&trivia)).unwrap().0, bytes);
    }

    #[test]
    fn tiff_template_round_trip() {
        let raster = Raster { width: 2, height: 2, channels: 16, samples: Samples::U16((0..64).collect()) };
        let bytes = write_tiff(&raster, None).unwrap();
        let (back, trivia) = read_tiff(&bytes).unwrap();
        assert_eq!(back, raster);
        assert_eq!(trivia, ImageTrivia::Tiff { header: None, trailer: Arc::from(&[][..]) });
        assert_eq!(write_tiff(&back, Some(&trivia)).unwrap(), bytes);
    }
}
