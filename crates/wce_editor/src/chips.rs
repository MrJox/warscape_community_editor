//! The viewport's own chips at its top right (docs/editor.md), for every tool: the camera's (the editor or game view, the camera mode and
//! its options, and the tool's part: the Tile tool's camera views) and the snapping's (the gizmo's steps, and the tool's part: the Tile
//! tool's grid and points). A tool's chips go left of them (`Viewport::chips_width`, `Viewport::chips_top`).

use crate::editor::Editor;
use wce_editor_core::camera_controller::CameraMode;
use crate::{gui, icons, theme};

const GIZMO_STEP_TIP: &str = "The gizmo's own steps (ImGuizmo's snap): a move, a turn or a scale goes by whole steps from where the drag began. \
For every tool; a tool's snapping (below) then applies on top of it.";

/// A chip: an icon, lit while `on`; true when clicked.
pub fn chip(on: bool, icon: &str, id: &str, tip: &str) -> bool {
    if on {
        let h = theme::palette().highlight;
        gui::push_style_color(gui::COL_TEXT, gui::v4(h[0], h[1], h[2], h[3]));
    }

    let clicked = gui::small_button(&format!("{icon}##{id}"));
    if on {
        gui::pop_style_color(1);
    }

    if gui::is_item_hovered() {
        gui::set_tooltip(tip);
    }

    clicked
}

/// The chips at the top right of the image (`origin`, `size`), beside the compass when the row (the tool's chips too) fits, under it
/// otherwise.
pub(crate) fn draw(e: &mut Editor, origin: gui::Vec2, size: gui::Vec2) {
    let vp = &e.viewport;
    let row = vp.chips_width + if vp.tool_chips_width > 0.0 { 4.0 + vp.tool_chips_width } else { 0.0 };
    let compass_right = origin.x + size.x * 0.5 + (size.x - 16.0).min(360.0) * 0.5;
    e.viewport.chips_top = if !vp.show_compass || origin.x + size.x - 8.0 - row - 8.0 >= compass_right { 6.0 } else { 58.0 };
    e.viewport.tool_chips_width = 0.0;

    gui::set_cursor_screen_pos(gui::v2(origin.x + size.x - e.viewport.chips_width - 8.0, origin.y + e.viewport.chips_top));
    gui::push_style_color(gui::COL_CHILD_BG, gui::v4(0.08, 0.08, 0.09, 0.78));
    if gui::begin_child("##viewchips", gui::v2(0.0, 0.0), gui::CHILD_AUTO_RESIZE, gui::WINDOW_NO_SCROLLBAR) {
        draw_snap_chip(e);
        gui::same_line();
        draw_camera_chip(e);
    }

    gui::end_child();
    let (lo, hi) = gui::item_rect();
    e.viewport.chips_width = hi.x - lo.x;
    gui::pop_style_color(1);
}

fn draw_camera_chip(e: &mut Editor) {
    let (mode, game) = (e.viewport.mode(), e.viewport.game_view);
    let tip = format!(
        "Camera: {}, {} view\nClick: the editor or game view, the camera mode and its options, the saved views",
        mode.label(),
        if game { "game" } else { "editor" },
    );

    if chip(game, icons::CAMERA, "camerachip", &tip) {
        gui::open_popup("camerachip");
    }

    if !gui::begin_popup("camerachip") {
        return;
    }

    gui::text_disabled("View");
    for (on, name, tip) in [
        (false, "Editor view", "The scene with the editor's markers, polygons, grid, selection outline and gizmo."),
        (true, "Game view", "The scene as the game shows it: no markers, polygons, grid, selection outline or gizmo."),
    ] {
        if gui::radio_button(&format!("{name}##camerachip"), e.viewport.game_view == on) {
            e.viewport.game_view = on;
        }

        if gui::is_item_hovered() {
            gui::set_tooltip(tip);
        }
    }

    gui::separator_text("Camera");
    for m in CameraMode::ALL {
        if gui::radio_button(&format!("{}##camerachip", m.label()), mode == m) && mode != m {
            e.set_camera_mode(m);
        }
    }

    // Saved once a slider is let go (Editor::draw_menu_bar).
    gui::separator();
    let o = &mut e.viewport.camera_options;
    gui::checkbox("Respect ground##camerachip", &mut o.respect_ground);
    gui::checkbox("Respect collisions (bounding boxes)##camerachip", &mut o.respect_collisions);
    gui::checkbox("Game height limits##camerachip", &mut o.game_limits);
    gui::set_next_item_width(160.0);
    gui::slider_float("Smoothing##camerachip", &mut o.smoothing, 0.0, 1.0, "%.2f");
    gui::set_next_item_width(160.0);
    gui::slider_float("Ground clearance##camerachip", &mut o.ground_clearance, 0.0, 10.0, "%.1f m");
    gui::checkbox("Edge scrolling##camerachip", &mut e.viewport.edge_scroll);

    e.with_active_tool(|t, e| t.camera_chip(e));
    gui::end_popup();
}

fn draw_snap_chip(e: &mut Editor) {
    let tool = e.with_active_tool(|t, e| t.snapping(e)).flatten();
    let gizmo = e.viewport.snap;
    let mut tip = format!("Snapping: {}", if gizmo { "the gizmo's steps" } else { "the gizmo's steps off" });
    if let Some(t) = &tool {
        tip.push_str(&format!(", {t}"));
    }

    tip.push_str("\nClick: the gizmo's steps and the tool's snapping");
    if chip(gizmo || tool.is_some(), icons::MAGNET, "snapchip", &tip) {
        gui::open_popup("snapchip");
    }

    if !gui::begin_popup("snapchip") {
        return;
    }

    gui::separator_text("Gizmo");
    gui::checkbox("Steps##snapchip", &mut e.viewport.snap);
    if gui::is_item_hovered() {
        gui::set_tooltip(GIZMO_STEP_TIP);
    }

    if e.viewport.snap {
        let vp = &mut e.viewport;
        gui::set_next_item_width(120.0);
        gui::drag_float("move step (m)##snapchip", &mut vp.snap_move, 0.1, 0.01, 100.0, "%.2f");
        gui::set_next_item_width(120.0);
        gui::drag_float("turn step (deg)##snapchip", &mut vp.snap_angle, 1.0, 1.0, 180.0, "%.1f");
        gui::set_next_item_width(120.0);
        gui::drag_float("scale step##snapchip", &mut vp.snap_scale, 0.01, 0.01, 10.0, "%.2f");
    }

    e.with_active_tool(|t, e| t.snap_chip(e));
    gui::end_popup();
}
