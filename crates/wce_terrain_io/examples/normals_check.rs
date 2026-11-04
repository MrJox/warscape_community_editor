//! The LF normal chunks of an LF edit (`wce_terrain_io::attila::normals`): a kit battle map's LF raised in a square, the chunks made:
//! the ones the square reaches only; outside the edited samples each pixel is the map's `normal.dds` (to DXT5's precision), on them the
//! LF's normal; and none for the map unedited. Exit 1 otherwise.
//!
//!   cargo run --release -p wce_terrain_io --example normals_check -- <kit map folder> [x y side]

use std::path::PathBuf;

use wce_formats::dds_decode::{decode_bc1_block, decode_bc3_alpha_block};
use wce_terrain::{LfRole, MapKind};
use wce_terrain_io::attila::normals::{MARGIN, NormalImage, SIDE, STEP, chunks, tiff_samples};
use wce_terrain_io::{MapFiles, attila};

/// The top level of a DXT5 DDS as RGBA.
fn dxt5(b: &[u8]) -> (usize, usize, Vec<[u8; 4]>) {
    let u = |o: usize| u32::from_le_bytes(b[o..o + 4].try_into().unwrap()) as usize;
    let (h, w) = (u(12), u(16));
    let mut px = vec![[0u8; 4]; w * h];
    let bw = w / 4;
    for by in 0..h / 4 {
        for bx in 0..bw {
            let blk = &b[128 + (by * bw + bx) * 16..][..16];
            let a = decode_bc3_alpha_block(&blk[..8]);
            let c = decode_bc1_block(&blk[8..], false);
            for i in 0..16 {
                px[(by * 4 + i / 4) * w + bx * 4 + i % 4] = [c[i][0], c[i][1], c[i][2], a[i]];
            }
        }
    }

    (w, h, px)
}

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let dir = PathBuf::from(&a[0]);
    let n = |i: usize, d: i64| a.get(i).and_then(|v| v.parse().ok()).unwrap_or(d);
    let (ex, ey, side) = (n(1, 3000), n(2, 2000), n(3, 40));
    let files = MapFiles::from_dir(&dir).expect("the map's folder");
    let mut m = attila::read("map", MapKind::Battle, &files).expect("the map");
    let cells = m.cells().expect("a tile map");
    let base = m.carried.iter().find(|(k, _)| k.eq_ignore_ascii_case("normal.dds")).and_then(|(_, b)| b.bytes().ok()).and_then(|b| NormalImage::from_dds(&b));
    let item = m.lf_mut(LfRole::Land).expect("an LF");
    let original = tiff_samples(&item.origin.bytes().expect("the LF")).expect("a 16-bit LF");
    let mut bad = 0;
    if !chunks(item.get(), &original, base.as_ref(), cells).is_empty() {
        println!("unedited: chunks made");
        bad += 1;
    }

    let f = item.edit();
    for y in ey..ey + side {
        for x in ex..ex + side {
            let v = f.at(x as u32, y as u32);
            f.set(x as u32, y as u32, v.saturating_add(2000));
        }
    }

    let made = chunks(item.get(), &original, base.as_ref(), cells);
    let (w, h) = (i64::from(item.get().width()), i64::from(item.get().height()));
    let cols = (w + STEP - 1) / STEP;
    let reaches = |k: i64| {
        let (ox, oy) = ((k % cols) * STEP - MARGIN, (k / cols) * STEP - MARGIN);
        ox < ex + side + 1 && ex - 1 < ox + SIDE && oy < ey + side + 1 && ey - 1 < oy + SIDE
    };
    let want: Vec<i64> = (0..cols * ((h + STEP - 1) / STEP)).filter(|&k| reaches(k)).collect();
    let got: Vec<i64> = made.iter().map(|(p, _)| p.trim_start_matches("normals/normal_").trim_end_matches(".dds").parse().unwrap()).collect();
    println!("edit {side} x {side} at ({ex}, {ey}): chunks {got:?} (the edit reaches {want:?}), {} bytes each", made.first().map_or(0, |c| c.1.len()));
    if got != want {
        bad += 1;
    }

    let Some(b) = base.as_ref() else {
        println!("no normal.dds to compare with");
        std::process::exit(i32::from(bad > 0));
    };

    for (path, bytes) in &made {
        let k: i64 = path.trim_start_matches("normals/normal_").trim_end_matches(".dds").parse().unwrap();
        let (ox, oy) = ((k % cols) * STEP - MARGIN, (k / cols) * STEP - MARGIN);
        let (cw, _, px) = dxt5(bytes);
        let (mut worst_out, mut inside, mut changed) = (0i32, 0, 0);
        for sy in 0..SIDE {
            for sx in 0..SIDE {
                let (x, y) = (ox + sx, oy + sy);
                if x < 0 || y < 0 || x >= w || y >= h || x >= i64::from(b.w) || y >= i64::from(b.h) {
                    continue;
                }

                let p = px[(sy as usize) * cw + sx as usize];
                let q = b.px[(y * i64::from(b.w) + x) as usize];
                let edited = x >= ex - 1 && x <= ex + side && y >= ey - 1 && y <= ey + side;
                let d = (i32::from(p[1]) - i32::from(q[1])).abs().max((i32::from(p[3]) - i32::from(q[0])).abs());
                if edited {
                    inside += 1;
                    if d > 8 {
                        changed += 1;
                    }
                } else {
                    worst_out = worst_out.max(d);
                }
            }
        }

        println!("  {path}: outside the edit within {worst_out} of normal.dds; on it {changed} of {inside} pixels differ from it");
        if worst_out > 40 {
            bad += 1;
        }
    }

    std::process::exit(i32::from(bad > 0));
}
