//! The conventions the generator copies from the game's buildings, measured: how tangents and bitangents follow the UVs, how many metres
//! a damage texture spans, the collapses' bone names, their `.meta` entries (sounds), the collision meshes' edge adjacency, which damage
//! textures exist, and the rows of a building.
//!
//!   cargo run --release -p wce_destruction --example destruction_calibrate -- <game folder> <schema_att.ron> [building key...]

use std::io::Cursor;
use std::path::{Path, PathBuf};

use rpfm_lib::files::cs2_parsed::Cs2Parsed;
use rpfm_lib::files::rigidmodel::RigidModel;
use rpfm_lib::files::{Decodeable, DecodeableExtraData};
use wce_formats::anim::AnimFile;
use wce_formats::anim_meta::{MetaBody, MetaFile};
use wce_vfs::{Config, GameProfile, Vfs};
use wce_walls::building::BuildingFiles;

fn main() {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    let (Some(game), Some(schema)) = (args.first(), args.get(1)) else {
        eprintln!("usage: destruction_calibrate <game folder> <schema_att.ron> [building key...]");
        std::process::exit(2);
    };

    let vfs = Vfs::mount(&Config::new(PathBuf::from(game), GameProfile::ATTILA)).expect("packs");
    let schema = rpfm_lib::schema::Schema::load(Path::new(schema), None).expect("schema");
    let keys = if args.len() > 2 { args[2..].to_vec() } else { vec!["roman_fort_straight".into(), "barbarian_fort_straight".into(), "roman_fort_gateway_a".into()] };

    let mut thresholds = std::collections::BTreeMap::<String, (usize, String)>::new();
    for path in vfs.list("db/battlefield_buildings_tables/", "") {
        let bytes = vfs.read(path).expect("read");
        let mut extra = DecodeableExtraData::default();
        extra.set_schema(Some(&schema));
        extra.set_table_name(Some("battlefield_buildings_tables"));
        let Ok(db) = rpfm_lib::files::db::DB::decode(&mut Cursor::new(&bytes), &Some(extra)) else { continue };
        let (Some(col), Some(key)) = (db.column_position_by_name("destruct_thresholds"), db.column_position_by_name("key")) else { continue };
        for row in db.data().iter() {
            let e = thresholds.entry(row[col].data_to_string().into_owned()).or_insert((0, row[key].data_to_string().into_owned()));
            e.0 += 1;
        }
    }

    println!("== destruct_thresholds (value: rows, an example): {thresholds:?}");
    println!("== damage textures");
    for t in vfs.list("rigidmodels/buildings/textures/", ".dds") {
        let l = t.to_lowercase();
        if ["damage", "rubble", "splinter", "damaged_soil"].iter().any(|k| l.contains(k)) {
            println!("  {t}");
        }
    }

    for key in &keys {
        let Ok(files) = BuildingFiles::read(&vfs, key) else {
            println!("== {key}: not found");
            continue;
        };

        println!("== {key}: {:?}", files.files.keys().collect::<Vec<_>>());
        for (name, bytes) in &files.files {
            if name.ends_with(".rigid_model_v2") {
                tangents(name, bytes);
            } else if name.ends_with("_anim.anim") {
                if let Some(a) = AnimFile::parse(bytes) {
                    println!("  {name}: bones {:?}", a.bones.iter().take(4).collect::<Vec<_>>());
                }
            } else if name.ends_with(".meta") {
                if let Some(m) = MetaFile::parse(bytes) {
                    for e in m.entries {
                        let what = match &e.body {
                            MetaBody::Effect { effect, scale, .. } => format!("effect {effect} scale {scale:?}"),
                            MetaBody::Volumetric { effect, floats } => format!("volume {effect} {floats:?}"),
                            MetaBody::Sound(raw) => format!("sound {}", raw.iter().map(|b| format!("{b:02x}")).collect::<String>()),
                        };
                        println!("  {name}: {} v{} {}..{} tag {:?} {what}", e.kind, e.version, e.start, e.end, e.tag);
                    }
                }
            } else if name.ends_with(".cs2.parsed") {
                adjacency(bytes);
            }
        }

        let copied = wce_walls::db::copy_rows(&vfs, &schema, &[(key.clone(), format!("{key}_x"))]).expect("rows");
        for (table, db) in &copied.tables {
            let names = db.definition().fields_processed().iter().map(|f| f.name().to_owned()).collect::<Vec<_>>();
            for row in db.data().iter() {
                let cells = names.iter().zip(row.iter()).map(|(n, c)| {
                    let s = c.data_to_string();
                    format!("{n}={}", if s.len() > 60 { format!("<{} chars>", s.len()) } else { s.into_owned() })
                }).collect::<Vec<_>>();
                println!("  row {table}: {}", cells.join(" | "));
            }
        }
    }
}

/// Per block of LOD 0: metres per UV unit (uv, uv2) and how the stored tangent and bitangent follow dP/du and dP/dv.
fn tangents(name: &str, bytes: &[u8]) {
    let m = RigidModel::decode(&mut Cursor::new(bytes), &Some(DecodeableExtraData::default())).expect("rmv2");
    let Some(lod) = m.lods().first() else { return };
    for (i, b) in lod.mesh_blocks().iter().enumerate() {
        let (v, ix) = (b.mesh().vertices(), b.mesh().indices());
        let (mut len_p, mut len_uv, mut len_uv2) = (0.0f64, 0.0f64, 0.0f64);
        let mut votes = [0i64; 5];
        for t in ix.chunks_exact(3) {
            let p = |k: usize| v[t[k] as usize].position().xyz();
            let uv = |k: usize| *v[t[k] as usize].texture_coordinate();
            let uv2 = |k: usize| *v[t[k] as usize].texture_coordinate2();
            let (e1, e2) = (p(1) - p(0), p(2) - p(0));
            let (d1, d2) = (uv(1) - uv(0), uv(2) - uv(0));
            len_p += f64::from(e1.norm() + e2.norm());
            len_uv += f64::from(d1.norm() + d2.norm());
            len_uv2 += f64::from((uv2(1) - uv2(0)).norm() + (uv2(2) - uv2(0)).norm());
            let det = d1.x * d2.y - d2.x * d1.y;
            if det.abs() < 1e-9 {
                continue;
            }

            let dpdu = (e1 * d2.y - e2 * d1.y) / det;
            let dpdv = (e2 * d1.x - e1 * d2.x) / det;
            let vt = v[t[0] as usize].tangent().xyz();
            let vb = v[t[0] as usize].bitangent().xyz();
            let vn = v[t[0] as usize].normal().xyz();
            let s = |x: f32| if x > 0.0 { 1 } else { -1 };
            votes[0] += s(vt.dot(&dpdu));
            votes[1] += s(vb.dot(&dpdv));
            votes[2] += s(vt.dot(&dpdv));
            votes[3] += s(vb.dot(&dpdu));
            votes[4] += s(vn.cross(&vt).dot(&vb));
            let face = e1.cross(&e2);
            votes[4] += 0;
            let _ = face;
        }

        let tex = b.material().textures().first().map(|t| t.path().rsplit('/').next().unwrap_or_default().to_owned()).unwrap_or_default();
        println!("  {name} block {i} {:?} {tex}: {:.2} m/uv, {:.2} m/uv2; t.dPdu {} b.dPdv {} t.dPdv {} b.dPdu {} (n x t).b {}; winding vs normal {}",
            b.mesh().material_type(), len_p / len_uv.max(1e-9), len_p / len_uv2.max(1e-9), votes[0], votes[1], votes[2], votes[3], votes[4], winding(b));
    }
}

/// How the triangles' winding agrees with the stored normals: + for counter-clockwise seen from the normal's side.
fn winding(b: &rpfm_lib::files::rigidmodel::MeshBlock) -> i64 {
    let v = b.mesh().vertices();
    b.mesh().indices().chunks_exact(3).map(|t| {
        let p = |k: usize| v[t[k] as usize].position().xyz();
        let n = (p(1) - p(0)).cross(&(p(2) - p(0)));
        if n.dot(&v[t[0] as usize].normal().xyz()) > 0.0 { 1 } else { -1 }
    }).sum()
}

/// The first level's collision mesh: does `across_face_index_k` name the triangle sharing edge k, and what marks no neighbour.
fn adjacency(bytes: &[u8]) {
    let parsed = Cs2Parsed::decode(&mut Cursor::new(bytes), &None).expect("cs2.parsed");
    let v = serde_json::to_value(&parsed).expect("serde");
    for d in v["pieces"][0]["destructs"].as_array().into_iter().flatten() {
        let tris = d["collision_3d"]["triangles"].as_array().cloned().unwrap_or_default();
        let verts = |t: &serde_json::Value| [t["vertex_1"].as_i64().unwrap_or(0), t["vertex_2"].as_i64().unwrap_or(0), t["vertex_3"].as_i64().unwrap_or(0)];
        let (mut ok, mut bad, mut none) = (0, 0, 0);
        let mut seen_none = std::collections::BTreeSet::new();
        for (i, t) in tris.iter().enumerate() {
            let tv = verts(t);
            for k in 1..=3 {
                let a = t[format!("across_face_index_{k}")].as_i64().unwrap_or(0);
                let (e1, e2) = (t[format!("edge_{k}_vertex_1")].as_i64().unwrap_or(0), t[format!("edge_{k}_vertex_2")].as_i64().unwrap_or(0));
                assert_eq!((e1, e2), (tv[k - 1], tv[k % 3]), "edge {k} of {i}");
                match usize::try_from(a).ok().and_then(|a| tris.get(a)) {
                    Some(o) if verts(o).contains(&e1) && verts(o).contains(&e2) => ok += 1,
                    Some(_) => bad += 1,
                    None => {
                        none += 1;
                        seen_none.insert(a);
                    }
                }
            }
        }

        println!("  {} collision: {} verts {} tris, across ok {ok} wrong {bad} none {none} {:?}; uk_1 {} uk_2 {} zero_4 {}; outlines {:?}", d["name"], d["collision_3d"]["vertices"].as_array().map_or(0, Vec::len), tris.len(), seen_none,
            d["collision_3d"]["uk_1"], d["collision_3d"]["uk_2"], d["collision_3d"]["zero_4"],
            d["collision_outlines"].as_array().into_iter().flatten().map(|o| (o["name"].clone(), o["uk_1"].clone(), o["vertices"]["outline"].as_array().map_or(0, Vec::len))).collect::<Vec<_>>());
        if let Some(t) = tris.first() {
            println!("    first: face {} padding {} zero {} {} {} {}", t["face_index"], t["padding"], t["zero_1"], t["zero_2"], t["zero_3"], t["zero_4"]);
        }

        let platforms = d["platforms"].as_array().cloned().unwrap_or_default();
        println!("    {} platforms, flags {:?}", platforms.len(), platforms.iter().map(|p| (p["flag_1"].as_bool(), p["flag_2"].as_bool(), p["flag_3"].as_bool())).collect::<std::collections::BTreeSet<_>>());
    }
}
