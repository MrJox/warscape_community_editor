//! The Scene panel of the Tile tool: the layer tree laid out like TEd's Scene tab (a name, then an eye and a cursor arrow at the right edge that
//! are greyed when off), with selection, rename (F2 / double-click), delete, new layer, group, duplicate, export type, and drag and drop to
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

use crate::tool::{TileTool, TreeRow, document_id, selected};

const ICON_WIDTH: f32 = 22.0;

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

fn draw_layer_icon(dl: DrawList, c: Vec2, col: u32) {
    for i in 0..3 {
        let y = c.y - 5.0 + i as f32 * 3.5;
        dl.rect_filled(v2(c.x - 6.0, y), v2(c.x + 6.0, y + 2.0), col, 1.0);
    }
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
            let layer = sc.is_layer(e);
            let mut kids = if layer { sc.children(e) } else { vec![] };
            let all_kids = kids.clone();
            if let Some(keep) = &keep {
                kids.retain(|k| keep.contains(k));
            }

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

    if tool.tree_doc != id || tool.tree_version != Some(d.tile().structure_version()) || tool.tree_open_version_seen != Some(tool.tree_open_version)
        || tool.tree_filter != tool.tree_filter_seen
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
        if gui::button("Group") {
            match d.group_items(&sel, "New layer") {
                Ok(_) => {
                    tool.status = "grouped into a new layer".to_owned();
                    tool.begin_rename_next = true;
                }

                Err(err) => tool.status = err,
            }
        }

        if gui::is_item_hovered() {
            gui::set_tooltip("Put the selected items into a new layer (Ctrl+G)");
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

    gui::set_next_item_width(-1.0);
    gui::input_text("##treefilter", Some("Filter by name or type"), &mut tool.tree_filter, 256, 0);

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

    // ---- what is on the tile, per instance type (the visibility of a type is a view setting of this tool, not a flag of the file)
    if gui::collapsing_header("Instance types", 0) {
        let d = e.docs.find_as_mut::<TileDocument>(id).expect("a tile");
        let counts = d.tile().type_counts().clone();
        let t = d.tile_mut();
        for (ty, n) in counts {
            let mut vis = !t.display().hidden_types.contains(&ty);
            if gui::checkbox(&format!("{ty} ({n})##t"), &mut vis) {
                if vis {
                    t.display_mut().hidden_types.remove(&ty);
                } else {
                    t.display_mut().hidden_types.insert(ty.clone());
                }

                t.display_changed();
            }
        }
    }

    gui::end();
}

/// What a row asked for this frame, applied once the rows are drawn (the edits move element indices).
enum RowAction {
    SetFlags(Vec<usize>, &'static str, bool),
    AddLayer(usize, String),
    Group(Vec<usize>),
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
            let clicked = gui::selectable_sized("##row", is_sel, gui::SELECTABLE_ALLOW_DOUBLE_CLICK, v2(avail - 2.0 * ICON_WIDTH, row_h));
            let row_hovered = gui::is_item_hovered();
            let (rmin, rmax) = (p0, v2(p0.x + avail - 2.0 * ICON_WIDTH, p0.y + row_h));
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

                if !sel.is_empty() && gui::menu_item("Group selection into a new layer", Some("Ctrl+G"), false, true) {
                    actions.push(RowAction::Group(sel.clone()));
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

                if row.layer && !is_graph && sc.doc().get(el, "export_type").is_some() && gui::begin_menu("Export type") {
                    let now = t.export_type(el);
                    for kind in ["Normal", "Escalation1", "Escalation2", "Escalation3"] {
                        if gui::menu_item(kind, None, now == kind, true) {
                            actions.push(RowAction::ExportType(el, format!("LayerExportType_{kind}")));
                        }
                    }

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
            if row.layer {
                draw_layer_icon(dl, v2(tx + 6.0, cy), text_col);
                tx += 18.0;
            }

            if tool_ref.rename_elem == Some(el) {
                gui::set_cursor_screen_pos(v2(tx, p0.y));
                gui::set_next_item_width((avail - 2.0 * ICON_WIDTH - (tx - p0.x) - 4.0).max(60.0));
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
                } else if !row.layer {
                    label += &format!("   {}", sc.type_of(el));
                }

                if row.layer && !is_graph {
                    let x = t.export_type(el);
                    if x != "Normal" {
                        label += &format!("  [{x}]");
                    }
                }

                dl.push_clip_rect(v2(tx, p0.y), v2(p0.x + avail - 2.0 * ICON_WIDTH - 2.0, p0.y + row_h));
                dl.text(v2(tx, p0.y + (row_h - gui::text_line_height()) * 0.5), text_col, &label);
                dl.pop_clip_rect();
            }

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

            RowAction::Group(sel) => match d.group_items(&sel, "New layer") {
                Ok(_) => {
                    tool.status = "grouped".to_owned();
                    tool.begin_rename_next = true;
                }

                Err(err) => tool.status = err,
            },

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
                d.rename(el, &name);
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

    if let Some((elems, layer, before)) = pending_drop {
        let d = e.docs.find_as_mut::<TileDocument>(id).expect("a tile");
        tool.status = match d.move_items(&elems, layer, before) {
            Ok(()) => "moved".to_owned(),
            Err(err) => err,
        };
    }
}
