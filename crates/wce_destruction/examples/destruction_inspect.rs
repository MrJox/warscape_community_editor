//! What a building folder holds, for comparing generated buildings with the game's: per model its skeleton, LODs and blocks (material,
//! bone, pivot, vertices, textures), per animation its bones and frames, per `.meta` its entries, and the logic file as JSON (long arrays
//! cut short).
//!
//!   cargo run --release -p wce_destruction --example destruction_inspect -- <game folder> <building key> [--mods a.pack;b.pack] [--full]

use std::io::Cursor;
use std::path::PathBuf;

use rpfm_lib::files::cs2_parsed::Cs2Parsed;
use rpfm_lib::files::rigidmodel::RigidModel;
use rpfm_lib::files::{Decodeable, DecodeableExtraData};
use serde_json::Value;
use wce_vfs::{Config, GameProfile, Vfs};
use wce_walls::building::BuildingFiles;

fn main() {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    let (Some(game), Some(key)) = (args.first(), args.get(1)) else {
        eprintln!("usage: destruction_inspect <game folder> <building key> [--mods a.pack;b.pack] [--full]");
        std::process::exit(2);
    };

    let full = args.iter().any(|a| a == "--full");
    let mut config = Config::new(PathBuf::from(game), GameProfile::ATTILA);
    config.mod_packs = args.iter().position(|a| a == "--mods").and_then(|i| args.get(i + 1)).map(|m| m.split(';').filter(|s| !s.is_empty()).map(str::to_owned).collect());
    let vfs = Vfs::mount(&config).expect("packs");
    let files = BuildingFiles::read(&vfs, key).expect("building");
    for (name, bytes) in &files.files {
        println!("== {name} ({} bytes)", bytes.len());
        if name.ends_with(".rigid_model_v2") {
            model(bytes);
        } else if name.ends_with(".anim") {
            anim(bytes, full);
        } else if name.ends_with(".meta") {
            meta(bytes);
        } else if name.ends_with(".cs2.parsed") {
            let parsed = Cs2Parsed::decode(&mut Cursor::new(bytes), &None).expect("cs2.parsed");
            let mut v = serde_json::to_value(&parsed).expect("serde");
            if !full {
                shorten(&mut v);
            }

            println!("{}", serde_json::to_string_pretty(&v).unwrap_or_default());
        } else if name.ends_with(".xml") {
            println!("{}", String::from_utf8_lossy(bytes));
        }
    }
}

fn model(bytes: &[u8]) {
    let m = RigidModel::decode(&mut Cursor::new(bytes), &Some(DecodeableExtraData::default())).expect("rmv2");
    println!("  version {} skeleton '{}' uk_1 {} lods {}", m.version(), m.skeleton_id(), m.uk_1(), m.lods().len());
    for (l, lod) in m.lods().iter().enumerate() {
        println!("  lod {l}: distance {} authored {} quality {} blocks {}", lod.visibility_distance(), lod.authored_lod_number(), lod.quality_level(), lod.mesh_blocks().len());
        if l > 0 {
            continue;
        }

        for b in lod.mesh_blocks() {
            let (mesh, mat) = (b.mesh(), b.material());
            let p = mat.v_pivot();
            println!("    '{}' {:?}/{:?} bone {} parent {} pivot ({:.3} {:.3} {:.3}) verts {} tris {} light '{}' shader {} bytes, mat '{}' dir '{}' filters '{}' uk {:?}",
                mesh.name(), mesh.material_type(), mat.vertex_format(), mat.i_matrix_index(), mat.i_parent_matrix_index(), p.x, p.y, p.z,
                mesh.vertices().len(), mesh.indices().len() / 3, mesh.lighting_constants(), mesh.shader_params().data().len(), mat.name(),
                mat.texture_directory(), mat.filters(), (mat.uk_1(), mat.uk_2(), mat.uk_3(), mat.uk_4(), mat.uk_5(), mat.uk_6()));
            for t in mat.textures() {
                println!("      tex {:?} {}", t.tex_type(), t.path());
            }

            if !mat.params_string().is_empty() || !mat.params_f32().is_empty() || !mat.params_i32().is_empty() || !mat.params_vector4df32().is_empty() {
                println!("      params s {:?} f {:?} i {:?} v {:?}", mat.params_string(), mat.params_f32(), mat.params_i32(), mat.params_vector4df32());
            }

            if let Some(v) = mesh.vertices().first() {
                println!("      v0 pos {:?} uv {:?} uv2 {:?} n {:?} t {:?} b {:?} c {:?} uk1 {:?}", v.position().as_slice(), v.texture_coordinate().as_slice(),
                    v.texture_coordinate2().as_slice(), v.normal().as_slice(), v.tangent().as_slice(), v.bitangent().as_slice(), v.color().as_slice(), v.uk_1().as_slice());
            }
        }
    }
}

fn anim(bytes: &[u8], full: bool) {
    let Some(a) = wce_formats::anim::Animation::decode(bytes, usize::MAX) else {
        println!("  (not read)");
        return;
    };

    println!("  fps {} end {} bones {} frames {}", a.fps, a.end, a.parents.len(), a.frames.len());
    let skel = String::from_utf8_lossy(&bytes[14..14 + u16::from_le_bytes([bytes[12], bytes[13]]) as usize]).into_owned();
    println!("  skeleton '{skel}' parents {:?}", a.parents);
    let last = a.frames.last().expect("frames");
    for (i, ((t0, q0), (t1, q1))) in a.frames[0].iter().zip(last).enumerate() {
        if full || i < 6 {
            println!("    bone {i}: first t {t0:?} q {q0:?} -> last t {t1:?} q {q1:?}");
        }
    }
}

fn meta(bytes: &[u8]) {
    match wce_formats::anim_meta::meta_effects(bytes) {
        Some(e) => {
            for x in e {
                println!("  {:?} '{}' {}..{} pos {:?} rot {:?} vol {:?} scale {}", x.tag, x.effect, x.start, x.end, x.position, x.rotation, x.volume, x.scale);
            }
        }

        None => println!("  (not read)"),
    }
}

fn shorten(v: &mut Value) {
    match v {
        Value::Array(a) => {
            if a.len() > 3 && a.iter().all(|x| x.is_number()) {
                let n = a.len();
                a.truncate(3);
                a.push(Value::String(format!("... {n} numbers")));
            } else if a.len() > 4 {
                let n = a.len();
                a.truncate(2);
                for x in a.iter_mut() {
                    shorten(x);
                }

                a.push(Value::String(format!("... {n} items")));
            } else {
                for x in a.iter_mut() {
                    shorten(x);
                }
            }
        }

        Value::Object(o) => {
            for (_, x) in o.iter_mut() {
                shorten(x);
            }
        }

        _ => {}
    }
}
