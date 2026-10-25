//! The Tile tool's actions outside its panels: the terrain brushes and the snapping in the toolbar (the brush's settings in the Inspector
//! while one is active), and what can be done to the selection, in the viewport's right-click menu and in the Edit menu.

use wce_editor::editor::Editor;
use wce_editor::{gui, icons};
use wce_editor_core::document::Document;
use wce_editor_core::inspector::Inspector;
use wce_scene::render_scene::SourceRef;
use wce_tile_editor::tile_document::{Placement, elem_of};

use crate::tool::{TileMode, TileTool, document, document_id, primary, selected};

/// The terrain modes in the toolbar (`TileMode` order after Select), their icons and tips.
const BRUSHES: [(&str, &str); 5] = [
    (icons::ARROW_BIG_UP, "Raise the terrain"), (icons::ARROW_BIG_DOWN, "Lower the terrain"), (icons::WAVES, "Smooth the terrain"),
    (icons::MINUS, "Flatten the terrain to a height"), (icons::RULER, "Level the terrain"),
];

impl TileTool {
    /// The toolbar: the terrain brushes (a click on the active one goes back to selecting) and the snapping.
    pub(crate) fn draw_toolbar(&mut self, e: &mut Editor) {
        if document_id(e).is_none() {
            return;
        }

        gui::text_disabled("|");
        gui::same_line();
        for (i, (icon, tip)) in BRUSHES.iter().enumerate() {
            let mode = TileMode::from_index(i + 1);
            let on = self.mode == mode;
            if on {
                gui::push_style_color(gui::COL_BUTTON, gui::style_color(gui::COL_BUTTON_ACTIVE));
            }

            if gui::small_button(&format!("{icon}##brush{i}")) {
                self.set_mode(if on { TileMode::Select } else { mode });
                if !on {
                    // Its settings.
                    e.pending_focus = Some("Inspector".to_owned());
                }
            }

            if on {
                gui::pop_style_color(1);
            }

            if gui::is_item_hovered() {
                gui::set_tooltip(&format!("{tip}\nIts settings are in the Inspector; Esc goes back to selecting"));
            }

            gui::same_line();
        }

        // The snapping: a button lit while it is on, its settings in a popup.
        if self.snap_grid {
            gui::push_style_color(gui::COL_BUTTON, gui::style_color(gui::COL_BUTTON_ACTIVE));
        }

        if gui::small_button(&format!("{}  {} m##snap", icons::GRID_3X3, wce_formats::c_num::printf_g(f64::from(self.grid_m)))) {
            gui::open_popup("tile_snap");
        }

        if self.snap_grid {
            gui::pop_style_color(1);
        }

        if gui::is_item_hovered() {
            gui::set_tooltip("Snapping of the grab (G) and of the placing, and the turn of Ctrl+R / R");
        }

        if gui::begin_popup("tile_snap") {
            gui::checkbox("snap to grid", &mut self.snap_grid);
            gui::set_next_item_width(120.0);
            gui::drag_float("grid (m)", &mut self.grid_m, 0.5, 0.5, 64.0, "%.3f");
            gui::set_next_item_width(120.0);
            gui::drag_float("rotate step (deg)", &mut self.angle_step, 1.0, 1.0, 180.0, "%.3f");
            gui::end_popup();
        }

        gui::same_line();
    }

    /// The Inspector while a terrain brush is active: its settings and the height under the cursor. False otherwise.
    pub(crate) fn inspect_brush(&mut self, e: &mut Editor, ins: &mut dyn Inspector) -> bool {
        if self.mode == TileMode::Select {
            return false;
        }

        let Some(d) = document(e) else { return false };
        ins.begin(d.id(), u64::MAX - 9);
        ins.section(&format!("{} brush", mode_name(self.mode)));
        ins.edit_slider("radius", "radius (m)", &mut self.brush.radius_m, 1.0, 120.0);
        ins.edit_slider("strength", "strength", &mut self.brush.strength, 0.5, 30.0);
        ins.edit_slider("falloff", "falloff", &mut self.brush.falloff, 0.0, 1.0);
        if self.mode == TileMode::Flatten {
            ins.edit_float("flatten", "flatten height (m)", &mut self.brush.flatten_height, 0.1, -1.0e6, 1.0e6);
        }

        let cursor = if self.terrain_hit {
            format!("{:.1}, {:.1}  height {:.2} m", self.cursor.x, self.cursor.z, d.tile().terrain_height(self.cursor.x, self.cursor.z))
        } else {
            "-".to_owned()
        };

        ins.text("cursor", "cursor", &cursor);
        true
    }

    /// A right click in the viewport: what is under the cursor joins the selection when it is not in it (the menu acts on the selection), and
    /// the menu opens.
    pub(crate) fn context_click(&mut self, e: &mut Editor, hit: SourceRef) {
        let Some(id) = document_id(e) else { return };
        if hit.doc == id && elem_of(hit.item).is_some() && !e.selection.contains(hit) {
            e.selection.set(hit);
        }

        self.context_menu = true;
    }

    /// The viewport's right-click menu (drawn in the viewport window).
    pub(crate) fn draw_context_menu(&mut self, e: &mut Editor) {
        if std::mem::take(&mut self.context_menu) {
            gui::open_popup("tile_context");
        }

        if !gui::begin_popup("tile_context") {
            return;
        }

        if gui::menu_item(&format!("{}  Add here...", icons::PLUS), Some("Shift+A"), false, true) {
            self.request_quick_add();
        }

        gui::separator();
        self.selection_items(e);
        gui::end_popup();
    }

    /// What can be done to the selection, as menu items (the viewport's right-click menu and the Edit menu).
    pub(crate) fn selection_items(&mut self, e: &mut Editor) {
        let sel = primary(e);
        let all = selected(e);
        let some = sel.is_some();
        let Some(d) = document(e) else { return };
        let one = |label: &str, icon: &str| format!("{icon}  {label}");
        if gui::menu_item(&one("Grab on the ground", icons::GRAB), Some("G"), false, some) {
            self.begin_grab(e);
            return;
        }

        if gui::menu_item(&one("Drop to the terrain", icons::ARROW_DOWN_TO_LINE), None, false, some) {
            let n = d.snap_to_terrain(&all, true);
            self.status = if n != 0 { format!("on the terrain: {n} item(s)") } else { "already on the terrain".to_owned() };
        }

        if gui::begin_menu_enabled("Placement", some) {
            if gui::menu_item("Landscape (stands on the terrain)", None, false, true) {
                let n = d.set_placement(&all, Placement::Landscape);
                self.status = format!("{n} set to landscape (snapped to the terrain)");
            }

            if gui::menu_item("Freeform (keeps its height)", None, false, true) {
                let n = d.set_placement(&all, Placement::Freeform);
                self.status = format!("{n} set to freeform");
            }

            if gui::menu_item("Snap the landscape ones to the terrain", None, false, true) {
                let n = d.snap_to_terrain(&all, false);
                self.status = format!("{n} landscape item(s) snapped");
            }

            gui::end_menu();
        }

        gui::separator();
        if gui::menu_item(&one("Duplicate", icons::COPY), Some("Ctrl+D"), false, some)
            && let Some(s) = sel
            && let Err(err) = d.duplicate(s)
        {
            self.status = err;
        }

        if gui::menu_item(&one("Turn", icons::ROTATE_CW), Some("Ctrl+R"), false, some)
            && let Some(s) = sel
        {
            d.rotate(s, self.angle_step);
        }

        if gui::menu_item(&one("Group", icons::GROUP), Some("Ctrl+G"), false, some) {
            self.status = self.group_selection(e).unwrap_or_else(|err| err);
            return;
        }

        if gui::menu_item(&one("Ungroup", icons::UNGROUP), Some("Ctrl+U"), false, some) {
            self.status = self.ungroup_selection(e).unwrap_or_else(|err| err);
            return;
        }

        if gui::menu_item(&one("New vegetation set from the selected trees...", icons::TREES), None, false, self.trees_selected(e)) {
            self.request_new_vegetation_set();
        }

        gui::separator();
        if gui::menu_item(&one("Delete", icons::TRASH_2), Some("Del"), false, some) {
            self.request_delete(e);
        }
    }
}

fn mode_name(m: TileMode) -> &'static str {
    match m {
        TileMode::Select => "Select",
        TileMode::Raise => "Raise",
        TileMode::Lower => "Lower",
        TileMode::Smooth => "Smooth",
        TileMode::Flatten => "Flatten",
        TileMode::Level => "Level",
    }
}
