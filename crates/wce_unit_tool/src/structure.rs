//! The Structure panel: the definition's tree laid out as the Tile tool's Scene panel lays out a tile's: one row per node (an arrow, the
//! node's icon, its name, then its details dimmed), at the right edge a column for its problems and one for what the preview shows of it.
//! The tree is flattened to the rows that are open and drawn through a list clipper. Selection by click or the arrow keys, rename in place
//! (a slot: F2 or a double-click), a context menu per row, Delete, Ctrl+D, and drag and drop: a node among its siblings, a definition from
//! Unit files onto a slot (a reference to it). Open rows are remembered by their path.

use std::collections::{HashMap, HashSet};

use wce_editor::editor::Editor;
use wce_editor::gui::{self, DrawList, Vec2, v2};
use wce_editor::{icons, theme};
use wce_editor_core::document::Document as _;
use wce_formats::variant_mesh::{Alternative, NodePath, VariantMesh, VariantMeshFile};
use wce_render::unit_assembly::SlotPin;
use wce_vfs::normalize;

use super::{Action, UnitTool, document, file_name};
use crate::document::Structural;

const ICON_WIDTH: f32 = 22.0;

/// The icon columns at the right edge: problems, and what the preview shows.
const ICON_COLUMNS: f32 = 2.0;

/// The drag and drop payloads: a node of the tree, a definition of Unit files.
pub(super) const NODE_PAYLOAD: &str = "VMD_NODE";
pub(super) const FILE_PAYLOAD: &str = "VMD_FILE";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Root,
    Slot,
    Mesh,

    /// A mesh without a model: slots grouped.
    Group,
    Reference,
}

impl Kind {
    fn icon(self) -> &'static str {
        match self {
            Kind::Root => icons::USER,
            Kind::Slot => icons::LAYERS,
            Kind::Mesh => icons::BOX,
            Kind::Group => icons::BOXES,
            Kind::Reference => icons::LINK_2,
        }
    }

    fn what(self) -> &'static str {
        match self {
            Kind::Root => "The file's root mesh",
            Kind::Slot => "Slot: a place for one part, filled by one of its alternatives",
            Kind::Mesh => "Mesh: a model with its decals, colour mask and tints",
            Kind::Group => "Mesh without a model: a group of slots",
            Kind::Reference => "Reference: another definition file, used whole",
        }
    }
}

#[derive(Clone, Debug)]
struct Row {
    path: NodePath,
    kind: Kind,
    depth: usize,
    element: usize,
    kids: usize,
    label: String,
    detail: String,

    /// A reference's file.
    file: String,
}

/// The tree's view state.
#[derive(Default)]
pub(super) struct TreeState {
    closed: HashSet<NodePath>,
    filter: String,

    /// The slot being renamed: its path and the text.
    rename: Option<(NodePath, String)>,
    rename_focus: bool,
    rename_seen: bool,

    /// The selection the tree last followed, to scroll to one made elsewhere (the Problems panel).
    synced: Option<NodePath>,
    doc: u32,

    /// The tree drawn this frame.
    snapshot: Option<std::sync::Arc<VariantMeshFile>>,
}

fn strip_ci<'a>(s: &'a str, suffix: &str) -> &'a str {
    if s.len() >= suffix.len() && s[s.len() - suffix.len()..].eq_ignore_ascii_case(suffix) { &s[..s.len() - suffix.len()] } else { s }
}

fn mesh_row(mesh: &VariantMesh, path: &NodePath, depth: usize, root: bool) -> Row {
    let model = mesh.model.as_deref().map(str::trim).filter(|m| !m.is_empty());
    let kind = match (root, model) {
        (true, _) => Kind::Root,
        (false, Some(_)) => Kind::Mesh,
        (false, None) => Kind::Group,
    };
    let label = match (root, model) {
        (_, Some(m)) => strip_ci(file_name(m), ".rigid_model_v2").to_owned(),
        (true, None) => "root".to_owned(),
        (false, None) => "group".to_owned(),
    };
    let mut detail = vec![];
    if mesh.tints.len() == 1 {
        detail.push("1 tint".to_owned());
    } else if mesh.tints.len() > 1 {
        detail.push(format!("{} tints", mesh.tints.len()));
    }

    if mesh.decal_diffuse.as_deref().is_some_and(|d| !d.trim().is_empty()) {
        detail.push("decal".to_owned());
    }

    if !mesh.meta_data.is_empty() {
        detail.push(mesh.meta_data.join(", "));
    }

    Row {
        path: path.clone(),
        kind,
        depth,
        element: mesh.element,
        kids: mesh.slots.len(),
        label,
        detail: detail.join("  "),
        file: String::new(),
    }
}

/// The rows: every node, below the open ones; with a filter, the nodes that match and the ones above them, open or not.
fn rows(tree: &VariantMeshFile, closed: &HashSet<NodePath>, words: &[String]) -> Vec<Row> {
    fn matches(row: &Row, words: &[String]) -> bool {
        wce_editor::pickers::matches(&format!("{} {} {}", row.label, row.detail, row.file), words)
    }

    fn mesh(m: &VariantMesh, path: &mut NodePath, depth: usize, closed: &HashSet<NodePath>, words: &[String], out: &mut Vec<Row>) -> bool {
        let row = mesh_row(m, path, depth, path.is_empty());
        let at = out.len();
        let mut any = matches(&row, words);
        out.push(row);
        let open = !words.is_empty() || !closed.contains(path);
        let before = out.len();
        for (i, s) in m.slots.iter().enumerate() {
            path.push(i);
            any |= slot(s, path, depth + 1, closed, words, out);
            path.pop();
        }

        if !open {
            out.truncate(before);
        }

        if !any && !words.is_empty() {
            out.truncate(at);
        }

        any
    }

    fn slot(
        s: &wce_formats::variant_mesh::Slot,
        path: &mut NodePath,
        depth: usize,
        closed: &HashSet<NodePath>,
        words: &[String],
        out: &mut Vec<Row>,
    ) -> bool {
        let mut detail = vec![];
        if let Some(a) = s.attach_point.as_deref().filter(|a| !a.trim().is_empty()) {
            detail.push(format!("@{a}"));
        }

        if let Some(p) = s.probability {
            detail.push(format!("{:.0}%", p * 100.0));
        }

        let row = Row {
            path: path.clone(),
            kind: Kind::Slot,
            depth,
            element: s.element,
            kids: s.alternatives.len(),
            label: if s.name.is_empty() { "(no name)".to_owned() } else { s.name.clone() },
            detail: detail.join("  "),
            file: String::new(),
        };
        let at = out.len();
        let mut any = matches(&row, words);
        out.push(row);
        let open = !words.is_empty() || !closed.contains(path);
        let before = out.len();
        for (j, a) in s.alternatives.iter().enumerate() {
            path.push(j);
            any |= match a {
                Alternative::Mesh(m) => mesh(m, path, depth + 1, closed, words, out),
                Alternative::Reference { element, definition } => {
                    let row = Row {
                        path: path.clone(),
                        kind: Kind::Reference,
                        depth: depth + 1,
                        element: *element,
                        kids: 0,
                        label: strip_ci(file_name(definition), ".variantmeshdefinition").to_owned(),
                        detail: String::new(),
                        file: definition.clone(),
                    };
                    let hit = matches(&row, words);
                    if words.is_empty() || hit {
                        out.push(row);
                    }

                    hit
                }
            };
            path.pop();
        }

        if !open {
            out.truncate(before);
        }

        if !any && !words.is_empty() {
            out.truncate(at);
        }

        any
    }

    let mut out = vec![];
    mesh(&tree.root, &mut vec![], 0, closed, words, &mut out);
    out
}

/// An icon (a glyph of the UI font) centred on `c`.
fn draw_icon(dl: DrawList, c: Vec2, icon: &str, col: u32) {
    let w = gui::calc_text_width(icon);
    dl.text(v2(c.x - w * 0.5, c.y - gui::text_line_height() * 0.5), col, icon);
}

fn faded(c: [f32; 4], t: f32) -> u32 {
    theme::u32(theme::mix(c, theme::palette_background(), t))
}

/// A button of the toolbar: an icon and a word, or an icon alone (with its tooltip).
fn tool_button(first: &mut bool, label: &str, enabled: bool, tip: &str) -> bool {
    if !*first {
        gui::same_line_or_wrap(gui::button_width(label));
    }

    *first = false;
    gui::begin_disabled(!enabled);
    let clicked = gui::button(label);
    gui::end_disabled();
    if gui::is_item_hovered_allow_disabled() {
        gui::set_tooltip(tip);
    }

    clicked
}

impl UnitTool {
    pub(super) fn draw_structure(&mut self, e: &mut Editor) -> Vec<Action> {
        let mut actions = vec![];
        gui::set_next_window_size(v2(380.0, 600.0), gui::COND_FIRST_USE_EVER);
        if !gui::begin("Structure", None, 0) {
            gui::end();
            return actions;
        }

        let Some(d) = document(e) else {
            gui::text_disabled("No definition open.");
            gui::text_wrapped("Open one from Unit files, from Units (a unit's own) or with File > Open.");
            gui::end();
            return actions;
        };

        let tree = d.tree();
        let (title, source, repaired, modified, id) = (d.title(), d.source().label.clone(), d.repaired(), d.modified(), d.id());
        if self.tree.doc != id {
            self.tree = TreeState { doc: id, ..TreeState::default() };
        }

        self.tree.snapshot = Some(tree.clone());

        // ---- the file
        gui::align_text_to_frame_padding();
        gui::text_colored(theme::palette().highlight, icons::FILE);
        gui::same_line();
        gui::text(&title);
        if modified {
            gui::same_line();
            gui::text_colored(theme::modified(), "*");
            if gui::is_item_hovered() {
                gui::set_tooltip("Unsaved changes (File > Save writes them into the project).");
            }
        }

        gui::same_line();
        gui::text_disabled(&source);
        if repaired {
            gui::text_colored(
                theme::warning(),
                &format!("{} Not valid XML: shown as the game reads it; Save writes that.", icons::TRIANGLE_ALERT),
            );
        }

        // ---- the toolbar
        let sel = self.selected.clone().filter(|p| tree.node(p).is_some());
        let kind = sel.as_ref().map(|p| kind_of(&tree, p));
        let elem = sel.as_ref().and_then(|p| tree.node(p)).map(|n| n.element());
        let on_mesh = matches!(kind, None | Some(Kind::Root | Kind::Mesh | Kind::Group));
        let on_slot = kind == Some(Kind::Slot);
        let not_root = sel.as_ref().is_some_and(|p| !p.is_empty());
        let target = elem.unwrap_or(tree.root.element);
        let mut first = true;
        if tool_button(
            &mut first,
            &format!("{} Slot", icons::PLUS),
            on_mesh,
            "A slot in the selected mesh (or the root): a place for one part.",
        ) {
            actions.push(Action::Edit(Structural::AddSlot("new_slot".to_owned()), target));
            self.tree.rename_focus = true;
        }

        if tool_button(&mut first, &format!("{} Mesh", icons::PLUS), on_slot, "A model as an alternative of the selected slot.")
            && let Some(t) = elem
        {
            actions.push(Action::Edit(Structural::AddMesh(String::new()), t));
        }

        if tool_button(
            &mut first,
            &format!("{} Reference", icons::PLUS),
            on_slot,
            "Another definition as an alternative of the selected slot (or drag one from Unit files onto the slot).",
        ) && let Some(t) = elem
        {
            actions.push(Action::Edit(Structural::AddReference(String::new()), t));
        }

        if tool_button(&mut first, icons::COPY, not_root, "Duplicate (Ctrl+D): a copy right after it.")
            && let Some(t) = elem
        {
            actions.push(Action::Edit(Structural::Duplicate, t));
        }

        if tool_button(&mut first, icons::ARROW_BIG_UP, not_root, "Move up, before the node above it (or drag it).")
            && let Some(t) = elem
        {
            actions.push(Action::Edit(Structural::MoveUp, t));
        }

        if tool_button(&mut first, icons::ARROW_BIG_DOWN, not_root, "Move down, after the node below it (or drag it).")
            && let Some(t) = elem
        {
            actions.push(Action::Edit(Structural::MoveDown, t));
        }

        if tool_button(&mut first, icons::TRASH_2, not_root, "Delete (Delete): the node and everything in it.")
            && let Some(t) = elem
        {
            actions.push(Action::Edit(Structural::Remove, t));
        }

        // ---- the filter
        gui::set_next_item_width(-1.0);
        gui::input_text(
            "##structure_filter",
            Some(&format!("{} Filter: slots, models, files", icons::SEARCH)),
            &mut self.tree.filter,
            128,
            0,
        );
        let words: Vec<String> = self.tree.filter.split_whitespace().map(str::to_lowercase).collect();

        // A selection made elsewhere: open the rows above it, scroll to it.
        let mut scroll_to = None;
        if self.selected != self.tree.synced {
            self.tree.synced.clone_from(&self.selected);
            if let Some(p) = &self.selected {
                for k in 0..p.len() {
                    self.tree.closed.remove(&p[..k]);
                }

                scroll_to = Some(p.clone());
            }
        }

        let rows = rows(&tree, &self.tree.closed, &words);

        // The problems by node, and the nodes above them.
        let mut problems: HashMap<NodePath, Vec<String>> = HashMap::new();
        let mut below: HashSet<NodePath> = HashSet::new();
        for p in &self.problems {
            problems.entry(p.path.clone()).or_default().push(p.text.clone());
            for k in 0..p.path.len() {
                below.insert(p.path[..k].to_vec());
            }
        }

        // ---- the tree
        let footer = gui::frame_height_with_spacing();
        if gui::begin_child("##structure_tree", v2(0.0, -footer), gui::CHILD_BORDERS, 0) {
            self.draw_rows(&rows, &problems, &below, scroll_to.as_ref(), &mut actions);
            self.keys(&rows, &tree, &mut actions);
        }

        gui::end_child();

        // ---- the counts
        let count = |k: &[Kind]| rows.iter().filter(|r| k.contains(&r.kind)).count();
        let total = format!(
            "{} slots, {} meshes, {} references{}",
            count(&[Kind::Slot]),
            count(&[Kind::Mesh, Kind::Group]),
            count(&[Kind::Reference]),
            if words.is_empty() { "" } else { " found" }
        );
        gui::text_disabled(&total);
        if !self.problems.is_empty() {
            gui::same_line();
            let n = self.problems.len();
            gui::text_colored(theme::warning(), &format!("{} {n} problem{}", icons::TRIANGLE_ALERT, if n == 1 { "" } else { "s" }));
        }

        gui::end();
        actions
    }

    fn draw_rows(
        &mut self,
        rows: &[Row],
        problems: &HashMap<NodePath, Vec<String>>,
        below: &HashSet<NodePath>,
        scroll_to: Option<&NodePath>,
        actions: &mut Vec<Action>,
    ) {
        let row_h = gui::text_line_height_with_spacing() + 2.0;
        let dl = gui::window_draw_list();
        if let Some(target) = scroll_to
            && let Some(i) = rows.iter().position(|r| r.path == *target)
        {
            let (y, h) = (i as f32 * row_h, gui::window_height());
            if y < gui::scroll_y() || y + row_h > gui::scroll_y() + h {
                gui::set_scroll_y((y - (h - row_h) * 0.5).max(0.0));
            }
        }

        let dragging_file = self.dragging_file.clone();
        gui::clipped_rows(rows.len(), row_h, |i| {
            let row = &rows[i];
            gui::push_id_int(i as i32);
            let p0 = gui::cursor_screen_pos();
            let avail = gui::content_region_avail().x;
            let text_w = avail - ICON_COLUMNS * ICON_WIDTH;
            let is_sel = self.selected.as_ref() == Some(&row.path);

            // The row: selection, double-click, context menu, drag source and drop target. Its highlight made as tall as the row.
            let spacing = gui::item_spacing().y;
            gui::set_cursor_screen_pos(v2(p0.x, p0.y + spacing * 0.5));
            gui::set_next_item_allow_overlap();
            let clicked =
                gui::selectable_sized("##row", is_sel, gui::SELECTABLE_ALLOW_DOUBLE_CLICK, v2(text_w, (row_h - spacing).max(1.0)));
            let hovered = gui::is_item_hovered();
            let (rmin, rmax) = (p0, v2(p0.x + text_w, p0.y + row_h));
            let x0 = p0.x + 4.0 + row.depth as f32 * 14.0;
            let cy = p0.y + row_h * 0.5;
            let on_arrow = {
                let m = gui::mouse_pos();
                row.kids != 0 && m.x >= x0 - 2.0 && m.x < x0 + 12.0
            };

            if clicked && !on_arrow {
                actions.push(Action::Select(Some(row.path.clone())));
                if gui::is_mouse_double_clicked(0) {
                    match row.kind {
                        Kind::Slot => {
                            self.tree.rename = Some((row.path.clone(), row.label.clone()));
                            self.tree.rename_focus = true;
                        }

                        Kind::Reference => actions.push(Action::Open(normalize(&row.file))),
                        _ => {}
                    }
                }
            }

            if hovered && gui::is_mouse_clicked(0) && on_arrow {
                if self.tree.closed.contains(&row.path) {
                    self.tree.closed.remove(&row.path);
                } else {
                    self.tree.closed.insert(row.path.clone());
                }
            }

            if hovered && row.kind != Kind::Root {
                gui::set_tooltip(&format!(
                    "{}{}",
                    row.kind.what(),
                    if row.file.is_empty() { String::new() } else { format!("\n{}", row.file) }
                ));
            }

            if gui::is_item_clicked_with(1) && !is_sel {
                actions.push(Action::Select(Some(row.path.clone())));
            }

            // Drag a node among its siblings.
            if row.kind != Kind::Root && gui::begin_drag_drop_source() {
                gui::set_drag_drop_payload(NODE_PAYLOAD, &row.path.iter().map(|&k| k as i32).collect::<Vec<_>>());
                gui::text(&format!("{} {}", row.kind.icon(), row.label));
                gui::end_drag_drop_source();
            }

            if gui::begin_drag_drop_target() {
                let c = theme::u32(theme::palette().highlight);
                if let Some((data, delivered)) = gui::accept_drag_drop_payload(NODE_PAYLOAD) {
                    let from: NodePath = data.iter().filter_map(|&k| usize::try_from(k).ok()).collect();
                    let siblings =
                        from.len() == row.path.len() && from[..from.len() - 1] == row.path[..row.path.len() - 1] && from != row.path;
                    if siblings {
                        let after = gui::mouse_pos().y > rmin.y + row_h * 0.5;
                        let y = if after { rmax.y } else { rmin.y };
                        dl.line(v2(rmin.x, y), v2(rmax.x, y), c, 2.0);
                        if delivered {
                            // Its place among the siblings once it is out of its own: before or after the row dropped on.
                            let (a, b) = (from[from.len() - 1], row.path[row.path.len() - 1]);
                            let to = match (a < b, after) {
                                (true, true) => b,
                                (true, false) => b - 1,
                                (false, true) => b + 1,
                                (false, false) => b,
                            };
                            actions.push(Action::Reorder(from, to));
                        }
                    }
                }

                if row.kind == Kind::Slot
                    && let Some((_, delivered)) = gui::accept_drag_drop_payload(FILE_PAYLOAD)
                {
                    dl.rect(rmin, rmax, c, 2.0, 1.5);
                    if delivered && let Some(file) = &dragging_file {
                        actions.push(Action::Edit(Structural::AddReference(file.clone()), row.element));
                    }
                }

                gui::end_drag_drop_target();
            }

            // Context menu.
            if gui::begin_popup_context_item("##rowmenu") {
                self.row_menu(row, actions);
                gui::end_popup();
            }

            // The arrow.
            if row.kids != 0 {
                let c = v2(x0 + 5.0, cy);
                let ac = theme::u32(theme::mix(theme::text(), theme::dim(), 0.4));
                if words_open(&self.tree, &row.path) {
                    dl.triangle_filled(v2(c.x - 4.0, c.y - 2.5), v2(c.x + 4.0, c.y - 2.5), v2(c.x, c.y + 3.5), ac);
                } else {
                    dl.triangle_filled(v2(c.x - 2.5, c.y - 4.0), v2(c.x - 2.5, c.y + 4.0), v2(c.x + 3.5, c.y), ac);
                }
            }

            // The icon, the name and the details.
            let own_problems = problems.get(&row.path);
            let text_col = if own_problems.is_some() { theme::u32(theme::warning()) } else { theme::u32(theme::text()) };
            let icon_col = match row.kind {
                Kind::Root => theme::u32(theme::palette().highlight),
                Kind::Slot => faded(theme::text(), 0.25),
                Kind::Reference => theme::u32(theme::mix(theme::palette().highlight, theme::text(), 0.4)),
                _ => text_col,
            };
            let ix = x0 + 16.0;
            draw_icon(dl, v2(ix + 7.0, cy), row.kind.icon(), icon_col);
            let tx = ix + 20.0;
            let renaming = self.tree.rename.as_ref().is_some_and(|(p, _)| *p == row.path);
            if renaming {
                gui::set_cursor_screen_pos(v2(tx, p0.y));
                gui::set_next_item_width((text_w - (tx - p0.x) - 4.0).max(60.0));
                if self.tree.rename_focus {
                    gui::set_keyboard_focus_here(0);
                    self.tree.rename_focus = false;
                    self.tree.rename_seen = false;
                }

                let mut text = self.tree.rename.as_ref().map(|(_, t)| t.clone()).unwrap_or_default();
                let enter = gui::input_text("##rename", None, &mut text, 128, gui::INPUT_ENTER_RETURNS_TRUE | gui::INPUT_AUTO_SELECT_ALL);
                if gui::is_item_active() {
                    self.tree.rename_seen = true;
                }

                if enter {
                    actions.push(Action::Rename(row.element, text));
                    self.tree.rename = None;
                } else if gui::is_key_pressed(gui::KEY_ESCAPE, true) || (self.tree.rename_seen && !gui::is_item_active()) {
                    self.tree.rename = None;
                } else if let Some((_, t)) = &mut self.tree.rename {
                    *t = text;
                }
            } else {
                dl.push_clip_rect(v2(tx, p0.y), v2(p0.x + text_w - 2.0, p0.y + row_h));
                let ty = p0.y + (row_h - gui::text_line_height()) * 0.5;
                dl.text(v2(tx, ty), text_col, &row.label);
                let mut w = gui::calc_text_size(&row.label).x;
                if row.kind == Kind::Slot && row.kids > 1 {
                    let n = format!("  {}", row.kids);
                    dl.text(v2(tx + w, ty), faded(theme::dim(), 0.2), &n);
                    w += gui::calc_text_size(&n).x;
                }

                if !row.detail.is_empty() {
                    dl.text(v2(tx + w + 10.0, ty), theme::u32(theme::dim()), &row.detail);
                }

                dl.pop_clip_rect();
            }

            // The problems' column.
            let px = p0.x + avail - 2.0 * ICON_WIDTH;
            gui::set_cursor_screen_pos(v2(px, p0.y));
            gui::invisible_button("##problems", v2(ICON_WIDTH, row_h), gui::BUTTON_LEFT);
            match own_problems {
                Some(list) => {
                    draw_icon(dl, v2(px + ICON_WIDTH * 0.5, cy), icons::TRIANGLE_ALERT, theme::u32(theme::warning()));
                    if gui::is_item_hovered() {
                        gui::set_tooltip(&list.join("\n"));
                    }
                }

                None if below.contains(&row.path) && !words_open(&self.tree, &row.path) => {
                    draw_icon(dl, v2(px + ICON_WIDTH * 0.5, cy), icons::TRIANGLE_ALERT, faded(theme::warning(), 0.5));
                    if gui::is_item_hovered() {
                        gui::set_tooltip("Problems below this node.");
                    }
                }

                None => {}
            }

            // What the preview shows: a slot's eye (the seed's, or nothing), an alternative's target (shown whatever the seed says).
            let sx = p0.x + avail - ICON_WIDTH;
            gui::set_cursor_screen_pos(v2(sx, p0.y));
            let pressed = gui::invisible_button("##shown", v2(ICON_WIDTH, row_h), gui::BUTTON_LEFT);
            let c = v2(sx + ICON_WIDTH * 0.5, cy);
            let lit = theme::u32(theme::palette().highlight);
            let quiet = faded(theme::dim(), 0.45);
            match row.kind {
                Kind::Slot => {
                    let pin = self.pins.slots.get(&row.path).copied();
                    let (icon, col, tip) = match pin {
                        None => (icons::EYE, faded(theme::text(), 0.35), "The seed picks what it shows: click to show nothing"),
                        Some(SlotPin::Off) => (icons::EYE_OFF, lit, "Shows nothing: click to let the seed pick"),
                        Some(SlotPin::Alternative(_)) => {
                            (icons::LOCATE_FIXED, lit, "Shows a chosen alternative: click to let the seed pick")
                        }
                    };
                    draw_icon(dl, c, icon, col);
                    if gui::is_item_hovered() {
                        gui::set_tooltip(tip);
                    }

                    if pressed {
                        actions.push(Action::Pin(row.path.clone(), if pin.is_none() { Some(SlotPin::Off) } else { None }));
                    }
                }

                Kind::Mesh | Kind::Group | Kind::Reference => {
                    let (slot, k) = (row.path[..row.path.len() - 1].to_vec(), row.path[row.path.len() - 1]);
                    let on = self.pins.slots.get(&slot) == Some(&SlotPin::Alternative(k));
                    if on || gui::is_item_hovered() {
                        draw_icon(dl, c, icons::LOCATE_FIXED, if on { lit } else { quiet });
                    }

                    if gui::is_item_hovered() {
                        gui::set_tooltip(if on {
                            "The preview shows this one: click to let the seed pick"
                        } else {
                            "Click to show this one in the preview, whatever the seed picks"
                        });
                    }

                    if pressed {
                        actions.push(Action::Pin(slot, if on { None } else { Some(SlotPin::Alternative(k)) }));
                    }
                }

                Kind::Root => {}
            }

            gui::set_cursor_screen_pos(p0);
            gui::dummy(v2(avail, row_h - gui::item_spacing().y));
            gui::pop_id();
        });
    }

    fn row_menu(&mut self, row: &Row, actions: &mut Vec<Action>) {
        let el = row.element;
        match row.kind {
            Kind::Root | Kind::Mesh | Kind::Group => {
                if gui::menu_item(&format!("{} Add a slot", icons::PLUS), None, false, true) {
                    actions.push(Action::Edit(Structural::AddSlot("new_slot".to_owned()), el));
                    self.tree.rename_focus = true;
                }

                if row.kind != Kind::Group && gui::menu_item(&format!("{} Add a tint", icons::PALETTE), None, false, true) {
                    actions.push(Action::Edit(Structural::AddTint, el));
                }
            }

            Kind::Slot => {
                if gui::menu_item(&format!("{} Add a mesh", icons::PLUS), None, false, true) {
                    actions.push(Action::Edit(Structural::AddMesh(String::new()), el));
                }

                if gui::menu_item(&format!("{} Add a reference", icons::PLUS), None, false, true) {
                    actions.push(Action::Edit(Structural::AddReference(String::new()), el));
                }

                if gui::menu_item(&format!("{} Rename", icons::PENCIL), Some("F2"), false, true) {
                    self.tree.rename = Some((row.path.clone(), row.label.clone()));
                    self.tree.rename_focus = true;
                }

                gui::separator();
                let pin = self.pins.slots.get(&row.path).copied();
                if gui::menu_item("Shows what the seed picks", None, pin.is_none(), true) {
                    actions.push(Action::Pin(row.path.clone(), None));
                }

                if gui::menu_item("Shows nothing", None, pin == Some(SlotPin::Off), true) {
                    actions.push(Action::Pin(row.path.clone(), Some(SlotPin::Off)));
                }
            }

            Kind::Reference => {
                if gui::menu_item(&format!("{} Open it", icons::FOLDER_OPEN), None, false, true) {
                    actions.push(Action::Open(normalize(&row.file)));
                }
            }
        }

        if matches!(row.kind, Kind::Mesh | Kind::Group | Kind::Reference) {
            let (slot, k) = (row.path[..row.path.len() - 1].to_vec(), row.path[row.path.len() - 1]);
            let on = self.pins.slots.get(&slot) == Some(&SlotPin::Alternative(k));
            if gui::menu_item(&format!("{} Show this one", icons::LOCATE_FIXED), None, on, true) {
                actions.push(Action::Pin(slot, if on { None } else { Some(SlotPin::Alternative(k)) }));
            }
        }

        if row.kind != Kind::Root {
            gui::separator();
            if gui::menu_item(&format!("{} Duplicate", icons::COPY), Some("Ctrl+D"), false, true) {
                actions.push(Action::Edit(Structural::Duplicate, el));
            }

            if gui::menu_item(&format!("{} Move up", icons::ARROW_BIG_UP), None, false, true) {
                actions.push(Action::Edit(Structural::MoveUp, el));
            }

            if gui::menu_item(&format!("{} Move down", icons::ARROW_BIG_DOWN), None, false, true) {
                actions.push(Action::Edit(Structural::MoveDown, el));
            }

            if gui::menu_item(&format!("{} Delete", icons::TRASH_2), Some("Delete"), false, true) {
                actions.push(Action::Edit(Structural::Remove, el));
            }
        }

        if row.kids != 0 {
            gui::separator();
            if gui::menu_item("Expand all below", None, false, true) {
                self.tree.closed.retain(|p| !p.starts_with(&row.path));
            }

            if gui::menu_item("Collapse all below", None, false, true) {
                self.collapse_below(&row.path);
            }
        }
    }

    /// Every node below `path` closed (the node itself too).
    fn collapse_below(&mut self, path: &NodePath) {
        let Some(tree) = self.tree.snapshot.clone() else { return };
        let mut pending = vec![path.clone()];
        while let Some(p) = pending.pop() {
            let kids = match tree.node(&p) {
                Some(wce_formats::variant_mesh::Node::Mesh(m)) => m.slots.len(),
                Some(wce_formats::variant_mesh::Node::Slot(s)) => s.alternatives.len(),
                _ => 0,
            };
            if kids != 0 {
                self.tree.closed.insert(p.clone());
                for k in 0..kids {
                    let mut c = p.clone();
                    c.push(k);
                    pending.push(c);
                }
            }
        }
    }

    /// The keys while the panel has the focus: arrows move and open, F2 renames, Delete deletes, Ctrl+D duplicates.
    fn keys(&mut self, rows: &[Row], tree: &VariantMeshFile, actions: &mut Vec<Action>) {
        if !gui::is_window_focused_with_children() || gui::is_any_item_active() || self.tree.rename.is_some() {
            return;
        }

        let at = self.selected.as_ref().and_then(|s| rows.iter().position(|r| r.path == *s));
        let pressed = |k| gui::is_key_pressed(k, true);
        if pressed(gui::KEY_DOWN_ARROW) {
            let next = at.map_or(0, |i| (i + 1).min(rows.len().saturating_sub(1)));
            if let Some(r) = rows.get(next) {
                actions.push(Action::Select(Some(r.path.clone())));
            }
        }

        if pressed(gui::KEY_UP_ARROW)
            && let Some(r) = rows.get(at.map_or(0, |i| i.saturating_sub(1)))
        {
            actions.push(Action::Select(Some(r.path.clone())));
        }

        let Some(i) = at else { return };
        let row = &rows[i];
        if pressed(gui::KEY_RIGHT_ARROW) && row.kids != 0 {
            self.tree.closed.remove(&row.path);
        }

        if pressed(gui::KEY_LEFT_ARROW) {
            if row.kids != 0 && !self.tree.closed.contains(&row.path) {
                self.tree.closed.insert(row.path.clone());
            } else if !row.path.is_empty() {
                actions.push(Action::Select(Some(row.path[..row.path.len() - 1].to_vec())));
            }
        }

        if pressed(gui::KEY_F2) && row.kind == Kind::Slot {
            self.tree.rename = Some((row.path.clone(), row.label.clone()));
            self.tree.rename_focus = true;
        }

        if row.kind != Kind::Root && tree.node(&row.path).is_some() {
            if pressed(gui::KEY_DELETE) {
                actions.push(Action::Edit(Structural::Remove, row.element));
            }

            if gui::io().KeyCtrl && pressed(gui::KEY_D) {
                actions.push(Action::Edit(Structural::Duplicate, row.element));
            }
        }
    }

    /// A slot added from the toolbar or a menu: its name in edit mode once it is selected.
    pub(super) fn rename_new_slot(&mut self, tree: &VariantMeshFile) {
        if let Some(p) = &self.selected
            && p.len() % 2 == 1
            && let Some(wce_formats::variant_mesh::Node::Slot(s)) = tree.node(p)
        {
            self.tree.rename = Some((p.clone(), s.name.clone()));
            self.tree.rename_focus = true;
        }
    }
}

fn kind_of(tree: &VariantMeshFile, path: &NodePath) -> Kind {
    match tree.node(path) {
        Some(wce_formats::variant_mesh::Node::Slot(_)) => Kind::Slot,
        Some(wce_formats::variant_mesh::Node::Reference { .. }) => Kind::Reference,
        Some(wce_formats::variant_mesh::Node::Mesh(m)) => mesh_row(m, path, 0, path.is_empty()).kind,
        None => Kind::Root,
    }
}

fn words_open(state: &TreeState, path: &NodePath) -> bool {
    !state.filter.trim().is_empty() || !state.closed.contains(path)
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use wce_formats::variant_mesh::VariantMeshFile;

    use super::{Kind, rows};

    const FILE: &str = r#"<VARIANT_MESH>
        <SLOT name="body"><VARIANT_MESH model="a/Body.rigid_model_v2"><TINT red="1" green="1" blue="1" random_range="0"/></VARIANT_MESH></SLOT>
        <SLOT name="shield" attach_point="weapon_02" probability="0.5">
            <VARIANT_MESH_REFERENCE definition="x/Round.VariantMeshDefinition"/>
            <VARIANT_MESH><SLOT name="boss"><VARIANT_MESH model="boss.rigid_model_v2"/></SLOT></VARIANT_MESH>
        </SLOT>
    </VARIANT_MESH>"#;

    #[test]
    fn rows_follow_the_open_nodes_and_the_filter() {
        let f = VariantMeshFile::parse(FILE.as_bytes().to_vec()).expect("parses");
        let all = rows(&f, &HashSet::new(), &[]);
        let labels: Vec<(&str, Kind)> = all.iter().map(|r| (r.label.as_str(), r.kind)).collect();
        assert_eq!(
            labels,
            [
                ("root", Kind::Root),
                ("body", Kind::Slot),
                ("Body", Kind::Mesh),
                ("shield", Kind::Slot),
                ("Round", Kind::Reference),
                ("group", Kind::Group),
                ("boss", Kind::Slot),
                ("boss", Kind::Mesh)
            ]
        );
        assert_eq!((all[3].detail.as_str(), all[2].detail.as_str()), ("@weapon_02  50%", "1 tint"));

        let closed = HashSet::from([vec![1]]);
        assert_eq!(rows(&f, &closed, &[]).len(), 4, "a closed slot hides its alternatives and theirs");

        let found = rows(&f, &closed, &["round".to_owned()]);
        let paths: Vec<Vec<usize>> = found.iter().map(|r| r.path.clone()).collect();
        assert_eq!(paths, [vec![], vec![1], vec![1, 0]], "a match and the nodes above it, closed or not");
    }
}
