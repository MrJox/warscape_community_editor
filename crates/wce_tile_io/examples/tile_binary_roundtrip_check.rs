//! Corpus gate of the exported (binary) tile format (`attila.tile_binary`, docs/plans/tile_roundtrip.md Phase 2 step 2): every tile folder
//! of the packs, read from the packs, through the model and back.
//!
//!   cargo run --release -p wce_tile_io --example tile_binary_roundtrip_check -- <pack or folder of packs>... [--filter TEXT] [--threads N]
//!       [--dir FOLDER]
//!
//! A tile folder is a folder under `terrain/tiles/` that holds files. Per folder: read, write; every part the format reads is
//! byte-identical (or written from its original bytes, counted as kept); the written files read back into the same model. `--dir` reads
//! extracted folders instead of packs. Prints counts per kind of file and the first problems; exits 1 on a difference.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Instant;

use rayon::prelude::*;
use rpfm_lib::files::pack::Pack;
use rpfm_lib::games::supported_games::SupportedGames;
use wce_tile_io::attila::TileBinary;
use wce_tile_io::{PartRead, PartWritten, TileFiles, TileFormat};
use wce_tile_model::Blob;

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
    kept: usize,
    differ: usize,
    failed: usize,
    opaque: usize,
}

#[derive(Default)]
struct Totals {
    tiles: usize,
    per_kind: BTreeMap<String, Counts>,
    model_mismatch: usize,
    write_failed: usize,
    problems: Vec<String>,
}

fn check(name: &str, files: &TileFiles, t: &Mutex<Totals>) {
    let mut local = Totals::default();
    let (tile, report) = match TileBinary.read(files) {
        Ok(r) => r,
        Err(e) => {
            t.lock().expect("totals").problems.push(format!("{name}: {e}"));
            return;
        }
    };

    for (part, how) in &report.parts {
        let c = local.per_kind.entry(kind_of(part)).or_default();
        c.files += 1;
        if let PartRead::Failed(e) = how {
            c.failed += 1;
            local.problems.push(format!("{name}/{part}: did not read ({e})"));
        }
    }

    for f in &tile.files {
        if !report.parts.iter().any(|(p, _)| p == &f.path) {
            local.per_kind.entry(kind_of(&f.path)).or_default().opaque += 1;
        }
    }

    match TileBinary.write(&tile) {
        Ok((written, wreport)) => {
            for (part, how) in &wreport.parts {
                if *how == PartWritten::Copied {
                    continue;
                }

                let c = local.per_kind.entry(kind_of(part)).or_default();
                let (a, b) = (files.folder.get(part).map(|b| b.bytes().unwrap_or_default()), written.folder.get(part).map(|b| b.bytes().unwrap_or_default()));
                if a == b {
                    if *how == PartWritten::Kept {
                        c.kept += 1;
                    } else {
                        c.identical += 1;
                    }
                } else {
                    c.differ += 1;
                    let (a, b) = (a.unwrap_or_default(), b.unwrap_or_default());
                    let i = a.iter().zip(b.iter()).take_while(|(x, y)| x == y).count();
                    local.problems.push(format!("{name}/{part}: differs at byte {i} (sizes {} / {})", a.len(), b.len()));
                }
            }

            match TileBinary.read(&written) {
                Ok((back, _)) if back.semantic() == tile.semantic() => {}
                Ok((back, _)) => {
                    local.model_mismatch += 1;
                    let mut what = vec![];
                    for d in &tile.documents {
                        if back.document(&d.name).is_none_or(|b| b.root.without_trivia() != d.root.without_trivia()) {
                            what.push(d.name.clone());
                        }
                    }

                    for m in &tile.maps {
                        if back.map(m.kind, m.index) != Some(m) {
                            what.push(format!("{:?} {}", m.kind, m.index));
                        }
                    }

                    if back.files.iter().map(|f| &f.path).ne(tile.files.iter().map(|f| &f.path)) {
                        what.push("the opaque files".to_owned());
                    }

                    local.problems.push(format!("{name}: the written files read into another model ({})", what.join(", ")));
                }

                Err(e) => {
                    local.model_mismatch += 1;
                    local.problems.push(format!("{name}: the written files do not read: {e}"));
                }
            }
        }

        Err(e) => {
            local.write_failed += 1;
            local.problems.push(format!("{name}: write: {e}"));
        }
    }

    let mut t = t.lock().expect("totals");
    t.tiles += 1;
    for (k, c) in local.per_kind {
        let to = t.per_kind.entry(k).or_default();
        to.files += c.files;
        to.identical += c.identical;
        to.kept += c.kept;
        to.differ += c.differ;
        to.failed += c.failed;
        to.opaque += c.opaque;
    }

    t.model_mismatch += local.model_mismatch;
    t.write_failed += local.write_failed;
    t.problems.extend(local.problems);
}

fn packs(arg: &Path, out: &mut Vec<PathBuf>) {
    if arg.is_dir() {
        let mut found = std::fs::read_dir(arg).map(|d| d.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "pack")).collect::<Vec<_>>()).unwrap_or_default();
        found.sort();
        out.extend(found);
    } else {
        out.push(arg.to_path_buf());
    }
}

fn dirs(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    let mut has_files = false;
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            dirs(&p, out);
        } else {
            has_files = true;
        }
    }

    if has_files && dir.to_string_lossy().contains("terrain/tiles/") {
        out.push(dir.to_path_buf());
    }
}

fn main() {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    let opt = |name: &str| args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned();
    let filter = opt("--filter");
    let threads = opt("--threads").and_then(|n| n.parse().ok()).unwrap_or(8);
    let pool = rayon::ThreadPoolBuilder::new().num_threads(threads).build().expect("pool");
    let totals = Mutex::new(Totals::default());
    let started = Instant::now();
    if let Some(dir) = opt("--dir") {
        let mut all = vec![];
        dirs(Path::new(&dir), &mut all);
        if let Some(f) = &filter {
            all.retain(|d| d.to_string_lossy().contains(f.as_str()));
        }

        pool.install(|| all.par_iter().for_each(|d| check(&d.display().to_string(), &TileFiles::from_dir(d).expect("list"), &totals)));
    } else {
        let mut inputs = vec![];
        let mut skip = false;
        for a in &args {
            if skip {
                skip = false;
            } else if a.starts_with("--") {
                skip = true;
            } else {
                packs(Path::new(a), &mut inputs);
            }
        }

        let games = SupportedGames::default();
        let game = games.game("attila").expect("attila");
        for pack_path in &inputs {
            let pack_name = pack_path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            let Ok(mut pack) = Pack::read_and_merge(std::slice::from_ref(pack_path), game, true, false, false) else {
                println!("{pack_name}: cannot read");
                continue;
            };

            let mut folders: BTreeMap<String, Vec<String>> = BTreeMap::new();
            for n in pack.files().keys().filter(|n| n.starts_with("terrain/tiles/")) {
                let (dir, _) = n.rsplit_once('/').unwrap_or(("", n));
                folders.entry(dir.to_owned()).or_default().push(n.clone());
            }

            if let Some(f) = &filter {
                folders.retain(|d, _| d.contains(f.as_str()));
            }

            // Read a batch of folders from the pack (one thread), check them side by side.
            let folders = folders.into_iter().collect::<Vec<_>>();
            for batch in folders.chunks(threads * 4) {
                let loaded = batch
                    .iter()
                    .map(|(dir, names)| {
                        let mut files = TileFiles::default();
                        for n in names {
                            let Some(rf) = pack.files_mut().get_mut(n) else { continue };
                            let Ok(Some(bytes)) = rf.encode(&None, false, false, true) else { continue };
                            files.folder.insert(n[dir.len() + 1..].to_owned(), Blob::Bytes(bytes.into()));
                        }

                        (format!("{pack_name}:{dir}"), files)
                    })
                    .collect::<Vec<_>>();

                pool.install(|| loaded.par_iter().for_each(|(name, files)| check(name, files, &totals)));
            }
        }
    }

    let t = totals.into_inner().expect("totals");
    println!("tile folders: {} ({:.0} s)", t.tiles, started.elapsed().as_secs_f64());
    println!("{:<44} {:>7} {:>9} {:>6} {:>6} {:>8} {:>7}", "kind", "files", "identical", "kept", "differ", "not read", "opaque");
    let mut bad = t.model_mismatch + t.write_failed;
    for (k, c) in &t.per_kind {
        bad += c.differ;
        println!("{k:<44} {:>7} {:>9} {:>6} {:>6} {:>8} {:>7}", c.files, c.identical, c.kept, c.differ, c.failed, c.opaque);
    }

    println!("models read back differently: {}; writes that failed: {}", t.model_mismatch, t.write_failed);
    if !t.problems.is_empty() {
        let mut p = t.problems;
        p.sort();
        println!("problems ({}; first 40):", p.len());
        for x in p.iter().take(40) {
            println!("  {x}");
        }
    }

    println!("{}", if bad == 0 { "all passed" } else { "FAILED" });
    std::process::exit(i32::from(bad != 0));
}
