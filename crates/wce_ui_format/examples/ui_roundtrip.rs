//! The round-trip gate: every `Version0xx` file under the given folders must convert to XML and back to the same bytes.
//!
//!   cargo run --release -p wce_ui_format --example ui_roundtrip -- <folder> [<folder> ...] [--xml-out <dir>]
//!
//! Prints the totals per version and every failure with its reason. Exit code 1 if any file failed.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            walk(&p, out);
        } else {
            out.push(p);
        }
    }
}

fn main() {
    let mut roots = Vec::new();
    let mut xml_out: Option<PathBuf> = None;
    let mut game: Option<String> = None;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        if a == "--game" {
            game = args.next();
        } else if a == "--xml-out" {
            xml_out = args.next().map(PathBuf::from);
        } else {
            roots.push(PathBuf::from(a));
        }
    }
    let mut files = Vec::new();
    for r in &roots {
        walk(r, &mut files);
    }
    files.sort();
    // --game <folder>: every UI-looking file of the mounted packs, as the editor would see them.
    let mut pack_files: Vec<(String, Vec<u8>)> = Vec::new();
    if let Some(game) = game {
        let vfs = wce_vfs::Vfs::mount(&wce_vfs::Config::new(game, wce_vfs::GameProfile::ATTILA)).expect("mount");
        for path in vfs.files().keys() {
            let name = path.rsplit('/').next().unwrap_or(path);
            let ui_like = path.starts_with("ui/") && (!name.contains('.') || name.ends_with(".twui") || name.ends_with(".cml") || name.ends_with(".fc") || name.ends_with(".twui.images"));
            if ui_like {
                if let Ok(d) = vfs.read(path) {
                    pack_files.push((path.clone(), d));
                }
            }
        }
    }
    let mut corpus: Vec<(String, Vec<u8>)> = files.iter().filter_map(|f| std::fs::read(f).ok().map(|d| (f.display().to_string(), d))).collect();
    corpus.extend(pack_files);

    let mut per_version: BTreeMap<u32, (usize, usize)> = BTreeMap::new();
    let mut failures: Vec<(PathBuf, String)> = Vec::new();
    let mut seen = 0usize;
    for (f, data) in &corpus {
        let f = &PathBuf::from(f);
        if !wce_ui_format::looks_like_ui(data) {
            continue;
        }
        seen += 1;
        let ver: u32 = std::str::from_utf8(&data[7..10]).unwrap().parse().unwrap();
        let e = per_version.entry(ver).or_default();
        e.0 += 1;
        let res = wce_ui_format::ui_to_xml(data).and_then(|x| {
            if let Some(dir) = &xml_out {
                let rel = f.file_name().unwrap().to_string_lossy().to_string();
                let data = &data;
                let _ = data;
                let _ = std::fs::create_dir_all(dir);
                let _ = std::fs::write(dir.join(format!("{}_{}.xml", seen, rel)), &x.xml);
            }
            wce_ui_format::xml_to_ui(&x.xml)
        });
        match res {
            Ok(back) if &back == data => e.1 += 1,
            Ok(back) => {
                let first = back.iter().zip(data.iter()).position(|(a, b)| a != b).unwrap_or(back.len().min(data.len()));
                failures.push((f.clone(), format!("bytes differ at {} (sizes {} vs {})", first, back.len(), data.len())));
            }
            Err(e) => failures.push((f.clone(), e.to_string())),
        }
    }
    println!("{} UI files", seen);
    for (v, (n, ok)) in &per_version {
        println!("  version {:03}: {}/{} identical", v, ok, n);
    }
    for (f, why) in &failures {
        println!("FAIL {}: {}", f.display(), why);
    }
    let ok: usize = per_version.values().map(|v| v.1).sum();
    println!("total {}/{} identical", ok, seen);
    if !failures.is_empty() {
        std::process::exit(1);
    }
}
