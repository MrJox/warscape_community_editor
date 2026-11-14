//! Prints what `cloth_sim` builds from a sail model file: its cloths (particles, pinned, links, bones, palette base), free ropes and spheres.
//!
//!   cargo run --release -p wce_render --example cloth_rig_dump -- <sails.rigid_model_v2>

fn main() {
    let path = std::env::args().nth(1).expect("usage: cloth_rig_dump <file.rigid_model_v2>");
    let rig = wce_render::cloth_sim::ClothRig::load(std::fs::read(&path).expect("read"));
    println!("{}", rig.map_or_else(|| "no cloth".to_owned(), |r| r.summary()));
}
