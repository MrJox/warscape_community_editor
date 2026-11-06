//! Flip mode: a vertical plane in the viewport and the Inspector's controls for mirroring the selection across it, moved or as a copy
//! (`TileDocument::flip`). The plane has a gizmo of its own that moves and turns it; a press off it puts the plane on the ground there.

use wce_editor::editor::Editor;
use wce_editor::gui;
use wce_foundation::math::Float4x4;
use wce_scene::render_scene::{Overlay, overlay_colour};
use wce_tile_editor::tile_document::{FlipPlane, Tile, TileDocument};

use crate::selection_tools::set_selection;
use crate::tool::{TileTool, document, document_id, selected};

/// How far the plane is drawn along its line, how high, and every how many metres (metres).
const PLANE_REACH: f32 = 1024.0;
const PLANE_HEIGHT: f32 = 60.0;
const PLANE_STEP: f32 = 32.0;

/// The most lines drawn from the selection to where it would go.
const MAX_LINES: usize = 128;

#[derive(Clone, Copy, Debug)]
pub(crate) struct FlipMode {
    pub plane: FlipPlane,

    /// A copy goes across, the selection stays.
    pub copy: bool,
    pub dragging: bool,

    /// How high its gizmo stood while it is dragged: the ground under it must not move it in the hand.
    pub gizmo_height: f32,
}

impl TileTool {
    /// Enters Flip mode with the plane through the selection's centre.
    pub fn begin_flip(&mut self, e: &mut Editor) -> Result<String, String> {
        let sel = selected(e);
        let d = document(e).ok_or("no tile is open")?;
        if sel.is_empty() {
            return Err("select what to flip first".to_owned());
        }

        let t = d.tile();
        let n = sel.len() as f32;
        let (x, z) = sel.iter().map(|&s| t.world_position(s)).fold((0.0, 0.0), |(x, z), p| (x + p.x / n, z + p.z / n));
        self.stop_making();
        self.flip = Some(FlipMode { plane: FlipPlane { x, z, angle: 0.0 }, copy: false, dragging: false, gizmo_height: 0.0 });
        Ok("flip: its gizmo moves and turns the plane, a press on the ground puts it there; the Inspector flips".to_owned())
    }

    /// Mirrors the selection across the plane.
    pub fn do_flip(&mut self, e: &mut Editor) -> Result<String, String> {
        let flip = self.flip.ok_or("not in Flip mode")?;
        let sel = selected(e);
        let id = document_id(e).ok_or("no tile is open")?;
        let into = if flip.copy { Some(document(e).and_then(|d| self.new_items_layer(d)).ok_or("this tile has no scene graph")?) } else { None };
        let d = e.docs.find_as_mut::<TileDocument>(id).ok_or("no tile is open")?;
        let went = d.flip(&sel, flip.plane, into)?;
        if flip.copy {
            set_selection(e, &went);
        }

        Ok(format!("flipped {} item(s){}", went.len(), if flip.copy { " as a copy" } else { "" }))
    }

    /// Puts the plane at the mouse: a press on the ground, and its drag.
    pub(crate) fn flip_drag(&mut self, start: bool) -> bool {
        let (hit, cursor) = (self.terrain_hit, self.cursor);
        let Some(flip) = &mut self.flip else { return false };
        flip.dragging = start || flip.dragging;
        if flip.dragging && hit {
            (flip.plane.x, flip.plane.z) = (cursor.x, cursor.z);
        }

        true
    }

    /// The plane's gizmo: x across the plane, z along it, on the ground.
    pub(crate) fn flip_gizmo(&self, e: &Editor, dragging: bool) -> Option<Float4x4> {
        let flip = self.flip?;
        let (x, z) = (flip.plane.x, flip.plane.z);
        let id = document_id(e)?;
        let y = if dragging { flip.gizmo_height } else { e.docs.find_as::<TileDocument>(id)?.tile().terrain_height(x, z) };
        let (s, c) = flip.plane.angle.to_radians().sin_cos();
        Some(Float4x4 { m: [[c, 0.0, -s, x], [0.0, 1.0, 0.0, y], [s, 0.0, c, z], [0.0, 0.0, 0.0, 1.0]] })
    }

    /// The gizmo was dragged.
    pub(crate) fn flip_gizmo_moved(&mut self, frame: &Float4x4) {
        let Some(flip) = &mut self.flip else { return };
        let m = &frame.m;
        (flip.plane.x, flip.plane.z, flip.gizmo_height) = (m[0][3], m[2][3], m[1][3]);
        flip.plane.angle = m[2][0].atan2(m[0][0]).to_degrees();
    }

    /// The Inspector in Flip mode. True while it shows.
    pub(crate) fn draw_flip_panel(&mut self, e: &mut Editor) -> bool {
        let Some(mut flip) = self.flip else { return false };
        let camera = e.viewport.camera().target;
        gui::separator_text("Flip");
        if gui::radio_button("Move##flip", !flip.copy) {
            flip.copy = false;
        }

        gui::same_line();
        if gui::radio_button("Copy##flip", flip.copy) {
            flip.copy = true;
        }

        gui::set_next_item_width(160.0);
        gui::drag_float("x##flip", &mut flip.plane.x, 0.5, -100_000.0, 100_000.0, "%.2f m");
        gui::set_next_item_width(160.0);
        gui::drag_float("z##flip", &mut flip.plane.z, 0.5, -100_000.0, 100_000.0, "%.2f m");
        gui::set_next_item_width(160.0);
        gui::drag_float("angle##flip", &mut flip.plane.angle, 0.5, -360.0, 360.0, "%.1f deg");
        if gui::button("Align X##flip") {
            flip.plane.angle = 0.0;
        }

        if gui::is_item_hovered() {
            gui::set_tooltip("The plane across the x axis: the selection goes to the other side along x.");
        }

        gui::same_line();
        if gui::button("Align Z##flip") {
            flip.plane.angle = 90.0;
        }

        if gui::is_item_hovered() {
            gui::set_tooltip("The plane across the z axis: the selection goes to the other side along z.");
        }

        gui::same_line();
        if gui::button("Move plane into view##flip") {
            (flip.plane.x, flip.plane.z) = (camera.x, camera.z);
        }

        self.flip = Some(flip);
        gui::begin_disabled(selected(e).is_empty());
        if gui::button("Flip##flipnow") {
            self.status = self.do_flip(e).unwrap_or_else(|why| why);
        }

        gui::end_disabled();
        gui::same_line();
        if gui::button("Done##flip") {
            self.flip = None;
        }

        gui::push_style_color(gui::COL_TEXT, gui::style_color(gui::COL_TEXT_DISABLED));
        gui::text_wrapped("Its gizmo moves and turns the plane; a press on the ground puts it there. Models are not mirrored: they turn.");
        gui::pop_style_color(1);
        true
    }

    /// The plane, and where the selection would go.
    pub(crate) fn flip_overlay(&self, t: &Tile, sel: &[usize], o: &mut Overlay) {
        let Some(flip) = self.flip else { return };
        let plane = flip.plane;
        let (white, gold) = (overlay_colour(1.0, 1.0, 1.0, 0.55), overlay_colour(1.0, 0.85, 0.2, 1.0));
        let along = plane.along();
        let foot = |d: f32| {
            let (x, z) = (plane.x + along.x * d, plane.z + along.z * d);
            wce_foundation::math::Float3::new(x, t.terrain_height(x, z), z)
        };

        let up = wce_foundation::math::Float3::new(0.0, PLANE_HEIGHT, 0.0);
        let steps = (PLANE_REACH / PLANE_STEP) as i32;
        for k in -steps..=steps {
            let (a, b) = (foot(k as f32 * PLANE_STEP), foot((k + 1) as f32 * PLANE_STEP));
            o.line(a, a + up, white);
            if k != steps {
                o.line(a, b, white);
                o.line(a + up, b + up, white);
            }
        }

        for &s in sel.iter().take(MAX_LINES) {
            let p = t.world_position(s);
            o.line(p, plane.mirror_metres(p), gold);
        }
    }
}
