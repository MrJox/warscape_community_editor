//! The groups of `.markers` files in their order: per file one line per group, tab separated: the file, the group's position, its first
//! record index, its record count, its key parts. For working out the order of `EMPIREUTILITY::MARKER_TABLE` (docs/formats/tile_compile.md).
//!
//!   cargo run --release -p wce_tile_io --example tile_marker_keys -- <file.markers>...

use std::collections::BTreeMap;
use std::path::Path;

use wce_tile_io::attila::TileBinary;
use wce_tile_io::{TileFiles, TileFormat};
use wce_tile_model::{Blob, Value};

fn main() {
    for path in std::env::args().skip(1) {
        let name = Path::new(&path).file_name().unwrap().to_string_lossy().into_owned();
        let mut folder = BTreeMap::new();
        folder.insert(name, Blob::Bytes(std::fs::read(&path).expect("read").into()));
        let Ok((tile, _)) = TileBinary.read(&TileFiles { folder, ..Default::default() }) else { continue };
        let Some(doc) = tile.documents.first() else { continue };
        let Some(groups) = doc.root.child("groups") else { continue };
        for (i, g) in groups.children.iter().enumerate() {
            let idx = g.get("indices").and_then(Value::as_ints).unwrap_or_default();
            let parts = g.children.iter().map(|k| k.get("value").map_or_else(String::new, |v| match v {
                Value::Str(s) => s.clone(),
                other => format!("{other:?}"),
            })).collect::<Vec<_>>();
            println!("{path}\t{i}\t{}\t{}\t{}", idx.iter().min().copied().unwrap_or(-1), idx.len(), parts.join("\u{1}"));
        }
    }
}
