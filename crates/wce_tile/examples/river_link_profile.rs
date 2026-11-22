//! river_link_profile: what a river looks like where it leaves a tile through a river link (docs/plans/rivers.md, R1 to R4). For every
//! tile with `river_spline<n>.xml` files under the given folders: each river end within half a cell of an edge, the cell it is at (both
//! ways of counting z, to settle the files' frame against the entry's cells), whether that cell is a river target, the water height, the
//! width, the tangent's angle against the edge's normal, how far past the edge and off the cell's centre it is, whether the end is the
//! river's first or last point and the `is_entry` of the rule beyond it; then the ground across the edge cell (editing heights and
//! `final_heights.dds`) at a few distances in from the edge. A summary at the end.
//!
//!   cargo run --release -p wce_tile --example river_link_profile -- <kit raw_data/terrain/tiles/battle> [...]

use std::path::{Path, PathBuf};

use wce_tile::tile_project::{FinalHeights, TileProject};

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(read) = std::fs::read_dir(dir) else { return };
    let mut has = false;
    for e in read.flatten() {
        let p = e.path();
        if p.is_dir() {
            walk(&p, out);
        } else if p.file_name().is_some_and(|n| n == "river_spline0.xml") {
            has = true;
        }
    }

    if has && dir.join("tile.ted").is_file() {
        out.push(dir.to_owned());
    }
}

/// A file's points: (x, y, z, width) of every `<POINTn .../>`, in order.
fn points(text: &str) -> Vec<[f64; 4]> {
    let attr = |tag: &str, name: &str| -> f64 {
        let key = format!(" {name}=\"");
        tag.find(&key).and_then(|i| tag[i + key.len()..].split('"').next()).and_then(|v| v.parse().ok()).unwrap_or(f64::NAN)
    };
    text.split("<POINT")
        .skip(1)
        .filter_map(|s| s.split("/>").next())
        .map(|tag| [attr(tag, "x"), attr(tag, "y"), attr(tag, "z"), attr(tag, "width")])
        .collect()
}

fn median(v: &mut [f64]) -> f64 {
    v.sort_by(f64::total_cmp);
    if v.is_empty() { f64::NAN } else { v[v.len() / 2] }
}

/// `--grid <tile>`: a tile's editing and final heights every 8 m (a linking tile such as `river/1x1_blend/aa`).
fn grid(dir: &Path) {
    let p = TileProject::open(dir).expect("tile");
    let (ex, ez) = (p.extent_x(), p.extent_z());
    let fin = std::fs::read(dir.join("final_heights.dds")).ok().and_then(|b| FinalHeights::parse(&b, ex, ez));
    println!("{}: {ex} x {ez} m; rows z, columns x 0..{ex} by 8", dir.display());
    let mut z = 0.0;
    while z <= ez {
        let mut ed = vec![];
        let mut fi = vec![];
        let mut x = 0.0;
        while x <= ex {
            ed.push(format!("{:.1}", p.height_at(x, z)));
            if let Some(f) = &fin {
                fi.push(format!("{:.1}", f.at(x, z)));
            }
            x += 8.0;
        }
        println!("z {z:>5}: editing {}\n         final   {}", ed.join(" "), fi.join(" "));
        z += 8.0;
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).is_some_and(|a| a == "--grid") {
        args[2..].iter().for_each(|a| grid(Path::new(a)));
        return;
    }

    let mut dirs = vec![];
    for a in std::env::args().skip(1) {
        walk(Path::new(&a), &mut dirs);
    }
    dirs.sort();

    let ins = [0.0, 4.0, 8.0, 16.0, 32.0, 64.0, 96.0, 128.0];
    let across: Vec<f64> = (-8..=8).map(|i| f64::from(i) * 8.0).collect();
    let (mut ys, mut widths, mut angles, mut pasts, mut offs) = (vec![], vec![], vec![], vec![], vec![]);
    let (mut on_target_flipped, mut on_target_plain, mut ends_n) = (0, 0, 0);
    let mut bed_rel: Vec<Vec<f64>> = vec![vec![]; across.len()];
    let mut fin_rel: Vec<Vec<f64>> = vec![vec![]; ins.len()];
    let mut ed_rel: Vec<Vec<f64>> = vec![vec![]; ins.len()];
    let mut flow = std::collections::BTreeMap::<String, usize>::new();

    for dir in &dirs {
        let p = match TileProject::open(dir) {
            Ok(p) => p,
            Err(e) => {
                println!("{}: {e}", dir.display());
                continue;
            }
        };
        let Some(entry) = p.entry.as_ref() else { continue };
        let (ex, ez) = (p.extent_x(), p.extent_z());
        let (cw, ch) = (p.info.cells_x as i32, p.info.cells_y as i32);
        let fin = std::fs::read(dir.join("final_heights.dds")).ok().and_then(|b| FinalHeights::parse(&b, ex, ez));
        let targets: Vec<_> = entry.link_targets().into_iter().filter(|t| t.tile_set.contains("river")).collect();
        let rules: Vec<_> = entry.links().into_iter().filter(|l| l.link_set.contains("river")).collect();
        println!(
            "{} ({cw}x{ch}, {}): targets {:?}; rules {:?}",
            dir.display(),
            entry.tile_set(),
            targets.iter().map(|t| (t.tile_set.as_str(), t.x, t.y)).collect::<Vec<_>>(),
            rules.iter().map(|l| (l.link_set.as_str(), l.x, l.y, l.is_entry, l.blend_size, l.test.as_str())).collect::<Vec<_>>()
        );

        for n in 0.. {
            let Ok(text) = std::fs::read_to_string(dir.join(format!("river_spline{n}.xml"))) else { break };
            let pts = points(&text);
            if pts.len() < 4 {
                continue;
            }

            for first in [true, false] {
                let (a, b) = if first { (pts[0], pts[1]) } else { (pts[pts.len() - 1], pts[pts.len() - 2]) };
                // The tangent going out of the river at this end.
                let (tx, tz) = (a[0] - b[0], a[2] - b[2]);
                let edges = [(a[0].abs(), 0), ((a[0] - ex).abs(), 1), ((a[2] - ez).abs(), 2), (a[2].abs(), 3)];
                let &(dist, edge) = edges.iter().min_by(|p, q| p.0.total_cmp(&q.0)).expect("four edges");
                if dist > 64.0 {
                    continue;
                }
                ends_n += 1;

                // The outward normal of the edge, how far past it, the cell along it.
                let (nx, nz) = [(-1.0, 0.0), (1.0, 0.0), (0.0, 1.0), (0.0, -1.0)][edge];
                let past = match edge {
                    0 => -a[0],
                    1 => a[0] - ex,
                    2 => a[2] - ez,
                    _ => -a[2],
                };
                let ang = (tz * nx - tx * nz).atan2(tx * nx + tz * nz).to_degrees();
                let col = ((a[0] / 128.0).floor() as i32).clamp(0, cw - 1);
                let zrow = ((a[2] / 128.0).floor() as i32).clamp(0, ch - 1);
                let (cell_plain, cell_flipped) = match edge {
                    0 | 1 => {
                        let x = if edge == 0 { 0 } else { cw - 1 };
                        ((x, zrow), (x, ch - 1 - zrow))
                    }
                    _ => {
                        let r = if edge == 2 { ch - 1 } else { 0 };
                        ((col, r), (col, ch - 1 - r))
                    }
                };
                let on = |c: (i32, i32)| targets.iter().any(|t| (t.x, t.y) == c);
                on_target_plain += usize::from(on(cell_plain));
                on_target_flipped += usize::from(on(cell_flipped));
                let centre = |i: i32| f64::from(i) * 128.0 + 64.0;
                let off = if edge < 2 { a[2] - centre(zrow) } else { a[0] - centre(col) };

                // The rule beyond the end's cell (entry numbering, z from +z), and its is_entry.
                let (cx, cy) = cell_flipped;
                let beyond = match edge {
                    0 => (cx - 1, cy),
                    1 => (cx + 1, cy),
                    2 => (cx, cy - 1),
                    _ => (cx, cy + 1),
                };
                let rule = rules.iter().find(|l| (l.x, l.y) == beyond);
                let key = format!(
                    "{} end, rule is_entry {}",
                    if first { "first" } else { "last" },
                    rule.map_or("none".to_owned(), |l| l.is_entry.to_string())
                );
                *flow.entry(key).or_default() += 1;

                println!(
                    "  river {n} {} end: edge {edge}, at ({:.1}, {:.2}, {:.1}) w {} past {past:.1} off {off:.1} angle {ang:.1}; cell plain {cell_plain:?} {} flipped {cell_flipped:?} {}; rule beyond {:?}",
                    if first { "first" } else { "last" },
                    a[0],
                    a[1],
                    a[2],
                    a[3],
                    if on(cell_plain) { "TARGET" } else { "-" },
                    if on(cell_flipped) { "TARGET" } else { "-" },
                    rule.map(|l| (l.link_set.as_str(), l.is_entry, l.blend_size, l.base_x, l.base_y))
                );
                ys.push(a[1]);
                widths.push(a[3]);
                angles.push(ang.abs());
                pasts.push(past);
                offs.push(off.abs());

                // The ground in from the edge along the river's line at the end, editing and final, relative to the water.
                let pos = |d_in: f64, d_across: f64| -> (f64, f64) {
                    let (ax, az) = (-nz, nx);
                    let (ex0, ez0) = match edge {
                        0 => (0.0, a[2]),
                        1 => (ex, a[2]),
                        2 => (a[0], ez),
                        _ => (a[0], 0.0),
                    };
                    ((ex0 - nx * d_in + ax * d_across).clamp(0.0, ex), (ez0 - nz * d_in + az * d_across).clamp(0.0, ez))
                };
                let mut ed_line = vec![];
                let mut fin_line = vec![];
                for (i, &d) in ins.iter().enumerate() {
                    let (x, z) = pos(d, 0.0);
                    let e = p.height_at(x, z) - a[1];
                    ed_line.push(format!("{d}:{e:.2}"));
                    ed_rel[i].push(e);
                    if let Some(f) = &fin {
                        let v = f.at(x, z) - a[1];
                        fin_line.push(format!("{d}:{v:.2}"));
                        fin_rel[i].push(v);
                    }
                }
                let mut cross = vec![];
                for (i, &s) in across.iter().enumerate() {
                    let (x, z) = pos(16.0, s);
                    let e = p.height_at(x, z) - a[1];
                    cross.push(format!("{e:.1}"));
                    bed_rel[i].push(e);
                }
                // Absolute cross-sections along the edge line itself and 16 m in, editing and final, -128..128 by 8 (clamped to the tile).
                for d in [0.0, 16.0, 24.0, 32.0, 48.0] {
                    let mut ed = vec![];
                    let mut fi = vec![];
                    for i in -16..=16 {
                        let (x, z) = pos(d, f64::from(i) * 8.0);
                        ed.push(format!("{:.1}", p.height_at(x, z)));
                        if let Some(f) = &fin {
                            fi.push(format!("{:.1}", f.at(x, z)));
                        }
                    }
                    println!("    ABS d={d} editing {}", ed.join(" "));
                    println!("    ABS d={d} final {}", fi.join(" "));
                }
                println!("    ground - water, in from the edge: editing {}", ed_line.join(" "));
                println!("    final {}", fin_line.join(" "));
                println!("    across at 16 m in (-64..64 by 8): {}", cross.join(" "));
            }
        }
    }

    println!(
        "\nSUMMARY: {} tiles, {ends_n} ends at an edge; on a target: plain z {on_target_plain}, flipped z {on_target_flipped}",
        dirs.len()
    );
    let q = |v: &mut Vec<f64>| {
        v.sort_by(f64::total_cmp);
        if v.is_empty() {
            return "-".to_owned();
        }
        let at = |f: f64| v[((v.len() - 1) as f64 * f) as usize];
        format!("min {:.2} p10 {:.2} median {:.2} p90 {:.2} max {:.2}", at(0.0), at(0.1), at(0.5), at(0.9), at(1.0))
    };
    println!("water y: {}", q(&mut ys));
    println!("width: {}", q(&mut widths));
    println!("|angle| to the normal: {}", q(&mut angles));
    println!("past the edge: {}", q(&mut pasts));
    println!("|off centre|: {}", q(&mut offs));
    for (i, d) in ins.iter().enumerate() {
        println!("in {d} m: editing - water {}; final - water {}", q(&mut ed_rel[i]), q(&mut fin_rel[i]));
    }
    let prof: Vec<String> = across.iter().zip(bed_rel.iter_mut()).map(|(s, v)| format!("{s}:{:.1}", median(v))).collect();
    println!("median cross-section at 16 m in: {}", prof.join(" "));
    for (k, v) in flow {
        println!("flow: {k}: {v}");
    }
}
