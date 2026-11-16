//! A tile's `hf_height_map.data` against its terrain mesh (`mesh.rigid_model_v2`, LOD 0, block 0) rasterised on the height map's grid:
//! where the height map has its samples (non-zero ones: a missing block reads as 0), how far the mesh is from them, with the mesh's z as
//! the height map's rows and turned the other way.
//!
//!   cargo run --release -p wce_tile_io --example hf_mesh_check -- <tile folder with both files>

use std::io::Cursor;

use rpfm_lib::files::rigidmodel::RigidModel;
use rpfm_lib::files::{Decodeable, DecodeableExtraData};
use wce_formats::chmf::ChmfFile;
use wce_tile_io::attila::terrain_heights::mesh_heights;

fn main() {
    let dir = std::path::PathBuf::from(std::env::args().nth(1).expect("usage: hf_mesh_check <tile folder>"));
    let hf = ChmfFile::parse(&std::fs::read(dir.join("hf_height_map.data")).expect("hf")).expect("chmf");
    let (w, h, samples) = hf.region_samples(0).expect("region");
    let mesh = std::fs::read(dir.join("mesh.rigid_model_v2")).expect("mesh");
    let model = RigidModel::decode(&mut Cursor::new(&mesh), &Some(DecodeableExtraData::default())).expect("rigid model");
    let present = samples.iter().filter(|&&s| s != 0).count();
    println!("{w} x {h} samples, spacing {}, {present} non-zero; min {} max {}", hf.spacing, hf.min, hf.max);
    // `WCE_KIT_TILE`: the kit's folder of the same tile; its `final_heights.dds` (the tile's frame, TEd's z) against the mesh at z and at
    // the extent - z.
    if let Ok(kit) = std::env::var("WCE_KIT_TILE") {
        let (ex, ez) = (f64::from(w) * f64::from(hf.spacing), f64::from(h) * f64::from(hf.spacing));
        let fh = wce_tile::tile_project::FinalHeights::parse(
            &std::fs::read(std::path::Path::new(&kit).join("final_heights.dds")).expect("final"),
            ex,
            ez,
        )
        .expect("an R32F final_heights.dds");
        {
            let grid = mesh_heights(&model, w, h, f64::from(hf.spacing), false).expect("mesh");
            let (mut lo, mut hi) = (f64::MAX, f64::MIN);
            for k in 0..64 {
                for i in 0..64 {
                    let v = fh.at(ex * f64::from(i) / 63.0, ez * f64::from(k) / 63.0);
                    (lo, hi) = (lo.min(v), hi.max(v));
                }
            }
            println!("kit final heights {lo:.2} .. {hi:.2}");
            for (x, z) in [(100.0, 100.0), (299.0, 147.0), (299.0, 237.0), (400.0, 300.0), (50.0, 300.0)] {
                let m = grid[(z as usize) * w as usize + x as usize];
                println!("  at {x} {z}: mesh {m:?}, final {:.2}, final mirrored {:.2}", fh.at(x, z), fh.at(x, ez - z));
            }
        }

        for flip in [false, true] {
            let grid = mesh_heights(&model, w, h, f64::from(hf.spacing), false).expect("mesh");
            let (mut n, mut sum, mut worst) = (0usize, 0.0f64, 0.0f64);
            for k in 0..h as usize {
                for i in 0..w as usize {
                    let Some(m) = grid[k * w as usize + i] else { continue };
                    let (x, z) = (i as f64 * f64::from(hf.spacing), k as f64 * f64::from(hf.spacing));
                    let d = (fh.at(x, if flip { ez - z } else { z }) - f64::from(m)).abs();
                    n += 1;
                    sum += d;
                    worst = worst.max(d);
                }
            }

            println!(
                "kit final heights, mesh z {}: {n} samples, mean {:.3}, worst {:.3}",
                if flip { "mirrored" } else { "as is" },
                sum / n.max(1) as f64,
                worst
            );
        }
    }

    // `WCE_MASK`: the entry's mask (rows top-down in the map, as `cells_x` x `cells_y` characters): the mesh's cells against it, the mask's
    // row v at mesh z = (rows - v - 0.5) * 128 (TEd's z) or at (v + 0.5) * 128.
    if let Ok(mask) = std::env::var("WCE_MASK") {
        let grid = mesh_heights(&model, w, h, f64::from(hf.spacing), false).expect("mesh");
        let (cx, cz) = ((f64::from(w) * f64::from(hf.spacing) / 128.0) as usize, (f64::from(h) * f64::from(hf.spacing) / 128.0) as usize);
        for teds in [true, false] {
            let mut agree = 0;
            for v in 0..cz {
                for c in 0..cx {
                    let z = if teds { (cz - v) as f64 * 128.0 - 64.0 } else { v as f64 * 128.0 + 64.0 };
                    let (i, k) = (((c as f64 * 128.0 + 64.0) / f64::from(hf.spacing)) as usize, (z / f64::from(hf.spacing)) as usize);
                    let covered = grid.get(k * w as usize + i).copied().flatten().is_some();
                    agree += usize::from(covered == (mask.as_bytes()[v * cx + c] == b'1'));
                }
            }

            println!(
                "mask rows at mesh z {}: {agree} of {} cells agree",
                if teds { "from the far edge (TEd's z)" } else { "from 0" },
                cx * cz
            );
        }
    }

    // The triangles' longest edges (x, z), by size.
    {
        let mesh = model.lods()[0].mesh_blocks()[0].mesh();
        let v: Vec<[f32; 3]> = mesh.vertices().iter().map(|v| v.position()).map(|p| [p.x, p.y, p.z]).collect();
        let mut hist = std::collections::BTreeMap::new();
        let mut apex = std::collections::BTreeMap::new();
        for t in mesh.indices().chunks_exact(3) {
            let p = |i: u16| v[i as usize];
            let e = |a: [f32; 3], b: [f32; 3]| (a[0] - b[0]).hypot(a[2] - b[2]);
            let (a, b, c) = (p(t[0] as u16), p(t[1] as u16), p(t[2] as u16));
            let l = e(a, b).max(e(b, c)).max(e(c, a));
            let bucket = [32.0, 64.0, 128.0, 256.0, 512.0, 1e9].into_iter().find(|&k| l <= k).unwrap();
            *hist.entry(bucket as i64).or_insert(0) += 1;
            if l > 512.0 {
                for &i in t {
                    *apex.entry(i).or_insert(0) += 1;
                }
            }
        }
        println!("{} triangles; longest edge up to: {hist:?}", mesh.indices().len() / 3);
        let mut top: Vec<_> = apex.into_iter().collect();
        top.sort_by_key(|&(_, n)| std::cmp::Reverse(n));
        for (i, n) in top.iter().take(5) {
            println!("  vertex {i} {:?} in {n} long triangles", v[*i as usize]);
        }
    }

    // `WCE_BMP`: the mesh's heights as the height map's rows (flipped) to a BMP: grey by height, red where no triangle covers a sample.
    if let Ok(out) = std::env::var("WCE_BMP") {
        let grid = mesh_heights(&model, w, h, f64::from(hf.spacing), true).expect("mesh");
        let (lo, hi) = grid.iter().flatten().fold((f32::MAX, f32::MIN), |(a, b), &v| (a.min(v), b.max(v)));
        let (w, h) = (w as usize, h as usize);
        let row = (w * 3).div_ceil(4) * 4;
        let mut b = Vec::with_capacity(54 + row * h);
        let le = |v: u32| v.to_le_bytes();
        b.extend_from_slice(b"BM");
        b.extend_from_slice(&le((54 + row * h) as u32));
        b.extend_from_slice(&[0; 4]);
        b.extend_from_slice(&le(54));
        b.extend_from_slice(&le(40));
        b.extend_from_slice(&le(w as u32));
        b.extend_from_slice(&le(h as u32));
        b.extend_from_slice(&[1, 0, 24, 0]);
        b.extend_from_slice(&[0; 24]);
        for j in 0..h {
            for i in 0..w {
                match grid[j * w + i] {
                    Some(v) => {
                        let g = ((v - lo) / (hi - lo).max(1e-6) * 255.0) as u8;
                        b.extend_from_slice(&[g, g, g]);
                    }
                    None => b.extend_from_slice(&[0, 0, 255]),
                }
            }
            b.resize(54 + row * (j + 1), 0);
        }
        std::fs::write(&out, b).expect("bmp");
        println!("{out}: {lo} .. {hi}");
    }

    // Points to read the mesh at (`x,z` in the files' frame, after the folder).
    if let Some(grid) = mesh_heights(&model, w, h, f64::from(hf.spacing), false) {
        for p in std::env::args().skip(2) {
            let v: Vec<f64> = p.split(',').filter_map(|s| s.parse().ok()).collect();
            let (i, k) = ((v[0] / f64::from(hf.spacing)).round() as usize, (v[1] / f64::from(hf.spacing)).round() as usize);
            println!("mesh at x {} z {}: {:?}", v[0], v[1], grid.get(k * w as usize + i).copied().flatten());
        }
    }

    for flip in [false, true] {
        let Some(grid) = mesh_heights(&model, w, h, f64::from(hf.spacing), flip) else {
            println!("flip {flip}: no mesh heights");
            continue;
        };

        let (mut n, mut sum, mut worst, mut covered) = (0usize, 0.0f64, 0.0f64, 0usize);
        for (k, &s) in samples.iter().enumerate() {
            let Some(m) = grid[k] else { continue };
            covered += 1;
            if s == 0 {
                continue;
            }

            let d = (f64::from(hf.metres(s)) - f64::from(m)).abs();
            n += 1;
            sum += d;
            worst = worst.max(d);
        }

        println!(
            "flip {flip}: mesh covers {covered} samples; against {n} of the height map's: mean {:.3}, worst {:.3}",
            sum / n.max(1) as f64,
            worst
        );
    }
}
