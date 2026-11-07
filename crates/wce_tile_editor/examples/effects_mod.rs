//! Builds a mod pack that lights what Attila leaves dark: it drops the effects of a standing piece's `.meta`, and plays the `vfx_emitters`
//! of its logic file (docs/re/buildings.md, "What gates a piece's effects"). The game stops that list only when the piece's own state
//! changes, so an attached building's list would outlive its wall: each always-on effect of a `.meta` goes to the list of the destruct
//! that attaches the model, at the same place, and to the model's own list only when nothing attaches it. A model that is attached
//! somewhere stays dark where it stands alone: there is no telling the two apart in its logic file.
//!
//! The entries of a destruct that play one effect share one running effect, moved through their places, so only one shows: each one after
//! the first plays a copy of it, `vfx/wce_<effect>_<n>.xml`. Those files go in a pack of their own, of movie type: the game ignores `vfx/`
//! in a mod-type pack, and loads a movie-type one always.
//!
//! A post placed where its destruct's mesh is higher than its flame is under the wall (the upper end of the step walls): the attachment
//! goes up, the model and its flame with it. Sconces stay: they hang under arches and roofs on purpose.
//!
//! The packs read are the game's, the one with `vfx/` among them. What comes out is <out pack>, the patched logic files, and beside it
//! `<out pack stem>_vfx.pack`, the effects.
//!
//!   cargo run --release -p wce_tile_editor --example effects_mod -- <out pack, or - to only list> <game pack>...

use std::collections::{BTreeMap, BTreeSet};
use std::io::Cursor;
use std::path::Path;

use rpfm_lib::files::cs2_parsed::Cs2Parsed;
use rpfm_lib::files::rigidmodel::RigidModel;
use rpfm_lib::files::{Decodeable, DecodeableExtraData, Encodeable};
use rpfm_lib::games::pfh_file_type::PFHFileType;
use serde_json::{Value, json};
use wce_formats::anim_meta::{MetaEffect, meta_effects};

/// Its model is 294 bytes and the game stops with a runtime error when it is placed; nothing attaches it.
const LEFT_OUT: &[&str] = &["greek_torch_sconce"];

/// The attached models that stand on the wall, and may be moved up.
const STANDING: &[&str] = &["torch_ground_post"];

/// A flame less than this over the mesh at its place is under it (metres), and goes this much over it: the step walls' lower posts are a
/// metre under, and `step_down_a`'s 0.47 and the bastions' 0.43 show well.
const UNDER: f32 = 0.25;
const OVER: f32 = 0.75;

type Triangle = [[f32; 3]; 3];

/// The triangles of a model's first level of detail.
fn triangles(bytes: &[u8]) -> Vec<Triangle> {
    let Ok(model) = RigidModel::decode(&mut Cursor::new(bytes), &Some(DecodeableExtraData::default())) else { return vec![] };
    let mut out = vec![];
    for block in model.lods().first().into_iter().flat_map(|l| l.mesh_blocks()) {
        let pivot = block.material().v_pivot();
        let (vertices, indices) = (block.mesh().vertices(), block.mesh().indices());
        let at = |i: u16| {
            vertices.get(usize::from(i)).map(|v| {
                let p = v.position();
                [p.x + pivot.x, p.y + pivot.y, p.z + pivot.z]
            })
        };

        out.extend(indices.chunks_exact(3).filter_map(|t| Some([at(t[0])?, at(t[1])?, at(t[2])?])));
    }

    out
}

/// The highest surface over (x, z), if any.
fn top_at(mesh: &[Triangle], x: f32, z: f32) -> Option<f32> {
    let height = |[a, b, c]: &Triangle| {
        let d = (b[2] - c[2]) * (a[0] - c[0]) + (c[0] - b[0]) * (a[2] - c[2]);
        if d.abs() < 1e-7 {
            return None;
        }

        let u = ((b[2] - c[2]) * (x - c[0]) + (c[0] - b[0]) * (z - c[2])) / d;
        let v = ((c[2] - a[2]) * (x - c[0]) + (a[0] - c[0]) * (z - c[2])) / d;
        (u >= 0.0 && v >= 0.0 && u + v <= 1.0).then(|| u * a[1] + v * b[1] + (1.0 - u - v) * c[1])
    };

    mesh.iter().filter_map(height).reduce(f32::max)
}

/// The always-on effects of a `.meta` (the `.meta` itself keeps them: the game ignores it).
fn always_on(bytes: &[u8]) -> Vec<MetaEffect> {
    let effects = meta_effects(bytes).unwrap_or_default();
    effects.into_iter().filter(|e| e.tag.is_empty() && e.volume.is_none() && !e.effect.starts_with("destruction_vfx")).collect()
}

/// A place as a logic file's matrix: the identity, the place in its last row.
fn matrix(at: [f32; 3]) -> Value {
    let mut m = serde_json::Map::new();
    for r in 0..4 {
        for c in 0..4 {
            let v = if r == 3 && c < 3 { at[c] } else if r == c { 1.0 } else { 0.0 };
            m.insert(format!("m{r}{c}"), json!(v));
        }
    }

    Value::Object(m)
}

fn decode(bytes: &[u8]) -> Result<Cs2Parsed, String> {
    Cs2Parsed::decode(&mut Cursor::new(bytes), &Some(DecodeableExtraData::default())).map_err(|e| e.to_string())
}

fn encode(mut parsed: Cs2Parsed) -> Result<Vec<u8>, String> {
    let mut out = vec![];
    parsed.encode(&mut out, &None).map_err(|e| e.to_string())?;
    Ok(out)
}

/// `at` through a logic file's row-vector matrix.
fn through(m: &Value, at: [f32; 3]) -> Option<[f32; 3]> {
    let g = |r: usize, c: usize| m[format!("m{r}{c}")].as_f64().map(|v| v as f32);
    let row = |c: usize| Some(at[0] * g(0, c)? + at[1] * g(1, c)? + at[2] * g(2, c)? + g(3, c)?);
    Some([row(0)?, row(1)?, row(2)?])
}

/// The building a `file_refs` entry attaches.
fn attached(file_ref: &Value) -> Option<String> {
    Some(file_ref["key"].as_str()?.split_once("_file:")?.1.to_lowercase())
}

/// Moves up the posts of a destruct that are under its mesh. Returns how many.
fn raise_posts(d: &mut Value, mesh: &[Triangle], models: &BTreeMap<String, Vec<MetaEffect>>) -> usize {
    let mut raised = 0;
    for f in d["file_refs"].as_array_mut().into_iter().flatten() {
        let Some(building) = attached(f).filter(|b| STANDING.contains(&b.as_str())) else { continue };
        let Some(flame) = models.get(&building).and_then(|e| e.first()).and_then(|e| through(&f["transform"], e.position)) else { continue };
        let Some(top) = top_at(mesh, flame[0], flame[2]).filter(|top| flame[1] < top + UNDER) else { continue };
        let y = f["transform"]["m31"].as_f64().unwrap_or_default() as f32;
        f["transform"]["m31"] = json!(y + top + OVER - flame[1]);
        raised += 1;
    }

    raised
}

/// What a destruct's list gets: its own effects, and those of what it attaches.
fn list_of(d: &Value, own: Option<&Vec<MetaEffect>>, models: &BTreeMap<String, Vec<MetaEffect>>) -> Vec<(String, [f32; 3])> {
    let mut out = own.into_iter().flatten().map(|e| (e.effect.clone(), e.position)).collect::<Vec<_>>();
    for f in d["file_refs"].as_array().into_iter().flatten() {
        let Some(effects) = attached(f).and_then(|b| models.get(&b)) else { continue };
        out.extend(effects.iter().filter_map(|e| Some((e.effect.clone(), through(&f["transform"], e.position)?))));
    }

    out
}

/// The logic file with the effects added, or `None` if it has nothing to add.
///
/// `copies`: effect -> the most a destruct plays, kept up. `mesh_of`: a destruct's mesh, by name. `raised`: posts moved up, kept up.
fn patched(
    bytes: &[u8],
    own: &BTreeMap<String, Vec<MetaEffect>>,
    models: &BTreeMap<String, Vec<MetaEffect>>,
    mesh_of: &dyn Fn(&str) -> Vec<Triangle>,
    copies: &mut BTreeMap<String, usize>,
    raised: &mut usize,
) -> Result<Option<Vec<u8>>, String> {
    let parsed = decode(bytes)?;
    let mut v = serde_json::to_value(&parsed).map_err(|e| e.to_string())?;
    let mut added = 0;
    for piece in v["pieces"].as_array_mut().ok_or("no pieces")? {
        for d in piece["destructs"].as_array_mut().ok_or("no destructs")? {
            let name = d["name"].as_str().ok_or("a destruct without a name")?.to_lowercase();
            if d["file_refs"].as_array().into_iter().flatten().filter_map(attached).any(|b| STANDING.contains(&b.as_str())) {
                *raised += raise_posts(d, &mesh_of(&name), models);
            }

            let effects = list_of(d, own.get(&name), models);
            if effects.is_empty() {
                continue;
            }

            if name.matches('_').count() != 1 {
                return Err(format!("{name}: the game would cut the key elsewhere"));
            }

            let list = d["vfx_emitters"].as_array_mut().ok_or("this version has no vfx_emitters")?;
            let mut seen = BTreeMap::<String, usize>::new();
            for (n, (effect, at)) in effects.into_iter().enumerate() {
                let count = seen.entry(effect.clone()).or_default();
                *count += 1;
                let most = copies.entry(effect.clone()).or_default();
                *most = (*most).max(*count);

                let played = if *count == 1 { effect.clone() } else { format!("wce_{effect}_{count:02}") };
                list.push(json!({ "key": format!("{name}_emitter_{played}_{:02}", n + 1), "matrix_1": matrix(at) }));
                added += 1;
            }
        }
    }

    if added == 0 {
        return Ok(None);
    }

    if encode(parsed)? != bytes {
        return Err("does not survive a round trip".to_owned());
    }

    encode(serde_json::from_value(v).map_err(|e| e.to_string())?).map(Some)
}

fn main() -> Result<(), String> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    let (out, packs) = args.split_first().ok_or("<out pack, or -> <game pack>...")?;
    let mut files = BTreeMap::new();
    for pack in packs {
        files.extend(wce_walls::pack::read_pack_files(Path::new(pack), "attila")?);
    }

    // Building key -> destruct -> its effects.
    let mut buildings: BTreeMap<String, BTreeMap<String, Vec<MetaEffect>>> = BTreeMap::new();
    for (path, bytes) in files.iter().filter(|(p, _)| p.starts_with("rigidmodels/buildings/") && p.ends_with(".meta") && !p.ends_with("_anim.meta")) {
        let effects = always_on(bytes);
        let (Some(key), Some(file), false) = (path.split('/').nth(2), path.rsplit('/').next(), effects.is_empty()) else { continue };
        for e in &effects {
            println!("{path}: {} at {:?}, turned {:?}, scale {} (start {} end {})", e.effect, e.position, e.rotation, e.scale, e.start, e.end);
        }

        if LEFT_OUT.contains(&key) {
            println!("  left out");
            continue;
        }

        let Some(destruct) = file.find("_piece").map(|at| file[at + 1..].trim_end_matches(".meta").to_lowercase()) else { continue };
        buildings.entry(key.to_owned()).or_default().entry(destruct).or_default().extend(effects);
    }

    // A model something attaches has its effects in the attaching destruct; the rest keep their own.
    let attaching = |bytes: &[u8]| -> Vec<String> {
        let Ok(v) = decode(bytes).and_then(|p| serde_json::to_value(&p).map_err(|e| e.to_string())) else { return vec![] };
        let destructs = v["pieces"].as_array().into_iter().flatten().flat_map(|p| p["destructs"].as_array().into_iter().flatten());
        destructs.flat_map(|d| d["file_refs"].as_array().into_iter().flatten()).filter_map(attached).collect()
    };

    let logic = files.iter().filter(|(p, _)| p.starts_with("rigidmodels/buildings/") && p.ends_with("_tech.cs2.parsed")).collect::<Vec<_>>();
    let mut models = BTreeMap::new();
    for key in logic.iter().flat_map(|(_, b)| attaching(b)).collect::<BTreeSet<_>>() {
        if let Some(by_destruct) = buildings.remove(&key) {
            println!("{key}: attached: its effects go to the destructs attaching it");
            models.insert(key, by_destruct.into_values().flatten().collect::<Vec<_>>());
        }
    }

    let (mut patches, mut entries, mut copies, mut raised) = (vec![], 0, BTreeMap::new(), 0);
    for (path, bytes) in logic {
        let own = path.split('/').nth(2).and_then(|key| buildings.get(key)).cloned().unwrap_or_default();
        let dir = &path[..path.rfind('/').unwrap_or_default() + 1];
        let key = path.split('/').nth(2).unwrap_or_default();
        let mesh_of = |destruct: &str| {
            let prefix = format!("{dir}{key}_{destruct}");
            let models = files.iter().filter(|(p, _)| p.starts_with(&prefix) && p.ends_with(".rigid_model_v2") && !p.contains("_anim"));
            models.flat_map(|(_, b)| triangles(b)).collect::<Vec<_>>()
        };

        let Some(new) = patched(bytes, &own, &models, &mesh_of, &mut copies, &mut raised).map_err(|e| format!("{path}: {e}"))? else { continue };
        println!("{path}: {} to {} bytes", bytes.len(), new.len());
        entries += 1;
        patches.push((path.clone(), new));
    }

    for (effect, &most) in copies.iter().filter(|(_, most)| **most > 1) {
        let path = format!("vfx/{effect}.xml");
        let xml = files.get(&path).ok_or(format!("{path}: not in these packs"))?;
        patches.extend((2..=most).map(|n| (format!("vfx/wce_{effect}_{n:02}.xml"), xml.clone())));
        println!("{effect}: at most {most} on a destruct");
    }

    println!("{entries} logic files, {raised} posts moved up");
    if out != "-" {
        let (vfx, logic) = patches.into_iter().partition::<Vec<_>, _>(|(p, _)| p.starts_with("vfx/"));
        wce_walls::pack::write_pack(Path::new(out), "attila", logic, vec![])?;

        let movie = format!("{}_vfx.pack", out.trim_end_matches(".pack"));
        wce_walls::pack::write_pack_as(Path::new(&movie), "attila", PFHFileType::Movie, vfx, vec![], "wce_lit_buildings_vfx")?;
    }

    Ok(())
}
