//! The bounding box of rigid models' first LOD (all its meshes), from the game's packs: the size a model is drawn at before any scale.
//!
//!   cargo run --release -p wce_terrain_io --example model_bounds -- <game dir> <model path or prefix>...
//!
//! A path ending in `/` lists every `.rigid_model_v2` under it.

use std::io::Cursor;

use rpfm_lib::files::Decodeable;
use rpfm_lib::files::rigidmodel::RigidModel;
use wce_vfs::{Config, GameProfile, Vfs};

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let mut cfg = Config::new(&a[0], GameProfile::from_env().clone());
    cfg.mod_packs = Some(vec![]);
    let vfs = Vfs::mount(&cfg).expect("vfs");
    for arg in &a[1..] {
        let paths: Vec<String> = if arg.ends_with('/') {
            vfs.list(arg, "").iter().filter(|p| p.ends_with(".rigid_model_v2")).map(|p| (*p).to_owned()).collect()
        } else {
            vec![arg.clone()]
        };

        for p in paths {
            let Ok(bytes) = vfs.read(&p) else {
                println!("{p}: not in the packs");
                continue;
            };

            let model = match RigidModel::decode(&mut Cursor::new(bytes.to_vec()), &None) {
                Ok(m) => m,
                Err(e) => {
                    println!("{p}: {e}");
                    continue;
                }
            };

            let Some(lod) = model.lods().first() else { continue };
            let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
            for b in lod.mesh_blocks() {
                let (mn, mx) = (b.mesh().min_bb(), b.mesh().max_bb());
                for (k, (l, h)) in [(mn.x, mx.x), (mn.y, mx.y), (mn.z, mx.z)].into_iter().enumerate() {
                    lo[k] = lo[k].min(l);
                    hi[k] = hi[k].max(h);
                }
            }

            let mut per: std::collections::BTreeMap<String, ([f32; 3], [f32; 3])> = std::collections::BTreeMap::new();
            for b in lod.mesh_blocks() {
                let (mn, mx) = (b.mesh().min_bb(), b.mesh().max_bb());
                let e = per.entry(format!("{:?}", b.mesh().material_type())).or_insert(([f32::MAX; 3], [f32::MIN; 3]));
                for (k, (l, h)) in [(mn.x, mx.x), (mn.y, mx.y), (mn.z, mx.z)].into_iter().enumerate() {
                    e.0[k] = e.0[k].min(l);
                    e.1[k] = e.1[k].max(h);
                }
            }

            for (k, (l, h)) in &per {
                println!(
                    "    {k}: size {:.2} x {:.2} x {:.2} (x {:.2}..{:.2}, z {:.2}..{:.2})",
                    h[0] - l[0],
                    h[1] - l[1],
                    h[2] - l[2],
                    l[0],
                    h[0],
                    l[2],
                    h[2]
                );
            }

            let kinds: std::collections::BTreeSet<String> =
                lod.mesh_blocks().iter().map(|b| format!("{:?}", b.mesh().material_type())).collect();
            println!(
                "{p}: x {:.2}..{:.2} y {:.2}..{:.2} z {:.2}..{:.2} (size {:.2} x {:.2} x {:.2}), {} meshes {kinds:?}",
                lo[0],
                hi[0],
                lo[1],
                hi[1],
                lo[2],
                hi[2],
                hi[0] - lo[0],
                hi[1] - lo[1],
                hi[2] - lo[2],
                lod.mesh_blocks().len()
            );
        }
    }
}
