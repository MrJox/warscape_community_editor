//! Gate of the video layer (docs/plans/cinematic.md, step 2):
//! - every `.ca_vp8` of the game and the given mod packs gives its bytes back through rpfm_lib (`Movie::read` -> `to_ca_vp8`);
//! - every movie decodes: each frame of the small ones, the first `--frames` of the large ones (all with `--frames 0`), each
//!   frame giving a picture of the movie's size;
//! - a synthetic clip encodes, is written as a CAMV movie, read back and decodes to the same frame count within a PSNR floor.
//!
//! Exits non-zero on a failure.
//!
//!   video_check <game folder> [--frames N] [mod pack...]

use std::path::PathBuf;
use std::time::Instant;

use wce_vfs::{Config, GameProfile, Vfs};
use wce_video::{Decoder, Encoder, EncoderSettings, I420, Movie, yuv};

const PSNR_FLOOR: f64 = 30.0;

fn main() {
    let mut args = std::env::args().skip(1);
    let Some(game) = args.next() else {
        eprintln!("video_check <game folder> [--frames N] [mod pack...]");
        std::process::exit(2);
    };
    let mut large_frames = 120usize;
    let mut mods = vec![];
    while let Some(a) = args.next() {
        if a == "--frames" {
            large_frames = args.next().and_then(|n| n.parse().ok()).unwrap_or(120);
        } else {
            mods.push(a);
        }
    }

    let mut failed: Vec<String> = vec![];
    let mut config = Config::new(PathBuf::from(game), GameProfile::from_env().clone());
    config.use_mods = !mods.is_empty();
    config.mod_packs = Some(mods);
    let vfs = Vfs::mount(&config).expect("the game's packs");

    let started = Instant::now();
    let (mut movies, mut frames_decoded, mut decode_s) = (0usize, 0usize, 0.0f64);
    for path in vfs.list("", ".ca_vp8") {
        let bytes = vfs.read(path).unwrap_or_else(|e| panic!("{path}: {e}"));
        let movie = match Movie::read(&bytes) {
            Ok(m) => m,
            Err(e) => {
                failed.push(format!("{path}: {e}"));
                continue;
            }
        };
        movies += 1;
        match movie.to_ca_vp8() {
            Ok(back) if back == bytes => {}
            Ok(back) => failed.push(format!(
                "{path}: written back as {} bytes, {} read; first difference at {:?}",
                back.len(),
                bytes.len(),
                back.iter().zip(&bytes).position(|(a, b)| a != b)
            )),
            Err(e) => failed.push(format!("{path}: {e}")),
        }
        let large = movie.width() * movie.height() > 640 * 480;
        let count = if large && large_frames > 0 { large_frames.min(movie.frame_count()) } else { movie.frame_count() };
        let mut decoder = Decoder::new(4).expect("decoder");
        let t = Instant::now();
        for i in 0..count {
            let frame = match movie.frame(i) {
                Ok(f) => f,
                Err(e) => {
                    failed.push(format!("{path}: {e}"));
                    break;
                }
            };
            match decoder.decode(frame) {
                Ok(Some(p)) if p.width == movie.width() && p.height == movie.height() => frames_decoded += 1,
                Ok(Some(p)) => {
                    failed.push(format!(
                        "{path}: frame {i} is {} x {}, the movie {} x {}",
                        p.width,
                        p.height,
                        movie.width(),
                        movie.height()
                    ));
                    break;
                }
                Ok(None) => failed.push(format!("{path}: frame {i} shows no picture")),
                Err(e) => {
                    failed.push(format!("{path}: frame {i}: {e}"));
                    break;
                }
            }
        }
        decode_s += t.elapsed().as_secs_f64();
    }
    println!("movies {movies}, frames decoded {frames_decoded} in {decode_s:.1} s");

    // Encode a synthetic clip: moving gradients and a bar, 30 fps.
    let (w, h, n) = (320u32, 180u32, 90usize);
    let source: Vec<I420> = (0..n)
        .map(|f| {
            let mut rgba = vec![0u8; (w * h * 4) as usize];
            for y in 0..h {
                for x in 0..w {
                    let o = ((y * w + x) * 4) as usize;
                    rgba[o] = ((x + f as u32 * 2) % 256) as u8;
                    rgba[o + 1] = ((y * 255) / h) as u8;
                    rgba[o + 2] = if (x as i32 - f as i32 * 3).rem_euclid(w as i32) < 24 { 230 } else { 40 };
                    rgba[o + 3] = 255;
                }
            }
            yuv::from_rgba(w, h, &rgba)
        })
        .collect();
    let t = Instant::now();
    let mut settings = EncoderSettings::movie(w, h, 30);
    settings.target_kbps = 2000;
    let mut encoder = Encoder::new(&settings).expect("encoder");
    let mut packets = vec![];
    for (i, p) in source.iter().enumerate() {
        packets.extend(encoder.encode(p, i == 0).expect("encode"));
    }
    packets.extend(encoder.finish().expect("flush"));
    let encode_s = t.elapsed().as_secs_f64();

    let written = Movie::from_frames(w, h, 30, &packets).and_then(|m| m.to_ca_vp8());
    match written.and_then(|b| Movie::read(&b).map(|m| (b, m))) {
        Ok((bytes, movie)) => {
            let mut decoder = Decoder::new(1).expect("decoder");
            let mut shown = vec![];
            for i in 0..movie.frame_count() {
                if let Some(p) = decoder.decode(movie.frame(i).unwrap()).expect("decode") {
                    shown.push(p);
                }
            }
            let worst = shown.iter().zip(&source).map(|(a, b)| psnr(&a.y, &b.y)).fold(f64::INFINITY, f64::min);
            println!(
                "encoded {n} frames {w} x {h} in {encode_s:.2} s: {} packets, {} bytes as CAMV, {} pictures back, worst luma PSNR {worst:.1} dB, {} fps header",
                packets.len(),
                bytes.len(),
                shown.len(),
                movie.fps()
            );
            if shown.len() != n {
                failed.push(format!("encode: {} pictures back of {n}", shown.len()));
            }
            if worst < PSNR_FLOOR {
                failed.push(format!("encode: worst PSNR {worst:.1} dB under {PSNR_FLOOR}"));
            }
            if &bytes[..4] != b"CAMV" || bytes[4..6] != [0, 0] {
                failed.push("encode: not a version-0 CAMV".into());
            }
        }
        Err(e) => failed.push(format!("encode: {e}")),
    }

    println!("{} s", started.elapsed().as_secs());
    if failed.is_empty() {
        println!("OK");
    } else {
        for f in &failed {
            println!("FAIL {f}");
        }
        std::process::exit(1);
    }
}

fn psnr(a: &[u8], b: &[u8]) -> f64 {
    let mse = a.iter().zip(b).map(|(x, y)| (*x as f64 - *y as f64).powi(2)).sum::<f64>() / a.len() as f64;
    if mse == 0.0 { 99.0 } else { 10.0 * (255.0 * 255.0 / mse).log10() }
}
