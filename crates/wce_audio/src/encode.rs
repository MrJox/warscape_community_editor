//! Encodes sound to a Wwise Vorbis WEM the game plays (docs/formats/wem.md): aoTuV encodes a standard Vorbis stream, set up as for
//! 44.1 kHz whatever the sound's rate (as Wwise does), and the stream is repacked the Wwise way: the setup's codebooks become IDs into
//! Wwise's library, the packets lose their type bit and window flags, and the header gets Wwise's fields, the setup's decoder sizes
//! among them ([`crate::setup_allocs`]).

use std::collections::HashMap;
use std::mem::MaybeUninit;
use std::sync::OnceLock;

use aotuv_lancer_vorbis_sys::{
    vorbis_analysis, vorbis_analysis_blockout, vorbis_analysis_buffer, vorbis_analysis_headerout, vorbis_analysis_init,
    vorbis_analysis_wrote, vorbis_bitrate_addblock, vorbis_bitrate_flushpacket, vorbis_block, vorbis_block_clear, vorbis_block_init,
    vorbis_comment, vorbis_comment_clear, vorbis_comment_init, vorbis_dsp_clear, vorbis_dsp_state, vorbis_encode_init_vbr, vorbis_info,
    vorbis_info_clear, vorbis_info_init,
};
use ogg_next_sys::ogg_packet;
use wce_audio_format::fnv;

use crate::bits::{BitReader, BitWriter, ilog};
use crate::codebooks::{self, lookup1_values};
use crate::decode::Pcm;
use crate::{AudioError, Result, setup_allocs};

/// The rate the encoder's setup is chosen for (Wwise's choice, whatever the sound's own rate).
const SETUP_RATE: i64 = 44_100;

/// The qualities (Wwise's 0 to 10) whose setups match Wwise's; 1 does not (docs/formats/wem.md).
pub const QUALITIES: [u8; 10] = [0, 2, 3, 4, 5, 6, 7, 8, 9, 10];

/// Wwise's default quality (its Voice and SFX presets).
pub const DEFAULT_QUALITY: u8 = 4;

/// A standard codebook's bits -> the library IDs holding it.
fn library_index() -> &'static HashMap<(usize, Vec<u8>), Vec<u32>> {
    static INDEX: OnceLock<HashMap<(usize, Vec<u8>), Vec<u32>>> = OnceLock::new();
    INDEX.get_or_init(|| {
        let mut index: HashMap<(usize, Vec<u8>), Vec<u32>> = HashMap::new();
        for id in 0..codebooks::library().len() as u32 {
            let mut w = BitWriter::new();
            if codebooks::rebuild(id, &mut w).is_ok() {
                let bits = w.bit_len();
                index.entry((bits, w.into_bytes())).or_default().push(id);
            }
        }
        index
    })
}

/// Copies one standard Vorbis codebook bit for bit.
fn copy_codebook(r: &mut BitReader, w: &mut BitWriter) -> Result<()> {
    let mut cp = |n: u32| {
        let v = r.read(n);
        w.write(v, n);
        v
    };
    if cp(24) != 0x56_4342 {
        return Err(AudioError::Invalid("a codebook without its sync pattern".into()));
    }
    let dimensions = cp(16);
    let entries = cp(24);
    if cp(1) == 1 {
        cp(5);
        let mut current = 0;
        while current < entries {
            current += cp(ilog(entries - current));
        }
    } else {
        let sparse = cp(1);
        for _ in 0..entries {
            if sparse == 0 || cp(1) == 1 {
                cp(5);
            }
        }
    }
    let lookup = cp(4);
    if lookup == 1 || lookup == 2 {
        cp(32);
        cp(32);
        let value_bits = cp(4) + 1;
        cp(1);
        let values = if lookup == 1 { lookup1_values(entries, dimensions) } else { entries * dimensions };
        for _ in 0..values {
            cp(value_bits);
        }
    }
    Ok(())
}

/// The library IDs for a setup's codebooks: where a codebook is in the library twice, Wwise uses the copies that keep a quality
/// level's IDs together; the assignment with the smallest sum of each ID's distance to its nearest other ID (ties: lowest).
fn pick_ids(candidates: &[Vec<u32>]) -> Vec<u32> {
    let cost = |ids: &[u32]| -> u64 {
        let mut s = ids.to_vec();
        s.sort_unstable();
        (0..s.len())
            .map(|k| {
                let before = (k > 0).then(|| s[k] - s[k - 1]);
                let after = (k + 1 < s.len()).then(|| s[k + 1] - s[k]);
                u64::from(before.into_iter().chain(after).min().unwrap_or(0))
            })
            .sum()
    };
    let choices: Vec<usize> = (0..candidates.len()).filter(|&k| candidates[k].len() > 1).collect();
    let mut best: Option<(u64, Vec<u32>)> = None;
    let combinations: usize = choices.iter().map(|&k| candidates[k].len()).product();
    for mut n in 0..combinations {
        let mut ids: Vec<u32> = candidates.iter().map(|c| c[0]).collect();
        for &k in &choices {
            ids[k] = candidates[k][n % candidates[k].len()];
            n /= candidates[k].len();
        }
        let key = (cost(&ids), ids);
        if best.as_ref().is_none_or(|b| key < *b) {
            best = Some(key);
        }
    }
    best.map(|b| b.1).unwrap_or_default()
}

/// A standard setup packet -> (Wwise's setup, each mode's block flag).
fn wwise_setup(setup: &[u8], channels: u32) -> Result<(Vec<u8>, Vec<bool>)> {
    let mut r = BitReader::new(setup);
    if r.read(8) != 5 || (0..6).map(|_| r.read(8) as u8).collect::<Vec<_>>() != b"vorbis" {
        return Err(AudioError::Invalid("not a Vorbis setup header".into()));
    }

    let mut w = BitWriter::new();
    let count = r.read(8) + 1;
    w.write(count - 1, 8);
    let mut candidates = vec![];
    for _ in 0..count {
        let mut book = BitWriter::new();
        copy_codebook(&mut r, &mut book)?;
        let key = (book.bit_len(), book.into_bytes());
        candidates.push(
            library_index()
                .get(&key)
                .cloned()
                .ok_or_else(|| AudioError::Unsupported("a codebook that is not in Wwise's library".into()))?,
        );
    }
    for id in pick_ids(&candidates) {
        w.write(id, 10);
    }

    let times = r.read(6) + 1;
    for _ in 0..times {
        r.read(16);
    }

    let cp = |w: &mut BitWriter, r: &mut BitReader, n: u32| {
        let v = r.read(n);
        w.write(v, n);
        v
    };
    let floors = cp(&mut w, &mut r, 6) + 1;
    for _ in 0..floors {
        if r.read(16) != 1 {
            return Err(AudioError::Unsupported("a floor 0 (Wwise has only floor 1)".into()));
        }
        let partitions = cp(&mut w, &mut r, 5);
        let classes: Vec<u32> = (0..partitions).map(|_| cp(&mut w, &mut r, 4)).collect();
        let mut dims = vec![0; classes.iter().copied().max().map_or(0, |m| m as usize + 1)];
        for d in &mut dims {
            *d = cp(&mut w, &mut r, 3) + 1;
            let sub = cp(&mut w, &mut r, 2);
            if sub != 0 {
                cp(&mut w, &mut r, 8);
            }
            for _ in 0..(1 << sub) {
                cp(&mut w, &mut r, 8);
            }
        }
        cp(&mut w, &mut r, 2);
        let range = cp(&mut w, &mut r, 4);
        for c in classes {
            for _ in 0..dims[c as usize] {
                cp(&mut w, &mut r, range);
            }
        }
    }

    let residues = cp(&mut w, &mut r, 6) + 1;
    for _ in 0..residues {
        w.write(r.read(16), 2);
        cp(&mut w, &mut r, 24);
        cp(&mut w, &mut r, 24);
        cp(&mut w, &mut r, 24);
        let classifications = cp(&mut w, &mut r, 6) + 1;
        cp(&mut w, &mut r, 8);
        let cascade: Vec<u32> = (0..classifications)
            .map(|_| {
                let low = cp(&mut w, &mut r, 3);
                let high = if cp(&mut w, &mut r, 1) == 1 { cp(&mut w, &mut r, 5) } else { 0 };
                high * 8 + low
            })
            .collect();
        for c in cascade {
            for k in 0..8 {
                if c & (1 << k) != 0 {
                    cp(&mut w, &mut r, 8);
                }
            }
        }
    }

    let mappings = cp(&mut w, &mut r, 6) + 1;
    for _ in 0..mappings {
        r.read(16);
        let submaps = if cp(&mut w, &mut r, 1) == 1 { cp(&mut w, &mut r, 4) + 1 } else { 1 };
        if cp(&mut w, &mut r, 1) == 1 {
            let steps = cp(&mut w, &mut r, 8) + 1;
            for _ in 0..steps * 2 {
                cp(&mut w, &mut r, ilog(channels - 1));
            }
        }
        cp(&mut w, &mut r, 2);
        if submaps > 1 {
            for _ in 0..channels {
                cp(&mut w, &mut r, 4);
            }
        }
        for _ in 0..submaps * 3 {
            cp(&mut w, &mut r, 8);
        }
    }

    let modes = cp(&mut w, &mut r, 6) + 1;
    let mut flags = vec![];
    for _ in 0..modes {
        flags.push(cp(&mut w, &mut r, 1) == 1);
        r.read(32);
        cp(&mut w, &mut r, 8);
    }

    Ok((w.into_bytes(), flags))
}

/// The channel mask Wwise writes for a channel count.
fn channel_mask(channels: u16) -> u32 {
    match channels {
        1 => 0x4,
        2 => 0x3,
        4 => 0x603,
        5 => 0x607,
        6 => 0x60F,
        _ => 0,
    }
}

/// Encodes a sound at a quality (Wwise's 0 to 10, one of [`QUALITIES`]).
///
/// # Errors
///
/// For 6 channels or more (not yet), a quality without Wwise's setup, or an encoder failure.
pub fn encode_wem(pcm: &Pcm, quality: u8) -> Result<Vec<u8>> {
    let channels = pcm.channels;
    if !(1..=5).contains(&channels) {
        return Err(AudioError::Unsupported(format!("{channels} channels (1 to 5 for now)")));
    }
    if !QUALITIES.contains(&quality) {
        return Err(AudioError::Unsupported(format!("quality {quality} (Wwise's own setup is not reproduced)")));
    }
    if pcm.frames() == 0 {
        return Err(AudioError::Invalid("an empty sound".into()));
    }

    let (setup, packets) = unsafe { run_encoder(pcm, f32::from(quality) / 10.0)? };
    let (ww_setup, block_flags) = wwise_setup(&setup, u32::from(channels))?;
    let hash = fnv::bytes(&ww_setup);
    let (alloc, alloc64) =
        setup_allocs::allocs(hash).ok_or_else(|| AudioError::Unsupported(format!("setup {hash} has no known decoder sizes")))?;

    let mode_bits = ilog(block_flags.len() as u32 - 1);
    let blocksizes = [256u64, 2048];
    let (mut audio, mut max_packet, mut decoded, mut previous) = (vec![], 0usize, 0u64, None);
    for (bytes, bits) in &packets {
        let mut r = BitReader::new(bytes);
        r.read(1);
        let mode = r.read(mode_bits);
        let long = *block_flags.get(mode as usize).ok_or_else(|| AudioError::Invalid("a packet's mode".into()))?;
        if long {
            r.read(2);
        }
        let mut w = BitWriter::new();
        w.write(mode, mode_bits);
        let left = bits.saturating_sub(r.position());
        w.copy(&mut r, left);
        let out = w.into_bytes();
        max_packet = max_packet.max(out.len());
        audio.extend_from_slice(&(out.len() as u16).to_le_bytes());
        audio.extend_from_slice(&out);
        let size = blocksizes[usize::from(long)];
        if let Some(p) = previous {
            decoded += p / 4 + size / 4;
        }
        previous = Some(size);
    }

    let frames = pcm.frames() as u64;
    let extra = decoded.saturating_sub(frames) as u16;
    let setup_packet_size = 2 + ww_setup.len();
    let data_size = setup_packet_size + audio.len();
    let mut data = Vec::with_capacity(data_size);
    data.extend_from_slice(&(ww_setup.len() as u16).to_le_bytes());
    data.extend_from_slice(&ww_setup);
    data.extend_from_slice(&audio);

    let mut fmt = vec![];
    fmt.extend_from_slice(&0xFFFFu16.to_le_bytes());
    fmt.extend_from_slice(&channels.to_le_bytes());
    fmt.extend_from_slice(&pcm.sample_rate.to_le_bytes());
    let avg = (data_size as f64 * f64::from(pcm.sample_rate) / frames as f64).round() as u32;
    fmt.extend_from_slice(&avg.to_le_bytes());
    fmt.extend_from_slice(&[0, 0, 0, 0, 0x30, 0, 0, 0]);
    fmt.extend_from_slice(&channel_mask(channels).to_le_bytes());
    for v in [frames as u32, setup_packet_size as u32, data_size as u32] {
        fmt.extend_from_slice(&v.to_le_bytes());
    }
    fmt.extend_from_slice(&0u16.to_le_bytes());
    fmt.extend_from_slice(&extra.to_le_bytes());
    fmt.extend_from_slice(&0u32.to_le_bytes());
    fmt.extend_from_slice(&(setup_packet_size as u32).to_le_bytes());
    fmt.extend_from_slice(&(max_packet as u16).to_le_bytes());
    fmt.extend_from_slice(&extra.to_le_bytes());
    for v in [alloc, alloc64, hash] {
        fmt.extend_from_slice(&v.to_le_bytes());
    }
    fmt.extend_from_slice(&[8, 11]);

    let mut body = b"WAVEfmt ".to_vec();
    body.extend_from_slice(&(fmt.len() as u32).to_le_bytes());
    body.extend_from_slice(&fmt);
    body.extend_from_slice(b"data");
    body.extend_from_slice(&(data.len() as u32).to_le_bytes());
    body.extend_from_slice(&data);
    if data.len() % 2 == 1 {
        body.push(0);
    }
    let mut wem = b"RIFF".to_vec();
    wem.extend_from_slice(&(body.len() as u32).to_le_bytes());
    wem.extend_from_slice(&body);
    Ok(wem)
}

/// Runs aoTuV: the standard setup packet and every audio packet with its exact bit count.
unsafe fn run_encoder(pcm: &Pcm, quality: f32) -> Result<(Vec<u8>, Vec<(Vec<u8>, usize)>)> {
    unsafe {
        let channels = usize::from(pcm.channels);
        let mut vi = MaybeUninit::<vorbis_info>::zeroed().assume_init();
        vorbis_info_init(&mut vi);
        if vorbis_encode_init_vbr(&mut vi, channels as _, SETUP_RATE as _, quality) != 0 {
            vorbis_info_clear(&mut vi);
            return Err(AudioError::Vorbis("aoTuV refused the settings".into()));
        }

        let mut vc = MaybeUninit::<vorbis_comment>::zeroed().assume_init();
        vorbis_comment_init(&mut vc);
        let mut vd = MaybeUninit::<vorbis_dsp_state>::zeroed().assume_init();
        vorbis_analysis_init(&mut vd, &mut vi);
        let mut vb = MaybeUninit::<vorbis_block>::zeroed().assume_init();
        vorbis_block_init(&mut vd, &mut vb);

        let mut headers = [MaybeUninit::<ogg_packet>::zeroed().assume_init(); 3];
        let [h0, h1, h2] = &mut headers;
        vorbis_analysis_headerout(&mut vd, &mut vc, h0, h1, h2);
        let setup = std::slice::from_raw_parts(h2.packet, h2.bytes as usize).to_vec();

        let mut packets = vec![];
        let mut pending_bits = std::collections::VecDeque::new();
        let mut drain = |vd: &mut vorbis_dsp_state, vb: &mut vorbis_block, packets: &mut Vec<(Vec<u8>, usize)>| {
            while vorbis_analysis_blockout(vd, vb) == 1 {
                vorbis_analysis(vb, std::ptr::null_mut());
                pending_bits.push_back(vb.opb.endbyte as usize * 8 + vb.opb.endbit as usize);
                vorbis_bitrate_addblock(vb);
                let mut op = MaybeUninit::<ogg_packet>::zeroed().assume_init();
                while vorbis_bitrate_flushpacket(vd, &mut op) == 1 {
                    let bytes = std::slice::from_raw_parts(op.packet, op.bytes as usize).to_vec();
                    let bits = pending_bits.pop_front().unwrap_or(bytes.len() * 8);
                    packets.push((bytes, bits));
                }
            }
        };

        const CHUNK: usize = 4096;
        let frames = pcm.frames();
        let mut done = 0;
        while done < frames {
            let n = CHUNK.min(frames - done);
            let buffer = vorbis_analysis_buffer(&mut vd, n as _);
            for c in 0..channels {
                let out = std::slice::from_raw_parts_mut(*buffer.add(c), n);
                for (k, v) in out.iter_mut().enumerate() {
                    *v = pcm.samples[(done + k) * channels + c];
                }
            }
            vorbis_analysis_wrote(&mut vd, n as _);
            done += n;
            drain(&mut vd, &mut vb, &mut packets);
        }
        vorbis_analysis_wrote(&mut vd, 0);
        drain(&mut vd, &mut vb, &mut packets);

        vorbis_block_clear(&mut vb);
        vorbis_dsp_clear(&mut vd);
        vorbis_comment_clear(&mut vc);
        vorbis_info_clear(&mut vi);
        Ok((setup, packets))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decode::decode_wem;

    fn tone(channels: u16, rate: u32, seconds: f32) -> Pcm {
        let frames = (rate as f32 * seconds) as usize;
        let mut samples = Vec::with_capacity(frames * usize::from(channels));
        for n in 0..frames {
            for c in 0..channels {
                let t = n as f32 / rate as f32;
                samples.push(0.3 * (std::f32::consts::TAU * (220.0 + 110.0 * f32::from(c)) * t).sin());
            }
        }
        Pcm { channels, sample_rate: rate, samples }
    }

    #[test]
    fn encodes_and_decodes_back() {
        for (channels, rate) in [(1, 28_000), (2, 44_100), (4, 32_000)] {
            let pcm = tone(channels, rate, 1.0);
            let wem = encode_wem(&pcm, DEFAULT_QUALITY).unwrap();
            let back = decode_wem(&wem).unwrap();
            assert_eq!((back.channels, back.sample_rate, back.frames()), (channels, rate, pcm.frames()));
            let err: f32 = pcm.samples.iter().zip(&back.samples).map(|(a, b)| (a - b).powi(2)).sum::<f32>() / pcm.samples.len() as f32;
            assert!(err < 1e-3, "{channels} channels at {rate} Hz: mean square error {err}");
        }
    }

    #[test]
    fn mono_setup_is_wwise_voice() {
        // Wwise 2014.1.3's Voice preset (quality 4) setup: the oracle's hash (docs/formats/wem.md).
        let wem = encode_wem(&tone(1, 44_100, 0.5), 4).unwrap();
        let w = wce_audio_format::wem::Wem::read(&wem).unwrap();
        assert_eq!(w.vorbis().unwrap().codebook_hash, 3_017_712_712);
        assert_eq!(fnv::bytes(w.vorbis_setup().unwrap()), 3_017_712_712);
    }
}
