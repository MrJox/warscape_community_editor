//! Phase 2 of docs/plans/procedural_walls.md: a fixed wall (an S curve over a ground with a slope and a dip, both families) laid out and
//! generated in full (every level's meshes, the logic and zone files), twice: the two runs must give the same bytes, every piece's firing
//! lines must land on a platform strip, and every file must read back.
//!
//!   cargo run --release -p wce_walls --example wall_generate_check -- <game folder> [--mods a.pack;b.pack]

use std::io::Cursor;
use std::path::PathBuf;
use std::time::Instant;

use rpfm_lib::files::cs2_parsed::Cs2Parsed;
use rpfm_lib::files::rigidmodel::RigidModel;
use rpfm_lib::files::{Decodeable, DecodeableExtraData};
use wce_vfs::{Config, GameProfile, Vfs};
use wce_walls::building::{BuildingFiles, is_piece_mesh};
use wce_walls::pieces::{PieceWarp, make_piece, pieces};
use wce_walls::profile::profile;
use wce_walls::solver::{WallInput, solve};

fn main() {
    if let Err(e) = run() {
        eprintln!("wall_generate_check: {e}");
        std::process::exit(1);
    }
}

fn ground(x: f64, z: f64) -> f64 {
    // A slope along x and a dip across it.
    0.04 * x - 3.0 * (-((z - 20.0) / 15.0).powi(2)).exp()
}

fn run() -> Result<(), String> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    let game = args.first().ok_or("usage: wall_generate_check <game folder> [--mods a.pack;b.pack]")?;
    let mut config = Config::new(PathBuf::from(game), GameProfile::ATTILA);
    config.mod_packs = args.iter().position(|a| a == "--mods").and_then(|i| args.get(i + 1)).map(|m| m.split(';').filter(|s| !s.is_empty()).map(str::to_owned).collect());
    let vfs = Vfs::mount(&config).map_err(|e| e.to_string())?;

    let mut bad = 0;
    for key in ["barbarian_fort_straight", "roman_fort_straight"] {
        let p = profile(key).ok_or("no profile")?;
        let base = BuildingFiles::read(&vfs, key)?;
        let input = WallInput { points: vec![[0.0, 0.0], [60.0, 30.0], [120.0, 0.0], [180.0, 25.0]], sharp: vec![false; 4], tolerance: 0.5, ..Default::default() };
        let plan = solve(&input, p, &ground);
        let ps = pieces(&plan, p);
        println!("== {key}: {:.0} long, {} pieces, {} step section(s), {} warning(s)", plan.length, ps.len(), plan.steps.len(), plan.warnings.len());

        let make = || -> Result<Vec<BuildingFiles>, String> {
            ps.iter().enumerate().map(|(i, &piece)| {
                let warp = PieceWarp { plan: &plan, profile: p, piece, ground: &ground };
                make_piece(&base, &format!("wce_check_{i}"), &warp, true, None)
            }).collect()
        };

        let t = Instant::now();
        let first = make()?;
        let ms = t.elapsed().as_millis();
        let second = make()?;
        let same = first.iter().zip(&second).all(|(a, b)| a.files == b.files);
        println!("  generated in {ms} ms ({:.0} per piece); a second run gives the same bytes: {same}", ms as f64 / ps.len() as f64);
        bad += usize::from(!same);

        for (i, b) in first.iter().enumerate() {
            let (mut meshes, mut bytes) = (0, 0);
            for (name, data) in &b.files {
                bytes += data.len();
                if is_piece_mesh(name) {
                    meshes += 1;
                    RigidModel::decode(&mut Cursor::new(data), &Some(DecodeableExtraData::default())).map_err(|e| format!("{name}: {e}"))?;
                }
            }

            let logic = b.files.get(&b.logic_name()).ok_or("no logic file")?;
            let parsed = Cs2Parsed::decode(&mut Cursor::new(logic), &None).map_err(|e| format!("logic: {e}"))?;
            let v = serde_json::to_value(&parsed).map_err(|e| e.to_string())?;
            let (mut plats, mut lines, mut orphans) = (0, 0, 0);
            for d in v["pieces"].as_array().into_iter().flatten().flat_map(|p| p["destructs"].as_array().into_iter().flatten()) {
                let n = d["platforms"].as_array().map_or(0, Vec::len);
                plats += n;
                for l in d["ef_lines"].as_array().into_iter().flatten() {
                    lines += 1;
                    orphans += usize::from(l["parent_index"].as_u64().is_none_or(|k| k as usize >= n));
                }
            }

            let pc = ps[i];
            println!("  piece {i}: s {:.0}..{:.0} (x {:.3}), {} files ({} meshes, {} KB), {} platforms, {} firing lines, {} off a platform",
                pc.s.0, pc.s.1, pc.scale, b.files.len(), meshes, bytes / 1024, plats, lines, orphans);
            bad += orphans;
        }
    }

    if bad == 0 {
        println!("all passed");
        Ok(())
    } else {
        Err(format!("{bad} problem(s)"))
    }
}
