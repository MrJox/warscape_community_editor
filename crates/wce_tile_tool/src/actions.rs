//! The Tile tool's actions outside its panels: the terrain brushes in the toolbar, the snapping in the viewport's snap chip (the brush's settings in the Inspector
//! while one is active), and what can be done to the selection, in the viewport's right-click menu and in the Edit menu.

use std::collections::HashMap;
use std::path::Path;

use wce_editor::editor::Editor;
use wce_editor::steam_detect::file_dialog;
use wce_editor::{gui, icons, theme};
use wce_editor_core::inspector::Inspector;
use wce_scene::render_scene::SourceRef;
use wce_tile::blend_field::PaintMode;
use wce_tile::ground_types::GROUND_TYPES;
use wce_tile_editor::tile_document::{GROUND_ERASE, Placement, elem_of};

use crate::styled;
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
/// The brushes' names in the mode strip's flyouts, in `BRUSHES`' order.
const BRUSH_NAMES: [&str; 9] = ["Raise", "Lower", "Smooth", "Flatten", "Level", "Noise", "Stamp", "Ground textures", "Ground types"];

const BRUSHES: [(&str, &str); 9] = [
    (icons::ARROW_BIG_UP, "Raise the terrain"),
    (icons::ARROW_BIG_DOWN, "Lower the terrain"),
    (icons::WAVES, "Smooth the terrain"),
    (icons::MINUS, "Flatten the terrain to a height"),
    (icons::RULER, "Level the terrain"),
    (icons::MOUNTAIN, "Roughen the terrain with noise: the same pattern wherever the brush goes, deeper the longer you paint"),
    (
        icons::IMAGE,
        "Stamp a shape into the terrain, one click at a time: the image brush's picture (else the brush's falloff) raised to the stamp's height",
    ),
    (icons::BRUSH, "Paint the ground's textures (Ctrl: erase, painting the first group's first variation)"),
    (icons::PALETTE, "Paint the ground types (shown in the ground types view while painting; Ctrl: erase)"),
];

/// The Ground brush on a prefab's stage (its tooltip).
/// What Paint the whole tile does (its tooltip).
const PAINT_WHOLE_TILE_TIP: &str = "One dab of the brush over the whole tile, its border included: the texture chosen above, at the brush's strength, in its mode \
     and only where the Blendmap filters below let it through; no falloff and no image. At strength 1 in normal mode with no filter every pixel becomes \
     that texture. It goes into the working terrain layer as a stroke does. One undo step.";

const FOOTPRINT_TIP: &str = "Paint the prefab's footprint: where a normalized prefab levels the ground it stands on, even where you sculpt nothing \
    (Ctrl: erase). Sculpted ground is in it already.";

/// The ground type a footprint is painted with on the stage (road: it shows well).
pub(crate) const FOOTPRINT_GROUND: u8 = 8;

impl TileTool {
    /// The toolbar: the terrain brushes (a click on the active one goes back to selecting).
    /// The tool's modes in the viewport's mode strip: a Sculpt chip (raise, lower, smooth, flatten, level, noise, stamp) and a Paint chip
    /// (the ground's textures, its ground types; a prefab's ground types paint its footprint, and it has no textures), each opening its
    /// brushes to the right; lit, with the brush's icon, while one of its brushes is on. True while a brush is on.
    pub(crate) fn draw_mode_chips(&mut self, e: &mut Editor) -> bool {
        if document_id(e).is_none() {
            return false;
        }

        let prefab = self.profile == crate::tool::Profile::Prefab;
        let sculpt: Vec<usize> = (0..7).collect();
        let paint: Vec<usize> = if prefab { vec![8] } else { vec![7, 8] };
        gui::separator();
        for (name, icon, brushes) in [("Sculpt", icons::MOUNTAIN, sculpt), ("Paint", icons::BRUSH, paint)] {
            let current = brushes.iter().copied().find(|&i| self.mode == TileMode::from_index(i + 1));
            let shown = current.map_or(icon, |i| BRUSHES[i].0);
            let tip = match current {
                Some(i) => format!(
                    "{name}: {}\nIts settings are in the Inspector; Esc goes back to selecting. Click: another brush",
                    self.brush_tip(i, prefab)
                ),
                None => format!("{name}: click for its brushes"),
            };
            let id = format!("modes{name}");
            if wce_editor::chips::chip(current.is_some(), shown, &id, &tip) {
                gui::open_popup(&id);
            }

            // The brushes open to the right of the strip.
            let (lo, hi) = gui::item_rect();
            gui::set_next_window_pos(gui::v2(hi.x + 6.0, lo.y), gui::COND_ALWAYS, gui::v2(0.0, 0.0));
            if gui::begin_popup(&id) {
                gui::text_disabled(name);
                for &i in &brushes {
                    let mode = TileMode::from_index(i + 1);
                    let (icon, _) = BRUSHES[i];
                    let on = self.mode == mode;
                    if gui::selectable(&format!("{icon}  {}", BRUSH_NAMES[i]), on, 0) {
                        self.set_mode(if on { TileMode::Select } else { mode });
                        if !on {
                            // Its settings.
                            e.pending_focus = Some("Inspector".to_owned());
                        }
                    }

                    if gui::is_item_hovered() {
                        gui::set_tooltip(&format!(
                            "{}\nIts settings are in the Inspector; Esc goes back to selecting",
                            self.brush_tip(i, prefab)
                        ));
                    }
                }

                gui::end_popup();
            }
        }

        self.mode != TileMode::Select
    }

    /// A brush's tooltip (a prefab's ground types paint its footprint).
    fn brush_tip(&self, i: usize, prefab: bool) -> &'static str {
        if prefab && TileMode::from_index(i + 1) == TileMode::Ground { FOOTPRINT_TIP } else { BRUSHES[i].1 }
    }

    /// The tile's snapping in the viewport's snap chip: of the grab (G), of the move gizmo and of the placing (to the grid, to points), and
    /// the turn of Ctrl+R / R.
    pub(crate) fn draw_snap_settings(&mut self) {
        gui::separator_text("Tile");
        if gui::is_item_hovered() {
            gui::set_tooltip(
                "The grab (G), the move gizmo and the placing (to the grid, to points), and the turn of Ctrl+R / R. While grabbing, PageUp / PageDown move a freeform item along the ground's normal.",
            );
        }

        gui::checkbox("snap to grid", &mut self.snap_grid);
        if gui::is_item_hovered() {
            gui::set_tooltip("What is grabbed, moved with the gizmo or placed goes to the grid below. After the points and the edges.");
        }

        gui::same_line();
        gui::checkbox("heights too", &mut self.snap_heights);
        if gui::is_item_hovered() {
            gui::set_tooltip(
                "The grid snaps the height too: a freeform item moved up or down with the gizmo goes to whole grid steps (horizontal and vertical snapping).",
            );
        }

        gui::set_next_item_width(120.0);
        gui::drag_float("grid (m)", &mut self.grid_m, 0.5, 0.5, 64.0, "%.3f");
        if gui::is_item_hovered() {
            gui::set_tooltip("The grid's step in metres (0.5 to 64).");
        }

        gui::checkbox("snap to points", &mut self.snap_vertices);
        if gui::is_item_hovered() {
            gui::set_tooltip(
                "Snap to vertices: what is grabbed or placed goes onto the nearest instance, polygon point or building footprint corner within the tolerance (1.5 m when it is 0). Before the edges and the grid.",
            );
        }

        gui::checkbox("snap to edges", &mut self.snap_edges);
        if gui::is_item_hovered() {
            gui::set_tooltip(
                "What is grabbed or placed goes onto the nearest polygon or line edge or building footprint side within the tolerance (1.5 m when it is 0), sliding along it. After the points, before the grid.",
            );
        }

        gui::checkbox("snap roads to road links", &mut self.snap_road_links);
        if gui::is_item_hovered() {
            gui::set_tooltip(
                "A procedural road's point, drawn or dragged, goes onto the middle of a tile edge a road leaves by (the tile's road links: roads, roads_stone, roads_mountain), within 12 m or the tolerance, so it meets the next tile's road. The links show as rings while a road is drawn or selected. Before the other snapping.",
            );
        }

        gui::checkbox("snap rivers to river links", &mut self.snap_river_links);
        if gui::is_item_hovered() {
            gui::set_tooltip(
                "A river's point, drawn or dragged, goes just past the middle of a river link's cell on the tile's edge (the tile's river and river_shallow targets), within 12 m or the tolerance; the river then takes the link's water height, width and direction, and the ground there is shaped as the game's export shapes it, so the river runs on into the next tile's. The links show as rings while a river is drawn or selected. Before the other snapping.",
            );
        }

        gui::set_next_item_width(120.0);
        gui::drag_float("tolerance (m)", &mut self.snap_tolerance, 0.05, 0.0, 32.0, "%.2f");
        if gui::is_item_hovered() {
            gui::set_tooltip(
                "TEd's snap tolerance. 0: the grid always snaps. Above 0: an axis snaps only when it is this close to a grid line (a magnet), and points snap within it.",
            );
        }

        gui::set_next_item_width(120.0);
        gui::drag_float("rotate step (deg)", &mut self.angle_step, 1.0, 1.0, 180.0, "%.3f");
        if gui::is_item_hovered() {
            gui::set_tooltip("The turn of R / Shift+R while placing, of Ctrl+R / Ctrl+Shift+R and Turn on the selection.");
        }

        gui::set_next_item_width(120.0);
        gui::drag_float("angle tolerance (deg)", &mut self.angle_tolerance, 0.5, 0.0, 90.0, "%.1f");
        if gui::is_item_hovered() {
            gui::set_tooltip(
                "The rotate gizmo: an item's heading goes to a whole rotate step (north-aligned) when it comes this close to one (a magnet). 0: off.",
            );
        }
    }

    /// The snap chip's tooltip: what of the tile's snapping is on.
    pub(crate) fn snapping_summary(&self) -> Option<String> {
        let grid = format!("the {} m grid", wce_formats::c_num::printf_g(f64::from(self.grid_m)));
        let mut parts = vec![];
        if self.snap_vertices {
            parts.push("points".to_owned());
        }

        if self.snap_edges {
            parts.push("edges".to_owned());
        }

        if self.snap_road_links {
            parts.push("roads to road links".to_owned());
        }

        if self.snap_river_links {
            parts.push("rivers to river links".to_owned());
        }

        if self.snap_grid {
            parts.push(if self.snap_heights { format!("{grid} (heights too)") } else { grid });
        }

        if self.angle_tolerance > 0.0 {
            parts.push(format!("headings within {} deg", wce_formats::c_num::printf_g(f64::from(self.angle_tolerance))));
        }

        (!parts.is_empty()).then(|| parts.join(", "))
    }

    /// The brush's settings in the Inspector, in the style of the texture rules' panel (`styled`): a header with the brush's icon and what
    /// it does (its close button goes back to selecting), a label column and a value column, the paint's textures as swatches, the ground
    /// types as coloured chips, then its shape, its filters and the strokes along a line.
    pub(crate) fn inspect_brush(&mut self, e: &mut Editor, _ins: &mut dyn Inspector) -> bool {
        if self.mode == TileMode::Select {
            return false;
        }

        let prefab = self.profile == crate::tool::Profile::Prefab;
        let brush = self.mode.index().saturating_sub(1).min(BRUSHES.len() - 1);
        let swatches = if self.mode == TileMode::Paint { self.ground_swatches(e) } else { Default::default() };
        let mut curve = self.brush_curve(e);
        let mut curve_changed = false;
        let mut image = self.image_brush(e);
        let (image_before, mut load_image) = (image.clone(), false);
        let sel = primary(e);
        let Some(d) = document(e) else { return false };
        let line = sel.filter(|&s| d.tile().is_polygon(s));
        let mut edit_rules = false;

        if styled::inspector_header(&format!("{}  {} brush", BRUSHES[brush].0, mode_name(self.mode)), self.brush_tip(brush, prefab)) {
            self.set_mode(TileMode::Select);
            return true;
        }

        let row = styled::row("Radius", "The brush's reach from its middle, metres (an image brush spans a square this far each way)");
        gui::set_next_item_width(row.width);
        gui::slider_float("##radius", &mut self.brush.radius_m, 1.0, 120.0, "%.1f m");

        if self.mode == TileMode::Ground && prefab {
            // The footprint: painted or not.
            let row = styled::row("Footprint", FOOTPRINT_TIP);
            let current = usize::from(self.ground_type != GROUND_ERASE);
            if let Some(i) = styled::segmented("footprint", &["Erase", "Paint"], Some(current), row.width) {
                self.ground_type = if i == 0 { GROUND_ERASE } else { FOOTPRINT_GROUND };
            }
        } else if self.mode == TileMode::Ground {
            // Hard values: no strength or falloff. The eraser is Ctrl, or "none".
            self.ground_type_chips();
        } else if self.mode == TileMode::Paint {
            let row = styled::row("Strength", "How fast the paint covers: at 1 a second of painting lays 98 % under the brush's middle");
            gui::set_next_item_width(row.width);
            gui::slider_float("##opacity", &mut self.brush.opacity, 0.01, 1.0, "%.2f");
        } else {
            let row = styled::row("Strength", "How fast the brush works: metres a second at its middle");
            gui::set_next_item_width(row.width);
            gui::slider_float("##strength", &mut self.brush.strength, 0.5, 30.0, "%.1f");
        }

        if self.mode == TileMode::Flatten {
            let row = styled::row("Height", "The height the brush flattens to, metres");
            gui::set_next_item_width(row.width);
            gui::drag_float("##flatten", &mut self.brush.flatten_height, 0.1, -1.0e6, 1.0e6, "%.2f m");
        }

        if self.mode == TileMode::Stamp {
            let row = styled::row(
                "Height",
                "What a click adds at the brush's middle (negative: a hollow); the rest follows the brush's shape: the image brush's picture when \
                 it is on, else the falloff.",
            );
            gui::set_next_item_width(row.width);
            gui::drag_float("##stamp_height", &mut self.brush.stamp_height_m, 0.1, -500.0, 500.0, "%.2f m");
        }

        if self.mode == TileMode::Noise {
            let row = styled::row(
                "Feature size",
                "How far apart the noise's bumps are: small for rough, stony ground, large for rolling hills. The strength is metres a second \
                 at the brush's middle.",
            );
            gui::set_next_item_width(row.width);
            gui::slider_float("##noise_scale", &mut self.brush.noise_scale_m, 2.0, 200.0, "%.0f m");
        }

        let mut paint_whole = false;
        if self.mode == TileMode::Paint {
            paint_whole = self.paint_rows(d, &swatches, &mut edit_rules);
        }

        // The shape: an image as the brush, else the falloff slider or the curve of the Brush Falloff window.
        if self.mode != TileMode::Ground {
            gui::separator_text("Shape");
            let (row, _) = styled::toggle_row(
                "imagebrush",
                "Image",
                &mut image.on,
                "A picture's brightness as the brush's weight (TEd's Load Brush; its brushes are in the Assembly Kit's \
                 editor_working_data/EditorBrushes), stretched over the square the radius spans",
            );
            if image.on {
                let name = Path::new(&image.path).file_name().map_or_else(|| "none".to_owned(), |n| n.to_string_lossy().into_owned());
                let load = format!("{}  Load...", icons::FOLDER_OPEN);
                let load_w = gui::button_width(&load);
                gui::text(&name);
                if gui::is_item_hovered() && !image.path.is_empty() {
                    gui::set_tooltip(&image.path);
                }

                gui::same_line_at(row.x + row.width - load_w);
                load_image = gui::button(&load);
                let row = styled::row("Turn", "The picture's turn: an angle, or a new one at random at every step of a stroke");
                let current = usize::from(image.random_angle);
                if let Some(i) = styled::segmented("turnkind", &["Fixed", "Random"], Some(current), row.width) {
                    image.random_angle = i == 1;
                }

                if !image.random_angle {
                    row.next_line();
                    gui::set_next_item_width(row.width);
                    gui::slider_float("##angle", &mut image.angle, 0.0, 360.0, "%.0f deg");
                }
            } else {
                gui::text_disabled("off: the falloff below");
                let (row, clicked) =
                    styled::toggle_row("curve", "Curve", &mut curve.on, "The falloff as a curve drawn in the Brush Falloff window");
                curve_changed = clicked;
                let label = if self.falloff_open { "Close the curve editor" } else { "Edit the curve..." };
                if styled::wide_button(label, row.width, true, "Opens the Brush Falloff window, where the curve is drawn") {
                    self.falloff_open = !self.falloff_open;
                }

                if !curve.on {
                    let row = styled::row("Falloff", "0: a hard edge; 1: a soft bell from the middle to the edge");
                    gui::set_next_item_width(row.width);
                    gui::slider_float("##falloff", &mut self.brush.falloff, 0.0, 1.0, "%.2f");
                }
            }
        }

        if self.mode == TileMode::Paint {
            self.paint_filter_rows();
        }

        // TEd's Spline Paint Stroke and Spline Road along the selected line; the cursor's place.
        gui::separator_text("Along a line");
        let width = gui::content_region_avail().x;
        let along = styled::wide_button(
            "Stroke along the selected line",
            width,
            line.is_some(),
            "The brush, as it is set, stroked along the selected spline, polygon or line in one go (one undo step); a closed polygon all \
             round. A spline whose points have a width (rivers, roads) is painted as wide as it is at each point, as TEd's Spline splat \
             does; other lines with the brush's radius. Select the line first, then pick the brush.",
        );
        let level = styled::wide_button("Level the ground along the selected line", width, line.is_some(), LEVEL_ALONG_TIP);
        let cursor = if self.terrain_hit {
            format!("{:.1}, {:.1}  height {:.2} m", self.cursor.x, self.cursor.z, d.tile().terrain_height(self.cursor.x, self.cursor.z))
        } else {
            "-".to_owned()
        };

        styled::row("Cursor", "The ground under the mouse");
        gui::text_disabled(&cursor);

        if level && let Some(line) = line {
            let d = document(e).expect("a tile");
            let msg = match d.level_along(line, &self.brush) {
                Ok(n) => format!("ground levelled along the line: {n} dabs"),
                Err(why) => why,
            };

            e.status(&msg, false);
        }

        if paint_whole {
            let d = document(e).expect("a tile");
            let msg = match d.paint_whole_tile(wce_tile_editor::tile_document::BrushKind::Paint(self.paint_channel), &self.brush) {
                Ok(true) => "the whole tile painted".to_owned(),
                Ok(false) => "nothing changed".to_owned(),
                Err(why) => why,
            };

            e.status(&msg, false);
        }

        if along && let (Some(line), Some(kind)) = (line, self.brush_kind()) {
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
            let start = if image.path.is_empty() { self.ted_brushes_folder(e) } else { Path::new(&image.path).to_path_buf() };
            if let Some(p) = file_dialog(false, &start, &filters) {
                image.path = p.display().to_string();
            }
        }

        if image != image_before
            && let Err(why) = self.set_image_brush(e, image)
        {
            e.status(&format!("image brush: {why}"), true);
        }

        // Leaves the brush for Select mode, where the Inspector shows the rules.
        if edit_rules {
            self.set_mode(TileMode::Select);
            self.open_texture_rules();
        }

        true
    }

    /// TEd's brushes (the first Assembly Kit's editor_working_data/EditorBrushes), where Load... starts without a picture yet.
    fn ted_brushes_folder(&self, e: &Editor) -> std::path::PathBuf {
        e.settings_ref()
            .assembly_kit_dirs()
            .into_iter()
            .map(|k| k.join("editor_working_data").join("EditorBrushes"))
            .find(|p| p.is_dir())
            .unwrap_or_default()
    }

    /// The ground types as coloured chips, three to a line, "none" (the eraser) first.
    fn ground_type_chips(&mut self) {
        gui::separator_text("Ground type");
        let cols = 3;
        let width = (gui::content_region_avail().x - gui::item_spacing().x * (cols - 1) as f32) / cols as f32;
        let erase = theme::mix(theme::palette_background(), theme::text(), 0.3);
        let all = std::iter::once(("none (erase)", erase, GROUND_ERASE)).chain(GROUND_TYPES.iter().enumerate().map(|(i, (name, rgb))| {
            (*name, [f32::from(rgb[0]) / 255.0, f32::from(rgb[1]) / 255.0, f32::from(rgb[2]) / 255.0, 1.0], i as u8)
        }));
        for (k, (name, colour, value)) in all.enumerate() {
            if k % cols != 0 {
                gui::same_line();
            }

            let tip = if value == GROUND_ERASE { "Takes the painted ground type away (Ctrl erases with any type)" } else { "" };
            if styled::colour_chip(&format!("gt{k}"), name, colour, self.ground_type == value, width, tip) {
                self.ground_type = value;
            }
        }
    }

    /// The paint brush's rows: its mode, the texture group and variation as swatches (or Grouped's and Auto-Flange's numbers), the
    /// texture rules under the paint and Paint the whole tile. True when Paint the whole tile was clicked.
    fn paint_rows(
        &mut self,
        d: &wce_tile_editor::tile_document::TileDocument,
        swatches: &HashMap<(String, u8), u64>,
        edit_rules: &mut bool,
    ) -> bool {
        // What the stroke does: TEd's Blend Map operations (docs/re/terrain.md), explained, since nobody knew what they did.
        const NAMES: [&str; 5] = ["Normal", "Auto-flange", "Rock painting", "Normal (grouped)", "Blur"];
        let mut m = match self.brush.paint_mode {
            PaintMode::Normal => 0,
            PaintMode::AutoFlange { .. } => 1,
            PaintMode::Rock => 2,
            PaintMode::Grouped { .. } => 3,
            PaintMode::Blur => 4,
        };

        let row = styled::row("Mode", "What a stroke does to the ground's textures (hover a mode for what it does)");
        gui::set_next_item_width(row.width);
        if gui::begin_combo("##paint_mode", NAMES[m]) {
            for (i, name) in NAMES.iter().enumerate() {
                if gui::selectable(name, i == m, 0) {
                    m = i;
                }

                if gui::is_item_hovered() {
                    gui::set_tooltip(PAINT_MODE_TIPS[i]);
                }
            }

            gui::end_combo();
        }

        if gui::is_item_hovered() {
            gui::set_tooltip(PAINT_MODE_TIPS[m]);
        }

        self.brush.paint_mode = match m {
            1 => PaintMode::AutoFlange { threshold: self.flange_threshold },
            2 => PaintMode::Rock,
            3 => PaintMode::Grouped { variation: self.grouped_variation },
            4 => PaintMode::Blur,
            _ => PaintMode::Normal,
        };

        // The tile's four texture groups, each with four variations (Auto-Flange picks the variations itself; the blur needs none).
        let groups = d.texture_group_slots();
        let (mut group, mut variation) = (usize::from(self.paint_channel / 4), usize::from(self.paint_channel % 4));
        if m != 4 {
            let row = styled::row("Texture", "The texture group the brush paints (the tile's four)");
            let size = styled::swatch_size(4, row.width, 64.0);
            for i in 0..4 {
                if i > 0 {
                    styled::swatch_gap();
                }

                let name = groups.get(i).filter(|g| !g.is_empty()).cloned();
                let image = name.as_ref().and_then(|g| swatches.get(&(g.clone(), 0)).copied());
                let caption = name.clone().unwrap_or_else(|| format!("{}: none", i + 1));
                if styled::swatch(&format!("pgroup{i}"), image, size, group == i, &caption, &caption) {
                    group = i;
                }
            }
        }

        match m {
            1 => {
                let row = styled::row("Threshold", FLANGE_THRESHOLD_TIP);
                gui::set_next_item_width(row.width);
                gui::slider_float("##flange_threshold", &mut self.flange_threshold, 0.0, 0.95, "%.2f");
                self.brush.paint_mode = PaintMode::AutoFlange { threshold: self.flange_threshold };
            }

            3 => {
                let row = styled::row("Variation", GROUPED_VARIATION_TIP);
                gui::set_next_item_width(row.width);
                gui::slider_float("##grouped_variation", &mut self.grouped_variation, 0.0, 3.0, "%.2f");
                self.brush.paint_mode = PaintMode::Grouped { variation: self.grouped_variation };
            }

            0 | 2 => {
                let g = groups.get(group).cloned().unwrap_or_default();
                let row = styled::row("Variation", "Which of the group's four textures");
                let size = styled::swatch_size(4, row.width, 64.0);
                for v in 0..4u8 {
                    if v > 0 {
                        styled::swatch_gap();
                    }

                    let image = swatches.get(&(g.clone(), v)).copied();
                    let tip = format!("{g}, texture {}", v + 1);
                    if styled::swatch(&format!("pvar{v}"), image, size, variation == usize::from(v), &format!("{}", v + 1), &tip) {
                        variation = usize::from(v);
                    }
                }
            }

            _ => {}
        }

        self.paint_channel = (group.min(3) * 4 + variation.min(3)) as u8;

        // The texture rules lie under what is painted: how many, and where they are edited.
        let shown = match d.texture_rules() {
            Some(r) => format!("{} under your paint", r.rules.iter().filter(|r| r.enabled).count()),
            None => "none".to_owned(),
        };
        let row = styled::row("Texture rules", "The textures the terrain lays by itself, under what you paint");
        let edit = format!("{}  Edit...", icons::LAYERS);
        let edit_w = gui::button_width(&edit);
        gui::text_disabled(&shown);
        gui::same_line_at(row.x + row.width - edit_w);
        if gui::button(&edit) {
            *edit_rules = true;
        }

        gui::spacing();
        styled::wide_button(&format!("{}  Paint the whole tile", icons::BRUSH), gui::content_region_avail().x, true, PAINT_WHOLE_TILE_TIP)
    }

    /// TEd's Blendmap Filters: the paint lands only on the heights and slopes in range.
    fn paint_filter_rows(&mut self) {
        gui::separator_text("Filters");
        let (row, _) =
            styled::toggle_row("fheight", "Height", &mut self.filter_height_on, "Paint only where the ground is between these heights");
        if self.filter_height_on {
            let half = (row.width - gui::item_spacing().x) * 0.5;
            let [mut lo, mut hi] = self.filter_height;
            gui::set_next_item_width(half);
            gui::drag_float("##fh_lo", &mut lo, 0.1, -1000.0, 1000.0, "from %.1f m");
            gui::same_line();
            gui::set_next_item_width(half);
            gui::drag_float("##fh_hi", &mut hi, 0.1, -1000.0, 1000.0, "to %.1f m");
            self.filter_height = if lo != self.filter_height[0] { [lo, hi.max(lo)] } else { [lo.min(hi), hi] };
        } else {
            gui::text_disabled("any height");
        }

        let (row, _) =
            styled::toggle_row("fslope", "Slope", &mut self.filter_slope_on, "Paint only where the ground's slope is between these angles");
        if self.filter_slope_on {
            let half = (row.width - gui::item_spacing().x) * 0.5;
            let [mut lo, mut hi] = self.filter_slope;
            gui::set_next_item_width(half);
            gui::slider_float("##fs_lo", &mut lo, 0.0, 90.0, "from %.0f deg");
            gui::same_line();
            gui::set_next_item_width(half);
            gui::slider_float("##fs_hi", &mut hi, 0.0, 90.0, "to %.0f deg");
            self.filter_slope = if lo != self.filter_slope[0] { [lo, hi.max(lo)] } else { [lo.min(hi), hi] };
            row.next_line();
        } else {
            gui::text_disabled("any slope");
            gui::set_cursor_pos_x(row.x);
        }

        if styled::wide_button("Use the no-go range (impassable slopes)", row.width, true, "The slopes units cannot cross") {
            (self.filter_slope_on, self.filter_slope) = (true, [wce_tile::blend_field::NOGO_SLOPE_DEGREES, 90.0]);
        }

        self.sync_paint_filter();
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

        if gui::menu_item(&format!("{}  Add here...", icons::PLUS), wce_editor::shortcuts::label("tile.quick_add").as_deref(), false, true)
        {
            self.request_quick_add();
        }

        if gui::is_item_hovered() {
            gui::set_tooltip("Opens the quick add search: what you pick is placed here.");
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
        if gui::menu_item(&one("Grab on the ground", icons::GRAB), wce_editor::shortcuts::label("tile.grab").as_deref(), false, some) {
            self.begin_grab(e);
            return;
        }

        if gui::is_item_hovered_allow_disabled() {
            gui::set_tooltip("Moves the selection with the mouse over the ground; a click drops it, Esc or a right click puts it back.");
        }

        self.select_menu(e);
        let Some(d) = document(e) else { return };

        if gui::menu_item(&one("Drop to the terrain", icons::ARROW_DOWN_TO_LINE), None, false, some) {
            let n = d.snap_to_terrain(&all, true);
            self.status = if n != 0 { format!("on the terrain: {n} item(s)") } else { "already on the terrain".to_owned() };
        }

        if gui::is_item_hovered_allow_disabled() {
            gui::set_tooltip("Puts the selected items on the ground, the freeform ones too; their placement does not change.");
        }

        if gui::begin_menu_enabled("Placement", some) {
            if gui::menu_item("Landscape (stands on the terrain)", None, false, true) {
                let n = d.set_placement(&all, Placement::Landscape);
                self.status = format!("{n} set to landscape (snapped to the terrain)");
            }

            if gui::is_item_hovered() {
                gui::set_tooltip("Sets the selected items to landscape: they are snapped to the terrain and follow it when it changes.");
            }

            if gui::menu_item("Freeform (keeps its height)", None, false, true) {
                let n = d.set_placement(&all, Placement::Freeform);
                self.status = format!("{n} set to freeform");
            }

            if gui::is_item_hovered() {
                gui::set_tooltip("Sets the selected items to freeform: they keep their height whatever the terrain does.");
            }

            if gui::menu_item("Snap the landscape ones to the terrain", None, false, true) {
                let n = d.snap_to_terrain(&all, false);
                self.status = format!("{n} landscape item(s) snapped");
            }

            if gui::is_item_hovered() {
                gui::set_tooltip("Puts the selected landscape items back on the ground; the freeform ones keep their height.");
            }

            gui::end_menu();
        }

        // Align / distribute the selected instances (the first selected is the one others line up with).
        let several = all.len() >= 2;
        if gui::begin_menu_enabled("Align / distribute", several) {
            use wce_tile_editor::tile_document::AlignTo;
            for (axis, name) in [(0, "x"), (2, "z"), (1, "height")] {
                if gui::begin_menu(&format!("Align {name}")) {
                    for (to, label) in [
                        (AlignTo::First, "to the first selected"),
                        (AlignTo::Min, "to the lowest"),
                        (AlignTo::Centre, "to the middle"),
                        (AlignTo::Max, "to the highest"),
                    ] {
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
                (
                    None,
                    "Distribute on a line",
                    "Even gaps on the straight line from the first selected to the last; the rest are put on it, in their order along it.",
                ),
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
        if gui::menu_item(&one("Copy", icons::COPY), wce_editor::shortcuts::label("tile.copy").as_deref(), false, some) {
            self.copy_selection(e, false);
            return;
        }

        if gui::menu_item(&one("Cut", icons::SCISSORS), wce_editor::shortcuts::label("tile.cut").as_deref(), false, some) {
            self.copy_selection(e, true);
            return;
        }

        let can_paste = !self.clipboard.is_empty();
        if gui::menu_item(&one("Paste here", icons::CLIPBOARD), wce_editor::shortcuts::label("tile.paste").as_deref(), false, can_paste) {
            self.paste(e, true);
            return;
        }

        if gui::is_item_hovered_allow_disabled() {
            gui::set_tooltip("Pastes the copied items with the first one at the mouse on the ground, the rest around it as they were.");
        }

        if gui::menu_item(
            &one("Paste in place", icons::CLIPBOARD),
            wce_editor::shortcuts::label("tile.paste_in_place").as_deref(),
            false,
            can_paste,
        ) {
            self.paste(e, false);
            return;
        }

        if gui::is_item_hovered_allow_disabled() {
            gui::set_tooltip("Pastes the copied items where they were copied from.");
        }

        let Some(d) = document(e) else { return };
        if gui::menu_item(&one("Duplicate", icons::COPY), wce_editor::shortcuts::label("tile.duplicate").as_deref(), false, some)
            && let Some(s) = sel
            && let Err(err) = d.duplicate(s)
        {
            self.status = err;
        }

        if gui::is_item_hovered_allow_disabled() {
            gui::set_tooltip("A copy of the selected item, 4 m along x from it.");
        }

        if gui::menu_item(&one("Flip...", icons::SPLIT), wce_editor::shortcuts::label("tile.flip").as_deref(), false, true) {
            self.flip_asked = true;
        }

        if gui::is_item_hovered() {
            gui::set_tooltip("Flip mode: mirror the selection, or the whole tile, across a plane (settings in the Inspector).");
        }

        if gui::menu_item(&one("Turn", icons::ROTATE_CW), wce_editor::shortcuts::label("tile.turn").as_deref(), false, some)
            && let Some(s) = sel
        {
            d.rotate(s, self.angle_step);
        }

        if gui::is_item_hovered_allow_disabled() {
            gui::set_tooltip("Turns the selected item by the rotate step of the snap settings.");
        }

        if gui::menu_item(&one("Group", icons::GROUP), wce_editor::shortcuts::label("tile.group").as_deref(), false, some) {
            self.status = self.group_selection(e).unwrap_or_else(|err| err);
            return;
        }

        if gui::is_item_hovered_allow_disabled() {
            gui::set_tooltip("Puts the selection into a new instance group in the working layer.");
        }

        if gui::menu_item(&one("Ungroup", icons::UNGROUP), wce_editor::shortcuts::label("tile.ungroup").as_deref(), false, some) {
            self.status = self.ungroup_selection(e).unwrap_or_else(|err| err);
            return;
        }

        if gui::is_item_hovered_allow_disabled() {
            gui::set_tooltip("Takes the selected instance groups apart into the working layer.");
        }

        if gui::menu_item(&one("New set from the selected trees or buildings...", icons::TREES), None, false, self.set_keys_selected(e)) {
            self.request_new_vegetation_set();
        }

        if gui::is_item_hovered_allow_disabled() {
            gui::set_tooltip("Makes a new vegetation set out of the selected trees (asks for its name).");
        }

        gui::separator();
        if gui::menu_item(&one("Delete", icons::TRASH_2), wce_editor::shortcuts::label("tile.delete").as_deref(), false, some) {
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
