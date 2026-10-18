//! Where a tile's rivers meet its edges (tile frame metres), from what the tile publishes: for checking the tile frame against its links.
//!
//!   cargo run --release -p wce_tile_editor --example water_edges -- <tile folder>

use std::path::PathBuf;

use wce_foundation::math::Float3;
use wce_scene::render_scene::{Payload, RenderScene};
use wce_tile::tile_project::TileProject;
use wce_tile_editor::tile_document::TileDocument;

fn main() {
    let dir = PathBuf::from(std::env::args().nth(1).expect("tile folder"));
    let mut doc = TileDocument::new(TileProject::open(&dir).expect("tile"), None);
    let t = doc.tile_mut();
    let (ex, ez) = (t.extent_x() as f32, t.extent_z() as f32);
    let mut scene = RenderScene::default();
    let layer = scene.add_layer(1, "t");
    wce_editor_core::document::Document::publish(&mut doc, &mut scene, layer);
    doc.tile_mut().update_view(&mut scene, Float3::new(0.0, 0.0, 0.0));
    println!("extent {ex} x {ez}");
    for e in scene.entities() {
        let Payload::Water(w) = &e.payload else { continue };
        if !w.river {
            continue;
        }

        let (mut lo, mut hi) = ([f32::MAX; 2], [f32::MIN; 2]);
        let mut edges = vec![];
        for v in &w.verts {
            let p = e.world.transform_point(Float3::new(v.x, v.y, v.z));
            lo = [lo[0].min(p.x), lo[1].min(p.z)];
            hi = [hi[0].max(p.x), hi[1].max(p.z)];
            if p.x < 20.0 || p.x > ex - 20.0 || p.z < 20.0 || p.z > ez - 20.0 {
                edges.push((p.x.round(), p.z.round()));
            }
        }

        edges.sort_by(|a, b| a.partial_cmp(b).unwrap());
        edges.dedup();
        println!("river: x {:.0}..{:.0}, z {:.0}..{:.0}; near the edges: {:?}", lo[0], hi[0], lo[1], hi[1], edges.iter().step_by((edges.len() / 12).max(1)).collect::<Vec<_>>());
    }
}
