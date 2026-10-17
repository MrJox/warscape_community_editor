//! The C++ `scene_check` corpus gate over every tile.ted below a folder:
//!   1. parse + save is byte-identical;
//!   2. for every instance with a position: set_position(read value) is a no-op (bytes unchanged);
//!   3. translate +1 on x/z of a sample of instances, and revert_all restores the original;
//!   4. format_ted_number(parse(v)) == v for position / orientation / scale values (counted, with the failures listed).
//!
//!   cargo run --release -p wce_tile --example scene_check -- <folder> [max_mb]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use wce_foundation::math::Double3;
use wce_tile::ted_scene::{TedScene, format_ted_number, parse_vec3};

fn teds(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            teds(&p, out);
        } else if p.file_name().is_some_and(|n| n == "tile.ted") {
            out.push(p);
        }
    }
}

fn main() {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    let max_mb = args.get(1).and_then(|a| a.parse::<f64>().ok()).unwrap_or(100.0);
    let mut files = vec![];
    teds(Path::new(&args[0]), &mut files);
    files.sort();
    let (mut parsed, mut parse_fail, mut save_diff, mut noop_diff, mut revert_diff, mut instances, mut fmt_ok, mut fmt_total) = (0, 0, 0, 0, 0, 0, 0, 0);
    let mut types = BTreeMap::<String, usize>::new();
    let mut notes = vec![];
    for path in files {
        let Ok(bytes) = std::fs::read(&path) else { continue };
        if bytes.len() as f64 > max_mb * 1048576.0 {
            continue;
        }

        let mut sc = match TedScene::parse(bytes.clone()) {
            Ok(sc) => sc,
            Err(e) => {
                parse_fail += 1;
                if notes.len() < 5 {
                    notes.push(format!("{}: {e}", path.display()));
                }

                continue;
            }
        };

        parsed += 1;
        if sc.save() != bytes {
            save_diff += 1;
            continue;
        }

        let mut n = 0;
        for e in 0..sc.doc().elements().len() {
            let Some(pos) = sc.position(e) else { continue };
            instances += 1;
            *types.entry(sc.type_of(e).to_owned()).or_default() += 1;
            sc.set_position(e, pos);
            for a in ["position", "orientation", "scale"] {
                let Some(txt) = sc.doc().get(e, a) else { continue };
                let pre = if a == "orientation" { "euler" } else { "vector" };
                let Some(v) = parse_vec3(&txt, pre) else { continue };
                fmt_total += 1;
                let back = format!("{pre}({},{},{})", format_ted_number(v.x), format_ted_number(v.y), format_ted_number(v.z));
                if back == txt {
                    fmt_ok += 1;
                } else if notes.len() < 8 {
                    notes.push(format!("format: {txt} -> {back}"));
                }
            }

            n += 1;
            if n % 997 == 0 {
                let _ = sc.translate(e, Double3::new(1.0, 0.0, 1.0));
            }
        }

        if n != 0 && !sc.modified() && sc.save() != bytes {
            noop_diff += 1;
        }

        sc.revert_all();
        if sc.save() != bytes {
            revert_diff += 1;
        }
    }

    println!("tile.ted files {parsed} (parse failures {parse_fail}, parse/save differs {save_diff})");
    println!("instances with a position: {instances}");
    println!("no-op set_position changed bytes: {noop_diff} files; revert_all did not restore: {revert_diff} files");
    println!("number formatting reproduced: {fmt_ok} of {fmt_total} ({:.3}%)", if fmt_total != 0 { 100.0 * fmt_ok as f64 / fmt_total as f64 } else { 0.0 });
    for (t, k) in &types {
        println!("  {k:9} {t}");
    }

    for n in &notes {
        println!("note: {n}");
    }
}
