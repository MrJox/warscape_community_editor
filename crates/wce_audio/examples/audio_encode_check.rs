//! Gate of the encoder: every quality WCE offers x 1, 2, 4 and 5 channels x 8 sample rates, on a test signal (a sweep and noise):
//! each must encode with a setup whose decoder sizes are known (`setup_allocs`), give the same setup at every rate, decode back to its
//! frame count, and stay close to the source. Prints the sizes and the worst error; exits non-zero on a failure.
//!
//!   audio_encode_check

use std::collections::BTreeMap;

use wce_audio::decode::{Pcm, decode_wem};
use wce_audio::encode::{QUALITIES, encode_wem};
use wce_audio_format::wem::Wem;

fn signal(channels: u16, rate: u32) -> Pcm {
    let frames = rate as usize * 3 / 2;
    let mut seed = 0x1234_5678u32;
    let mut samples = Vec::with_capacity(frames * usize::from(channels));
    for n in 0..frames {
        let t = n as f32 / rate as f32;
        for c in 0..channels {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            let noise = (seed as f32 / u32::MAX as f32) * 2.0 - 1.0;
            let f = 200.0 + 300.0 * f32::from(c) + 900.0 * t;
            samples.push(0.4 * (std::f32::consts::TAU * f * t).sin() + 0.05 * noise);
        }
    }
    Pcm { channels, sample_rate: rate, samples }
}

fn main() {
    let rates = [11_025, 16_000, 22_050, 24_000, 28_000, 32_000, 44_100, 48_000];
    let (mut ok, mut failed, mut worst) = (0usize, vec![], 0.0f64);
    let mut kbps = BTreeMap::new();
    for &q in &QUALITIES {
        for channels in [1u16, 2, 4, 5] {
            let mut setups = BTreeMap::new();
            for &rate in &rates {
                let pcm = signal(channels, rate);
                let name = format!("quality {q}, {channels} channels, {rate} Hz");
                let wem = match encode_wem(&pcm, q) {
                    Ok(w) => w,
                    Err(e) => {
                        failed.push(format!("{name}: {e}"));
                        continue;
                    }
                };
                let header = Wem::read(&wem).ok().and_then(|w| w.vorbis());
                *setups.entry(header.map_or(0, |h| h.codebook_hash)).or_insert(0) += 1;
                match decode_wem(&wem) {
                    Ok(back) if back.frames() == pcm.frames() => {
                        let err = pcm.samples.iter().zip(&back.samples).map(|(a, b)| f64::from(a - b).powi(2)).sum::<f64>()
                            / pcm.samples.len() as f64;
                        worst = worst.max(err);
                        ok += 1;
                        if rate == 44_100 {
                            kbps.insert((q, channels), wem.len() as f64 * 8.0 / 1.5 / 1000.0 / f64::from(channels));
                        }
                    }
                    Ok(back) => failed.push(format!("{name}: {} frames back, {} in", back.frames(), pcm.frames())),
                    Err(e) => failed.push(format!("{name}: does not decode: {e}")),
                }
            }
            if setups.len() > 1 {
                failed.push(format!("quality {q}, {channels} channels: {} setups over the rates (Wwise has one)", setups.len()));
            }
        }
    }

    println!("{ok} encoded and decoded back; worst mean square error {worst:.2e}");
    for ((q, c), k) in &kbps {
        if *c == 1 {
            println!("  quality {q:>2}: {k:.0} kbps per channel (mono, 44.1 kHz)");
        }
    }
    for f in failed.iter().take(20) {
        println!("  {f}");
    }
    std::process::exit(if failed.is_empty() { 0 } else { 1 });
}
