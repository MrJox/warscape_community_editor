//! The vertices of a terrain mesh (`mesh.rigid_model_v2` of an exported tile), for decoding the tile's heights.
//!
//!   cargo run --release -p wce_tile_io --example tile_mesh_dump -- <mesh.rigid_model_v2> [lod]
//!
//! Prints the model's LODs and blocks, then one line per vertex of the LOD (default 0): block, x, y, z.

use std::io::Cursor;

use rpfm_lib::files::rigidmodel::RigidModel;
use rpfm_lib::files::{Decodeable, DecodeableExtraData};

fn main() {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    let Some(path) = args.first() else {
        eprintln!("usage: tile_mesh_dump <mesh.rigid_model_v2> [lod]");
        std::process::exit(2);
    };

    let lod = args.get(1).and_then(|l| l.parse().ok()).unwrap_or(0usize);
    let bytes = std::fs::read(path).expect("read");
    let model = RigidModel::decode(&mut Cursor::new(&bytes), &Some(DecodeableExtraData::default())).expect("rigid model");
    eprintln!("version {}, {} lods", model.version(), model.lods().len());
    for (i, l) in model.lods().iter().enumerate() {
        let blocks = l.mesh_blocks();
        eprintln!(
            "lod {i}: {} blocks, {} vertices, {} indices; materials {:?}",
            blocks.len(),
            blocks.iter().map(|b| b.mesh().vertices().len()).sum::<usize>(),
            blocks.iter().map(|b| b.mesh().indices().len()).sum::<usize>(),
            blocks.iter().map(|b| format!("{:?}/{:?}", b.mesh().material_type(), b.material().vertex_format())).collect::<Vec<_>>(),
        );
    }

    for (b, block) in model.lods()[lod].mesh_blocks().iter().enumerate() {
        for v in block.mesh().vertices() {
            let p = v.position();
            println!("{b}\t{}\t{}\t{}", p.x, p.y, p.z);
        }
    }
}
