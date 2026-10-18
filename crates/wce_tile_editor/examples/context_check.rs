//! Builds the map context of a tile without the app (time, memory, entities), as the Tile panel's Map context does.
//!
//!   cargo run --release -p wce_tile_editor --example context_check -- <game dir> <tile folder> <map name | campaign> [placement] [radius] [far]

use std::path::PathBuf;
use std::sync::Arc;

use wce_tile::battle_map::context::Spot;
use wce_tile::battle_map::tile_db::TileDatabase;
use wce_tile::battle_map::{load_game_map, load_kit_map};
use wce_tile::tile_project::TileProject;
use wce_tile_editor::terrain_bake::TerrainLook;
use wce_tile_editor::tile_document::context::{ContextRequest, build_context};
use wce_vfs::files::Files;
use wce_vfs::{Config, GameProfile, Vfs};

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let (Some(game), Some(tile), Some(map)) = (a.first(), a.get(1), a.get(2)) else {
        eprintln!("context_check <game dir> <tile folder> <map | campaign> [placement] [radius] [far]");
        std::process::exit(2);
    };

    let n = |i: usize, d: i32| a.get(i).and_then(|v| v.parse().ok()).unwrap_or(d);
    let vfs: Arc<dyn Files> = Arc::new(Vfs::mount(&Config::new(PathBuf::from(game), GameProfile::ATTILA)).expect("game files"));
    let project = TileProject::open(&PathBuf::from(tile)).expect("tile");
    let raw = project.raw_root.clone().expect("a kit tile");
    let db = TileDatabase::load(&raw.join("terrain/tiles/battle/_tile_database")).expect("database");
    let t0 = std::time::Instant::now();
    let m = if map == "campaign" {
        let r = |f: &str| vfs.read(&format!("terrain/battles/main_attila_map/{f}"));
        load_game_map(map, &db, &r("tile_map.index").expect("index"), &r("tile_map.tiles").expect("tiles"), r("lf_height_map.dds").ok().as_deref())
    } else {
        load_kit_map(&db, &raw.join("terrain/battles").join(map))
    }
    .expect("map");
    println!("map {} ({:.1} s)", m.name, t0.elapsed().as_secs_f64());

    let found = m.find(&project.info.location, &project.info.variation);
    let spot = match found.get(n(3, 0) as usize) {
        Some(&i) => Spot { x: m.placements[i].x, y: m.placements[i].y, rotation: m.placements[i].rotation },
        None => Spot { x: m.width as i32 / 2, y: m.height as i32 / 2, rotation: 0x10 },
    };

    let rq = ContextRequest {
        map: Arc::new(m), spot, cells: (project.info.cells_x as i32, project.info.cells_y as i32), unit: project.unit_scale, radius: n(4, 8), far: n(5, 48),
        raw_root: raw, look: TerrainLook::default(), trees: true,
    };

    drop(project);
    let built = build_context(&rq, Some(vfs));
    let mut kinds = std::collections::BTreeMap::new();
    for (_, p) in &built.entities {
        *kinds.entry(format!("{:?}", p.kind())).or_insert(0usize) += 1;
    }

    println!("{}\n{kinds:?}", built.report);
    if let Some(pat) = a.iter().position(|v| v == "--model").and_then(|i| a.get(i + 1)) {
        for (w, p) in &built.entities {
            let wce_scene::render_scene::Payload::MeshInstance(mi) = p else { continue };
            if mi.model.contains(pat.as_str()) {
                let s = |r: usize| (w.m[r][0].powi(2) + w.m[r][1].powi(2) + w.m[r][2].powi(2)).sqrt();
                println!("  {} at ({:.0}, {:.1}, {:.0}) scale ~({:.1}, {:.1}, {:.1})", mi.model, w.m[0][3], w.m[1][3], w.m[2][3], s(0), s(1), s(2));
            }
        }
    }

    if a.iter().any(|v| v == "--water") {
        for (w, p) in &built.entities {
            let wce_scene::render_scene::Payload::Water(water) = p else { continue };
            let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
            for v in &water.verts {
                let q = w.transform_point(wce_foundation::math::Float3::new(v.x, v.y, v.z));
                lo = [lo[0].min(q.x), lo[1].min(q.y), lo[2].min(q.z)];
                hi = [hi[0].max(q.x), hi[1].max(q.y), hi[2].max(q.z)];
            }

            println!("  water river={} x {:.0}..{:.0} y {:.0}..{:.0} z {:.0}..{:.0} ({} verts)", water.river, lo[0], hi[0], lo[1], hi[1], lo[2], hi[2], water.verts.len());
        }
    }
    for m in built.missing.iter().take(5) {
        println!("  missing {m}");
    }
}
