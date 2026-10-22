//! The values of the scalar fields of `.markers` records, per class and version and field: how often each value occurs (the 8 most
//! frequent). For working out what the export writes in the fields not understood yet (docs/re/ted_export.md).
//!
//!   cargo run --release -p wce_tile_io --example tile_marker_fields -- <file.markers>...

use std::collections::BTreeMap;
use std::path::Path;

use wce_tile_io::attila::TileBinary;
use wce_tile_io::{TileFiles, TileFormat};
use wce_tile_model::{Blob, Value};

fn main() {
    let mut tally: BTreeMap<(String, i64, String), BTreeMap<String, usize>> = BTreeMap::new();
    for path in std::env::args().skip(1) {
        let name = Path::new(&path).file_name().unwrap().to_string_lossy().into_owned();
        let mut folder = BTreeMap::new();
        folder.insert(name, Blob::Bytes(std::fs::read(&path).expect("read").into()));
        let Ok((tile, _)) = TileBinary.read(&TileFiles { folder, ..Default::default() }) else { continue };
        let Some(doc) = tile.documents.first() else { continue };
        let version = doc.root.get("version").and_then(Value::as_int).unwrap_or(-1);
        // The key of each record (from the groups), for crossing a field with it.
        let mut key_of = BTreeMap::new();
        if let Some(gs) = doc.root.child("groups") {
            for g in &gs.children {
                let k = g.children.first().and_then(|k| k.get("value")).map(|v| format!("{v:?}")).unwrap_or_default();
                for &i in g.get("indices").and_then(Value::as_ints).unwrap_or_default() {
                    key_of.insert(i as usize, k.clone());
                }
            }
        }

        for (ri, rec) in doc.root.children.iter().filter(|c| c.class == "record").enumerate() {
            if let Ok(cross) = std::env::var("WCE_CROSS")
                && let Some(v) = rec.get(&cross)
            {
                let k = key_of.get(&ri).cloned().unwrap_or_default();
                let short = k.rsplit('\\').next().unwrap_or(&k).to_owned();
                *tally.entry((format!("{} cross", doc.root.class), version, format!("{cross}={v:?} is_decal={:?}", rec.get("is_decal")))).or_default().entry(short).or_default() += 1;
            }

            for p in rec.props() {
                let v = match &p.value {
                    Value::Floats(_) | Value::Ints(_) | Value::Bytes(_) => continue,
                    other => format!("{other:?}"),
                };
                *tally.entry((doc.root.class.to_string(), version, p.name.to_string())).or_default().entry(v).or_default() += 1;
            }
        }
    }

    for ((class, version, field), values) in &tally {
        let mut v = values.iter().collect::<Vec<_>>();
        v.sort_by(|a, b| b.1.cmp(a.1));
        println!("{class} v{version} {field}: {}", v.iter().take(8).map(|(x, n)| format!("{x} ({n})")).collect::<Vec<_>>().join(", "));
    }
}
