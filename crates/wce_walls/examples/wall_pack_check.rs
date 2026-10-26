//! Phase 5 of docs/plans/procedural_walls.md: a pack exported with procedural walls (`save_exported` of a tile with walls) holds what
//! the game needs of every generated building: its folder (the meshes of every level, the logic and zone files), its
//! `battlefield_buildings` and `models_building` rows (the logic file's path its own, firing and docking lines), and its instances in the
//! tile's `building_list`.
//!
//!   cargo run --release -p wce_walls --example wall_pack_check -- <pack> <rpfm schema of the game> [game key: attila, rome_2]

use std::collections::{BTreeMap, BTreeSet};
use std::io::Cursor;

use rpfm_lib::files::db::DB;
use rpfm_lib::files::esf::ESF;
use rpfm_lib::files::pack::Pack;
use rpfm_lib::files::{Decodeable, DecodeableExtraData};
use rpfm_lib::games::supported_games::SupportedGames;
use rpfm_lib::schema::Schema;
use wce_walls::lines;

fn main() {
    if let Err(e) = run() {
        eprintln!("wall_pack_check: {e}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    let (pack_path, schema, game) = match args.as_slice() {
        [p, s] => (p, s, "attila"),
        [p, s, g] => (p, s, g.as_str()),
        _ => return Err("usage: wall_pack_check <pack> <rpfm schema of the game> [game key: attila, rome_2]".to_owned()),
    };
    let schema = Schema::load(std::path::Path::new(schema), None).map_err(|e| e.to_string())?;
    let games = SupportedGames::default();
    let mut pack = Pack::read_and_merge(&[pack_path.into()], games.game(game).ok_or_else(|| format!("rpfm knows no {game}"))?, true, false, false).map_err(|e| e.to_string())?;
    let names = pack.files().keys().cloned().collect::<Vec<_>>();
    let mut read = |n: &str| pack.files_mut().get_mut(n).and_then(|f| f.encode(&None, false, false, true).ok().flatten());

    // The generated buildings' folders.
    let mut folders: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for n in &names {
        if let Some(rest) = n.strip_prefix("rigidmodels/buildings/") && let Some((key, file)) = rest.split_once('/') {
            folders.entry(key.to_owned()).or_default().push(file.to_owned());
        }
    }

    // The rows.
    let mut rows: BTreeMap<String, BTreeMap<String, Vec<rpfm_lib::files::table::DecodedData>>> = BTreeMap::new();
    let mut columns: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for n in names.iter().filter(|n| n.starts_with("db/")) {
        let table = n.split('/').nth(1).unwrap_or_default().to_owned();
        let bytes = read(n).ok_or_else(|| format!("{n}: no bytes"))?;
        let mut extra = DecodeableExtraData::default();
        extra.set_schema(Some(&schema));
        extra.set_table_name(Some(&table));
        let db = DB::decode(&mut Cursor::new(&bytes), &Some(extra)).map_err(|e| format!("{n}: {e}"))?;
        let id = db.column_position_by_name("key").unwrap_or(0);
        columns.insert(table.clone(), db.definition().fields_processed().iter().map(|f| f.name().to_owned()).collect());
        for r in db.data().iter() {
            rows.entry(table.clone()).or_default().insert(r[id].data_to_string().into_owned(), r.clone());
        }
    }

    // The instances of the building lists.
    let mut placed: BTreeMap<String, usize> = BTreeMap::new();
    for n in names.iter().filter(|n| n.ends_with("/building_list")) {
        let bytes = read(n).ok_or_else(|| format!("{n}: no bytes"))?;
        let esf = ESF::decode(&mut Cursor::new(&bytes), &None).map_err(|e| format!("{n}: {e}"))?;
        for b in wce_tile_io::attila::building_list::read(&esf)? {
            *placed.entry(b.key).or_default() += 1;
        }
    }

    let generated = folders.keys().filter(|k| k.starts_with("wce_")).cloned().collect::<BTreeSet<_>>();
    let mut bad = 0;
    println!("{} generated buildings in {pack_path}", generated.len());
    let cell = |table: &str, key: &str, col: &str| -> Option<rpfm_lib::files::table::DecodedData> {
        let i = columns.get(table)?.iter().position(|c| c == col)?;
        rows.get(table)?.get(key).map(|r| r[i].clone())
    };

    for key in &generated {
        let files = &folders[key];
        let meshes = files.iter().filter(|f| f.ends_with(".rigid_model_v2")).count();
        let logic = files.iter().any(|f| *f == format!("{key}_tech.cs2.parsed"));
        let zones = files.iter().any(|f| *f == format!("{key}_zone_tech.xml"));
        let bb = rows.get("battlefield_buildings_tables").is_some_and(|t| t.contains_key(key));
        let logic_path = cell("models_building_tables", key, "logic_file").map(|c| c.data_to_string().into_owned()).unwrap_or_default();
        let bytes = |c: Option<rpfm_lib::files::table::DecodedData>| match c {
            Some(rpfm_lib::files::table::DecodedData::SequenceU16(b) | rpfm_lib::files::table::DecodedData::SequenceU32(b)) => b,
            _ => vec![],
        };
        let ef = lines::read_ef_lines(&bytes(cell("models_building_tables", key, "EFLines"))).map_or(0, |l| l.len());
        let docking = lines::read_docking_lines(&bytes(cell("models_building_tables", key, "DockingLines"))).map_or(0, |l| l.len());
        let own = logic_path.to_lowercase() == format!("buildings\\{key}\\{key}_tech.cs2.parsed");
        let n = placed.get(key).copied().unwrap_or(0);
        let ok = meshes > 0 && logic && zones && bb && own && ef > 0 && n == 1;
        bad += usize::from(!ok);
        println!("  {key}: {meshes} meshes, logic {logic}, zones {zones}, battlefield_buildings {bb}, models_building logic_file own {own}, {ef} firing lines, {docking} docking lines, placed {n}{}",
            if ok { "" } else { "  <- PROBLEM" });
    }

    if generated.is_empty() || bad > 0 {
        return Err(format!("{bad} problem(s)"));
    }

    println!("all passed");
    Ok(())
}
