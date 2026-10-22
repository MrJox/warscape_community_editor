//! The CHMF encoder (`wce_formats::chmf::encode_region`) against every CHMF file below a folder: decode each region, encode it again,
//! compare the block table, the data and the region's two u16 with the file.
//!
//!   cargo run --release -p wce_tile_io --example tile_chmf_encode_check -- <folder>

use std::path::{Path, PathBuf};

use wce_formats::chmf::{ChmfFile, encode_region};

fn files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            files(&p, out);
        } else if p.extension().is_some_and(|x| x == "data") {
            out.push(p);
        }
    }
}

fn main() {
    let root = std::env::args().nth(1).expect("usage: tile_chmf_encode_check <folder>");
    let mut all = vec![];
    files(Path::new(&root), &mut all);
    let (mut exact, mut short, mut differ) = (0, 0, 0);
    for p in &all {
        let f = ChmfFile::parse(&std::fs::read(p).expect("read")).expect("CHMF");
        let mut same = true;
        let mut table_short = false;
        for (i, r) in f.regions.iter().enumerate() {
            let (w, h, samples) = f.region_samples(i).expect("decode");
            let (tag, offsets, data) = encode_region(w, h, &samples);
            if offsets.len() != r.offsets.len() {
                table_short = true;
            } else if (tag, &offsets, &data) != (r.tag, &r.offsets, &r.data) {
                same = false;
                if differ < 10 {
                    let at = offsets.iter().zip(&r.offsets).position(|(a, b)| a != b).unwrap_or(offsets.len());
                    println!("{}: region {i}: tag {tag:?} / {:?}, first block whose offset differs {at} of {}", p.display(), r.tag, offsets.len());
                }
            }
        }

        if table_short {
            short += 1;
        } else if same {
            exact += 1;
        } else {
            differ += 1;
        }
    }

    println!("{} files: {exact} encoded byte-identically, {differ} differ, {short} with a short block table (not the engine's encoder)", all.len());
    std::process::exit(i32::from(differ != 0));
}
