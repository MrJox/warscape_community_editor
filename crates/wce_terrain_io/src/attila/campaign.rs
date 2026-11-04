//! A campaign map's built files made from its `.terry` (BOB_Terrain's `Terry file` action on `terrain/campaigns/<map>`; docs/re/terry.md,
//! "Campaign maps"):
//!
//! - `prop_marker.markers` (`PROP_MARKER` version 9): a record per prop (`ECPropMesh` with its `ECMesh`) in entity order, its group the
//!   model's path; the matrix the kit's `ECTransform` world transform (`matrix`); `unknown_64` 1; the seasons of `ECSeasonMask` (spring
//!   1, summer 2, autumn 4, winter 8); `unknown_74` the fauna bit (the model's path has `/fauna/`); the visibility flags of `ECPropMesh`;
//! - `particle_emitter_marker.markers` (`PARTICLE_EMITTER_MARKER` version 9): a record per `ECVFX`, its group the effect, `autoplay`,
//!   the visibility flags, `scale` and the seasons;
//! - `environment_collection.xml`: the environment volumes (`ECEnvironmentVolume`): one of outer radius 0 the global lighting, the others
//!   spheres (position, radii; `%f`); CRLF lines.
//!
//! The groups are in the order of the engine's marker table (`wce_tile_io::attila::compile::text_key_group_order`).

use wce_formats::markers::{KeyPart, MarkerGroup, MarkersFile, Value, record_from_fields};
use wce_terrain::terry::{EntityKind, TerryEntity, TerryProject};

const REPOSITORY_ID: u32 = 645_563;
const PROP_CLASS: u32 = 645_566;
const PARTICLE_CLASS: u32 = 618_864;

/// An `ECTransform`'s numbers (rotation, scale, position).
fn numbers(p: &TerryProject, e: &TerryEntity, attr: &str, default: [f64; 3]) -> [f64; 3] {
    let v: Vec<f64> = p.component(e, "ECTransform").and_then(|c| p.doc.get(c, attr)).map(|v| v.split_whitespace().filter_map(|t| t.parse().ok()).collect()).unwrap_or_default();
    if v.len() == 3 { [v[0], v[1], v[2]] } else { default }
}

/// `CA::ca_sincos`: x87 `fsin` / `fcos` of an f32, rounded to f32.
fn sincos(a: f32) -> (f32, f32) {
    let (s, c) = f64::from(a).sin_cos();
    (s as f32, c as f32)
}

/// An entity's marker matrix as the kit's `QTU::ECTransform` makes it, all in f32 (docs/re/terry.md, "Campaign maps"):
/// `on_rotation_changed` turns the rotation (degrees) into a quaternion from the half angles' sines and cosines, `update_transform` the
/// quaternion into a rotation matrix `R` and the transform `R` times the scale on each column, then the position. A marker's row `i` is
/// the transform's column `i`.
fn matrix(p: &TerryProject, e: &TerryEntity) -> [f32; 16] {
    let r = numbers(p, e, "rotation", [0.0; 3]).map(|v| v as f32);
    let s = numbers(p, e, "scale", [1.0; 3]).map(|v| v as f32);
    let w = p.world_position(e.id);
    let half = |d: f32| d * std::f32::consts::PI * (1.0f32 / 180.0) * 0.5;
    let (sx, cx) = sincos(half(r[0]));
    let (sy, cy) = sincos(half(r[1]));
    let (sz, cz) = sincos(half(r[2]));
    let (czcy, szsy, szcy, czsy) = (cz * cy, sz * sy, sz * cy, cz * sy);
    let (qx, qy, qz, qw) = (czcy * sx - szsy * cx, szcy * sx + czsy * cx, szcy * cx - czsy * sx, szsy * sx + czcy * cx);
    let (x2, y2, z2, w2) = (qx * 2.0, qy * 2.0, qz * 2.0, qw * 2.0);
    let (xx, yy, zz) = (x2 * qx, y2 * qy, z2 * qz);
    let (xy, xz, yz, wx, wy, wz) = (qy * x2, qz * x2, qz * y2, w2 * qx, w2 * qy, w2 * qz);
    let rot = [
        [(1.0 - yy) - zz, xy - wz, xz + wy],
        [xy + wz, (1.0 - xx) - zz, yz - wx],
        [xz - wy, yz + wx, (1.0 - xx) - yy],
    ];

    let mut m = [0f32; 16];
    for (i, sc) in s.iter().enumerate() {
        for (j, row) in rot.iter().enumerate() {
            // The generic products' terms times 0 make a zero +0.
            m[i * 4 + j] = row[i] * sc + 0.0;
        }
    }

    m[12..15].copy_from_slice(&[w[0] as f32, w[1] as f32, w[2] as f32]);
    m[15] = 1.0;
    m
}

fn flag(p: &TerryProject, e: &TerryEntity, component: &str, attr: &str) -> u8 {
    u8::from(p.component(e, component).and_then(|c| p.doc.get(c, attr)).as_deref() == Some("true"))
}

/// `ECSeasonMask`'s seasons as the markers' bits (spring 1, summer 2, autumn 4, winter 8); without one every season.
fn seasons(p: &TerryProject, e: &TerryEntity) -> u32 {
    if p.component(e, "ECSeasonMask").is_none() {
        return 15;
    }

    ["spring", "summer", "autumn", "winter"].iter().enumerate().filter(|(_, s)| flag(p, e, "ECSeasonMask", s) == 1).map(|(i, _)| 1 << i).sum()
}

fn visibility(p: &TerryProject, e: &TerryEntity, component: &str) -> Vec<(&'static str, Value)> {
    ["visible_inside_snow_zone", "visible_outside_snow_zone", "visible_inside_destruction_zone", "visible_outside_destruction_zone"]
        .iter().map(|a| (*a, Value::U8(flag(p, e, component, a)))).collect()
}

/// A markers file of records keyed by text (the groups in the marker table's order).
fn markers(class: &str, class_id: u32, records: Vec<(String, Vec<u8>)>) -> Vec<u8> {
    let mut first: Vec<String> = vec![];
    let mut indices: std::collections::HashMap<String, Vec<u32>> = std::collections::HashMap::new();
    for (i, (k, _)) in records.iter().enumerate() {
        if !indices.contains_key(k) {
            first.push(k.clone());
        }

        indices.entry(k.clone()).or_default().push(i as u32);
    }

    let groups = wce_tile_io::attila::compile::text_key_group_order(&first).into_iter()
        .map(|i| MarkerGroup { key: vec![KeyPart::Wide(first[i].encode_utf16().collect())], indices: indices[&first[i]].clone() })
        .collect();
    MarkersFile {
        repository: "BASE_MARKER_REPOSITORY".to_owned(), repository_id: REPOSITORY_ID, repository_u32: 1, repository_u16: 1, class_id,
        class_name: class.to_owned(), version: 9, records: records.into_iter().map(|r| r.1).collect(), groups: Some(groups),
    }
    .write()
}

/// `prop_marker.markers`.
///
/// # Errors
///
/// A record does not fit the layout (not expected).
pub fn prop_markers(p: &TerryProject) -> Result<Vec<u8>, String> {
    let mut records = vec![];
    for e in &p.entities {
        let EntityKind::Prop(model) = p.kind(e) else { continue };
        let mut f: Vec<(&str, Value)> = vec![
            ("transform", Value::Matrix(matrix(p, e))), ("unknown_64", Value::U8(1)), ("is_decal", Value::U8(0)), ("unknown_66", Value::U8(0)),
            ("unknown_67", Value::U8(0)), ("unknown_68", Value::U8(0)), ("unknown_69", Value::U8(0)), ("seasons", Value::U32(seasons(p, e))),
            // BOB_Terrain: the fauna bit of a model under `/fauna/` (case-sensitive).
            ("unknown_74", Value::U8(u8::from(model.contains("/fauna/")))),
        ];
        f.extend(visibility(p, e, "ECPropMesh"));
        records.push((model, record_from_fields("PROP_MARKER", 9, &f).map_err(|err| err.0)?));
    }

    Ok(markers("PROP_MARKER", PROP_CLASS, records))
}

/// `particle_emitter_marker.markers`.
///
/// # Errors
///
/// A record does not fit the layout (not expected).
pub fn particle_markers(p: &TerryProject) -> Result<Vec<u8>, String> {
    let mut records = vec![];
    for e in &p.entities {
        let EntityKind::Vfx(vfx) = p.kind(e) else { continue };
        let scale: f32 = p.component(e, "ECVFX").and_then(|c| p.doc.get(c, "scale")).and_then(|v| v.trim().parse().ok()).unwrap_or(1.0);
        let mut f: Vec<(&str, Value)> = vec![
            ("transform", Value::Matrix(matrix(p, e))), ("unknown_64", Value::Bytes(vec![0; 6])), ("autoplay", Value::U8(flag(p, e, "ECVFX", "autoplay"))),
            ("use_in_outfield", Value::U8(0)),
        ];
        f.extend(visibility(p, e, "ECVFX"));
        f.push(("scale", Value::F32(scale)));
        f.push(("seasons", Value::U32(seasons(p, e))));
        records.push((vfx, record_from_fields("PARTICLE_EMITTER_MARKER", 9, &f).map_err(|err| err.0)?));
    }

    Ok(markers("PARTICLE_EMITTER_MARKER", PARTICLE_CLASS, records))
}

/// `environment_collection.xml`.
pub fn environment_collection(p: &TerryProject) -> Vec<u8> {
    let f = wce_terrain::deployment::printf_f;
    let mut global = String::new();
    let mut spheres = String::new();
    for e in &p.entities {
        let Some(c) = p.component(e, "ECEnvironmentVolume") else { continue };
        let get = |a: &str| p.doc.get(c, a).unwrap_or_default();
        let num = |a: &str| get(a).trim().parse::<f64>().unwrap_or(0.0);
        if num("outer_radius") == 0.0 {
            global = get("lighting_file");
            continue;
        }

        let w = p.world_position(e.id);
        spheres += &format!(
            "\t\t<SPHERE serialise_version='1' lighting='{}' x='{}' y='{}' z='{}' inner_radius='{}' outer_radius='{}'/>\n",
            get("lighting_file"), f(f64::from(w[0] as f32)), f(f64::from(w[1] as f32)), f(f64::from(w[2] as f32)), f(num("inner_radius")), f(num("outer_radius"))
        );
    }

    format!("<ENVIRONMENT_COLLECTION serialise_version='1' global_lighting='{global}'>\n\t<ENVIRONMENT_SPHERES>\n{spheres}\t</ENVIRONMENT_SPHERES>\n</ENVIRONMENT_COLLECTION>\n").replace('\n', "\r\n").into_bytes()
}
