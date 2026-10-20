//! Wall bridging columns: what a wall shows at its ends when it is damaged, so that the standing neighbour's cut is covered.
//!
//! A building's `<key>_tech.cs2.parsed` (its logic file) lists per piece the destruction states (`piece01_destruct01`, ...), and a state
//! can attach other buildings: `file_refs` whose key is `<state>_file:<building>` with a transform in the piece's frame. The walls use it
//! for their bridging columns: `roman_fort_straight`'s rubble (`piece01_destruct03`) attaches `roman_fort_bridgingcolumn_right` at x = -10
//! and `roman_fort_bridgingcolumn_left` at x = +10, its two ends. A column is a building of its own, standing (`destruct01`) or broken
//! (`destruct02`).
//!
//! The rule (the user's description of the game): a damaged wall shows its columns, so a pillar stands between it and an intact
//! neighbour; once both walls on either side of a joint are destroyed, the pillar between them is destroyed too.
//! ASSUMPTION: the engine's pairing of neighbours was not traced; here two walls are neighbours at a joint when both attach a column there
//! (their columns within 0.5 m of each other): every column of such a joint is drawn broken, a column alone stands. The two broken
//! columns of a joint are halves of one: barbarian_fort_bridgingcolumn_left's broken state is only a trench of disturbed soil (a terrain
//! blend mesh), the _right one's holds the splintered posts and the rubble; drawing only one of them left a hole at the joint.
//!
//! The same mechanism attaches other buildings too: the walls' `torch_sconce` torches (on their intact states), drawn the same way.
//!
//! The columns follow the walls live: their placement is recomputed when the walls' transforms, levels or visibility change.

use std::collections::HashMap;
use std::io::Cursor;
use std::sync::Arc;

use rpfm_lib::files::cs2_parsed::Cs2Parsed;
use rpfm_lib::files::{Decodeable, DecodeableExtraData};
use wce_foundation::math::{Float3, Float4x4};
use wce_foundation::{log, wce_prof};
use wce_scene::render_scene::{EntityId, LayerId, MeshInstance, Payload, RenderScene};

use super::index::{destruct_number, static_piece};
use super::view::{piece_local, piece_model};
use super::{Tile, item_of};

/// Two columns closer than this (metres) are the same joint.
const JOINT_TOLERANCE_M: f32 = 0.5;

/// A column a destruction state attaches: the building and its transform in the piece's frame (metres).
#[derive(Clone, Debug)]
struct ColumnRef {
    key: String,
    local: Float4x4,
}

/// A building's attached columns: piece name (`piece01`) -> per destruction index, the columns.
type Columns = HashMap<String, Vec<Vec<ColumnRef>>>;

/// A column building's standing piece files and their level counts.
#[derive(Clone, Debug, Default)]
struct ColumnModel {
    pieces: Vec<(String, u32)>,
}

#[derive(Default)]
pub(crate) struct ColumnState {
    /// Building key -> its columns (None: no logic file, or nothing attached).
    logic: HashMap<String, Option<Arc<Columns>>>,
    models: HashMap<String, Option<Arc<ColumnModel>>>,
    entities: Vec<EntityId>,

    /// What the entities were built from.
    signature: u64,
}

/// A row-vector transform of the logic file (translation in m30..m32) as the editor's column-vector matrix.
fn transform_of(t: &serde_json::Value) -> Option<Float4x4> {
    let g = |r: usize, c: usize| t.get(format!("m{r}{c}")).and_then(serde_json::Value::as_f64).map(|v| v as f32);
    let mut m = Float4x4::IDENTITY;
    for r in 0..4 {
        for c in 0..4 {
            m.m[c][r] = g(r, c)?;
        }
    }

    Some(m)
}

/// The columns of a logic file.
fn parse_columns(bytes: &[u8]) -> Result<Columns, String> {
    let parsed = Cs2Parsed::decode(&mut Cursor::new(bytes), &Some(DecodeableExtraData::default())).map_err(|e| e.to_string())?;

    // rpfm_lib keeps the fields private; their serde form is public.
    let v = serde_json::to_value(&parsed).map_err(|e| e.to_string())?;
    let mut out = Columns::new();
    for piece in v.get("pieces").and_then(serde_json::Value::as_array).into_iter().flatten() {
        let Some(name) = piece.get("name").and_then(serde_json::Value::as_str) else { continue };
        let mut states: Vec<Vec<ColumnRef>> = vec![];
        for d in piece.get("destructs").and_then(serde_json::Value::as_array).into_iter().flatten() {
            let index = d.get("index").and_then(serde_json::Value::as_u64).unwrap_or(states.len() as u64) as usize;
            if states.len() <= index {
                states.resize(index + 1, vec![]);
            }

            for f in d.get("file_refs").and_then(serde_json::Value::as_array).into_iter().flatten() {
                let key = f.get("key").and_then(serde_json::Value::as_str).unwrap_or_default();
                let Some((_, building)) = key.split_once("_file:") else { continue };
                let Some(local) = f.get("transform").and_then(transform_of) else { continue };
                states[index].push(ColumnRef { key: building.to_lowercase(), local });
            }
        }

        if states.iter().any(|s| !s.is_empty()) {
            out.insert(name.to_lowercase(), states);
        }
    }

    Ok(out)
}

/// The piece name (`piece01`) of a piece file.
fn piece_name(path: &str) -> Option<String> {
    let file = path.rsplit('/').next()?;
    let at = file.find("_piece")?;
    let rest = &file[at + 1..];
    let end = rest.find('_').unwrap_or(rest.len());
    Some(rest[..end].to_owned())
}

impl Tile {

    fn building_columns(&mut self, key: &str) -> Option<Arc<Columns>> {
        if let Some(c) = self.columns.logic.get(key) {
            return c.clone();
        }

        let path = format!("rigidmodels/buildings/{key}/{key}_tech.cs2.parsed");
        let found = self.files.as_deref().filter(|f| f.exists(&path)).and_then(|f| f.read(&path).ok()).and_then(|bytes| match parse_columns(&bytes) {
            Ok(c) => (!c.is_empty()).then(|| Arc::new(c)),
            Err(error) => {
                log::warn(format!("{path}: {error} (its bridging columns are not drawn)"));
                None
            }
        });

        self.columns.logic.insert(key.to_owned(), found.clone());
        found
    }

    fn column_model(&mut self, key: &str) -> Option<Arc<ColumnModel>> {
        if let Some(m) = self.columns.models.get(key) {
            return m.clone();
        }

        let found = self.files.as_deref().and_then(|files| {
            let prefix = format!("rigidmodels/buildings/{key}/");
            let paths = files.list(&prefix, ".rigid_model_v2");
            let mut pieces = vec![];
            for p in &paths {
                let file = &p[p.rfind('/').map_or(0, |s| s + 1)..];
                if !static_piece(file) || !file.contains("destruct01") {
                    continue;
                }

                let stem = &file[..file.find("destruct01").unwrap_or(0)];
                let levels = paths.iter().filter(|q| q.contains(stem) && static_piece(q)).map(|q| destruct_number(q)).max().unwrap_or(1);
                pieces.push((p.clone(), levels));
            }

            (!pieces.is_empty()).then(|| Arc::new(ColumnModel { pieces }))
        });

        if found.is_none() {
            log::warn(format!("bridging column {key}: no model (not drawn)"));
        }

        self.columns.models.insert(key.to_owned(), found.clone());
        found
    }

    /// Every column the shown walls attach at their current levels: (world matrix, column key, owning element).
    fn placed_columns(&mut self) -> Vec<(Float4x4, String, usize)> {
        if self.display.hidden_types.contains("BUILDING_INSTANCE") {
            return vec![];
        }

        let mut out = vec![];
        for e in self.instances.clone() {
            if self.tscene().type_of(e) != "BUILDING_INSTANCE" || !self.shown(e) {
                continue;
            }

            let Some(m) = self.model_of(e).filter(|m| m.ok).cloned() else { continue };
            let Some(key) = self.tscene().doc().get(e, "key").map(|k| k.to_lowercase()) else { continue };
            let Some(columns) = self.building_columns(&key) else { continue };
            let saved = self.saved_destruction_level(e, m.levels);
            let world = self.world_matrix(e);
            for (k, piece) in m.pieces.iter().enumerate() {
                let Some(states) = piece_name(piece).and_then(|n| columns.get(&n)) else { continue };
                let props = self.piece_props(e, k).cloned();
                if props.as_ref().is_some_and(|p| !p.visible) {
                    continue;
                }

                let level = props.as_ref().map_or(saved, |p| if p.destruction_level < 0 { saved } else { p.destruction_level });
                let top = i64::from(m.piece_levels.get(k).copied().unwrap_or(m.levels).max(1)) - 1;
                let state = level.clamp(0, top) as usize;
                let piece_world = props.as_ref().map_or(world, |p| world * piece_local(p));
                for c in states.get(state).into_iter().flatten() {
                    out.push((piece_world * c.local, c.key.clone(), e));
                }
            }
        }

        out
    }

    /// Per frame: places the bridging columns again when the walls changed.
    pub(crate) fn update_columns(&mut self, scene: &mut RenderScene, layer: LayerId) {
        wce_prof!("tile.update_columns");
        let placed = self.placed_columns();
        let signature = placed.iter().fold(placed.len() as u64, |h, (w, key, e)| {
            let h = w.m.iter().flatten().fold(h, |h, v| (h ^ u64::from(v.to_bits())).wrapping_mul(1_099_511_628_211));
            key.bytes().fold(h ^ *e as u64, |h, b| (h ^ u64::from(b)).wrapping_mul(1_099_511_628_211))
        });

        if signature == self.columns.signature && (placed.is_empty() || !self.columns.entities.is_empty()) {
            return;
        }

        self.columns.signature = signature;
        for &id in &self.columns.entities {
            scene.remove(id);
        }

        self.columns.entities.clear();

        // The joints: columns at one spot (scene units) come from the walls on both sides.
        let tol = JOINT_TOLERANCE_M / self.unit() as f32;
        let pos = |w: &Float4x4| Float3::new(w.m[0][3], w.m[1][3], w.m[2][3]);
        let shared = (0..placed.len()).map(|i| {
            let p = pos(&placed[i].0);
            placed.iter().enumerate().any(|(j, (w, _, e))| {
                let q = pos(w);
                j != i && *e != placed[i].2 && (p.x - q.x).abs() < tol && (p.y - q.y).abs() < tol && (p.z - q.z).abs() < tol
            })
        }).collect::<Vec<_>>();

        for (i, (world, key, elem)) in placed.iter().enumerate() {
            let Some(model) = self.column_model(key) else { continue };
            let level = i64::from(shared[i]);
            for (base, levels) in &model.pieces {
                let mi = MeshInstance { model: piece_model(base, level, *levels), lod: true, p5: [0.0, 0.0, 0.0, 1.0], ..Default::default() };
                let id = scene.add(layer, *world, Payload::MeshInstance(mi), self.source(item_of(*elem)));
                if let Some(en) = scene.find_mut(id) {
                    en.locked = true;
                }

                self.columns.entities.push(id);
            }
        }

        scene.touch();
    }

    /// Forgets the columns (the scene they were in is gone).
    pub(crate) fn reset_columns(&mut self) {
        self.columns.entities.clear();
        self.columns.signature = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::piece_name;

    #[test]
    fn piece_names() {
        assert_eq!(piece_name("rigidmodels/buildings/roman_fort_straight/roman_fort_straight_piece01_destruct01.rigid_model_v2").as_deref(), Some("piece01"));
        assert_eq!(piece_name("a/b/x_piece12_destruct03_0.rigid_model_v2").as_deref(), Some("piece12"));
    }
}
