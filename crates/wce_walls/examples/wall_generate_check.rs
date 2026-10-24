//! Phases 2-4 of docs/plans/procedural_walls.md: a fixed wall (an S curve with a sharp corner over a ground with a slope and a dip, both
//! families: curves, a mitre, step sections, door sections) laid out and generated in full (every level's meshes, the logic and zone
//! files), twice: the two runs must give the same bytes, every piece's firing lines must land on a platform strip, every door's foot
//! must be on the ground, and every file must read back. A piece with a stair keeps it at the levels its step piece has, and at the
//! others (the rubble: the step pieces have none) is the base alone, as made without steps.
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
use wce_walls::pieces::{PieceWarp, StepFiles, door_pieces, level_of, make_piece, pieces, step_ranges};
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
    let flat = |_: f64, z: f64| 1.0 + 0.002 * z;
    let cases: [(&str, Vec<[f64; 2]>, Vec<bool>, &dyn Fn(f64, f64) -> f64); 3] = [
        ("barbarian_fort_straight", vec![[0.0, 0.0], [60.0, 30.0], [120.0, 0.0], [180.0, 25.0], [260.0, 25.0]], vec![false, false, true, false, false], &ground),
        ("roman_fort_straight", vec![[0.0, 0.0], [60.0, 30.0], [120.0, 0.0], [180.0, 25.0], [260.0, 25.0]], vec![false, false, true, false, false], &ground),
        ("roman_fort_straight", vec![[0.0, 0.0], [400.0, 0.0]], vec![false, false], &flat),
    ];

    for (key, points, sharp, ground) in cases {
        let p = profile(key).ok_or("no profile")?;
        let base = BuildingFiles::read(&vfs, key)?;
        let (up, down) = (BuildingFiles::read(&vfs, p.step_up)?, BuildingFiles::read(&vfs, p.step_down)?);
        let steps = StepFiles { up: Some(&up), down: Some(&down) };
        let input = WallInput { points, sharp, tolerance: 0.5, access_every: 200.0, defence_every: 120.0, ..Default::default() };
        let plan = solve(&input, p, ground);
        let (rigid, ps): (Vec<_>, Vec<_>) = pieces(&plan, p).into_iter().partition(|q| q.rigid);
        println!("  towers and bastions: {:?}", rigid.iter().map(|q| (q.s.0.round(), q.base.unwrap_or_default())).collect::<Vec<_>>());
        println!("== {key}: {:.0} long, {} pieces, {} step section(s), {} warning(s)", plan.length, ps.len(), plan.steps.len(), plan.warnings.len());

        let door = (!p.door.is_empty()).then(|| BuildingFiles::read(&vfs, p.door)).transpose()?;
        let (doors, _) = door_pieces(&plan, p, &ps, ground, &|_| true);
        println!("  door sections at pieces {doors:?}");
        let make = || -> Result<Vec<BuildingFiles>, String> {
            ps.iter().enumerate().map(|(i, &piece)| {
                let warp = PieceWarp::new(&plan, p, piece, ground);
                let own = piece.base.map(|k| BuildingFiles::read(&vfs, k)).transpose()?;
                let from = own.as_ref().or(door.as_ref().filter(|_| doors.contains(&i))).unwrap_or(&base);
                make_piece(from, &format!("wce_check_{i}"), &warp, steps, true, None)
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
            let (mut plats, mut lines, mut orphans, mut feet) = (0, 0, 0, 0);
            let piece = ps[i];
            let (s, c) = piece.yaw.sin_cos();
            for d in v["pieces"].as_array().into_iter().flatten().flat_map(|p| p["destructs"].as_array().into_iter().flatten()) {
                let n = d["platforms"].as_array().map_or(0, Vec::len);
                plats += n;

                // A door's foot (a pipe's lowest point) must be on the ground.
                for pipe in d["pipes"].as_array().into_iter().flatten() {
                    let pts = pipe["line"]["outline"].as_array().cloned().unwrap_or_default();
                    let low = pts.iter().min_by(|a, b| a["y"].as_f64().unwrap_or(0.0).total_cmp(&b["y"].as_f64().unwrap_or(0.0)));
                    if let Some(q) = low {
                        let (lx, ly, lz) = (q["x"].as_f64().unwrap_or(0.0), q["y"].as_f64().unwrap_or(0.0), q["z"].as_f64().unwrap_or(0.0));
                        let (wx, wz) = (piece.origin[0] + lx * c - lz * s, piece.origin[2] + lx * s + lz * c);
                        if (piece.origin[1] + ly - ground(wx, wz)).abs() > 0.01 {
                            orphans += 1;
                        }
                        feet += 1;
                    }
                }
                for l in d["ef_lines"].as_array().into_iter().flatten() {
                    lines += 1;
                    orphans += usize::from(l["parent_index"].as_u64().is_none_or(|k| k as usize >= n));
                }
            }

            // A stair stands only where its step piece has the level: elsewhere the piece is the base alone.
            let mut stair = String::new();
            if !step_ranges(&plan, p, &piece).is_empty() && piece.base.is_none() && !doors.contains(&i) {
                let levels = [&up, &down].iter().flat_map(|f| f.files.keys()).filter(|n| is_piece_mesh(n)).map(|n| level_of(n)).max().unwrap_or(0);
                let warp = PieceWarp::new(&plan, p, piece, ground);
                let bare = make_piece(&base, &format!("wce_check_{i}"), &warp, StepFiles { up: None, down: None }, true, None)?;
                let destructs = |f: &BuildingFiles| -> Result<Vec<serde_json::Value>, String> {
                    let l = Cs2Parsed::decode(&mut Cursor::new(&f.files[&f.logic_name()]), &None).map_err(|e| e.to_string())?;
                    let v = serde_json::to_value(&l).map_err(|e| e.to_string())?;
                    Ok(v["pieces"][0]["destructs"].as_array().cloned().unwrap_or_default())
                };
                let (with, without) = (destructs(b)?, destructs(&bare)?);
                let mut wrong = 0;
                // The sections go into the first piece's meshes, and into the logic when the step pieces have one.
                for (name, data) in b.files.iter().filter(|(n, _)| is_piece_mesh(n) && n.contains("_piece01_")) {
                    let same = bare.files.get(name) == Some(data);
                    wrong += usize::from(same == (level_of(name) <= levels));
                }

                let step_logic = [&up, &down].iter().any(|f| f.files.contains_key(&f.logic_name()));
                for (k, (a, z)) in with.iter().zip(&without).enumerate().filter(|_| step_logic) {
                    let same = a["platforms"] == z["platforms"];
                    wrong += usize::from(same == (k as u32 + 1 <= levels));
                }

                stair = format!(", a stair at levels 1..{levels} only: {}", if wrong == 0 { "yes" } else { "NO" });
                orphans += wrong;
            }

            let pc = ps[i];
            println!("  piece {i}: s {:.0}..{:.0} (x {:.3}), {} files ({} meshes, {} KB), {} platforms, {} firing lines, {} door feet{stair}, {} problems",
                pc.s.0, pc.s.1, pc.scale, b.files.len(), meshes, bytes / 1024, plats, lines, feet, orphans);
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
