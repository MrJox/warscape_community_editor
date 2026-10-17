//! The inspector framework. A document describes the properties of one of its items by calling an [`Inspector`]; the ImGui implementation draws
//! widgets, tests use a recording one. Every property has a stable [`PropertyPath`] {document, item, field}: edits, undo entries and the future
//! Cinematic editor's animation tracks all name properties this way.

use std::fmt;

use wce_scene::render_scene::DocumentId;

#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct PropertyPath {
    pub doc: DocumentId,
    pub item: u64,
    pub field: String,
}

impl fmt::Display for PropertyPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}/{}", self.doc, self.item, self.field)
    }
}

/// The item an inspector is describing.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct InspectTarget {
    pub doc: DocumentId,
    pub item: u64,
}

pub trait Inspector {
    fn target(&self) -> InspectTarget;
    fn set_target(&mut self, target: InspectTarget);

    fn begin(&mut self, doc: DocumentId, item: u64) {
        self.set_target(InspectTarget { doc, item });
    }

    fn path(&self, field: &str) -> PropertyPath {
        let t = self.target();
        PropertyPath { doc: t.doc, item: t.item, field: field.to_owned() }
    }

    fn section(&mut self, title: &str);

    /// Read-only.
    fn text(&mut self, field: &str, label: &str, value: &str);

    /// The `edit_*` calls return true on every frame the value changed (a drag produces many); `gesture_ended` is true once, on the frame the
    /// drag or typing ends, which is where merged commands are closed (`History::break_merge`). The usual speed and range of `edit_float` are
    /// 0.1 and +-1e9; of `edit_float3` 0.1, of `edit_float2` 0.01 and of `edit_int` 0.2.
    fn edit_float(&mut self, field: &str, label: &str, v: &mut f32, speed: f32, lo: f32, hi: f32) -> bool;
    fn edit_float3(&mut self, field: &str, label: &str, v: &mut [f32; 3], speed: f32) -> bool;
    fn edit_float2(&mut self, field: &str, label: &str, v: &mut [f32; 2], speed: f32) -> bool;
    fn edit_int(&mut self, field: &str, label: &str, v: &mut i64, speed: f32) -> bool;

    /// Colours are linear values that may exceed 1 (HDR); alpha is a fourth component.
    fn edit_color3(&mut self, field: &str, label: &str, v: &mut [f32; 3]) -> bool;
    fn edit_color4(&mut self, field: &str, label: &str, v: &mut [f32; 4]) -> bool;
    fn edit_bool(&mut self, field: &str, label: &str, v: &mut bool) -> bool;
    fn edit_string(&mut self, field: &str, label: &str, v: &mut String) -> bool;
    fn edit_choice(&mut self, field: &str, label: &str, index: &mut i32, choices: &[String]) -> bool;
    fn gesture_ended(&mut self) -> bool;
}
