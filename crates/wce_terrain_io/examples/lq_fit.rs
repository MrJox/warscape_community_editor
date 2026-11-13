//! Exploration: candidate filters for a campaign map's `lf_height_map_lq.dds` (the LF quartered) against BOB's: per candidate the
//! samples that differ and the first of them.
//!
//!   cargo run --release -p wce_terrain_io --example lq_fit -- <kit raw_data> <map> <BOB's output folder>

use std::path::PathBuf;

use wce_terrain::{LfRole, MapKind};
use wce_terrain_io::{MapFiles, attila};

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let (raw, out) = (PathBuf::from(&a[0]), PathBuf::from(&a[2]));
    let files = MapFiles::from_dir(&raw.join("terrain/campaigns").join(&a[1])).expect("folder");
    let m = attila::read(&a[1], MapKind::Campaign, &files).expect("map");
    let f = m.lf(LfRole::Land).expect("LF").get();
    let (w0, h0) = (f.width() as usize, f.height() as usize);
    let l0: Vec<u16> = f.to_le_rows().as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes(*c)).collect();
    let want: Vec<u16> = std::fs::read(out.join("lf_height_map_lq.dds")).expect("lq")[128..]
        .as_chunks::<2>()
        .0
        .iter()
        .map(|c| u16::from_le_bytes(*c))
        .collect();
    let mid: Vec<u16> = std::fs::read(out.join("lf_height_map.dds")).expect("dds")[128..]
        .as_chunks::<2>()
        .0
        .iter()
        .map(|c| u16::from_le_bytes(*c))
        .collect();
    let (w1, h1) = (w0 / 2, h0 / 2);
    let (w2, h2) = (w1 / 2, h1 / 2);
    println!("level 0 {w0} x {h0}, lq {} samples (want {w2} x {h2} = {})", want.len(), w2 * h2);
    let inv = 1.0f32 / 65535.0;
    let box4 = |src: &[f32], w: usize, x: usize, y: usize| {
        (((src[y * w + x] + src[y * w + x + 1]) + src[(y + 1) * w + x]) + src[(y + 1) * w + x + 1]) * 0.25
    };
    let f0: Vec<f32> = l0.iter().map(|&v| f32::from(v) * inv).collect();
    let f1: Vec<f32> = (0..w1 * h1).map(|i| box4(&f0, w0, 2 * (i % w1), 2 * (i / w1))).collect();
    let m1: Vec<f32> = mid.iter().map(|&v| f32::from(v) * inv).collect();
    let cands: Vec<(&str, Vec<u16>)> = vec![
        ("float chain, trunc", (0..w2 * h2).map(|i| (box4(&f1, w1, 2 * (i % w2), 2 * (i / w2)) * 65535.0) as u16).collect()),
        ("float chain, round", (0..w2 * h2).map(|i| (box4(&f1, w1, 2 * (i % w2), 2 * (i / w2)) * 65535.0 + 0.5) as u16).collect()),
        ("from BOB's level 1 u16, trunc", (0..w2 * h2).map(|i| (box4(&m1, w1, 2 * (i % w2), 2 * (i / w2)) * 65535.0) as u16).collect()),
        (
            "from BOB's level 1 u16, round",
            (0..w2 * h2).map(|i| (box4(&m1, w1, 2 * (i % w2), 2 * (i / w2)) * 65535.0 + 0.5) as u16).collect(),
        ),
        (
            "level 1 ints, (a+b+c+d)/4",
            (0..w2 * h2)
                .map(|i| {
                    let (x, y) = (2 * (i % w2), 2 * (i / w2));
                    ((u32::from(mid[y * w1 + x])
                        + u32::from(mid[y * w1 + x + 1])
                        + u32::from(mid[(y + 1) * w1 + x])
                        + u32::from(mid[(y + 1) * w1 + x + 1]))
                        / 4) as u16
                })
                .collect(),
        ),
        (
            "level 1 ints, (a+b+c+d+2)/4",
            (0..w2 * h2)
                .map(|i| {
                    let (x, y) = (2 * (i % w2), 2 * (i / w2));
                    ((u32::from(mid[y * w1 + x])
                        + u32::from(mid[y * w1 + x + 1])
                        + u32::from(mid[(y + 1) * w1 + x])
                        + u32::from(mid[(y + 1) * w1 + x + 1])
                        + 2)
                        / 4) as u16
                })
                .collect(),
        ),
        (
            "level 0 4x4 floats, trunc",
            (0..w2 * h2)
                .map(|i| {
                    let (x, y) = (4 * (i % w2), 4 * (i / w2));
                    let mut s = 0f32;
                    for dy in 0..4 {
                        for dx in 0..4 {
                            s += f0[(y + dy) * w0 + x + dx];
                        }
                    }
                    (s / 16.0 * 65535.0) as u16
                })
                .collect(),
        ),
        (
            "level 0 4x4 ints, /16",
            (0..w2 * h2)
                .map(|i| {
                    let (x, y) = (4 * (i % w2), 4 * (i / w2));
                    let mut s = 0u32;
                    for dy in 0..4 {
                        for dx in 0..4 {
                            s += u32::from(l0[(y + dy) * w0 + x + dx]);
                        }
                    }
                    (s / 16) as u16
                })
                .collect(),
        ),
    ];

    for (name, v) in &cands {
        let bad: Vec<usize> = (0..v.len().min(want.len())).filter(|&i| v[i] != want[i]).collect();
        let first = bad.first().map(|&i| (i % w2, i / w2, v[i], want[i]));
        let worst = bad.iter().map(|&i| (i32::from(v[i]) - i32::from(want[i])).abs()).max().unwrap_or(0);
        println!("{name:34} {:9} differ (worst {worst}), first {first:?}", bad.len());
    }

    let (x, y) = (0usize, 0usize);
    println!(
        "BOB's lq (0,0) {}; level 1 around: {:?} {:?}",
        want[0],
        &mid[y * w1 + x..y * w1 + x + 2],
        &mid[(y + 1) * w1 + x..(y + 1) * w1 + x + 2]
    );
    println!("level 0 4x4: {:?}", (0..4).map(|dy| l0[dy * w0..dy * w0 + 4].to_vec()).collect::<Vec<_>>());
}
