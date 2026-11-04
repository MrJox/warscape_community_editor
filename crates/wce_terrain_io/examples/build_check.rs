//! The whole build of a kit battle map as the Terrain tool exports it (`wce_terrain_io::attila::build`: the map's folder read into the
//! model, every built file made from it) against a BOB_Terrain run's outputs for the same folder (a scratch kit's working_data). Every
//! file BOB wrote must come out byte for byte, but `tile_map.tiles`' LF floats of cells that are no tile's origin (BOB leaves them
//! uninitialised: docs/re/terry.md, "BOB_Terrain's outputs"). Exit 1 otherwise.
//! `WCE_OUT=<folder>`: our versions of the differing files written there.
//!
//!   cargo run --release -p wce_terrain_io --example build_check -- <kit raw_data> <map | campaigns/<map>> <BOB's output folder>

use std::collections::BTreeMap;
use std::path::PathBuf;

use wce_terrain::MapKind;
use wce_terrain_io::attila::build::{BuildInputs, build};
use wce_terrain_io::{MapFiles, attila};
use wce_tile::battle_map::tile_db::TileDatabase;
use wce_tile::battle_map::tile_map::TileMap;

/// BOB's files under a folder, by relative path (`/`).
fn listing(dir: &std::path::Path, prefix: &str, out: &mut BTreeMap<String, PathBuf>) {
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        let rel = if prefix.is_empty() { name } else { format!("{prefix}/{name}") };
        if e.path().is_dir() {
            listing(&e.path(), &rel, out);
        } else {
            out.insert(rel, e.path());
        }
    }
}

/// Two tile maps the same but for the LF of non-origin cells.
fn same_tile_maps(a: &TileMap, b: &TileMap) -> bool {
    a.width == b.width && a.height == b.height && a.names == b.names && a.layers.iter().zip(&b.layers).all(|(x, y)| {
        x.len() == y.len() && x.iter().zip(y).all(|(c, d)| {
            let (mut c, mut d) = (*c, *d);
            if c.index <= 0 {
                c.lf = [0.0; 2];
                d.lf = [0.0; 2];
            }

            c == d
        })
    })
}

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let [raw, map, out] = &a[..] else {
        eprintln!("build_check <kit raw_data> <map> <BOB's output folder>");
        std::process::exit(2);
    };

    let (raw, out) = (PathBuf::from(raw), PathBuf::from(out));
    let t0 = std::time::Instant::now();
    // `campaigns/<map>`: a campaign map (its folder, its tile database).
    let (kind, folder, name, base) = match map.strip_prefix("campaigns/") {
        Some(n) => (MapKind::Campaign, "terrain/campaigns", n, "campaign"),
        None => (MapKind::Battle, "terrain/battles", map.as_str(), "battle"),
    };

    let files = MapFiles::from_dir(&raw.join(folder).join(name)).expect("the map's folder");
    let m = attila::read(name, kind, &files).expect("the map");
    let db = TileDatabase::load(&raw.join(format!("terrain/tiles/{base}/_tile_database"))).expect("tile database");
    let env = std::fs::read(raw.join("warscape_asset_variation_db/environment_map.xml")).unwrap_or_default();
    let upgrades = std::fs::read(raw.join("terrain/tiles/battle/tile_upgrades.xml")).unwrap_or_default();
    let built = build(&m, &BuildInputs { db: &db, environment_map: &env, tile_upgrades: &upgrades }).expect("the build");
    println!("{map}: {} files built in {:.1} s ({} messages)", built.files.len(), t0.elapsed().as_secs_f64(), built.messages.len());

    let mut theirs = BTreeMap::new();
    listing(&out, "", &mut theirs);
    let ours: BTreeMap<&str, &Vec<u8>> = built.files.iter().map(|(k, v)| (k.as_str(), v)).collect();
    let (mut same, mut bad) = (0, 0);
    for (rel, path) in &theirs {
        if rel.ends_with(".agf") {
            continue;
        }

        let want = std::fs::read(path).unwrap_or_default();
        let Some(mine) = ours.get(rel.as_str()) else {
            println!("  {rel}: not built");
            bad += 1;
            continue;
        };

        if **mine == want {
            same += 1;
        } else if rel == "tile_map.tiles"
            && let (Ok(x), Ok(y)) = (TileMap::parse(&ours["tile_map.index"][..], mine), TileMap::parse(&std::fs::read(out.join("tile_map.index")).unwrap_or_default(), &want))
            && same_tile_maps(&x, &y)
        {
            same += 1;
            println!("  {rel}: the same but for the LF of non-origin cells");
        } else {
            bad += 1;
            // `WCE_OUT`: a folder for our versions of the files that differ.
            if let Ok(dir) = std::env::var("WCE_OUT") {
                std::fs::write(PathBuf::from(dir).join(rel), mine).expect("WCE_OUT writable");
            }

            let first = mine.iter().zip(&want).position(|(x, y)| x != y);
            println!("  {rel}: DIFFERS (sizes {} / {}, first at {first:?})", mine.len(), want.len());
        }
    }

    let extra: Vec<&str> = ours.keys().filter(|k| !theirs.contains_key(**k)).copied().collect();
    println!("{same} of {} of BOB's files the same; built besides: {extra:?}", theirs.keys().filter(|k| !k.ends_with(".agf")).count());
    if bad > 0 {
        std::process::exit(1);
    }
}
