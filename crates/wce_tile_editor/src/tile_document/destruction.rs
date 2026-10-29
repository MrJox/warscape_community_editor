//! The Destruction mode's preview (docs/plans/building_destruction.md): a building being destroyed is drawn from the files the generator
//! made (in `wce_vfs::generated`, under a preview key that changes with every remake, so the model cache never shows an old one) in place
//! of its own pieces, at the level the artist looks at; a collapse plays as the tile's own collapses do (a bone entity per fragment, the
//! collapse's and the new level's effects); the damage volumes are drawn as boxes. The tool asks through [`DestructionRequest`]s, which
//! the tile carries out when it publishes (it has the scene then).

use std::collections::{BTreeMap, VecDeque};
use std::time::Instant;

use wce_formats::anim::Animation;
use wce_formats::anim_meta::MetaEffect;
use wce_foundation::math::{Float3, Float4x4};
use wce_scene::render_scene::{EntityId, LayerId, MODEL_BONE_SUFFIX, MeshInstance, Overlay, Payload, RenderScene, overlay_colour};
use wce_walls::building::folder;

use super::columns::quat_matrix;
use super::{Tile, item_of};

/// A box drawn for a damage volume: centre, half size, turn about the vertical (the building's frame), and whether it is selected.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VolumeBox {
    pub centre: [f32; 3],
    pub half: [f32; 3],
    pub yaw: f32,
    pub selected: bool,
}

/// What the tool asks of the preview.
#[derive(Clone, Debug)]
pub enum DestructionRequest {
    /// Draw `elem` from these files (a building folder under `key`, pieces `pieces`) at level `level` (0 intact).
    Show { elem: usize, key: String, files: BTreeMap<String, Vec<u8>>, pieces: Vec<u32>, level: usize },

    /// Show a level (0 intact).
    Level(usize),

    /// Play the collapse into damage level `n` (from level n - 1).
    Play(usize),

    /// The damage volumes to draw.
    Volumes(Vec<VolumeBox>),

    /// Leave: the building's own pieces again, the preview's files forgotten.
    Clear,
}

/// A collapse playing.
struct Playing {
    level: usize,
    start: Instant,
    pieces: Vec<PiecePlay>,
}

struct PiecePlay {
    anim: Option<Animation>,
    bones: std::ops::Range<EntityId>,
    effects: VecDeque<MetaEffect>,
    source: String,
}

#[derive(Default)]
pub(crate) struct DestructionPreview {
    pending: Vec<DestructionRequest>,
    elem: Option<usize>,
    key: Option<String>,
    pieces: Vec<u32>,
    level: usize,

    /// The standing entities (one per piece), the overlay, and the building's own piece entities hidden (with whether they were shown).
    standing: Vec<EntityId>,
    overlay: Option<EntityId>,
    hidden: Vec<(EntityId, bool)>,
    volumes: Vec<VolumeBox>,
    playing: Option<Playing>,
}

impl Tile {
    /// Queues a request for the next publish.
    pub fn destruction_request(&mut self, r: DestructionRequest) {
        self.destruction.pending.push(r);
    }

    /// The building the preview shows, if any.
    pub fn destruction_elem(&self) -> Option<usize> {
        self.destruction.elem
    }

    /// Whether a collapse is playing in the preview.
    pub fn destruction_playing(&self) -> bool {
        self.destruction.playing.is_some()
    }

    /// The level the preview shows.
    pub fn destruction_level(&self) -> usize {
        self.destruction.level
    }

    fn preview_file(&self, piece: u32, level: usize, anim: bool) -> Option<String> {
        let key = self.destruction.key.as_ref()?;
        Some(format!("{}{key}_piece{piece:02}_destruct{:02}{}.rigid_model_v2", folder(key), level + 1, if anim { "_anim" } else { "" }))
    }

    /// Per frame (from publish): carries out the requests, plays the collapse.
    pub(crate) fn update_destruction(&mut self, scene: &mut RenderScene, layer: LayerId) {
        for r in std::mem::take(&mut self.destruction.pending) {
            match r {
                DestructionRequest::Show { elem, key, files, pieces, level } => {
                    if self.destruction.elem != Some(elem) {
                        self.restore_destroyed(scene);
                        self.hide_own_pieces(scene, elem);
                    }

                    if let Some(old) = self.destruction.key.take() {
                        wce_vfs::generated::remove_prefix(&folder(&old));
                    }

                    for (name, bytes) in files {
                        wce_vfs::generated::put(&format!("{}{name}", folder(&key)), bytes);
                    }

                    self.destruction.elem = Some(elem);
                    self.destruction.key = Some(key);
                    self.destruction.pieces = pieces;
                    self.stop_playing(scene);
                    self.show_destruction_level(scene, layer, level);
                }

                DestructionRequest::Level(level) => {
                    self.stop_playing(scene);
                    self.show_destruction_level(scene, layer, level);
                }

                DestructionRequest::Play(level) => self.start_playing(scene, layer, level),
                DestructionRequest::Volumes(v) => {
                    self.destruction.volumes = v;
                    self.draw_volumes(scene, layer);
                }

                DestructionRequest::Clear => {
                    self.stop_playing(scene);
                    self.restore_destroyed(scene);
                    if let Some(old) = self.destruction.key.take() {
                        wce_vfs::generated::remove_prefix(&folder(&old));
                    }

                    self.destruction.volumes.clear();
                    self.draw_volumes(scene, layer);
                    self.destruction.elem = None;
                }
            }
        }

        self.step_playing(scene, layer);
    }

    fn hide_own_pieces(&mut self, scene: &mut RenderScene, elem: usize) {
        let count = self.model_of(elem).map_or(0, |m| m.pieces.len());
        let Some(first) = self.first_entity.get(elem).copied().filter(|&f| f != 0) else { return };
        for k in 0..count as EntityId {
            if let Some(en) = scene.find_mut(first + k) {
                self.destruction.hidden.push((first + k, en.visible));
                en.visible = false;
            }
        }
        scene.touch();
    }

    fn restore_destroyed(&mut self, scene: &mut RenderScene) {
        for (id, visible) in std::mem::take(&mut self.destruction.hidden) {
            if let Some(en) = scene.find_mut(id) {
                en.visible = visible;
            }
        }

        for id in std::mem::take(&mut self.destruction.standing) {
            scene.remove(id);
        }
        scene.touch();
    }

    fn show_destruction_level(&mut self, scene: &mut RenderScene, layer: LayerId, level: usize) {
        for id in std::mem::take(&mut self.destruction.standing) {
            scene.remove(id);
        }

        let Some(elem) = self.destruction.elem else { return };
        self.destruction.level = level;
        let world = self.world_matrix(elem);
        let source = self.source(item_of(elem));
        for &p in &self.destruction.pieces.clone() {
            let Some(model) = self.preview_file(p, level, false) else { continue };
            if self.files.as_deref().is_some_and(|f| !f.exists(&model)) {
                continue;
            }

            let id = scene.add(layer, world, Payload::MeshInstance(MeshInstance { model, lod: true, ..Default::default() }), source);
            if let Some(en) = scene.find_mut(id) {
                en.locked = true;
            }
            self.destruction.standing.push(id);
        }
        scene.touch();
    }

    fn start_playing(&mut self, scene: &mut RenderScene, layer: LayerId, level: usize) {
        self.stop_playing(scene);
        if level == 0 {
            return;
        }

        self.show_destruction_level(scene, layer, level - 1);
        let Some(elem) = self.destruction.elem else { return };
        let world = self.world_matrix(elem);
        let source = self.source(item_of(elem));
        let mut pieces = vec![];
        for &p in &self.destruction.pieces.clone() {
            let Some(mesh) = self.preview_file(p, level - 1, true) else { continue };
            let stem = mesh.trim_end_matches(".rigid_model_v2").to_owned();
            let anim = self.files.as_deref().and_then(|f| f.read(&format!("{stem}.anim")).ok()).and_then(|b| Animation::decode(&b, usize::MAX));
            let first = scene.next_id();
            if let Some(a) = &anim {
                for k in 0..a.parents.len() {
                    let id = scene.add(layer, world, Payload::MeshInstance(MeshInstance { model: format!("{mesh}{MODEL_BONE_SUFFIX}{k}"), lod: false, ..Default::default() }), source);
                    if let Some(en) = scene.find_mut(id) {
                        en.locked = true;
                    }
                }
            }
            let bones = first..scene.next_id();

            // The new level's bursts at the start, the collapse's own at their times.
            let mut effects = vec![];
            if let Some(next) = self.preview_file(p, level, false) {
                let meta = format!("{}.meta", next.trim_end_matches(".rigid_model_v2"));
                if let Some(m) = self.meta_entries(&meta) {
                    effects.extend(m.iter().filter(|e| e.tag.is_empty()).cloned().map(|mut e| {
                        e.start = 0.0;
                        e
                    }));
                }
            }

            if anim.is_some() && let Some(m) = self.meta_entries(&format!("{stem}.meta")) {
                effects.extend(m.iter().filter(|e| e.tag.is_empty()).cloned());
            }

            effects.sort_by(|a, b| a.start.total_cmp(&b.start));
            let source = self.preview_file(p, level - 1, false).unwrap_or_default();
            pieces.push(PiecePlay { anim, bones, effects: effects.into(), source });
        }

        // The standing pieces the collapse starts from are hidden once the fragments draw (they are bone 0 of the collapse).
        for id in std::mem::take(&mut self.destruction.standing) {
            scene.remove(id);
        }

        self.destruction.playing = Some(Playing { level, start: Instant::now(), pieces });
        scene.touch();
    }

    fn stop_playing(&mut self, scene: &mut RenderScene) {
        if let Some(p) = self.destruction.playing.take() {
            for piece in p.pieces {
                scene.remove_range(piece.bones);
            }
            scene.touch();
        }
    }

    fn step_playing(&mut self, scene: &mut RenderScene, layer: LayerId) {
        let Some(mut playing) = self.destruction.playing.take() else { return };
        let Some(elem) = self.destruction.elem else { return };
        let world = self.world_matrix(elem);
        let now = Instant::now();
        let t = now.duration_since(playing.start).as_secs_f32();
        let mut running = false;
        for piece in &mut playing.pieces {
            while piece.effects.front().is_some_and(|e| e.start <= t) {
                let fx = piece.effects.pop_front().expect("a front");
                let source = piece.source.clone();
                self.spawn_effect(scene, layer, elem, world, &fx, Some(&source), now);
            }

            let Some(a) = &piece.anim else { continue };
            if t < a.duration() {
                running = true;
            }

            for (id, (tr, q)) in piece.bones.clone().zip(a.sample(t)) {
                let mut m = quat_matrix(q);
                for (r, v) in tr.into_iter().enumerate() {
                    m.m[r][3] = v;
                }
                scene.update_world(id, world * m);
            }
        }

        if running || playing.pieces.iter().any(|p| !p.effects.is_empty()) {
            self.destruction.playing = Some(playing);
            return;
        }

        for piece in playing.pieces {
            scene.remove_range(piece.bones);
        }

        self.show_destruction_level(scene, layer, playing.level);
    }

    fn draw_volumes(&mut self, scene: &mut RenderScene, layer: LayerId) {
        let mut o = Overlay { depth_test: false, pickable: false, ..Default::default() };
        if let Some(elem) = self.destruction.elem {
            let world = self.world_matrix(elem);
            for v in &self.destruction.volumes {
                let (s, c) = v.yaw.sin_cos();
                let corner = |x: f32, y: f32, z: f32| {
                    let (lx, ly, lz) = (x * v.half[0], y * v.half[1], z * v.half[2]);
                    // The volume's frame turns by -yaw into the building's (Volume::local turns by +yaw).
                    let p = Float3::new(v.centre[0] + c * lx + s * lz, v.centre[1] + ly, v.centre[2] - s * lx + c * lz);
                    world.transform_point(p)
                };
                let col = if v.selected { overlay_colour(1.0, 0.8, 0.1, 1.0) } else { overlay_colour(1.0, 0.3, 0.2, 0.9) };
                let pts = [-1.0f32, 1.0];
                for &a in &pts {
                    for &b in &pts {
                        o.line(corner(-1.0, a, b), corner(1.0, a, b), col);
                        o.line(corner(a, -1.0, b), corner(a, 1.0, b), col);
                        o.line(corner(a, b, -1.0), corner(a, b, 1.0), col);
                    }
                }
            }
        }

        match self.destruction.overlay {
            Some(id) if scene.find(id).is_some() => scene.update_payload(id, Payload::Overlay(o)),
            _ => {
                let source = self.source(0);
                let id = scene.add(layer, Float4x4::IDENTITY, Payload::Overlay(o), source);
                if let Some(en) = scene.find_mut(id) {
                    en.locked = true;
                }
                self.destruction.overlay = Some(id);
            }
        }
    }
}
