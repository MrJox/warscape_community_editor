//! The BCA generator (wce_terrain::bca) against Terry's output: a kit map's BCAs made from its tile map and the spec, compared with a
//! `.terry` Terry generated (by name: type and centre cell; their deployment zones' regions' polylines).
//!
//! Terry builds its battle field from the tile map the game's file system finds first: the game packs' `tile_map.index` and `.tiles`
//! over the kit's working_data (BOB's): `BCA_TILEMAP=<folder>` reads them from a folder (else the battle field is BOB's fill of the kit
//! map). Exit 1 unless every BCA (name, type, cell) and every deployment zone (name, regions, points to 5 cm) agrees.
//!
//!   cargo run --release -p wce_terrain --example bca_check -- <kit raw_data/terrain> <map> <bca_generation_spec.xml> <Terry's .terry>

use std::collections::BTreeMap;
use std::path::PathBuf;

use wce_terrain::bca::{Spec, generate};
use wce_terrain::terry::{EntityKind, TerryProject};
use wce_tile::battle_map::kit_battle_field;
use wce_tile::battle_map::tile_db::TileDatabase;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let [terrain, map, spec, oracle] = &a[..] else {
        eprintln!("bca_check <kit raw_data/terrain> <map> <bca_generation_spec.xml> <Terry's .terry>");
        std::process::exit(2);
    };

    let terrain = PathBuf::from(terrain);
    let db = TileDatabase::load(&terrain.join("tiles/battle/_tile_database")).expect("tile database");
    let (w, h, mut instances) = kit_battle_field(&db, &terrain.join("battles").join(map)).expect("the map");
    if let Ok(dir) = std::env::var("BCA_TILEMAP") {
        // The battle field of another tile map (TILE_MAP::build_battle_field over the whole map).
        let dir = PathBuf::from(dir);
        let m = wce_tile::battle_map::tile_map::TileMap::parse(&std::fs::read(dir.join("tile_map.index")).expect("index"),
            &std::fs::read(dir.join("tile_map.tiles")).expect("tiles")).expect("tile map");
        let mut by_name = std::collections::HashMap::new();
        for (t, tile) in db.tiles.iter().enumerate() {
            for v in 0..tile.variations.len() {
                by_name.insert(db.tile_location_of(t, v).to_lowercase(), (t, v));
            }
        }

        let mut missing = 0;
        instances.clear();
        for c in 0..(m.width * m.height) as usize {
            for layer in 0..2 {
                let cell = m.layers[layer][c];
                if cell.index <= 0 {
                    continue;
                }

                let Some(&(tile, variation)) = by_name.get(&m.names[(cell.index - 1) as usize].to_lowercase()) else { missing += 1; continue };
                instances.push(wce_tile::battle_map::fill::Placement {
                    tile, variation, x: cell.origin[0] as i16, y: cell.origin[1] as i16, flags: cell.flags | if cell.flipped != 0 { 4 } else { 0 },
                    climate: cell.climate, lf: cell.lf,
                });
            }
        }

        println!("tile map {}: {} x {}, {} instances, {missing} not in the database", dir.display(), m.width, m.height, instances.len());
    }

    let spec = Spec::parse(&std::fs::read(spec).expect("spec"));
    println!("spec: {} types ({}), border {}, delete_invalid {}", spec.types.len(), spec.types.iter().map(|t| format!("{} {}", t.name, t.infield.len())).collect::<Vec<_>>().join(", "), spec.map_border_for_outfield, spec.delete_invalid);
    let masks = wce_terrain::bca::load_masks(&db, &instances, &[terrain.parent().map(PathBuf::from).unwrap_or_default()]);
    let t0 = std::time::Instant::now();
    let ours = generate(&spec, &db, w, h, &instances, &masks);
    println!("generated in {:.1} s", t0.elapsed().as_secs_f64());
    let kept: Vec<_> = ours.iter().filter(|b| !(spec.delete_invalid && !b.errors.is_empty())).collect();
    println!("{map}: {w} x {h} cells, {} tile instances, {} deployment masks; {} BCAs made, {} kept", instances.len(), masks.len(), ours.len(), kept.len());

    let p = TerryProject::parse(std::fs::read(oracle).expect("oracle")).expect("xml");
    if let Ok(out) = std::env::var("BCA_WRITE") {
        // The oracle's project with its procedural BCAs replaced by ours (y 0), for checking the writer by running on it.
        let mut q = p.clone();
        q.remove_entities(&wce_terrain::bca::procedural(&q)).expect("removed");
        let (xml, _) = wce_terrain::bca::terry_xml(&spec, &kept, w, h, q.next_id(), &|_, _| 0.0);
        q.insert_entities(&xml).expect("inserted");
        std::fs::write(&out, q.save()).expect("written");
        println!("written {out}");
    }

    let mut theirs: BTreeMap<String, (String, (i32, i32))> = BTreeMap::new();
    for e in &p.entities {
        let EntityKind::Bca(kind) = p.kind(e) else { continue };
        let pos = p.world_position(e.id);
        let cell = ((pos[0] / 256.0).floor() as i32 + (w / 2) as i32, (h / 2) as i32 - (pos[2] / 256.0).floor() as i32);
        theirs.insert(e.name.clone(), (kind, cell));
    }

    let mine: BTreeMap<String, (String, (i32, i32))> = kept.iter().map(|b| (b.name.clone(), (b.kind.clone(), b.cell))).collect();
    let (mut same, mut shown) = (0, 0);
    for name in mine.keys().chain(theirs.keys()).collect::<std::collections::BTreeSet<_>>() {
        match (mine.get(name), theirs.get(name)) {
            (Some(a), Some(b)) if a == b => same += 1,
            (a, b) => {
                if shown < 25 {
                    println!("  {name}: ours {a:?}, Terry's {b:?}");
                    shown += 1;
                }
            }
        }
    }

    println!("{same} of {} (Terry: {}) agree", mine.len(), theirs.len());
    if let Ok(r) = std::env::var("BCA_SHOW") {
        let (a, b) = r.split_once('-').map(|(a, b)| (a.parse::<usize>().unwrap_or(0), b.parse::<usize>().unwrap_or(0))).unwrap_or((0, 0));
        for k in a..=b {
            let name = format!("BCA {k:05}");
            let Some((kind, cell)) = theirs.get(&name) else { println!("  {name}: (Terry: none)"); continue };
            match ours.iter().find(|x| &x.kind == kind && x.cell == *cell) {
                Some(x) => {
                    let p = &instances[x.instance];
                    let t = &db.tiles[p.tile];
                    println!("  {name} {kind} {cell:?}: instance {} origin ({}, {}) flags {:#x} {} {}x{} (ours {})", x.instance, p.x, p.y, p.flags, t.location, t.width, t.height, x.name);
                }
                None => println!("  {name} {kind} {cell:?}: not made by us"),
            }
        }
    }
    // Deployment zones: Terry's under each of its BCAs against ours for the BCA of that type and cell.
    let (mut dz_same, mut dz_total, mut dz_shown) = (0, 0, 0);
    let mut dz_counts = (0usize, 0usize, 0usize);
    let (cw, ch) = ((w / 2) as f64, (h / 2) as f64);
    for e in &p.entities {
        let EntityKind::Bca(kind) = p.kind(e) else { continue };
        let Some((_, cell)) = theirs.get(&e.name) else { continue };
        let Some(b) = kept.iter().find(|b| b.kind == kind && b.cell == *cell) else { continue };
        let mut terry_dzs: Vec<(String, Vec<Vec<[f64; 2]>>)> = vec![];
        for &c in &e.children {
            let Some(dz) = p.entity(c).filter(|d| matches!(p.kind(d), EntityKind::DeploymentZone)) else { continue };
            let mut regions = vec![];
            for &r in &dz.children {
                let Some(re) = p.entity(r) else { continue };
                let Some((pts, _)) = p.polyline(re) else { continue };
                let o = p.world_position(r);
                regions.push(pts.iter().map(|q| [o[0] + q[0], o[2] + q[1]]).collect());
            }

            terry_dzs.push((dz.name.clone(), regions));
        }

        let ours_dzs: Vec<(String, Vec<Vec<[f64; 2]>>)> = b.zones.iter().map(|z| (z.name.clone(), z.regions.iter()
            .map(|r| r.points.iter().map(|&(x, y)| [(f64::from(x) - cw) * 256.0, (ch - f64::from(y)) * 256.0]).collect()).collect())).collect();
        dz_total += 1;
        dz_counts.0 += terry_dzs.len();
        dz_counts.1 += terry_dzs.iter().map(|d| d.1.len()).sum::<usize>();
        dz_counts.2 += terry_dzs.iter().flat_map(|d| d.1.iter()).map(Vec::len).sum::<usize>();
        let close = |a: &[Vec<[f64; 2]>], b: &[Vec<[f64; 2]>]| a.len() == b.len() && a.iter().zip(b).all(|(x, y)| x.len() == y.len()
            && x.iter().zip(y).all(|(p, q)| (p[0] - q[0]).abs() < 0.05 && (p[1] - q[1]).abs() < 0.05));
        let same = terry_dzs.len() == ours_dzs.len() && terry_dzs.iter().zip(&ours_dzs).all(|(a, b)| a.0 == b.0 && close(&a.1, &b.1));
        if same {
            dz_same += 1;
        } else if dz_shown < 4 {
            dz_shown += 1;
            println!("  {} {kind} {cell:?} deployment zones differ:", e.name);
            for (n, rs) in &terry_dzs {
                println!("    Terry's {n}: {}", rs.iter().map(|r| format!("{r:.1?}")).collect::<Vec<_>>().join(" | "));
            }

            for (n, rs) in &ours_dzs {
                println!("    ours    {n}: {}", rs.iter().map(|r| format!("{r:.1?}")).collect::<Vec<_>>().join(" | "));
            }
        }
    }

    println!("deployment zones: {dz_same} of {dz_total} BCAs the same ({} DZs, {} DZRs, {} points of Terry's compared)", dz_counts.0, dz_counts.1, dz_counts.2);
    let set = |m: &BTreeMap<String, (String, (i32, i32))>| m.values().cloned().collect::<std::collections::BTreeSet<_>>();
    let (a, b) = (set(&mine), set(&theirs));
    println!("BCAs by type and cell: {} common, {} only ours, {} only Terry's", a.intersection(&b).count(), a.difference(&b).count(), b.difference(&a).count());
    let removed: Vec<String> = ours.iter().filter(|b| !b.errors.is_empty()).map(|b| format!("{} {} {:?}", b.name, b.kind, b.cell)).collect();
    println!("with errors: {}: {}", removed.len(), removed.iter().take(12).cloned().collect::<Vec<_>>().join("; "));
    if same != mine.len() || mine.len() != theirs.len() || dz_same != dz_total {
        std::process::exit(1);
    }
}
