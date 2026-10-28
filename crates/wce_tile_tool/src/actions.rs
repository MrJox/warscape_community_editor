//! The Tile tool's actions outside its panels: the terrain brushes and the snapping in the toolbar (the brush's settings in the Inspector
//! while one is active), and what can be done to the selection, in the viewport's right-click menu and in the Edit menu.

use std::path::Path;

use wce_editor::editor::Editor;
use wce_editor::steam_detect::file_dialog;
use wce_editor::{gui, icons};
use wce_editor_core::document::Document;
use wce_editor_core::inspector::Inspector;
use wce_scene::render_scene::SourceRef;
use wce_tile::blend_field::PaintMode;
use wce_tile::ground_types::GROUND_TYPES;
use wce_tile_editor::tile_document::{GROUND_ERASE, Placement, elem_of};

use crate::tool::{TileMode, TileTool, document, document_id, primary, selected};

/// What Level the ground along the selected line does (its tooltip).
pub(crate) const LEVEL_ALONG_TIP: &str = "TEd's Spline Road: levels the ground under the selected line to the line's own height, as wide as the spline is at each point (a \
     road's or river's width; other lines: the brush's radius), the brush's falloff making the banks. Buildings standing on the ground \
     follow it. One undo step.";

/// What each paint mode does (its tooltip): TEd's Blend Map operations as traced in docs/re/terrain.md.
const PAINT_MODE_TIPS: [&str; 5] = [
    "Normal: the texture group's variation gains the brush's weight of what the other textures hold.",
    "Auto-Flange (TEd's): paints the texture group with a graded rim of its four variations. Each pixel builds up a weight while you paint \
     (more dabs, nearer the brush's middle: higher). Above the threshold the pixel becomes all the group, its variation sliding from 0 \
     (the brush's edge, a light touch) to 3 (its middle, more painting); below the threshold only variation 0 fades in. It sets the \
     pixel from how the stroke found it: painting the same spot longer moves it towards variation 3, it does not pile up.",
    "Rock painting (TEd's): the texture's weight follows the slope under the brush: none below about 26 degrees, 0.24 at 45, 0.8 at 60, \
     all of it from about 72. Paint a rock texture over cliffs and it lands on the steep parts only. Unlike TEd's, it follows the brush's \
     strength and falloff and never takes the texture away where there is already more of it than the slope asks for.",
    "Normal (Grouped) (TEd's): paints the texture group at an in-between variation: 1.3 lays 70 % of variation 1 and 30 % of variation 2, \
     so that two variations blend smoothly instead of one replacing the other.",
    "Blur (TEd's blend smoothing): softens the ground's texture edges under the brush; each texture's weight moves towards its average \
     over the pixels around (2 pixels each way), more the longer you paint. No texture group is needed.",
];

/// Normal (Grouped)'s variation (its tooltip).
const GROUPED_VARIATION_TIP: &str = "Where between the group's variations the paint goes: a whole number is that variation, 1.5 is half variation 1 \
    and half variation 2.";

/// Auto-Flange's threshold (its tooltip).
const FLANGE_THRESHOLD_TIP: &str = "The stroke weight (0..1) where the group takes over: under it only variation 0 fades in (a soft rim), over \
    it the pixel is all the group, variations 0 to 3 from the edge inwards. 0: no rim, the group everywhere the brush touches.";

/// The terrain modes in the toolbar (`TileMode` order after Select), their icons and tips.
const BRUSHES: [(&str, &str); 9] = [
    (icons::ARROW_BIG_UP, "Raise the terrain"), (icons::ARROW_BIG_DOWN, "Lower the terrain"), (icons::WAVES, "Smooth the terrain"),
    (icons::MINUS, "Flatten the terrain to a height"), (icons::RULER, "Level the terrain"),
    (icons::MOUNTAIN, "Roughen the terrain with noise: the same pattern wherever the brush goes, deeper the longer you paint"),
    (icons::IMAGE, "Stamp a shape into the terrain, one click at a time: the image brush's picture (else the brush's falloff) raised to the stamp's height"),
    (icons::BRUSH, "Paint the ground's textures (Ctrl: erase, painting the first group's first variation)"),
    (icons::PALETTE, "Paint the ground types (shown in the ground types view while painting; Ctrl: erase)"),
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

        self.draw_camera_views_button(e);
        gui::same_line();

        // The snapping: a button lit while it is on, its settings in a popup.
        let snapping = self.snap_grid || self.snap_vertices;
        if snapping {
            gui::push_style_color(gui::COL_BUTTON, gui::style_color(gui::COL_BUTTON_ACTIVE));
        }

        if gui::small_button(&format!("{}  {} m##snap", icons::GRID_3X3, wce_formats::c_num::printf_g(f64::from(self.grid_m)))) {
            gui::open_popup("tile_snap");
        }

        if snapping {
            gui::pop_style_color(1);
        }

        if gui::is_item_hovered() {
            gui::set_tooltip("Snapping of the grab (G), of the move gizmo and of the placing (to the grid, to points), and the turn of Ctrl+R / R. While grabbing, PageUp / PageDown move a freeform item along the ground's normal.");
        }

        if gui::begin_popup("tile_snap") {
            gui::checkbox("snap to grid", &mut self.snap_grid);
            gui::set_next_item_width(120.0);
            gui::drag_float("grid (m)", &mut self.grid_m, 0.5, 0.5, 64.0, "%.3f");
            gui::checkbox("snap to points", &mut self.snap_vertices);
            if gui::is_item_hovered() {
                gui::set_tooltip("TEd's Snap to vertices: what is grabbed or placed goes onto the nearest instance or polygon point within the tolerance (1.5 m when it is 0). Before the grid.");
            }

            gui::set_next_item_width(120.0);
            gui::drag_float("tolerance (m)", &mut self.snap_tolerance, 0.05, 0.0, 32.0, "%.2f");
            if gui::is_item_hovered() {
                gui::set_tooltip("TEd's snap tolerance. 0: the grid always snaps. Above 0: an axis snaps only when it is this close to a grid line (a magnet), and points snap within it.");
            }
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

        let mut curve = self.brush_curve(e);
        let mut curve_changed = false;
        let mut image = self.image_brush(e);
        let (image_before, mut load_image) = (image.clone(), false);
        let sel = primary(e);
        let Some(d) = document(e) else { return false };
        let line = sel.filter(|&s| d.tile().is_polygon(s));
        ins.begin(d.id(), u64::MAX - 9);
        ins.section(&format!("{} brush", mode_name(self.mode)));
        ins.edit_slider("radius", "radius (m)", &mut self.brush.radius_m, 1.0, 120.0);
        if self.mode == TileMode::Ground {
            // Hard values: no strength or falloff. The eraser is Ctrl, or "none".
            let mut names = vec!["none (erase)".to_owned()];
            names.extend(GROUND_TYPES.iter().map(|(n, _)| (*n).to_owned()));
            let mut i = if self.ground_type == GROUND_ERASE { 0 } else { i32::from(self.ground_type) + 1 };
            ins.edit_choice("ground_type", "ground type", &mut i, &names);
            self.ground_type = if i <= 0 { GROUND_ERASE } else { (i - 1).min(GROUND_TYPES.len() as i32 - 1) as u8 };
        } else if self.mode == TileMode::Paint {
            ins.edit_slider("opacity", "strength", &mut self.brush.opacity, 0.01, 1.0);
        } else {
            ins.edit_slider("strength", "strength", &mut self.brush.strength, 0.5, 30.0);
        }

        if self.mode != TileMode::Ground {
            // An image as the brush, else the falloff slider or the curve of the Brush Falloff window.
            ins.edit_bool("image", "image brush", &mut image.on);
            if image.on {
                let name = Path::new(&image.path).file_name().map_or_else(|| "(none)".to_owned(), |n| n.to_string_lossy().into_owned());
                ins.text("image_file", "image", &name);
                load_image = ins.button("load_image", "Load image...", true);
                ins.edit_bool("random_angle", "random turn", &mut image.random_angle);
                if !image.random_angle {
                    ins.edit_slider("angle", "turn (deg)", &mut image.angle, 0.0, 360.0);
                }
            } else {
                curve_changed = ins.edit_bool("curve", "falloff curve", &mut curve.on);
                if !curve.on {
                    ins.edit_slider("falloff", "falloff", &mut self.brush.falloff, 0.0, 1.0);
                }

                if ins.button("edit_curve", if self.falloff_open { "Close the curve editor" } else { "Edit the curve..." }, true) {
                    self.falloff_open = !self.falloff_open;
                }
            }
        }

        if self.mode == TileMode::Paint {
            // What the stroke does: TEd's Blend Map operations (docs/re/terrain.md), explained, since nobody knew what they did.
            let mut m = match self.brush.paint_mode {
                PaintMode::Normal => 0,
                PaintMode::AutoFlange { .. } => 1,
                PaintMode::Rock => 2,
                PaintMode::Grouped { .. } => 3,
                PaintMode::Blur => 4,
            };

            let names = ["normal", "auto-flange", "rock painting", "normal (grouped)", "blur"].map(str::to_owned);
            let tips = PAINT_MODE_TIPS.map(str::to_owned);
            ins.edit_choice_tips("paint_mode", "mode", &mut m, &names, &tips);
            self.brush.paint_mode = match m {
                1 => PaintMode::AutoFlange { threshold: self.flange_threshold },
                2 => PaintMode::Rock,
                3 => PaintMode::Grouped { variation: self.grouped_variation },
                4 => PaintMode::Blur,
                _ => PaintMode::Normal,
            };

            // The entry's four texture groups, each with four variations (Auto-Flange picks the variations itself).
            let g = d.project().entry.as_ref().map(|en| en.texture_group()).unwrap_or_default();
            let names = [g.red, g.green, g.blue, g.alpha].iter().enumerate().map(|(i, n)| if n.is_empty() { format!("({}: none)", i + 1) } else { n.clone() }).collect::<Vec<_>>();
            let (mut group, mut variation) = (i32::from(self.paint_channel / 4), i32::from(self.paint_channel % 4));
            if m != 4 {
                ins.edit_choice("group", "texture group", &mut group, &names);
            }

            if m == 3 {
                ins.edit_slider("grouped_variation", "variation", &mut self.grouped_variation, 0.0, 3.0);
                ins.tooltip(GROUPED_VARIATION_TIP);
                self.brush.paint_mode = PaintMode::Grouped { variation: self.grouped_variation };
            } else if m == 4 {
                // The blur needs no channel.
            } else if m == 1 {
                ins.edit_slider("flange_threshold", "threshold", &mut self.flange_threshold, 0.0, 0.95);
                ins.tooltip(FLANGE_THRESHOLD_TIP);
                self.brush.paint_mode = PaintMode::AutoFlange { threshold: self.flange_threshold };
            } else {
                ins.edit_choice("variation", "variation", &mut variation, &["0", "1", "2", "3"].map(str::to_owned));
            }

            self.paint_channel = (group.clamp(0, 3) * 4 + variation.clamp(0, 3)) as u8;
        }

        if self.mode == TileMode::Paint {
            // TEd's Blendmap Filters: the paint lands only on the heights and slopes in range.
            ins.section("Blendmap filters");
            ins.edit_bool("filter_height_on", "height filter", &mut self.filter_height_on);
            if self.filter_height_on {
                let [mut lo, mut hi] = self.filter_height;
                ins.edit_float("filter_height_min", "min height (m)", &mut lo, 0.1, -1000.0, 1000.0);
                ins.edit_float("filter_height_max", "max height (m)", &mut hi, 0.1, -1000.0, 1000.0);
                self.filter_height = if lo != self.filter_height[0] { [lo, hi.max(lo)] } else { [lo.min(hi), hi] };
            }

            ins.edit_bool("filter_slope_on", "slope filter", &mut self.filter_slope_on);
            if self.filter_slope_on {
                let [mut lo, mut hi] = self.filter_slope;
                ins.edit_slider("filter_slope_min", "min slope (deg)", &mut lo, 0.0, 90.0);
                ins.edit_slider("filter_slope_max", "max slope (deg)", &mut hi, 0.0, 90.0);
                self.filter_slope = if lo != self.filter_slope[0] { [lo, hi.max(lo)] } else { [lo.min(hi), hi] };
            }

            if ins.button("filter_nogo", "Use the no-go range (impassable slopes)", true) {
                (self.filter_slope_on, self.filter_slope) = (true, [wce_tile::blend_field::NOGO_SLOPE_DEGREES, 90.0]);
            }

            self.sync_paint_filter();
        }

        if self.mode == TileMode::Flatten {
            ins.edit_float("flatten", "flatten height (m)", &mut self.brush.flatten_height, 0.1, -1.0e6, 1.0e6);
        }

        if self.mode == TileMode::Stamp {
            ins.edit_float("stamp_height", "stamp height (m)", &mut self.brush.stamp_height_m, 0.1, -500.0, 500.0);
            ins.tooltip("What a click adds at the brush's middle (negative: a hollow); the rest follows the brush's shape: the image brush's picture when it is on, else the falloff.");
        }

        if self.mode == TileMode::Noise {
            ins.edit_slider("noise_scale", "feature size (m)", &mut self.brush.noise_scale_m, 2.0, 200.0);
            ins.tooltip("How far apart the noise's bumps are: small for rough, stony ground, large for rolling hills. The strength is metres a second at the brush's middle.");
        }

        let cursor = if self.terrain_hit {
            format!("{:.1}, {:.1}  height {:.2} m", self.cursor.x, self.cursor.z, d.tile().terrain_height(self.cursor.x, self.cursor.z))
        } else {
            "-".to_owned()
        };

        ins.text("cursor", "cursor", &cursor);

        // TEd's Spline Paint Stroke: the brush along the selected spline, polygon or line.
        let along = ins.button("stroke_along", "Stroke along the selected line", line.is_some());
        ins.tooltip(
            "The brush, as it is set, stroked along the selected spline, polygon or line in one go (one undo step); a closed polygon all \
             round. A spline whose points have a width (rivers, roads) is painted as wide as it is at each point, as TEd's Spline splat \
             does; other lines with the brush's radius. Select the line first, then pick the brush.",
        );
        let level = ins.button("level_along", "Level the ground along the selected line", line.is_some());
        ins.tooltip(LEVEL_ALONG_TIP);
        if level
            && let Some(line) = line
        {
            let d = document(e).expect("a tile");
            let msg = match d.level_along(line, &self.brush) {
                Ok(n) => format!("ground levelled along the line: {n} dabs"),
                Err(why) => why,
            };

            e.status(&msg, false);
        }

        if along
            && let (Some(line), Some(kind)) = (line, self.brush_kind())
        {
            let d = document(e).expect("a tile");
            let msg = match d.stroke_along(kind, line, &self.brush) {
                Ok(n) => format!("stroked along the line: {n} dabs"),
                Err(why) => why,
            };

            e.status(&msg, false);
        }

        if curve_changed {
            self.set_brush_curve(e, curve, true);
        }

        if load_image {
            let filters = [("Grey image (PNG, DDS)".to_owned(), "*.png *.dds".to_owned()), ("All files".to_owned(), "*.*".to_owned())];
            if let Some(p) = file_dialog(false, Path::new(&image.path), &filters) {
                image.path = p.display().to_string();
            }
        }

        if image != image_before
            && let Err(why) = self.set_image_brush(e, image)
        {
            e.status(&format!("image brush: {why}"), true);
        }

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
        if document(e).is_none() {
            return;
        }

        let one = |label: &str, icon: &str| format!("{icon}  {label}");
        if gui::menu_item(&one("Grab on the ground", icons::GRAB), Some("G"), false, some) {
            self.begin_grab(e);
            return;
        }

        self.select_menu(e);
        let Some(d) = document(e) else { return };

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

        // Align / distribute the selected instances (the first selected is the one others line up with).
        let several = all.len() >= 2;
        if gui::begin_menu_enabled("Align / distribute", several) {
            use wce_tile_editor::tile_document::AlignTo;
            for (axis, name) in [(0, "x"), (2, "z"), (1, "height")] {
                if gui::begin_menu(&format!("Align {name}")) {
                    for (to, label) in [(AlignTo::First, "to the first selected"), (AlignTo::Min, "to the lowest"), (AlignTo::Centre, "to the middle"), (AlignTo::Max, "to the highest")] {
                        if gui::menu_item(label, None, false, true) {
                            let n = d.align(&all, axis, to);
                            self.status = format!("aligned {name}: {n} moved");
                        }
                    }

                    gui::end_menu();
                }

                if axis == 1 && gui::is_item_hovered() {
                    gui::set_tooltip("The freeform ones only: a landscape instance stands on the terrain.");
                }
            }

            gui::separator();
            for (axis, label, tip) in [
                (Some(0), "Distribute along x", "Even gaps along x between the two furthest apart; the rest keep their order and their z."),
                (Some(2), "Distribute along z", "Even gaps along z between the two furthest apart; the rest keep their order and their x."),
                (None, "Distribute on a line", "Even gaps on the straight line from the first selected to the last; the rest are put on it, in their order along it."),
            ] {
                if gui::menu_item(label, None, false, all.len() >= 3) {
                    let n = d.distribute(&all, axis);
                    self.status = format!("distributed: {n} moved");
                }

                if gui::is_item_hovered() {
                    gui::set_tooltip(tip);
                }
            }

            gui::end_menu();
        }

        gui::separator();
        if gui::menu_item(&one("Copy", icons::COPY), Some("Ctrl+C"), false, some) {
            self.copy_selection(e, false);
            return;
        }

        if gui::menu_item(&one("Cut", icons::SCISSORS), Some("Ctrl+X"), false, some) {
            self.copy_selection(e, true);
            return;
        }

        let can_paste = !self.clipboard.is_empty();
        if gui::menu_item(&one("Paste here", icons::CLIPBOARD), Some("Ctrl+V"), false, can_paste) {
            self.paste(e, true);
            return;
        }

        if gui::menu_item(&one("Paste in place", icons::CLIPBOARD), Some("Ctrl+Shift+V"), false, can_paste) {
            self.paste(e, false);
            return;
        }

        let Some(d) = document(e) else { return };
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
        TileMode::Noise => "Noise",
        TileMode::Stamp => "Stamp",
        TileMode::Paint => "Paint",
        TileMode::Ground => "Ground types",
    }
}
