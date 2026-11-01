//! The river splines fitted to the terrain meshes' river blocks (`attila::river::fit_splines`): per mesh the rivers, rows and segments,
//! how far the block's rows are from the fitted curves (the centre line, and the width, flow, alpha and foam of the nearest place), and
//! that tessellating them again gives as many rivers. The rows made again are not compared with the block's: BOB keeps a row only where
//! the river turns by about 11 degrees, so its chords sag from the curve (by up to 0.5 % of the radius) in other places for another
//! segmentation. Fails when one is off by more than the tolerances.
//!
//!   cargo run --release -p wce_tile_io --example river_fit_check -- <game key> <data folder> [text in the path]

use std::path::PathBuf;

use rpfm_lib::files::pack::Pack;
use rpfm_lib::games::supported_games::SupportedGames;
use wce_tile_io::attila::river::{RiverRow, RiverSegment, fit_splines, read_river_block, river_rows, tessellate};

/// Fitted segments sampled densely, as rows (the alpha lerped, the other values the segment's).
fn dense(segments: &[RiverSegment]) -> Vec<RiverRow> {
    let mut out = vec![];
    for (k, s) in segments.iter().enumerate() {
        for i in 0..=256 {
            let t = i as f32 / 256.0;
            let u = 1.0 - t;
            let w = [u * u * u, 3.0 * t * u * u, 3.0 * t * t * u, t * t * t];
            let centre = [0, 1, 2].map(|c| (0..4).map(|j| w[j] * s.points[j][c]).sum());
            let alpha = s.alpha[0] + (s.alpha[1] - s.alpha[0]) * t;
            out.push(RiverRow { centre, along: k as f32 + t, width: s.width[0], flow: s.flow[0], alpha, foam: s.foam[0] });
        }
    }

    out
}

/// The distance from a point to the polyline of dense rows' centres (x, z), and the rows nearest to it (a junction's both sides).
fn nearest(p: [f32; 3], rows: &[RiverRow]) -> (f32, Vec<&RiverRow>) {
    let mut line = f32::MAX;
    for w in rows.windows(2) {
        let (a, b) = (w[0].centre, w[1].centre);
        let (dx, dz) = (b[0] - a[0], b[2] - a[2]);
        let len2 = dx * dx + dz * dz;
        let t = if len2 > 0.0 { (((p[0] - a[0]) * dx + (p[2] - a[2]) * dz) / len2).clamp(0.0, 1.0) } else { 0.0 };
        line = line.min((p[0] - a[0] - dx * t).hypot(p[2] - a[2] - dz * t));
    }

    let d = |r: &RiverRow| (p[0] - r.centre[0]).hypot(p[2] - r.centre[2]);
    let best = rows.iter().map(d).fold(f32::MAX, f32::min);
    (line, rows.iter().filter(|r| d(r) <= best + 0.01).collect())
}

fn main() {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    let games = SupportedGames::default();
    let game = games.game(&args[0]).expect("game");
    let dir = PathBuf::from(&args[1]);
    let filter = args.get(2).cloned().unwrap_or_default();
    let mut packs = std::fs::read_dir(&dir).expect("data").flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "pack")).collect::<Vec<_>>();
    packs.sort();
    let (mut meshes, mut bad) = (0, 0);
    let mut worst = [0.0f32; 4];
    for p in packs {
        let Ok(mut pack) = Pack::read_and_merge(std::slice::from_ref(&p), game, true, false, false) else { continue };
        let names = pack.files().keys().filter(|n| n.starts_with("terrain/tiles/") && n.ends_with("/mesh.rigid_model_v2") && n.contains(&filter)).cloned().collect::<Vec<_>>();
        for n in names {
            let Some(Ok(Some(b))) = pack.files_mut().get_mut(&n).map(|f| f.encode(&None, false, false, true)) else { continue };
            let Some(block) = read_river_block(&b) else { continue };
            meshes += 1;
            let rivers = river_rows(&block);
            let fitted = rivers.iter().map(|r| fit_splines(r)).collect::<Vec<_>>();
            let again = river_rows(&tessellate(&fitted, block.format));
            let segments = fitted.iter().map(Vec::len).sum::<usize>();
            if std::env::var("VERBOSE").is_ok() {
                for (k, (old, new)) in rivers.iter().zip(&again).enumerate() {
                    println!("river {k}");
                    for r in old {
                        println!("  old {:.5} c {:?} w {} flow {} alpha {} foam {}", r.along, r.centre, r.width, r.flow, r.alpha, r.foam);
                    }
                    for s in &fitted[k] {
                        println!("  seg {:?} w {:?} a {:?}", s.points, s.width, s.alpha);
                    }
                    for r in new {
                        println!("  new {:.5} c {:?} w {} alpha {}", r.along, r.centre, r.width, r.alpha);
                    }
                }
            }
            let rows = rivers.iter().map(Vec::len).sum::<usize>();
            let mut off = [0.0f32; 4];
            if again.len() != rivers.len() {
                off = [f32::INFINITY; 4];
            } else {
                for (old, fit) in rivers.iter().zip(&fitted) {
                    let new = &dense(fit);
                    for r in old {
                        let (d, near) = nearest(r.centre, new);
                        let least = |f: fn(&RiverRow) -> f32| near.iter().map(|q| (f(q) - f(r)).abs()).fold(f32::MAX, f32::min);
                        off[0] = off[0].max(d);
                        off[1] = off[1].max(least(|q| q.width));
                        off[2] = off[2].max(least(|q| q.alpha));
                        off[3] = off[3].max(least(|q| q.flow).max(least(|q| q.foam)));
                    }
                }
            }

            for k in 0..4 {
                worst[k] = worst[k].max(off[k]);
            }

            let fails = off[0] > 0.1 || off[1] > 0.5 || off[2] > 0.05 || off[3] > 1e-3;
            bad += usize::from(fails);
            println!(
                "{}{n}: {} rivers, {rows} rows, {segments} segments; off {:.3} m, width {:.3}, alpha {:.3}, flow / foam {:.4}",
                if fails { "FAIL " } else { "" },
                rivers.len(),
                off[0],
                off[1],
                off[2],
                off[3]
            );
        }
    }

    println!("{meshes} meshes with a river block, {bad} off; worst: {:.3} m, width {:.3}, alpha {:.3}, flow / foam {:.4}", worst[0], worst[1], worst[2], worst[3]);
    if bad > 0 {
        std::process::exit(1);
    }
}
