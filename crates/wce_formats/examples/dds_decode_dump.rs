//! Differential dump of the CPU DDS decoder, in the format of the C++ harness: every DDS below a folder, mips 0, 1, 3 and past the last, hashed.
//!
//!   cargo run --release -p wce_formats --example dds_decode_dump -- <folder>

use std::path::{Path, PathBuf};

use wce_formats::dds_decode::{dds_decode_mip, dds_info};

fn fnv(d: &[u8]) -> u64 {
    d.iter().fold(14695981039346656037, |h, &c| (h ^ u64::from(c)).wrapping_mul(1099511628211))
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    for e in std::fs::read_dir(dir).unwrap().flatten() {
        let p = e.path();
        if p.is_dir() {
            walk(&p, out);
        } else if p.extension().is_some_and(|x| x == "dds") {
            out.push(p);
        }
    }
}

fn main() {
    let root = PathBuf::from(std::env::args().nth(1).unwrap());
    let mut files = vec![];
    walk(&root, &mut files);
    files.sort();
    for p in files {
        let b = std::fs::read(&p).unwrap();
        let i = dds_info(&b);
        print!("{} {}x{} m{} {}", p.strip_prefix(&root).unwrap().to_string_lossy(), i.width, i.height, i.mips, i.format);
        if !i.format.is_empty() {
            for m in [0, 1, 3, 99] {
                match dds_decode_mip(&b, m) {
                    Ok(img) => print!(" {}x{}:{:016x}", img.w, img.h, fnv(&img.px)),
                    Err(why) => print!(" [{why}]"),
                }
            }
        }

        println!();
    }
}
