//! A terrain mesh (`mesh.rigid_model_v2`, `outfield_mesh.rigid_model_v2`) as text: per mesh block of LOD 0 its vertices (`v x y z u v
//! nx ny nz`) and triangles (`t a b c`), for comparing BOB's meshes with ours.
//!
//!   cargo run --release -p wce_tile_io --example terrain_mesh_dump -- <file.rigid_model_v2> > out.txt

use std::io::Cursor;

use rpfm_lib::files::rigidmodel::RigidModel;
use rpfm_lib::files::{Decodeable, DecodeableExtraData};

fn main() {
    let bytes = std::fs::read(std::env::args().nth(1).expect("a file")).expect("readable");
    let model = RigidModel::decode(&mut Cursor::new(&bytes), &Some(DecodeableExtraData::default())).expect("decodes");
    for (b, block) in model.lods()[0].mesh_blocks().iter().enumerate() {
        let mesh = block.mesh();
        println!("block {b} {:?} vertices {} triangles {}", mesh.material_type(), mesh.vertices().len(), mesh.indices().len() / 3);
        for v in mesh.vertices() {
            let (p, t, n) = (v.position(), v.texture_coordinate(), v.normal());
            println!("v {} {} {} {} {} {} {} {}", p.x, p.y, p.z, t.x, t.y, n.x, n.y, n.z);
        }

        for t in mesh.indices().chunks(3) {
            println!("t {} {} {}", t[0], t[1], t[2]);
        }
    }
}
