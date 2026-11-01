//! The shared panels (outliner, inspector, assets, log, history, commands) and the Dear ImGui implementation of the inspector framework.

use wce_editor_core::command_registry::CommandRegistry;
use wce_editor_core::document::OutlinerNode;
use wce_editor_core::inspector::{InspectTarget, Inspector};
use wce_editor_core::selection::Selection;
use wce_foundation::log::{self, LogLevel};
use wce_scene::render_scene::SourceRef;

use crate::editor::Editor;
use crate::gui;

// ---- ImGuiInspector

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
fn label_column() -> f32 {
    (gui::content_region_avail().x * 0.4).max(110.0)
}

impl ImGuiInspector {
    fn id(&self, field: &str) -> String {
        format!("##{}", self.path(field))
    }

    /// A field's mark note, when it is marked (or a field it is part of).
    fn mark(&self, field: &str) -> Option<&str> {
        self.marks.iter().find(|(f, _)| field == f || field.strip_prefix(f.as_str()).is_some_and(|r| r.starts_with('.'))).map(|(_, n)| n.as_str())
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
                if disabled || dim { gui::text_disabled(label) } else { gui::text(label) }
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
        clicked && enabled
    }

    fn section(&mut self, title: &str) {
        gui::separator_text(title);
    }

    fn text(&mut self, field: &str, label: &str, value: &str) {
        self.draw_label(field, label, true);
        gui::same_line_at(label_column());
        gui::text(value);
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
            asset_folder: String::new(), asset_filter: String::new(), asset_only_tool: true, assets_built_for: String::new(), asset_folders: vec![],
            asset_files: vec![], show_info: true, show_warn: true, show_error: true, log_seen: 0, command_line: String::new(),
        }
    }
}

// ---- outliner

/// What a click in the outline asks for.
enum OutlineClick {
    Set(SourceRef),
    Toggle(SourceRef),
}

/// Nodes below the second level start closed and only an open node draws its children; a long list of leaves is clipped to the rows on screen.
fn draw_node(sel: &Selection, n: &OutlinerNode, depth: i32, click: &mut Option<OutlineClick>) {
    gui::push_id_int(n.source.item as i32);
    let selected = n.selectable && sel.contains(n.source);
    if n.children.is_empty() {
        if gui::selectable(&n.label, selected, 0) && n.selectable {
            *click = Some(if gui::io().KeyCtrl { OutlineClick::Toggle(n.source) } else { OutlineClick::Set(n.source) });
        }

        gui::pop_id();
        return;
    }

    let mut flags = gui::TREE_OPEN_ON_ARROW | gui::TREE_SPAN_AVAIL_WIDTH;
    if depth < 2 {
        flags |= gui::TREE_DEFAULT_OPEN;
    }

    if selected {
        flags |= gui::TREE_SELECTED;
    }

    let open = gui::tree_node_ex(&n.label, flags);
    if gui::is_item_clicked() && !gui::is_item_toggled_open() && n.selectable {
        *click = Some(OutlineClick::Set(n.source));
    }

    if open {
        let leaves = n.children.len() > 40 && n.children.iter().all(|c| c.children.is_empty());
        if leaves {
            gui::clipped(n.children.len(), |i| draw_node(sel, &n.children[i], depth + 1, click));
        } else {
            for c in &n.children {
                draw_node(sel, c, depth + 1, click);
            }
        }

        gui::tree_pop();
    }

    gui::pop_id();
}

pub fn outliner(e: &mut Editor) {
    if !gui::begin("Outliner", Some(&mut e.panels.outliner), 0) {
        gui::end();
        return;
    }

    if e.docs.order().is_empty() {
        gui::text_disabled("No document open.");
    }

    let unsaved = e.docs.unsaved().len();
    if e.docs.order().len() > 1 || unsaved != 0 {
        gui::begin_disabled(unsaved == 0);
        let label = if unsaved != 0 { format!("Save all ({unsaved})") } else { "Save all".to_owned() };
        if gui::small_button(&label) {
            let _ = CommandRegistry::run(e, "save_all");
        }

        gui::end_disabled();
        gui::same_line();
        gui::text_disabled(&format!("{} documents", e.docs.order().len()));
    }

    // The documents of every tool, grouped by document: eye = layer visibility, * = unsaved changes, the tag names the tool, the colour the
    // active one.
    let titles = e.tool_titles();
    for id in e.docs.order().to_vec() {
        let Some(d) = e.docs.find(id) else { continue };
        let (doc_title, modified, can_save) = (d.title(), d.modified(), d.can_save());
        gui::push_id_int(id as i32);
        let mut visible = e.docs.visible(&e.scene, id);
        if gui::checkbox("##vis", &mut visible) {
            e.docs.set_visible(&mut e.scene, id, visible);
        }

        if gui::is_item_hovered() {
            gui::set_tooltip("Show or hide this document in the viewport");
        }

        gui::same_line();
        let tool = e.docs.tool_of(id);
        let tag = titles.iter().find(|t| t.0 == tool).map(|t| format!("[{}] ", t.1)).unwrap_or_default();
        let title = format!("{tag}{doc_title}{}{}", if modified { " *" } else { "" }, if Some(id) == e.weather_preview_document() { "  (weather preview)" } else { "" });
        let active = id == e.docs.active_id();
        if active {
            gui::push_style_color(gui::COL_TEXT, gui::style_color(gui::COL_CHECK_MARK));
        } else if modified {
            gui::push_style_color(gui::COL_TEXT, crate::theme::v4(crate::theme::modified()));
        }

        let open = gui::collapsing_header(&format!("{title}###doc"), gui::TREE_DEFAULT_OPEN);
        if active || modified {
            gui::pop_style_color(1);
        }

        if gui::is_item_clicked() {
            e.docs.set_active(id);
        }

        // Edit it in its own tool.
        if gui::is_item_hovered() && gui::is_mouse_double_clicked(0) {
            e.set_tool(&tool);
        }

        let mut close = false;
        if gui::begin_popup_context_item("docctx") {
            if gui::menu_item("Save", None, false, can_save && modified) {
                match e.docs.save_one(id) {
                    Ok(msg) => e.status(&msg, false),
                    Err(msg) => e.status(&msg, true),
                }
            }

            if gui::menu_item("Close", None, false, true) {
                close = true;
            }

            gui::separator();
            if gui::menu_item("Edit in its tool", None, false, !tool.is_empty()) {
                e.set_tool(&tool);
            }

            if e.docs.order().len() > 1 && gui::menu_item("Show only this document", None, false, true) {
                for other in e.docs.order().to_vec() {
                    e.docs.set_visible(&mut e.scene, other, other == id);
                }
            }

            if e.docs.order().len() > 1 && gui::menu_item("Show all documents", None, false, true) {
                for other in e.docs.order().to_vec() {
                    e.docs.set_visible(&mut e.scene, other, true);
                }
            }

            gui::end_popup();
        }

        if open {
            let mut click = None;
            if let Some(d) = e.docs.find_mut(id) {
                draw_node(&e.selection, d.outline_cached(), 0, &mut click);
            }

            match click {
                Some(OutlineClick::Set(r)) => {
                    e.selection.set(r);
                    e.docs.set_active(r.doc);
                }

                Some(OutlineClick::Toggle(r)) => {
                    e.selection.toggle(r);
                    e.docs.set_active(r.doc);
                }

                None => {}
            }
        }

        gui::pop_id();
        if close {
            e.ask_close(id);
        }
    }

    gui::end();
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
    let key = format!("{}|{}|{}{}|{file_count}", st.asset_folder, st.asset_filter, e.active_tool_id(), if st.asset_only_tool { "1" } else { "0" });
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

    e.panel_data.command_line = line;
    gui::separator();
    for entry in e.commands().entries() {
        gui::text(&entry.usage);
        gui::same_line_at(300.0);
        gui::text_disabled(&entry.help);
    }

    gui::end();
}
