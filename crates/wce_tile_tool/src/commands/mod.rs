//! The tile tool's commands: everything the panels and the mouse do (the old Tile Studio script flags), plus test commands (assert_*).
//!
//! | Studio flag | command |
//! |---|---|
//! | `--tile <folder>` | `tile <folder>` |
//! | `--paint k x z n` | `paint k x z n` |
//! | `--select t\|n` | `tile_select t\|n` |
//! | `--move dx dz` | `tile_move dx dz` |
//! | `--duplicate`, `--delete`, `--rotate d`, `--add key` | `duplicate`, `delete_item`, `rotate d`, `add_building key` |
//! | `--add-polygon s`, `--polygon-point op i x z` | `add_polygon s`, `polygon_point op i x z` |
//! | `--new-layer n`, `--move-to-layer n` | `new_layer n`, `move_to_layer n` |
//! | `--save-to dir`, `--undo n` / `--redo n` | `save_to dir`, `undo` / `redo` (the editor's, one step each) |
//!
//! The app maps the old flag names to these commands.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use wce_editor::editor::Editor;
use wce_editor_core::command_registry::{CommandRegistry, CommandResult};
use wce_editor_core::document::{Document, SaveTarget};
use wce_formats::tile_index::TileIndex;
use wce_foundation::math::Float3;
use wce_tile::ground_types::GROUND_TYPES;
use wce_tile::world_transform::world_transform;
use wce_tile_editor::terrain_bake::TerrainView;
use wce_tile_editor::tile_document::layers::LayerType;
use wce_tile_editor::tile_document::{BrushKind, GROUND_ERASE, Placement, TileDocument};

use crate::tool::{HINTS, MODE_NAMES, TileMode, TileTool, document, matches_words, primary, select_elem, selected, with_tile};
use wce_tile::property_enums::WALL_BASE;

mod edit;
mod files;
mod layers;
mod mouse;
mod placement;
mod prefabs;
mod terrain;
mod view;
mod walls;

/// An inspector that writes the rows it is given, with their values (for `tile_inspect`).
#[derive(Default)]
struct DumpInspector {
    target: wce_editor_core::inspector::InspectTarget,
    lines: Vec<String>,

    /// One edit to apply to the row with this name (`tile_edit`): the value as text (a number, true / false, text, a choice's number or
    /// name, `r,g,b,a`, `Name=true` for a row of check boxes).
    edit: Option<(String, String)>,
    applied: bool,

    /// Text rows applied and not yet ended.
    ended: bool,
}

impl DumpInspector {
    fn wants(&mut self, field: &str, label: &str) -> Option<String> {
        let (f, v) = self.edit.as_ref()?;
        if f != field && f != label {
            return None;
        }

        let v = v.clone();
        self.applied = true;
        Some(v)
    }
}

impl wce_editor_core::inspector::Inspector for DumpInspector {
    fn target(&self) -> wce_editor_core::inspector::InspectTarget {
        self.target
    }

    fn set_target(&mut self, target: wce_editor_core::inspector::InspectTarget) {
        self.target = target;
    }

    fn section(&mut self, title: &str) {
        self.lines.push(format!("== {title}"));
    }

    fn mark_fields(&mut self, marks: Vec<(String, String)>) {
        for (f, note) in marks {
            self.lines.push(format!("  ! {f}: {note}"));
        }
    }

    fn text(&mut self, _: &str, label: &str, value: &str) {
        self.lines.push(format!("  {label}: {value}  (read-only)"));
    }

    fn edit_float(&mut self, field: &str, label: &str, v: &mut f32, _: f32, lo: f32, hi: f32) -> bool {
        self.lines.push(format!("  {label}: {v}  [{lo} .. {hi}]"));
        match self.wants(field, label).and_then(|t| t.parse::<f32>().ok()) {
            Some(x) => {
                *v = x;
                true
            }

            None => false,
        }
    }

    fn edit_float3(&mut self, field: &str, label: &str, v: &mut [f32; 3], _: f32) -> bool {
        self.lines.push(format!("  {label}: {v:?}"));
        match self.wants(field, label).and_then(|t| parse_floats::<3>(&t)) {
            Some(x) => {
                *v = x;
                true
            }

            None => false,
        }
    }

    fn edit_float2(&mut self, field: &str, label: &str, v: &mut [f32; 2], _: f32) -> bool {
        self.lines.push(format!("  {label}: {v:?}"));
        match self.wants(field, label).and_then(|t| parse_floats::<2>(&t)) {
            Some(x) => {
                *v = x;
                true
            }

            None => false,
        }
    }

    fn edit_int(&mut self, field: &str, label: &str, v: &mut i64, _: f32) -> bool {
        self.lines.push(format!("  {label}: {v}"));
        match self.wants(field, label).and_then(|t| t.parse::<i64>().ok()) {
            Some(x) => {
                *v = x;
                true
            }

            None => false,
        }
    }

    fn edit_color3(&mut self, field: &str, label: &str, v: &mut [f32; 3]) -> bool {
        self.lines.push(format!("  {label}: {v:?}"));
        match self.wants(field, label).and_then(|t| parse_floats::<3>(&t)) {
            Some(x) => {
                *v = x;
                true
            }

            None => false,
        }
    }

    fn edit_color4(&mut self, field: &str, label: &str, v: &mut [f32; 4]) -> bool {
        self.lines.push(format!("  {label}: {v:?}"));
        match self.wants(field, label).and_then(|t| parse_floats::<4>(&t)) {
            Some(x) => {
                *v = x;
                true
            }

            None => false,
        }
    }

    fn edit_bool(&mut self, field: &str, label: &str, v: &mut bool) -> bool {
        self.lines.push(format!("  {label}: {v}"));
        match self.wants(field, label).as_deref() {
            Some("true" | "1") => {
                *v = true;
                true
            }

            Some("false" | "0") => {
                *v = false;
                true
            }

            _ => false,
        }
    }

    fn edit_string(&mut self, field: &str, label: &str, v: &mut String) -> bool {
        self.lines.push(format!("  {label}: '{v}'"));
        match self.wants(field, label) {
            Some(x) => {
                *v = x;
                self.ended = true;
                true
            }

            None => false,
        }
    }

    fn edit_choice(&mut self, field: &str, label: &str, index: &mut i32, choices: &[String]) -> bool {
        let shown = usize::try_from(*index).ok().and_then(|i| choices.get(i)).map_or("?", String::as_str);
        self.lines.push(format!("  {label}: {shown}  ({} choices)", choices.len()));
        let Some(t) = self.wants(field, label) else { return false };
        match t.parse::<i32>().ok().or_else(|| choices.iter().position(|c| *c == t).map(|i| i as i32)) {
            Some(i) => {
                *index = i;
                true
            }

            None => false,
        }
    }

    fn gesture_ended(&mut self) -> bool {
        std::mem::take(&mut self.ended)
    }

    fn edit_slider(&mut self, field: &str, label: &str, v: &mut f32, lo: f32, hi: f32) -> bool {
        self.lines.push(format!("  {label}: {v}  slider [{lo} .. {hi}]"));
        match self.wants(field, label).and_then(|t| t.parse::<f32>().ok()) {
            Some(x) => {
                *v = x;
                true
            }

            None => false,
        }
    }

    fn edit_int_slider(&mut self, field: &str, label: &str, v: &mut i64, lo: i64, hi: i64) -> bool {
        self.lines.push(format!("  {label}: {v}  slider [{lo} .. {hi}]"));
        match self.wants(field, label).and_then(|t| t.parse::<i64>().ok()) {
            Some(x) => {
                *v = x;
                true
            }

            None => false,
        }
    }

    fn edit_angle(&mut self, field: &str, label: &str, v: &mut f32, max: f32) -> bool {
        self.lines.push(format!("  {label}: {:.2} deg  [0 .. {:.0}]", v.to_degrees(), max.to_degrees()));

        // The value is in degrees, as typed in the box.
        match self.wants(field, label).and_then(|t| t.parse::<f32>().ok()) {
            Some(x) => {
                *v = x.to_radians();
                true
            }

            None => false,
        }
    }

    fn edit_checks(&mut self, field: &str, label: &str, names: &[&str], on: &mut [bool]) -> bool {
        let set = names.iter().zip(on.iter()).map(|(n, o)| format!("{n}={o}")).collect::<Vec<_>>().join(" ");
        self.lines.push(format!("  {label}: {set}"));
        let Some(t) = self.wants(field, label) else { return false };
        let Some((name, value)) = t.split_once('=') else { return false };
        match (names.iter().position(|n| *n == name), value) {
            (Some(i), "true") => on[i] = true,
            (Some(i), "false") => on[i] = false,
            _ => return false,
        }

        true
    }
}

/// `a,b,c` as numbers.
fn parse_floats<const N: usize>(text: &str) -> Option<[f32; N]> {
    let v = text.split(',').map(|x| x.trim().parse::<f32>().ok()).collect::<Option<Vec<_>>>()?;
    v.try_into().ok()
}

fn ok(m: impl Into<String>) -> CommandResult {
    CommandResult::success(m)
}

fn fail(m: impl Into<String>) -> CommandResult {
    CommandResult::fail(m)
}

/// The whole string as a float (strtof with nothing left over).
fn number(s: &str) -> Option<f32> {
    if s.is_empty() { None } else { s.parse().ok() }
}

/// C's atoi: the leading integer, 0 when there is none.
fn atoi(s: &str) -> i64 {
    wce_formats::c_num::strtol(s).unwrap_or(0)
}

/// `std::to_string` of a float: `%f`.
fn f6(v: f32) -> String {
    format!("{v:.6}")
}

fn no_tile() -> CommandResult {
    fail("no tile open")
}

/// Runs `f` with the tile tool and the active tile.
fn with_doc(e: &mut Editor, f: impl FnOnce(&mut TileTool, &mut Editor) -> CommandResult) -> CommandResult {
    if document(e).is_none() {
        return no_tile();
    }

    with_tile(e, f)
}

/// Runs the `mouse` command for a command that drives the scripted mouse: the caller returns `message` and the frames the mouse needs (the
/// C++ registry kept the nested command's wait).
fn mouse(e: &mut Editor, args: &[String], message: impl Into<String>) -> CommandResult {
    let r = CommandRegistry::run_args(e, "mouse", args);
    if r.ok { ok(message).wait(r.wait_frames) } else { r }
}

/// Clicks the viewport pixel of a world point with the scripted mouse; None when the point is not in view.
fn click_at(e: &mut Editor, p: Float3, message: impl Into<String>) -> Option<CommandResult> {
    let (x, y) = e.viewport.project(p)?;
    Some(mouse(e, &["click".to_owned(), format!("{x:.1}"), format!("{y:.1}")], message))
}

pub fn register(c: &mut CommandRegistry<Editor>) {
    files::register(c);
    terrain::register(c);
    edit::register(c);
    walls::register(c);
    mouse::register(c);
    layers::register(c);
    prefabs::register(c);
    placement::register(c);
    view::register(c);
}

/// The first selected instance of `sel` (what "selected" names).
fn primary_of(sel: &[usize], d: &TileDocument) -> Option<usize> {
    sel.iter().copied().find(|&el| d.tile().is_instance(el))
}

/// An element named on a command line: "#123" (an element index), a layer path ("Walls/Standard_Walls"), "selected" (the first selected
/// item, read before the document was borrowed).
fn element_arg(d: &TileDocument, a: &str, primary: Option<usize>) -> Option<usize> {
    if a.is_empty() {
        return None;
    }

    if a == "selected" {
        return primary;
    }

    if let Some(n) = a.strip_prefix('#') {
        return usize::try_from(atoi(n)).ok();
    }

    d.tile().find_layer(a)
}

/// The selected group or layer, when no instance is selected.
fn selected_group_or_layer(e: &Editor) -> Option<usize> {
    let d = e.docs.find_as::<TileDocument>(crate::tool::document_id(e)?)?;
    crate::tool::selected(e).into_iter().find(|&el| d.tile().tscene().is_layer(el) && d.tile().tscene().type_of(el) != "SCENE_GRAPH")
}
