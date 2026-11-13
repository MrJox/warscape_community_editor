//! Fixes > Light wall torches, for Attila: it drops the effects of a standing piece's `.meta`, and plays the `vfx_emitters` of its logic
//! file (docs/re/buildings.md, "What gates a piece's effects"). This builds what makes them play, from the game and the chosen mods.
//!
//! Each always-on effect of a `.meta` goes to the list of the destruct that attaches the model, at the same place, as the game stops a
//! list only when its own piece changes state: on the model itself it would outlive its wall. It goes to the model's own list only when
//! nothing attaches it. A model that is attached somewhere stays dark where it stands alone: its logic file cannot tell the two apart.
//!
//! The entries of a destruct that play one effect share one running effect, moved through their places, so only one shows: each one after
//! the first plays a copy of it, `vfx/wce_<effect>_<n>.xml`. Those files go in a pack of their own, of movie type: the game ignores `vfx/`
//! in a mod-type pack (docs/re/game_command_line.md, "What loads before the mods"), and loads a movie-type one always.
//!
//! A post placed where its destruct's mesh is higher than its flame is under the wall (the upper end of the step walls): the attachment
//! goes up, the model and its flame with it. Sconces stay: they hang under arches and roofs on purpose.

use std::collections::{BTreeMap, BTreeSet};
use std::io::Cursor;
use std::path::Path;

use rpfm_lib::files::cs2_parsed::Cs2Parsed;
use rpfm_lib::files::rigidmodel::RigidModel;
use rpfm_lib::files::{Decodeable, DecodeableExtraData, Encodeable};
use rpfm_lib::games::pfh_file_type::PFHFileType;
use serde_json::{Value, json};
use wce_formats::anim_meta::{MetaEffect, meta_effects};
use wce_vfs::Vfs;

/// The packs, in the game's data folder: the logic files, and the effects' copies.
pub const LOGIC_PACK: &str = "wce_lit_buildings.pack";
pub const VFX_PACK: &str = "wce_lit_buildings_vfx.pack";

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
            let v = if r == 3 && c < 3 {
                at[c]
            } else if r == c {
                1.0
            } else {
                0.0
            };
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
        let Some(flame) = models.get(&building).and_then(|e| e.first()).and_then(|e| through(&f["transform"], e.position)) else {
            continue;
        };
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

/// What [`build`] made.
#[derive(Default)]
pub struct Built {
    /// Path in the packs, bytes.
    pub logic: Vec<(String, Vec<u8>)>,
    pub effects: Vec<(String, Vec<u8>)>,
    pub raised: usize,

    /// Logic files with something to add that could not be patched (a mod's, in a form this does not write back), and why.
    pub skipped: Vec<String>,
}

/// Builds the packs' contents from what `files` shows. `files` must not have [`LOGIC_PACK`] among its packs: that would patch its own work.
///
/// # Errors
///
/// An effect to copy is not in the files.
pub fn build(files: &Vfs) -> Result<Built, String> {
    const BUILDINGS: &str = "rigidmodels/buildings/";

    // Building key -> destruct -> its effects.
    let mut buildings: BTreeMap<String, BTreeMap<String, Vec<MetaEffect>>> = BTreeMap::new();
    for path in files.list(BUILDINGS, ".meta").into_iter().filter(|p| !p.ends_with("_anim.meta")) {
        let Ok(bytes) = files.read(path) else { continue };
        let effects = always_on(&bytes);
        let (Some(key), Some(file), false) = (path.split('/').nth(2), path.rsplit('/').next(), effects.is_empty()) else { continue };
        if LEFT_OUT.contains(&key) {
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

    let logic = files.list(BUILDINGS, "_tech.cs2.parsed").into_iter().filter_map(|p| Some((p, files.read(p).ok()?))).collect::<Vec<_>>();
    let mut models = BTreeMap::new();
    for key in logic.iter().flat_map(|(_, b)| attaching(b)).collect::<BTreeSet<_>>() {
        if let Some(by_destruct) = buildings.remove(&key) {
            models.insert(key, by_destruct.into_values().flatten().collect::<Vec<_>>());
        }
    }

    let (mut built, mut copies) = (Built::default(), BTreeMap::new());
    for (path, bytes) in logic {
        let own = path.split('/').nth(2).and_then(|key| buildings.get(key)).cloned().unwrap_or_default();
        let dir = &path[..path.rfind('/').unwrap_or_default() + 1];
        let key = path.split('/').nth(2).unwrap_or_default();
        let mesh_of = |destruct: &str| {
            let meshes = files.list(&format!("{dir}{key}_{destruct}"), ".rigid_model_v2");
            meshes
                .into_iter()
                .filter(|p| !p.contains("_anim"))
                .filter_map(|p| files.read(p).ok())
                .flat_map(|b| triangles(&b))
                .collect::<Vec<_>>()
        };

        match patched(&bytes, &own, &models, &mesh_of, &mut copies, &mut built.raised) {
            Ok(Some(new)) => built.logic.push((path.to_owned(), new)),
            Ok(None) => {}
            Err(why) => built.skipped.push(format!("{path}: {why}")),
        }
    }

    for (effect, &most) in copies.iter().filter(|(_, most)| **most > 1) {
        let path = format!("vfx/{effect}.xml");
        let xml = files.read(&path).map_err(|e| format!("{path}: {e}"))?;
        built.effects.extend((2..=most).map(|n| (format!("vfx/wce_{effect}_{n:02}.xml"), xml.clone())));
    }

    Ok(built)
}

/// Writes one pack by way of a file beside it, so that a failure leaves the old one.
fn replace(path: &Path, kind: PFHFileType, files: Vec<(String, Vec<u8>)>, table_file: &str) -> Result<(), String> {
    let new = path.with_extension("new.pack");
    wce_walls::pack::write_pack_as(&new, "attila", kind, files, vec![], table_file)?;
    std::fs::rename(&new, path).map_err(|e| {
        let _ = std::fs::remove_file(&new);
        format!("{}: {e}", path.display())
    })
}

/// Puts the packs in the game's data folder, in place of the ones there.
///
/// # Errors
///
/// A pack can't be written (the game has it open, say).
pub fn install(data: &Path, built: Built) -> Result<(), String> {
    replace(&data.join(LOGIC_PACK), PFHFileType::Mod, built.logic, "wce_lit_buildings")?;
    replace(&data.join(VFX_PACK), PFHFileType::Movie, built.effects, "wce_lit_buildings_vfx")
}

/// Takes the packs out of the game's data folder.
///
/// # Errors
///
/// A pack can't be deleted (the game has it open, say).
pub fn uninstall(data: &Path) -> Result<(), String> {
    for pack in [LOGIC_PACK, VFX_PACK] {
        let path = data.join(pack);
        if path.is_file() {
            std::fs::remove_file(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const FLAT: Triangle = [[0.0, 2.0, 0.0], [10.0, 2.0, 0.0], [0.0, 2.0, 10.0]];
    const HIGH: Triangle = [[0.0, 5.0, 0.0], [10.0, 5.0, 0.0], [0.0, 5.0, 10.0]];

    #[test]
    fn the_top_is_the_highest_surface_there() {
        assert_eq!(top_at(&[FLAT, HIGH], 1.0, 1.0), Some(5.0));
        assert_eq!(top_at(&[FLAT], 1.0, 1.0), Some(2.0));
        assert_eq!(top_at(&[FLAT], 9.0, 9.0), None);
    }

    #[test]
    fn a_place_goes_through_a_row_vector_matrix() {
        let mut m = matrix([1.0, 2.0, 3.0]);
        assert_eq!(through(&m, [0.0, 0.5, 0.0]), Some([1.0, 2.5, 3.0]));

        // Turned a quarter: x goes to z.
        m["m00"] = json!(0.0);
        m["m02"] = json!(1.0);
        m["m20"] = json!(1.0);
        m["m22"] = json!(0.0);
        assert_eq!(through(&m, [2.0, 0.0, 0.0]), Some([1.0, 2.0, 5.0]));
    }

    #[test]
    fn a_post_under_the_mesh_goes_up_and_one_over_it_stays() {
        let post = MetaEffect {
            tag: String::new(),
            effect: "fire".to_owned(),
            start: 0.0,
            end: 0.0,
            position: [0.0, 2.0, 0.0],
            rotation: [0.0, 0.0, 0.0, 1.0],
            bone: -1,
            scale: 1.0,
            volume: None,
        };
        let models = BTreeMap::from([("torch_ground_post".to_owned(), vec![post])]);
        let file_ref = |y: f32| json!({ "key": "piece01_destruct01_file:torch_ground_post", "transform": matrix([1.0, y, 1.0]) });

        // The flame at 2 + 2, the mesh at 5: 0.75 over it.
        let mut d = json!({ "file_refs": [file_ref(2.0)] });
        assert_eq!(raise_posts(&mut d, &[HIGH], &models), 1);
        assert!((d["file_refs"][0]["transform"]["m31"].as_f64().unwrap() - 3.75).abs() < 1e-5);
        assert_eq!(list_of(&d, None, &models), [("fire".to_owned(), [1.0, 5.75, 1.0])]);

        // The flame at 6, well over it.
        let mut d = json!({ "file_refs": [file_ref(4.0)] });
        assert_eq!(raise_posts(&mut d, &[HIGH], &models), 0);
        assert_eq!(d["file_refs"][0]["transform"]["m31"].as_f64(), Some(4.0));
    }
}
