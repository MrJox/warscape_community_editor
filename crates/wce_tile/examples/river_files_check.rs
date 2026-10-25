//! river_files_check: TEd's `river_spline<n>.xml` written again from every tile.ted of a kit (`wce_tile::river_files`), compared with the
//! files TEd wrote next to it: the same text, or the same text but for numbers in their last digit (TEd writes from the floats it holds, which
//! tile.ted keeps to six digits). Prints the largest difference of a number.
//!
//!   cargo run --release -p wce_tile --example river_files_check -- <kit raw_data/terrain/tiles>

use std::path::Path;

use wce_tile::river_files::river_files;
use wce_tile::ted_scene::TedScene;

fn walk(dir: &Path, out: &mut Vec<std::path::PathBuf>) {
    let Ok(read) = std::fs::read_dir(dir) else { return };
    for e in read.flatten() {
        let p = e.path();
        if p.is_dir() {
            walk(&p, out);
        } else if p.file_name().is_some_and(|n| n == "tile.ted") {
            out.push(p);
        }
    }
}

/// The same text apart from numbers that differ in their last digits (relative 1e-5, or 1e-4 absolute).
fn same_but_last_digits(a: &[u8], b: &[u8], max_diff: &mut f64) -> bool {
    let split = |t: &[u8]| String::from_utf8_lossy(t).split('"').map(str::to_owned).collect::<Vec<_>>();
    let (a, b) = (split(a), split(b));
    if a.len() != b.len() {
        return false;
    }

    for (x, y) in a.iter().zip(&b) {
        if x == y {
            continue;
        }

        let (Ok(p), Ok(q)) = (x.parse::<f64>(), y.parse::<f64>()) else { return false };
        let d = (p - q).abs();
        if d > 1e-4_f64.max(p.abs() * 1e-5) {
            return false;
        }

        *max_diff = max_diff.max(d);
    }

    true
}

fn main() {
    let root = std::env::args().nth(1).expect("river_files_check <kit raw_data/terrain/tiles>");
    let mut teds = vec![];
    walk(Path::new(&root), &mut teds);
    teds.sort();
    let (mut same, mut close, mut differ, mut missing, mut tiles) = (0, 0, 0, 0, 0);
    let mut max_diff = 0.0f64;

    // The rivers of today's TEd (a `flatten` attribute): their files follow tile.ted; older ones were often not written again.
    let (mut today_ok, mut today_bad) = (0, 0);
    for ted in teds {
        let dir = ted.parent().expect("a folder");
        if !dir.join("river_spline0.xml").exists() {
            continue;
        }

        let Ok(scene) = TedScene::parse(std::fs::read(&ted).expect("tile.ted")) else { continue };
        tiles += 1;
        let modern = wce_tile::river_files::rivers(&scene).iter().map(|&r| scene.doc().get(r, "flatten").is_some()).collect::<Vec<_>>();
        for (n, (name, text)) in river_files(&scene, Some(dir), dir.to_string_lossy().replace('\\', "/").contains("/tiles/campaign/")).into_iter().enumerate() {
            let today = modern.get(n).copied().unwrap_or(false);
            match std::fs::read(dir.join(&name)) {
                Ok(ted_text) if ted_text == text => {
                    same += 1;
                    if today {
                        today_ok += 1;
                    }
                }
                Ok(ted_text) if same_but_last_digits(&ted_text, &text, &mut max_diff) => {
                    close += 1;
                    if today {
                        today_ok += 1;
                    }
                }
                Ok(ted_text) => {
                    differ += 1;
                    if today {
                        today_bad += 1;
                    }

                    if today && today_bad <= 3 {
                        let a = String::from_utf8_lossy(&ted_text);
                        let b = String::from_utf8_lossy(&text);
                        let line = a.lines().zip(b.lines()).find(|(x, y)| x != y);
                        println!("differs: {} {name}\n  TEd:  {:?}\n  here: {:?}", dir.display(), line.map(|l| l.0), line.map(|l| l.1));
                    }
                }

                Err(_) => missing += 1,
            }
        }
    }

    println!("rivers of today's TEd (with flatten): {today_ok} as TEd wrote them, {today_bad} different");
    println!("{tiles} tiles with river files: {same} the same, {close} the same but for last digits (largest difference {max_diff:.6}), {differ} different, {missing} not written by TEd");
}
