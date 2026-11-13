//! Flip mode: a vertical plane in the viewport and the Inspector's controls for mirroring the selection across it, moved or as a copy
//! (`TileDocument::flip`). The plane has a gizmo of its own that moves and turns it; a press off it puts the plane on the ground there.

use wce_editor::editor::Editor;
use wce_editor::gui::{self, label_column, row};
use wce_foundation::math::Float4x4;
use wce_scene::render_scene::{Overlay, overlay_colour};
use wce_tile_editor::tile_document::{FlipPlane, Tile, TileDocument, TileFlipAxis};

use crate::selection_tools::set_selection;
use crate::tool::{TileTool, document, document_id, selected};

/// How far the plane is drawn along its line, how high, and every how many metres (metres).
const PLANE_REACH: f32 = 1024.0;
const PLANE_HEIGHT: f32 = 60.0;
const PLANE_STEP: f32 = 32.0;

/// What the Whole tile buttons do (their tooltips).
const WHOLE_TILE_TIP_X: &str = "Flips the whole tile about its middle: west and east change places. Every item of every layer (hidden and locked ones \
     too), the height map, the textures, the ground types, the terrain layers and the tile's links go across, as one undo step; the plane above plays no part.";
const WHOLE_TILE_TIP_Z: &str = "Flips the whole tile about its middle: north and south change places. Every item of every layer (hidden and locked ones \
     too), the height map, the textures, the ground types, the terrain layers and the tile's links go across, as one undo step; the plane above plays no part.";

/// The most lines drawn from the selection to where it would go.
const MAX_LINES: usize = 128;

/// One of two choices side by side, the current one lit.
fn choice(label: &str, on: bool, width: f32, tip: &str) -> bool {
    if on {
        gui::push_style_color(gui::COL_BUTTON, gui::style_color(gui::COL_BUTTON_ACTIVE));
    }

    let clicked = gui::button_sized(label, gui::v2(width, 0.0));
    if on {
        gui::pop_style_color(1);
    }

    if gui::is_item_hovered() {
        gui::set_tooltip(tip);
    }

    clicked
}

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
    /// Enters Flip mode with the plane through the selection's centre, or the tile's without a selection (its Whole tile buttons need none).
    pub fn begin_flip(&mut self, e: &mut Editor) -> Result<String, String> {
        let sel = selected(e);
        let d = document(e).ok_or("no tile is open")?;
        let t = d.tile();
        let n = sel.len() as f32;
        let (x, z) = if sel.is_empty() {
            ((d.project().extent_x() * 0.5) as f32, (d.project().extent_z() * 0.5) as f32)
        } else {
            sel.iter().map(|&s| t.world_position(s)).fold((0.0, 0.0), |(x, z), p| (x + p.x / n, z + p.z / n))
        };

        self.stop_making();
        self.flip = Some(FlipMode { plane: FlipPlane { x, z, angle: 0.0 }, copy: false, dragging: false, gizmo_height: 0.0 });
        Ok("flip: its gizmo moves and turns the plane, a press on the ground puts it there; the Inspector flips".to_owned())
    }

    /// Flips the whole tile, its terrain and links too (`TileDocument::flip_tile`).
    pub fn do_flip_tile(&mut self, e: &mut Editor, axis: TileFlipAxis) -> Result<String, String> {
        let id = document_id(e).ok_or("no tile is open")?;
        let d = e.docs.find_as_mut::<TileDocument>(id).ok_or("no tile is open")?;
        d.flip_tile(axis)
    }

    /// A whole-tile flip asked for: a modal says it is under way for two frames (so that it is on screen), then the flip runs.
    pub(crate) fn draw_flip_tile_busy(&mut self, e: &mut Editor) {
        const TITLE: &str = "Flipping the tile";
        /// The flip is done: the message stays one frame more, over the tile being published again (the next frame's start).
        const DONE: u8 = u8::MAX;
        let Some((axis, frames)) = self.flip_tile_pending else { return };
        gui::open_popup(TITLE);
        if gui::begin_popup_modal(TITLE, None, gui::WINDOW_ALWAYS_AUTO_RESIZE) {
            gui::text(&format!("Flipping the whole tile along {}...", if axis == TileFlipAxis::X { "x" } else { "z" }));
            gui::text_disabled("Every item, the terrain and the links: this takes a while on a big tile.");
            if frames == DONE {
                gui::close_current_popup();
            }

            gui::end_popup();
        }

        match frames {
            DONE => self.flip_tile_pending = None,
            0 => {
                let start = std::time::Instant::now();
                self.status = match self.do_flip_tile(e, axis) {
                    Ok(msg) => format!("{msg} ({:.1} s)", start.elapsed().as_secs_f32()),
                    Err(why) => why,
                };
                self.flip_tile_pending = Some((axis, DONE));
            }

            n => self.flip_tile_pending = Some((axis, n - 1)),
        }
    }

    /// Mirrors the selection across the plane.
    pub fn do_flip(&mut self, e: &mut Editor) -> Result<String, String> {
        let flip = self.flip.ok_or("not in Flip mode")?;
        let sel = selected(e);
        let id = document_id(e).ok_or("no tile is open")?;
        let into =
            if flip.copy { Some(document(e).and_then(|d| self.new_items_layer(d)).ok_or("this tile has no scene graph")?) } else { None };
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
        let count = selected(e).len();
        gui::separator_text("Flip");
        gui::text_disabled(&match count {
            0 => "Nothing selected".to_owned(),
            1 => "1 item selected".to_owned(),
            n => format!("{n} items selected"),
        });

        gui::spacing();
        let two = (gui::cursor_pos_x() + gui::content_region_avail().x - label_column() - gui::item_spacing().x) * 0.5;
        row("Action");
        if choice("Move##flip", !flip.copy, two, "The selection goes across the plane.") {
            flip.copy = false;
        }

        gui::same_line();
        if choice("Copy##flip", flip.copy, two, "A mirrored copy goes across, the selection stays.") {
            flip.copy = true;
        }

        gui::spacing();
        gui::separator_text("Plane");
        row("Position (m)");
        gui::set_next_item_width(-1.0);
        let mut at = [flip.plane.x, flip.plane.z];
        if gui::drag_float2("##flipat", &mut at, 0.5, "%.2f") {
            [flip.plane.x, flip.plane.z] = at;
        }

        if gui::is_item_hovered() {
            gui::set_tooltip("A point the plane goes through: x and z, in metres.");
        }

        row("Angle");
        gui::set_next_item_width(-1.0);
        gui::drag_float("##flipangle", &mut flip.plane.angle, 0.5, -360.0, 360.0, "%.1f deg");
        if gui::is_item_hovered() {
            gui::set_tooltip("The plane's heading in degrees: 0 flips along x, 90 along z.");
        }
        row("Align to");
        if choice(
            "X axis##flip",
            flip.plane.angle == 0.0,
            two,
            "The plane across the x axis: the selection goes to the other side along x.",
        ) {
            flip.plane.angle = 0.0;
        }

        gui::same_line();
        if choice(
            "Z axis##flip",
            flip.plane.angle == 90.0,
            two,
            "The plane across the z axis: the selection goes to the other side along z.",
        ) {
            flip.plane.angle = 90.0;
        }

        gui::set_cursor_pos_x(label_column());
        if gui::button_sized("Move plane into view##flip", gui::v2(-1.0, 0.0)) {
            (flip.plane.x, flip.plane.z) = (camera.x, camera.z);
        }

        if gui::is_item_hovered() {
            gui::set_tooltip("Puts the plane through the point the camera looks at; its angle stays.");
        }

        self.flip = Some(flip);
        gui::spacing();
        gui::separator();
        gui::spacing();
        let done = gui::button_width("Done");
        let wide = gui::content_region_avail().x - done - gui::item_spacing().x;
        gui::begin_disabled(count == 0);
        gui::push_style_color(gui::COL_BUTTON, gui::style_color(gui::COL_BUTTON_ACTIVE));
        if gui::button_sized(if flip.copy { "Flip a copy##flipnow" } else { "Flip##flipnow" }, gui::v2(wide, 0.0)) {
            self.status = self.do_flip(e).unwrap_or_else(|why| why);
        }

        if gui::is_item_hovered_allow_disabled() {
            gui::set_tooltip("Mirrors the selection across the plane (moved or as a copy, as chosen above).");
        }

        gui::pop_style_color(1);
        gui::end_disabled();
        gui::same_line();
        if gui::button("Done##flip") {
            self.flip = None;
        }

        if gui::is_item_hovered() {
            gui::set_tooltip("Leaves Flip mode (Esc too).");
        }

        gui::spacing();
        gui::push_style_color(gui::COL_TEXT, gui::style_color(gui::COL_TEXT_DISABLED));
        gui::text_wrapped("Its gizmo moves and turns the plane; a press on the ground puts it there. Models are not mirrored: they turn.");
        gui::pop_style_color(1);

        // The whole tile: everything in it, its terrain and its links, about its middle. The plane above plays no part.
        gui::spacing();
        gui::separator_text("Whole tile");
        row("Flip along");
        let mut axis = None;
        if gui::button_sized("X##fliptile", gui::v2(two, 0.0)) {
            axis = Some(TileFlipAxis::X);
        }

        if gui::is_item_hovered() {
            gui::set_tooltip(WHOLE_TILE_TIP_X);
        }

        gui::same_line();
        if gui::button_sized("Z##fliptile", gui::v2(two, 0.0)) {
            axis = Some(TileFlipAxis::Z);
        }

        if gui::is_item_hovered() {
            gui::set_tooltip(WHOLE_TILE_TIP_Z);
        }

        // The flip of a big tile takes seconds: a message is shown first (`draw_flip_tile_busy`), then it runs.
        if let Some(axis) = axis {
            self.flip_tile_pending = Some((axis, 2));
        }

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
