//! Prints what rpfm_lib decodes of a rigid model: per LOD-0 mesh the material type, vertex format, pivot, header bounds and the bounds of the decoded vertices.
//!
//!   cargo run --release -p wce_render --example rmv_dump -- <file.rigid_model_v2>

use std::io::Cursor;

use rpfm_lib::files::rigidmodel::RigidModel;
use rpfm_lib::files::{Decodeable, DecodeableExtraData};

fn main() {
    let bytes = std::fs::read(std::env::args().nth(1).expect("a file")).expect("readable");
    let extra = DecodeableExtraData::default();
    let model = RigidModel::decode(&mut Cursor::new(&bytes), &Some(extra)).expect("decodes");
    for (l, lod) in model.lods().iter().enumerate() {
        for block in lod.mesh_blocks() {
            let (mesh, material) = (block.mesh(), block.material());
            let (mut lo, mut hi) = ([1e30f32; 3], [-1e30f32; 3]);
            for v in mesh.vertices() {
                let p = v.position();
                for (a, c) in [p.x, p.y, p.z].into_iter().enumerate() {
                    lo[a] = lo[a].min(c);
                    hi[a] = hi[a].max(c);
                }
            }

            let pv = material.v_pivot();
            println!("lod {l} type {:?} format {:?} verts {} pivot {} {} {} decoded bounds {lo:?} {hi:?}", mesh.material_type(), material.vertex_format(), mesh.vertices().len(), pv.x, pv.y, pv.z);
        }
    }
}
