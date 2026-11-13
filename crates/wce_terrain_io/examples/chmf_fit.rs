//! Exploration: a campaign map's `lf_height_map.data` (BOB's) against the LF: which LF samples a region's samples are, for candidate
//! strides.
//!
//!   cargo run --release -p wce_terrain_io --example chmf_fit -- <kit raw_data> <map> <BOB's lf_height_map.data>

use std::path::PathBuf;

use wce_formats::chmf::ChmfFile;
use wce_terrain::{LfRole, MapKind};
use wce_terrain_io::attila::heights::convert;
use wce_terrain_io::{MapFiles, attila};

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let raw = PathBuf::from(&a[0]);
    let files = MapFiles::from_dir(&raw.join("terrain/campaigns").join(&a[1])).expect("folder");
    let m = attila::read(&a[1], MapKind::Campaign, &files).expect("map");
    let f = m.lf(LfRole::Land).expect("LF").get();
    let w0 = f.width() as usize;
    let l0: Vec<u16> = f.to_le_rows().as_chunks::<2>().0.iter().map(|c| convert(u16::from_le_bytes(*c))).collect();
    let c = ChmfFile::parse(&std::fs::read(&a[2]).expect("chmf")).expect("chmf");
    for region in [0usize, 1, 128, 5000] {
        let (sw, sh, s) = c.region_samples(region).expect("region");
        let r = c.regions[region].rect;
        println!("region {region} rect {r:?} {sw} x {sh}: first row {:?}", &s[..8]);
        println!("  first column {:?}", (0..8).map(|y| s[y * sw as usize]).collect::<Vec<_>>());
        // Candidates: LF (x0 + sx * i, y0 + sy * j) for strides and origins in level-0 samples.
        for (name, sx, sy) in [("x 8 y 1", 8, 1), ("x 1 y 1", 1, 1), ("x 8 y 8", 8, 8), ("x 1 y 8", 1, 8)] {
            let (x0, y0) = (r[0] as usize * 8, r[1] as usize * 8);
            let hits = (0..sh as usize)
                .flat_map(|j| (0..sw as usize).map(move |i| (i, j)))
                .filter(|&(i, j)| l0.get((y0 + j * sy) * w0 + x0 + i * sx) == Some(&s[j * sw as usize + i]))
                .count();
            println!("  {name}: {hits} of {} samples", sw * sh);
        }
    }

    // Each 16-row block of region 0 and 128: the (stride, x offset, y offset) of the LF it matches best.
    for region in [0usize, 128] {
        let (sw, _, s) = c.region_samples(region).expect("region");
        let r = c.regions[region].rect;
        for b in 0..8usize {
            let mut best = (0, 0, 0, 0);
            for stride in [8usize, 4, 2, 1, 16] {
                for oy in 0..(16 * stride * 8) {
                    for ox in 0..(stride * 8).min(64) {
                        let (x0, y0) = (r[0] as usize * 8 + ox, r[1] as usize * 8 + oy);
                        let hits = (0..16usize)
                            .flat_map(|j| (0..16usize).map(move |i| (i, j)))
                            .filter(|&(i, j)| l0.get((y0 + j * stride) * w0 + x0 + i * stride) == Some(&s[(16 * b + j) * sw as usize + i]))
                            .count();
                        if hits > best.0 {
                            best = (hits, stride, ox, oy);
                        }
                    }
                }
            }

            println!("region {region} block {b}: best {best:?} (hits, stride, x offset, y offset in LF samples)");
        }
    }

    println!("LF row 0 {:?}\nLF column 0 {:?}", &l0[..12], (0..12).map(|y| l0[y * w0]).collect::<Vec<_>>());
}
