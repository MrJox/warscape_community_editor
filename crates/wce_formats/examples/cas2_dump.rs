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
                    let range = |v: &mut dyn Iterator<Item = [f32; 3]>| {
                        v.fold(([f32::MAX; 3], [f32::MIN; 3]), |(lo, hi), p| (std::array::from_fn(|k| lo[k].min(p[k])), std::array::from_fn(|k| hi[k].max(p[k]))))
                    };
                    println!("    positions {:?}", range(&mut m.positions.iter().copied()));
                    println!("    groups (triangles, material) {:?}, uv channels {:?}", m.groups, m.uvs.first().map(Vec::len));
                    let mut alphas: Vec<String> = m.colours.iter().map(|c| format!("{c:?}")).collect();
                    alphas.sort();
                    alphas.dedup();
                    println!("    {} distinct colours, e.g. {:?}", alphas.len(), &alphas[..alphas.len().min(6)]);
                    if m.uvs.first().is_some_and(|c| !c.is_empty()) {
                        println!("    uv0 {:?}", range(&mut m.uvs.iter().filter_map(|c| c.first().copied())));
                    }
                }
            }
        }
    }
}
