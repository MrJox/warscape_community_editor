//! The shared panels (inspector, assets, log, history, commands) and the Dear ImGui implementation of the inspector framework.

use wce_editor_core::command_registry::CommandRegistry;
use wce_editor_core::inspector::{InspectTarget, Inspector};
use wce_foundation::log::{self, LogLevel};

use crate::editor::Editor;
use crate::gui::{self, label_column};

// ---- ImGuiInspector

thread_local! {
    /// The curve key being dragged (`Inspector::edit_curve`): the row's property path and the key's index. The inspector is made each frame.
    static CURVE_DRAG: std::cell::RefCell<Option<(String, usize)>> = const { std::cell::RefCell::new(None) };
}

/// Draws one property per call as a Dear ImGui row. `edit_*` return true on every frame the value changed; `gesture_ended` is true on the frame
/// an edit is finished (the widget lost focus after a change).
#[derive(Debug, Default)]
pub struct ImGuiInspector {
    target: InspectTarget,
    ended: bool,

    /// The marked fields and their notes (`Inspector::mark_fields`), for the current target.
    marks: Vec<(String, String)>,

    /// `Inspector::hint` / `dim_next` for the next row, and the hint of the row being drawn (its widget shows it too).
    next_hint: String,
    next_dim: bool,
    next_format: Option<String>,
    row_hint: String,

    /// The toolbar's coordinate system (`Inspector::local_coordinates`).
    local: bool,
}

/// The value column starts after the longest label that fits: 40% of the panel, at least 110 px.
impl ImGuiInspector {
    fn id(&self, field: &str) -> String {
        format!("##{}", self.path(field))
    }

    /// A field's mark note, when it is marked (or a field it is part of).
    fn mark(&self, field: &str) -> Option<&str> {
        self.marks
            .iter()
            .find(|(f, _)| field == f || field.strip_prefix(f.as_str()).is_some_and(|r| r.starts_with('.')))
            .map(|(_, n)| n.as_str())
    }

    /// A label: in the warning colour with the note as its tooltip when its field is marked.
    fn draw_label(&mut self, field: &str, label: &str, disabled: bool) {
        self.row_hint = std::mem::take(&mut self.next_hint);
        let dim = std::mem::take(&mut self.next_dim);
        match self.mark(field) {
            Some(note) => {
                gui::text_colored(crate::theme::warning(), &format!("{} {label}", crate::icons::TRIANGLE_ALERT));
                if gui::is_item_hovered() {
                    gui::set_tooltip(note);
                }
            }

            None => {
                if disabled || dim {
                    gui::text_disabled(label)
                } else {
                    gui::text(label)
                }
                self.show_hint();
            }
        }
    }

    /// The float format of this row: the one `format_next` asked for, else `default`.
    fn take_format(&mut self, default: &str) -> String {
        self.next_format.take().unwrap_or_else(|| default.to_owned())
    }

    /// The row's hint, when the last item is hovered.
    fn show_hint(&self) {
        if !self.row_hint.is_empty() && gui::is_item_hovered() {
            gui::set_tooltip(&self.row_hint);
        }
    }

    fn label(&mut self, field: &str, label: &str, full_width: bool) {
        self.draw_label(field, label, false);
        gui::same_line_at(label_column());
        if full_width {
            gui::set_next_item_width(-1.0);
        }
    }

    fn finish_widget(&mut self) {
        self.ended = gui::is_item_deactivated_after_edit();
        self.show_hint();
    }
}

impl Inspector for ImGuiInspector {
    fn target(&self) -> InspectTarget {
        self.target
    }

    fn local_coordinates(&self) -> bool {
        self.local
    }

    fn set_target(&mut self, target: InspectTarget) {
        if target != self.target {
            self.marks.clear();
        }

        self.target = target;
    }

    fn mark_fields(&mut self, marks: Vec<(String, String)>) {
        self.marks = marks;
    }

    fn hint(&mut self, text: &str) {
        self.next_hint = text.to_owned();
    }

    fn dim_next(&mut self) {
        self.next_dim = true;
    }

    fn format_next(&mut self, format: &str) {
        self.next_format = Some(format.to_owned());
    }

    fn button(&mut self, field: &str, label: &str, enabled: bool) -> bool {
        gui::begin_disabled(!enabled);
        let clicked = gui::small_button(&format!("{label}{}", self.id(field)));
        gui::end_disabled();

        // Its hint, greyed out too (it says why).
        let hint = std::mem::take(&mut self.next_hint);
        if !hint.is_empty() && gui::is_item_hovered_allow_disabled() {
            gui::set_tooltip(&hint);
        }

        clicked && enabled
    }

    fn row_buttons(&mut self, buttons: &[(&str, &str, &str, bool)], note: &str) -> Option<usize> {
        let mut clicked = None;
        for (i, &(field, icon, tip, enabled)) in buttons.iter().enumerate() {
            gui::same_line();
            gui::begin_disabled(!enabled);
            if gui::button(&format!("{icon}{}", self.id(field))) && enabled {
                clicked = Some(i);
            }

            gui::end_disabled();
            if gui::is_item_hovered_allow_disabled() {
                gui::set_tooltip(tip);
            }
        }

        gui::same_line();
        gui::align_text_to_frame_padding();
        gui::text_disabled(note);
        clicked
    }

    fn section(&mut self, title: &str) {
        gui::separator_text(title);
    }

    fn open_section(&mut self, title: &str) -> bool {
        gui::collapsing_header(title, gui::TREE_DEFAULT_OPEN)
    }

    fn text(&mut self, field: &str, label: &str, value: &str) {
        self.draw_label(field, label, true);
        gui::same_line_at(label_column());
        gui::text(value);
        self.show_hint();
    }

    fn images(&mut self, field: &str, label: &str, images: &[(u64, f32, f32, &str)]) {
        self.draw_label(field, label, true);
        for (i, (id, w, h, caption)) in images.iter().enumerate() {
            if i > 0 {
                gui::same_line();
            }

            gui::begin_group();
            gui::image(*id, gui::v2(*w, *h));
            self.show_hint();
            gui::text_disabled(caption);
            gui::end_group();
        }
    }

    fn plot(&mut self, field: &str, label: &str, series: &[([f32; 4], Vec<f32>)], x_end: &str) {
        self.draw_label(field, label, true);
        let w = gui::content_region_avail().x.max(60.0);
        let (lo, size) = (gui::cursor_screen_pos(), gui::v2(w, 64.0));
        let hi = gui::v2(lo.x + size.x, lo.y + size.y);
        let dl = gui::window_draw_list();
        dl.rect_filled(lo, hi, gui::col32(18, 18, 20, 255), 2.0);
        for (colour, values) in series {
            if values.len() < 2 {
                continue;
            }

            let step = size.x / (values.len() - 1) as f32;
            let pts = values
                .iter()
                .enumerate()
                .map(|(i, v)| gui::v2(lo.x + i as f32 * step, hi.y - 1.0 - v.clamp(0.0, 1.0) * (size.y - 2.0)))
                .collect::<Vec<_>>();
            dl.polyline(&pts, crate::theme::u32(*colour), false, 1.5);
        }

        let dim = crate::theme::u32(crate::theme::dim());
        dl.text(gui::v2(lo.x + 4.0, hi.y - 16.0), dim, "0");
        dl.text(gui::v2(hi.x - gui::calc_text_width(x_end) - 4.0, hi.y - 16.0), dim, x_end);
        gui::dummy(size);
        self.show_hint();
    }

    fn edit_curve(&mut self, field: &str, label: &str, keys: &mut Vec<(f32, f32)>, lo: f32, hi: f32, colour: [f32; 4]) -> bool {
        const GRAB: f32 = 7.0;
        self.draw_label(field, label, false);
        let path = self.path(field).to_string();
        let w = gui::content_region_avail().x.max(80.0);
        let (min, size) = (gui::cursor_screen_pos(), gui::v2(w, 96.0));
        let max = gui::v2(min.x + size.x, min.y + size.y);
        let span = if hi - lo > 1e-6 { hi - lo } else { 1.0 };
        let to_px = |(t, v): (f32, f32)| gui::v2(min.x + t.clamp(0.0, 1.0) * size.x, max.y - ((v - lo) / span).clamp(0.0, 1.0) * size.y);
        let from_px = |p: gui::Vec2| (((p.x - min.x) / size.x).clamp(0.0, 1.0), lo + ((max.y - p.y) / size.y).clamp(0.0, 1.0) * span);

        gui::invisible_button(&self.id(field), size, gui::BUTTON_LEFT | gui::BUTTON_RIGHT);
        let (hovered, active) = (gui::is_item_hovered(), gui::is_item_active());
        let mouse = gui::mouse_pos();
        let near = keys.iter().position(|&k| {
            let p = to_px(k);
            (p.x - mouse.x).hypot(p.y - mouse.y) <= GRAB
        });

        let mut changed = false;
        let dragging = CURVE_DRAG.with(|d| d.borrow().as_ref().filter(|(f, _)| *f == path).map(|(_, k)| *k));
        if active && gui::is_mouse_clicked(0) {
            if let Some(k) = near {
                CURVE_DRAG.with(|d| *d.borrow_mut() = Some((path.clone(), k)));
            }
        }

        if let Some(k) = dragging.filter(|&k| k < keys.len()) {
            if active && gui::is_mouse_down(0) {
                // A key stays between its neighbours; the ends keep their t.
                let (mut t, v) = from_px(mouse);
                let first_or_last = k == 0 || k + 1 == keys.len();
                if first_or_last {
                    t = keys[k].0;
                } else {
                    t = t.clamp(keys[k - 1].0, keys[k + 1].0);
                }

                if keys[k] != (t, v) {
                    keys[k] = (t, v);
                    changed = true;
                }
            } else {
                CURVE_DRAG.with(|d| *d.borrow_mut() = None);
                self.ended = true;
            }
        } else if hovered && gui::is_mouse_double_clicked(0) && near.is_none() {
            let (t, v) = from_px(mouse);
            let at = keys.iter().position(|k| k.0 > t).unwrap_or(keys.len());
            keys.insert(at, (t, v));
            changed = true;
            self.ended = true;
        } else if hovered && gui::is_mouse_clicked(1) && keys.len() > 2 {
            if let Some(k) = near {
                keys.remove(k);
                changed = true;
                self.ended = true;
            }
        }

        let dl = gui::window_draw_list();
        dl.rect_filled(min, max, gui::col32(18, 18, 20, 255), 2.0);
        let grid = gui::col32(255, 255, 255, 18);
        for i in 1..4 {
            let x = min.x + size.x * i as f32 / 4.0;
            dl.line(gui::v2(x, min.y), gui::v2(x, max.y), grid, 1.0);
            let y = min.y + size.y * i as f32 / 4.0;
            dl.line(gui::v2(min.x, y), gui::v2(max.x, y), grid, 1.0);
        }

        if !keys.is_empty() {
            // Flat before the first key and after the last (the runtime clamps).
            let mut pts = vec![to_px((0.0, keys[0].1))];
            pts.extend(keys.iter().map(|&k| to_px(k)));
            pts.push(to_px((1.0, keys[keys.len() - 1].1)));
            dl.polyline(&pts, crate::theme::u32(colour), false, 1.5);
        }

        let held = CURVE_DRAG.with(|d| d.borrow().as_ref().filter(|(f, _)| *f == path).map(|(_, k)| *k));
        for (i, &k) in keys.iter().enumerate() {
            let p = to_px(k);
            let hot = held == Some(i) || (held.is_none() && hovered && near == Some(i));
            dl.circle_filled(p, if hot { 5.0 } else { 3.5 }, if hot { gui::col32(255, 255, 255, 255) } else { crate::theme::u32(colour) });
        }

        let dim = crate::theme::u32(crate::theme::dim());
        dl.text(gui::v2(min.x + 4.0, min.y + 2.0), dim, &format!("{hi:.3}"));
        dl.text(gui::v2(min.x + 4.0, max.y - 16.0), dim, &format!("{lo:.3}"));
        if let Some(k) = held.or(near.filter(|_| hovered)).and_then(|k| keys.get(k)) {
            gui::set_tooltip(&format!("t {:.3}, {:.4}", k.0, k.1));
        } else if hovered {
            let tip = "Drag a key to move it, double-click to add one, right-click a key to remove it.";
            gui::set_tooltip(&if self.row_hint.is_empty() { tip.to_owned() } else { format!("{}\n\n{tip}", self.row_hint) });
        }

        changed
    }

    fn gradient(&mut self, field: &str, label: &str, stops: &[(f32, [f32; 4])]) {
        self.draw_label(field, label, true);
        gui::same_line_at(label_column());
        let w = gui::content_region_avail().x.max(40.0);
        let (min, size) = (gui::cursor_screen_pos(), gui::v2(w, gui::frame_height()));
        let dl = gui::window_draw_list();
        let cell = size.y * 0.5;
        let mut x = 0.0;
        while x < size.x {
            for row in 0..2 {
                let dark = ((x / cell) as i32 + row) % 2 == 0;
                let c = if dark { gui::col32(70, 70, 70, 255) } else { gui::col32(110, 110, 110, 255) };
                let y0 = min.y + row as f32 * cell;
                dl.rect_filled(gui::v2(min.x + x, y0), gui::v2(min.x + (x + cell).min(size.x), y0 + cell), c, 0.0);
            }

            x += cell;
        }

        let colour_at = |t: f32| -> [f32; 4] {
            let Some(first) = stops.first() else { return [0.0; 4] };
            if t <= first.0 {
                return first.1;
            }

            for pair in stops.windows(2) {
                let ((t0, c0), (t1, c1)) = (pair[0], pair[1]);
                if t <= t1 {
                    let a = if t1 > t0 { (t - t0) / (t1 - t0) } else { 1.0 };
                    return std::array::from_fn(|i| c0[i] + (c1[i] - c0[i]) * a);
                }
            }

            stops.last().map_or([0.0; 4], |s| s.1)
        };

        let n = (size.x / 3.0).ceil().max(1.0) as usize;
        for i in 0..n {
            let (x0, x1) = (size.x * i as f32 / n as f32, size.x * (i + 1) as f32 / n as f32);
            let c = colour_at((i as f32 + 0.5) / n as f32).map(|v| v.clamp(0.0, 1.0));
            dl.rect_filled(gui::v2(min.x + x0, min.y), gui::v2(min.x + x1 + 0.5, min.y + size.y), crate::theme::u32(c), 0.0);
        }

        gui::dummy(size);
        self.show_hint();
    }

    fn edit_float(&mut self, field: &str, label: &str, v: &mut f32, speed: f32, lo: f32, hi: f32) -> bool {
        self.label(field, label, true);
        let changed = gui::drag_float(&self.id(field), v, speed, lo, hi, &self.take_format("%.3f"));
        self.finish_widget();
        changed
    }

    fn edit_float3(&mut self, field: &str, label: &str, v: &mut [f32; 3], speed: f32) -> bool {
        self.label(field, label, true);
        let changed = gui::drag_float3(&self.id(field), v, speed, &self.take_format("%.3f"));
        self.finish_widget();
        changed
    }

    fn edit_float2(&mut self, field: &str, label: &str, v: &mut [f32; 2], speed: f32) -> bool {
        self.label(field, label, true);
        let changed = gui::drag_float2(&self.id(field), v, speed, &self.take_format("%.6g"));
        self.finish_widget();
        changed
    }

    fn edit_int(&mut self, field: &str, label: &str, v: &mut i64, speed: f32) -> bool {
        self.label(field, label, true);
        let changed = gui::drag_i64(&self.id(field), v, speed);
        self.finish_widget();
        changed
    }

    fn edit_color3(&mut self, field: &str, label: &str, v: &mut [f32; 3]) -> bool {
        self.label(field, label, true);
        let changed = gui::color_edit3(&self.id(field), v, gui::COLOR_FLOAT | gui::COLOR_HDR);
        self.finish_widget();
        changed
    }

    fn edit_color4(&mut self, field: &str, label: &str, v: &mut [f32; 4]) -> bool {
        self.label(field, label, true);
        let changed = gui::color_edit4(&self.id(field), v, gui::COLOR_FLOAT | gui::COLOR_HDR | gui::COLOR_ALPHA_BAR);
        self.finish_widget();
        changed
    }

    fn edit_bool(&mut self, field: &str, label: &str, v: &mut bool) -> bool {
        self.label(field, label, false);
        let changed = gui::checkbox(&self.id(field), v);
        self.ended = changed;
        self.show_hint();
        changed
    }

    fn disabled_bool(&mut self, field: &str, label: &str, v: bool) {
        self.label(field, label, false);
        gui::begin_disabled(true);
        let mut shown = v;
        gui::checkbox(&self.id(field), &mut shown);
        gui::end_disabled();
        self.show_hint();
    }

    fn disabled_float3(&mut self, field: &str, label: &str, v: [f32; 3]) {
        gui::begin_disabled(true);
        let mut shown = v;
        self.edit_float3(field, label, &mut shown, 0.0);
        gui::end_disabled();
    }

    fn edit_string(&mut self, field: &str, label: &str, v: &mut String) -> bool {
        self.label(field, label, true);
        let changed = gui::input_text(&self.id(field), None, v, 512, 0);
        self.finish_widget();
        changed
    }

    fn edit_choice(&mut self, field: &str, label: &str, index: &mut i32, choices: &[String]) -> bool {
        self.label(field, label, true);
        let mut changed = false;
        let shown = usize::try_from(*index).ok().and_then(|i| choices.get(i)).map_or("", String::as_str);
        if gui::begin_combo(&self.id(field), shown) {
            for (i, c) in choices.iter().enumerate() {
                if gui::selectable(c, i as i32 == *index, 0) {
                    *index = i as i32;
                    changed = true;
                }
            }

            gui::end_combo();
        }

        self.ended = changed;
        self.show_hint();
        changed
    }

    fn edit_choice_tips(&mut self, field: &str, label: &str, index: &mut i32, choices: &[String], tips: &[String]) -> bool {
        self.label(field, label, true);
        let mut changed = false;
        let current = usize::try_from(*index).ok();
        let shown = current.and_then(|i| choices.get(i)).map_or("", String::as_str);
        if gui::begin_combo(&self.id(field), shown) {
            for (i, c) in choices.iter().enumerate() {
                if gui::selectable(c, i as i32 == *index, 0) {
                    *index = i as i32;
                    changed = true;
                }

                if let Some(tip) = tips.get(i).filter(|t| !t.is_empty())
                    && gui::is_item_hovered()
                {
                    gui::set_tooltip(tip);
                }
            }

            gui::end_combo();
        } else if let Some(tip) = current.and_then(|i| tips.get(i)).filter(|t| !t.is_empty())
            && gui::is_item_hovered()
        {
            gui::set_tooltip(tip);
        }

        self.ended = changed;
        changed
    }

    fn tooltip(&mut self, text: &str) {
        if gui::is_item_hovered() {
            gui::set_tooltip(text);
        }
    }

    fn gesture_ended(&mut self) -> bool {
        self.ended
    }

    fn edit_slider(&mut self, field: &str, label: &str, v: &mut f32, lo: f32, hi: f32) -> bool {
        self.label(field, label, false);
        gui::set_next_item_width(72.0);
        let mut changed = gui::drag_float(&format!("##{}.box", self.path(field)), v, (hi - lo) * 0.005, lo, hi, "%.3f");
        let mut ended = gui::is_item_deactivated_after_edit();
        gui::same_line();
        gui::set_next_item_width(-1.0);
        changed |= gui::slider_float(&format!("##{}.slider", self.path(field)), v, lo, hi, "");
        ended |= gui::is_item_deactivated_after_edit();
        self.ended = ended;
        self.show_hint();
        changed
    }

    fn edit_int_slider(&mut self, field: &str, label: &str, v: &mut i64, lo: i64, hi: i64) -> bool {
        self.label(field, label, false);
        let mut small = (*v).clamp(lo, hi) as i32;
        gui::set_next_item_width(-1.0);
        let changed = gui::slider_int(&self.id(field), &mut small, lo as i32, hi as i32);
        self.finish_widget();
        if changed {
            *v = i64::from(small);
        }

        changed
    }

    fn edit_angle(&mut self, field: &str, label: &str, radians: &mut f32, max: f32) -> bool {
        self.label(field, label, true);
        let mut deg = radians.to_degrees();
        let changed = gui::drag_float(&self.id(field), &mut deg, 0.5, 0.0, max.to_degrees(), "%.1f\u{b0}");
        self.finish_widget();
        if changed {
            *radians = deg.to_radians();
        }

        changed
    }

    fn edit_checks(&mut self, field: &str, label: &str, names: &[&str], on: &mut [bool]) -> bool {
        self.draw_label(field, label, false);
        let mut changed = false;
        let mut ended = false;
        for (i, name) in names.iter().enumerate() {
            if i % 2 == 1 {
                gui::same_line_at(label_column() + 110.0);
            } else {
                if i > 0 {
                    gui::text("");
                }

                gui::same_line_at(label_column());
            }

            if gui::checkbox(&format!("{name}##{}.{name}", self.path(field)), &mut on[i]) {
                changed = true;
                ended = true;
            }

            self.show_hint();
        }

        self.ended = ended;
        changed
    }
}

// ---- panel state

/// What the panels remember between frames.
#[derive(Debug)]
pub struct PanelState {
    // Assets: the folder (lower case, "" or ending with '/'), the filter, and the listing with what it was built for.
    asset_folder: String,
    asset_filter: String,
    asset_only_tool: bool,
    assets_built_for: String,
    asset_folders: Vec<String>,
    asset_files: Vec<String>,

    // Log.
    show_info: bool,
    show_warn: bool,
    show_error: bool,
    log_seen: u64,

    // Commands.
    command_line: String,
}

impl Default for PanelState {
    fn default() -> Self {
        Self {
            asset_folder: String::new(),
            asset_filter: String::new(),
            asset_only_tool: true,
            assets_built_for: String::new(),
            asset_folders: vec![],
            asset_files: vec![],
            show_info: true,
            show_warn: true,
            show_error: true,
            log_seen: 0,
            command_line: String::new(),
        }
    }
}

// ---- inspector

pub fn inspector(e: &mut Editor) {
    if !gui::begin("Inspector", Some(&mut e.panels.inspector), 0) {
        gui::end();
        return;
    }

    // The active tool's contents first (what it is about to make).
    let mut ins = ImGuiInspector { local: e.viewport.local_coordinates, ..Default::default() };
    if e.with_active_tool(|t, e| t.inspect(e, &mut ins)) == Some(true) {
        gui::end();
        return;
    }

    if e.selection.is_empty() {
        gui::text_disabled("Nothing selected.");
    } else {
        let r = e.selection.items()[0];
        let count = e.selection.items().len();
        match e.docs.find_mut(r.doc) {
            None => gui::text_disabled("The document is closed."),
            Some(d) => {
                // Several: what they have in common, when the document tells (the items of the first one's document).
                if count > 1 {
                    let items = e.selection.items().iter().copied().filter(|i| i.doc == r.doc).collect::<Vec<_>>();
                    let mut ins = ImGuiInspector { local: e.viewport.local_coordinates, ..Default::default() };
                    ins.begin(r.doc, r.item);
                    if d.inspect_many(&items, &mut ins) {
                        e.with_active_tool(|t, e| t.inspect_below(e));
                        gui::end();
                        return;
                    }

                    gui::text_disabled(&format!("{count} items selected, showing the first"));
                }

                let what = d.describe(r);
                gui::text(if what.is_empty() { "Item" } else { &what });
                gui::separator();
                let mut ins = ImGuiInspector { local: e.viewport.local_coordinates, ..Default::default() };
                ins.begin(r.doc, r.item);
                d.inspect(r, &mut ins);
                e.with_active_tool(|t, e| t.inspect_below(e));
            }
        }
    }

    gui::end();
}

// ---- assets

fn rebuild_assets(e: &Editor, st: &mut PanelState) {
    st.asset_folders.clear();
    st.asset_files.clear();
    let Some(vfs) = e.vfs() else { return };
    let exts: &[String] = if st.asset_only_tool { e.active_tool_opens() } else { &[] };
    let filter = st.asset_filter.to_lowercase();
    let mut last_folder = String::new();
    for path in vfs.files().range(st.asset_folder.clone()..).map(|(p, _)| p).take_while(|p| p.starts_with(&st.asset_folder)) {
        let rest = &path[st.asset_folder.len()..];

        // a file counts when it passes the extension and text filters; folders show only if they hold such a file
        let passes = (exts.is_empty() || exts.iter().any(|x| path.ends_with(x.as_str()))) && (filter.is_empty() || path.contains(&filter));
        if let Some(slash) = rest.find('/') {
            let sub = &rest[..slash];
            if passes && sub != last_folder {
                last_folder = sub.to_owned();
                st.asset_folders.push(last_folder.clone());
            }

            continue;
        }

        if !passes {
            continue;
        }

        if st.asset_files.len() < 3000 {
            st.asset_files.push(rest.to_owned());
        }
    }
}

pub fn assets(e: &mut Editor) {
    if !gui::begin("Assets", Some(&mut e.panels.assets), 0) {
        gui::end();
        return;
    }

    let Some(file_count) = e.vfs().map(|v| v.files().len()) else {
        gui::text_disabled("The game packs are not mounted.");
        gui::end();
        return;
    };

    let mut st = std::mem::take(&mut e.panel_data);
    let has_exts = !e.active_tool_opens().is_empty();
    gui::set_next_item_width(220.0);
    gui::input_text("##filter", Some("filter"), &mut st.asset_filter, 128, 0);
    gui::same_line();
    if has_exts {
        gui::checkbox("only this tool's files", &mut st.asset_only_tool);
    }

    gui::same_line();
    gui::text_disabled(&format!("/{}", st.asset_folder));
    let key = format!(
        "{}|{}|{}{}|{file_count}",
        st.asset_folder,
        st.asset_filter,
        e.active_tool_id(),
        if st.asset_only_tool { "1" } else { "0" }
    );
    if key != st.assets_built_for {
        rebuild_assets(e, &mut st);
        st.assets_built_for = key;
    }

    if gui::begin_child("##list", gui::v2(0.0, 0.0), gui::CHILD_BORDERS, 0) {
        if !st.asset_folder.is_empty() && gui::selectable("[..]", false, 0) {
            let f = &st.asset_folder[..st.asset_folder.len() - 1];
            st.asset_folder = f.rfind('/').map_or_else(String::new, |cut| f[..=cut].to_owned());
        }

        let mut into = None;
        for f in &st.asset_folders {
            if gui::selectable(&format!("[{f}]"), false, 0) {
                into = Some(format!("{f}/"));
            }
        }

        let mut open = None;
        for f in &st.asset_files {
            let full = format!("{}{f}", st.asset_folder);
            if gui::selectable(f, false, gui::SELECTABLE_ALLOW_DOUBLE_CLICK) && gui::is_mouse_double_clicked(0) {
                open = Some(full.clone());
            }

            if gui::is_item_hovered() {
                gui::set_tooltip(&format!("{full}\n(double-click to open)"));
            }
        }

        if let Some(sub) = into {
            st.asset_folder += &sub;
        }

        if let Some(full) = open {
            match e.open_path(&full, true, true) {
                Ok(msg) => e.status(&msg, false),
                Err(msg) => e.status(&msg, true),
            }
        }
    }

    gui::end_child();
    gui::end();
    e.panel_data = st;
}

// ---- log

pub fn log(e: &mut Editor) {
    if !gui::begin("Log", Some(&mut e.panels.log), 0) {
        gui::end();
        return;
    }

    let st = &mut e.panel_data;
    gui::checkbox("info", &mut st.show_info);
    gui::same_line();
    gui::checkbox("warnings", &mut st.show_warn);
    gui::same_line();
    gui::checkbox("errors", &mut st.show_error);
    gui::same_line();
    if gui::small_button("Clear") {
        log::clear();
    }

    gui::separator();
    if gui::begin_child("##log", gui::v2(0.0, 0.0), 0, gui::WINDOW_HORIZONTAL_SCROLLBAR) {
        let at_bottom = gui::scroll_y() >= gui::scroll_max_y() - 4.0;
        crate::theme::push_mono();
        for entry in log::entries() {
            let shown = match entry.level {
                LogLevel::Info => st.show_info,
                LogLevel::Warning => st.show_warn,
                LogLevel::Error => st.show_error,
            };

            if !shown {
                continue;
            }

            let colour = match entry.level {
                LogLevel::Error => crate::theme::v4(crate::theme::error()),
                LogLevel::Warning => crate::theme::v4(crate::theme::warning()),
                LogLevel::Info => gui::style_color(gui::COL_TEXT),
            };

            gui::push_style_color(gui::COL_TEXT, colour);
            gui::text(&entry.text);
            gui::pop_style_color(1);
        }

        crate::theme::pop_font();

        let now = log::sequence();
        if now != st.log_seen && at_bottom {
            gui::set_scroll_here_y(1.0);
        }

        st.log_seen = now;
    }

    gui::end_child();
    gui::end();
}

// ---- history

pub fn history(e: &mut Editor) {
    if !gui::begin("History", Some(&mut e.panels.history), 0) {
        gui::end();
        return;
    }

    let Some(d) = e.docs.active_mut() else {
        gui::text_disabled("No document.");
        gui::end();
        return;
    };

    gui::text_disabled(&d.title());
    let h = d.history_mut();
    gui::begin_disabled(!h.can_undo());
    if gui::small_button("Undo") {
        h.undo();
    }

    gui::end_disabled();
    gui::same_line();
    gui::begin_disabled(!h.can_redo());
    if gui::small_button("Redo") {
        h.redo();
    }

    gui::end_disabled();
    gui::separator();
    if gui::selectable("(opened)", h.position() == 0, 0) {
        h.jump_to(0);
    }

    for i in 0..h.size() {
        let applied = i < h.position();
        if !applied {
            gui::push_style_color(gui::COL_TEXT, gui::style_color(gui::COL_TEXT_DISABLED));
        }

        if gui::selectable(&format!("{}##h{i}", h.name(i)), h.position() == i + 1, 0) {
            h.jump_to(i + 1);
        }

        if !applied {
            gui::pop_style_color(1);
        }
    }

    gui::end();
}

// ---- commands (help + console)

pub fn commands(e: &mut Editor) {
    if !gui::begin("Commands", Some(&mut e.panels.commands), 0) {
        gui::end();
        return;
    }

    gui::set_next_item_width(-1.0);
    let mut line = std::mem::take(&mut e.panel_data.command_line);
    if gui::input_text("##cmd", Some("command, e.g. select 1   (Enter runs it)"), &mut line, 256, gui::INPUT_ENTER_RETURNS_TRUE) {
        let r = CommandRegistry::run(e, &line);
        if !r.message.is_empty() || !r.ok {
            log::add(if r.ok { LogLevel::Info } else { LogLevel::Error }, format!("{line} -> {}", r.message));
        }

        line.clear();
        gui::set_keyboard_focus_here(-1);
    }

    // The line's first word filters the list as it is typed (`command_matches`); a click on a command puts it in the line.
    let word = line.split_whitespace().next().unwrap_or_default().to_owned();
    let shown = command_matches(&word, e.commands().entries().map(|c| (c.name.as_str(), c.help.as_str())));
    let total = e.commands().entries().count();
    if !word.is_empty() {
        gui::text_disabled(&format!("{} of {total} commands match '{word}'", shown.len()));
    }

    e.panel_data.command_line = line;
    gui::separator();
    let entries = e.commands().entries().map(|c| (c.name.clone(), c.usage.clone(), c.help.clone())).collect::<Vec<_>>();
    for &i in &shown {
        let (name, usage, help) = &entries[i];
        if gui::selectable(&format!("{usage}##cmd_{name}"), false, 0) {
            e.panel_data.command_line = format!("{name} ");
        }

        if gui::is_item_hovered() {
            gui::set_tooltip("Click: put it in the command line");
        }

        gui::same_line_at(300.0);
        gui::text_disabled(help);
    }

    gui::end();
}

/// The commands a typed word picks (indices into `commands`, given as name and help), best first: the name itself, then the names that
/// start with it, that have it, and last the commands whose help mentions it; case aside. No word: all of them, in their order.
fn command_matches<'a>(word: &str, commands: impl Iterator<Item = (&'a str, &'a str)>) -> Vec<usize> {
    let word = word.to_lowercase();
    let mut ranked = commands
        .enumerate()
        .filter_map(|(i, (name, help))| {
            let name = name.to_lowercase();
            let rank = if word.is_empty() || name == word {
                0
            } else if name.starts_with(&word) {
                1
            } else if name.contains(&word) {
                2
            } else if help.to_lowercase().contains(&word) {
                3
            } else {
                return None;
            };
            Some((rank, i))
        })
        .collect::<Vec<_>>();
    ranked.sort();
    ranked.into_iter().map(|(_, i)| i).collect()
}

#[cfg(test)]
mod command_filter_tests {
    use super::command_matches;

    #[test]
    fn the_first_word_picks_the_name_then_its_starts_then_the_rest_then_the_help() {
        let cmds = [("tile_select", "select elements"), ("select", "select by index"), ("selection_info", "log it"), ("quit", "close")];
        let pick = |w: &str| command_matches(w, cmds.iter().copied());
        assert_eq!(pick(""), vec![0, 1, 2, 3], "no word: all, in order");
        assert_eq!(pick("select"), vec![1, 2, 0], "the name, a name starting with it, one having it");
        assert_eq!(pick("SEL"), vec![1, 2, 0]);
        assert_eq!(pick("elements"), vec![0], "the help, last");
        assert!(pick("nothing").is_empty());
    }
}
