//! A CS2's meshes and scene nodes, as `wce_formats::cas2` reads them.
//!
//!   cargo run --release -p wce_formats --example cas2_dump -- <file.cs2> [...]

use wce_formats::cas2::Cas2File;

fn main() {
    for path in std::env::args().skip(1) {
        match Cas2File::parse(&std::fs::read(&path).expect("read")) {
            Err(e) => println!("{path}: {e}"),
            Ok(f) => {
                println!("{path}: version {}, {} meshes, {} nodes", f.version, f.meshes.len(), f.nodes.len());
                for m in &f.meshes {
                    let class = m.strings.iter().find(|(k, _)| k == "class_rigidINFO").map(|(_, v)| v.join(","));
                    println!("  mesh {} ({class:?}): {} vertices, {} faces", m.name, m.positions.len(), m.faces.len());
                }
            }
        }
    }
}
