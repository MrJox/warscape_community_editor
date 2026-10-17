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
}

/// The value column starts after the longest label that fits: 40% of the panel, at least 110 px.
fn label_column() -> f32 {
    (gui::content_region_avail().x * 0.4).max(110.0)
}

impl ImGuiInspector {
    fn id(&self, field: &str) -> String {
        format!("##{}", self.path(field))
    }

    fn label(&self, label: &str, full_width: bool) {
        gui::text(label);
        gui::same_line_at(label_column());
        if full_width {
            gui::set_next_item_width(-1.0);
        }
    }

    fn finish_widget(&mut self) {
        self.ended = gui::is_item_deactivated_after_edit();
    }
}

impl Inspector for ImGuiInspector {
    fn target(&self) -> InspectTarget {
        self.target
    }

    fn set_target(&mut self, target: InspectTarget) {
        self.target = target;
    }

    fn section(&mut self, title: &str) {
        gui::separator_text(title);
    }

    fn text(&mut self, _field: &str, label: &str, value: &str) {
        gui::text_disabled(label);
        gui::same_line_at(label_column());
        gui::text(value);
    }

    fn edit_float(&mut self, field: &str, label: &str, v: &mut f32, speed: f32, lo: f32, hi: f32) -> bool {
        self.label(label, true);
        let changed = gui::drag_float(&self.id(field), v, speed, lo, hi, "%.3f");
        self.finish_widget();
        changed
    }

    fn edit_float3(&mut self, field: &str, label: &str, v: &mut [f32; 3], speed: f32) -> bool {
        self.label(label, true);
        let changed = gui::drag_float3(&self.id(field), v, speed, "%.3f");
        self.finish_widget();
        changed
    }

    fn edit_float2(&mut self, field: &str, label: &str, v: &mut [f32; 2], speed: f32) -> bool {
        self.label(label, true);
        let changed = gui::drag_float2(&self.id(field), v, speed, "%.6g");
        self.finish_widget();
        changed
    }

    fn edit_int(&mut self, field: &str, label: &str, v: &mut i64, speed: f32) -> bool {
        self.label(label, true);
        let changed = gui::drag_i64(&self.id(field), v, speed);
        self.finish_widget();
        changed
    }

    fn edit_color3(&mut self, field: &str, label: &str, v: &mut [f32; 3]) -> bool {
        self.label(label, true);
        let changed = gui::color_edit3(&self.id(field), v, gui::COLOR_FLOAT | gui::COLOR_HDR);
        self.finish_widget();
        changed
    }

    fn edit_color4(&mut self, field: &str, label: &str, v: &mut [f32; 4]) -> bool {
        self.label(label, true);
        let changed = gui::color_edit4(&self.id(field), v, gui::COLOR_FLOAT | gui::COLOR_HDR | gui::COLOR_ALPHA_BAR);
        self.finish_widget();
        changed
    }

    fn edit_bool(&mut self, field: &str, label: &str, v: &mut bool) -> bool {
        self.label(label, false);
        let changed = gui::checkbox(&self.id(field), v);
        self.ended = changed;
        changed
    }

    fn edit_string(&mut self, field: &str, label: &str, v: &mut String) -> bool {
        self.label(label, true);
        let changed = gui::input_text(&self.id(field), None, v, 512, 0);
        self.finish_widget();
        changed
    }

    fn edit_choice(&mut self, field: &str, label: &str, index: &mut i32, choices: &[String]) -> bool {
        self.label(label, true);
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
        changed
    }

    fn gesture_ended(&mut self) -> bool {
        self.ended
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
        let (doc_title, modified, can_save, lights) = (d.title(), d.modified(), d.can_save(), d.environment().is_some());
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
        let title = format!("{tag}{doc_title}{}{}", if modified { " *" } else { "" }, if id == e.weather_preview() { "  (weather preview)" } else { "" });
        let active = id == e.docs.active_id();
        if active {
            gui::push_style_color(gui::COL_TEXT, gui::style_color(gui::COL_CHECK_MARK));
        } else if modified {
            gui::push_style_color(gui::COL_TEXT, gui::v4(1.0, 0.75, 0.3, 1.0));
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

            if lights {
                let previewing = id == e.weather_preview();
                if gui::menu_item(if previewing { "Stop previewing this weather" } else { "Preview this weather on the scene" }, None, false, true) {
                    let msg = e.set_weather_preview(if previewing { 0 } else { id }).unwrap_or_else(|m| m);
                    e.status(&msg, false);
                }
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

    if e.selection.is_empty() {
        gui::text_disabled("Nothing selected.");
    } else {
        let r = e.selection.items()[0];
        let count = e.selection.items().len();
        match e.docs.find_mut(r.doc) {
            None => gui::text_disabled("The document is closed."),
            Some(d) => {
                if count > 1 {
                    gui::text_disabled(&format!("{count} items selected, showing the first"));
                }

                let what = d.describe(r);
                gui::text(if what.is_empty() { "Item" } else { &what });
                gui::separator();
                let mut ins = ImGuiInspector::default();
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
                LogLevel::Error => gui::v4(1.0, 0.45, 0.4, 1.0),
                LogLevel::Warning => gui::v4(1.0, 0.8, 0.4, 1.0),
                LogLevel::Info => gui::style_color(gui::COL_TEXT),
            };

            gui::push_style_color(gui::COL_TEXT, colour);
            gui::text(&entry.text);
            gui::pop_style_color(1);
        }

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
