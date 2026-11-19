//! Reading sound files to import: WAV (PCM 8, 16, 24, 32-bit, float 32 and 64, WAVE_FORMAT_EXTENSIBLE) and Ogg Vorbis (Ogg pages
//! read here, the packets decoded by lewton). The result is a [`Pcm`] in the file's channel order (WAVE order).

use lewton::audio::{PreviousWindowRight, read_audio_packet_generic};
use lewton::header::{read_header_comment, read_header_ident, read_header_setup};
use lewton::samples::InterleavedSamples;

use crate::decode::{Pcm, decode_wem};
use crate::{AudioError, Result};

/// Reads a WAV, Ogg Vorbis or WEM file, by its content.
///
/// # Errors
///
/// When the file is none of these, or a format they can hold that is not read here (compressed WAV, Ogg with another codec).
pub fn read_sound(bytes: &[u8]) -> Result<Pcm> {
    if bytes.starts_with(b"OggS") {
        return read_ogg(bytes);
    }
    if bytes.len() >= 22 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WAVE" {
        let codec = find_chunk(bytes, b"fmt ").map(|f| u16::from_le_bytes([f[0], f[1]]));
        return if codec == Some(0xFFFF) { decode_wem(bytes) } else { read_wav(bytes) };
    }
    Err(AudioError::Unsupported("not a WAV, Ogg Vorbis or WEM file".into()))
}

fn find_chunk<'a>(bytes: &'a [u8], tag: &[u8; 4]) -> Option<&'a [u8]> {
    let mut at = 12;
    while at + 8 <= bytes.len() {
        let size = u32::from_le_bytes(bytes[at + 4..at + 8].try_into().ok()?) as usize;
        let data = bytes.get(at + 8..(at + 8 + size).min(bytes.len()))?;
        if &bytes[at..at + 4] == tag {
            return Some(data);
        }
        at += 8 + size + (size & 1);
    }
    None
}

/// Reads a WAV file.
///
/// # Errors
///
/// When it has no `fmt ` or `data`, or a sample format that is not read here.
pub fn read_wav(bytes: &[u8]) -> Result<Pcm> {
    let fmt = find_chunk(bytes, b"fmt ").filter(|f| f.len() >= 16).ok_or_else(|| AudioError::Invalid("a WAV without fmt".into()))?;
    let data = find_chunk(bytes, b"data").ok_or_else(|| AudioError::Invalid("a WAV without data".into()))?;
    let mut codec = u16::from_le_bytes([fmt[0], fmt[1]]);
    let channels = u16::from_le_bytes([fmt[2], fmt[3]]);
    let sample_rate = u32::from_le_bytes(fmt[4..8].try_into().expect("4 bytes"));
    let bits = u16::from_le_bytes([fmt[14], fmt[15]]);
    if codec == 0xFFFE && fmt.len() >= 26 {
        codec = u16::from_le_bytes([fmt[24], fmt[25]]);
    }
    if channels == 0 || sample_rate == 0 {
        return Err(AudioError::Invalid("a WAV with no channels or no rate".into()));
    }

    let samples: Vec<f32> = match (codec, bits) {
        (1, 8) => data.iter().map(|&b| (f32::from(b) - 128.0) / 128.0).collect(),
        (1, 16) => data.chunks_exact(2).map(|c| f32::from(i16::from_le_bytes([c[0], c[1]])) / 32768.0).collect(),
        (1, 24) => data.chunks_exact(3).map(|c| (i32::from_le_bytes([0, c[0], c[1], c[2]]) >> 8) as f32 / 8_388_608.0).collect(),
        (1, 32) => data.chunks_exact(4).map(|c| i32::from_le_bytes(c.try_into().expect("4")) as f32 / 2_147_483_648.0).collect(),
        (3, 32) => data.chunks_exact(4).map(|c| f32::from_le_bytes(c.try_into().expect("4"))).collect(),
        (3, 64) => data.chunks_exact(8).map(|c| f64::from_le_bytes(c.try_into().expect("8")) as f32).collect(),
        _ => return Err(AudioError::Unsupported(format!("WAV codec {codec} with {bits}-bit samples"))),
    };
    let whole = samples.len() - samples.len() % usize::from(channels);
    Ok(Pcm { channels, sample_rate, samples: samples[..whole].to_vec() })
}

/// The packets of an Ogg stream (the first logical stream), with the last page's granule position.
fn ogg_packets(bytes: &[u8]) -> Result<(Vec<Vec<u8>>, i64)> {
    let (mut packets, mut current, mut at, mut serial, mut granule) = (vec![], vec![], 0usize, None, 0i64);
    while at + 27 <= bytes.len() {
        if &bytes[at..at + 4] != b"OggS" {
            return Err(AudioError::Invalid(format!("no Ogg page at byte {at}")));
        }
        let page_granule = i64::from_le_bytes(bytes[at + 6..at + 14].try_into().expect("8 bytes"));
        let page_serial = u32::from_le_bytes(bytes[at + 14..at + 18].try_into().expect("4 bytes"));
        let segments = usize::from(bytes[at + 26]);
        let table = bytes.get(at + 27..at + 27 + segments).ok_or_else(|| AudioError::Invalid("a cut Ogg page".into()))?;
        let mut body = at + 27 + segments;
        let ours = *serial.get_or_insert(page_serial) == page_serial;
        for &lacing in table {
            let piece = bytes.get(body..body + usize::from(lacing)).ok_or_else(|| AudioError::Invalid("a cut Ogg page".into()))?;
            if ours {
                current.extend_from_slice(piece);
                if lacing < 255 {
                    packets.push(std::mem::take(&mut current));
                }
            }
            body += usize::from(lacing);
        }
        if ours && page_granule >= 0 {
            granule = page_granule;
        }
        at = body;
    }
    Ok((packets, granule))
}

/// Reads an Ogg Vorbis file.
///
/// # Errors
///
/// When it is not Ogg Vorbis or does not decode.
pub fn read_ogg(bytes: &[u8]) -> Result<Pcm> {
    let (packets, granule) = ogg_packets(bytes)?;
    if packets.len() < 3 {
        return Err(AudioError::Invalid("an Ogg file without the three Vorbis headers".into()));
    }
    let ident = read_header_ident(&packets[0]).map_err(|e| AudioError::Unsupported(format!("not Ogg Vorbis: {e:?}")))?;
    read_header_comment(&packets[1]).map_err(|e| AudioError::Vorbis(format!("comment header: {e:?}")))?;
    let setup = read_header_setup(&packets[2], ident.audio_channels, (ident.blocksize_0, ident.blocksize_1))
        .map_err(|e| AudioError::Vorbis(format!("setup header: {e:?}")))?;
    let mut pwr = PreviousWindowRight::new();
    let mut samples = vec![];
    for (i, p) in packets[3..].iter().enumerate() {
        let decoded: InterleavedSamples<f32> =
            read_audio_packet_generic(&ident, &setup, p, &mut pwr).map_err(|e| AudioError::Vorbis(format!("packet {i}: {e:?}")))?;
        samples.extend_from_slice(&decoded.samples);
    }
    let channels = u16::from(ident.audio_channels);
    if granule > 0 {
        samples.truncate(granule as usize * usize::from(channels));
    }
    Ok(Pcm { channels, sample_rate: ident.audio_sample_rate, samples })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wav(codec: u16, channels: u16, bits: u16, data: &[u8]) -> Vec<u8> {
        let mut fmt = vec![];
        fmt.extend_from_slice(&codec.to_le_bytes());
        fmt.extend_from_slice(&channels.to_le_bytes());
        fmt.extend_from_slice(&22_050u32.to_le_bytes());
        fmt.extend_from_slice(&0u32.to_le_bytes());
        fmt.extend_from_slice(&0u16.to_le_bytes());
        fmt.extend_from_slice(&bits.to_le_bytes());
        let mut b = b"RIFF\0\0\0\0WAVEfmt ".to_vec();
        b.extend_from_slice(&16u32.to_le_bytes());
        b.extend_from_slice(&fmt);
        b.extend_from_slice(b"data");
        b.extend_from_slice(&(data.len() as u32).to_le_bytes());
        b.extend_from_slice(data);
        b
    }

    #[test]
    fn reads_wav_formats() {
        let p = read_sound(&wav(1, 2, 16, &[0, 0x40, 0, 0xC0])).unwrap();
        assert_eq!((p.channels, p.sample_rate, p.samples.clone()), (2, 22_050, vec![0.5, -0.5]));
        let p = read_sound(&wav(1, 1, 24, &[0, 0, 0x40])).unwrap();
        assert_eq!(p.samples, vec![0.5]);
        let p = read_sound(&wav(3, 1, 32, &0.25f32.to_le_bytes())).unwrap();
        assert_eq!(p.samples, vec![0.25]);
        assert!(read_sound(&wav(2, 1, 4, &[0])).is_err());
    }
}
