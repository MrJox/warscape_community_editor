//! Round trip of the files of exported (binary) tiles, per kind of file (docs/plans/tile_roundtrip.md, Phase 2).
//!
//!   cargo run --release -p wce_tile_io --example tile_binary_check -- <folder>... [--filter TEXT] [--show N]
//!
//! Every file below the folders (an extraction made by `tile_pack_inventory --extract`, or a kit's working_data): its codec
//! (`wce_tile_io::attila::binary`), decode and encode, compared with the file. Prints per kind of file and codec: files, identical,
//! differing, not read, and the first problems.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use rayon::prelude::*;
use wce_tile_io::attila::binary::{Codec, codec_of, round_trip};

fn files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            files(&p, out);
        } else {
            out.push(p);
        }
    }
}

/// `rome_med.tree_list.bin` -> `X.tree_list.bin`, digits -> N.
fn kind_of(name: &str) -> String {
    let name = name.to_lowercase();
    let name = ["rome_", "att_", "tdd_"].iter().find_map(|p| name.strip_prefix(p)).map_or(name.clone(), |rest| {
        let tail = rest.find(['.', '_']).map_or("", |i| &rest[i..]);
        format!("X{tail}")
    });

    let mut out = String::new();
    let mut digit = false;
    for c in name.chars() {
        if c.is_ascii_digit() {
            if !digit {
                out.push('N');
            }

            digit = true;
        } else {
            out.push(c);
            digit = false;
        }
    }

    out
}

#[derive(Default)]
struct Counts {
    files: usize,
    identical: usize,
    differ: usize,
    failed: usize,
    bytes: u64,
}

fn main() {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    let opt = |name: &str| args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned();
    let filter = opt("--filter");
    let show = opt("--show").and_then(|n| n.parse().ok()).unwrap_or(30);
    let mut all = vec![];
    let mut skip = false;
    for a in &args {
        if skip {
            skip = false;
            continue;
        }

        if a.starts_with("--") {
            skip = true;
            continue;
        }

        files(Path::new(a), &mut all);
    }

    if let Some(f) = &filter {
        all.retain(|p| p.to_string_lossy().contains(f.as_str()));
    }

    let counts: Mutex<BTreeMap<(String, Codec), Counts>> = Mutex::default();
    let problems: Mutex<Vec<String>> = Mutex::default();
    all.par_iter().for_each(|p| {
        let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let Ok(bytes) = std::fs::read(p) else { return };
        let codec = codec_of(&name, &bytes[..bytes.len().min(64)]);
        let result = round_trip(codec, &bytes);
        let mut c = counts.lock().expect("counts");
        let e = c.entry((kind_of(&name), codec)).or_default();
        e.files += 1;
        e.bytes += bytes.len() as u64;
        match result {
            Ok(out) if out == bytes => e.identical += 1,
            Ok(out) => {
                e.differ += 1;
                let i = out.iter().zip(&bytes).take_while(|(a, b)| a == b).count();
                problems.lock().expect("problems").push(format!("{}: differs at byte {i} (sizes {} / {})", p.display(), bytes.len(), out.len()));
            }

            Err(err) => {
                e.failed += 1;
                problems.lock().expect("problems").push(format!("{}: {err}", p.display()));
            }
        }
    });

    println!("{} files", all.len());
    println!("{:<44} {:<18} {:>7} {:>9} {:>7} {:>7} {:>9}", "kind", "codec", "files", "identical", "differ", "failed", "MB");
    let mut bad = 0;
    for ((kind, codec), c) in counts.into_inner().expect("counts") {
        bad += c.differ + c.failed;
        println!("{kind:<44} {:<18} {:>7} {:>9} {:>7} {:>7} {:>9.1}", format!("{codec:?}"), c.files, c.identical, c.differ, c.failed, c.bytes as f64 / 1e6);
    }

    let mut problems = problems.into_inner().expect("problems");
    problems.sort();
    if !problems.is_empty() {
        println!("problems ({}; first {show}):", problems.len());
        for p in problems.iter().take(show) {
            println!("  {p}");
        }
    }

    std::process::exit(i32::from(bad != 0));
}
