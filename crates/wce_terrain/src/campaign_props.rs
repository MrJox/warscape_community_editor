//! Lossless, existing-record campaign prop translation. The marker codec owns serialization;
//! the editable model keeps immutable native bytes and source record indices, never a reconstructed scene.

use std::{collections::BTreeMap, sync::Arc};
use wce_formats::markers::{KeyPart, MarkersFile, Value};

/// A native prop's document item id (separate from Terry ids and the other campaign previews).
pub const PROP_KEY: u64 = 1 << 59;
pub fn record_id(item: u64) -> Option<u32> {
    (item & !u64::from(u32::MAX) == PROP_KEY).then_some(item as u32)
}

#[derive(Clone, Debug)]
pub struct CampaignProps {
    original: Arc<[u8]>,
    file: Arc<MarkersFile>,
    models: Arc<Vec<String>>,
    positions: BTreeMap<u32, [f32; 3]>,
}

impl CampaignProps {
    pub fn parse(bytes: Arc<[u8]>) -> Result<Self, String> {
        let file = MarkersFile::parse(&bytes).map_err(|e| e.to_string())?;
        if file.class_name != "PROP_MARKER" || !matches!(file.version, 4 | 6 | 9) {
            return Err("prop translation supports PROP_MARKER versions 4, 6 and 9".into());
        }
        // The existing writer must preserve every byte before it can be used for an edit.
        if file.write().as_slice() != bytes.as_ref() {
            return Err("marker representation cannot be written losslessly".into());
        }
        let mut models = vec![String::new(); file.records.len()];
        for group in file.groups.iter().flatten() {
            let [KeyPart::Wide(key)] = group.key.as_slice() else {
                return Err("ambiguous prop model group".into());
            };
            let model = String::from_utf16(key).map_err(|_| "invalid prop model name")?;
            if model.is_empty() { return Err("empty prop model name".into()); }
            for &id in &group.indices {
                let target = models.get_mut(id as usize).ok_or("prop group index is out of range")?;
                if !target.is_empty() { return Err("prop belongs to more than one model group".into()); }
                target.clone_from(&model);
            }
        }
        if models.iter().any(String::is_empty) { return Err("prop has no unique model group".into()); }
        Ok(Self { original: bytes, file: Arc::new(file), models: Arc::new(models), positions: BTreeMap::new() })
    }

    pub fn len(&self) -> usize { self.models.len() }
    pub fn is_empty(&self) -> bool { self.models.is_empty() }
    pub fn model(&self, id: u32) -> Option<&str> { self.models.get(id as usize).map(String::as_str) }
    pub fn changed(&self) -> impl Iterator<Item = u32> + '_ { self.positions.keys().copied() }

    pub fn matrix(&self, id: u32) -> Result<[f32; 16], String> {
        self.model(id).ok_or("no such prop record")?;
        let fields = self.file.fields(id as usize).map_err(|e| e.to_string())?;
        let mut matrix = fields.into_iter().find_map(|(name, v)| match v {
            Value::Matrix(m) if name == "transform" => Some(m), _ => None,
        }).ok_or("prop has no transform")?;
        if let Some(position) = self.positions.get(&id) { matrix[12..15].copy_from_slice(position); }
        Ok(matrix)
    }

    pub fn position(&self, id: u32) -> Result<[f32; 3], String> {
        let m = self.matrix(id)?;
        Ok([m[12], m[13], m[14]])
    }

    /// Validates before mutation. Numeric no-ops retain the original bits (including signed zero).
    pub fn translate(&mut self, id: u32, position: [f32; 3]) -> Result<bool, String> {
        let before = self.position(id)?;
        if !position.iter().chain(before.iter()).all(|v| v.is_finite()) {
            return Err("prop position must be finite".into());
        }
        if before == position { return Ok(false); }
        let record = &self.file.records[id as usize];
        let original = std::array::from_fn(|i| f32::from_le_bytes(record[48 + i * 4..52 + i * 4].try_into().unwrap()));
        if position == original { self.positions.remove(&id); }
        else { self.positions.insert(id, position); }
        Ok(true)
    }

    pub fn write(&self) -> Arc<[u8]> {
        if self.positions.is_empty() { return self.original.clone(); }
        let mut file = self.file.as_ref().clone();
        for (&id, position) in &self.positions {
            for (i, value) in position.iter().enumerate() {
                file.records[id as usize][48 + i * 4..52 + i * 4].copy_from_slice(&value.to_le_bytes());
            }
        }
        Arc::from(file.write())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wce_formats::markers::MarkerGroup;
    fn fixture(version: u32) -> MarkersFile {
        let mut record = vec![0; match version {4=>69,6=>74,_=>79}];
        for i in 0..16 { record[i*4..i*4+4].copy_from_slice(&(i as f32 * 0.25).to_le_bytes()); }
        record[64] = 17;
        MarkersFile { repository: "BASE_MARKER_REPOSITORY".into(), repository_id: 645563,
            repository_u32: 1, repository_u16: 1, class_id: 645566, class_name: "PROP_MARKER".into(), version,
            records: vec![record.clone(), record], groups: Some(vec![MarkerGroup {
                key: vec![KeyPart::Wide("model.rigid_model_v2".encode_utf16().collect())], indices: vec![0,1],
            }]) }
    }
    #[test]
    fn both_profiles_preserve_noop_and_only_translate_selected_record() {
        for version in [4,6,9] {
            let original: Arc<[u8]> = Arc::from(fixture(version).write());
            let mut p = CampaignProps::parse(original.clone()).unwrap();
            assert_eq!(p.write(), original);
            let before = p.position(1).unwrap();
            assert!(!p.translate(1, before).unwrap());
            p.translate(1, [10.,20.,30.]).unwrap();
            let file = MarkersFile::parse(&p.write()).unwrap();
            let mut expected = fixture(version);
            expected.records[1][48..60].copy_from_slice(&[10f32,20.,30.].iter().flat_map(|v| v.to_le_bytes()).collect::<Vec<_>>());
            assert_eq!(file, expected);
            p.translate(1, before).unwrap();
            assert_eq!(p.write(), original);
        }
    }
    #[test]
    fn refusal_is_immutable_and_unknown_profiles_and_groups_remain_unsupported() {
        let bytes: Arc<[u8]> = Arc::from(fixture(4).write());
        let mut p = CampaignProps::parse(bytes.clone()).unwrap();
        for position in [[f32::NAN,0.,0.], [0.,f32::INFINITY,0.]] { assert!(p.translate(0,position).is_err()); }
        assert!(p.translate(2,[1.;3]).is_err());
        assert_eq!(p.write(),bytes);
        let mut f=fixture(4); f.groups.as_mut().unwrap()[0].indices.push(0);
        assert!(CampaignProps::parse(Arc::from(f.write())).is_err());
        let mut f=fixture(4); f.groups=None;
        assert!(CampaignProps::parse(Arc::from(f.write())).is_err());
        let mut f=fixture(4); f.version=5; f.records.clear();
        assert!(CampaignProps::parse(Arc::from(f.write())).is_err());
    }
}
