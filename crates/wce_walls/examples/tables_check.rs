//! Checks a game's tables (`tables/<game key>`: walls, bridges, road fences and road types; docs/editor.md, "Tables in files") against the
//! game's own files: every building they name is in its packs (its logic file), every wall base's straight has its family's frame, depth
//! and walkway, every fence runs about a fence piece's 5 m along the axis its table says, and the dirt road types' decals are there.
//!
//!   WCE_GAME_KEY=rome_2 cargo run --release -p wce_walls --example tables_check -- <game folder>
//!
//! `WCE_MODS` (packs, comma separated) mounts mods, and the tables' files of those mods are read over the game's (as the editor does).
//! Prints each finding; exits non-zero when anything is missing or off.

use std::path::PathBuf;

use wce_vfs::files::Files;
use wce_vfs::{Config, GameProfile, Vfs};

fn main() {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    let Some(game_dir) = args.first() else {
        eprintln!("usage: tables_check <game folder>");
        std::process::exit(2);
    };

    let profile = GameProfile::from_env().clone();
    let mut config = Config::new(PathBuf::from(game_dir), profile.clone());
    if let Ok(mods) = std::env::var("WCE_MODS") {
        config.use_mods = true;
        config.mod_packs = Some(mods.split(',').map(|m| m.trim().to_owned()).filter(|m| !m.is_empty()).collect());
    }

    let vfs = Vfs::mount(&config).expect("the game's packs");
    // `TABLES_LIST=<folder>,<suffix>`: the packs' files there instead (looking for a game's own pieces).
    if let Ok(list) = std::env::var("TABLES_LIST") {
        let (folder, suffix) = list.split_once(',').unwrap_or((&list, ""));
        for f in vfs.list(folder, suffix) {
            println!("{f}");
        }

        return;
    }

    let tables = wce_tile::user_files::tables_dir().join(profile.key);
    let text = |name: &str| std::fs::read_to_string(tables.join(name)).unwrap_or_else(|e| panic!("{}: {e}", tables.join(name).display()));
    // The files over each table for the mods mounted, as the editor reads them.
    let mods = config.mod_packs.clone().unwrap_or_default();
    let extras = |name: &str| wce_tile::user_files::extra_files(&tables, name, &mods);
    let logic = |k: &str| vfs.read(&format!("{}{k}_tech.cs2.parsed", wce_walls::building::folder(k))).ok();
    let mut bad = 0;
    let mut say = |ok: bool, what: String| {
        if !ok {
            bad += 1;
        }

        println!("{} {what}", if ok { "  ok " } else { "  BAD" });
    };

    println!("== {} ({})", profile.display_name, tables.display());

    // The walls: each base's pieces there, its straight's frame, depth and walkway its table's.
    let walls = wce_walls::profile::parse_with(&text("walls.toml"), &extras("walls.toml")).expect("walls");
    for b in &walls.bases {
        println!("-- wall {} ({})", b.key, b.family);
        let pieces = [b.key, b.cap_start, b.cap_end, b.step_up, b.step_down, b.door].into_iter().chain(b.defences.iter().copied());
        let missing = pieces.filter(|k| !k.is_empty() && logic(k).is_none()).collect::<Vec<_>>();
        say(missing.is_empty(), format!("pieces in the packs (missing: {missing:?})"));
        let Some(bytes) = logic(b.key) else { continue };
        let joint = wce_walls::logic::joint_extent_x(&bytes).unwrap_or((0.0, 0.0));
        let near = |a: f64, b: f64, tol: f64| (a - b).abs() <= tol;
        say(
            near(f64::from(joint.0), b.x.0, 0.3) && near(f64::from(joint.1), b.x.1, 0.3),
            format!("frame x {:.2}..{:.2} (table {:.2}..{:.2})", joint.0, joint.1, b.x.0, b.x.1),
        );
        let walk = wce_walls::logic::walkway_z(&bytes, b.walkway as f32);
        say(walk.is_some(), format!("a walkway at {} (z {walk:?})", b.walkway));
        // What to compare with another game's (the same building, the same numbers): its logic box and a hash of its logic file.
        let [x, z] = wce_walls::logic::extent_xz(&bytes).unwrap_or_default();
        println!("  sig {} box x {:.2}..{:.2} z {:.2}..{:.2} hash {:x}", b.key, x.0, x.1, z.0, z.1, hash(&bytes));
    }

    // The bridges.
    let bridges = wce_walls::bridge::parse_with(&text("bridges.toml"), &extras("bridges.toml")).expect("bridges");
    for b in &bridges.bases {
        let missing = std::iter::once(b.key).chain(b.end).filter(|k| logic(k).is_none()).collect::<Vec<_>>();
        say(missing.is_empty(), format!("bridge {}: {} and its end in the packs (missing: {missing:?})", b.name, b.key));
        for k in std::iter::once(b.key).chain(b.end) {
            if let Some(bytes) = logic(k) {
                println!("  sig {k} hash {:x}", hash(&bytes));
            }
        }
    }

    // The fences: each about 5 m along its axis (logic box, scene units: 2 a metre), its joiners there.
    let fences = wce_tile::road_fences::parse_with(&text("road_fences.toml"), &extras("road_fences.toml")).expect("fences");
    for f in std::iter::once(&fences.fence).chain(&fences.named) {
        let Some(bytes) = logic(&f.key) else {
            say(false, format!("fence {}: {} not in the packs", f.piece(), f.key));
            continue;
        };

        let [x, z] = wce_walls::logic::extent_xz(&bytes).unwrap_or_default();
        let (lx, lz) = (f64::from(x.1 - x.0) / 2.0, f64::from(z.1 - z.0) / 2.0);
        let along = if f.along == wce_tile::road_fences::Along::X { lx } else { lz };
        let axis_ok = if f.along == wce_tile::road_fences::Along::X { lx >= lz } else { lz >= lx };
        println!("  sig {} hash {:x}", f.key, hash(&bytes));
        // The logic box reaches past the model a little (`lowfence_2`: 5.7 m, its model 5.1).
        say(
            axis_ok && (along - 5.0).abs() <= 0.75,
            format!("fence {}: {} {along:.2} m along {:?} (x {lx:.2} m, z {lz:.2} m)", f.piece(), f.key, f.along),
        );
        let missing = f.joiners.iter().filter(|j| logic(j).is_none()).collect::<Vec<_>>();
        if !f.joiners.is_empty() {
            say(missing.is_empty(), format!("fence {}: joiners in the packs (missing: {missing:?})", f.piece()));
        }
    }

    // The dirt road types' decals, when the game has a type.
    let types = wce_tile::road_dirt::parse_with(&text("road_types.toml"), &extras("road_types.toml")).expect("road types");
    println!("-- road types {:?}", types.iter().map(|t| t.name).collect::<Vec<_>>());
    let missing = wce_tile::road_dirt::DECALS
        .iter()
        .filter(|d| !vfs.exists(&format!("rigidmodels/decals/battle/{d}.rigid_model_v2")))
        .collect::<Vec<_>>();
    say(types.is_empty() || missing.is_empty(), format!("the dirt roads' decals in the packs (missing: {missing:?})"));

    println!("== {bad} finding(s)");
    if bad > 0 {
        std::process::exit(1);
    }
}

/// A file's hash, to see whether two games have the same building.
fn hash(bytes: &[u8]) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    bytes.hash(&mut h);
    h.finish()
}
