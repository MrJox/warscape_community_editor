//! Lists a rigid_model_v2's mesh blocks (LOD 0): material type, name, vertex count, bounds and the material's extra parameters.
//!
//!   cargo run --release -p wce_formats --example rmv2_mats -- <file.rigid_model_v2>

use std::io::Cursor;

use rpfm_lib::files::rigidmodel::RigidModel;
use rpfm_lib::files::{Decodeable, DecodeableExtraData};

fn main() {
    let path = std::env::args().nth(1).expect("usage: rmv2_mats <file>");
    let data = std::fs::read(&path).expect("read");
    let mut extra = DecodeableExtraData::default();
    extra.set_disk_file_path(Some(&path));
    let m = RigidModel::decode(&mut Cursor::new(data), &Some(extra)).expect("decode");
    let Some(lod) = m.lods().first() else { return };
    for (i, block) in lod.mesh_blocks().iter().enumerate() {
        let mesh = block.mesh();
        let v = mesh.vertices();
        let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
        for p in v {
            let q = [p.position().x, p.position().y, p.position().z];
            for k in 0..3 {
                lo[k] = lo[k].min(q[k]);
                hi[k] = hi[k].max(q[k]);
            }
        }

        let tex: Vec<String> = block.material().textures().iter().map(|t| t.path().clone()).collect();
        if v.len() <= 32 {
            for p in v {
                println!("   pos {:?} colour {:?} normal {:?}", p.position(), p.color(), p.normal());
            }
        }

        println!("{i}: {:?} '{}' verts {} lo {:?} hi {:?} tex {:?}", mesh.material_type(), block.material().name(), v.len(), lo, hi, tex);
        let mat = block.material();
        let v4: Vec<(i32, [f32; 4])> = mat.params_vector4df32().iter().map(|(k, v)| (*k, [v.x, v.y, v.z, v.w])).collect();
        println!("   params string {:?} f32 {:?} i32 {:?} vec4 {:?}", mat.params_string(), mat.params_f32(), mat.params_i32(), v4);
    }
}
