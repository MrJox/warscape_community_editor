//! Encodes the same synthetic clip and prints a hash of the bytes: run it twice (two processes) to see whether the encoder gives
//! the same movie. `encode_determinism <threads> [lag]`.

use wce_video::{Encoder, EncoderSettings, Movie, yuv};

fn main() {
    let a: Vec<u32> = std::env::args().skip(1).filter_map(|x| x.parse().ok()).collect();
    let (w, h) = (640u32, 360u32);
    let mut s = EncoderSettings::movie(w, h, 30);
    s.threads = a.first().copied().unwrap_or(s.threads);
    s.lag_in_frames = a.get(1).copied().unwrap_or(s.lag_in_frames);
    let mut enc = Encoder::new(&s).unwrap();
    let mut packets = vec![];
    for f in 0..60u32 {
        let rgba: Vec<u8> = (0..h)
            .flat_map(|y| {
                (0..w).flat_map(move |x| [((x * 3 + f * 7) % 256) as u8, ((y * 2 + x / 3) % 256) as u8, ((x ^ y ^ f) % 256) as u8, 255])
            })
            .collect();
        packets.extend(enc.encode(&yuv::from_rgba(w, h, &rgba), f == 0).unwrap());
    }
    packets.extend(enc.finish().unwrap());
    let bytes = Movie::from_frames(w, h, 30, &packets).unwrap().to_ca_vp8().unwrap();
    let hash = bytes.iter().fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ *b as u64).wrapping_mul(0x100_0000_01b3));
    println!("threads {} lag {}: {} bytes, hash {hash:016x}", s.threads, s.lag_in_frames, bytes.len());
}
