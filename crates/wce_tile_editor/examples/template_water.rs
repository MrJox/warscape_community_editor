//! Exploration: a tile's map-context template (as the Terrain tool opens it): its extent and its river water's bounds in the tile's frame.
//!
//!   cargo run --release -p wce_tile_editor --example template_water -- <tile variation folder>

use wce_scene::render_scene::Payload;
use wce_tile_editor::terrain_bake::TerrainLook;
use wce_tile_editor::tile_document::context::template::load_any_template;

fn main() {
    let dir = std::path::PathBuf::from(std::env::args().nth(1).expect("tile folder"));
    let t = load_any_template(&dir, &TerrainLook::default(), false, None, None).expect("template");
    println!("extent {:?}", t.extent);
    for (w, p) in &t.entities {
        if let Payload::Water(water) = p {
            let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
            for v in &water.verts {
                for (k, c) in [v.x, v.y, v.z].into_iter().enumerate() {
                    lo[k] = lo[k].min(c);
                    hi[k] = hi[k].max(c);
                }
            }

            println!(
                "water river {} verts {}: x {:.1} .. {:.1}, y {:.2} .. {:.2}, z {:.1} .. {:.1}; matrix translation {:.1} {:.1} {:.1}",
                water.river,
                water.verts.len(),
                lo[0],
                hi[0],
                lo[1],
                hi[1],
                lo[2],
                hi[2],
                w.m[0][3],
                w.m[1][3],
                w.m[2][3]
            );
        }
    }
}
