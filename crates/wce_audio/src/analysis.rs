//! A sound's levels: sample peak, RMS and integrated loudness (ITU-R BS.1770-4: K-weighting, 400 ms blocks with 75 % overlap,
//! the absolute gate at -70 LUFS and the relative gate 10 LU below), for the tool's sound info.

use crate::decode::Pcm;

#[derive(Clone, Debug, PartialEq)]
pub struct Stats {
    /// The loudest sample, dBFS.
    pub peak_db: f64,
    /// The root mean square of every sample, dBFS.
    pub rms_db: f64,
    /// Integrated loudness, LUFS; None for a sound shorter than one 400 ms block or silent.
    pub loudness: Option<f64>,
    /// Samples at or over full scale.
    pub clipped: usize,
    /// The loudest sample of each channel, dBFS.
    pub channel_peaks_db: Vec<f64>,
    /// The mean of every sample (a DC offset), -1 to 1.
    pub dc_offset: f64,
}

fn db(amplitude: f64) -> f64 {
    if amplitude > 0.0 { 20.0 * amplitude.log10() } else { f64::NEG_INFINITY }
}

/// A biquad (a0 normalised to 1).
#[derive(Clone, Copy)]
struct Biquad {
    b: [f64; 3],
    a: [f64; 2],
}

impl Biquad {
    /// The K-weighting's two stages at a sample rate (B. De Man's analog-fitted parameters: exactly the standard's 48 kHz
    /// coefficients there, and the same curve at other rates).
    fn k_weighting(rate: f64) -> [Biquad; 2] {
        let (g, q, f0) = (3.999_843_853_973_347, 0.707_175_236_955_419_6, 1_681.974_450_955_533);
        let k = (std::f64::consts::PI * f0 / rate).tan();
        let vh = 10f64.powf(g / 20.0);
        let vb = vh.powf(0.499_666_774_154_541_6);
        let a0 = 1.0 + k / q + k * k;
        let shelf = Biquad {
            b: [(vh + vb * k / q + k * k) / a0, 2.0 * (k * k - vh) / a0, (vh - vb * k / q + k * k) / a0],
            a: [2.0 * (k * k - 1.0) / a0, (1.0 - k / q + k * k) / a0],
        };
        let (q, f0) = (0.500_327_037_323_877_3, 38.135_470_876_024_44);
        let k = (std::f64::consts::PI * f0 / rate).tan();
        let a0 = 1.0 + k / q + k * k;
        let high_pass = Biquad { b: [1.0, -2.0, 1.0], a: [2.0 * (k * k - 1.0) / a0, (1.0 - k / q + k * k) / a0] };
        [shelf, high_pass]
    }

    fn run(&self, x: &mut [f64]) {
        let (mut x1, mut x2, mut y1, mut y2) = (0.0, 0.0, 0.0, 0.0);
        for v in x.iter_mut() {
            let y = self.b[0] * *v + self.b[1] * x1 + self.b[2] * x2 - self.a[0] * y1 - self.a[1] * y2;
            (x2, x1, y2, y1) = (x1, *v, y1, y);
            *v = y;
        }
    }
}

/// BS.1770's channel weights in WAVE order: front left, right, centre 1; LFE 0; the surrounds 1.41.
fn weights(channels: usize) -> Vec<f64> {
    match channels {
        4 => vec![1.0, 1.0, 1.41, 1.41],
        5 => vec![1.0, 1.0, 1.0, 1.41, 1.41],
        6 => vec![1.0, 1.0, 1.0, 0.0, 1.41, 1.41],
        n => vec![1.0; n],
    }
}

pub fn analyse(pcm: &Pcm) -> Stats {
    let channels = usize::from(pcm.channels.max(1));
    let (mut peak, mut sum, mut total, mut clipped) = (0.0f64, 0.0f64, 0.0f64, 0);
    let mut channel_peaks = vec![0.0f64; channels];
    for (i, s) in pcm.samples.iter().enumerate() {
        let v = f64::from(s.abs());
        peak = peak.max(v);
        channel_peaks[i % channels] = channel_peaks[i % channels].max(v);
        sum += v * v;
        total += f64::from(*s);
        clipped += usize::from(v >= 1.0);
    }
    let n = pcm.samples.len().max(1) as f64;
    Stats {
        peak_db: db(peak),
        rms_db: db((sum / n).sqrt()),
        loudness: loudness(pcm, channels),
        clipped,
        channel_peaks_db: channel_peaks.into_iter().map(db).collect(),
        dc_offset: total / n,
    }
}

fn loudness(pcm: &Pcm, channels: usize) -> Option<f64> {
    let rate = f64::from(pcm.sample_rate);
    let frames = pcm.frames();
    let (block, step) = ((0.4 * rate) as usize, (0.1 * rate) as usize);
    if rate <= 0.0 || block == 0 || step == 0 || frames < block {
        return None;
    }
    let filters = Biquad::k_weighting(rate);
    // Each channel K-weighted and squared, then summed per 100 ms step so the overlapping blocks add four steps.
    let steps = frames / step;
    let mut power = vec![vec![0.0f64; steps]; channels];
    for (c, power) in power.iter_mut().enumerate() {
        let mut x: Vec<f64> = (0..frames).map(|f| f64::from(pcm.samples[f * channels + c])).collect();
        filters.iter().for_each(|f| f.run(&mut x));
        for (s, p) in power.iter_mut().enumerate() {
            *p = x[s * step..(s + 1) * step].iter().map(|v| v * v).sum();
        }
    }
    let w = weights(channels);
    let blocks: Vec<Vec<f64>> = (0..=steps.saturating_sub(4))
        .filter(|b| b + 4 <= steps)
        .map(|b| power.iter().map(|p| p[b..b + 4].iter().sum::<f64>() / (4 * step) as f64).collect())
        .collect();
    let level = |z: &[f64]| -0.691 + 10.0 * z.iter().zip(&w).map(|(z, w)| z * w).sum::<f64>().log10();
    let mean =
        |set: &[&Vec<f64>]| -> Vec<f64> { (0..channels).map(|c| set.iter().map(|z| z[c]).sum::<f64>() / set.len() as f64).collect() };
    let gated: Vec<&Vec<f64>> = blocks.iter().filter(|z| level(z) > -70.0).collect();
    if gated.is_empty() {
        return None;
    }
    let relative = level(&mean(&gated)) - 10.0;
    let gated: Vec<&Vec<f64>> = gated.into_iter().filter(|z| level(z) > relative).collect();
    if gated.is_empty() {
        return None;
    }
    Some(level(&mean(&gated)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(rate: u32, amplitude: f32, seconds: f32) -> Pcm {
        let n = (rate as f32 * seconds) as usize;
        let samples = (0..n).map(|i| amplitude * (2.0 * std::f32::consts::PI * 997.0 * i as f32 / rate as f32).sin()).collect();
        Pcm { channels: 1, sample_rate: rate, samples }
    }

    /// BS.1770's reference: a 0 dBFS 997 Hz sine in one channel is -3.01 LUFS, at any rate.
    #[test]
    fn reference_tone() {
        for rate in [48_000, 44_100, 32_000] {
            let stats = analyse(&sine(rate, 1.0, 5.0));
            let lufs = stats.loudness.unwrap();
            assert!((lufs + 3.01).abs() < 0.05, "{rate} Hz: {lufs}");
            assert!(stats.peak_db.abs() < 0.01 && (stats.rms_db + 3.01).abs() < 0.01);
        }
        let quieter = analyse(&sine(48_000, 0.1, 5.0)).loudness.unwrap();
        assert!((quieter + 23.01).abs() < 0.05, "{quieter}");
        assert_eq!(analyse(&sine(48_000, 1.0, 0.2)).loudness, None);
    }
}
