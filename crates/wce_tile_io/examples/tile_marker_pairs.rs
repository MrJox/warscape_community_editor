//! A markers file of a pack tile against the kit tile.ted's instances of a class: each record's group key and matrix, the nearest kit
//! instance of that key (world transform, scale), to find how markers map to TEd instances (docs/formats/tile_decompile.md).
//!
//!   cargo run --release -p wce_tile_io --example tile_marker_pairs -- <markers file> <kit tile folder> <TEd class> [records]

use std::collections::BTreeMap;
use std::path::Path;

use wce_tile::tile_project::TileProject;
use wce_tile::world_transform::world_transform;
use wce_tile_io::attila::TileBinary;
use wce_tile_io::{TileFiles, TileFormat};
use wce_tile_model::{Blob, Value};

fn main() {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    let (Some(markers), Some(kit), Some(class)) = (args.first(), args.get(1), args.get(2)) else {
        eprintln!("usage: tile_marker_pairs <markers file> <kit tile folder> <TEd class> [records]");
        std::process::exit(2);
    };

    let n = args.get(3).and_then(|n| n.parse().ok()).unwrap_or(4usize);
    let name = Path::new(markers).file_name().unwrap().to_string_lossy().into_owned();
    let mut folder = BTreeMap::new();
    folder.insert(name.clone(), Blob::Bytes(std::fs::read(markers).expect("read").into()));
    let (tile, _) = TileBinary.read(&TileFiles { folder, ..Default::default() }).expect("read");
    let root = &tile.documents[0].root;
    let mut key_of = BTreeMap::new();
    if let Some(gs) = root.child("groups") {
        for g in &gs.children {
            let key = g.children.first().and_then(|k| k.get("value")).map(|v| format!("{v:?}")).unwrap_or_default();
            for &i in g.get("indices").and_then(Value::as_ints).unwrap_or_default() {
                key_of.insert(i as usize, key.clone());
            }
        }
    }

    let project = TileProject::open(Path::new(kit)).expect("tile");
    let sc = &project.scene;
    let extent_z = project.extent_z();
    let inst = (0..sc.doc().elements().len()).filter(|&e| sc.type_of(e) == class.as_str()).collect::<Vec<_>>();
    println!("{} records, {} {class} in tile.ted, extent z {extent_z}", root.children.iter().filter(|c| c.class == "record").count(), inst.len());
    for (i, rec) in root.children.iter().filter(|c| c.class == "record").enumerate().take(n) {
        let m = rec.get("transform").and_then(Value::as_floats).map(<[f64]>::to_vec).or_else(|| {
            Some(vec![rec.get("x")?.as_f64()?, rec.get("y")?.as_f64()?, rec.get("z")?.as_f64()?])
        });

        let m = m.unwrap_or_default();
        let (tx, ty, tz) = if m.len() == 16 { (m[12], m[13], m[14]) } else { (m[0], m[1], m[2]) };
        println!("record {i}: key {} matrix {:?}", key_of.get(&i).cloned().unwrap_or_default(), m.iter().map(|x| format!("{x:.4}")).collect::<Vec<_>>());
        let other = rec.props().iter().filter(|p| p.name != "transform").map(|p| format!("{}={:?}", p.name, p.value)).collect::<Vec<_>>();
        println!("  fields {}", other.join(" "));
        // Nearest kit instance of the class to the building frame's guess (scene x = 2 tx, z = 2 (extent - tz)).
        let guess = (2.0 * tx, ty, 2.0 * (extent_z - tz));
        let best = inst
            .iter()
            .map(|&e| (e, world_transform(sc, e)))
            .min_by(|a, b| {
                let d = |w: &wce_tile::world_transform::WorldTransform| (w.position.x - guess.0).hypot(w.position.z - guess.2);
                d(&a.1).total_cmp(&d(&b.1))
            });

        if let Some((e, w)) = best {
            println!(
                "  nearest {class} {e}: key {:?} world pos ({:.4}, {:.4}, {:.4}) rot {:?} scale {:?} orientation {:?} ({})",
                sc.doc().get(e, "key").or_else(|| sc.doc().get(e, "vfx")),
                w.position.x, w.position.y, w.position.z,
                w.rotation.m.iter().map(|r| r.map(|x| format!("{x:.4}"))).collect::<Vec<_>>(),
                sc.doc().get(e, "scale"),
                sc.doc().get(e, "orientation"),
                sc.layer_path(e)
            );
        }
    }
}
