//! Wwise sound files (`.wem`): a RIFF `WAVE` file whose `fmt ` names the codec. Every Attila WEM is Wwise Vorbis (codec 0xFFFF) with
//! a 0x42-byte `fmt ` that embeds the Vorbis header (docs/formats/wem.md); `data` holds an optional seek table, the setup packet and
//! the audio packets, each behind a u16 size.
//!
//! [`Wem`] keeps every chunk as bytes (so writing gives back the file); [`Wem::format`] and [`Wem::vorbis`] read the fields.

use crate::io::{Put, Reader};
use crate::{FormatError, Result};

/// A RIFF sound file, chunk by chunk.
#[derive(Clone, Debug, PartialEq)]
pub struct Wem {
    /// The RIFF size field as stored.
    pub riff_size: u32,
    pub chunks: Vec<RiffChunk>,
    /// Bytes after the last chunk, if any.
    pub trailing: Vec<u8>,
}

/// One RIFF chunk. Odd-sized chunks are followed by a pad byte, kept as read.
#[derive(Clone, Debug, PartialEq)]
pub struct RiffChunk {
    pub tag: [u8; 4],
    pub data: Vec<u8>,
    pub pad: Option<u8>,
}

/// The `fmt ` chunk's common fields.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Format {
    pub codec: u16,
    pub channels: u16,
    pub sample_rate: u32,
    pub avg_bytes_per_second: u32,
    pub block_align: u16,
    pub bits_per_sample: u16,
}

/// The Wwise Vorbis codec.
pub const CODEC_VORBIS: u16 = 0xFFFF;

/// The Wwise Vorbis header of a 0x42-byte `fmt ` (bank version 112): the WAVE extension, then Wwise's runtime header.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VorbisHeader {
    /// The extension's size (0x30).
    pub extension_size: u16,
    /// 0 from Wwise 2014.1 for 16-bit sources; 16 or 24 in some multichannel vanilla files.
    pub valid_bits: u16,
    /// 0x4 mono, 0x3 stereo, 0x603 quad, 0x607 five channels, 0x60F 5.1.
    pub channel_mask: u32,
    pub total_frames: u32,
    /// Relative to the end of the seek table: the first audio packet.
    pub loop_start_packet_offset: u32,
    /// Relative to the end of the seek table: the end of the data.
    pub loop_end_packet_offset: u32,
    pub loop_begin_extra: u16,
    pub loop_end_extra: u16,
    pub seek_table_size: u32,
    /// The first audio packet, from the start of `data`.
    pub vorbis_data_offset: u32,
    pub max_packet_size: u16,
    /// Samples decoded past `total_frames`.
    pub last_granule_extra: u16,
    pub decode_alloc_size: u32,
    pub decode_x64_alloc_size: u32,
    /// FNV-1 (crate::fnv::bytes) of the setup packet's payload.
    pub codebook_hash: u32,
    pub blocksize_0_exp: u8,
    pub blocksize_1_exp: u8,
}

impl Wem {
    /// Reads a RIFF file.
    ///
    /// # Errors
    ///
    /// Fails when the file is not RIFF `WAVE` or a chunk runs past the end.
    pub fn read(bytes: &[u8]) -> Result<Self> {
        let mut r = Reader::new(bytes);
        if &r.array::<4>()? != b"RIFF" {
            return Err(FormatError::Invalid("not a RIFF file".into()));
        }

        let riff_size = r.u32()?;
        if &r.array::<4>()? != b"WAVE" {
            return Err(FormatError::Invalid("not a WAVE file".into()));
        }

        let mut chunks = vec![];
        while r.left() >= 8 {
            let tag: [u8; 4] = r.array()?;
            let size = r.u32()? as usize;
            let data = r.take(size)?.to_vec();
            let pad = if size % 2 == 1 && r.left() > 0 { Some(r.u8()?) } else { None };
            chunks.push(RiffChunk { tag, data, pad });
        }

        Ok(Self { riff_size, chunks, trailing: r.rest().to_vec() })
    }

    /// Writes the file. What [`Wem::read`] read comes back byte for byte.
    pub fn write(&self) -> Vec<u8> {
        let mut out = b"RIFF".to_vec();
        out.put_u32(self.riff_size);
        out.extend_from_slice(b"WAVE");
        for c in &self.chunks {
            out.extend_from_slice(&c.tag);
            out.put_u32(c.data.len() as u32);
            out.extend_from_slice(&c.data);
            if let Some(pad) = c.pad {
                out.put_u8(pad);
            }
        }

        out.extend_from_slice(&self.trailing);
        out
    }

    pub fn chunk(&self, tag: &[u8; 4]) -> Option<&[u8]> {
        self.chunks.iter().find(|c| &c.tag == tag).map(|c| c.data.as_slice())
    }

    /// The `fmt ` chunk's common fields.
    pub fn format(&self) -> Option<Format> {
        let mut r = Reader::new(self.chunk(b"fmt ")?);
        Some(Format {
            codec: r.u16().ok()?,
            channels: r.u16().ok()?,
            sample_rate: r.u32().ok()?,
            avg_bytes_per_second: r.u32().ok()?,
            block_align: r.u16().ok()?,
            bits_per_sample: r.u16().ok()?,
        })
    }

    /// The Wwise Vorbis header, for a Vorbis WEM with a 0x42-byte `fmt `.
    pub fn vorbis(&self) -> Option<VorbisHeader> {
        let fmt = self.chunk(b"fmt ")?;
        if fmt.len() != 0x42 || u16::from_le_bytes([fmt[0], fmt[1]]) != CODEC_VORBIS {
            return None;
        }

        let mut r = Reader::new(&fmt[0x10..]);
        let h = VorbisHeader {
            extension_size: r.u16().ok()?,
            valid_bits: r.u16().ok()?,
            channel_mask: r.u32().ok()?,
            total_frames: r.u32().ok()?,
            loop_start_packet_offset: r.u32().ok()?,
            loop_end_packet_offset: r.u32().ok()?,
            loop_begin_extra: r.u16().ok()?,
            loop_end_extra: r.u16().ok()?,
            seek_table_size: r.u32().ok()?,
            vorbis_data_offset: r.u32().ok()?,
            max_packet_size: r.u16().ok()?,
            last_granule_extra: r.u16().ok()?,
            decode_alloc_size: r.u32().ok()?,
            decode_x64_alloc_size: r.u32().ok()?,
            codebook_hash: r.u32().ok()?,
            blocksize_0_exp: r.u8().ok()?,
            blocksize_1_exp: r.u8().ok()?,
        };
        Some(h)
    }

    /// The setup packet's payload of a Wwise Vorbis WEM.
    pub fn vorbis_setup(&self) -> Option<&[u8]> {
        let h = self.vorbis()?;
        let data = self.chunk(b"data")?;
        let at = h.seek_table_size as usize;
        let size = u16::from_le_bytes([*data.get(at)?, *data.get(at + 1)?]) as usize;
        data.get(at + 2..at + 2 + size)
    }

    /// Duration in seconds, for a Vorbis WEM.
    pub fn duration(&self) -> Option<f64> {
        let rate = self.format()?.sample_rate;
        let frames = self.vorbis()?.total_frames;
        (rate > 0).then(|| f64::from(frames) / f64::from(rate))
    }
}
