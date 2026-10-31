//! The heights a custom-mesh tile's (cliff's) vertices are drawn at in a game map, as the game's `vs30_custom_geom` gives them: the LF at
//! the vertex's map position (metres, `LF * 3000 - 20`) plus 2 y. Per placement over a window of cells: the plateau's vertices (weight
//! 0) and the faces' (the rest), their lowest, median and highest heights, and the LF under them. `$WCE_LOW`: also every plateau vertex
//! above the skirt (y > 0) drawn below 20 m, where it is. `$WCE_ROWS`: the map cells the plateau (above the skirt) lies over.
//!
//!   cargo run --release -p wce_tile_editor --example cliff_heights -- <game dir> <folder with tile_map.index/.tiles/lf_height_map.dds> <x0> <y0> <x1> <y1>

use std::path::PathBuf;

use wce_formats::cas2::Cas2File;
use wce_tile::battle_map::load_game_map;
use wce_tile::battle_map::tile_db::TileDatabase;
use wce_tile_editor::tile_document::context::custom_mesh::from_cs2;

fn stats(v: &mut [f64]) -> String {
    if v.is_empty() {
        return "-".to_owned();
    }

    v.sort_by(f64::total_cmp);
    format!("{:7.1} {:7.1} {:7.1}", v[0], v[v.len() / 2], v[v.len() - 1])
}

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let w: Vec<i32> = a.iter().skip(2).filter_map(|v| v.parse().ok()).collect();
    let (Some(game), Some(map), &[x0, y0, x1, y1]) = (a.first(), a.get(1), &w[..]) else {
        eprintln!("cliff_heights <game dir> <map folder> <x0> <y0> <x1> <y1>");
        std::process::exit(2);
    };

    let raw = PathBuf::from(game).join("assembly_kit/raw_data");
    let db = TileDatabase::load(&raw.join("terrain/tiles/battle/_tile_database")).expect("database");
    let dir = PathBuf::from(map);
    let r = |n: &str| std::fs::read(dir.join(n)).ok();
    let m = load_game_map("game", &db, &r("tile_map.index").expect("index"), &r("tile_map.tiles").expect("tiles"), r("lf_height_map.dds").as_deref()).expect("map");
    println!("placement                                   at          rot   plateau min / median / max (m)   faces                    LF under the plateau");
    for p in &m.placements {
        let c = p.rect();
        if !(c[0] < x1 && x0 < c[2] && c[1] < y1 && y0 < c[3]) || !p.location.to_lowercase().contains("cliff_custom") {
            continue;
        }

        let folder = wce_tile::sources::tile_dir(&raw, &p.location, &p.variation);
        let Some(cs2) = wce_tile::sources::file_any_case(&folder, wce_tile::sources::CUSTOM_MESH_CS2) else { continue };
        let depth = f64::from(p.h) * 128.0;
        let mesh = from_cs2(&Cas2File::parse(&std::fs::read(&cs2).expect("cs2")).expect("a CS2"), String::new(), 1, depth as f32).expect("a mesh");
        let to_map = p.to_map();
        let (mut top, mut face, mut lf) = (vec![], vec![], vec![]);
        for v in mesh.lods[0].vertices.iter() {
            let (x, y, z) = (f64::from(v.position[0]), f64::from(v.position[1]), depth + f64::from(v.position[2]));
            let (u, w) = to_map.apply(x, z);
            let l = m.lf_metres(u, w);
            if v.normal[3] == 0 {
                if std::env::var_os("WCE_LOW").is_some() && l + 2.0 * y < 20.0 && y > 0.0 {
                    println!("    low plateau vertex: tile ({x:.1}, {:.1}) y {y:.2}, map ({u:.3}, {w:.3}), LF {l:.1}", -f64::from(v.position[2]));
                }

                top.push(l + 2.0 * y);
                lf.push(l);
            } else {
                face.push(l + 2.0 * y);
            }
        }

        if std::env::var_os("WCE_ROWS").is_some() {
            let (mut lo, mut hi) = ((f64::MAX, f64::MAX), (f64::MIN, f64::MIN));
            for v in mesh.lods[0].vertices.iter().filter(|v| v.normal[3] == 0 && v.position[1] > 0.0) {
                let (u, w) = to_map.apply(f64::from(v.position[0]), depth + f64::from(v.position[2]));
                (lo, hi) = ((lo.0.min(u), lo.1.min(w)), (hi.0.max(u), hi.1.max(w)));
            }

            println!("    plateau (above the skirt) over cells x {:.2} .. {:.2}, y {:.2} .. {:.2}", lo.0, hi.0, lo.1, hi.1);
        }

        let name = format!("{}\\{}", p.location.rsplit(['\\', '/']).next().unwrap_or_default(), p.variation);
        println!("{name:40} ({:4}, {:4}) {:#06x}  {}   {}   {}", p.x, p.y, p.rotation, stats(&mut top), stats(&mut face), stats(&mut lf));
    }
}
