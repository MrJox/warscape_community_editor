//! The kit pieces of each wall culture (`wce_walls::kit`, docs/plans/kit_walls.md phase 1): per base of `profile::BASES` (or the bases
//! given), its pieces by role with their span, turn and rise, and the keys of its prefix that are left out, with why.
//!
//!   cargo run --release -p wce_walls --example kit_pieces -- <game folder> [base ...]
//!
//! `WCE_MODS` (packs, comma separated) mounts mods (TDD's packs for its cultures); `WCE_GAME_KEY` picks the game (default attila).
//! Exits non-zero when a base's culture has no straight. `KIT_DETAIL=key,...` prints those pieces' joints, and their pieces' platforms (level
//! 0: corners, height range), instead.

use std::collections::BTreeMap;
use std::path::PathBuf;

use wce_vfs::{Config, GameProfile, Vfs};
use wce_walls::kit::{Role, catalogue, culture_keys};

fn main() {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    let Some(game) = args.first() else {
        eprintln!("usage: kit_pieces <game folder> [base ...]");
        std::process::exit(2);
    };

    let profile = GameProfile::from_env().clone();
    let mut config = Config::new(PathBuf::from(game), profile);
    if let Ok(mods) = std::env::var("WCE_MODS") {
        config.use_mods = true;
        config.mod_packs = Some(mods.split(',').map(|m| m.trim().to_owned()).filter(|m| !m.is_empty()).collect());
    }

    let vfs = Vfs::mount(&config).expect("the game's packs");
    let keys = vfs.list("rigidmodels/buildings/", "_tech.cs2.parsed").into_iter().filter_map(|p| p.rsplit('/').next()?.strip_suffix("_tech.cs2.parsed").map(str::to_owned)).collect::<Vec<_>>();
    let bases = if args.len() > 1 { args[1..].to_vec() } else { wce_walls::profile::BASES.iter().map(|b| b.key.to_owned()).collect() };
    let read = |k: &str| vfs.read(&format!("{}{k}_tech.cs2.parsed", wce_walls::building::folder(k))).ok();
    if let Ok(detail) = std::env::var("KIT_DETAIL") {
        for key in detail.split(',') {
            let Some(bytes) = read(key) else { println!("{key}: no logic file"); continue };
            println!("== {key}: joints {:?}", wce_walls::kit::joints(&bytes));
            println!("   logic::ends {:?}; walkway_ends {:?}", wce_walls::logic::ends(&bytes), wce_walls::kit::walkway_ends(&bytes));
            let parsed = <rpfm_lib::files::cs2_parsed::Cs2Parsed as rpfm_lib::files::Decodeable>::decode(&mut std::io::Cursor::new(&bytes), &None).expect("cs2.parsed");
            let v = serde_json::to_value(&parsed).expect("json");
            println!("   {} piece(s)", v["pieces"].as_array().map_or(0, Vec::len));
            for (i, piece) in v["pieces"].as_array().into_iter().flatten().enumerate() {
                let levels = piece["destructs"].as_array().cloned().unwrap_or_default();
                println!("   piece {i}: {} level(s) {:?}, platforms per level {:?}", levels.len(), levels.iter().map(|d| d["index"].as_u64()).collect::<Vec<_>>(), levels.iter().map(|d| d["platforms"].as_array().map_or(0, Vec::len)).collect::<Vec<_>>());
                let d = levels.iter().find(|d| d["index"].as_u64() == Some(0)).or(levels.first()).cloned().unwrap_or_default();
                for (k, p) in d["platforms"].as_array().into_iter().flatten().enumerate() {
                    let pts = p["vertices"]["outline"].as_array().cloned().unwrap_or_default();
                    let ys = pts.iter().map(|q| q["y"].as_f64().unwrap_or(0.0)).collect::<Vec<_>>();
                    let xz = pts.iter().map(|q| format!("({:.1},{:.1})", q["x"].as_f64().unwrap_or(0.0), q["z"].as_f64().unwrap_or(0.0))).collect::<Vec<_>>().join(" ");
                    println!("   piece {i} platform {k}: {} corners, y {:.2}..{:.2}: {xz}", pts.len(), ys.iter().copied().fold(f64::MAX, f64::min), ys.iter().copied().fold(f64::MIN, f64::max));
                }
            }
        }

        return;
    }
    let mut failed = false;
    for base in &bases {
        let keys = culture_keys(base, keys.iter().map(String::as_str));
        let c = catalogue(base, &keys, &read);
        let mut counts = BTreeMap::new();
        for p in &c.pieces {
            *counts.entry(format!("{:?}", p.role)).or_insert(0) += 1;
        }

        println!("== {base}: {} key(s), {} piece(s) {counts:?}, {} left out", keys.len(), c.pieces.len(), c.left_out.len());
        for p in &c.pieces {
            println!("   {:<9} {:<55} span {:6.2} turn {:7.2} rise {:6.2} shift {:6.2} walk {:6.2} footprint {}", format!("{:?}", p.role), p.key, p.span, p.turn, p.rise, p.shift, p.walk, p.footprint.len());
        }

        for (k, why) in &c.left_out {
            println!("   left out  {k:<55} {why}");
        }

        if c.of(Role::Straight).next().is_none() && !keys.is_empty() {
            println!("   NO STRAIGHT");
            failed = true;
        }
    }

    if failed {
        std::process::exit(1);
    }
}
