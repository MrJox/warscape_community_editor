//! The Cinematic tool's document: a scene project (`<projects>/cinematics/<key>/project.json`) in the editor's command stack, so
//! Edit > Undo / Redo, History, File > Save and the unsaved prompts work as in the other tools. An edit is a snapshot (the project
//! before and after, shared).

use std::any::Any;
use std::path::PathBuf;
use std::sync::Arc;

use wce_cinematic::scene::{Interp, PathKey, Project, TrackContent};
use wce_foundation::math::{Float3, Float4x4, rotation_y, translation};

use crate::items::{SceneItem, camera_key_of, camera_key_of_mut, vfx_of, vfx_of_mut};
use wce_editor_core::command::{Command, CommandStack, History};
use wce_editor_core::document::{Document, DocumentCore, SaveTarget};
use wce_scene::render_scene::{LayerId, RenderScene, SourceRef};

struct Snapshot {
    name: String,
    before: Arc<Project>,
    after: Arc<Project>,
    /// Non-zero: one drag (a gizmo's, a number's); its edits fold into one entry until the gesture ends.
    merge: u64,
}

impl Command<Project> for Snapshot {
    fn name(&self) -> String {
        self.name.clone()
    }
    fn apply(&mut self, p: &mut Project) {
        *p = (*self.after).clone();
    }
    fn revert(&mut self, p: &mut Project) {
        *p = (*self.before).clone();
    }
    fn merge_key(&self) -> u64 {
        self.merge
    }
    fn merge_from(&mut self, next: &dyn Command<Project>) -> bool {
        let Some(n) = next.as_any().downcast_ref::<Snapshot>() else { return false };
        self.after = n.after.clone();
        true
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// What the gizmo and the viewport need of the tool's state: the clip and playhead, where each unit stands there, the scene's units
/// per metre. The tool sets it every frame.
#[derive(Clone, Debug, Default)]
pub struct DocView {
    pub clip: Option<u32>,
    pub time: f64,
    pub units: f32,
    /// Unit id -> (place, facing) at the playhead.
    pub placed: std::collections::HashMap<u32, ([f32; 2], f32)>,
    /// Unit id -> the ground's height where it stands (metres; none: 0).
    pub heights: std::collections::HashMap<u32, f32>,
}

pub struct SceneDocument {
    core: DocumentCore,
    history: CommandStack<Project>,
    path: PathBuf,
    /// Taken back from an autosave copy: unsaved until its first save.
    restored: bool,
    pub view: DocView,
}

impl SceneDocument {
    pub fn new(project: Project, path: PathBuf) -> Self {
        Self { core: DocumentCore::new(), history: CommandStack::new(project), path, restored: false, view: DocView::default() }
    }

    /// # Errors
    ///
    /// The file can't be read or is not a project.
    pub fn open(path: PathBuf) -> Result<Self, String> {
        let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let p = Project::from_json(&text).map_err(|e| format!("{}: {e}", path.display()))?;
        let mut d = Self::new(p, path);
        d.history.mark_saved();
        Ok(d)
    }

    /// A scene taken back from its autosave copy (`copy`, the copy's project file) that saves to `path`.
    ///
    /// # Errors
    ///
    /// The copy doesn't read.
    pub fn restored(copy: &std::path::Path, path: PathBuf) -> Result<Self, String> {
        let text = std::fs::read_to_string(copy).map_err(|e| format!("{}: {e}", copy.display()))?;
        let mut d = Self::new(Project::from_json(&text)?, path);
        d.restored = true;
        Ok(d)
    }

    pub fn project(&self) -> &Project {
        self.history.model()
    }

    /// An edit: the project becomes `after` (sorted), undoable as `name`. Nothing when it is the same.
    pub fn push(&mut self, name: &str, mut after: Project) {
        after.normalize();
        if &after == self.history.model() {
            return;
        }
        let before = Arc::new(self.history.model().clone());
        self.history.push(Box::new(Snapshot { name: name.to_owned(), before, after: Arc::new(after), merge: 0 }));
    }

    /// An edit of a drag (`gesture` non-zero): the drag's edits are one undo entry, closed by [`Self::end_gesture`].
    pub fn push_merged(&mut self, name: &str, mut after: Project, gesture: u64) {
        after.normalize();
        if &after == self.history.model() {
            return;
        }
        let before = Arc::new(self.history.model().clone());
        self.history.push(Box::new(Snapshot { name: name.to_owned(), before, after: Arc::new(after), merge: gesture }));
    }

    pub fn end_gesture(&mut self) {
        self.history.break_merge();
    }

    /// Where an item is in the scene (scene units): a unit where the playhead has it, an effect at its place, an army's deployment
    /// zone at its centre.
    pub fn transform_of(&self, item: SceneItem) -> Option<Float4x4> {
        let units = self.view.units.max(1e-3);
        let p = self.project();
        let (x, y, z, yaw) = match item {
            SceneItem::Unit(id) => {
                let u = p.unit(id)?;
                // Where the playhead has it now (not last frame's: a drag edits it every frame).
                let (at, facing) = self
                    .view
                    .clip
                    .and_then(|c| p.clip(c))
                    .and_then(|c| {
                        let f = wce_cinematic::eval::evaluate(p, c, &wce_cinematic::eval::SpeedTable::default(), self.view.time);
                        f.units.iter().find(|s| s.unit == id).map(|s| (s.position, s.facing))
                    })
                    .unwrap_or((u.start, u.facing));
                (at[0], self.view.heights.get(&id).copied().unwrap_or(0.0), at[1], facing)
            }
            SceneItem::Vfx { track, index } => {
                let v = vfx_of(p, track, index)?;
                (v.position[0], v.position[1], v.position[2], v.rotation_y)
            }
            SceneItem::Army(i) => {
                let a = p.armies.get(i)?;
                (a.deployment_centre[0], 0.0, a.deployment_centre[1], a.deployment_orientation)
            }
            SceneItem::CameraKey { track, index } => {
                let k = camera_key_of(p, track, index)?;
                let yaw = (k.target[0] - k.position[0]).atan2(k.target[2] - k.position[2]);
                (k.position[0], k.position[1], k.position[2], yaw)
            }
            SceneItem::CameraTarget { track, index } => {
                let k = camera_key_of(p, track, index)?;
                (k.target[0], k.target[1], k.target[2], 0.0)
            }
            SceneItem::Camera(track) => {
                // Where it is at the playhead, looking along its aim (the gizmo turns it every way).
                let c = self.camera_now(track)?;
                return Some(look_matrix(c.position.map(|v| v / units), c.target.map(|v| v / units)));
            }
            SceneItem::Battle => return None,
        };
        Some(translation(Float3::new(x / units, y / units, z / units)) * rotation_y(yaw))
    }

    /// A camera's view at the playhead (metres).
    pub fn camera_now(&self, track: u32) -> Option<wce_cinematic::eval::CameraState> {
        let keys = match &crate::items::camera_track_of(self.project(), track)?.content {
            TrackContent::Camera { keys } => keys,
            _ => return None,
        };
        wce_cinematic::eval::camera_at(keys, self.view.time)
    }

    /// Moves and turns items by `delta` (scene units). A unit with a path in the clip shown is keyed there at the playhead (its
    /// start stays); without one, its start moves. Turning changes where it faces at the start.
    pub fn move_items(&mut self, items: &[SceneItem], delta: &Float4x4, gesture: u64) {
        let units = self.view.units.max(1e-3);
        let (clip, t) = (self.view.clip, self.view.time);
        let turn = delta.m[0][2].atan2(delta.m[2][2]);
        let moved: Vec<(SceneItem, Float4x4)> = items.iter().filter_map(|&i| Some((i, *delta * self.transform_of(i)?))).collect();
        let mut p = self.project().clone();
        for (item, m) in moved {
            let at = [m.m[0][3] * units, m.m[1][3] * units, m.m[2][3] * units];
            match item {
                SceneItem::Unit(id) => {
                    let path_track = clip.and_then(|c| p.clip(c)).and_then(|c| {
                        c.tracks.iter().find_map(|tr| match &tr.content {
                            TrackContent::Unit { unit, path, .. } if *unit == id && !path.is_empty() => Some(tr.id),
                            _ => None,
                        })
                    });
                    match (clip, path_track) {
                        (Some(c), Some(track)) => {
                            if let Some(tr) = p.clip_mut(c).and_then(|c| c.tracks.iter_mut().find(|tr| tr.id == track))
                                && let TrackContent::Unit { path, .. } = &mut tr.content
                            {
                                match path.iter_mut().find(|k| (k.t - t).abs() < 1e-3) {
                                    Some(k) => k.position = [at[0], at[2]],
                                    None => path.push(PathKey { t, position: [at[0], at[2]], interp: Interp::Linear, gait: None }),
                                }
                            }
                        }
                        _ => {
                            if let Some(u) = p.units.iter_mut().find(|u| u.id == id) {
                                u.start = [at[0], at[2]];
                            }
                        }
                    }
                    if turn.abs() > 1e-5
                        && let Some(u) = p.units.iter_mut().find(|u| u.id == id)
                    {
                        u.facing = wrap_angle(u.facing + turn);
                    }
                }
                SceneItem::Vfx { track, index } => {
                    if let Some(v) = vfx_of_mut(&mut p, track, index) {
                        v.position = at;
                        v.rotation_y = wrap_angle(v.rotation_y + turn);
                    }
                }
                SceneItem::Army(i) => {
                    if let Some(a) = p.armies.get_mut(i) {
                        a.deployment_centre = [at[0], at[2]];
                        a.deployment_orientation = wrap_angle(a.deployment_orientation + turn);
                    }
                }
                SceneItem::CameraKey { track, index } => {
                    if let Some(k) = camera_key_of_mut(&mut p, track, index) {
                        // The target keeps its offset from the camera, turned with it.
                        let off = [k.target[0] - k.position[0], k.target[1] - k.position[1], k.target[2] - k.position[2]];
                        let (s, c) = turn.sin_cos();
                        let off = [off[0] * c + off[2] * s, off[1], -off[0] * s + off[2] * c];
                        k.position = at;
                        k.target = [at[0] + off[0], at[1] + off[1], at[2] + off[2]];
                    }
                }
                SceneItem::CameraTarget { track, index } => {
                    if let Some(k) = camera_key_of_mut(&mut p, track, index) {
                        k.target = at;
                    }
                }
                SceneItem::Camera(track) => {
                    // Keyed at the playhead (a key there takes it; else a new one), its aim along the moved matrix's forward, as far
                    // ahead as it was.
                    let Some(now) = self.camera_now(track) else { continue };
                    let reach = (0..3).map(|i| (now.target[i] - now.position[i]).powi(2)).sum::<f32>().sqrt().max(1.0);
                    let f = [m.m[0][2], m.m[1][2], m.m[2][2]];
                    let len = (f[0] * f[0] + f[1] * f[1] + f[2] * f[2]).sqrt().max(1e-6);
                    let target = [at[0] + f[0] / len * reach, at[1] + f[1] / len * reach, at[2] + f[2] / len * reach];
                    if let Some(tr) = p.clips.iter_mut().flat_map(|c| &mut c.tracks).find(|tr| tr.id == track)
                        && let TrackContent::Camera { keys } = &mut tr.content
                    {
                        match keys.iter_mut().find(|k| (k.t - t).abs() < 1e-3) {
                            Some(k) => {
                                k.position = at;
                                k.target = target;
                            }
                            None => {
                                keys.push(wce_cinematic::scene::CameraKey {
                                    t,
                                    position: at,
                                    target,
                                    fov: now.fov,
                                    interp: Interp::Smooth,
                                });
                                keys.sort_by(|a, b| a.t.total_cmp(&b.t));
                            }
                        }
                    }
                }
                SceneItem::Battle => {}
            }
        }
        self.push_merged("move", p, gesture.max(1));
    }
}

/// The matrix of something at `pos` looking at `target`: its z axis forward, y up, x to its right (as `rotation_y` turns +z).
pub fn look_matrix(pos: [f32; 3], target: [f32; 3]) -> Float4x4 {
    let norm = |v: [f32; 3]| {
        let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
        if l < 1e-6 { None } else { Some([v[0] / l, v[1] / l, v[2] / l]) }
    };
    let cross = |a: [f32; 3], b: [f32; 3]| [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]];
    let f = norm([target[0] - pos[0], target[1] - pos[1], target[2] - pos[2]]).unwrap_or([0.0, 0.0, 1.0]);
    let r = norm(cross([0.0, 1.0, 0.0], f)).unwrap_or([1.0, 0.0, 0.0]);
    let u = cross(f, r);
    Float4x4 { m: [[r[0], u[0], f[0], pos[0]], [r[1], u[1], f[1], pos[1]], [r[2], u[2], f[2], pos[2]], [0.0, 0.0, 0.0, 1.0]] }
}

/// An angle in (-pi, pi].
pub fn wrap_angle(a: f32) -> f32 {
    let tau = std::f32::consts::TAU;
    let r = (a + std::f32::consts::PI).rem_euclid(tau) - std::f32::consts::PI;
    if r <= -std::f32::consts::PI { r + tau } else { r }
}

impl Document for SceneDocument {
    fn core(&self) -> &DocumentCore {
        &self.core
    }
    fn core_mut(&mut self) -> &mut DocumentCore {
        &mut self.core
    }
    fn history(&self) -> &dyn History {
        &self.history
    }
    fn history_mut(&mut self) -> &mut dyn History {
        &mut self.history
    }
    fn title(&self) -> String {
        self.project().name.clone()
    }
    fn kind(&self) -> String {
        "cinematic".to_owned()
    }
    fn can_save(&self) -> bool {
        true
    }
    fn modified(&self) -> bool {
        self.restored || self.history.modified()
    }
    /// The scene brings its own ground (the preview's): the weather tool's sample map stays hidden while it is open.
    fn shows_scene(&self) -> bool {
        true
    }
    fn autosave_meta(&self) -> Option<wce_editor_core::autosave::AutosaveMeta> {
        Some(wce_editor_core::autosave::AutosaveMeta {
            kind: self.kind(),
            title: self.title(),
            target: self.path.clone(),
            open: self.path.display().to_string(),
            from_packs: false,
            inner: "project.json".into(),
            ..Default::default()
        })
    }
    fn autosave(&mut self, dir: &std::path::Path) -> Result<(), String> {
        let to = dir.join("project.json");
        std::fs::write(&to, self.project().to_json()).map_err(|e| format!("cannot write {}: {e}", to.display()))
    }
    fn save(&mut self, target: &SaveTarget) -> Result<(), String> {
        let to = if target.path.as_os_str().is_empty() { self.path.clone() } else { target.path.clone() };
        if let Some(dir) = to.parent() {
            std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        }
        wce_foundation::files::write_replacing(&to, self.project().to_json().as_bytes()).map_err(|e| format!("{}: {e}", to.display()))?;
        if wce_editor_core::document::same_path(&to, &self.path) {
            self.history.mark_saved();
            self.restored = false;
        }
        Ok(())
    }
    fn file_path(&self) -> Option<PathBuf> {
        Some(self.path.clone())
    }
    fn source_key(&self) -> String {
        self.path.display().to_string().replace('\\', "/").to_lowercase()
    }
    fn opened_from(&self, path: &str) -> bool {
        wce_editor_core::document::same_path(std::path::Path::new(path), &self.path)
    }
    fn describe(&self, item: SourceRef) -> String {
        SceneItem::decode(item.item).map_or_else(String::new, |i| i.describe(self.project(), str::to_owned))
    }
    fn item_transform(&self, item: SourceRef) -> Option<Float4x4> {
        self.transform_of(SceneItem::decode(item.item)?)
    }
    fn transform_items(&mut self, items: &[SourceRef], delta: &Float4x4, gesture: u64) {
        let items: Vec<SceneItem> = items.iter().filter_map(|r| SceneItem::decode(r.item)).collect();
        self.move_items(&items, delta, gesture);
    }
    /// The 3D preview draws the scene itself (the tool), not through the document's layer.
    fn publish(&mut self, _scene: &mut RenderScene, _layer: LayerId) {}
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unit(id: u32) -> wce_cinematic::scene::SceneUnit {
        wce_cinematic::scene::SceneUnit {
            id,
            name: "u".into(),
            unit: "x".into(),
            army: 0,
            start: [0.0, 0.0],
            facing: 0.0,
            width: 30.0,
            men: 10,
            seed: 1,
            general: false,
            command: true,
            character: String::new(),
        }
    }

    #[test]
    fn a_drag_is_one_undo_entry() {
        let mut d = SceneDocument::new(Project::new("t", "attila"), PathBuf::from("t/project.json"));
        d.view.units = 1.0;
        let mut p = d.project().clone();
        let id = p.new_id();
        p.units.push(unit(id));
        d.push("add", p);
        let before = d.history.position();
        for k in 1..=5 {
            d.move_items(&[SceneItem::Unit(id)], &translation(Float3::new(1.0, 0.0, 0.0)), 7);
            assert_eq!(d.project().unit(id).unwrap().start[0], k as f32);
        }
        assert_eq!(d.history.position(), before + 1, "five moves of one drag, one entry");
        d.end_gesture();
        d.move_items(&[SceneItem::Unit(id)], &translation(Float3::new(1.0, 0.0, 0.0)), 7);
        assert_eq!(d.history.position(), before + 2, "a new drag, a new entry");
    }

    #[test]
    fn a_unit_with_a_path_is_keyed_at_the_playhead() {
        let mut p = Project::new("t", "attila");
        let id = p.new_id();
        p.units.push(unit(id));
        let clip = p.clips[0].id;
        crate::scene_ui::add_track(&mut p, clip, "Unit", Some(id), "");
        for c in &mut p.clips {
            for t in &mut c.tracks {
                if let TrackContent::Unit { path, .. } = &mut t.content {
                    path.push(PathKey { t: 0.0, position: [0.0, 0.0], interp: Interp::Linear, gait: None });
                }
            }
        }
        let mut d = SceneDocument::new(p, PathBuf::from("t/project.json"));
        d.view = DocView { clip: Some(clip), time: 2.0, units: 1.0, ..Default::default() };
        d.move_items(&[SceneItem::Unit(id)], &translation(Float3::new(5.0, 0.0, 0.0)), 3);
        let p = d.project();
        assert_eq!(p.unit(id).unwrap().start, [0.0, 0.0], "the start stays");
        let keys: Vec<(f64, [f32; 2])> = p.clips[0]
            .tracks
            .iter()
            .find_map(|t| match &t.content {
                TrackContent::Unit { path, .. } => Some(path.iter().map(|k| (k.t, k.position)).collect()),
                _ => None,
            })
            .unwrap();
        assert_eq!(keys, vec![(0.0, [0.0, 0.0]), (2.0, [5.0, 0.0])]);
    }
}
