//! Checked, per-resource planning for the existing v7 converter. Candidates never constitute authority.
use crate::{Result, file_error};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use wce_formats::xml_doc::XmlDoc;
use wce_terrain::{
    MapModel,
    map::BuiltSceneSource,
    terry::{EntityKind, TerryProject},
};
use wce_tile::battle_map::locations::BattleLocations;
use wce_tile_model::Blob;

#[derive(Clone, Copy)]
enum Value {
    Number,
    RectangleSize,
    Position,
}

#[derive(Clone)]
struct Field {
    owner: u64,
    value: Value,
}
type Fields = BTreeMap<(usize, String), Field>;

fn error(what: &str) -> crate::MapIoError {
    file_error("locations.blm", what)
}

fn number(s: &str) -> Result<f64> {
    s.parse::<f64>().ok().filter(|v| v.is_finite()).ok_or_else(|| error("the native scene requires finite numeric values"))
}

fn value(s: &str, kind: Value) -> Result<Vec<f64>> {
    match kind {
        Value::Position => {
            let ns = s.split_whitespace().map(number).collect::<Result<Vec<_>>>()?;
            if ns.len() != 3 {
                return Err(error("the catchment position requires three coordinates"));
            }
            Ok(ns)
        }
        Value::RectangleSize => {
            let n = number(s)?;
            if n < 0.0 {
                return Err(error("catchment rectangle sizes cannot be negative"));
            }
            Ok(vec![n])
        }
        Value::Number => {
            let n = number(s)?;
            if !(n as f32).is_finite() {
                return Err(error("the edited deployment value is outside the native float range"));
            }
            Ok(vec![n])
        }
    }
}

fn fields(p: &TerryProject) -> Fields {
    let mut fields = BTreeMap::new();
    for e in &p.entities {
        match p.kind(e) {
            EntityKind::CatchmentRegion => {
                if let Some(c) = p.component(e, "ECRectangle") {
                    for name in ["width", "height"] {
                        fields.insert((c, name.into()), Field { owner: e.id, value: Value::RectangleSize });
                    }
                }
                if let Some(c) = p.component(e, "ECTransform") {
                    fields.insert((c, "position".into()), Field { owner: e.id, value: Value::Position });
                }
            }
            EntityKind::DeploymentRegion => {
                if let Some(c) = p.component(e, "ECPolyline") {
                    let elems = p.doc.elements();
                    for &line in elems[c].children.iter().filter(|&&i| elems[i].name == "polyline") {
                        for &point in elems[line].children.iter().filter(|&&i| elems[i].name == "point") {
                            for name in ["x", "y"] {
                                fields.insert((point, name.into()), Field { owner: e.id, value: Value::Number });
                            }
                        }
                    }
                }
            }
            EntityKind::DeploymentZone => {
                if let Some(c) = p.component(e, "ECDeploymentZone")
                    && p.doc.get(c, "orientation").is_some()
                {
                    fields.insert((c, "orientation".into()), Field { owner: e.id, value: Value::Number });
                }
            }
            _ => {}
        }
    }
    fields
}

fn shape(a: &XmlDoc, b: &XmlDoc) -> Result<()> {
    if a.roots() != b.roots() || a.elements().len() != b.elements().len() {
        return Err(error("structural scene changes require a separately verified native editing capability"));
    }
    for (a, b) in a.elements().iter().zip(b.elements()) {
        if a.name != b.name
            || a.parent != b.parent
            || a.children != b.children
            || !a.attrs.iter().map(|a| &a.name).eq(b.attrs.iter().map(|a| &a.name))
        {
            return Err(error("scene structure or attributes are not represented by the preservation-safe native writer"));
        }
    }
    Ok(())
}

/// Account for every delta, not just differences visible in a lossy candidate. Unknown text,
/// attributes, component switches and structural edits cannot masquerade as a numeric no-op.
fn classify(original: &TerryProject, current: &TerryProject, project: bool) -> Result<(Fields, BTreeSet<u64>)> {
    shape(&original.doc, &current.doc)?;
    let allowed = fields(original);
    let mut changed = BTreeSet::new();
    let mut masked = current.doc.clone();
    for (i, e) in original.doc.elements().iter().enumerate() {
        for a in &e.attrs {
            let before = original.doc.get(i, &a.name).ok_or_else(|| error("missing original scene attribute"))?;
            let after = current.doc.get(i, &a.name).ok_or_else(|| error("missing current scene attribute"))?;
            if before == after {
                continue;
            }
            if e.name == "entity" && a.name == "name" {
                if project {
                    return Err(error(
                        "scene entity names cannot be restored by this native project format; undo the label change before Save (native export does not save labels)",
                    ));
                }
            } else if let Some(field) = allowed.get(&(i, a.name.clone())) {
                let old = value(&before, field.value)?;
                let new = value(&after, field.value)?;
                if matches!(field.value, Value::Position) && old[1] != new[1] {
                    return Err(error("vertical catchment changes have no native BLM representation"));
                }
                if old != new {
                    changed.insert(field.owner);
                }
            } else {
                return Err(error(
                    "this Inspector or location-field change cannot be preserved by the verified v7 native writer; undo it before Save or export",
                ));
            }
            masked.set(i, &a.name, &before);
        }
    }
    if masked.save().as_slice() != original.doc.origin_bytes() {
        return Err(error("scene text or structure contains unrepresented changes; restore them before Save or export"));
    }
    Ok((allowed, changed))
}

fn names(original: &TerryProject, current: &TerryProject) -> Result<()> {
    for e in &current.entities {
        if let EntityKind::Bca(kind) = current.kind(e) {
            let old = original.entity(e.id).and_then(|e| match original.kind(e) {
                EntityKind::Bca(k) => Some(k),
                _ => None,
            });
            if old.as_deref() != Some(kind.as_str()) && (!kind.is_ascii() || kind.len() > u16::MAX as usize) {
                return Err(error(
                    "edited battle-type names require ASCII and a length representable by the native u16 prefix; other name encoding is unsupported",
                ));
            }
        }
    }
    Ok(())
}

fn dependencies(source: &BuiltSceneSource) -> Result<()> {
    if source.ambiguous_locations {
        return Err(error("multiple BLM keys make edited serialization ambiguous"));
    }
    for n in &source.required {
        match source.deployments.get(n).map(Vec::as_slice) {
            Some([_]) => {}
            Some(_) => return Err(error("a referenced deployment number has multiple native resource keys; reopen an unambiguous source")),
            None => return Err(error("a required deployment is missing; restore it before making native scene changes")),
        }
    }
    Ok(())
}

fn locations(bytes: &[u8]) -> Result<BattleLocations> {
    BattleLocations::parse(bytes).map_err(|e| file_error("locations.blm", e))
}

fn unchanged_layout(a: &BattleLocations, b: &BattleLocations) -> Result<()> {
    if (a.version, a.width, a.height) != (b.version, b.width, b.height) || a.tail != b.tail || a.types.len() != b.types.len() {
        return Err(error("the scene converter changed protected BLM structure"));
    }
    for ((ak, ar), (bk, br)) in a.types.iter().zip(&b.types) {
        if ak != bk || ar.len() != br.len() {
            return Err(error("native type-group changes are not supported by this preservation plan"));
        }
        for (a, b) in ar.iter().zip(br) {
            if (a.centre, a.id, a.extra) != (b.centre, b.id, b.extra) || b.rect[0] > b.rect[2] || b.rect[1] > b.rect[3] {
                return Err(error("location identity, centre or opaque fields changed outside the supported rectangle edit"));
            }
        }
    }
    Ok(())
}

fn rectangle_ranges(p: &TerryProject, changed: &BTreeSet<u64>, blm: &BattleLocations) -> Result<()> {
    let centres = [(blm.width / 2) as i32, (blm.height / 2) as i32];
    for e in p.entities.iter().filter(|e| changed.contains(&e.id) && matches!(p.kind(e), EntityKind::CatchmentRegion)) {
        let size = p.rectangle(e).ok_or_else(|| error("missing catchment rectangle"))?;
        let at = p.world_position(e.id);
        let corners = [at[0] - size[0] / 2.0, -(at[2] + size[1] / 2.0), at[0] + size[0] / 2.0, -(at[2] - size[1] / 2.0)];
        for (i, world) in corners.into_iter().enumerate() {
            let offset = (world / 256.0).trunc();
            let cell = f64::from(centres[i % 2]) + offset;
            if !offset.is_finite()
                || offset < f64::from(i32::MIN)
                || offset > f64::from(i32::MAX)
                || cell < f64::from(i32::MIN)
                || cell > f64::from(i32::MAX)
            {
                return Err(error("the edited rectangle is outside the native integer cell range"));
            }
        }
    }
    Ok(())
}

fn candidates(files: Vec<(String, Vec<u8>)>, source: &BuiltSceneSource) -> Result<BTreeMap<String, Vec<u8>>> {
    let mut mapped = BTreeMap::new();
    let mut seen = BTreeSet::new();
    for (generated, bytes) in files {
        let n = super::source::deployment_number(&generated).ok_or_else(|| error("unexpected generated deployment key"))?;
        if !source.required.contains(&n) || !seen.insert(n) {
            return Err(error("record or deployment creation/deletion requires a separately verified capability"));
        }
        let Some(keys) = source.deployments.get(&n) else {
            return Err(error("cannot synthesize a missing deployment"));
        };
        let [key] = keys.as_slice() else {
            return Err(error("ambiguous deployment source"));
        };
        mapped.insert(key.clone(), bytes);
    }
    if seen != source.required {
        return Err(error("the scene lost a required deployment reference"));
    }
    Ok(mapped)
}

/// Produce a complete, immutable content plan. File provenance remains untouched in m.carried.
/// The project-purpose flag only governs scene metadata; it grants no filesystem publication rights.
pub(in crate::built) fn plan(m: &MapModel, project: bool) -> Result<BTreeMap<String, Blob>> {
    let mut out = BTreeMap::new();
    let Some(source) = super::source::authority(m)? else { return Ok(out) };
    super::source::retain(source, &mut out);
    let current = m.project.as_ref().ok_or_else(|| error("missing captured scene"))?.get();
    let current_xml = current.save();
    if current_xml.as_slice() == source.scene.as_ref() {
        return Ok(out);
    }
    drop(current_xml);

    let original = TerryProject::parse(source.scene.to_vec()).map_err(|e| error(&format!("invalid captured scene: {e}")))?;
    names(&original, current)?;
    let (allowed, changed) = classify(&original, current, project)?;
    if changed.is_empty() {
        return Ok(out);
    }
    dependencies(source)?;
    let native = source.resources.get(&source.locations_key).ok_or_else(|| error("missing captured BLM"))?;
    let blm = locations(&native.bytes)?;
    if blm.version != 7 || blm.width == 0 || blm.height == 0 {
        return Err(error("the captured BLM version or dimensions are not editable"));
    }
    // Checked byte encoding does not establish high-byte type-name authoring semantics.
    // Retain I1's conservative scene-authoring restriction independently of the direct codec.
    if super::records(&original, blm.width, blm.height).iter().any(|(name, _)| !name.is_ascii() || name.len() > u16::MAX as usize) {
        return Err(error("this native type name is not supported for scene authoring; retain the original resources"));
    }
    rectangle_ranges(current, &changed, &blm)?;

    let (b0, d0) = super::write(&original, &blm)?;
    let (b1, d1) = super::write(current, &blm)?;
    unchanged_layout(&locations(&b0)?, &locations(&b1)?)?;
    if b1 != b0 {
        if b0.as_slice() != native.bytes.as_ref() {
            return Err(error(
                "this BLM contains native information the scene cannot rebuild losslessly; restore the edit or retain the original resource",
            ));
        }
        out.insert(source.locations_key.clone(), Blob::Bytes(Arc::from(b1)));
    }
    let d0 = candidates(d0, source)?;
    let d1 = candidates(d1, source)?;
    for (key, after) in d1 {
        let before = d0.get(&key).ok_or_else(|| error("deployment candidate set changed"))?;
        if &after != before {
            let raw = source.resources.get(&key).ok_or_else(|| file_error(&key, "missing captured deployment"))?;
            if before.as_slice() != raw.bytes.as_ref() {
                return Err(file_error(&key, "this edited deployment contains native information the scene cannot rebuild losslessly"));
            }
            out.insert(key, Blob::Bytes(Arc::from(after)));
        }
    }

    // Confirm that the intended supported fields actually survive the native representation. This
    // rejects sub-cell/f32 changes the converter would silently quantize away, rather than mistaking
    // candidate equality for successful persistence. Check the whole affected rectangle/polyline.
    let written = locations(&out[&source.locations_key].bytes()?)?;
    let mut xml = BTreeMap::new();
    for n in &source.required {
        let key = &source.deployments[n][0];
        xml.insert(*n, wce_tile::vegetation::ca_xml::parse(&out[key].bytes()?));
    }
    let reopened = super::scene(&written, &xml).map_err(|e| error(&e))?;
    shape(&original.doc, &reopened.doc)?;
    for ((elem, attr), field) in allowed.iter().filter(|(_, f)| changed.contains(&f.owner)) {
        let intended = current.doc.get(*elem, attr).ok_or_else(|| error("missing intended native field"))?;
        let restored = reopened.doc.get(*elem, attr).ok_or_else(|| error("missing restored native field"))?;
        if value(&intended, field.value)? != value(&restored, field.value)? {
            return Err(error(
                "this scene change cannot be restored exactly on the native cell/float representation; use representable coordinates",
            ));
        }
    }
    Ok(out)
}
