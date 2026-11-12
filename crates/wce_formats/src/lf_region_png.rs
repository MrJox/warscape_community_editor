//! Exact land-LF interchange: unsigned 16-bit grayscale, file rows top first.
//! Metadata never transforms the samples. Validate the bounded container before the general PNG decoder allocates.

use crate::png_file::{PngError, PngFile, crc32};

pub const MAX_SAMPLES: usize = 1_048_576;
/// Includes compressed pixels and ancillary metadata; bounds file reads as well as decoding.
pub const MAX_FILE_BYTES: usize = 16 * 1024 * 1024;

#[derive(Debug, PartialEq, Eq)]
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub samples: Vec<u16>,
}

pub fn sample_count(width: u32, height: u32) -> Result<usize, PngError> {
    let n = u64::from(width) * u64::from(height);
    if width == 0 || height == 0 || n > MAX_SAMPLES as u64 {
        return Err(PngError(format!("LF region must contain 1..={MAX_SAMPLES} samples")));
    }
    Ok(n as usize)
}

/// Encode raw samples without normalization, conversion, flipping or resampling.
pub fn encode(width: u32, height: u32, samples: &[u16]) -> Result<Vec<u8>, PngError> {
    if samples.len() != sample_count(width, height)? {
        return Err(PngError("LF region sample count does not match its dimensions".into()));
    }
    PngFile {
        width,
        height,
        bit_depth: 16,
        color_type: 0,
        pixels: samples.iter().flat_map(|s| s.to_be_bytes()).collect(),
        ..Default::default()
    }
    .encode()
}

/// Decode only non-interlaced, 16-bit grayscale PNG. Reject malformed framing/CRCs and excessive dimensions before allocation.
pub fn decode(bytes: &[u8]) -> Result<Image, PngError> {
    let bad = |s: &str| PngError(s.into());
    if bytes.len() > MAX_FILE_BYTES || bytes.get(..8) != Some(b"\x89PNG\r\n\x1a\n") {
        return Err(bad("LF region requires a PNG file of at most 16 MiB"));
    }
    let (mut pos, mut header, mut data, mut data_ended, mut end) = (8usize, false, false, false, false);
    while pos < bytes.len() {
        let head = bytes.get(pos..pos + 8).ok_or_else(|| bad("truncated PNG chunk"))?;
        let n = u32::from_be_bytes(head[..4].try_into().unwrap()) as usize;
        let next =
            pos.checked_add(12).and_then(|p| p.checked_add(n)).filter(|&p| p <= bytes.len()).ok_or_else(|| bad("truncated PNG chunk"))?;
        let kind = &head[4..8];
        let payload = &bytes[pos + 8..next - 4];
        let crc = u32::from_be_bytes(bytes[next - 4..next].try_into().unwrap());
        if crc != crc32(&bytes[pos + 4..next - 4]) {
            return Err(bad("PNG chunk CRC mismatch"));
        }
        if !header && kind != b"IHDR" {
            return Err(bad("PNG must start with IHDR"));
        }
        if data && kind != b"IDAT" {
            data_ended = true;
        }
        match kind {
            b"IHDR" => {
                if header || n != 13 {
                    return Err(bad("invalid or duplicate PNG IHDR"));
                }
                sample_count(u32::from_be_bytes(payload[..4].try_into().unwrap()), u32::from_be_bytes(payload[4..8].try_into().unwrap()))?;
                if payload[8..] != [16, 0, 0, 0, 0] {
                    return Err(bad("LF region requires non-interlaced 16-bit grayscale PNG (no alpha)"));
                }
                header = true;
            }
            b"IDAT" => {
                if data_ended {
                    return Err(bad("PNG IDAT chunks must be consecutive"));
                }
                data = true;
            }
            b"IEND" => {
                if n != 0 || !data || next != bytes.len() {
                    return Err(bad("invalid PNG IEND or trailing bytes"));
                }
                end = true;
            }
            b"tRNS" => return Err(bad("LF region PNG must not have transparency")),
            _ if kind[0] & 32 == 0 => return Err(bad("unsupported critical PNG chunk")),
            _ => {}
        }
        pos = next;
    }
    if !end {
        return Err(bad("PNG has no IEND"));
    }
    let png = PngFile::parse(bytes)?;
    Ok(Image {
        width: png.width,
        height: png.height,
        samples: png.pixels.chunks_exact(2).map(|s| u16::from_be_bytes([s[0], s[1]])).collect(),
    })
}
