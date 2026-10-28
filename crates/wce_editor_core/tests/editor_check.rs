//! The C++ `editor_check` gate: the editor core without a window or GPU. Command stack (merge, undo, redo, saved state), document manager and
//! scene layers, selection, command registry and script runner, camera controllers, property paths.

use std::any::Any;

use wce_editor_core::camera_controller::{CameraController, CameraInput, CameraOptions, CameraWorld, FlyCamera, GAME_MAX_HEIGHT, NoWorld, OrbitCamera, TopDownCamera, TotalWarCamera, WalkCamera};
use wce_editor_core::command::{Command, CommandStack, History};
use wce_editor_core::command_registry::{CommandHost, CommandRegistry, CommandResult, ScriptRunner};
use wce_editor_core::document::{Document, DocumentCore, SaveTarget};
use wce_editor_core::document_manager::DocumentManager;
use wce_editor_core::inspector::{InspectTarget, Inspector, PropertyPath};
use wce_editor_core::selection::Selection;
use wce_foundation::math::{Float3, Float4x4, length, translation};
use wce_scene::render_scene::{LayerId, Overlay, Payload, RenderScene, SourceRef};
use wce_scene::camera::Camera;

/// An inspector that records what the document asks for and plays back scripted edits.
#[derive(Default)]
struct RecordingInspector {
    target: InspectTarget,

    /// "<kind> <path>"
    calls: Vec<String>,

    /// The next edit_float reports a change to `value_to_set`.
    edit_next: bool,
    value_to_set: f32,
    ended: bool,
}

impl RecordingInspector {
    fn record(&mut self, kind: &str, field: &str) {
        let call = format!("{kind} {}", self.path(field));
        self.calls.push(call);
    }
}

impl Inspector for RecordingInspector {
    fn target(&self) -> InspectTarget {
        self.target
    }

    fn set_target(&mut self, target: InspectTarget) {
        self.target = target;
    }

    fn section(&mut self, title: &str) {
        self.calls.push(format!("section {title}"));
    }

    fn text(&mut self, field: &str, _: &str, _: &str) {
        self.record("text", field);
    }

    fn edit_float(&mut self, field: &str, _: &str, v: &mut f32, _: f32, _: f32, _: f32) -> bool {
        self.record("float", field);
        if !self.edit_next {
            return false;
        }

        *v = self.value_to_set;
        true
    }

    fn edit_float3(&mut self, field: &str, _: &str, _: &mut [f32; 3], _: f32) -> bool {
        self.record("float3", field);
        false
    }

    fn edit_float2(&mut self, field: &str, _: &str, _: &mut [f32; 2], _: f32) -> bool {
        self.record("float2", field);
        false
    }

    fn edit_int(&mut self, field: &str, _: &str, _: &mut i64, _: f32) -> bool {
        self.record("int", field);
        false
    }

    fn edit_color3(&mut self, field: &str, _: &str, _: &mut [f32; 3]) -> bool {
        self.record("color3", field);
        false
    }

    fn edit_color4(&mut self, field: &str, _: &str, _: &mut [f32; 4]) -> bool {
        self.record("color4", field);
        false
    }

    fn edit_bool(&mut self, field: &str, _: &str, _: &mut bool) -> bool {
        self.record("bool", field);
        false
    }

    fn edit_string(&mut self, field: &str, _: &str, _: &mut String) -> bool {
        self.record("string", field);
        false
    }

    fn edit_choice(&mut self, field: &str, _: &str, _: &mut i32, _: &[String]) -> bool {
        self.record("choice", field);
        false
    }

    fn gesture_ended(&mut self) -> bool {
        self.ended
    }
}

/// What kind of document a NumberDoc plays.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Role {
    Plain,

    /// Brings its own ground (a tile).
    Ground,

    /// The stand-in scene a tool shows while there is none (the weather tool's sample map).
    StandIn,
}

/// A document of N numbers; items are the indices; each edit is a command.
struct NumberDoc {
    core: DocumentCore,
    history: CommandStack<Vec<f32>>,
    role: Role,
    saves: i32,
    publishes: i32,
}

impl NumberDoc {
    fn new(n: usize) -> Self {
        Self::with_role(n, Role::Plain)
    }

    fn with_role(n: usize, role: Role) -> Self {
        Self { core: DocumentCore::new(), history: CommandStack::new(vec![0.0; n]), role, saves: 0, publishes: 0 }
    }

    fn values(&self) -> &[f32] {
        self.history.model()
    }

    fn set(&mut self, i: usize, v: f32, key: u64) {
        let before = self.values()[i];
        self.history.push(Box::new(SetCommand { i, before, after: v, key }));
    }
}

struct SetCommand {
    i: usize,
    before: f32,
    after: f32,
    key: u64,
}

impl Command<Vec<f32>> for SetCommand {
    fn name(&self) -> String {
        format!("set {}", self.i)
    }

    fn apply(&mut self, values: &mut Vec<f32>) {
        values[self.i] = self.after;
    }

    fn revert(&mut self, values: &mut Vec<f32>) {
        values[self.i] = self.before;
    }

    fn merge_key(&self) -> u64 {
        self.key
    }

    fn merge_from(&mut self, next: &dyn Command<Vec<f32>>) -> bool {
        match next.as_any().downcast_ref::<SetCommand>() {
            Some(n) if n.i == self.i => {
                self.after = n.after;
                true
            }

            _ => false,
        }
    }

    fn dirty_items(&self) -> Vec<u64> {
        vec![self.i as u64]
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl Document for NumberDoc {
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
        "numbers".to_owned()
    }

    fn kind(&self) -> String {
        "numbers".to_owned()
    }

    fn can_save(&self) -> bool {
        self.role != Role::StandIn
    }

    fn save(&mut self, _: &SaveTarget) -> Result<(), String> {
        self.saves += 1;
        Ok(())
    }

    fn publish(&mut self, scene: &mut RenderScene, layer: LayerId) {
        self.publishes += 1;
        scene.clear_layer(layer);
        for (i, &v) in self.values().iter().enumerate() {
            scene.add(layer, translation(Float3::new(v, 0.0, 0.0)), Payload::Overlay(Overlay::default()), SourceRef { doc: self.id(), item: i as u64 });
        }
    }

    fn inspect(&mut self, r: SourceRef, ins: &mut dyn Inspector) {
        ins.section("Number");
        let mut v = self.values()[r.item as usize];
        if ins.edit_float("value", "value", &mut v, 0.1, -1e9, 1e9) {
            self.set(r.item as usize, v, (u64::from(self.id()) << 32) | r.item);
        }

        if ins.gesture_ended() {
            self.history.break_merge();
        }
    }

    fn item_transform(&self, r: SourceRef) -> Option<Float4x4> {
        self.values().get(r.item as usize).map(|&v| translation(Float3::new(v, 0.0, 0.0)))
    }

    fn transform_items(&mut self, items: &[SourceRef], delta: &Float4x4, gesture: u64) {
        for r in items {
            let v = self.values()[r.item as usize] + delta.m[0][3];
            self.set(r.item as usize, v, gesture);
        }
    }

    fn shows_scene(&self) -> bool {
        self.role == Role::Ground
    }

    fn stand_in(&self) -> bool {
        self.role == Role::StandIn
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

#[test]
fn command_stack() {
    let mut d = NumberDoc::new(3);
    assert!(!d.history.modified() && !d.history.can_undo() && !d.history.can_redo(), "a new stack is clean");
    d.set(0, 1.0, 0);
    d.set(1, 2.0, 0);
    assert!(d.history.size() == 2 && d.history.position() == 2 && d.history.modified(), "two edits, modified");
    assert!(d.history.undo() && d.values()[1] == 0.0 && d.history.position() == 1, "undo");
    assert!(d.history.redo() && d.values()[1] == 2.0, "redo");
    d.history.undo();
    d.set(2, 5.0, 0);
    assert!(!d.history.can_redo() && d.history.size() == 2, "a new edit drops the redo tail");
    d.history.mark_saved();
    assert!(!d.history.modified(), "saved");
    d.history.undo();
    assert!(d.history.modified(), "undo after saving is a change");
    d.history.redo();
    assert!(!d.history.modified(), "back at the saved position");
    d.history.undo();

    // The saved state was in the dropped tail.
    d.set(1, 9.0, 0);
    assert!(d.history.modified(), "after dropping the saved state the document stays modified");

    // Merged gesture: one entry for many pushes, until break_merge.
    let mut g = NumberDoc::new(1);
    for i in 1..=10 {
        g.set(0, i as f32, 77);
    }

    assert!(g.history.size() == 1 && g.values()[0] == 10.0, "a drag gesture is one undo entry");
    g.history.undo();
    assert!(g.values()[0] == 0.0, "undoing the merged gesture restores the value before the drag");
    g.history.redo();
    g.history.break_merge();
    g.set(0, 11.0, 77);
    assert!(g.history.size() == 2, "break_merge ends the gesture");
    g.history.mark_saved();
    g.set(0, 12.0, 77);
    assert!(g.history.size() == 3, "an edit right after saving never merges into the saved entry");
    g.history.jump_to(0);
    assert!(g.values()[0] == 0.0 && g.history.position() == 0, "jump_to");
    g.history.jump_to(3);
    assert!(g.values()[0] == 12.0, "jump_to forward");
    assert!(g.history.last_dirty().items == [0] && !g.history.last_dirty().all, "the stack reports the touched items");

    // Everything since the stack was made: several steps of undo and redo in one frame add up.
    let d = g.history.take_dirty();
    assert!(!d.all && d.items.first() == Some(&0), "take_dirty collects the items of every step since the last call");
    let again = g.history.take_dirty();
    assert!(!again.all && again.items.is_empty(), "take_dirty starts over after it was taken");
}

fn number_doc(docs: &DocumentManager, id: u32) -> &NumberDoc {
    docs.find_as::<NumberDoc>(id).unwrap()
}

fn number_doc_mut(docs: &mut DocumentManager, id: u32) -> &mut NumberDoc {
    docs.find_as_mut::<NumberDoc>(id).unwrap()
}

#[test]
fn documents_and_the_scene() {
    let mut scene = RenderScene::default();
    let mut docs = DocumentManager::default();
    let ia = docs.add(&mut scene, Box::new(NumberDoc::new(3)), "t");
    let ib = docs.add(&mut scene, Box::new(NumberDoc::new(2)), "t");
    assert!(docs.order().len() == 2 && docs.active_id() == ib, "two documents, the last one active");
    assert!(scene.entities().len() == 5 && scene.layers().len() == 2, "each document has a layer with its entities");
    assert!(scene.entities_of(SourceRef { doc: ia, item: 1 }).len() == 1, "entities_of finds the entity of an item");
    let before = number_doc(&docs, ia).publishes;
    docs.update(&mut scene);
    assert!(number_doc(&docs, ia).publishes == before, "an unchanged document is not re-published");
    number_doc_mut(&mut docs, ia).set(1, 4.0, 0);
    docs.update(&mut scene);
    assert!(number_doc(&docs, ia).publishes == before + 1, "an edited document is re-published");
    assert!(scene.find(scene.entities_of(SourceRef { doc: ia, item: 1 })[0]).unwrap().world.m[0][3] == 4.0, "the scene shows the edit");
    assert!(docs.any_modified(), "modified");
    let mut msgs = vec![];
    assert!(docs.save_all(&mut msgs) && number_doc(&docs, ia).saves == 1 && !docs.any_modified(), "save_all saves what changed");
    docs.set_visible(&mut scene, ia, false);
    assert!(!scene.drawable(scene.find(scene.entities_of(SourceRef { doc: ia, item: 0 })[0]).unwrap()), "a hidden document's entities are not drawable");
    assert!(docs.close(&mut scene, ia) && scene.entities().len() == 2 && docs.active_id() == ib, "closing removes the layer and its entities");
}

#[test]
fn cross_tool_composition() {
    let mut scene = RenderScene::default();
    let mut docs = DocumentManager::default();
    let is = docs.add(&mut scene, Box::new(NumberDoc::with_role(2, Role::StandIn)), "weather");
    assert!(docs.visible(&scene, is) && !docs.shows_scene(), "a stand-in scene is shown while no document brings its own ground");
    let ig = docs.add(&mut scene, Box::new(NumberDoc::with_role(3, Role::Ground)), "tile");
    assert!(docs.shows_scene() && !docs.visible(&scene, is) && docs.visible(&scene, ig), "opening a tile hides the stand-in scene");
    let io = docs.add(&mut scene, Box::new(NumberDoc::new(1)), "weather");
    assert!(docs.unsaved().is_empty(), "nothing is unsaved after opening");

    // The user shows it again: it stays shown until the last tile closes and opens again.
    docs.set_visible(&mut scene, is, true);
    docs.update(&mut scene);
    assert!(docs.visible(&scene, is), "a toggle of the user's stays");
    let is2 = docs.add(&mut scene, Box::new(NumberDoc::with_role(1, Role::StandIn)), "weather");
    assert!(!docs.visible(&scene, is2), "a stand-in scene added while a tile is open starts hidden");
    number_doc_mut(&mut docs, ig).set(0, 1.0, 0);
    number_doc_mut(&mut docs, io).set(0, 2.0, 0);
    assert!(docs.unsaved() == [ig, io], "unsaved lists the modified documents in opening order");
    assert!(docs.save_one(io).is_ok() && number_doc(&docs, io).saves == 1 && number_doc(&docs, ig).saves == 0 && docs.unsaved() == [ig], "save_one saves one document and marks it saved");
    assert!(docs.save_one(is).is_err_and(|m| m.contains("cannot be saved")), "a document that cannot be saved says so");
    assert!(docs.save_one(9999).is_err(), "no such document");
    assert!(docs.close(&mut scene, ig), "close the tile");
    assert!(!docs.shows_scene() && docs.visible(&scene, is) && docs.visible(&scene, is2), "closing the last tile shows the stand-in scenes again");
}

#[test]
fn selection() {
    let mut scene = RenderScene::default();
    let mut docs = DocumentManager::default();
    let id = docs.add(&mut scene, Box::new(NumberDoc::new(4)), "t");
    let r = |item| SourceRef { doc: id, item };
    let mut sel = Selection::default();
    sel.set(r(1));
    sel.add(r(2));
    sel.add(r(2));
    assert!(sel.items().len() == 2 && sel.contains(r(1)), "set and add");
    sel.toggle(r(1));
    assert!(sel.items().len() == 1 && !sel.contains(r(1)), "toggle removes");
    assert!(sel.entities(&scene).len() == 1, "selected items map to scene entities");
    let rev = sel.revision();
    sel.set(r(2));
    assert!(sel.revision() == rev, "setting the same single item changes nothing");
    sel.remove_document(id);
    assert!(sel.is_empty(), "closing a document drops its items");
}

/// The host of the registry test: the registry and what its commands change.
#[derive(Default)]
struct Host {
    commands: CommandRegistry<Host>,
    calls: i32,
    last: String,
}

impl CommandHost for Host {
    fn commands(&self) -> &CommandRegistry<Self> {
        &self.commands
    }
}

#[test]
fn command_registry_and_scripts() {
    let mut host = Host::default();
    host.commands.add("count", "count", "", |h, _| {
        h.calls += 1;
        CommandResult::success("")
    });

    host.commands.add("say", "say words", "", |h, a| {
        h.last = a.first().cloned().unwrap_or_default();
        CommandResult::success("")
    });

    host.commands.add("bad", "bad", "", |_, _| CommandResult::fail("nope"));
    host.commands.add("pause", "pause N", "", |_, a| CommandResult::success("").wait(a[0].parse().unwrap_or(0)));
    host.commands.add("boom", "boom", "", |_, _| panic!("kaput"));
    assert!(CommandRegistry::<Host>::tokenize("a \"b c\" d # comment").len() == 3, "tokenize groups quotes and drops comments");
    assert!(CommandRegistry::<Host>::tokenize("say \"\"").len() == 2, "an empty quoted argument is an argument");
    assert!(CommandRegistry::run(&mut host, "say \"hello world\"").ok && host.last == "hello world", "run with a quoted argument");
    assert!(!CommandRegistry::run(&mut host, "nothing").ok, "unknown command fails");
    let boom = CommandRegistry::run(&mut host, "boom");
    assert!(!boom.ok && boom.message.contains("kaput"), "a panic becomes a failed result");

    let mut s = ScriptRunner::default();
    s.load_text("# a script\ncount\n\n-bad\ncount\npause 3\ncount\n");
    let mut frames = 0;
    while !s.done() && frames < 20 {
        s.step(&mut host);
        frames += 1;
    }

    assert!(!s.failed() && host.calls == 3, "an optional failing line does not stop the script");
    assert!(frames == 4, "pause 3 holds the script for three frames");
    let mut t = ScriptRunner::default();
    t.load_text("count\nbad\ncount\n");
    host.calls = 0;
    t.step(&mut host);
    assert!(t.failed() && host.calls == 1 && t.error().contains("bad"), "a failing line stops the script with its line number");
}

/// A flat ground at y = 0 and one 10 m box at (50, 0..10, 0).
struct FlatWorld;

impl CameraWorld for FlatWorld {
    fn ground(&mut self, _x: f32, _z: f32) -> Option<f32> {
        Some(0.0)
    }

    fn ray_ground(&mut self, o: Float3, d: Float3) -> Option<Float3> {
        (d.y < -1e-6).then(|| o + d * (-o.y / d.y))
    }

    fn push_out(&mut self, p: Float3, r: f32) -> Float3 {
        let (lo, hi) = (Float3::new(45.0, 0.0, -5.0), Float3::new(55.0, 10.0, 5.0));
        let inside = |p: Float3, m: f32| p.x > lo.x - m && p.x < hi.x + m && p.y > lo.y - m && p.y < hi.y + m && p.z > lo.z - m && p.z < hi.z + m;
        if !inside(p, r) {
            return p;
        }

        // Out through the top (enough for the test).
        Float3::new(p.x, hi.y + r, p.z)
    }
}

#[test]
fn cameras() {
    let opts = CameraOptions { smoothing: 0.0, ..Default::default() };
    let mut o = TotalWarCamera::default();
    o.set_orbit(0.6, 0.35, 120.0, Float3::new(0.0, 5.0, 0.0));
    let c = o.camera();
    let (yaw, pitch, dist) = (0.6f32, 0.35f32, 120.0f32);
    let eye = Float3::new(0.0, 5.0, 0.0) + Float3::new(yaw.sin() * pitch.cos(), pitch.sin(), -yaw.cos() * pitch.cos()) * dist;
    assert!(length(c.eye - eye) < 1e-3 && length(c.target - Float3::new(0.0, 5.0, 0.0)) < 1e-3, "the orbit convention (`cam`) still means the same view");
    let mut o2 = TotalWarCamera::default();
    o2.set_camera(&c);
    assert!(length(o2.camera().eye - c.eye) < 1e-3 && length(o2.camera().target - c.target) < 1e-2, "Total War camera round trip through a Camera");

    let e0 = o.camera().eye;
    o.update(&CameraInput { forward: true, dt: 1.0, ..Default::default() }, &opts, &mut FlatWorld);
    assert!(length(o.camera().eye - e0) > 1.0 && (o.camera().eye.y - e0.y).abs() < 1e-3, "WASD moves over the ground, not up or down");
    let (y0, p0) = (o.yaw, o.pitch);
    o.update(&CameraInput { turn_right: true, dt: 0.5, ..Default::default() }, &opts, &mut FlatWorld);
    assert!(o.yaw > y0, "E turns right");
    let h0 = o.camera().eye.y;
    o.update(&CameraInput { wheel: 3.0, dt: 0.01, ..Default::default() }, &opts, &mut FlatWorld);
    assert!(o.camera().eye.y < h0 && o.pitch > p0, "the wheel goes down along the view and flattens it");
    o.update(&CameraInput { rotate: true, dx: 100.0, dt: 0.01, ..Default::default() }, &opts, &mut FlatWorld);
    assert!(o.yaw > y0, "dragging turns the camera");

    // Ground and collisions.
    let mut g = TotalWarCamera::default();
    g.set_camera(&Camera { eye: Float3::new(0.0, -5.0, 0.0), target: Float3::new(0.0, -5.0, 10.0), ..Camera::default() });
    g.update(&CameraInput { dt: 0.01, ..Default::default() }, &opts, &mut FlatWorld);
    assert!((g.camera().eye.y - 1.0).abs() < 1e-4, "the eye is put back 1 m above the ground");
    g.set_camera(&Camera { eye: Float3::new(50.0, 5.0, 0.0), target: Float3::new(50.0, 5.0, 10.0), ..Camera::default() });
    g.update(&CameraInput { dt: 0.01, ..Default::default() }, &CameraOptions { respect_collisions: true, ..opts }, &mut FlatWorld);
    assert!(g.camera().eye.y >= 10.0, "the eye leaves a bounding box");
    g.set_camera(&Camera { eye: Float3::new(0.0, 300.0, 0.0), target: Float3::new(0.0, 0.0, 10.0), ..Camera::default() });
    g.update(&CameraInput { dt: 0.01, ..Default::default() }, &CameraOptions { game_limits: true, ..opts }, &mut FlatWorld);
    assert!((g.camera().eye.y - GAME_MAX_HEIGHT).abs() < 1e-3, "the game's limits hold the height");

    // Smoothing: the view catches up over a few frames.
    let mut s = TotalWarCamera::default();
    let smooth = CameraOptions { smoothing: 1.0, ..opts };
    let start = s.camera().eye;
    s.update(&CameraInput { forward: true, dt: 0.03, ..Default::default() }, &smooth, &mut NoWorld);
    let first = length(s.camera().eye - start);
    assert!(first > 0.0 && first < length(s.eye - start) * 0.5, "smoothing shows part of the move");

    // Top-down: orthographic, straight down, north up; the wheel zooms about the mouse.
    let mut t = TopDownCamera::default();
    t.set_camera(&c);
    let tc = t.camera();
    assert!(tc.is_ortho() && (tc.target - tc.eye).y < 0.0 && (tc.target.x - tc.eye.x).abs() < 1e-4, "the top-down view looks straight down");
    let under = |t: &TopDownCamera| t.camera().ray(0.5, 0.5, 1.0).0;
    let before = under(&t);
    t.update(&CameraInput { wheel: 2.0, cursor: Some([0.5, 0.5]), aspect: 1.0, view_h: 500.0, dt: 0.01, ..Default::default() }, &opts, &mut FlatWorld);
    let after = under(&t);
    assert!(t.height < length(c.target - c.eye) * 2.0 && (before.x - after.x).abs() < 1e-2 && (before.z - after.z).abs() < 1e-2, "zoom keeps the point under the mouse");
    let back = t.handoff();
    assert!(!back.is_ortho(), "the top-down view hands over a perspective view");

    // Walk: on the ground at eye height.
    let mut w = WalkCamera::default();
    w.set_camera(&c);
    w.update(&CameraInput { forward: true, dt: 1.0, ..Default::default() }, &opts, &mut FlatWorld);
    assert!((w.camera().eye.y - w.eye_height).abs() < 1e-4, "the walker's eye is at its height above the ground");

    // Orbit: the `cam` convention, round trip; the wheel zooms; WASD moves the target. Fly: continues from a view and moves along it.
    let mut r = OrbitCamera::default();
    r.set_orbit(0.6, 0.35, 120.0, Float3::new(0.0, 5.0, 0.0));
    assert!(length(r.camera().eye - c.eye) < 1e-3, "the orbit camera and the Total War camera agree on `cam`");
    let mut r2 = OrbitCamera::default();
    r2.set_camera(&r.camera());
    assert!((r2.yaw - 0.6).abs() < 1e-4 && (r2.pitch - 0.35).abs() < 1e-4 && (r2.dist - 120.0).abs() < 1e-2, "orbit round trip through a Camera");
    r.update(&CameraInput { wheel: 1.0, dt: 0.01, ..Default::default() }, &opts, &mut NoWorld);
    assert!(r.dist < 120.0, "the wheel zooms in");
    let t0 = r.target;
    r.update(&CameraInput { forward: true, dt: 0.1, ..Default::default() }, &opts, &mut FlatWorld);
    assert!(length(r.target - t0) > 1.0 && r.target.y == 0.0, "WASD moves the target over the ground");
    let mut f = FlyCamera::default();
    f.set_camera(&c);
    assert!(length(f.camera().eye - c.eye) < 1e-3, "fly continues from the view");
    let e1 = f.eye;
    f.update(&CameraInput { forward: true, dt: 1.0, ..Default::default() }, &opts, &mut NoWorld);
    assert!(length(f.eye - e1) > 1.0, "fly moves forward");
    assert!((o.lens.fov_y - 1.13446).abs() < 1e-5 && o.lens.znear == 1.0 && o.lens.zfar == 13000.0, "the lens is the game's battle camera");
}

#[test]
fn inspector_edits_become_merged_commands() {
    let mut d = NumberDoc::new(2);
    let r = SourceRef { doc: d.id(), item: 1 };
    let mut ins = RecordingInspector::default();
    ins.begin(d.id(), 1);
    d.inspect(r, &mut ins);
    assert!(ins.calls.len() == 2 && ins.calls[0] == "section Number" && ins.calls[1] == format!("float {}/1/value", d.id()), "the document describes its item through the inspector");
    ins.edit_next = true;
    for v in [1.0, 2.0, 3.0] {
        ins.value_to_set = v;
        d.inspect(r, &mut ins);
    }

    assert!(d.values()[1] == 3.0 && d.history.size() == 1, "a drag in the inspector is one undo entry");
    ins.ended = true;
    ins.edit_next = false;
    d.inspect(r, &mut ins);
    ins.ended = false;
    ins.edit_next = true;
    ins.value_to_set = 4.0;
    d.inspect(r, &mut ins);
    assert!(d.history.size() == 2, "the end of a gesture closes the merge");
    d.history.undo();
    assert!(d.values()[1] == 3.0, "undo goes back to the value before the second gesture");
}

#[test]
fn property_paths() {
    let a = PropertyPath { doc: 1, item: 2, field: "offset".into() };
    let b = a.clone();
    let c = PropertyPath { item: 3, ..a.clone() };
    assert!(a == b && a != c && a.to_string() == "1/2/offset", "property paths compare and print");
}
