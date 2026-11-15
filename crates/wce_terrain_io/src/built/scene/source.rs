//! Capture content once, keeping File source associations available to document publication.
use crate::{Result, file_error};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use wce_terrain::{
    MapModel,
    map::{BuiltSceneSource, SceneResource},
    terry::TerryProject,
};
use wce_tile::battle_map::locations::BattleLocations;
use wce_tile_model::Blob;

pub(super) fn deployment_number(path: &str) -> Option<u32> {
    path.strip_prefix("deployment/")?.strip_prefix("deployment_")?.strip_suffix(".xml")?.parse().ok()
}

fn resource(path: &str) -> bool {
    path.eq_ignore_ascii_case("locations.blm")
        || path.to_ascii_lowercase().starts_with("locations.blm.")
        || deployment_number(path).is_some()
}

pub(in crate::built) fn load(m: &MapModel) -> Result<Option<(TerryProject, Arc<BuiltSceneSource>)>> {
    let Some((key, blob)) = m.carried.iter().find(|(p, _)| p.eq_ignore_ascii_case("locations.blm")) else { return Ok(None) };
    // Other variants and parser-unhandled inputs remain carried opaquely, as before.
    let Ok(bytes) = blob.bytes() else { return Ok(None) };
    let Ok(blm) = BattleLocations::parse(&bytes) else { return Ok(None) };
    if blm.version != 7 {
        return Ok(None);
    }

    let mut resources = BTreeMap::new();
    let mut deployments: BTreeMap<u32, Vec<String>> = BTreeMap::new();
    let mut xml = BTreeMap::new();
    for (path, original) in m.carried.iter().filter(|(p, _)| resource(p)) {
        let data = if path == key { bytes.clone() } else { original.bytes()? };
        if let Some(n) = deployment_number(path) {
            deployments.entry(n).or_default().push(path.clone());
            xml.insert(n, wce_tile::vegetation::ca_xml::parse(&data));
        }
        resources.insert(path.clone(), SceneResource { original: original.clone(), bytes: data });
    }
    let project = super::scene(&blm, &xml).map_err(|e| file_error(key, e))?;
    let source = BuiltSceneSource {
        locations_key: key.clone(),
        // One retained XML buffer per loaded generation; Arc clones share it through undo/redo.
        scene: Arc::from(project.doc.origin_bytes()),
        required: blm.types.iter().flat_map(|(_, rs)| rs).filter(|r| r.id != -1).map(|r| (r.id as u32).wrapping_sub(0xffff_0000)).collect(),
        ambiguous_locations: resources.keys().filter(|p| p.eq_ignore_ascii_case("locations.blm")).count() != 1,
        resources,
        deployments,
    };
    Ok(Some((project, Arc::new(source))))
}

pub(in crate::built) fn authority(m: &MapModel) -> Result<Option<&BuiltSceneSource>> {
    let Some(source) = m.built_scene.as_deref() else {
        if m.project.as_ref().is_some_and(|p| p.path == super::super::SCENE || p.edited()) {
            return Err(file_error("locations.blm", "the edited scene has no captured native authority; reopen the map"));
        }
        return Ok(None);
    };
    let project = m.project.as_ref().ok_or_else(|| file_error("locations.blm", "the captured scene was removed; reopen the map"))?;
    if project.path != super::super::SCENE || project.get().doc.origin_bytes() != source.scene.as_ref() {
        return Err(file_error("locations.blm", "the scene origin was replaced; reopen the map"));
    }
    let keys: BTreeSet<_> = m.carried.keys().filter(|p| resource(p)).collect();
    if keys != source.resources.keys().collect() {
        return Err(file_error("locations.blm", "the native scene resource set was replaced or removed; reopen the map"));
    }
    for (key, captured) in &source.resources {
        // Compare semantic associations, not current bytes at a mutable pathname. Filesystem freshness
        // belongs to P0-03. Keeping these original File entries in carried preserves its provenance input.
        if m.carried.get(key) != Some(&captured.original) {
            return Err(file_error(key, "the semantic source association was replaced; reopen the map"));
        }
    }
    Ok(Some(source))
}

pub(in crate::built) fn retain(source: &BuiltSceneSource, folder: &mut BTreeMap<String, Blob>) {
    for (key, captured) in &source.resources {
        folder.insert(key.clone(), Blob::Bytes(captured.bytes.clone()));
    }
}
