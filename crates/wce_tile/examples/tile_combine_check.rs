//! Combines the variations of every kit tile that has several (docs/plans/tile_variations.md, "Gates") and checks that each comes back
//! byte for byte: as tile.wce from the combined file (`variations::to_variation`), and as the kit's tile.ted through `to_ted`. Prints what
//! each combination shares and the space it saves.
//!
//!     cargo run --release -p wce_tile --example tile_combine_check -- <kit raw_data/terrain/tiles/battle> [word in the location] [max tiles]
//!
//! Exits with 1 when any variation does not come back.

use std::path::{Path, PathBuf};
use std::time::Instant;

use wce_tile::project_files::{to_ted, to_wce};
use wce_tile::variations::merge::{Source, check, merge};
use wce_tile::variations::to_variation;

/// An attribute's value in a line of the entry (`name='value'`).
fn attr(line: &str, name: &str) -> Option<String> {
    let at = line.find(&format!(" {name}="))? + name.len() + 2;
    let q = line[at..].chars().next()?;
    let rest = &line[at + 1..];
    Some(rest[..rest.find(q)?].to_owned())
}

/// The entries of a tile database: (file, location, variations).
fn entries(battle: &Path) -> Vec<(PathBuf, String, Vec<String>)> {
    let mut out = vec![];
    let dir = battle.join("_tile_database").join("TILES");
    let mut files = std::fs::read_dir(&dir).into_iter().flatten().flatten().map(|e| e.path()).collect::<Vec<_>>();
    files.sort();
    for f in files {
        let Ok(text) = std::fs::read(&f) else { continue };
        let text = String::from_utf8_lossy(&text);
        let Some(location) = text.lines().find(|l| l.contains("<TILE ")).and_then(|l| attr(l, "location")) else { continue };
        let variations = text.lines().filter(|l| l.contains("<VARIATION ")).filter_map(|l| attr(l, "location")).collect::<Vec<_>>();
        out.push((f, location, variations));
    }

    out
}

fn main() {
    let args = std::env::args().collect::<Vec<_>>();
    let battle = PathBuf::from(args.get(1).expect("usage: tile_combine_check <kit tiles/battle> [word] [max]"));
    let word = args.get(2).cloned().unwrap_or_default();
    let max = args.get(3).and_then(|m| m.parse().ok()).unwrap_or(usize::MAX);
    let raw = battle.ancestors().nth(3).expect("<raw_data>/terrain/tiles/battle").to_path_buf();
    let (mut tiles, mut failed, mut skipped, mut ted_alone) = (0, 0, 0, 0);
    let (mut bytes_in, mut bytes_out, mut inst_in, mut inst_out) = (0usize, 0usize, 0usize, 0usize);
    for (file, location, variations) in entries(&battle) {
        if variations.len() < 2 || !location.contains(&word) || tiles >= max {
            continue;
        }

        let dir = location.split(['\\', '/']).filter(|s| !s.is_empty()).fold(raw.clone(), |p, s| p.join(s));
        let mut teds = vec![];
        for v in &variations {
            if let Ok(t) = std::fs::read(dir.join(v).join("tile.ted")) {
                teds.push((v.clone(), t));
            }
        }

        if teds.len() < 2 {
            skipped += 1;
            continue;
        }

        tiles += 1;
        let t0 = Instant::now();
        let sources =
            teds.iter().map(|(v, t)| to_wce(t.clone()).map(|text| Source { variation: v.clone(), text })).collect::<Result<Vec<_>, _>>();
        let sources = match sources {
            Ok(s) => s,
            Err(e) => {
                println!("{location}: does not read: {e}");
                failed += 1;
                continue;
            }
        };

        let combined = match merge(&sources) {
            Ok(c) => c,
            Err(e) => {
                println!("{location}: not combined: {e}");
                failed += 1;
                continue;
            }
        };

        let mut bad = check(&combined.text, &sources).err();
        if bad.is_none() {
            for ((v, ted), source) in teds.iter().zip(&sources) {
                // What TEd gets from the variation's own tile.wce, combined or not: the kit's file, unless `to_ted` alone changes it
                // (noted, not a failure of the combination).
                let alone = to_ted(source.text.clone()).map(|(t, _)| t);
                if alone.as_ref() != Ok(ted) {
                    let at = alone.as_ref().map_or(0, |a| a.iter().zip(ted).position(|(x, y)| x != y).unwrap_or(a.len().min(ted.len())));
                    println!(
                        "  note: {location} {v}: to_ted(to_wce(tile.ted)) is not the kit's file by itself (first difference at byte {at})"
                    );
                    ted_alone += 1;
                }

                let back = to_variation(&combined.text, v).and_then(to_ted).map(|(t, _)| t);
                if back != alone {
                    bad = Some(format!("{v}: the tile.ted made from it is not what its own tile.wce gives"));
                    break;
                }
            }
        }

        // `WCE_COMBINE_OUT=<folder>`: the combined files written there, to look at.
        if let Ok(out) = std::env::var("WCE_COMBINE_OUT") {
            let name = location.replace(['\\', '/'], "_");
            let _ = std::fs::create_dir_all(&out);
            let _ = std::fs::write(Path::new(&out).join(format!("{name}.wce")), &combined.text);
        }

        let s = &combined.stats;
        bytes_in += s.bytes_in;
        bytes_out += s.bytes_out;
        inst_in += s.instances.iter().sum::<usize>();
        inst_out += s.combined;
        let names = teds.iter().map(|(v, _)| v.as_str()).collect::<Vec<_>>().join(",");
        let file = file.file_name().unwrap_or_default().to_string_lossy();
        match bad {
            None => {
                let vs = teds.iter().map(|(v, _)| v.clone()).collect::<Vec<_>>();
                println!(
                    "{location} [{names}] ok, {:.1}s: {}; shared {}; reference {} ({file})",
                    t0.elapsed().as_secs_f32(),
                    s.summary(),
                    s.pairs(&vs),
                    vs[s.reference()]
                );
            }
            Some(e) => {
                failed += 1;
                println!("{location} [{names}] DIFFERS: {e}");
            }
        }
    }

    println!(
        "{tiles} tiles combined, {failed} failed, {skipped} with fewer than two tile.ted, {ted_alone} tile.ted that to_ted does not give back by itself; {inst_in} instances in {inst_out} ({:.0}%), {:.0} MB in {:.0} MB ({:.0}%)",
        100.0 * inst_out as f64 / inst_in.max(1) as f64,
        bytes_in as f64 / 1e6,
        bytes_out as f64 / 1e6,
        100.0 * bytes_out as f64 / bytes_in.max(1) as f64
    );
    if failed != 0 {
        std::process::exit(1);
    }
}
