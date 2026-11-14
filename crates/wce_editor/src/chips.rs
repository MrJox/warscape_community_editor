//! The viewport's own chips at its top right (docs/editor.md), for every tool: the camera's (the editor or game view, the camera mode and
//! its options, and the tool's part: the Tile tool's camera views) and the snapping's (the gizmo's steps, and the tool's part: the Tile
//! tool's grid and points). A tool's chips go left of them (`Viewport::chips_width`, `Viewport::chips_top`).

use crate::editor::Editor;
use crate::{gui, icons, theme};
use wce_editor_core::camera_controller::CameraMode;

const GIZMO_STEP_TIP: &str = "The gizmo's own steps (ImGuizmo's snap): a move, a turn or a scale goes by whole steps from where the drag began. \
For every tool; a tool's snapping (below) then applies on top of it.";

/// A chip's padding: all of its pill.
const CHIP_PADDING: gui::Vec2 = gui::Vec2 { x: 12.0, y: 8.0 };

/// Space between a chip's popup and its pill, and the window's edge.
const POPUP_GAP: f32 = 4.0;
const POPUP_MARGIN: f32 = 8.0;

/// [`gui::begin_popup`] for the chip drawn last: the popup hangs from the pill every frame, so that it follows the pill when the window or the
/// layout changes, on the side of the pill the window has room on, and never longer than that room.
pub fn begin_chip_popup(id: &str) -> bool {
    let (lo, hi) = gui::item_rect();
    let display = gui::io().DisplaySize;
    let right = (lo.x + hi.x) * 0.5 > display.x * 0.5;
    let below = (lo.y + hi.y) * 0.5 < display.y * 0.5;
    let at = gui::v2(if right { hi.x } else { lo.x }, if below { hi.y + POPUP_GAP } else { lo.y - POPUP_GAP });
    gui::set_next_window_pos(at, gui::COND_ALWAYS, gui::v2(if right { 1.0 } else { 0.0 }, if below { 0.0 } else { 1.0 }));
    let room = if below { display.y - at.y } else { at.y } - POPUP_MARGIN;
    gui::set_next_window_size_max(gui::v2(display.x - 2.0 * POPUP_MARGIN, room.max(80.0)));
    gui::begin_popup(id)
}

/// A bar between two groups of chips in a pill, in line with them.
pub fn chip_separator() {
    gui::push_style_var_v(gui::VAR_FRAME_PADDING, gui::v2(2.0, CHIP_PADDING.y));
    gui::align_text_to_frame_padding();
    gui::text_disabled("|");
    gui::pop_style_var(1);
    gui::same_line_touching();
}

thread_local! {
    /// The chip clicked last (its popup is the one open, if any): a click on it while blocked closes the popup rather than opening it again.
    static LAST_CHIP: std::cell::RefCell<String> = const { std::cell::RefCell::new(String::new()) };
}

/// A chip: an icon that is all of its pill (`gui::begin_pill`), lit while `on`; true when clicked.
pub fn chip(on: bool, icon: &str, id: &str, tip: &str) -> bool {
    if on {
        let h = theme::palette().highlight;
        gui::push_style_color(gui::COL_TEXT, gui::v4(h[0], h[1], h[2], h[3]));
    }

    gui::push_style_var_v(gui::VAR_FRAME_PADDING, CHIP_PADDING);
    gui::push_style_color(gui::COL_BUTTON, gui::v4(0.0, 0.0, 0.0, 0.0));
    let pressed = gui::button(&format!("{icon}##{id}"));

    // With another chip's popup open, the click that closes it opens this one too (no second click needed); a click on the chip whose popup
    // is open only closes it, as before.
    let through = gui::is_mouse_clicked(0) && !gui::is_item_hovered() && gui::is_item_hovered_through_popup();
    let own = LAST_CHIP.with(|l| *l.borrow() == id);
    let clicked = pressed || (through && !own);
    if clicked {
        LAST_CHIP.with(|l| id.clone_into(&mut l.borrow_mut()));
    } else if through && own {
        LAST_CHIP.with(|l| l.borrow_mut().clear());
    }
    gui::pop_style_color(1);
    gui::pop_style_var(1);
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
    if gui::begin_pill("##viewchips") {
        draw_snap_chip(e);
        gui::same_line_touching();
        draw_camera_chip(e);
    }

    gui::end_child();
    let (lo, hi) = gui::item_rect();
    e.viewport.chips_width = hi.x - lo.x;
    gui::pop_style_color(1);
}

/// The viewport's mode strip, a vertical pill at the image's left edge (`origin`, `size`), under the tool's chips there: what the left
/// mouse does (Select / Move / Rotate / Scale, 1 to 4), the gizmo's coordinates (Global / Local, L), then the tool's own modes
/// (`Tool::viewport_modes`).
pub(crate) fn draw_mode_strip(e: &mut Editor, origin: gui::Vec2, size: gui::Vec2) {
    const TOP: f32 = 100.0;
    if size.y < TOP + 160.0 {
        return;
    }

    gui::set_cursor_screen_pos(gui::v2(origin.x + 8.0, origin.y + TOP));
    gui::push_style_color(gui::COL_CHILD_BG, gui::v4(0.08, 0.08, 0.09, 0.78));
    if gui::begin_pill("##modestrip") {
        // Whether one of the tool's modes is on is the last frame's (its chips are drawn after these).
        let tool_mode = e.viewport.tool_mode_on;
        for (g, (icon, name)) in
            [(icons::MOUSE_POINTER_2, "Select"), (icons::MOVE, "Move"), (icons::ROTATE_3D, "Rotate"), (icons::SCALE_3D, "Scale")]
                .iter()
                .enumerate()
        {
            let mode = crate::viewport::GizmoMode::from_index(g);
            let on = !tool_mode && e.viewport.gizmo == mode;
            let tip = format!("{name} ({}){}", g + 1, if tool_mode { "\nLeaves the brush" } else { "" });
            if chip(on, icon, &format!("gizmo{g}"), &tip) {
                e.viewport.gizmo = mode;
                e.with_active_tool(|t, e| t.leave_mode(e));
            }
        }

        let local = e.viewport.local_coordinates;
        let tip = if local {
            "Local coordinates (L): the move and rotate gizmo's axes and the Inspector's position and orientation are the parent's (the group, \
             prefab or layer the item is in), as the file holds them. Click: global"
        } else {
            "Global coordinates (L): the move and rotate gizmo's axes and the Inspector's position and orientation are the world's. Click: local \
             (relative to the group, prefab or layer the item is in)"
        };
        gui::separator();
        if chip(local, if local { icons::BOX } else { icons::GLOBE }, "coords", tip) {
            e.viewport.local_coordinates = !local;
        }

        let on = e.with_active_tool(|t, e| t.viewport_modes(e)).unwrap_or(false);
        e.viewport.tool_mode_on = on;
    }

    gui::end_child();
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

    if !begin_chip_popup("camerachip") {
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

    // The editor view's fog (a deviation from the game's look: from high over a map the weather's fog washes everything out).
    let mut fog = e.viewport.fog;
    if e.viewport.game_view {
        gui::begin_disabled(true);
    }

    if gui::slider_float("Fog (editor view)##camerachip", &mut fog, 0.0, 1.0, "%.2f") {
        e.viewport.fog = fog;
    }

    if e.viewport.game_view {
        gui::end_disabled();
    }

    if gui::is_item_hovered() {
        gui::set_tooltip(
            "How much of the weather's fog the editor view draws (1: as the game does). Thinner fog shows a map from high up; the game view always has the game's fog.",
        );
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

    if !begin_chip_popup("snapchip") {
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
