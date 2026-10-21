//! Streams the map context of a tile without the app (time, memory, entities), as the Tile panel's Map context does: the chunks around the
//! open tile, then (`--walk <cells>`) around a focus moved that far east, step by step.
//!
//!   cargo run --release -p wce_tile_editor --example context_check -- <game dir> <tile folder> <map name | campaign> [placement] [near] [mid]
//!       [far] [--no-cache | --clear-cache] [--walk <cells>] [--model <pattern>] [--water]

use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::Arc;

use wce_foundation::math::{Float3, Float4x4};
use wce_scene::render_scene::Payload;
use wce_tile::battle_map::context::Spot;
use wce_tile::battle_map::tile_db::TileDatabase;
use wce_tile::battle_map::{load_game_map, load_kit_map};
use wce_tile::tile_project::TileProject;
use wce_tile_editor::terrain_bake::TerrainLook;
use wce_tile_editor::tile_document::context::{CHUNK, ChunkChange, ChunkKey, ContextRequest, Streamer, cache};
use wce_vfs::files::Files;
use wce_vfs::{Config, GameProfile, Vfs};

/// The chunks on the "scene", as the panel keeps them.
fn apply(shown: &mut HashMap<ChunkKey, Vec<(Float4x4, Payload)>>, changes: Vec<ChunkChange>) -> (usize, usize) {
    let (mut set, mut removed) = (0, 0);
    for c in changes {
        match c {
            ChunkChange::Set(k, _, e) => {
                shown.insert(k, e);
                set += 1;
            }

            ChunkChange::Remove(k) => {
                shown.remove(&k);
                removed += 1;
            }
        }
    }

    (set, removed)
}

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let (Some(game), Some(tile), Some(map)) = (a.first(), a.get(1), a.get(2)) else {
        eprintln!("context_check <game dir> <tile folder> <map | campaign> [placement] [near] [mid] [far] [--no-cache | --clear-cache] [--walk <cells>]");
        std::process::exit(2);
    };

    let n = |i: usize, d: i32| a.get(i).filter(|v| !v.starts_with("--")).and_then(|v| v.parse().ok()).unwrap_or(d);
    let flag = |f: &str| a.iter().position(|v| v == f).and_then(|i| a.get(i + 1)).cloned();
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

    let dir = cache::cache_dir();
    if a.iter().any(|v| v == "--clear-cache") {
        println!("cache: {} removed", cache::clear(&dir));
    }

    let rq = ContextRequest {
        map: Arc::new(m), spot, cells: (project.info.cells_x as i32, project.info.cells_y as i32), unit: project.unit_scale, near: n(4, 8), mid: n(5, 24),
        far: n(6, 64), raw_root: raw, look: TerrainLook::default(), trees: true, plain: None, cache: (!a.iter().any(|v| v == "--no-cache")).then_some(dir),
    };

    drop(project);
    let t1 = std::time::Instant::now();
    let mut s = Streamer::new(rq, Some(vfs));
    let mut shown = HashMap::new();
    let focus = s.open_centre();
    apply(&mut shown, s.run_to_idle(focus));
    println!("first: {:.1} s\n{}", t1.elapsed().as_secs_f64(), s.report());

    // The LF map against the map's own LF, at the open tile's corners and centre and a chunk away: the same heights (scene units).
    if let Some(lf) = &s.frame().lf {
        let f = s.frame();
        println!("LF map {} x {} from ({:.0}, {:.0}) every {}, normals {} x {}", lf.nx, lf.nz, lf.x0, lf.z0, lf.step, lf.normal.w, lf.normal.h);
        let o = f.open;
        for (cx, cy) in [(o[0], o[1]), (o[2], o[1]), (o[0], o[3]), (o[2], o[3]), ((o[0] + o[2]) / 2, (o[1] + o[3]) / 2), (o[0] - CHUNK, o[1]), (o[2] + CHUNK, o[3])] {
            let (x, z) = f.from_map.apply(f64::from(cx) + 0.3, f64::from(cy) + 0.6);
            let want = (f.map.lf_metres(f64::from(cx) + 0.3, f64::from(cy) + 0.6) - f.lf_mean) / f.unit;
            println!("  cell ({cx}, {cy}) at ({x:.0}, {z:.0}): {:.2} (map {want:.2})", lf.height_at(x as f32, z as f32));
        }
    }
    let mut kinds = BTreeMap::new();
    for (_, p) in shown.values().flatten() {
        *kinds.entry(format!("{:?}", p.kind())).or_insert(0usize) += 1;
    }

    println!("{kinds:?}");

    // Walking east a chunk at a time: what each step builds.
    if let Some(cells) = flag("--walk").and_then(|v| v.parse::<i32>().ok()) {
        let f = s.frame().clone();
        let (cx, cy) = f.cell_of(focus.0, focus.1);
        let mut step = 0;
        while step < cells {
            step = (step + CHUNK).min(cells);
            let to = f.from_map.apply(cx + f64::from(step), cy);
            let t = std::time::Instant::now();
            let (set, removed) = apply(&mut shown, s.run_to_idle(to));
            let n: usize = shown.values().map(Vec::len).sum();
            println!("  +{step} cells: {set} chunks set, {removed} removed in {:.2} s, {n} entities", t.elapsed().as_secs_f64());
        }

        println!("{}", s.report());
    }

    if let Some(pat) = flag("--model") {
        for (w, p) in shown.values().flatten() {
            let Payload::MeshInstance(mi) = p else { continue };
            if mi.model.contains(pat.as_str()) {
                let sc = |r: usize| (w.m[r][0].powi(2) + w.m[r][1].powi(2) + w.m[r][2].powi(2)).sqrt();
                println!("  {} at ({:.0}, {:.1}, {:.0}) scale ~({:.1}, {:.1}, {:.1})", mi.model, w.m[0][3], w.m[1][3], w.m[2][3], sc(0), sc(1), sc(2));
            }
        }
    }

    if a.iter().any(|v| v == "--water") {
        for (w, p) in shown.values().flatten() {
            let Payload::Water(water) = p else { continue };
            let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
            for v in &water.verts {
                let q = w.transform_point(Float3::new(v.x, v.y, v.z));
                lo = [lo[0].min(q.x), lo[1].min(q.y), lo[2].min(q.z)];
                hi = [hi[0].max(q.x), hi[1].max(q.y), hi[2].max(q.z)];
            }

            println!("  water river={} x {:.0}..{:.0} y {:.0}..{:.0} z {:.0}..{:.0} ({} verts)", water.river, lo[0], hi[0], lo[1], hi[1], lo[2], hi[2], water.verts.len());
        }
    }

    for m in s.failed() {
        println!("  not opened: {m}");
    }
}
