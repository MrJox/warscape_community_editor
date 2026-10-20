//! A building piece's collapse between two destruction levels, played when its level goes up (an edit, an undo or a redo): the game's
//! collapse animation instead of an abrupt change, with the effects that go with it. Lowering a level is instant.
//!
//! The collapse from level L to L + 1 is the piece's `..._destruct{L+1}_anim` (wce_formats::anim; "building" skeleton): its mesh's blocks
//! each follow one bone (their material's `i_matrix_index`), stored in the bone's frame. It is drawn with one entity per bone (the model
//! cache's `<file>#bone=K`), moved every frame to the piece's world matrix times the bone's; the standing piece is hidden meanwhile, and shows
//! its new level at the end. Skipping levels chains the collapses (0 -> 2 plays `destruct01_anim`, then `destruct02_anim`). A level
//! without a collapse animation (most houses, the siege engines' weighted collapses) changes at once, with its effects.
//!
//! The effects of a collapse step, each spawned at its start time and removed a while after (the particles live on for seconds):
//! - the collapse's own `<anim>.meta`: its dust (`VOLUMETRIC_EFFECT`, `destruction_vfx_area_dust_*`, over the volume) and its effects;
//! - the new level's standing `.meta`: its bursts (`VOLUMETRIC_EFFECT` such as `destruction_vfx_generic_rigid_stone`, untagged
//!   `destruction_vfx_*` explosions), at the step's start;
//! - from both, the `fire_transition` effects when the building burns (its `burn` property).
//!
//! ASSUMPTIONS (docs/re/buildings.md): the new level's bursts play when the collapse starts; the collapse's last frame meets the next level's
//! standing mesh; an effect's bone (rarely set) is not followed; the sounds are not played.

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::Instant;

use wce_formats::anim::Animation;
use wce_formats::anim_meta::{MetaEffect, meta_effects};
use wce_foundation::math::{Float3, Float4x4, translation};
use wce_foundation::log;
use wce_scene::render_scene::{EntityId, LayerId, MODEL_BONE_SUFFIX, MeshInstance, Payload, RenderScene, VfxEmitter};

use super::columns::quat_matrix;
use super::index::{destruct_number, plain_piece, working_anim};
use super::view::piece_local;
use super::{Tile, item_of};

/// How long an effect of a collapse stays after it started (seconds): the dust's particles live up to 8 s after their short emitters.
const EFFECT_LINGER_S: f32 = 15.0;

/// A collapse animation: its mesh file and its decoded frames.
pub(crate) struct CollapseAnim {
    mesh: String,
    anim: Animation,
}

/// The step being played (from a level to the next).
struct Step {
    anim: Option<Arc<CollapseAnim>>,
    start: Instant,

    /// The bone entities (consecutive ids from the first).
    bones: std::ops::Range<EntityId>,

    /// The effects still to start, by start time (seconds into the step).
    effects: VecDeque<MetaEffect>,
}

/// A piece collapsing towards `target`.
struct Collapse {
    elem: usize,
    piece: usize,

    /// The levels still to collapse from, after the current step.
    next: VecDeque<i64>,
    target: i64,
    step: Option<Step>,
}

#[derive(Default)]
pub(crate) struct CollapseState {
    active: Vec<Collapse>,

    /// Collapse mesh file -> its animation (None: no animation this reads).
    anims: HashMap<String, Option<Arc<CollapseAnim>>>,

    /// `.meta` path -> its entries (None: no file, or one this doesn't read).
    metas: HashMap<String, Option<Arc<Vec<MetaEffect>>>>,

    /// The collapses' effect entities and when they go.
    effects: Vec<(EntityId, Instant)>,
}

/// The column-vector matrix of a bone's local transform.
fn bone_matrix(t: [f32; 3], q: [f32; 4]) -> Float4x4 {
    let mut m = quat_matrix(q);
    for (r, v) in t.into_iter().enumerate() {
        m.m[r][3] = v;
    }

    m
}

/// The effects of a collapse step's `.meta` entries that play: the untagged ones, and `fire_transition` when burning.
fn step_effect(e: &MetaEffect, burning: bool) -> bool {
    match e.tag.as_str() {
        "" => true,
        "fire_transition" => burning,
        _ => false,
    }
}

/// Of a standing piece's `.meta`, what plays when the piece reaches its level: its bursts (volumes, `destruction_vfx_*`).
fn burst_effect(e: &MetaEffect, burning: bool) -> bool {
    step_effect(e, burning) && (e.volume.is_some() || e.effect.starts_with("destruction_vfx"))
}

impl Tile {
    /// The level a piece collapses to, while it collapses.
    pub(crate) fn collapse_target(&self, elem: usize, piece: usize) -> Option<i64> {
        self.collapses.active.iter().find(|c| c.elem == elem && c.piece == piece).map(|c| c.target)
    }

    /// The level a piece entity shows (its model's `destructNN`), when it can tell.
    pub(crate) fn shown_level(scene: &RenderScene, id: EntityId) -> Option<i64> {
        match &scene.find(id)?.payload {
            Payload::MeshInstance(mi) => {
                let file = &mi.model[mi.model.rfind('/').map_or(0, |s| s + 1)..];
                Some(i64::from(destruct_number(file)) - 1).filter(|&l| l >= 0)
            }

            _ => None,
        }
    }

    /// Plays piece `piece` of `elem` collapsing from level `from` to `to` (chained after a collapse it is already playing).
    pub(crate) fn begin_collapse(&mut self, elem: usize, piece: usize, from: i64, to: i64) {
        if let Some(c) = self.collapses.active.iter_mut().find(|c| c.elem == elem && c.piece == piece) {
            c.next.extend(c.target.max(from)..to);
            c.target = c.target.max(to);
            return;
        }

        self.collapses.active.push(Collapse { elem, piece, next: (from..to).collect(), target: to, step: None });
    }

    /// Stops a piece's collapse (its level went down, or it was hidden): its bones go, the standing piece shows again.
    pub(crate) fn cancel_collapse(&mut self, scene: &mut RenderScene, elem: usize, piece: usize) {
        let Some(at) = self.collapses.active.iter().position(|c| c.elem == elem && c.piece == piece) else { return };
        let c = self.collapses.active.remove(at);
        if let Some(step) = c.step {
            scene.remove_range(step.bones);
        }
    }

    /// Forgets the collapses (the scene they were in is gone).
    pub(crate) fn reset_collapses(&mut self) {
        self.collapses.active.clear();
        self.collapses.effects.clear();
    }

    /// Whether a collapse is playing (the view has to be drawn again).
    pub fn collapsing(&self) -> bool {
        !self.collapses.active.is_empty() || !self.collapses.effects.is_empty()
    }

    /// A `.meta`'s entries (cached).
    fn meta_entries(&mut self, path: &str) -> Option<Arc<Vec<MetaEffect>>> {
        if let Some(m) = self.collapses.metas.get(path) {
            return m.clone();
        }

        let found = self.files.as_deref().filter(|f| f.exists(path)).and_then(|f| f.read(path).ok()).and_then(|bytes| {
            let e = meta_effects(&bytes);
            if e.is_none() {
                log::warn(format!("{path}: an entry this does not read (its effects are not shown)"));
            }

            e.map(Arc::new)
        });

        self.collapses.metas.insert(path.to_owned(), found.clone());
        found
    }

    /// The collapse animation of a standing piece file (`..._destruct01.rigid_model_v2` -> `..._destruct01_anim`), cached.
    fn collapse_anim(&mut self, standing: &str) -> Option<Arc<CollapseAnim>> {
        let plain = if working_anim(standing) { plain_piece(standing) } else { standing.to_owned() };
        let stem = format!("{}_anim", plain.strip_suffix(".rigid_model_v2").unwrap_or(&plain));
        if let Some(a) = self.collapses.anims.get(&stem) {
            return a.clone();
        }

        let mesh = format!("{stem}.rigid_model_v2");
        let files = self.files.as_deref();
        let found = files.filter(|f| f.exists(&mesh)).and_then(|f| f.read(&format!("{stem}.anim")).ok()).and_then(|bytes| {
            let anim = Animation::decode(&bytes, usize::MAX);
            if anim.is_none() {
                log::warn(format!("{stem}.anim: not an animation this reads (the collapse is not played)"));
            }

            anim.map(|anim| Arc::new(CollapseAnim { mesh: mesh.clone(), anim }))
        });

        self.collapses.anims.insert(stem, found.clone());
        found
    }

    /// The world matrix of piece `piece` of `elem` (its building's, times its own placement).
    fn piece_world(&self, elem: usize, piece: usize) -> Float4x4 {
        let p = self.piece_props.get(&(elem, piece)).cloned().unwrap_or_default();
        self.world_matrix(elem) * piece_local(&p)
    }

    /// The standing file of piece `piece` of `elem` at `level`.
    fn standing_file(&self, elem: usize, piece: usize, level: i64) -> Option<String> {
        let m = self.model_of(elem)?;
        let base = m.pieces.get(piece)?.clone();
        let levels = m.piece_levels.get(piece).copied().unwrap_or(m.levels);
        Some(self.piece_file(&base, level, levels))
    }

    /// Whether the building of `elem` (or the piece) burns.
    fn burning(&self, elem: usize, piece: usize) -> bool {
        self.tscene().doc().get(elem, "burn").is_some_and(|v| v.trim() == "true") || self.piece_props.get(&(elem, piece)).is_some_and(|p| p.burn)
    }

    /// Starts the step from `level`: its bones, and its effects queued.
    fn start_step(&mut self, scene: &mut RenderScene, layer: LayerId, elem: usize, piece: usize, level: i64, now: Instant) -> Step {
        let burning = self.burning(elem, piece);
        let world = self.piece_world(elem, piece);
        let standing = self.standing_file(elem, piece, level);
        let anim = standing.as_deref().and_then(|s| self.collapse_anim(s));
        let mut effects = vec![];

        // The new level's bursts, at the start.
        if let Some(next) = self.standing_file(elem, piece, level + 1) {
            let meta = format!("{}.meta", next.strip_suffix(".rigid_model_v2").unwrap_or(&next));
            if let Some(m) = self.meta_entries(&meta) {
                effects.extend(m.iter().filter(|e| burst_effect(e, burning)).cloned().map(|mut e| {
                    e.start = 0.0;
                    e
                }));
            }
        }

        // The collapse's own, at their times.
        let mut first = scene.next_id();
        let mut count = 0;
        if let Some(a) = &anim {
            let meta = format!("{}.meta", a.mesh.strip_suffix(".rigid_model_v2").unwrap_or(&a.mesh));
            if let Some(m) = self.meta_entries(&meta) {
                effects.extend(m.iter().filter(|e| step_effect(e, burning)).cloned());
            }

            let pose = self.bone_worlds(a, 0.0);
            let hidden = self.display.hidden_types.contains("BUILDING_INSTANCE");
            first = scene.next_id();
            for (k, bone) in pose.iter().enumerate() {
                let mi = MeshInstance { model: format!("{}{MODEL_BONE_SUFFIX}{k}", a.mesh), lod: true, p5: [0.0, 0.0, 0.0, 1.0], ..Default::default() };
                let id = scene.add(layer, world * *bone, Payload::MeshInstance(mi), self.source(item_of(elem)));
                if let Some(en) = scene.find_mut(id) {
                    en.locked = true;
                    en.visible = !hidden;
                }

                count += 1;
            }
        }

        effects.sort_by(|a, b| a.start.total_cmp(&b.start));
        Step { anim, start: now, bones: first..first + count, effects: effects.into() }
    }

    /// The bones' matrices (piece space) of a collapse at `t` seconds.
    fn bone_worlds(&self, a: &CollapseAnim, t: f32) -> Vec<Float4x4> {
        let mut out: Vec<Float4x4> = Vec::with_capacity(a.anim.parents.len());
        for (i, (t, q)) in a.anim.sample(t).into_iter().enumerate() {
            let local = bone_matrix(t, q);
            let world = match usize::try_from(a.anim.parents[i]).ok().filter(|&p| p < i) {
                Some(p) => out[p] * local,
                None => local,
            };

            out.push(world);
        }

        out
    }

    /// Spawns a collapse's effect on a piece.
    fn spawn_effect(&mut self, scene: &mut RenderScene, layer: LayerId, elem: usize, world: Float4x4, fx: &MetaEffect, now: Instant) {
        let at = world * translation(Float3::new(fx.position[0], fx.position[1], fx.position[2])) * quat_matrix(fx.rotation);
        let vfx = VfxEmitter { effect: fx.effect.clone(), scale: fx.scale / self.unit() as f32, volume: fx.volume };
        let id = scene.add(layer, at, Payload::VfxEmitter(vfx), self.source(item_of(elem)));
        if let Some(en) = scene.find_mut(id) {
            en.locked = true;
        }

        let linger = EFFECT_LINGER_S + (fx.end - fx.start).max(0.0);
        self.collapses.effects.push((id, now + std::time::Duration::from_secs_f32(linger)));
    }

    /// Per frame: plays the collapses (moves their bones, starts their effects, chains their steps) and removes the effects that are over.
    pub(crate) fn update_collapses(&mut self, scene: &mut RenderScene, layer: LayerId) {
        let now = Instant::now();
        let before = self.collapses.effects.len();
        self.collapses.effects.retain(|&(id, until)| {
            if until > now {
                return true;
            }

            scene.remove(id);
            false
        });

        if self.collapses.effects.len() != before {
            scene.touch();
        }

        if self.collapses.active.is_empty() {
            return;
        }

        let mut active = std::mem::take(&mut self.collapses.active);
        active.retain_mut(|c| {
            let world = self.piece_world(c.elem, c.piece);
            loop {
                if c.step.is_none() {
                    let Some(level) = c.next.pop_front() else { break };
                    c.step = Some(self.start_step(scene, layer, c.elem, c.piece, level, now));
                }

                let Some(step) = c.step.as_mut() else { break };
                let t = now.duration_since(step.start).as_secs_f32();
                while step.effects.front().is_some_and(|e| e.start <= t) {
                    let fx = step.effects.pop_front().expect("a front");
                    self.spawn_effect(scene, layer, c.elem, world, &fx, now);
                }

                let duration = step.anim.as_ref().map_or(0.0, |a| a.anim.duration());
                if t < duration {
                    if let Some(a) = step.anim.clone() {
                        for (id, bone) in step.bones.clone().zip(self.bone_worlds(&a, t)) {
                            scene.update_world(id, world * bone);
                        }
                    }

                    return true;
                }

                // The step is over: its remaining effects start now, its bones go.
                let step = c.step.take().expect("a step");
                for fx in &step.effects {
                    self.spawn_effect(scene, layer, c.elem, world, fx, now);
                }

                scene.remove_range(step.bones);
            }

            // Done: the standing piece shows its level.
            let id = self.first_entity.get(c.elem).copied().filter(|&f| f != 0).map(|f| f + c.piece as EntityId);
            let visible = self.piece_props.get(&(c.elem, c.piece)).is_none_or(|p| p.visible) && !self.display.hidden_types.contains("BUILDING_INSTANCE");
            if let Some(en) = id.and_then(|id| scene.find_mut(id)) {
                en.visible = visible;
                scene.touch();
            }

            false
        });

        // Collapses begun while these played (none: begin_collapse runs from flush_pieces, not from here).
        active.append(&mut self.collapses.active);
        self.collapses.active = active;
    }
}
