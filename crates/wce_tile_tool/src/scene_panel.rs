//! The Scene panel of the Tile tool: the layer tree laid out like TEd's Scene tab (a name, then at the right edge the type's icon, where it is
//! exported, an eye and a cursor arrow, greyed when off), type toggles at the top (what the list and the viewport show), with selection, rename (F2 / double-click), delete, new layer, group, duplicate, export type, and drag and drop to
//! reorder or move into a layer. The eye is OIF_VISIBLE and the cursor arrow OIF_ACTIVE (selectable): `TileDocument::set_flags`. The tree is
//! flattened to the rows that are open and drawn through a list clipper, so a layer with 30,000 instances costs what its visible rows cost.
//! Open layers are remembered by their name path, which survives the index shifts of structural edits (a node id made of the element index
//! would not).

use std::collections::{HashMap, HashSet};

use wce_editor::editor::Editor;
use wce_editor::gui::{self, DrawList, Vec2, v2};
use wce_editor::theme;
use wce_tile::ted_scene::TedScene;
use wce_editor_core::document::Document;
use wce_tile_editor::tile_document::TileDocument;

use crate::entity_types::{EXPORT_TYPES, effective_export, export_icon, export_label, own_export, type_icon};
use crate::tool::{TileTool, TreeRow, document_id, selected};

const ICON_WIDTH: f32 = 22.0;

/// The icon columns at the right edge: the type, the export, the eye and the cursor arrow.
const ICON_COLUMNS: f32 = 4.0;

/// Elements below a deleted layer: ask first.
const CONFIRM_DELETE_ABOVE: usize = 200;

fn parent_layer(sc: &TedScene, e: usize) -> Option<usize> {
    let mut p = sc.doc().elements()[e].parent;
    while let Some(q) = p {
        if sc.is_layer(q) {
            return Some(q);
        }

        p = sc.doc().elements()[q].parent;
    }

    None
}

fn icon_colour(on: bool, inherited_off: bool) -> u32 {
    if !on {
        theme::u32(theme::mix(theme::dim(), theme::palette_background(), 0.35))
    } else if inherited_off {
        theme::u32(theme::dim())
    } else {
        theme::u32(theme::text())
    }
}

fn draw_eye(dl: DrawList, c: Vec2, on: bool, inherited_off: bool) {
    let col = icon_colour(on, inherited_off);
    dl.ellipse(c, v2(6.5, 3.6), col, 20, 1.3);
    dl.circle_filled(c, 2.2, col);
    if !on {
        dl.line(v2(c.x - 6.0, c.y + 4.0), v2(c.x + 6.0, c.y - 4.0), col, 1.3);
    }
}

fn draw_cursor(dl: DrawList, c: Vec2, on: bool, inherited_off: bool) {
    let col = icon_colour(on, inherited_off);
    let tip = v2(c.x - 3.5, c.y - 6.0);
    dl.triangle_filled(tip, v2(tip.x, tip.y + 11.0), v2(tip.x + 7.5, tip.y + 8.0), col);
    dl.line(v2(tip.x + 3.0, tip.y + 8.0), v2(tip.x + 6.0, tip.y + 13.0), col, 1.8);
}

/// An icon (a glyph of the UI font) centred on `c`.
fn draw_icon(dl: DrawList, c: Vec2, icon: &str, col: u32) {
    let w = gui::calc_text_width(icon);
    dl.text(v2(c.x - w * 0.5, c.y - gui::text_line_height() * 0.5), col, icon);
}

/// A layer's name path as the tree keys it (`root/<name>#<ordinal among the siblings of that name>/...`, see `rebuild_tree`): it survives the
/// index shifts of structural edits.
pub(crate) fn layer_key(sc: &TedScene, layer: usize) -> Option<String> {
    let mut chain = vec![layer];
    let mut p = parent_layer(sc, layer);
    while let Some(l) = p {
        chain.push(l);
        p = parent_layer(sc, l);
    }

    chain.reverse();
    if Some(chain[0]) != sc.scene_graph() {
        return None;
    }

    let mut key = "root".to_owned();
    for w in chain.windows(2) {
        let (parent, child) = (w[0], w[1]);
        let nm = label_of(sc, child);
        let ordinal = sc.children(parent).into_iter().take_while(|&k| k != child).filter(|&k| label_of(sc, k) == nm).count();
        key = format!("{key}/{nm}#{ordinal}");
    }

    Some(key)
}

/// The layer with a name path (`layer_key`), if there still is one.
pub(crate) fn layer_by_key(sc: &TedScene, key: &str) -> Option<usize> {
    let mut parts = key.split('/');
    if parts.next() != Some("root") {
        return None;
    }

    let mut at = sc.scene_graph()?;
    for part in parts {
        let (nm, ordinal) = part.rsplit_once('#')?;
        let ordinal = ordinal.parse::<usize>().ok()?;
        at = sc.children(at).into_iter().filter(|&k| label_of(sc, k) == nm).nth(ordinal).filter(|&k| sc.is_layer(k))?;
    }

    Some(at)
}

/// The name shown for an element: its name, else its type.
fn label_of(sc: &TedScene, e: usize) -> String {
    let n = sc.name(e);
    if n.is_empty() { sc.type_of(e).to_owned() } else { n }
}

impl TileTool {
    pub(crate) fn rebuild_tree(&mut self, d: &TileDocument) {
        self.tree_rows.clear();
        self.tree_filter_seen = self.tree_filter.clone();
        let hidden = &d.tile().display().hidden_types;
        self.tree_hidden_seen = Some(hidden.iter().cloned().collect());
        self.tree_prefab_seen = self.prefab_version;
        let sc = d.tile().tscene();
        let Some(g) = sc.scene_graph() else { return };

        // With a filter: only the elements that match or hold a match, their layers open.
        let filter = self.tree_filter.trim().to_lowercase();
        let keep = (!filter.is_empty()).then(|| {
            let mut keep = HashSet::new();
            mark_matches(sc, g, &filter, &mut keep);
            keep.insert(g);
            keep
        });

        let mut stack = vec![(g, 0usize, "root".to_owned())];

        // Depth first, children in order: the stack holds them reversed.
        while let Some((e, depth, key)) = stack.pop() {
            // A copy of a prefab is one row (its contents are listed only while it is edited).
            let layer = sc.is_layer(e) && !self.closed_prefab(d, e);
            let mut kids = if layer { sc.children(e) } else { vec![] };
            let all_kids = kids.clone();
            if let Some(keep) = &keep {
                kids.retain(|k| keep.contains(k));
            }

            // The types turned off at the top are not listed (layers and groups always are).
            kids.retain(|&k| sc.is_layer(k) || !hidden.contains(sc.type_of(k)));

            let open = depth == 0 || (layer && !kids.is_empty() && (keep.is_some() || self.tree_open.contains(&key)));
            self.tree_rows.push(TreeRow { elem: e, depth, key: key.clone(), layer, open, kids: kids.len() });
            if !open {
                continue;
            }

            // The keys count every child, shown or not: a filtered row keeps the key it has unfiltered.
            let mut seen: HashMap<String, usize> = HashMap::new();
            let mut children = vec![];
            for k in all_kids {
                let nm = label_of(sc, k);
                let ordinal = seen.entry(nm.clone()).or_default();
                if kids.contains(&k) {
                    children.push((k, depth + 1, format!("{key}/{nm}#{ordinal}")));
                }

                *ordinal += 1;
            }

            stack.extend(children.into_iter().rev());
        }

        self.tree_doc = d.id();
        self.tree_version = Some(d.tile().structure_version());
        self.tree_open_version_seen = Some(self.tree_open_version);
    }

    /// Deletes the selection (layers with everything in them), asking first when a layer holds a lot.
    pub(crate) fn request_delete(&mut self, e: &mut Editor) {
        let sel = selected(e);
        let Some(d) = crate::tool::document(e) else { return };
        if sel.is_empty() {
            return;
        }

        let sc = d.tile().tscene();
        let weight = sel.iter().map(|&el| sc.doc().subtree_size(el)).sum::<usize>();
        if weight > CONFIRM_DELETE_ABOVE && sel.iter().any(|&el| sc.is_layer(el)) {
            self.delete_confirm = true;
            self.delete_weight = weight;
            return;
        }

        d.remove_items(&sel);
        self.status = "deleted".to_owned();
    }

    pub(crate) fn begin_rename(&mut self, d: &TileDocument, elem: usize) {
        self.rename_elem = Some(elem);
        self.rename_focus = true;
        self.rename_buf = d.tile().tscene().name(elem);
    }

    /// Opens the layers above an element picked elsewhere (the viewport) and asks for its row to be scrolled into view. An element inside a
    /// group that the tree does not expand is shown through the group's row.
    pub(crate) fn reveal(&mut self, d: &TileDocument, elem: usize) {
        let sc = d.tile().tscene();

        // The layers above (nearest first; the file nests a `children` element between a layer and its contents, so the tree's parent of an
        // element is the nearest layer above it), and the row: the element itself, or the ancestor that is a child of the nearest layer.
        let mut layers = vec![];
        let mut p = parent_layer(sc, elem);
        while let Some(l) = p {
            layers.push(l);
            p = parent_layer(sc, l);
        }

        let Some(&nearest) = layers.first() else { return };
        let kids = sc.children(nearest);
        let mut row = elem;
        while !kids.contains(&row) {
            let Some(up) = sc.doc().elements()[row].parent else { return };
            row = up;
        }

        // Their keys as rebuild_tree makes them, from the scene graph down.
        layers.reverse();
        let Some(first) = layers.iter().position(|&l| Some(l) == sc.scene_graph()) else { return };
        let mut key = "root".to_owned();
        self.tree_open.insert(key.clone());
        for w in layers[first..].windows(2) {
            let (layer, child) = (w[0], w[1]);
            let nm = label_of(sc, child);
            let ordinal = sc.children(layer).into_iter().take_while(|&k| k != child).filter(|&k| label_of(sc, k) == nm).count();
            key = format!("{key}/{nm}#{ordinal}");
            self.tree_open.insert(key.clone());
        }

        self.tree_open_version += 1;
        self.tree_scroll_to = Some(row);
    }

    /// The open-state key of an element: the name path of the layers above it (see `rebuild_tree`).
    pub(crate) fn tree_key_of(&self, elem: usize) -> String {
        self.tree_rows.iter().find(|r| r.elem == elem).map(|r| r.key.clone()).unwrap_or_default()
    }
}

/// Marks the elements under `e` that match the filter (name or type, lower-cased) or hold one; true when `e` is marked.
fn mark_matches(sc: &TedScene, e: usize, filter: &str, keep: &mut HashSet<usize>) -> bool {
    let mut any = false;
    if sc.is_layer(e) {
        for k in sc.children(e) {
            any |= mark_matches(sc, k, filter, keep);
        }
    }

    if any || label_of(sc, e).to_lowercase().contains(filter) || sc.type_of(e).to_lowercase().contains(filter) {
        keep.insert(e);
        return true;
    }

    false
}

/// Where a drop on a row puts the dragged items.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Zone {
    Before,
    Into,
    After,
}

pub fn draw_layers_panel(tool: &mut TileTool, e: &mut Editor) {
    gui::set_next_window_size(v2(380.0, 600.0), gui::COND_FIRST_USE_EVER);
    if !gui::begin("Scene", None, 0) {
        gui::end();
        return;
    }

    let Some(id) = document_id(e) else {
        gui::text_disabled("No tile open.");
        gui::end();
        return;
    };

    tool.prefab_library(e);
    let d = e.docs.find_as::<TileDocument>(id).expect("a tile");
    let Some(graph) = d.tile().tscene().scene_graph() else {
        gui::text_disabled("This tile has no SCENE_GRAPH.");
        gui::end();
        return;
    };

    // A selection made elsewhere (the viewport): open the tree down to it and scroll it into view. The tree's own clicks mark what they
    // selected as followed already.
    let first = selected(e).first().copied();
    if first != tool.tree_synced {
        tool.tree_synced = first;
        if let Some(el) = first {
            tool.reveal(d, el);
        }
    }

    let hidden_now = d.tile().display().hidden_types.iter().cloned().collect::<Vec<_>>();
    if tool.tree_doc != id || tool.tree_version != Some(d.tile().structure_version()) || tool.tree_open_version_seen != Some(tool.tree_open_version)
        || tool.tree_filter != tool.tree_filter_seen || tool.tree_hidden_seen.as_ref() != Some(&hidden_now) || tool.tree_prefab_seen != tool.prefab_version
    {
        tool.rebuild_tree(d);
    }

    // ---- toolbar
    {
        let sel = selected(e);
        let d = e.docs.find_as_mut::<TileDocument>(id).expect("a tile");
        let sc = d.tile().tscene();

        // Where a new layer goes: inside the selected layer, else next to the selected item, else the scene graph.
        let target = match sel.first() {
            Some(&s) if sc.is_layer(s) => s,
            Some(&s) => parent_layer(sc, s).map_or(graph, |p| p.max(graph)),
            None => graph,
        };

        let one_layer = sel.len() == 1 && sc.is_layer(sel[0]) && sc.type_of(sel[0]) != "SCENE_GRAPH";
        if gui::button("New layer") {
            match d.add_layer(Some(target), "New layer") {
                Ok(_) => {
                    tool.tree_open.insert(tool.tree_key_of(target));
                    tool.tree_open_version += 1;
                    tool.begin_rename_next = true;
                    tool.status = "layer added".to_owned();
                }

                Err(err) => tool.status = err,
            }
        }

        gui::same_line();
        gui::begin_disabled(sel.is_empty());
        let into = tool.new_items_layer(d);
        if gui::button("Group") {
            tool.status = into.ok_or_else(|| "this tile has no scene graph".to_owned()).and_then(|into| d.group_instances(&sel, into, "")).map_or_else(|err| err, |_| "grouped".to_owned());
        }

        if gui::is_item_hovered() {
            gui::set_tooltip("Put the selected items into a new instance group in the working layer (Ctrl+G)");
        }

        gui::same_line();
        let sc = d.tile().tscene();
        let has_group = sel.iter().any(|&s| matches!(sc.type_of(s), "UserInstanceGroup" | "INSTANCE_GROUP"));
        gui::begin_disabled(!has_group);
        if gui::button("Ungroup") {
            tool.status = into.ok_or_else(|| "this tile has no scene graph".to_owned()).and_then(|into| d.ungroup(&sel, into))
                .map_or_else(|err| err, |freed| format!("ungrouped: {} items", freed.len()));
        }

        gui::end_disabled();
        if gui::is_item_hovered() {
            gui::set_tooltip("Take the selected instance groups apart into the working layer (Ctrl+U)");
        }

        gui::same_line();
        let delete = gui::button("Delete");
        gui::end_disabled();
        gui::same_line();
        gui::begin_disabled(!one_layer);
        if gui::button("Duplicate")
            && let Err(err) = d.duplicate_layer(sel[0])
        {
            tool.status = err;
        }

        gui::end_disabled();
        gui::same_line();
        gui::text_disabled(&format!("{} selected", sel.len()));
        if delete {
            tool.request_delete(e);
        }
    }

    // The working layer, where new items go.
    {
        let d = e.docs.find_as::<TileDocument>(id).expect("a tile");
        let working = tool.working_layer_of(d);
        if working.is_none() && tool.working_layer.as_ref().is_some_and(|(doc, _)| *doc == id) {
            tool.working_layer = None;
            tool.status = "the working layer is gone: new items go to the root".to_owned();
        }

        gui::align_text_to_frame_padding();
        gui::text_colored(theme::palette().highlight, wce_editor::icons::SQUARE_PEN);
        gui::same_line();
        match working {
            Some(l) => {
                gui::text(&format!("Working layer: {}", TileTool::layer_label(d, l)));
                gui::same_line();
                if gui::small_button(&format!("{}##clearworking", wce_editor::icons::X)) {
                    tool.working_layer = None;
                }

                if gui::is_item_hovered() {
                    gui::set_tooltip("Clear the working layer: new items go to the root");
                }
            }

            None => gui::text_disabled("Working layer: the root (right click a layer to set one)"),
        }
    }

    gui::set_next_item_width(-1.0);
    gui::input_text("##treefilter", Some("Filter by name or type"), &mut tool.tree_filter, 256, 0);
    draw_type_toggles(e, id);

    // ---- the tree
    let footer = gui::frame_height_with_spacing() * 1.2;
    if gui::begin_child("scenetree", v2(0.0, -footer), gui::CHILD_BORDERS, 0) {
        draw_tree(tool, e, id);
    }

    gui::end_child();

    // A layer created by a button or a menu: its name goes into edit mode as soon as the selection has caught up with the hint.
    if tool.begin_rename_next {
        let sel = selected(e);
        let d = e.docs.find_as::<TileDocument>(id).expect("a tile");
        let sc = d.tile().tscene();
        if sel.len() == 1 && sc.is_layer(sel[0]) && sc.type_of(sel[0]) != "SCENE_GRAPH" {
            tool.begin_rename(d, sel[0]);

            // Open its ancestors so the row is visible.
            let mut p = parent_layer(sc, sel[0]);
            while let Some(l) = p {
                tool.tree_open.insert(tool.tree_key_of(l));
                p = parent_layer(sc, l);
            }

            tool.tree_open_version += 1;
            tool.begin_rename_next = false;
        } else {
            tool.rename_wait += 1;
            if tool.rename_wait > 5 {
                tool.begin_rename_next = false;
            }
        }
    } else {
        tool.rename_wait = 0;
    }

    // ---- delete confirmation
    if tool.delete_confirm {
        gui::open_popup("Delete layers?");
        tool.delete_confirm = false;
    }

    if gui::begin_popup_modal("Delete layers?", None, gui::WINDOW_ALWAYS_AUTO_RESIZE) {
        gui::text(&format!("The selection holds {} elements. Delete it?", tool.delete_weight));
        if gui::button("Delete") {
            let sel = selected(e);
            e.docs.find_as_mut::<TileDocument>(id).expect("a tile").remove_items(&sel);
            tool.status = "deleted".to_owned();
            gui::close_current_popup();
        }

        gui::same_line();
        if gui::button("Cancel") || gui::is_key_pressed(gui::KEY_ESCAPE, true) {
            gui::close_current_popup();
        }

        gui::end_popup();
    }

    tool.draw_save_prefab_popup(e);
    gui::end();
}

/// The types on the tile as toggles (icon and count): a type turned off is neither listed nor shown in the viewport (a view setting of this
/// tool, not a flag of the file). A right click shows that type alone; a right click on the only one shown shows them all again.
fn draw_type_toggles(e: &mut Editor, id: wce_scene::render_scene::DocumentId) {
    let Some(d) = e.docs.find_as_mut::<TileDocument>(id) else { return };
    let counts = d.tile().type_counts().clone();
    let hidden = d.tile().display().hidden_types.clone();
    let mut change: Option<Vec<(String, bool)>> = None;
    // The buttons flow left to right and wrap at the panel's edge.
    let right_edge = gui::cursor_screen_pos().x + gui::content_region_avail().x;
    for (i, (ty, n)) in counts.iter().enumerate() {
        let on = !hidden.contains(ty);
        let label = format!("{} {n}##ty{i}", type_icon(ty));
        let width = gui::calc_text_width(&format!("{} {n}", type_icon(ty))) + gui::style_frame_padding().x * 2.0;
        if i != 0 && gui::item_rect().1.x + gui::item_spacing().x + width < right_edge {
            gui::same_line();
        }

        gui::push_style_color(gui::COL_BUTTON, if on { gui::style_color(gui::COL_BUTTON_ACTIVE) } else { gui::style_color(gui::COL_FRAME_BG) });
        if !on {
            gui::push_style_color(gui::COL_TEXT, gui::style_color(gui::COL_TEXT_DISABLED));
        }

        if gui::small_button(&label) {
            change = Some(vec![(ty.clone(), !on)]);
        }

        if gui::is_item_clicked_with(1) {
            let only = counts.keys().all(|t| t == ty || hidden.contains(t)) && on;
            change = Some(counts.keys().map(|t| (t.clone(), only || t == ty)).collect());
        }

        gui::pop_style_color(if on { 1 } else { 2 });
        if gui::is_item_hovered() {
            gui::set_tooltip(&format!("{ty}: {n}\nClick to {}; right click to show only this type (again: all)", if on { "hide" } else { "show" }));
        }
    }

    if let Some(change) = change {
        let t = d.tile_mut();
        for (ty, on) in change {
            if on {
                t.display_mut().hidden_types.remove(&ty);
            } else {
                t.display_mut().hidden_types.insert(ty);
            }
        }

        t.display_changed();
    }
}

/// A layer's export types (TEd's "Export" property), the current one ticked.
fn export_menu(sc: &TedScene, layer: usize, actions: &mut Vec<RowAction>) {
    let now = own_export(sc, layer);
    for (kind, label) in EXPORT_TYPES {
        if gui::menu_item(&format!("{}  {label}", export_icon(kind)), None, now == kind, true) {
            actions.push(RowAction::ExportType(layer, format!("LayerExportType_{kind}")));
        }
    }
}

/// A prefab action asked from a row's menu.
enum PrefabAction {
    Edit(usize),
    Apply,
    Cancel,
    Unpack(usize),
    Save(usize),
}

/// What a row asked for this frame, applied once the rows are drawn (the edits move element indices).
enum RowAction {
    SetFlags(Vec<usize>, &'static str, bool),
    AddLayer(usize, String),
    GroupIntoLayer(Vec<usize>),
    Group(Vec<usize>),
    Ungroup(Vec<usize>),
    SetWorking(Option<usize>),
    Prefab(PrefabAction),
    MoveInto(Vec<usize>, usize),
    Rename(usize),
    DuplicateLayer(usize),
    Delete,
    ExportType(usize, String),
    CommitRename(usize, String),
    ToggleOpen(String, bool),
}

fn draw_tree(tool: &mut TileTool, e: &mut Editor, id: wce_scene::render_scene::DocumentId) {
    let row_h = gui::text_line_height_with_spacing() + 2.0;
    let dl = gui::window_draw_list();
    let window_hovered = gui::is_window_hovered_with_children();
    let sel_all = selected(e);
    let mut actions = vec![];
    let mut pending_drop: Option<(Vec<usize>, usize, Option<usize>)> = None;
    let mut activate = false;
    if let Some(target) = tool.tree_scroll_to.take()
        && let Some(i) = tool.tree_rows.iter().position(|r| r.elem == target)
    {
        let (y, h) = (i as f32 * row_h, gui::window_height());
        if y < gui::scroll_y() || y + row_h > gui::scroll_y() + h {
            gui::set_scroll_y((y - (h - row_h) * 0.5).max(0.0));
        }
    }

    {
        let d = e.docs.find_as::<TileDocument>(id).expect("a tile");
        let working = tool.working_layer_of(d);
        let edited_copy = tool.edited_copy(d);
        let lib = tool.prefabs.clone();
        let prefab_of = |el: usize| -> Option<String> {
            let sc = d.tile().tscene();
            (sc.type_of(el) == "UserInstanceGroup").then(|| lib.as_ref().and_then(|l| l.get(&sc.name(el)).map(|p| p.key.clone()))).flatten()
        };
        let t = d.tile();
        let sc = t.tscene();
        let rows = tool.tree_rows.clone();
        let selection = &mut e.selection;
        let tool_ref = &mut *tool;
        gui::clipped_rows(rows.len(), row_h, |i| {
            let row = &rows[i];
            let el = row.elem;
            gui::push_id_int(i as i32);
            let r = d.ref_of(el);
            let is_sel = selection.contains(r);
            let (own_vis, own_act) = (t.own_flag(el, "OIF_VISIBLE"), t.own_flag(el, "OIF_ACTIVE"));
            let (vis, act) = (t.shown(el), t.selectable(el));
            let p0 = gui::cursor_screen_pos();
            let avail = gui::content_region_avail().x;
            let is_graph = sc.type_of(el) == "SCENE_GRAPH";

            // The row: selection, double-click to rename, context menu, drag source and drop target.
            gui::set_next_item_allow_overlap();
            let clicked = gui::selectable_sized("##row", is_sel, gui::SELECTABLE_ALLOW_DOUBLE_CLICK, v2(avail - ICON_COLUMNS * ICON_WIDTH, row_h));
            let row_hovered = gui::is_item_hovered();
            let (rmin, rmax) = (p0, v2(p0.x + avail - ICON_COLUMNS * ICON_WIDTH, p0.y + row_h));
            if clicked && tool_ref.rename_elem != Some(el) {
                let io = gui::io();
                if io.KeyCtrl {
                    selection.toggle(r);
                } else if io.KeyShift && !selection.is_empty() && tool_ref.anchor.is_some() {
                    if let Some(a) = rows.iter().position(|x| Some(x.elem) == tool_ref.anchor) {
                        for x in &rows[a.min(i)..=a.max(i)] {
                            selection.add(d.ref_of(x.elem));
                        }
                    }
                } else {
                    selection.set(r);
                    tool_ref.anchor = Some(el);
                }

                activate = true;
                if gui::is_mouse_double_clicked(0) && !is_graph {
                    actions.push(RowAction::Rename(el));
                }
            }

            if gui::is_item_clicked_with(1) && !is_sel {
                selection.set(r);
                tool_ref.anchor = Some(el);
            }

            // Drag source: the row, or the whole selection when the row is part of it.
            if !is_graph && gui::begin_drag_drop_source() {
                let elems = if is_sel { sel_all.clone() } else { vec![el] };
                gui::set_drag_drop_payload("TILE_ELEMS", &elems.iter().map(|&x| x as i32).collect::<Vec<_>>());
                gui::text(&format!("{} item{}", elems.len(), if elems.len() == 1 { "" } else { "s" }));
                gui::end_drag_drop_source();
            }

            if gui::begin_drag_drop_target() {
                if let Some((data, delivered)) = gui::accept_drag_drop_payload("TILE_ELEMS") {
                    let y = (gui::mouse_pos().y - rmin.y) / row_h;
                    let mut zone = if row.layer {
                        if y < 0.25 { Zone::Before } else if y > 0.75 { Zone::After } else { Zone::Into }
                    } else if y < 0.5 {
                        Zone::Before
                    } else {
                        Zone::After
                    };

                    if is_graph {
                        zone = Zone::Into;
                    }

                    let (layer, before) = match zone {
                        Zone::Into => (Some(el), None),
                        Zone::Before => (parent_layer(sc, el), Some(el)),
                        Zone::After => {
                            // An open layer: "after" it means its first child position, which is "into" at the front.
                            if row.layer && row.open && row.kids != 0 {
                                (Some(el), sc.children(el).first().copied())
                            } else {
                                // In front of the next sibling.
                                let layer = parent_layer(sc, el);
                                let sibs = layer.map(|l| sc.children(l)).unwrap_or_default();
                                (layer, sibs.iter().position(|&s| s == el).and_then(|k| sibs.get(k + 1).copied()))
                            }
                        }
                    };

                    // The indicator.
                    let c = theme::u32(theme::palette().highlight);
                    if zone == Zone::Into {
                        dl.rect(rmin, rmax, c, 2.0, 1.5);
                    } else {
                        let y = if zone == Zone::Before { rmin.y } else { rmax.y };
                        dl.line(v2(rmin.x, y), v2(rmax.x, y), c, 2.0);
                    }

                    if let (true, Some(l)) = (delivered, layer) {
                        pending_drop = Some((data.iter().filter_map(|&x| usize::try_from(x).ok()).collect(), l, before));
                    }
                }

                gui::end_drag_drop_target();
            }

            // Context menu.
            if gui::begin_popup_context_item("rowmenu") {
                let sel = &sel_all;
                if row.layer && gui::menu_item("New layer inside", None, false, true) {
                    actions.push(RowAction::AddLayer(el, row.key.clone()));
                }

                if !sel.is_empty() && gui::menu_item("Group", Some("Ctrl+G"), false, true) {
                    actions.push(RowAction::Group(sel.clone()));
                }

                let has_group = sel.iter().any(|&s| matches!(sc.type_of(s), "UserInstanceGroup" | "INSTANCE_GROUP"));
                if has_group && gui::menu_item("Ungroup", Some("Ctrl+U"), false, true) {
                    actions.push(RowAction::Ungroup(sel.clone()));
                }

                if !sel.is_empty() && gui::menu_item("Group into a new layer", None, false, true) {
                    actions.push(RowAction::GroupIntoLayer(sel.clone()));
                }

                // Prefabs: a copy is edited, unpacked; a plain instance group is saved as one.
                match prefab_of(el) {
                    Some(_) if Some(el) == edited_copy => {
                        if gui::menu_item("Apply the prefab edit", None, false, true) {
                            actions.push(RowAction::Prefab(PrefabAction::Apply));
                        }

                        if gui::menu_item("Cancel the prefab edit", None, false, true) {
                            actions.push(RowAction::Prefab(PrefabAction::Cancel));
                        }
                    }

                    Some(_) => {
                        if gui::menu_item("Edit prefab", None, false, edited_copy.is_none()) {
                            actions.push(RowAction::Prefab(PrefabAction::Edit(el)));
                        }

                        if gui::menu_item("Unpack prefab", None, false, true) {
                            actions.push(RowAction::Prefab(PrefabAction::Unpack(el)));
                        }
                    }

                    None if sc.type_of(el) == "UserInstanceGroup" && gui::menu_item("Save as prefab...", None, false, true) => {
                        actions.push(RowAction::Prefab(PrefabAction::Save(el)));
                    }

                    None => {}
                }

                if matches!(sc.type_of(el), "LayerInstance" | "SCENE_GRAPH") {
                    if Some(el) == working {
                        if gui::menu_item("Clear the working layer", None, false, true) {
                            actions.push(RowAction::SetWorking(None));
                        }
                    } else if gui::menu_item("Set as the working layer", None, false, true) {
                        actions.push(RowAction::SetWorking(Some(el)));
                    }
                }

                if row.layer && !sel.is_empty() && !sel.contains(&el) && gui::menu_item("Move selection here", None, false, true) {
                    actions.push(RowAction::MoveInto(sel.clone(), el));
                }

                gui::separator();
                if !is_graph {
                    if gui::menu_item("Rename", Some("F2"), false, true) {
                        actions.push(RowAction::Rename(el));
                    }

                    if row.layer && gui::menu_item("Duplicate", None, false, true) {
                        actions.push(RowAction::DuplicateLayer(el));
                    }

                    if gui::menu_item("Delete", Some("Del"), false, true) {
                        actions.push(RowAction::Delete);
                    }
                }

                gui::separator();
                let targets = if is_sel { sel.clone() } else { vec![el] };
                if gui::menu_item(if own_vis { "Hide" } else { "Show" }, None, false, true) {
                    actions.push(RowAction::SetFlags(targets.clone(), "OIF_VISIBLE", !own_vis));
                }

                if gui::menu_item(if own_act { "Lock (not selectable)" } else { "Unlock" }, None, false, true) {
                    actions.push(RowAction::SetFlags(targets, "OIF_ACTIVE", !own_act));
                }

                if sc.type_of(el) == "LayerInstance" && gui::begin_menu("Export") {
                    export_menu(sc, el, &mut actions);
                    gui::end_menu();
                }

                gui::end_popup();
            }

            // What is drawn on the row.
            let cy = p0.y + row_h * 0.5;
            let x0 = p0.x + 4.0 + row.depth as f32 * 14.0;
            if row.layer && row.kids != 0 {
                let c = v2(x0 + 5.0, cy);
                let ac = theme::u32(theme::mix(theme::text(), theme::dim(), 0.4));
                if row.open {
                    dl.triangle_filled(v2(c.x - 4.0, c.y - 2.5), v2(c.x + 4.0, c.y - 2.5), v2(c.x, c.y + 3.5), ac);
                } else {
                    dl.triangle_filled(v2(c.x - 2.5, c.y - 4.0), v2(c.x - 2.5, c.y + 4.0), v2(c.x + 3.5, c.y), ac);
                }

                // A click on the arrow toggles; the row's own click does not select then.
                let m = gui::mouse_pos();
                if row_hovered && gui::is_mouse_clicked(0) && m.x >= x0 - 2.0 && m.x < x0 + 12.0 && m.y >= p0.y && m.y < p0.y + row_h {
                    actions.push(RowAction::ToggleOpen(row.key.clone(), row.open));
                }
            }

            let text_col = if !vis {
                theme::u32(theme::dim())
            } else if !act {
                theme::u32(theme::mix(theme::text(), theme::dim(), 0.5))
            } else {
                theme::u32(theme::text())
            };

            let mut tx = x0 + 16.0;

            // The working layer: a pen before its name.
            if Some(el) == working {
                draw_icon(dl, v2(tx + 7.0, cy), wce_editor::icons::SQUARE_PEN, theme::u32(theme::palette().highlight));
                tx += 18.0;
            }

            if tool_ref.rename_elem == Some(el) {
                gui::set_cursor_screen_pos(v2(tx, p0.y));
                gui::set_next_item_width((avail - ICON_COLUMNS * ICON_WIDTH - (tx - p0.x) - 4.0).max(60.0));
                if tool_ref.rename_focus {
                    gui::set_keyboard_focus_here(0);
                    tool_ref.rename_focus = false;
                    tool_ref.rename_active_seen = false;
                }

                let enter = gui::input_text("##rename", None, &mut tool_ref.rename_buf, 128, gui::INPUT_ENTER_RETURNS_TRUE | gui::INPUT_AUTO_SELECT_ALL);
                if gui::is_item_active() {
                    tool_ref.rename_active_seen = true;
                }

                if enter {
                    actions.push(RowAction::CommitRename(el, tool_ref.rename_buf.clone()));
                    tool_ref.rename_elem = None;
                } else if gui::is_key_pressed(gui::KEY_ESCAPE, true) || (tool_ref.rename_active_seen && !gui::is_item_active()) {
                    tool_ref.rename_elem = None;
                }
            } else {
                let mut label = label_of(sc, el);
                if row.layer && row.kids != 0 {
                    label += &format!("  ({})", row.kids);
                }

                dl.push_clip_rect(v2(tx, p0.y), v2(p0.x + avail - ICON_COLUMNS * ICON_WIDTH - 2.0, p0.y + row_h));
                dl.text(v2(tx, p0.y + (row_h - gui::text_line_height()) * 0.5), text_col, &label);
                dl.pop_clip_rect();
            }

            // The type (a prefab copy's is Prefab) and where it is exported.
            let ty = sc.type_of(el);
            let prefab = prefab_of(el);
            let edited = prefab.is_some() && Some(el) == edited_copy;
            gui::set_cursor_screen_pos(v2(p0.x + avail - 4.0 * ICON_WIDTH, p0.y));
            gui::invisible_button("##type", v2(ICON_WIDTH, row_h), gui::BUTTON_LEFT);
            if gui::is_item_hovered() {
                match &prefab {
                    Some(k) if edited => gui::set_tooltip(&format!("Prefab {k}, being edited: right click to apply or cancel")),
                    Some(k) => gui::set_tooltip(&format!("Prefab {k}")),
                    None => gui::set_tooltip(ty),
                }
            }

            let (icon, icol) = match prefab {
                Some(_) if edited => (crate::prefab_panel::PREFAB_ICON, theme::u32(crate::prefab_panel::edited_colour())),
                Some(_) => (crate::prefab_panel::PREFAB_ICON, text_col),
                None => (type_icon(ty), text_col),
            };

            draw_icon(dl, v2(p0.x + avail - 3.5 * ICON_WIDTH, cy), icon, icol);
            let (export, decided_by) = effective_export(sc, el);
            let own = sc.type_of(el) == "LayerInstance";
            gui::set_cursor_screen_pos(v2(p0.x + avail - 3.0 * ICON_WIDTH, p0.y));
            if gui::invisible_button("##export", v2(ICON_WIDTH, row_h), gui::BUTTON_LEFT) && own {
                gui::open_popup("exportmenu");
            }

            if gui::is_item_hovered() {
                let by = match decided_by {
                    Some(l) if l == el => " (this layer)".to_owned(),
                    Some(l) => format!(" (layer {})", label_of(sc, l)),
                    None => String::new(),
                };

                let click = if own { "\nClick to change it" } else { "" };
                gui::set_tooltip(&format!("{}{by}{click}", export_label(&export)));
            }

            if gui::begin_popup("exportmenu") {
                export_menu(sc, el, &mut actions);
                gui::end_popup();
            }

            let inherited = decided_by != Some(el);
            let base = if export == "Off" { theme::error() } else { theme::text() };
            let ecol = if inherited { theme::mix(base, theme::palette_background(), 0.45) } else { base };
            draw_icon(dl, v2(p0.x + avail - 2.5 * ICON_WIDTH, cy), export_icon(&export), theme::u32(ecol));

            // The eye and the cursor arrow.
            let targets = if is_sel { sel_all.clone() } else { vec![el] };
            gui::set_cursor_screen_pos(v2(p0.x + avail - 2.0 * ICON_WIDTH, p0.y));
            if gui::invisible_button("##eye", v2(ICON_WIDTH, row_h), gui::BUTTON_LEFT) {
                actions.push(RowAction::SetFlags(targets.clone(), "OIF_VISIBLE", !own_vis));
            }

            if gui::is_item_hovered() {
                gui::set_tooltip(if own_vis { "Visible: click to hide" } else { "Hidden: click to show" });
            }

            draw_eye(dl, v2(p0.x + avail - 1.5 * ICON_WIDTH, cy), own_vis, own_vis && !vis);
            gui::set_cursor_screen_pos(v2(p0.x + avail - ICON_WIDTH, p0.y));
            if gui::invisible_button("##act", v2(ICON_WIDTH, row_h), gui::BUTTON_LEFT) {
                actions.push(RowAction::SetFlags(targets, "OIF_ACTIVE", !own_act));
            }

            if gui::is_item_hovered() {
                gui::set_tooltip(if own_act { "Selectable: click to lock" } else { "Locked: click to make selectable" });
            }

            draw_cursor(dl, v2(p0.x + avail - 0.5 * ICON_WIDTH, cy), own_act, own_act && !act);
            // Dear ImGui 1.92 wants an item, not a bare cursor move, to advance to the next row; the item spacing it adds is taken off, so
            // that the next row starts row_h below, as the C++ panel's cursor move did.
            gui::set_cursor_screen_pos(p0);
            gui::dummy(v2(avail, row_h - gui::item_spacing().y));
            gui::pop_id();
        });
    }

    // A click on a row makes the tile the active document; the tree does not scroll to what it selected itself.
    if activate {
        e.docs.set_active(id);
        tool.tree_synced = selected(e).first().copied();
    }

    // Keys while the pointer is over the tree.
    if window_hovered && !gui::io().WantTextInput {
        let sel = selected(e);
        let d = e.docs.find_as::<TileDocument>(id).expect("a tile");
        if gui::is_key_pressed(gui::KEY_F2, false) && sel.len() == 1 && d.tile().tscene().type_of(sel[0]) != "SCENE_GRAPH" {
            actions.push(RowAction::Rename(sel[0]));
        }
    }

    let mut prefab_actions = vec![];
    for action in actions {
        let d = e.docs.find_as_mut::<TileDocument>(id).expect("a tile");
        match action {
            RowAction::SetFlags(targets, flag, on) => {
                d.set_flags(&targets, flag, on);
            }

            RowAction::AddLayer(layer, key) => match d.add_layer(Some(layer), "New layer") {
                Ok(_) => {
                    tool.tree_open.insert(key);
                    tool.tree_open_version += 1;
                    tool.begin_rename_next = true;
                    tool.status = "layer added".to_owned();
                }

                Err(err) => tool.status = err,
            },

            RowAction::GroupIntoLayer(sel) => match d.group_items(&sel, "New layer") {
                Ok(_) => {
                    tool.status = "grouped into a new layer".to_owned();
                    tool.begin_rename_next = true;
                }

                Err(err) => tool.status = err,
            },

            RowAction::Group(sel) => {
                let into = tool.new_items_layer(d);
                tool.status = into.ok_or_else(|| "this tile has no scene graph".to_owned()).and_then(|into| d.group_instances(&sel, into, "")).map_or_else(|err| err, |_| "grouped".to_owned());
            }

            RowAction::Ungroup(sel) => {
                let into = tool.new_items_layer(d);
                tool.status = into.ok_or_else(|| "this tile has no scene graph".to_owned()).and_then(|into| d.ungroup(&sel, into))
                    .map_or_else(|err| err, |freed| format!("ungrouped: {} items", freed.len()));
            }

            RowAction::SetWorking(layer) => tool.status = tool.set_working_layer(d, layer).unwrap_or_else(|err| err),

            RowAction::Prefab(p) => prefab_actions.push(p),

            RowAction::MoveInto(sel, layer) => {
                let name = d.tile().tscene().name(layer);
                tool.status = match d.move_items(&sel, layer, None) {
                    Ok(()) => format!("moved into {name}"),
                    Err(err) => err,
                };
            }

            RowAction::Rename(el) => tool.begin_rename(d, el),
            RowAction::DuplicateLayer(el) => {
                if let Err(err) = d.duplicate_layer(el) {
                    tool.status = err;
                }
            }

            RowAction::Delete => tool.request_delete(e),
            RowAction::ExportType(el, value) => {
                d.set_attribute(el, "export_type", &value);
            }

            RowAction::CommitRename(el, name) => {
                // The working layer is kept by its name path: renamed, it is keyed again.
                let was_working = tool.working_layer_of(d) == Some(el);
                d.rename(el, &name);
                if was_working {
                    let _ = tool.set_working_layer(d, Some(el));
                }
            }

            RowAction::ToggleOpen(key, open) => {
                if open {
                    tool.tree_open.remove(&key);
                } else {
                    tool.tree_open.insert(key);
                }

                tool.tree_open_version += 1;
            }
        }
    }

    // The prefab actions need the editor (the library, the selection).
    for p in prefab_actions {
        let result = match p {
            PrefabAction::Edit(el) => tool.edit_prefab(e, el),
            PrefabAction::Apply => tool.apply_prefab_edit(e),
            PrefabAction::Cancel => tool.cancel_prefab_edit(e),
            PrefabAction::Unpack(el) => tool.unpack_prefab(e, el),
            PrefabAction::Save(el) => {
                let d = e.docs.find_as::<TileDocument>(id).expect("a tile");
                tool.request_save_prefab(d, el);
                Ok(String::new())
            }
        };

        tool.status = result.unwrap_or_else(|err| err);
    }

    if let Some((elems, layer, before)) = pending_drop {
        let d = e.docs.find_as_mut::<TileDocument>(id).expect("a tile");
        tool.status = match d.move_items(&elems, layer, before) {
            Ok(()) => "moved".to_owned(),
            Err(err) => err,
        };
    }
}
