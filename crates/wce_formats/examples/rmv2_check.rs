//! Gate for the rigid_model_v2 reader (rpfm_lib's RigidModel): parses every .rigid_model_v2 under the given folders, of every version
//! rpfm_lib reads (1, 3, 5, 6, 7, 8), and writes each back: the bytes must be the file's. Counts per version. Port of
//! `cli/rmv2_check.cpp` (which, as the C++ reader, took version 6 only); the editor's model cache draws versions 5 and 6.
//!
//!   cargo run --release -p wce_formats --example rmv2_check -- <dir>...

use std::collections::BTreeMap;
use std::io::Cursor;
use std::path::Path;

use rpfm_lib::files::rigidmodel::RigidModel;
use rpfm_lib::files::rigidmodel::materials::MaterialType;
use rpfm_lib::files::{Decodeable, DecodeableExtraData, Encodeable};

/// The material type of the C++ reader's RS_STANDARD_V5.
const RS_STANDARD_V5: u16 = 68;

#[derive(Default)]
struct Stats {
    files: usize,
    failed: usize,

    /// version -> (read, failed, written back to other bytes)
    by_version: BTreeMap<u32, (usize, usize, usize)>,

    /// shader -> (with geometry, without)
    by_shader: BTreeMap<u16, (usize, usize)>,

    // RS_STANDARD_V5 texture params / alpha modes.
    std_textures: BTreeMap<i32, usize>,
    std_alpha: BTreeMap<i32, usize>,
    errors: BTreeMap<String, usize>,
}

/// The C++ reader read vertices only from mesh blocks with the standard material header (MESH_HEADER_V5): every material type rpfm_lib
/// does not read with a layout of its own (decals, terrain, rivers, cloth, ropes, lights ...).
fn standard_layout(t: MaterialType) -> bool {
    use MaterialType as M;
    !matches!(
        t,
        M::RsTerrain
            | M::BowWave
            | M::DebugGeometry
            | M::RiverOutfield
            | M::RsRiver
            | M::WeightedTextureBlend
            | M::ProjectedDecal
            | M::ProjectedDecalV2
            | M::ProjectedDecalV3
            | M::ProjectedDecalV4
            | M::CustomTerrain
            | M::CollisionShape
            | M::WeightedCloth
            | M::Rope
            | M::TextureCombo
            | M::TextureComboVertexWind
            | M::StaticPointLight
            | M::Uk27
            | M::Uk54
            | M::Uk56
            | M::Uk57
            | M::AlphaBlend
            | M::Cloth
    )
}

fn parse(bytes: &[u8]) -> Result<RigidModel, String> {
    if bytes.get(..4) != Some(b"RMV2") {
        return Err("rmv2: bad signature".to_owned());
    }

    RigidModel::decode(&mut Cursor::new(bytes), &Some(DecodeableExtraData::default())).map_err(|e| format!("rmv2: {e}"))
}

fn walk(dir: &Path, s: &mut Stats) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            walk(&p, s);
            continue;
        }

        if p.extension().is_none_or(|x| x != "rigid_model_v2") {
            continue;
        }

        s.files += 1;
        let bytes = std::fs::read(&p).unwrap_or_default();
        let version = bytes.get(4..8).map_or(0, |b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]));
        let v = s.by_version.entry(version).or_default();
        match parse(&bytes) {
            Ok(mut m) => {
                v.0 += 1;
                let mut back = Vec::new();
                let same = m.encode(&mut back, &None).is_ok() && back == bytes;
                if !same {
                    v.2 += 1;
                    s.failed += 1;
                    *s.errors.entry(format!("rmv2 version {version}: written back to other bytes")).or_default() += 1;
                }

                for lod in m.lods() {
                    for block in lod.mesh_blocks() {
                        let material_type = *block.mesh().material_type();
                        let shader = u16::from(material_type);
                        let geometry = standard_layout(material_type) && !block.mesh().vertices().is_empty();
                        let c = s.by_shader.entry(shader).or_default();
                        if geometry {
                            c.0 += 1;
                        } else {
                            c.1 += 1;
                        }

                        if shader == RS_STANDARD_V5 && geometry {
                            let material = block.material();
                            for t in material.textures() {
                                *s.std_textures.entry(*t.tex_type() as i32).or_default() += 1;
                            }

                            let alpha = material.params_i32().iter().find(|(id, _)| *id == 0).map_or(0, |(_, v)| *v);
                            *s.std_alpha.entry(alpha).or_default() += 1;
                        }
                    }
                }
            }

            Err(e) => {
                v.1 += 1;
                s.failed += 1;
                *s.errors.entry(e).or_default() += 1;
            }
        }
    }
}

fn main() {
    let mut s = Stats::default();
    for dir in std::env::args().skip(1) {
        walk(Path::new(&dir), &mut s);
    }

    println!("{} files, {} failed", s.files, s.failed);
    for (e, n) in &s.errors {
        println!("  {n:5}  {e}");
    }

    for (version, (read, failed, differs)) in &s.by_version {
        println!("version {version}: {read:6} read ({differs} written back to other bytes), {failed} failed");
    }

    for (shader, (with, without)) in &s.by_shader {
        println!("shader {shader:3}: {with:6} meshes with geometry, {without} without");
    }

    let textures = s.std_textures.iter().map(|(t, n)| format!(" {t}:{n}")).collect::<String>();
    let alpha = s.std_alpha.iter().map(|(a, n)| format!(" {a}:{n}")).collect::<String>();
    println!("RS_STANDARD_V5 texture params:{textures}");
    println!("RS_STANDARD_V5 alpha modes:{alpha}");
    std::process::exit(i32::from(s.failed != 0));
}
