//! Phase 0 of docs/plans/procedural_walls.md: a test pack of wall pieces copied from the game's under new keys, plain and bent, to
//! check in game what a generated wall needs (units onto it and on to vanilla neighbours, ladders and towers, breaching, levels with no
//! collapse).
//!
//!   cargo run --release -p wce_walls --example wall_spike -- <game folder> <rpfm schema_att.ron> <out .pack> [--mods a.pack;b.pack]
//!       [--angle degrees]
//!
//! The pieces (the angle is 10 degrees unless given):
//! - `wce_spike_wall8_copy`: `barbarian_fort_straight` renamed, nothing else changed (the pack and database path);
//! - `wce_spike_wall8_bent`: the same bent by the angle about its start, without collapses;
//! - `wce_spike_wall15_copy`: `roman_fort_straight_with_doors_a` renamed;
//! - `wce_spike_wall15_bent`: the same bent, without collapses (its doors' links moved with it).

use std::path::PathBuf;

use rpfm_lib::schema::Schema;
use wce_vfs::{Config, GameProfile, Vfs};
use wce_walls::building::{BuildingFiles, is_piece_mesh};
use wce_walls::warp::{Bend, Warp};
use wce_walls::{db, lines, logic, mesh, pack, zones};

/// Points are added to long outlines every this many metres before bending.
const STEP: f32 = 1.0;

/// A bent docking line is cut into lines of at most this many metres (CA's 20 m radius curve has about 5).
const DOCKING_STEP: f32 = 5.0;

fn main() {
    if let Err(e) = run() {
        eprintln!("wall_spike: {e}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    let flag = |name: &str| args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned();
    let plain = args.iter().enumerate().filter(|(i, a)| !a.starts_with("--") && (*i == 0 || !args[i - 1].starts_with("--"))).map(|(_, a)| a.clone()).collect::<Vec<_>>();
    let [game, schema, out] = plain.as_slice() else {
        return Err("usage: wall_spike <game folder> <rpfm schema_att.ron> <out .pack> [--mods a.pack;b.pack] [--angle degrees]".to_owned());
    };

    let angle = flag("--angle").map(|a| a.parse::<f32>().map_err(|e| format!("--angle: {e}"))).transpose()?.unwrap_or(10.0).to_radians();
    let mut config = Config::new(PathBuf::from(game), GameProfile::ATTILA);
    config.mod_packs = flag("--mods").map(|m| m.split(';').filter(|s| !s.is_empty()).map(str::to_owned).collect());
    let vfs = Vfs::mount(&config).map_err(|e| e.to_string())?;
    if !vfs.missing_mods().is_empty() {
        println!("missing mods: {}", vfs.missing_mods().join(", "));
    }

    let schema = Schema::load(&PathBuf::from(schema), None).map_err(|e| format!("{schema}: {e}"))?;
    let variants = [
        ("barbarian_fort_straight", "wce_spike_wall8_copy", None),
        ("barbarian_fort_straight", "wce_spike_wall8_bent", Some(angle)),
        ("roman_fort_straight_with_doors_a", "wce_spike_wall15_copy", None),
        ("roman_fort_straight_with_doors_a", "wce_spike_wall15_bent", Some(angle)),
    ];

    let mut files = vec![];
    let mut bends: Vec<(String, Bend)> = vec![];
    for (base, key, bend) in variants {
        let source = BuildingFiles::read(&vfs, base)?;
        let logic_path = format!("{}{}", wce_walls::building::folder(base), source.logic_name());
        let from = vfs.owner(&logic_path).map(|i| vfs.packs()[i].name.clone()).unwrap_or_default();
        let logic_bytes = source.files.get(&source.logic_name()).ok_or_else(|| format!("{base}: no logic file"))?.clone();
        let (x0, x1) = logic::extent_x(&logic_bytes)?;
        println!("{key}: from {base} ({from}), x {x0} .. {x1}, {} files", source.files.len());
        println!("  logic file round trip through serde: {}", logic::round_trips(&logic_bytes)?);

        let mut b = source.renamed(key);
        if let Some(a) = bend {
            b = b.without_collapses();
            let warp = Bend::turning(x0, x1, a);
            let (logic_name, zones_name) = (b.logic_name(), b.zones_name());
            for (name, bytes) in &mut b.files {
                if is_piece_mesh(name) {
                    let longest = mesh::longest_edge(bytes)?;
                    let same = mesh::round_trips(bytes)?;
                    *bytes = mesh::warp_mesh(bytes, &warp, STEP)?;
                    println!("  {name}: round trip {same}, longest triangle edge along x {longest:.1} m -> {:.1} m", mesh::longest_edge(bytes)?);
                } else if *name == logic_name {
                    let (w, notes) = logic::warp_logic(bytes, &warp, STEP)?;
                    *bytes = w;
                    println!("  {name}: platforms {} -> {}, firing lines off their strip {}", notes.platforms.0, notes.platforms.1, notes.orphan_lines);
                    for w in notes.warnings {
                        println!("    {w}");
                    }
                } else if *name == zones_name {
                    *bytes = zones::warp_zones(&String::from_utf8_lossy(bytes), &warp, STEP)?.into_bytes();
                }
            }

            bends.push((key.to_owned(), Bend::turning(x0, x1, a)));
            let end = warp.point([x1, 0.0, 0.0]);
            println!("  bent {:.1} degrees about x = {x0}: the end (x = {x1}, z = 0) is at x {:.3}, z {:.3}, facing {:.1} degrees",
                a.to_degrees(), end[0], end[2], warp.yaw([x1, 0.0, 0.0]).to_degrees());
        }

        files.extend(b.into_pack_files());
    }

    let renames = variants.iter().map(|(base, key, _)| (base.to_string(), key.to_string())).collect::<Vec<_>>();
    let mut rows = db::copy_rows(&vfs, &schema, &renames)?;

    // The bent pieces' rows: their firing and docking lines bent with them.
    for (key, warp) in &bends {
        let table = rows.tables.get_mut("models_building_tables").ok_or("no models_building rows")?;
        if let Some(b) = db::cell_mut(table, key, "EFLines").and_then(db::bytes_mut) {
            *b = lines::write_ef_lines(&lines::warp_ef_lines(&lines::read_ef_lines(b)?, warp));
        }

        if let Some(b) = db::cell_mut(table, key, "DockingLines").and_then(db::bytes_mut) {
            let (old, new) = { let old = lines::read_docking_lines(b)?; let new = lines::warp_docking_lines(&old, warp, DOCKING_STEP); (old, new) };
            *b = lines::write_docking_lines(&new);
            println!("{key}: docking lines {} -> {}", old.len(), new.len());
        }
    }

    for (table, t) in &rows.tables {
        println!("{table}: {} rows (from {})", t.data().len(), rows.sources[table].iter().cloned().collect::<Vec<_>>().join(", "));
    }

    let out = PathBuf::from(out);
    let count = files.len();
    pack::write_pack(&out, "attila", files, rows.tables.into_iter().collect())?;
    println!("wrote {} ({count} files)", out.display());
    Ok(())
}
