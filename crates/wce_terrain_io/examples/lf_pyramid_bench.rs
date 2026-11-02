//! The LF picking pyramid (`wce_terrain::pyramid`) on a real map: the map folder's `lf_heights.tif` read into the model, the pyramid
//! made, rays from above at random places and angles (time and hits), and one LF edit's update.
//!
//!   cargo run --release -p wce_terrain_io --example lf_pyramid_bench -- <map folder>

use std::time::Instant;

use wce_terrain::pyramid::HeightPyramid;
use wce_terrain::{LfRole, MapKind};
use wce_terrain_io::{MapFiles, attila};

fn main() {
    let Some(dir) = std::env::args().nth(1).map(std::path::PathBuf::from) else {
        eprintln!("lf_pyramid_bench <map folder>");
        std::process::exit(2);
    };

    let t0 = Instant::now();
    let files = MapFiles::from_dir(&dir).expect("the folder");
    let m = attila::read("map", MapKind::Battle, &files).expect("the map");
    let Some(lf) = m.lf(LfRole::Land).map(|i| i.get().clone()) else {
        eprintln!("no lf_heights.tif");
        std::process::exit(1);
    };

    println!("read {} x {} samples in {} ms", lf.width(), lf.height(), t0.elapsed().as_millis());
    let t0 = Instant::now();
    let mut p = HeightPyramid::new(lf.width(), lf.height(), |x, y| lf.at(x, y));
    println!("pyramid in {} ms, range {:?}", t0.elapsed().as_millis(), p.range());

    // A simple generator (the same rays every run).
    let mut s = 0x1234_5678u64;
    let mut rnd = || {
        s = s.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
        (s >> 33) as f32 / (1u64 << 31) as f32
    };

    let (w, h) = (lf.width() as f32, lf.height() as f32);
    let n = 10_000;
    let t0 = Instant::now();
    let mut hits = 0;
    for _ in 0..n {
        let o = [rnd() * w, 70_000.0, rnd() * h];
        let d = [(rnd() - 0.5) * 2.0, -40.0 - rnd() * 400.0, (rnd() - 0.5) * 2.0];
        hits += usize::from(p.intersect(o, d, |x, y| lf.at(x, y)).is_some());
    }

    let us = t0.elapsed().as_secs_f64() * 1e6 / f64::from(n);
    println!("{n} rays: {hits} hits, {us:.1} us a ray");

    // A brush-sized edit's update.
    let mut lf = lf;
    let (cx, cy) = (lf.width() / 2, lf.height() / 2);
    for y in cy - 128..cy + 128 {
        for x in cx - 128..cx + 128 {
            lf.set(x, y, lf.at(x, y).saturating_add(500));
        }
    }

    let t0 = Instant::now();
    p.update((cx - 128, cy - 128, cx + 128, cy + 128), |x, y| lf.at(x, y));
    println!("update of 256 x 256 samples in {} us", t0.elapsed().as_micros());
}
