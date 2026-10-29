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
//!
//! The effects of the pieces (wce_formats::anim_meta: `<piece file>.meta`), on the buildings' own pieces and on what they attach, are
//! placed the same way: the torches' `action_vfx_torch_fire_small`, a lighthouse's fire. Shown: the untagged effects that loop (not the
//! `destruction_vfx_*` bursts a piece plays once when it reaches its level), and the `ip_fire` ones while the building burns (its `burn`
//! property). The capture smoke, the fire transitions and the timed fires belong to battle events and are not shown.
//! ASSUMPTION: an effect's `scale` is applied like a placed VFXInstance's; its bone and its string flag are not used.

use std::collections::{HashMap, HashSet};
use std::io::Cursor;
use std::sync::Arc;

use rpfm_lib::files::cs2_parsed::Cs2Parsed;
use rpfm_lib::files::{Decodeable, DecodeableExtraData};
use wce_formats::anim_meta::{MetaEffect, meta_effects};
use wce_foundation::math::{Float3, Float4x4, translation};
use wce_foundation::{log, wce_prof};
use wce_scene::render_scene::{EntityId, LayerId, MeshInstance, Payload, RenderScene, VfxEmitter};

use super::index::{copied_piece, destruct_number, piece_copy, static_piece};
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

pub(crate) struct ColumnState {
    /// Something they depend on changed (a building's transform, attributes, pieces or models, the shown types): placed again at the next
    /// update. Walking every building each frame cost ~20 ms on a city.
    pub(crate) dirty: bool,

    /// Building key -> its columns (None: no logic file, or nothing attached).
    logic: HashMap<String, Option<Arc<Columns>>>,
    models: HashMap<String, Option<Arc<ColumnModel>>>,

    /// Piece file -> the effects of its `.meta` (None: no file, or nothing shown in it).
    metas: HashMap<String, Option<Arc<Vec<MetaEffect>>>>,

    /// Building key -> whether it attaches or plays anything (a building without either is skipped every frame).
    relevant: HashMap<String, bool>,

    /// The entities placed for each building (its columns and effects) and what they show (a hash): kept while that does not change.
    by_elem: HashMap<usize, (u64, Vec<EntityId>)>,

    /// What the entities were built from.
    signature: u64,

    /// Each building's columns and pieces with effects (`building_placed`), and the buildings changed since (moved, edited): only those
    /// are walked again, not the whole city (2 ms a frame while a building was dragged). `dirty` forgets them all.
    placed: HashMap<usize, Arc<BuildingPlaced>>,
    changed: HashSet<usize>,
}

/// What a building shows (`update_columns`): a hash of it, its columns (world matrix, column key, broken or standing) and its pieces.
type Shows = (u64, Vec<(Float4x4, String, i64)>, Vec<PlacedPiece>);

/// A building's columns (world matrix, column key, owning element) and its pieces (`PlacedPiece`).
type BuildingPlaced = (Vec<(Float4x4, String, usize)>, Vec<PlacedPiece>);

impl Default for ColumnState {
    fn default() -> Self {
        Self {
            dirty: true, logic: HashMap::new(), models: HashMap::new(), metas: HashMap::new(), relevant: HashMap::new(), by_elem: HashMap::new(), signature: 0,
            placed: HashMap::new(), changed: HashSet::new(),
        }
    }
}

/// A row-vector transform of the logic file (translation in m30..m32) as the editor's column-vector matrix.
pub(super) fn transform_of(t: &serde_json::Value) -> Option<Float4x4> {
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

/// A placed piece whose effects play: its world matrix, its file, the element and whether it burns.
type PlacedPiece = (Float4x4, String, usize, bool);

/// Whether an effect of a `.meta` plays on a standing piece in the editor (see the module notes).
pub(super) fn shown_effect(e: &MetaEffect, burning: bool) -> bool {
    match e.tag.as_str() {
        "" => e.volume.is_none() && !e.effect.starts_with("destruction_vfx"),
        "ip_fire" => burning,
        _ => false,
    }
}

/// A rotation quaternion (x y z w) as a matrix.
pub(crate) fn quat_matrix(q: [f32; 4]) -> Float4x4 {
    let len = (q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3]).sqrt().max(1e-6);
    let [x, y, z, w] = q.map(|v| v / len);
    let mut m = Float4x4::IDENTITY;
    m.m[0] = [1.0 - 2.0 * (y * y + z * z), 2.0 * (x * y - z * w), 2.0 * (x * z + y * w), 0.0];
    m.m[1] = [2.0 * (x * y + z * w), 1.0 - 2.0 * (x * x + z * z), 2.0 * (y * z - x * w), 0.0];
    m.m[2] = [2.0 * (x * z - y * w), 2.0 * (y * z + x * w), 1.0 - 2.0 * (x * x + y * y), 0.0];
    m
}

/// The piece name (`piece01`) of a piece file.
pub(super) fn piece_name(path: &str) -> Option<String> {
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

    /// The effects of a piece file's `.meta` that can show (cached).
    fn piece_effects(&mut self, file: &str) -> Option<Arc<Vec<MetaEffect>>> {
        if let Some(m) = self.columns.metas.get(file) {
            return m.clone();
        }

        let path = format!("{}.meta", file.strip_suffix(".rigid_model_v2").unwrap_or(file));
        let found = self.files.as_deref().filter(|f| f.exists(&path)).and_then(|f| f.read(&path).ok()).and_then(|bytes| match meta_effects(&bytes) {
            Some(e) => {
                let e = e.into_iter().filter(|e| shown_effect(e, true)).collect::<Vec<_>>();
                (!e.is_empty()).then(|| Arc::new(e))
            }

            None => {
                log::warn(format!("{path}: an entry this does not read (its effects are not shown)"));
                None
            }
        });

        self.columns.metas.insert(file.to_owned(), found.clone());
        found
    }

    /// Whether a building attaches columns or has pieces with effects.
    fn relevant_building(&mut self, key: &str) -> bool {
        if let Some(&r) = self.columns.relevant.get(key) {
            return r;
        }

        let metas = self.files.as_deref().is_some_and(|f| f.list(&format!("rigidmodels/buildings/{key}/"), ".meta").iter().any(|p| !p.ends_with("_anim.meta")));
        let r = metas || self.building_columns(key).is_some();
        self.columns.relevant.insert(key.to_owned(), r);
        r
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
                if !static_piece(file) || !file.contains("destruct01") || piece_copy(p, |q| paths.iter().any(|x| x == q)) {
                    continue;
                }

                let stem = &file[..file.find("destruct01").unwrap_or(0)];
                let levels = paths.iter().filter(|q| q.contains(stem) && static_piece(q) && copied_piece(q).is_none()).map(|q| destruct_number(q)).max().unwrap_or(1);
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

    /// Every column the shown walls attach at their current levels: (world matrix, column key, owning element); and every shown piece of the
    /// buildings that have effects.
    fn placed_columns(&mut self) -> (Vec<(Float4x4, String, usize)>, Vec<PlacedPiece>) {
        if self.display.hidden_types.contains("BUILDING_INSTANCE") {
            return (vec![], vec![]);
        }

        let (mut out, mut pieces) = (vec![], vec![]);
        for i in 0..self.instances.len() {
            let e = self.instances[i];
            let placed = match self.columns.placed.get(&e) {
                Some(p) => p.clone(),
                None => {
                    let p = Arc::new(self.building_placed(e));
                    self.columns.placed.insert(e, p.clone());
                    p
                }
            };

            out.extend_from_slice(&placed.0);
            pieces.extend_from_slice(&placed.1);
        }

        (out, pieces)
    }

    /// Something every building's columns depend on changed (the models, the shown types, the display): all walked again.
    pub(crate) fn columns_changed(&mut self) {
        self.columns.dirty = true;
        self.columns.placed.clear();
    }

    /// An element moved or was edited: a building's columns are walked again; anything else (a group, a layer: what is in it moved too)
    /// has every building's walked again.
    pub(crate) fn columns_element_changed(&mut self, elem: usize) {
        self.columns.dirty = true;
        if elem < self.tscene().doc().elements().len() && self.tscene().type_of(elem) == "BUILDING_INSTANCE" {
            self.columns.changed.insert(elem);
        } else {
            self.columns.placed.clear();
        }
    }

    /// One instance's columns and pieces with effects (nothing for what is not a shown building that attaches or plays anything).
    fn building_placed(&mut self, e: usize) -> BuildingPlaced {
        let (mut out, mut pieces) = (vec![], vec![]);
        {
            if self.tscene().type_of(e) != "BUILDING_INSTANCE" || !self.shown(e) {
                return (out, pieces);
            }

            let Some(m) = self.model_of(e).filter(|m| m.ok).cloned() else { return (out, pieces) };
            let Some(key) = self.tscene().doc().get(e, "key").map(|k| k.to_lowercase()) else { return (out, pieces) };
            if !self.relevant_building(&key) {
                return (out, pieces);
            }

            let columns = self.building_columns(&key);
            let saved = self.saved_destruction_level(e, m.levels);
            let world = self.world_matrix(e);
            let burns = self.tscene().doc().get(e, "burn").is_some_and(|v| v.trim() == "true");
            for (k, piece) in m.pieces.iter().enumerate() {
                let props = self.piece_props(e, k).cloned();
                if props.as_ref().is_some_and(|p| !p.visible) {
                    continue;
                }

                let level = props.as_ref().map_or(saved, |p| if p.destruction_level < 0 { saved } else { p.destruction_level });
                let levels = m.piece_levels.get(k).copied().unwrap_or(m.levels).max(1);
                let state = level.clamp(0, i64::from(levels) - 1) as usize;
                let piece_world = props.as_ref().map_or(world, |p| world * piece_local(p));
                pieces.push((piece_world, self.piece_file(piece, level, levels), e, props.as_ref().map_or(burns, |p| p.burn || burns)));
                let Some(states) = piece_name(piece).and_then(|n| columns.as_ref()?.get(&n).cloned()) else { continue };
                for c in states.get(state).into_iter().flatten() {
                    out.push((piece_world * c.local, c.key.clone(), e));
                }
            }
        }

        (out, pieces)
    }

    /// Per frame: places the bridging columns again when the walls changed.
    pub(crate) fn update_columns(&mut self, scene: &mut RenderScene, layer: LayerId) {
        wce_prof!("tile.update_columns");
        if !std::mem::take(&mut self.columns.dirty) {
            return;
        }

        // Only the buildings that changed are walked again (a change of everything forgot them all: `columns_changed`).
        for e in std::mem::take(&mut self.columns.changed) {
            self.columns.placed.remove(&e);
        }

        let (placed, pieces) = {
            wce_prof!("tile.columns.gather");
            self.placed_columns()
        };
        let hash = |h: u64, w: &Float4x4, key: &str, e: usize| {
            let h = w.m.iter().flatten().fold(h, |h, v| (h ^ u64::from(v.to_bits())).wrapping_mul(1_099_511_628_211));
            key.bytes().fold(h ^ e as u64, |h, b| (h ^ u64::from(b)).wrapping_mul(1_099_511_628_211))
        };

        let signature = placed.iter().fold(placed.len() as u64, |h, (w, key, e)| hash(h, w, key, *e));
        let signature = pieces.iter().fold(signature ^ pieces.len() as u64, |h, (w, file, e, burns)| hash(h ^ u64::from(*burns), w, file, *e));
        if signature == self.columns.signature && (placed.is_empty() && pieces.is_empty() || !self.columns.by_elem.is_empty()) {
            return;
        }

        self.columns.signature = signature;

        // The joints: columns at one spot (scene units) come from the walls on both sides.
        let tol = JOINT_TOLERANCE_M / self.unit() as f32;
        let pos = |w: &Float4x4| Float3::new(w.m[0][3], w.m[1][3], w.m[2][3]);
        let shared = {
            wce_prof!("tile.columns.joints");
            (0..placed.len()).map(|i| {
                let p = pos(&placed[i].0);
                placed.iter().enumerate().any(|(j, (w, _, e))| {
                    let q = pos(w);
                    j != i && *e != placed[i].2 && (p.x - q.x).abs() < tol && (p.y - q.y).abs() < tol && (p.z - q.z).abs() < tol
                })
            }).collect::<Vec<_>>()
        };

        // What each building shows: its columns (broken where they are shared) and its pieces with effects. A building whose share did not
        // change keeps its entities: placing them all again restarted every torch and dust of the city on each frame a building moved.
        let mut shows: HashMap<usize, Shows> = HashMap::new();
        for (i, (world, key, elem)) in placed.iter().enumerate() {
            let level = i64::from(shared[i]);
            let en = shows.entry(*elem).or_insert_with(|| (0xcbf2_9ce4_8422_2325, vec![], vec![]));
            en.0 = hash(en.0 ^ level as u64, world, key, *elem);
            en.1.push((*world, key.clone(), level));
        }

        for piece in pieces {
            let en = shows.entry(piece.2).or_insert_with(|| (0xcbf2_9ce4_8422_2325, vec![], vec![]));
            en.0 = hash(en.0 ^ u64::from(piece.3) ^ 0x9e37, &piece.0, &piece.1, piece.2);
            en.2.push(piece);
        }

        let mut gone: Vec<EntityId> = vec![];
        let old = std::mem::take(&mut self.columns.by_elem);
        for (elem, (sig, ids)) in old {
            match shows.get(&elem) {
                Some(now) if now.0 == sig => {
                    self.columns.by_elem.insert(elem, (sig, ids));
                }

                _ => gone.extend(ids),
            }
        }

        gone.sort_unstable();
        let mut ranges: Vec<std::ops::Range<EntityId>> = vec![];
        for id in gone {
            match ranges.last_mut() {
                Some(r) if r.end == id => r.end = id + 1,
                _ => ranges.push(id..id + 1),
            }
        }

        scene.remove_ranges(&ranges);
        let unit = self.unit() as f32;
        for (elem, (sig, columns, pieces)) in shows {
            if self.columns.by_elem.contains_key(&elem) {
                continue;
            }

            let mut ids = vec![];
            let mut effect_pieces = pieces;
            for (world, key, level) in columns {
                let Some(model) = self.column_model(&key) else { continue };
                for (base, levels) in &model.pieces {
                    let file = piece_model(base, level, *levels);
                    let mi = MeshInstance { model: file.clone(), lod: true, p5: [0.0, 0.0, 0.0, 1.0], ..Default::default() };
                    let id = scene.add(layer, world, Payload::MeshInstance(mi), self.source(item_of(elem)));
                    if let Some(en) = scene.find_mut(id) {
                        en.locked = true;
                    }

                    ids.push(id);
                    effect_pieces.push((world, file, elem, false));
                }
            }

            // The pieces' effects, in their model space.
            for (world, file, elem, burns) in effect_pieces {
                let Some(effects) = self.piece_effects(&file) else { continue };
                for fx in effects.iter().filter(|fx| shown_effect(fx, burns)) {
                    let at = world * translation(Float3::new(fx.position[0], fx.position[1], fx.position[2])) * quat_matrix(fx.rotation);
                    let vfx = VfxEmitter { effect: fx.effect.clone(), scale: fx.scale / unit, mesh: None };
                    let id = scene.add(layer, at, Payload::VfxEmitter(vfx), self.source(item_of(elem)));
                    if let Some(en) = scene.find_mut(id) {
                        en.locked = true;
                    }

                    ids.push(id);
                }
            }

            self.columns.by_elem.insert(elem, (sig, ids));
        }

        // No touch: the removals are logged and the entities added are new to the scene's followers; a touch made the renderer build its
        // draws and every overlay again on each frame a building moved.
    }

    /// Forgets the columns (the scene they were in is gone).
    pub(crate) fn reset_columns(&mut self) {
        self.columns.by_elem.clear();
        self.columns.signature = 0;
        self.columns.dirty = true;
        self.columns.placed.clear();
        self.columns.changed.clear();
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
