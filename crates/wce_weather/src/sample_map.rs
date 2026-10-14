//! Sample map: randomly picked building models on a flat ground, so that environments can be judged on something with shape. This is the
//! weather tool's default view; it publishes into the shared `RenderScene` like any document.
//!
//! A building is a folder of pieces; `<name>_pieceNN_destruct01` is each piece's intact state. Piece placement nodes in
//! `<name>_tech.cs2.parsed` are identity in every file checked, so the pieces are drawn at the origin of the building (docs/re/renderer.md).

use std::collections::BTreeMap;
use std::io::Cursor;

use rpfm_lib::files::rigidmodel::RigidModel;
use rpfm_lib::files::rigidmodel::materials::MaterialType;
use rpfm_lib::files::rigidmodel::vertices::VertexFormat;
use rpfm_lib::files::{Decodeable, DecodeableExtraData};
use wce_foundation::math::{Float3, Float4x4, translation};
use wce_foundation::std_random::{Mt19937, shuffle};
use wce_scene::render_scene::{DocumentId, LayerId, MeshInstance, Payload, RenderScene, SourceRef, Terrain};
use wce_vfs::files::Files;

pub const BUILDINGS: &str = "rigidmodels/buildings/";

#[derive(Clone, Debug, Default, PartialEq)]
pub struct SampleMapResult {
    pub buildings: usize,

    /// Footprint of the grid, tallest building as y.
    pub bounds_min: Float3,
    pub bounds_max: Float3,

    /// Grid centre (x, 0, z) of building i + 1, where its pieces were placed.
    pub centres: Vec<Float3>,
    pub errors: Vec<String>,
}

struct PieceInfo {
    path: String,
    bmin: Float3,
    bmax: Float3,
}

fn min3(a: Float3, b: Float3) -> Float3 {
    Float3::new(a.x.min(b.x), a.y.min(b.y), a.z.min(b.z))
}

fn max3(a: Float3, b: Float3) -> Float3 {
    Float3::new(a.x.max(b.x), a.y.max(b.y), a.z.max(b.z))
}

const EMPTY_MIN: Float3 = Float3 { x: 1e30, y: 1e30, z: 1e30 };
const EMPTY_MAX: Float3 = Float3 { x: -1e30, y: -1e30, z: -1e30 };

/// A piece is usable when the renderer can draw it (the version the model cache reads): standard meshes of the static vertex format in LOD 0. Its bounds are the file's mesh boxes.
fn piece_info(files: &dyn Files, path: &str, errors: &mut Vec<String>) -> Option<PieceInfo> {
    let model = files.read(path).and_then(|bytes| RigidModel::decode(&mut Cursor::new(&bytes), &Some(DecodeableExtraData::default())).map_err(|error| error.to_string()));
    let model = match model {
        Ok(model) if *model.version() != 6 => {
            errors.push(format!("{path}: rmv2: version {} (expected 6)", model.version()));
            return None;
        }

        Ok(model) if !model.lods().is_empty() => model,
        Ok(_) => {
            errors.push(format!("{path}: no LODs"));
            return None;
        }

        Err(error) => {
            errors.push(format!("{path}: {error}"));
            return None;
        }
    };

    let mut out = PieceInfo { path: path.to_owned(), bmin: EMPTY_MIN, bmax: EMPTY_MAX };
    let mut any = false;
    for block in model.lods()[0].mesh_blocks() {
        let (mesh, material) = (block.mesh(), block.material());
        if *mesh.material_type() != MaterialType::DefaultMaterial || *material.vertex_format() != VertexFormat::Static || mesh.vertices().is_empty() || mesh.indices().is_empty() {
            continue;
        }

        let (lo, hi) = (mesh.min_bb(), mesh.max_bb());
        out.bmin = min3(out.bmin, Float3::new(lo.x, lo.y, lo.z));
        out.bmax = max3(out.bmax, Float3::new(hi.x, hi.y, hi.z));
        any = true;
    }

    any.then_some(out)
}

/// Fills `layer` (cleared first) with the ground and up to `count` houses chosen by `seed` from the building folders under `prefix`; returns
/// what it placed.
pub fn publish_sample_map(files: &dyn Files, scene: &mut RenderScene, layer: LayerId, doc: DocumentId, seed: u32, count: usize, prefix: &str) -> SampleMapResult {
    let mut res = SampleMapResult::default();
    scene.clear_layer(layer);
    let ground = Terrain::flat(1000.0, "terrain/textures/battle/default/grass/0_su_diffuse.dds", 8.0);
    scene.add(layer, Float4x4::IDENTITY, Payload::Terrain(ground), SourceRef { doc, item: 0 });

    let mut folders = BTreeMap::<String, Vec<String>>::new();
    for p in files.list(prefix, ".rigid_model_v2") {
        let slash = p.rfind('/').unwrap_or(0);
        let file = &p[slash + 1..];
        if file.contains("destruct") && !file.contains("destruct01") {
            continue;
        }

        folders.entry(p[..slash].to_owned()).or_default().push(p.clone());
    }

    let mut names = folders.keys().cloned().collect::<Vec<_>>();
    if names.is_empty() {
        return res;
    }

    shuffle(&mut names, &mut Mt19937::new(seed));
    names.truncate(count * 3);

    struct Building {
        pieces: Vec<PieceInfo>,
        bmin: Float3,
        bmax: Float3,
    }

    let mut picked = vec![];
    for n in &names {
        if picked.len() >= count {
            break;
        }

        let mut b = Building { pieces: vec![], bmin: EMPTY_MIN, bmax: EMPTY_MAX };
        for path in &folders[n] {
            let Some(piece) = piece_info(files, path, &mut res.errors) else { continue };
            b.bmin = min3(b.bmin, piece.bmin);
            b.bmax = max3(b.bmax, piece.bmax);
            b.pieces.push(piece);
        }

        if b.pieces.is_empty() {
            continue;
        }

        // House-sized only.
        let ext = b.bmax - b.bmin;
        if ext.x < 2.0 || ext.z < 2.0 || ext.x > 80.0 || ext.z > 80.0 || ext.y < 1.5 {
            continue;
        }

        picked.push(b);
    }

    // Grid layout with spacing from the largest footprint.
    let cell = picked.iter().fold(0.0f32, |cell, b| cell.max(b.bmax.x - b.bmin.x).max(b.bmax.z - b.bmin.z)) + 4.0;
    let side = (picked.len() as f64).sqrt().ceil() as usize;
    res.bounds_min = EMPTY_MIN;
    res.bounds_max = EMPTY_MAX;
    for (i, b) in picked.iter().enumerate() {
        let cx = ((i % side) as f32 - (side - 1) as f32 * 0.5) * cell;
        let cz = ((i / side) as f32 - (side - 1) as f32 * 0.5) * cell;
        let c = (b.bmin + b.bmax) * 0.5;
        let world = translation(Float3::new(cx - c.x, -b.bmin.y, cz - c.z));
        for piece in &b.pieces {
            let mi = MeshInstance { model: piece.path.clone(), ..Default::default() };
            scene.add(layer, world, Payload::MeshInstance(mi), SourceRef { doc, item: i as u64 + 1 });
        }

        res.centres.push(Float3::new(cx, 0.0, cz));
        res.bounds_min = Float3::new(res.bounds_min.x.min(cx - cell * 0.5), 0.0, res.bounds_min.z.min(cz - cell * 0.5));
        res.bounds_max = Float3::new(res.bounds_max.x.max(cx + cell * 0.5), res.bounds_max.y.max(b.bmax.y - b.bmin.y), res.bounds_max.z.max(cz + cell * 0.5));
    }

    res.buildings = picked.len();
    res
}
