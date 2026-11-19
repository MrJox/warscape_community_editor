//! Decodes a Wwise Vorbis WEM (bank version 112: 0x42-byte `fmt `, 2-byte packet headers, packed packets, codebooks from the
//! library) to interleaved f32 samples: the packets are rebuilt as standard Vorbis (what ww2ogg does, without the Ogg pages) and
//! decoded by `lewton`. docs/formats/wem.md.

use lewton::audio::{PreviousWindowRight, read_audio_packet_generic};
use lewton::header::{read_header_ident, read_header_setup};
use lewton::samples::InterleavedSamples;
use wce_audio_format::wem::{CODEC_VORBIS, Wem};

use crate::bits::{BitReader, BitWriter, ilog};
use crate::{AudioError, Result, codebooks};

/// Decoded sound: interleaved samples in the WEM's channel order (WAVE order: FL, FR, FC, LFE, BL, BR).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Pcm {
    pub channels: u16,
    pub sample_rate: u32,
    pub samples: Vec<f32>,
}

impl Pcm {
    pub fn frames(&self) -> usize {
        if self.channels == 0 { 0 } else { self.samples.len() / usize::from(self.channels) }
    }

    pub fn seconds(&self) -> f64 {
        if self.sample_rate == 0 { 0.0 } else { self.frames() as f64 / f64::from(self.sample_rate) }
    }
}

/// The standard Vorbis setup packet rebuilt from Wwise's, and each mode's block flag.
fn rebuild_setup(ww: &[u8], channels: u32) -> Result<(Vec<u8>, Vec<bool>)> {
    let mut r = BitReader::new(ww);
    let mut w = BitWriter::new();
    w.write(5, 8);
    b"vorbis".iter().for_each(|&c| w.write(u32::from(c), 8));

    let count = r.read(8) + 1;
    w.write(count - 1, 8);
    for _ in 0..count {
        codebooks::rebuild(r.read(10), &mut w)?;
    }

    // Time domain transforms: Wwise drops them; Vorbis has one, 0.
    w.write(0, 6);
    w.write(0, 16);

    let copy = |w: &mut BitWriter, r: &mut BitReader, n: u32| {
        let v = r.read(n);
        w.write(v, n);
        v
    };

    let floors = copy(&mut w, &mut r, 6) + 1;
    for _ in 0..floors {
        w.write(1, 16);
        let partitions = copy(&mut w, &mut r, 5);
        let classes: Vec<u32> = (0..partitions).map(|_| copy(&mut w, &mut r, 4)).collect();
        let max_class = classes.iter().copied().max().map_or(0, |m| m + 1);
        let mut dimensions = vec![0; max_class as usize];
        for d in &mut dimensions {
            *d = copy(&mut w, &mut r, 3) + 1;
            let subclasses = copy(&mut w, &mut r, 2);
            if subclasses != 0 {
                copy(&mut w, &mut r, 8);
            }
            for _ in 0..(1 << subclasses) {
                copy(&mut w, &mut r, 8);
            }
        }

        copy(&mut w, &mut r, 2);
        let range_bits = copy(&mut w, &mut r, 4);
        for c in classes {
            for _ in 0..dimensions[c as usize] {
                copy(&mut w, &mut r, range_bits);
            }
        }
    }

    let residues = copy(&mut w, &mut r, 6) + 1;
    for _ in 0..residues {
        let kind = r.read(2);
        w.write(kind, 16);
        copy(&mut w, &mut r, 24);
        copy(&mut w, &mut r, 24);
        copy(&mut w, &mut r, 24);
        let classifications = copy(&mut w, &mut r, 6) + 1;
        copy(&mut w, &mut r, 8);
        let mut cascade = vec![];
        for _ in 0..classifications {
            let low = copy(&mut w, &mut r, 3);
            let high = if copy(&mut w, &mut r, 1) == 1 { copy(&mut w, &mut r, 5) } else { 0 };
            cascade.push(high * 8 + low);
        }
        for c in cascade {
            for k in 0..8 {
                if c & (1 << k) != 0 {
                    copy(&mut w, &mut r, 8);
                }
            }
        }
    }

    let mappings = copy(&mut w, &mut r, 6) + 1;
    for _ in 0..mappings {
        w.write(0, 16);
        let submaps = if copy(&mut w, &mut r, 1) == 1 { copy(&mut w, &mut r, 4) + 1 } else { 1 };
        if copy(&mut w, &mut r, 1) == 1 {
            let steps = copy(&mut w, &mut r, 8) + 1;
            let bits = ilog(channels - 1);
            for _ in 0..steps {
                copy(&mut w, &mut r, bits);
                copy(&mut w, &mut r, bits);
            }
        }
        copy(&mut w, &mut r, 2);
        if submaps > 1 {
            for _ in 0..channels {
                copy(&mut w, &mut r, 4);
            }
        }
        for _ in 0..submaps {
            copy(&mut w, &mut r, 8);
            copy(&mut w, &mut r, 8);
            copy(&mut w, &mut r, 8);
        }
    }

    let modes = copy(&mut w, &mut r, 6) + 1;
    let mut block_flags = vec![];
    for _ in 0..modes {
        block_flags.push(copy(&mut w, &mut r, 1) == 1);
        w.write(0, 16);
        w.write(0, 16);
        copy(&mut w, &mut r, 8);
    }
    w.write(1, 1);

    if r.overran() || r.position().div_ceil(8) != ww.len() {
        return Err(AudioError::Invalid(format!("the setup packet is {} bytes but reads {} bits", ww.len(), r.position())));
    }

    Ok((w.into_bytes(), block_flags))
}

/// The packets after the setup: (offset, size) in `data`.
fn audio_packets(data: &[u8], first: usize) -> Result<Vec<(usize, usize)>> {
    let mut packets = vec![];
    let mut at = first;
    while at + 2 <= data.len() {
        let size = usize::from(u16::from_le_bytes([data[at], data[at + 1]]));
        if at + 2 + size > data.len() {
            return Err(AudioError::Invalid(format!("an audio packet at {at} runs past the data")));
        }

        packets.push((at + 2, size));
        at += 2 + size;
    }

    Ok(packets)
}

/// Decodes a WEM file.
///
/// # Errors
///
/// When the file is not a v112 Wwise Vorbis WEM, or its packets do not decode.
pub fn decode_wem(bytes: &[u8]) -> Result<Pcm> {
    let wem = Wem::read(bytes)?;
    let format = wem.format().ok_or_else(|| AudioError::Invalid("no fmt chunk".into()))?;
    if format.codec != CODEC_VORBIS {
        return Err(AudioError::Unsupported(format!("codec {:#06x}", format.codec)));
    }

    let header = wem.vorbis().ok_or_else(|| AudioError::Unsupported("a Vorbis fmt that is not 0x42 bytes".into()))?;
    let data = wem.chunk(b"data").ok_or_else(|| AudioError::Invalid("no data chunk".into()))?;
    let setup = wem.vorbis_setup().ok_or_else(|| AudioError::Invalid("no setup packet".into()))?;
    let channels = u32::from(format.channels);
    let (setup, block_flags) = rebuild_setup(setup, channels)?;

    let mut ident = vec![1];
    ident.extend_from_slice(b"vorbis");
    ident.extend_from_slice(&0u32.to_le_bytes());
    ident.push(format.channels as u8);
    ident.extend_from_slice(&format.sample_rate.to_le_bytes());
    ident.extend_from_slice(&0u32.to_le_bytes());
    ident.extend_from_slice(&(format.avg_bytes_per_second.wrapping_mul(8)).to_le_bytes());
    ident.extend_from_slice(&0u32.to_le_bytes());
    ident.push(header.blocksize_0_exp | (header.blocksize_1_exp << 4));
    ident.push(1);

    let ident = read_header_ident(&ident).map_err(|e| AudioError::Vorbis(format!("identification header: {e:?}")))?;
    let setup = read_header_setup(&setup, ident.audio_channels, (ident.blocksize_0, ident.blocksize_1))
        .map_err(|e| AudioError::Vorbis(format!("setup header: {e:?}")))?;

    let mode_bits = ilog(block_flags.len() as u32 - 1);
    let packets = audio_packets(data, header.vorbis_data_offset as usize)?;
    let mode_of = |&(at, size): &(usize, usize)| -> Option<usize> {
        (size > 0).then(|| BitReader::new(&data[at..at + size]).read(mode_bits) as usize)
    };

    let mut pwr = PreviousWindowRight::new();
    let mut samples = Vec::with_capacity(header.total_frames as usize * channels as usize);
    let mut previous_long = false;
    for (i, packet) in packets.iter().enumerate() {
        let (at, size) = *packet;
        let mut r = BitReader::new(&data[at..at + size]);
        let mut w = BitWriter::new();
        w.write(0, 1);
        let mode = r.read(mode_bits) as usize;
        w.write(mode as u32, mode_bits);
        let long = *block_flags.get(mode).ok_or_else(|| AudioError::Invalid(format!("packet {i}: mode {mode}")))?;
        if long {
            let next_long = packets.get(i + 1).and_then(mode_of).and_then(|m| block_flags.get(m).copied()).unwrap_or(false);
            w.write(u32::from(previous_long), 1);
            w.write(u32::from(next_long), 1);
        }
        previous_long = long;
        let rest = r.left();
        w.copy(&mut r, rest);

        let decoded: InterleavedSamples<f32> = read_audio_packet_generic(&ident, &setup, &w.into_bytes(), &mut pwr)
            .map_err(|e| AudioError::Vorbis(format!("packet {i}: {e:?}")))?;
        samples.extend_from_slice(&decoded.samples);
    }

    samples.truncate(header.total_frames as usize * channels as usize);
    Ok(Pcm { channels: format.channels, sample_rate: format.sample_rate, samples })
}
