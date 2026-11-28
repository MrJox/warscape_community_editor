//! The selected river in the Inspector, below its properties (docs/plans/rivers.md, step 3): its long profile (the water and the ground
//! under its middle along it; its points dragged up and down there, those joined to a link fixed), *Make downhill*, *Reverse river*, and a
//! ramp of its points' values between two of them, and its widths randomized within a deviation by a seed (`river_widths.rs`).

use wce_editor::editor::Editor;
use wce_editor::gui::{self, col32, v2};
use wce_scene::render_scene::DocumentId;
use wce_tile_editor::tile_document::TileDocument;
use wce_tile_editor::tile_document::river_profile::RAMPS;
use wce_tile_editor::tile_document::river_widths::DEFAULT_WIDTH_DEV;

use crate::tool::{TileTool, primary};

/// The profile's height (pixels) and a point's pick radius.
const GRAPH_HEIGHT: f32 = 170.0;
const PICK_PX: f32 = 7.0;

/// The profile's state: the point being dragged (the river, its index, the drag's gesture), and the ramp's settings.
pub(crate) struct RiverUi {
    drag: Option<(usize, usize, u64)>,

    /// A press on the selected river draws a new ford (dragged along it from there).
    pub(crate) drawing_ford: bool,

    /// A ford's band in the profile being dragged: its index, its part, the drag's gesture and the history's place when it began.
    band_drag: Option<(usize, wce_tile_editor::tile_document::river_fords::FordPart, u64, usize)>,

    ramp: i32,
    from: i32,
    to: i32,
    values: [f32; 2],

    /// The width variation's settings shown (the deviation from and to, percent; the seed), and the river they were read from.
    width_for: Option<usize>,
    width_dev: [f32; 2],
    width_seed: i32,
}

impl Default for RiverUi {
    fn default() -> Self {
        Self {
            drag: None,
            drawing_ford: false,
            band_drag: None,
            ramp: 0,
            from: 0,
            to: -1,
            values: [45.0, 30.0],
            width_for: None,
            width_dev: [DEFAULT_WIDTH_DEV.0 as f32, DEFAULT_WIDTH_DEV.1 as f32],
            width_seed: 0,
        }
    }
}

impl RiverUi {
    /// A point of the profile is being dragged.
    pub(crate) fn dragging(&self) -> bool {
        self.drag.is_some() || self.band_drag.is_some()
    }
}

impl TileTool {
    /// A river's sections below its profile, the document's (its bed, its shores, the other attributes at the bottom), in the
    /// Inspector's rows.
    pub(crate) fn inspect_river_tail(&mut self, e: &mut Editor, id: DocumentId) {
        use wce_editor_core::inspector::Inspector;
        let Some(river) = primary(e) else { return };
        let Some(d) = e.docs.find_as_mut::<TileDocument>(id) else { return };
        if d.tile().tscene().type_of(river) != "SplineRiverInstance" {
            return;
        }

        let mut ins = wce_editor::panels::ImGuiInspector::default();
        ins.begin(id, wce_tile_editor::tile_document::item_of(river));
        d.inspect_river_tail(river, &mut ins);
    }

    pub(crate) fn inspect_river(&mut self, e: &mut Editor, id: DocumentId) {
        let Some(river) = primary(e) else { return };
        let selection = crate::tool::selected(e);
        let Some(d) = e.docs.find_as_mut::<TileDocument>(id) else { return };
        if d.tile().tscene().type_of(river) != "SplineRiverInstance" {
            self.river_ui.drag = None;
            return;
        }

        let Some(p) = d.river_profile(river) else { return };
        if !gui::collapsing_header("River profile", gui::TREE_DEFAULT_OPEN) {
            return;
        }

        // The ranges: along, and the heights of the water and the ground with a margin.
        let lo = p.samples.iter().flat_map(|s| [s.water, s.ground]).fold(f64::INFINITY, f64::min) - 1.0;
        let hi = p.samples.iter().flat_map(|s| [s.water, s.ground]).fold(f64::NEG_INFINITY, f64::max) + 1.0;
        let width = gui::content_region_avail().x.max(120.0);
        let at = gui::cursor_screen_pos();
        gui::invisible_button("##river_profile", v2(width, GRAPH_HEIGHT), 0);
        let hovered = gui::is_item_hovered();
        let active = gui::is_item_active();
        let to_px = |along: f64, y: f64| {
            v2(
                at.x + (along / p.length.max(1e-6)) as f32 * width,
                at.y + GRAPH_HEIGHT - ((y - lo) / (hi - lo).max(1e-6)) as f32 * GRAPH_HEIGHT,
            )
        };
        let dl = gui::window_draw_list();
        dl.rect_filled(at, v2(at.x + width, at.y + GRAPH_HEIGHT), col32(24, 26, 30, 255), 3.0);
        // The fords as bands over their length.
        for &(a, l) in &p.fords {
            let (x0, x1) = (to_px(a - l * 0.5, lo).x.max(at.x), to_px(a + l * 0.5, lo).x.min(at.x + width));
            dl.rect_filled(v2(x0, at.y), v2(x1, at.y + GRAPH_HEIGHT), col32(60, 90, 60, 160), 0.0);
        }

        let ground = p.samples.iter().map(|s| to_px(s.along, s.ground)).collect::<Vec<_>>();
        let water = p.samples.iter().map(|s| to_px(s.along, s.water)).collect::<Vec<_>>();
        dl.polyline(&ground, col32(150, 120, 80, 255), false, 2.0);
        dl.polyline(&water, col32(80, 160, 255, 255), false, 2.0);

        // The points: fixed ones (joined to a link) grey squares, the others circles; the one under the mouse or dragged lit.
        let mouse = gui::mouse_pos();
        let near = p
            .points
            .iter()
            .filter(|q| !q.locked)
            .map(|q| (q, to_px(q.along, q.water)))
            .filter(|(_, c)| (c.x - mouse.x).hypot(c.y - mouse.y) <= PICK_PX)
            .min_by(|a, b| (a.1.x - mouse.x).abs().total_cmp(&(b.1.x - mouse.x).abs()))
            .map(|(q, _)| q.index);
        for q in &p.points {
            let c = to_px(q.along, q.water);
            let lit = self.river_ui.drag.is_some_and(|(r, i, _)| r == river && i == q.index) || (hovered && near == Some(q.index));
            if q.locked {
                dl.rect_filled(v2(c.x - 4.0, c.y - 4.0), v2(c.x + 4.0, c.y + 4.0), col32(170, 170, 170, 255), 0.0);
            } else {
                dl.circle_filled(c, if lit { 6.0 } else { 4.5 }, if lit { col32(255, 220, 60, 255) } else { col32(230, 240, 255, 255) });
            }
        }

        dl.text(v2(at.x + 4.0, at.y + 2.0), col32(200, 200, 200, 255), &format!("{hi:.1} m"));
        dl.text(v2(at.x + 4.0, at.y + GRAPH_HEIGHT - 16.0), col32(200, 200, 200, 255), &format!("{lo:.1} m   {:.0} m long", p.length));

        // A press on a point starts its drag; held, the point follows the mouse up and down; let go, the drag's entry closes.
        // A ford's band: near one of its edges, that end; inside it, the whole ford (when no point is under the mouse).
        use wce_tile_editor::tile_document::river_fords::FordPart;
        let band = p.fords.iter().enumerate().find_map(|(k, &(a, l))| {
            let (x0, x1) = (to_px(a - l * 0.5, lo).x, to_px(a + l * 0.5, lo).x);
            if (mouse.x - x0).abs() <= 5.0 {
                Some((k, FordPart::Start))
            } else if (mouse.x - x1).abs() <= 5.0 {
                Some((k, FordPart::End))
            } else {
                (mouse.x > x0 && mouse.x < x1).then_some((k, FordPart::Middle))
            }
        });
        if hovered && gui::is_mouse_clicked(0) {
            self.river_ui.drag = near.map(|i| (river, i, d.next_gesture()));
            if self.river_ui.drag.is_none() {
                use wce_editor_core::document::Document;
                let history = d.history().position();
                self.river_ui.band_drag = band.map(|(k, part)| (k, part, d.next_gesture(), history));
            }
        }

        if let Some((k, part, g, _)) = self.river_ui.band_drag {
            if active {
                let along = f64::from((mouse.x - at.x) / width) * p.length;
                d.move_ford_part(river, k, part, along, g);
            } else {
                use wce_editor_core::document::Document;
                self.river_ui.band_drag = None;
                d.history_mut().break_merge();
            }
        }

        if hovered && self.river_ui.drag.is_none() && self.river_ui.band_drag.is_none() && band.is_some() {
            gui::set_mouse_cursor_resize_ew();
        }

        if let Some((r, i, g)) = self.river_ui.drag {
            if active && r == river {
                let y = lo + f64::from((at.y + GRAPH_HEIGHT - mouse.y) / GRAPH_HEIGHT) * (hi - lo);
                if let Err(why) = d.set_river_heights(river, &[(i, y)], "Move river point height", g) {
                    self.status = why;
                }
            } else {
                self.river_ui.drag = None;
            }
        }

        if hovered && self.river_ui.drag.is_none() {
            gui::set_tooltip(
                "The river's water (blue) and the ground under its middle (brown) along it, from its first point. Drag a point up or \
                 down to set its water; the grey ones are joined to a river link and keep the link's height.",
            );
        }

        if gui::button("Make downhill") {
            self.status = match d.make_river_downhill(river) {
                Ok(0) => "the river runs downhill already".to_owned(),
                Ok(n) => format!("{n} point(s) lowered or raised to run downhill"),
                Err(why) => why,
            };
        }

        if gui::is_item_hovered() {
            gui::set_tooltip(
                "From its higher end to its lower one, no point higher than the one before it; the points joined to a link keep their \
                 height (a river between two links at one height runs level).",
            );
        }

        gui::same_line();
        if gui::button("Reverse river") {
            self.status = match d.flip_spline(river) {
                Ok(()) => "the river runs the other way".to_owned(),
                Err(why) => why,
            };
        }

        if gui::is_item_hovered() {
            gui::set_tooltip("Its points the other way round: its first point is its last. The arrows on it show the way it runs.");
        }

        // Split at the point picked in the viewport; join with another river selected with it.
        let point = self.poly_point.filter(|&i| i > 0 && i + 1 < p.points.len());
        gui::same_line();
        gui::begin_disabled(point.is_none());
        if gui::button("Split here")
            && let Some(i) = point
        {
            self.status = match d.split_river(river, i) {
                Ok(_) => format!("split at point {i}: two rivers"),
                Err(why) => why,
            };
        }

        gui::end_disabled();
        if gui::is_item_hovered_allow_disabled() {
            gui::set_tooltip(
                "Splits the river at the point picked in the viewport (not an end): the part after it becomes a river of its own.",
            );
        }

        let other = selection.into_iter().find(|&s| s != river && d.tile().tscene().type_of(s) == "SplineRiverInstance");
        if let Some(other) = other {
            gui::same_line();
            if gui::button("Join rivers") {
                self.status = match d.join_rivers(river, other) {
                    Ok(()) => "joined into one river".to_owned(),
                    Err(why) => why,
                };
            }

            if gui::is_item_hovered() {
                gui::set_tooltip("Joins the two selected rivers whose ends meet into one (either turned round as needed).");
            }
        }

        // Fords: a stretch raised to a wading depth, at the picked point or a place along it.
        gui::separator_text("Fords");
        let drawing = self.river_ui.drawing_ford;
        if drawing {
            gui::push_style_color(gui::COL_BUTTON, gui::style_color(gui::COL_BUTTON_ACTIVE));
        }

        let label = if drawing { "Drawing a ford: press on the river and drag##drawford" } else { "Draw ford in the viewport##drawford" };
        let clicked = gui::button_sized(label, v2(gui::content_region_avail().x, 0.0));
        if drawing {
            gui::pop_style_color(1);
        }

        if clicked {
            self.river_ui.drawing_ford = !drawing;
            self.status = if drawing {
                String::new()
            } else {
                "press on the river's water and drag along it: the ford runs from there to where you let go".to_owned()
            };
        }

        if gui::is_item_hovered() {
            gui::set_tooltip(
                "Press on the river's water in the viewport and drag along it: a ford from there to where you let go. A ford's handles on \
                 the water (its start, middle and end) drag it along the river, as do its band's edges and middle in the profile above.",
            );
        }

        let mut remove = None;
        for (k, &(a, l)) in p.fords.iter().enumerate() {
            gui::row(&format!("ford {}", k + 1));
            gui::text(&format!("at {a:.0} m, {l:.0} m long"));
            gui::same_line();
            if gui::small_button(&format!("{}##ford{k}", wce_editor::icons::X)) {
                remove = Some(k);
            }

            if gui::is_item_hovered() {
                gui::set_tooltip("Take the ford away");
            }
        }

        if let Some(k) = remove {
            d.remove_river_ford(river, k);
        }

        // The ramp: a value from one point to another, by the distance along.
        let n = p.points.len() as i32;
        if self.river_ui.to < 0 || self.river_ui.to >= n {
            self.river_ui.to = n - 1;
        }

        self.river_ui.from = self.river_ui.from.clamp(0, n - 1);
        gui::separator_text("Gradual change along the river");
        if gui::is_item_hovered() {
            gui::set_tooltip(
                "Changes one setting of the river's points gradually from one point to another, by their distance along it: a river \
                 widening towards its mouth, its current slowing as it widens, its foam fading.",
            );
        }

        let labels = RAMPS.iter().map(|r| r.1).collect::<Vec<_>>();
        gui::row_tip("setting", "What changes: the water's width (the game's metres), its flow speed, its foam or its terrain offset.");
        gui::set_next_item_width(-1.0);
        gui::combo("##river_ramp_value", &mut self.river_ui.ramp, &labels);
        gui::row_tip(
            "from point",
            "The point the change starts at: 0 is the river's first point, the first dot of the profile above from the left.",
        );
        gui::set_next_item_width(-1.0);
        gui::slider_int("##river_ramp_from", &mut self.river_ui.from, 0, n - 1);
        gui::row_tip("to point", "The point the change ends at.");
        gui::set_next_item_width(-1.0);
        gui::slider_int("##river_ramp_to", &mut self.river_ui.to, 0, n - 1);
        gui::row_tip(
            "start, end value",
            "The setting at the start point and at the end point; the points between take it by their distance along.",
        );
        gui::set_next_item_width(-1.0);
        gui::drag_float2("##river_ramp_values", &mut self.river_ui.values, 0.1, "%.2f");
        gui::row("");
        if gui::button_sized("Apply##river_ramp", v2(-1.0, 0.0)) {
            let (attr, _) = RAMPS[self.river_ui.ramp.clamp(0, RAMPS.len() as i32 - 1) as usize];
            let [a, b] = self.river_ui.values;
            self.status =
                match d.ramp_river(river, attr, self.river_ui.from as usize, self.river_ui.to as usize, f64::from(a), f64::from(b)) {
                    Ok(()) => format!("{attr} changed gradually from point {} to point {}", self.river_ui.from, self.river_ui.to),
                    Err(why) => why,
                };
        }

        if gui::is_item_hovered() {
            gui::set_tooltip(
                "Sets the value on the points from one to the other, from the first number to the second by their distance along the \
                 river (a river widening towards its mouth). The width is the game's (the river file's), half the point's for today's TEd.",
            );
        }

        // The widths randomized: each point's within a deviation, drawn by a seed (the river's own settings, read when it is picked).
        if self.river_ui.width_for != Some(river) {
            let s = d.tile().river_width_settings(river);
            self.river_ui.width_for = Some(river);
            self.river_ui.width_dev = [s.min as f32, s.max as f32];
            self.river_ui.width_seed = (s.seed % 1_000_000) as i32;
        }

        gui::separator_text("Width variation");
        if gui::is_item_hovered() {
            gui::set_tooltip(
                "Each point's width changed by a deviation drawn between the two percentages by the seed: the same seed, the same river. \
                 Randomizing again starts from the widths it had before (no piling up); the points joined to a river link keep theirs. \
                 Its points' widths are also dragged in the viewport, by the squares on the edges of its water.",
            );
        }

        gui::row_tip(
            "deviation (%)",
            "How much narrower (the first, negative) to how much wider (the second) a point may get, in percent.",
        );
        gui::set_next_item_width(-1.0);
        if gui::drag_float2("##river_width_dev", &mut self.river_ui.width_dev, 0.5, "%.0f %%") {
            for v in &mut self.river_ui.width_dev {
                *v = v.clamp(-90.0, 300.0);
            }
        }

        gui::row_tip("seed", "The draw: another seed, another set of widths within the same deviation.");
        let reroll = gui::frame_height();
        gui::set_next_item_width(gui::content_region_avail().x - reroll - gui::item_spacing().x);
        gui::input_int("##river_width_seed", &mut self.river_ui.width_seed);
        self.river_ui.width_seed = self.river_ui.width_seed.max(0);
        gui::same_line();
        let mut apply = false;
        if crate::styled::icon_button(wce_editor::icons::REFRESH_CW, "river_width_reroll", "Another seed, and randomize with it") {
            self.river_ui.width_seed = ((self.river_ui.width_seed as u32).wrapping_mul(2_654_435_761).wrapping_add(1) % 1_000_000) as i32;
            apply = true;
        }

        gui::row("");
        let half = (gui::content_region_avail().x - gui::item_spacing().x) * 0.5;
        apply |= gui::button_sized("Randomize widths", v2(half, 0.0));
        if gui::is_item_hovered() {
            gui::set_tooltip("Each point's width (but those joined to a river link) within the deviation, by the seed; one undo entry.");
        }

        if apply {
            let [a, b] = self.river_ui.width_dev;
            self.status = match d.randomize_river_widths(river, f64::from(a), f64::from(b), self.river_ui.width_seed as u64) {
                Ok(n) => format!("{n} point width(s) randomized"),
                Err(why) => why,
            };
        }

        gui::same_line();
        if gui::button_sized("Reset widths", v2(half, 0.0)) {
            self.status = match d.reset_river_widths(river) {
                Ok(0) => "no randomized width to put back".to_owned(),
                Ok(n) => format!("{n} point width(s) put back"),
                Err(why) => why,
            };
        }

        if gui::is_item_hovered() {
            gui::set_tooltip("Puts back the widths the points had before they were randomized (not those set by hand since).");
        }
    }
}
