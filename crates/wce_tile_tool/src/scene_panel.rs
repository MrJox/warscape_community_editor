//! The Scene panel of the Tile tool: the layer tree laid out like TEd's Scene tab (a name, then at the right edge the type's icon, where it is
//! exported, an eye and a cursor arrow, greyed when off), a types' filter by the name filter (what the list and the viewport show), with selection, rename (F2 / double-click), delete, new layer, group, duplicate, export type, and drag and drop to
//! reorder or move into a layer. The eye is OIF_VISIBLE and the cursor arrow OIF_ACTIVE (selectable): `TileDocument::set_flags`. The tree is
//! flattened to the rows that are open and drawn through a list clipper, so a layer with 30,000 instances costs what its visible rows cost.
//! Open layers are remembered by their name path, which survives the index shifts of structural edits (a node id made of the element index
//! would not).

use std::collections::{HashMap, HashSet};

use wce_editor::editor::Editor;
use wce_editor::gui::{self, DrawList, Vec2, v2};
use wce_editor::{icons, theme};
use wce_editor_core::document::Document;
use wce_tile::ted_scene::TedScene;
use wce_tile_editor::tile_document::TileDocument;

use crate::entity_types::{EXPORT_TYPES, effective_export, export_icon, export_label, own_export, type_icon};
use crate::tool::{TileTool, TreeRow, document_id, selected};

const ICON_WIDTH: f32 = 22.0;

/// The icon columns at the right edge: the type, the export, the eye and the cursor arrow.
const ICON_COLUMNS: f32 = 4.0;

/// The variations column of a combined project: the narrowest it can be dragged to.
const VARIATIONS_MIN_WIDTH: f32 = 40.0;

/// The variations column's width: the one dragged to (`TileSettings::scene_variations_width`), else as wide as the variations' names
/// side by side; none without variations.
fn variations_width(e: &Editor, all: &[String]) -> f32 {
    if all.is_empty() {
        return 0.0;
    }

    let set = e.tool_settings::<crate::settings::TileSettings>(crate::settings::SETTINGS_ID).scene_variations_width;
    if set > 0.0 { set.max(VARIATIONS_MIN_WIDTH) } else { (gui::calc_text_width(&all.join(", ")) + 12.0).max(VARIATIONS_MIN_WIDTH) }
}

impl TileTool {
    /// A checkbox per variation of the open combined project: whether it shows (docs/plans/tile_variations.md, Revision 1: the user
    /// works on the tile, the variations shown are a filter of the view). The last one shown can't be unticked. Side by side
    /// (`inline`: the Scene panel's header) or one under the other (a menu).
    /// The variations shown asked for (`shown_pending`): counted down a frame at a time so that "Updating..." is on screen first, then
    /// shown (blocking that frame), the overlay kept for the frame after (what is made again then).
    pub(crate) fn apply_shown_pending(&mut self, e: &mut Editor) {
        self.shown_overlay = self.shown_overlay.saturating_sub(u8::from(self.shown_pending.is_none()));
        let Some((list, frames)) = self.shown_pending.as_mut() else { return };
        if *frames > 0 {
            *frames -= 1;
            return;
        }

        let list = std::mem::take(list);
        self.shown_pending = None;
        self.shown_overlay = 2;
        let Some(id) = document_id(e) else { return };
        let d = e.docs.find_as_mut::<TileDocument>(id).expect("a tile");
        if let Err(m) = d.tile_mut().show_variations(&mut e.scene, &list) {
            e.status(&m, false);
        }
    }

    pub(crate) fn shown_variation_items(&mut self, e: &mut Editor) {
        self.shown_variation_checkboxes(e, false);
    }

    fn shown_variation_checkboxes(&mut self, e: &mut Editor, inline: bool) {
        let Some(id) = document_id(e) else { return };
        let Some(d) = e.docs.find_as::<TileDocument>(id) else { return };
        let all = d.tile().variations().to_vec();
        // What was asked for already, while it waits to be shown (the viewport's "Updating...").
        let shown = self.shown_pending.as_ref().map_or_else(|| d.tile().shown_variations().to_vec(), |(l, _)| l.clone());
        let mut want = None;
        for (i, v) in all.iter().enumerate() {
            if inline && i > 0 {
                gui::same_line();
            }

            let mut on = shown.contains(v);
            let last = on && shown.len() == 1;
            gui::begin_disabled(last);
            if gui::checkbox(&format!("{v}##shown_variation"), &mut on) {
                want = Some(all.iter().filter(|x| if *x == v { on } else { shown.contains(x) }).cloned().collect::<Vec<_>>());
            }

            gui::end_disabled();
            if gui::is_item_hovered_allow_disabled() {
                gui::set_tooltip(&format!(
                    "Show what {v} has. A filter of the view, not of the tile: what belongs to no variation shown is hidden, and an element several have is drawn as the first shown has it.{}",
                    if last { "\nThe last one shown: tick another first." } else { "" }
                ));
            }
        }

        // Shown once the viewport has said it is updating (the switch blocks a frame for seconds on a big tile).
        if let Some(list) = want {
            self.shown_pending = Some((list, 2));
        }
    }
}

/// The tree's header: Name, the variations (a combined project's; its left edge dragged resizes it) and the icon columns, as the rows
/// lay them out (the scrollbar's width left for at the right).
fn draw_tree_header(e: &mut Editor, all: &[String]) {
    let dl = gui::window_draw_list();
    let p0 = gui::cursor_screen_pos();
    let h = gui::text_line_height_with_spacing() + 2.0;
    let pad = gui::style_scrollbar_size() + gui::style_frame_padding().x;
    let avail = gui::content_region_avail().x - pad;
    let var_w = variations_width(e, all);
    let dim = theme::u32(theme::mix(theme::text(), theme::palette_background(), 0.35));
    let ty = p0.y + (h - gui::text_line_height()) * 0.5;
    let cy = p0.y + h * 0.5;
    dl.text(v2(p0.x + 6.0, ty), dim, "Name");

    if var_w > 0.0 {
        let x0 = p0.x + avail - ICON_COLUMNS * ICON_WIDTH - var_w;
        dl.push_clip_rect(v2(x0 + 2.0, p0.y), v2(x0 + var_w - 2.0, p0.y + h));
        dl.text(v2(x0 + 4.0, ty), dim, "Variations");
        dl.pop_clip_rect();
        dl.line(v2(x0, p0.y + 3.0), v2(x0, p0.y + h - 3.0), dim, 1.0);

        // The column's left edge: dragged left widens it, right narrows it; a double click fits it to the names again.
        gui::set_cursor_screen_pos(v2(x0 - 4.0, p0.y));
        gui::invisible_button("##varsedge", v2(8.0, h), gui::BUTTON_LEFT);
        if gui::is_item_hovered() || gui::is_item_active() {
            gui::set_mouse_cursor_resize_ew();
            gui::set_tooltip("Drag to resize the variations column; double click: as wide as the names");
        }

        if gui::is_item_hovered() && gui::is_mouse_double_clicked(0) {
            e.update_tool_settings(crate::settings::SETTINGS_ID, |s: &mut crate::settings::TileSettings| s.scene_variations_width = 0.0);
        } else if gui::is_item_active() {
            let dx = gui::io().MouseDelta.x;
            if dx != 0.0 {
                let max = (avail - ICON_COLUMNS * ICON_WIDTH - 80.0).max(VARIATIONS_MIN_WIDTH);
                let w = (var_w - dx).clamp(VARIATIONS_MIN_WIDTH, max);
                e.update_tool_settings(crate::settings::SETTINGS_ID, |s: &mut crate::settings::TileSettings| s.scene_variations_width = w);
            }
        }
    }

    // The icon columns, each with its rows' icon and what it is.
    let cols: [(&str, &str); 4] = [
        ("##h_type", "Type (a prefab's copy: Prefab)"),
        ("##h_export", "Export: how the layer is exported (click a layer's to change it)"),
        ("##h_eye", "Visible: click a row's to hide or show it"),
        ("##h_act", "Selectable: click a row's to lock it or make it selectable"),
    ];
    for (k, (id, tip)) in cols.iter().enumerate() {
        let x = p0.x + avail - (4 - k) as f32 * ICON_WIDTH;
        gui::set_cursor_screen_pos(v2(x, p0.y));
        gui::invisible_button(id, v2(ICON_WIDTH, h), gui::BUTTON_LEFT);
        if gui::is_item_hovered() {
            gui::set_tooltip(tip);
        }

        let c = v2(x + ICON_WIDTH * 0.5, cy);
        match k {
            0 => draw_icon(dl, c, type_icon("LayerInstance"), dim),
            1 => draw_icon(dl, c, export_icon("Normal"), dim),
            2 => draw_eye(dl, c, true, false),
            _ => draw_cursor(dl, c, true, false),
        }
    }

    gui::set_cursor_screen_pos(p0);
    gui::dummy(v2(avail + pad, h));
    dl.line(v2(p0.x, p0.y + h), v2(p0.x + avail + pad, p0.y + h), dim, 1.0);
}

/// The variations popup of a layer's, group's or folder's row: one checkbox per variation, and "as its parent's".
fn variations_menu(all: &[String], set: &[String], own: bool, targets: &[usize], actions: &mut Vec<RowAction>) {
    for v in all {
        let mut on = set.contains(v);
        if gui::checkbox(v, &mut on) {
            let list = all.iter().filter(|x| if *x == v { on } else { set.contains(x) }).cloned().collect::<Vec<_>>();
            if !list.is_empty() {
                actions.push(RowAction::SetVariations(targets.to_vec(), Some(list)));
            }
        }
    }

    if own && gui::menu_item("As its parent's", None, false, true) {
        actions.push(RowAction::SetVariations(targets.to_vec(), None));
    }
}

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

/// What an element is directly in, for a drop next to it: its layer, or its holder (`wce_tile::parents`: a deployment zone, a siege AI
/// node or edge, a capture location).
fn parent_container(sc: &TedScene, e: usize) -> Option<usize> {
    let els = sc.doc().elements();
    std::iter::successors(els[e].parent, |&q| els[q].parent)
        .find(|&q| sc.is_layer(q) || wce_tile::parents::child_kind(sc.type_of(q)).is_some())
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

/// Whether an element opens in the tree to show what it holds: a layer (or a group), or a holder (`wce_tile::parents`), whose shapes are
/// items of their own.
fn opens(sc: &TedScene, e: usize) -> bool {
    sc.is_layer(e) || sc.type_of(e) == wce_tile::variations::FOLDER || wce_tile::parents::child_kind(sc.type_of(e)).is_some()
}

/// The name shown for an element: its name, else its type; a variation folder by its variations.
fn label_of(sc: &TedScene, e: usize) -> String {
    if sc.type_of(e) == wce_tile::variations::FOLDER {
        return format!("Only in {}", sc.doc().get(e, wce_tile::variations::VARIATIONS_ATTR).unwrap_or_default().replace(',', ", "));
    }

    let n = sc.name(e);
    if n.is_empty() { sc.type_of(e).to_owned() } else { n }
}

impl TileTool {
    pub(crate) fn rebuild_tree(&mut self, d: &TileDocument) {
        self.tree.rows.clear();
        self.tree.filter_seen = self.tree.filter.clone();
        let hidden = &d.tile().display().hidden_types;
        self.tree.hidden_seen = Some(hidden.iter().cloned().collect());
        self.tree.prefab_seen = self.prefab_version;
        let sc = d.tile().tscene();
        let Some(g) = sc.scene_graph() else { return };

        // With a filter: only the elements that match or hold a match, their layers open.
        let filter = self.tree.filter.trim().to_lowercase();
        let mut opened = HashSet::new();
        let keep = (!filter.is_empty()).then(|| {
            let mut keep = HashSet::new();
            mark_matches(d, g, &filter, &mut keep, &mut opened);
            keep.insert(g);
            keep
        });

        let mut stack = vec![(g, 0usize, "root".to_owned())];

        // Depth first, children in order: the stack holds them reversed.
        while let Some((e, depth, key)) = stack.pop() {
            // A copy of a prefab opens like a group: a click on one of its items opens the copy for overrides (`select_whole_prefabs`).
            let layer = sc.is_layer(e);
            let branch = opens(sc, e);
            let mut kids = if branch { d.tile().tree_children(e) } else { vec![] };
            let all_kids = kids.clone();
            if let Some(keep) = &keep {
                kids.retain(|k| keep.contains(k));
            }

            // The types turned off at the top are not listed (layers and groups always are).
            kids.retain(|&k| sc.is_layer(k) || !hidden.contains(sc.type_of(k)));

            let open = depth == 0 || (branch && !kids.is_empty() && (opened.contains(&e) || self.tree.open.contains(&key)));
            self.tree.rows.push(TreeRow { elem: e, depth, key: key.clone(), layer, branch, open, kids: kids.len() });
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

        self.tree.doc = d.id();
        self.tree.version = Some(d.tile().structure_version());
        self.tree.open_version_seen = Some(self.tree.open_version);
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
        self.tree.rename_elem = Some(elem);
        self.tree.rename_focus = true;
        self.tree.rename_buf = d.tile().tscene().name(elem);
    }

    /// Opens the layers above an element picked elsewhere (the viewport) and asks for its row to be scrolled into view. An element inside a
    /// group that the tree does not expand is shown through the group's row.
    pub(crate) fn reveal(&mut self, d: &TileDocument, elem: usize) {
        let sc = d.tile().tscene();

        // The layers above (nearest first; the file nests a `children` element between a layer and its contents, so the tree's parent of an
        // element is the nearest layer above it), and the row: the element itself, or the ancestor that is a child of the nearest layer.
        // (A holder opens too: a shape picked in the viewport is shown in it.)
        let tile = d.tile();
        let up = |e: usize| std::iter::successors(tile.tree_parent(e), |&q| tile.tree_parent(q)).find(|&q| opens(sc, q));

        let mut layers = vec![];
        let mut p = up(elem);
        while let Some(l) = p {
            layers.push(l);
            p = up(l);
        }

        let Some(&nearest) = layers.first() else { return };
        let kids = tile.tree_children(nearest);
        let mut row = elem;
        while !kids.contains(&row) {
            let Some(up) = tile.tree_parent(row) else { return };
            row = up;
        }

        // Their keys as rebuild_tree makes them, from the scene graph down.
        layers.reverse();
        let Some(first) = layers.iter().position(|&l| Some(l) == sc.scene_graph()) else { return };
        let mut key = "root".to_owned();
        self.tree.open.insert(key.clone());
        for w in layers[first..].windows(2) {
            let (layer, child) = (w[0], w[1]);
            let nm = label_of(sc, child);
            let ordinal = tile.tree_children(layer).into_iter().take_while(|&k| k != child).filter(|&k| label_of(sc, k) == nm).count();
            key = format!("{key}/{nm}#{ordinal}");
            self.tree.open.insert(key.clone());
        }

        self.tree.open_version += 1;
        self.tree.scroll_to = Some(row);
    }

    /// The open-state key of an element: the name path of the layers above it (see `rebuild_tree`).
    pub(crate) fn tree_key_of(&self, elem: usize) -> String {
        self.tree.rows.iter().find(|r| r.elem == elem).map(|r| r.key.clone()).unwrap_or_default()
    }
}

/// Marks the elements under `e` that match the filter (name or type, lower-cased) or hold one; true when `e` is marked.
/// The rows a filter keeps: the elements that match, everything below them (`keep`), and the layers above them, which open (`open`; a
/// match opens too, to show what it holds; below it, rows open as the user left them).
fn mark_matches(d: &TileDocument, e: usize, filter: &str, keep: &mut HashSet<usize>, open: &mut HashSet<usize>) -> bool {
    let sc = d.tile().tscene();
    if label_of(sc, e).to_lowercase().contains(filter) || sc.type_of(e).to_lowercase().contains(filter) {
        let mut stack = vec![e];
        while let Some(x) = stack.pop() {
            keep.insert(x);
            if opens(sc, x) {
                stack.extend(d.tile().tree_children(x));
            }
        }

        open.insert(e);
        return true;
    }

    let mut any = false;
    if opens(sc, e) {
        for k in d.tile().tree_children(e) {
            any |= mark_matches(d, k, filter, keep, open);
        }
    }

    if any {
        keep.insert(e);
        open.insert(e);
    }

    any
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

    // A tile just opened: the Scene tree in front, to edit it (a few frames: a dock layout being applied takes the focus back).
    if tool.scene_focus_frames > 0 {
        tool.scene_focus_frames -= 1;
        gui::set_next_window_focus();
    }

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
    if d.tile().tscene().scene_graph().is_none() {
        gui::text_disabled("This tile has no SCENE_GRAPH.");
        gui::end();
        return;
    }

    // A selection made elsewhere (the viewport): open the tree down to it and scroll it into view. The tree's own clicks mark what they
    // selected as followed already.
    let first = selected(e).first().copied();
    if first != tool.tree.synced {
        tool.tree.synced = first;
        if let Some(el) = first {
            tool.reveal(d, el);
        }
    }

    let hidden_now = d.tile().display().hidden_types.iter().cloned().collect::<Vec<_>>();
    if tool.tree.doc != id
        || tool.tree.version != Some(d.tile().structure_version())
        || tool.tree.open_version_seen != Some(tool.tree.open_version)
        || tool.tree.filter != tool.tree.filter_seen
        || tool.tree.hidden_seen.as_ref() != Some(&hidden_now)
        || tool.tree.prefab_seen != tool.prefab_version
    {
        tool.rebuild_tree(d);
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
        let stage = tool.stage_group(d);
        match working {
            Some(l) if Some(l) == stage => gui::text(&format!("Working layer: the prefab ({})", d.tile().tscene().name(l))),
            Some(l) => {
                gui::text(&format!("Working layer: {}", TileTool::layer_label(d, l)));
                gui::same_line();
                if gui::small_button(&format!("{}##clearworking", wce_editor::icons::X)) {
                    tool.working_layer = None;
                }

                if gui::is_item_hovered() {
                    gui::set_tooltip(if stage.is_some() {
                        "Clear the working layer: new items go to the prefab"
                    } else {
                        "Clear the working layer: new items go to the root"
                    });
                }
            }

            None => gui::text_disabled("Working layer: the root (right click a layer to set one)"),
        }
    }

    // The name filter, and the types' filter at its right.
    let hidden_n = e.docs.find_as::<TileDocument>(id).map_or(0, |d| d.tile().display().hidden_types.len());
    let types_label = if hidden_n == 0 { icons::FUNNEL.to_owned() } else { format!("{} {hidden_n}", icons::FUNNEL) };
    let types_width = gui::calc_text_width(&types_label) + gui::style_frame_padding().x * 2.0;
    gui::set_next_item_width(-(types_width + gui::item_spacing().x));
    gui::input_text("##treefilter", Some("Filter by name or type"), &mut tool.tree.filter, 256, 0);
    gui::same_line();
    draw_type_filter(e, id, &types_label, hidden_n);

    // ---- the tree, under its header; a combined project's variations shown above it
    let all = e.docs.find_as::<TileDocument>(id).map(|d| d.tile().variations().to_vec()).unwrap_or_default();
    if !all.is_empty() {
        gui::text_disabled("Show:");
        gui::same_line();
        tool.shown_variation_checkboxes(e, true);
    }

    draw_tree_header(e, &all);
    if gui::begin_child("scenetree", v2(0.0, 0.0), gui::CHILD_BORDERS, 0) {
        draw_tree(tool, e, id);
    }

    gui::end_child();

    // A layer created by a button or a menu: its name goes into edit mode as soon as the selection has caught up with the hint.
    if tool.tree.begin_rename_next {
        let sel = selected(e);
        let d = e.docs.find_as::<TileDocument>(id).expect("a tile");
        let sc = d.tile().tscene();
        if sel.len() == 1 && sc.is_layer(sel[0]) && sc.type_of(sel[0]) != "SCENE_GRAPH" {
            tool.begin_rename(d, sel[0]);

            // Open its ancestors so the row is visible.
            let mut p = parent_layer(sc, sel[0]);
            while let Some(l) = p {
                tool.tree.open.insert(tool.tree_key_of(l));
                p = parent_layer(sc, l);
            }

            tool.tree.open_version += 1;
            tool.tree.begin_rename_next = false;
        } else {
            tool.tree.rename_wait += 1;
            if tool.tree.rename_wait > 5 {
                tool.tree.begin_rename_next = false;
            }
        }
    } else {
        tool.tree.rename_wait = 0;
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

/// The types' filter: a button (lit, with the count, while some are hidden) that opens the types on the tile with their icons and counts. A
/// type turned off is neither listed nor shown in the viewport (a view setting of this tool, not a flag of the file). A right click shows
/// that type alone; a right click on the only one shown shows them all again.
fn draw_type_filter(e: &mut Editor, id: wce_scene::render_scene::DocumentId, label: &str, hidden_n: usize) {
    if hidden_n != 0 {
        let h = theme::palette().highlight;
        gui::push_style_color(gui::COL_TEXT, gui::v4(h[0], h[1], h[2], h[3]));
    }

    if gui::button(&format!("{label}##typefilter")) {
        gui::open_popup("typefilter");
    }

    if hidden_n != 0 {
        gui::pop_style_color(1);
    }

    if gui::is_item_hovered() {
        gui::set_tooltip(&if hidden_n == 0 {
            "Types shown: all".to_owned()
        } else {
            format!("{hidden_n} type(s) hidden: click to choose")
        });
    }

    if !gui::begin_popup("typefilter") {
        return;
    }

    let Some(d) = e.docs.find_as_mut::<TileDocument>(id) else {
        gui::end_popup();
        return;
    };

    let counts = d.tile().type_counts().clone();
    let hidden = d.tile().display().hidden_types.clone();
    let mut change: Option<Vec<(String, bool)>> = None;
    if gui::small_button("All") {
        change = Some(counts.keys().map(|t| (t.clone(), true)).collect());
    }

    if gui::is_item_hovered() {
        gui::set_tooltip("Shows every type in the tree and the viewport.");
    }

    gui::same_line();
    if gui::small_button("None") {
        change = Some(counts.keys().map(|t| (t.clone(), false)).collect());
    }

    if gui::is_item_hovered() {
        gui::set_tooltip("Hides every type from the tree and the viewport.");
    }

    gui::same_line();
    gui::text_disabled("right click: only that one");
    gui::separator();
    for (ty, n) in &counts {
        let mut on = !hidden.contains(ty);
        if gui::checkbox(&format!("{}  {}  ({n})##ty{ty}", type_icon(ty), crate::entity_types::type_name(ty)), &mut on) {
            change = Some(vec![(ty.clone(), on)]);
        }

        if gui::is_item_clicked_with(1) {
            let only = counts.keys().all(|t| t == ty || hidden.contains(t)) && !hidden.contains(ty);
            change = Some(counts.keys().map(|t| (t.clone(), only || t == ty)).collect());
        }

        if gui::is_item_hovered() {
            gui::set_tooltip(ty);
        }
    }

    gui::end_popup();
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

/// A layer's export types (TEd's "Export" property), the current one ticked; the escalation ones when the game has them.
fn export_menu(sc: &TedScene, layer: usize, escalation: bool, actions: &mut Vec<RowAction>) {
    let now = own_export(sc, layer);
    for (kind, label) in EXPORT_TYPES.into_iter().filter(|(kind, _)| escalation || !kind.starts_with("Escalation") || now == *kind) {
        if gui::menu_item(&format!("{}  {label}", export_icon(kind)), None, now == kind, true) {
            actions.push(RowAction::ExportType(layer, format!("LayerExportType_{kind}")));
        }
    }
}

/// A prefab action asked from a row's menu.
enum PrefabAction {
    OpenInTool(usize),
    Unpack(usize),
    SaveForKit(usize),
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
    SetVariations(Vec<usize>, Option<Vec<String>>),
    VariationFolder(Vec<usize>, Vec<String>),
    SplitVariation(Vec<usize>, String),
    CommitRename(usize, String),
    ToggleOpen(String, bool),
}

fn draw_tree(tool: &mut TileTool, e: &mut Editor, id: wce_scene::render_scene::DocumentId) {
    let row_h = gui::text_line_height_with_spacing() + 2.0;
    let dl = gui::window_draw_list();
    let window_hovered = gui::is_window_hovered_with_children();
    let sel_all = selected(e);
    let escalation = e.game().features.escalation;
    let mut actions = vec![];
    let mut pending_drop: Option<(Vec<usize>, usize, Option<usize>)> = None;
    let mut activate = false;
    if let Some(target) = tool.tree.scroll_to.take()
        && let Some(i) = tool.tree.rows.iter().position(|r| r.elem == target)
    {
        let (y, h) = (i as f32 * row_h, gui::window_height());
        if y < gui::scroll_y() || y + row_h > gui::scroll_y() + h {
            gui::set_scroll_y((y - (h - row_h) * 0.5).max(0.0));
        }
    }

    // A prefab is edited in the Prefab tool (not from its own stage).
    let prefab_tool = tool.profile == crate::tool::Profile::Tile && e.with_tool::<crate::prefab_tool::PrefabTool, _>(|_, _| ()).is_some();

    {
        let d = e.docs.find_as::<TileDocument>(id).expect("a tile");

        // The override marks are found again after an edit (or a new library).
        let marks_key = (id, d.history().version(), tool.prefab_version);
        if tool.override_rows.0 != marks_key {
            tool.override_rows = (marks_key, HashMap::new());
        }

        let working = tool.working_layer_of(d);
        let stage = tool.stage_group(d);
        let in_stage = |l: usize| stage.is_none_or(|g| l == g || d.tile().tscene().is_descendant(l, g));
        let can_work = |l: usize| TileTool::can_work_in(stage, d.tile().tscene(), l);
        let lib = tool.prefabs.clone();
        let prefab_of =
            |el: usize| -> Option<String> { d.prefab_key(el).and_then(|k| lib.as_ref().and_then(|l| l.get(&k).map(|p| p.key.clone()))) };
        let t = d.tile();
        let sc = t.tscene();
        let all = t.variations().to_vec();
        let var_w = variations_width(e, &all);
        let right = ICON_COLUMNS * ICON_WIDTH + var_w;
        let rows = tool.tree.rows.clone();
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
            // A selectable's highlight reaches half the item spacing past it at the top and the bottom (to meet the next one in a
            // list): made that much shorter and moved down by half of it, it fills its own row exactly (it overlapped the rows around).
            let spacing = gui::item_spacing().y;
            gui::set_cursor_screen_pos(v2(p0.x, p0.y + spacing * 0.5));
            gui::set_next_item_allow_overlap();
            let clicked =
                gui::selectable_sized("##row", is_sel, gui::SELECTABLE_ALLOW_DOUBLE_CLICK, v2(avail - right, (row_h - spacing).max(1.0)));
            let row_hovered = gui::is_item_hovered();
            let (rmin, rmax) = (p0, v2(p0.x + avail - right, p0.y + row_h));
            if clicked && tool_ref.tree.rename_elem != Some(el) {
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
                tool_ref.tree.picked = true;
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
                    let mut zone = if row.branch {
                        if y < 0.25 {
                            Zone::Before
                        } else if y > 0.75 {
                            Zone::After
                        } else {
                            Zone::Into
                        }
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
                        Zone::Before => (parent_container(sc, el), Some(el)),
                        Zone::After => {
                            // An open layer: "after" it means its first child position, which is "into" at the front.
                            if row.branch && row.open && row.kids != 0 {
                                (Some(el), sc.children(el).first().copied())
                            } else {
                                // In front of the next sibling.
                                let layer = parent_container(sc, el);
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

                    // On a prefab's stage, nothing goes outside the prefab.
                    let layer = layer.filter(|&l| in_stage(l));
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

                if !sel.is_empty() && gui::menu_item("Group", wce_editor::shortcuts::label("tile.group").as_deref(), false, true) {
                    actions.push(RowAction::Group(sel.clone()));
                }

                let has_group = sel.iter().any(|&s| matches!(sc.type_of(s), "UserInstanceGroup" | "INSTANCE_GROUP"));
                if has_group && gui::menu_item("Ungroup", wce_editor::shortcuts::label("tile.ungroup").as_deref(), false, true) {
                    actions.push(RowAction::Ungroup(sel.clone()));
                }

                if !sel.is_empty() {
                    if gui::menu_item("Group into a new layer", None, false, true) {
                        actions.push(RowAction::GroupIntoLayer(sel.clone()));
                    }

                    if gui::is_item_hovered() {
                        gui::set_tooltip("Moves the selected items into a new layer, then name it.");
                    }
                }

                // Prefabs: a copy is edited, unpacked; a plain instance group is saved as one.
                match prefab_of(el) {
                    Some(_) => {
                        if prefab_tool && gui::menu_item("Open in Prefab tool", None, false, true) {
                            actions.push(RowAction::Prefab(PrefabAction::OpenInTool(el)));
                        }

                        if gui::menu_item("Unpack prefab", None, false, true) {
                            actions.push(RowAction::Prefab(PrefabAction::Unpack(el)));
                        }

                        if gui::is_item_hovered() {
                            gui::set_tooltip("Makes this copy of the prefab a plain group, no longer tied to the prefab.");
                        }

                        if gui::menu_item("Save prefab for Assembly Kit", None, false, true) {
                            actions.push(RowAction::Prefab(PrefabAction::SaveForKit(el)));
                        }

                        if gui::is_item_hovered() {
                            gui::set_tooltip("Writes the prefab as TEd reads it (no ground) into an Assembly Kit's art/prefabs.");
                        }
                    }

                    None if sc.type_of(el) == "UserInstanceGroup" => {
                        if gui::menu_item("Save as prefab...", None, false, true) {
                            actions.push(RowAction::Prefab(PrefabAction::Save(el)));
                        }

                        if gui::is_item_hovered() {
                            gui::set_tooltip("Writes the group as a new prefab; the group becomes a reference to it.");
                        }
                    }

                    None => {}
                }

                if can_work(el) {
                    if Some(el) == working {
                        if Some(el) != stage && gui::menu_item("Clear the working layer", None, false, true) {
                            actions.push(RowAction::SetWorking(None));
                        }
                    } else {
                        if gui::menu_item("Set as the working layer", None, false, true) {
                            actions.push(RowAction::SetWorking(Some(el)));
                        }

                        if gui::is_item_hovered() {
                            gui::set_tooltip("Makes it the working layer: new items go into it.");
                        }
                    }
                }

                // A combined project's: the selection into a folder of some variations, or split for one.
                if !all.is_empty() && !sel.is_empty() && !is_graph {
                    if gui::begin_menu("Move to a variation folder") {
                        for v in &all {
                            if gui::menu_item(&format!("Only in {v}"), None, false, true) {
                                actions.push(RowAction::VariationFolder(sel.clone(), vec![v.clone()]));
                            }
                        }

                        if all.len() > 2 {
                            gui::separator();
                            for v in &all {
                                if gui::menu_item(&format!("In all but {v}"), None, false, true) {
                                    let others = all.iter().filter(|x| *x != v).cloned().collect::<Vec<_>>();
                                    actions.push(RowAction::VariationFolder(sel.clone(), others));
                                }
                            }
                        }

                        gui::end_menu();
                    }

                    if gui::begin_menu("Split for a variation") {
                        for v in &all {
                            if gui::menu_item(v, None, false, true) {
                                actions.push(RowAction::SplitVariation(sel.clone(), v.clone()));
                            }

                            if gui::is_item_hovered() {
                                gui::set_tooltip(&format!(
                                    "Copies the selection into a folder for {v} and puts the originals in one for the other variations:\nwhat you change in the copies then is {v}'s only."
                                ));
                            }
                        }

                        gui::end_menu();
                    }
                }

                // Into a layer, or a holder (`wce_tile::parents`): what goes only inside a holder goes into one alone.
                let holder = wce_tile::parents::child_kind(sc.type_of(el)).is_some();
                let fits = sel.iter().all(|&s| wce_tile::parents::fits(sc.type_of(s), sc.type_of(el)).is_ok());
                if (row.layer || holder) && in_stage(el) && !sel.is_empty() && !sel.contains(&el) {
                    if gui::menu_item("Move selection here", None, false, fits) {
                        actions.push(RowAction::MoveInto(sel.clone(), el));
                    }

                    if gui::is_item_hovered_allow_disabled() {
                        gui::set_tooltip(if fits {
                            "Moves the selected items into this one."
                        } else {
                            "Moves the selected items into this one; some of them can't go in it."
                        });
                    }
                }

                gui::separator();
                if !is_graph {
                    if gui::menu_item("Rename", wce_editor::shortcuts::label("tile.rename").as_deref(), false, true) {
                        actions.push(RowAction::Rename(el));
                    }

                    if row.layer && gui::menu_item("Duplicate", None, false, true) {
                        actions.push(RowAction::DuplicateLayer(el));
                    }

                    if gui::menu_item("Delete", wce_editor::shortcuts::label("tile.delete").as_deref(), false, true) {
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
                    export_menu(sc, el, escalation, &mut actions);
                    gui::end_menu();
                }

                gui::end_popup();
            }

            // What is drawn on the row.
            let cy = p0.y + row_h * 0.5;
            let x0 = p0.x + 4.0 + row.depth as f32 * 14.0;
            if row.branch && row.kids != 0 {
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

            // In a prefab reference: an item that overrides the prefab's values (in the warning colour, "override"), and the reference or a
            // group in it holding some (how many).
            let reference = if d.is_prefab_reference(el) { Some(el) } else { d.reference_above(el).map(|(r, _)| r) };
            let (overridden, holds) = match reference {
                Some(r) => {
                    let items = tool_ref.override_rows.1.entry(r).or_insert_with(|| d.overridden_items(r).into_iter().collect());
                    let els = sc.doc().elements();
                    let below = items
                        .iter()
                        .filter(|&&i| {
                            i != el
                                && std::iter::successors(els.get(i).and_then(|x| x.parent), |&p| els.get(p).and_then(|x| x.parent))
                                    .any(|p| p == el)
                        })
                        .count();
                    (items.contains(&el), below)
                }

                None => (false, 0),
            };

            let text_col = if overridden {
                theme::u32(theme::warning())
            } else if !vis {
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

            if tool_ref.tree.rename_elem == Some(el) {
                gui::set_cursor_screen_pos(v2(tx, p0.y));
                gui::set_next_item_width((avail - right - (tx - p0.x) - 4.0).max(60.0));
                if tool_ref.tree.rename_focus {
                    gui::set_keyboard_focus_here(0);
                    tool_ref.tree.rename_focus = false;
                    tool_ref.tree.rename_active_seen = false;
                }

                let enter = gui::input_text(
                    "##rename",
                    None,
                    &mut tool_ref.tree.rename_buf,
                    128,
                    gui::INPUT_ENTER_RETURNS_TRUE | gui::INPUT_AUTO_SELECT_ALL,
                );
                if gui::is_item_active() {
                    tool_ref.tree.rename_active_seen = true;
                }

                if enter {
                    actions.push(RowAction::CommitRename(el, tool_ref.tree.rename_buf.clone()));
                    tool_ref.tree.rename_elem = None;
                } else if gui::is_key_pressed(gui::KEY_ESCAPE, true) || (tool_ref.tree.rename_active_seen && !gui::is_item_active()) {
                    tool_ref.tree.rename_elem = None;
                }
            } else {
                let mut label = label_of(sc, el);
                if row.branch && row.kids != 0 {
                    label += &format!("  ({})", row.kids);
                }

                if overridden {
                    label = format!("{} {label}  (override)", wce_editor::icons::TRIANGLE_ALERT);
                }

                dl.push_clip_rect(v2(tx, p0.y), v2(p0.x + avail - right - 2.0, p0.y + row_h));
                let ty = p0.y + (row_h - gui::text_line_height()) * 0.5;
                dl.text(v2(tx, ty), text_col, &label);
                if holds != 0 {
                    let w = gui::calc_text_size(&label).x;
                    dl.text(v2(tx + w + 8.0, ty), theme::u32(theme::warning()), &format!("{} {holds}", wce_editor::icons::TRIANGLE_ALERT));
                }

                dl.pop_clip_rect();
            }

            // The type (a prefab copy's is Prefab) and where it is exported.
            // A combined project's variations: the element's (its own bright, inherited dimmed); a click on a layer's, a group's or a
            // folder's changes them.
            if !all.is_empty() {
                let x0 = p0.x + avail - 4.0 * ICON_WIDTH - var_w;
                let doc = sc.doc();
                let set = wce_tile::variations::effective(doc, el, &all);
                let own = wce_tile::variations::own_variations(doc, el).is_some();
                let carries = wce_tile::variations::edit::can_carry(sc, el);
                gui::set_cursor_screen_pos(v2(x0, p0.y));
                if gui::invisible_button("##vars", v2(var_w, row_h), gui::BUTTON_LEFT) && carries {
                    gui::open_popup("varmenu");
                }

                let text = if set.len() == all.len() { "all".to_owned() } else { set.join(", ") };
                if gui::is_item_hovered() {
                    let whose = if own { "its own" } else { "as its parent's" };
                    let click = if carries { "\nClick to change them" } else { "\nAn instance belongs where its folder does" };
                    gui::set_tooltip(&format!("Variations: {} ({whose}){click}", set.join(", ")));
                }

                let col = if own { theme::text() } else { theme::mix(theme::text(), theme::palette_background(), 0.5) };
                dl.push_clip_rect(v2(x0 + 2.0, p0.y), v2(x0 + var_w - 4.0, p0.y + row_h));
                dl.text(v2(x0 + 4.0, cy - gui::text_line_height() * 0.5), theme::u32(col), &text);
                dl.pop_clip_rect();
                if gui::begin_popup("varmenu") {
                    let targets = if is_sel { sel_all.clone() } else { vec![el] };
                    variations_menu(&all, &set, own, &targets, &mut actions);
                    gui::end_popup();
                }
            }

            let ty = sc.type_of(el);
            let prefab = prefab_of(el);
            gui::set_cursor_screen_pos(v2(p0.x + avail - 4.0 * ICON_WIDTH, p0.y));
            gui::invisible_button("##type", v2(ICON_WIDTH, row_h), gui::BUTTON_LEFT);
            if gui::is_item_hovered() {
                match &prefab {
                    Some(k) => gui::set_tooltip(&format!("Prefab {k}")),
                    None => gui::set_tooltip(ty),
                }
            }

            let (icon, icol) = match prefab {
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
                export_menu(sc, el, escalation, &mut actions);
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

    // A click on a row: the tree does not scroll to what it selected itself.
    if activate {
        tool.leave_for_selection(e);
        tool.tree.synced = selected(e).first().copied();
    }

    // Keys while the pointer is over the tree.
    if window_hovered && !gui::io().WantTextInput {
        let sel = selected(e);
        let d = e.docs.find_as::<TileDocument>(id).expect("a tile");
        if wce_editor::shortcuts::pressed("tile.rename", false) && sel.len() == 1 && d.tile().tscene().type_of(sel[0]) != "SCENE_GRAPH" {
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
                    tool.tree.open.insert(key);
                    tool.tree.open_version += 1;
                    tool.tree.begin_rename_next = true;
                    tool.status = "layer added".to_owned();
                }

                Err(err) => tool.status = err,
            },

            RowAction::GroupIntoLayer(sel) => match d.group_items(&sel, "New layer") {
                Ok(_) => {
                    tool.status = "grouped into a new layer".to_owned();
                    tool.tree.begin_rename_next = true;
                }

                Err(err) => tool.status = err,
            },

            RowAction::Group(sel) => {
                let into = tool.new_items_layer(d);
                tool.status = into
                    .ok_or_else(|| "this tile has no scene graph".to_owned())
                    .and_then(|into| d.group_instances(&sel, into, ""))
                    .map_or_else(|err| err, |_| "grouped".to_owned());
            }

            RowAction::Ungroup(sel) => {
                let into = tool.new_items_layer(d);
                tool.status = into
                    .ok_or_else(|| "this tile has no scene graph".to_owned())
                    .and_then(|into| d.ungroup(&sel, into))
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

            RowAction::SetVariations(targets, list) => {
                tool.status = d.set_variations(&targets, list.as_deref()).map_or_else(|err| err, |()| "variations set".to_owned());
            }

            RowAction::VariationFolder(sel, list) => {
                tool.status =
                    d.to_variation_folder(&sel, &list).map_or_else(|err| err, |_| format!("moved into a folder for {}", list.join(", ")));
            }

            RowAction::SplitVariation(sel, v) => {
                tool.status =
                    d.split_for_variation(&sel, &v).map_or_else(|err| err, |c| format!("split for {v}: {} copies, selected", c.len()));
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
                    tool.tree.open.remove(&key);
                } else {
                    tool.tree.open.insert(key);
                }

                tool.tree.open_version += 1;
            }
        }
    }

    // The prefab actions need the editor (the library, the selection).
    for p in prefab_actions {
        let result = match p {
            PrefabAction::OpenInTool(el) => tool.open_in_prefab_tool(e, el),
            PrefabAction::Unpack(el) => tool.unpack_prefab(e, el),
            PrefabAction::SaveForKit(el) => {
                let key = e.docs.find_as::<TileDocument>(id).and_then(|d| tool.prefab_of(d, el));
                match key {
                    Some(k) => tool.save_prefab_for_kit_ask(e, &k),
                    None => Err("not a copy of a prefab".to_owned()),
                }
            }
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
