//! A custom-mesh tile's kit mesh (`custom_mesh.cs2`) against BOB's (`custom_mesh.rigid_model_v2`, the packs'): per LOD, every model vertex
//! looked up by its position among the CS2's, and how its texture coordinates and normal relate to that vertex's.
//!
//!   cargo run --release -p wce_render --example custom_mesh_check -- <custom_mesh.cs2> <custom_mesh.rigid_model_v2>
//!
//! `$WCE_SAMPLES` set: the first corners of LOD 0 side by side. rpfm reads the CustomTerrain vertex's uv as two f32 where the game has a
//! half2 (then 4 zero bytes): its uvs and normals here are not the drawn ones (`docs/re/battle_maps.md`, custom-mesh tiles).

use std::collections::HashMap;
use std::io::Cursor;

use rpfm_lib::files::rigidmodel::RigidModel;
use rpfm_lib::files::{Decodeable, DecodeableExtraData};
use wce_formats::cas2::Cas2File;

fn key(p: [f32; 3]) -> [i64; 3] {
    p.map(|c| (f64::from(c) * 1000.0).round() as i64)
}

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let (Some(cs2), Some(rmv)) = (a.first(), a.get(1)) else {
        eprintln!("custom_mesh_check <custom_mesh.cs2> <custom_mesh.rigid_model_v2>");
        std::process::exit(2);
    };

    let kit = Cas2File::parse(&std::fs::read(cs2).expect("the CS2")).expect("a CS2");
    let model = RigidModel::decode(&mut Cursor::new(&std::fs::read(rmv).expect("the model")), &Some(DecodeableExtraData::default())).expect("a model");
    for (l, lod) in model.lods().iter().enumerate() {
        let Some(m) = kit.meshes.get(l) else { break };
        let at: HashMap<[i64; 3], usize> = m.positions.iter().enumerate().map(|(i, p)| (key(*p), i)).collect();
        for block in lod.mesh_blocks() {
            let mesh = block.mesh();
            let (mut found, mut du, mut dv, mut dfu, mut dfv, mut dn, mut uk) = (0, 0f32, 0f32, 0f32, 0f32, 0f32, HashMap::<[u8; 4], usize>::new());
            for v in mesh.vertices() {
                let p = v.position();
                let Some(&i) = at.get(&key([p.x, p.y, p.z])) else { continue };
                found += 1;
                let (t, k) = (v.texture_coordinate(), m.uvs[i].first().copied().unwrap_or_default());
                du = du.max((t.x - k[0]).abs());
                dv = dv.max((t.y - k[1]).abs());
                dfv = dfv.max((t.y - (1.0 - k[1])).abs());
                dfu = dfu.max((t.x - (1.0 - k[0])).abs());
                let (n, c) = (v.normal(), m.normals[i]);
                dn = dn.max((n.x - c[0]).abs().max((n.y - c[1]).abs()).max((n.z - c[2]).abs()));
                let u = v.uk_1();
                *uk.entry([u.x, u.y, u.z, u.w]).or_default() += 1;
            }

            // Triangle by triangle (BOB orders them its own way): the CS2 face with the same three corners, corner by corner.
            let (idx, verts) = (mesh.indices(), mesh.vertices());
            let mut faces: HashMap<[[i64; 3]; 3], usize> = HashMap::new();
            for (t, f) in m.faces.iter().enumerate() {
                let mut c = f.map(|k| key(m.positions[k as usize]));
                c.sort_unstable();
                faces.insert(c, t);
            }

            let (mut same_pos, mut ru, mut rv, mut rfv, mut rn, mut swaps) = (0, 0f32, 0f32, 0f32, 0f32, 0);
            for tri in 0..idx.len() / 3 {
                let corners: Vec<usize> = (0..3).map(|c| usize::from(idx[tri * 3 + c])).collect();
                let pos = |k: usize| { let p = verts[k].position(); key([p.x, p.y, p.z]) };
                let mut c = [pos(corners[0]), pos(corners[1]), pos(corners[2])];
                c.sort_unstable();
                let Some(&ft) = faces.get(&c) else { continue };
                let f = m.faces[ft];
                same_pos += 1;
                // The same winding: corner 0 of the model is some corner of the face, the next ones follow it.
                let first = (0..3).find(|&j| key(m.positions[f[j] as usize]) == pos(corners[0])).unwrap_or(0);
                if key(m.positions[f[(first + 1) % 3] as usize]) != pos(corners[1]) {
                    swaps += 1;
                }

                for (cc, &mv) in corners.iter().enumerate() {
                    let Some(kc) = f.iter().map(|&k| k as usize).find(|&k| key(m.positions[k]) == pos(mv)) else { continue };
                    let _ = cc;
                    let (v, k) = (&verts[mv], m.uvs[kc].first().copied().unwrap_or_default());
                    let t = v.texture_coordinate();
                    if l == 0 && tri < 2 && std::env::var_os("WCE_SAMPLES").is_some() {
                        let (n, g, u) = (v.normal(), v.tangent(), v.uk_1());
                        println!(
                            "    model uv ({:.4}, {:.4}) n ({:.3}, {:.3}, {:.3}, {:.3}) t ({:.3}, {:.3}, {:.3}, {:.3}) uk {:?} | kit uv {:?} n {:?}",
                            t.x, t.y, n.x, n.y, n.z, n.w, g.x, g.y, g.z, g.w, [u.x, u.y, u.z, u.w], m.uvs[kc], m.normals[kc]
                        );
                    }

                    ru = ru.max((t.x - k[0]).abs());
                    rv = rv.max((t.y - k[1]).abs());
                    rfv = rfv.max((t.y - (1.0 - k[1])).abs());
                    let (n, kn) = (v.normal(), m.normals[kc]);
                    rn = rn.max((n.x - kn[0]).abs().max((n.y - kn[1]).abs()).max((n.z - kn[2]).abs()));
                }
            }

            println!("  by triangle: {same_pos} of {} model triangles are CS2 faces ({swaps} wound the other way); uv |du| {ru:.4} |dv| {rv:.4} |1-v| {rfv:.4}; normal {rn:.3}", idx.len() / 3);
            let mut uks: Vec<_> = uk.into_iter().collect();
            uks.sort_by_key(|(_, n)| std::cmp::Reverse(*n));
            uks.truncate(4);
            println!(
                "lod {l} ({}): model {} vertices {} indices, kit {} vertices {} triangles; {found} found by position; uv max |du| {du:.4} |dv| {dv:.4}, \
                 |1-v| {dfv:.4}, |1-u| {dfu:.4}; normal max diff {dn:.3}; uk_1 most {uks:?}",
                m.name, mesh.vertices().len(), mesh.indices().len(), m.positions.len(), m.faces.len()
            );
        }
    }
}
