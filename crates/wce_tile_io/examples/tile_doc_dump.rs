//! A file of an exported tile as the model document `attila.tile_binary` reads it: the tree, the first values of each node.
//!
//!   cargo run --release -p wce_tile_io --example tile_doc_dump -- <file> [max children per node] [max depth]

use std::collections::BTreeMap;

use wce_tile_io::attila::TileBinary;
use wce_tile_io::{TileFiles, TileFormat};
use wce_tile_model::{Blob, Node};

fn show(n: &Node, depth: usize, max: usize, max_depth: usize) {
    let pad = "  ".repeat(depth);
    let props = n.props().iter().map(|p| format!("{}={}", p.name, format!("{:?}", p.value).chars().take(80).collect::<String>())).collect::<Vec<_>>().join(" ");
    println!("{pad}{} ({} children) {}", n.class, n.children.len(), props.chars().take(3000).collect::<String>());
    if depth >= max_depth {
        return;
    }

    for c in n.children.iter().take(max) {
        show(c, depth + 1, max, max_depth);
    }
}

fn main() {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    let path = std::path::Path::new(args.first().expect("usage: tile_doc_dump <file> [max children] [max depth]"));
    let max = args.get(1).and_then(|m| m.parse().ok()).unwrap_or(3);
    let max_depth = args.get(2).and_then(|m| m.parse().ok()).unwrap_or(8);
    let name = path.file_name().unwrap().to_string_lossy().into_owned();
    let mut folder = BTreeMap::new();
    folder.insert(name.clone(), Blob::Bytes(std::fs::read(path).expect("read").into()));
    let (tile, report) = TileBinary.read(&TileFiles { folder, ..Default::default() }).expect("read");
    println!("{:?}", report.parts);
    for d in &tile.documents {
        show(&d.root, 0, max, max_depth);
    }
}
