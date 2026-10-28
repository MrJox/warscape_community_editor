//! Editing the tile's links (Phase 6a of docs/plans/tile_ted_parity.md): the database entry's link targets (what the tile offers at an
//! edge cell) and rules (what a neighbour's cell must or must not be), as the Tile panel's Tile links list and an editor of the picked one.
//! Each change is one undo entry (`TileDocument::set_links`; typing a number is one).

use wce_editor::editor::Editor;
use wce_editor::{gui, icons};
use wce_formats::tile_entry::{TileLink, TileLinkTarget};
use wce_editor_core::document::Document;
use wce_scene::render_scene::DocumentId;
use wce_tile_editor::tile_document::TileDocument;

use crate::tool::TileTool;

/// What a field of a link does (its tooltip).
const TARGET_SET_TIP: &str = "The kind of join this tile offers at that cell of its edge (a river, a road, the sea...): a neighbour whose rule asks for it there can be put next to it.";
const RULE_SET_TIP: &str = "The kind of tile (by its link set) the neighbour's cell must be, or must not be.";
const CELL_TIP: &str = "The cell, 128 m: x along +x from 0, y from the +z edge. A rule names a neighbour's cell: x = -1 or the width, y = -1 or the height is just past an edge.";
const TEST_TIP: &str = "Must be: a tile can only be put here if the neighbour's cell is of that link set (a map cell with no neighbour yet passes). Must not be: it is refused when it is.";
const ENTRY_TIP: &str = "For rivers: the game takes the flow of a river tile from the is_entry of its river link on the side it was reached from (the river's flip).";
const BLEND_TIP: &str = "How many pixels of the neighbour's terrain are blended into this tile at that edge (the kit uses 64 mostly, 16, 24, 128).";
const OFFLINE_TIP: &str = "Turns off the export's pre-blend at that edge (from the name; not tested).";
const BASE_TIP: &str = "The cell of this tile the rule belongs to (the kit mostly has 1, 0; what the game does with it is not known).";

impl TileTool {
    /// The link set names offered (the kit's tile sets, read once per kit), with `current` in them.
    fn link_set_names(&mut self, d: &TileDocument, current: &str) -> Vec<String> {
        let root = d.project().entry.as_ref().and_then(|en| en.raw_root()).unwrap_or_default();
        if self.link_sets.as_ref().is_none_or(|(r, _)| *r != root) {
            self.link_sets = Some((root.clone(), wce_tile::link_checks::kit_tile_sets(&root)));
        }

        let mut names = self.link_sets.as_ref().map(|(_, n)| n.clone()).unwrap_or_default();
        if !current.is_empty() && !names.iter().any(|n| n == current) {
            names.insert(0, current.to_owned());
        }

        names
    }

    /// The links list (a click picks one and looks at its cell) and the editor of the picked one, with adding and removing.
    pub(crate) fn draw_link_editor(&mut self, e: &mut Editor, id: DocumentId) -> Option<(i32, i32)> {
        let d = e.docs.find_as_mut::<TileDocument>(id)?;
        let entry = d.project().entry.as_ref()?;
        let (mut targets, mut links) = (entry.link_targets(), entry.links());
        let width = entry.width() as i32;
        let height = entry.height() as i32;
        let mut look = None;
        let mut changed: Option<&str> = None;
        let mut gesture = 0u64;

        // The list.
        for (i, t) in targets.iter().enumerate() {
            let on = self.link_pick == Some((true, i));
            if gui::selectable(&format!("{}  target: {} at ({}, {})##lt{i}", icons::FLAG, t.tile_set, t.x, t.y), on, 0) {
                self.link_pick = Some((true, i));
                look = Some((t.x, t.y));
            }
        }

        for (i, l) in links.iter().enumerate() {
            let on = self.link_pick == Some((false, i));
            let test = if l.test == "TLT_NOT_EQUALS" { "must not be" } else { "must be" };
            if gui::selectable(&format!("{}  ({}, {}) {test} {}##lr{i}", icons::ROUTE, l.x, l.y, l.link_set), on, 0) {
                self.link_pick = Some((false, i));
                look = Some((l.x, l.y));
            }
        }

        if gui::small_button(&format!("{}  Add a target", icons::PLUS)) {
            targets.push(TileLinkTarget { elem: 0, serialise_version: 1, tile_set: entry.tile_set(), x: 0, y: 0 });
            self.link_pick = Some((true, targets.len() - 1));
            changed = Some("Add a link target");
        }

        if gui::is_item_hovered() {
            gui::set_tooltip(TARGET_SET_TIP);
        }

        gui::same_line();
        if gui::small_button(&format!("{}  Add a rule", icons::PLUS)) {
            links.push(TileLink {
                elem: 0, serialise_version: 1, link_set: entry.tile_set(), x: -1, y: 0, base_x: 1, base_y: 0, is_entry: false, blend_size: 64,
                no_offline_blend: false, test: "TLT_EQUALS".to_owned(), has_blend_quad: true,
            });
            self.link_pick = Some((false, links.len() - 1));
            changed = Some("Add a link rule");
        }

        if gui::is_item_hovered() {
            gui::set_tooltip("A condition on a neighbour's cell: what it must (or must not) be for this tile to be put next to it.");
        }

        // The picked one.
        let pick = self.link_pick.filter(|&(t, i)| if t { i < targets.len() } else { i < links.len() });
        if let Some((is_target, i)) = pick {
            gui::separator();
            let int_field = |label: &str, v: &mut i32, tip: &str, g: &mut u64, key: u64| -> bool {
                gui::set_next_item_width(110.0);
                let c = gui::input_int(label, v);
                if gui::is_item_hovered() {
                    gui::set_tooltip(tip);
                }

                if c {
                    *g = key;
                }

                c
            };

            if is_target {
                let t = &mut targets[i];
                let names = self.link_set_names(d, &t.tile_set);
                if let Some(n) = set_combo("tile set##lt", &t.tile_set, &names, TARGET_SET_TIP) {
                    t.tile_set = n;
                    changed = Some("Change a link target");
                }

                if int_field("x##ltx", &mut t.x, CELL_TIP, &mut gesture, 0x10) | int_field("y##lty", &mut t.y, CELL_TIP, &mut gesture, 0x11) {
                    changed = Some("Move a link target");
                }

                if gui::small_button(&format!("{}  Remove the target", icons::TRASH_2)) {
                    targets.remove(i);
                    self.link_pick = None;
                    changed = Some("Remove a link target");
                }
            } else {
                let l = &mut links[i];
                let names = self.link_set_names(d, &l.link_set);
                if let Some(n) = set_combo("link set##lr", &l.link_set, &names, RULE_SET_TIP) {
                    l.link_set = n;
                    changed = Some("Change a link rule");
                }

                let mut test = i32::from(l.test == "TLT_NOT_EQUALS");
                gui::set_next_item_width(140.0);
                if gui::combo("test##lr", &mut test, &["must be", "must not be"]) {
                    l.test = if test == 1 { "TLT_NOT_EQUALS" } else { "TLT_EQUALS" }.to_owned();
                    changed = Some("Change a link rule");
                }

                if gui::is_item_hovered() {
                    gui::set_tooltip(TEST_TIP);
                }

                if int_field("x##lrx", &mut l.x, CELL_TIP, &mut gesture, 0x20) | int_field("y##lry", &mut l.y, CELL_TIP, &mut gesture, 0x21) {
                    changed = Some("Move a link rule");
                }

                let mut blend = l.blend_size as i32;
                if int_field("blend size (px)##lrb", &mut blend, BLEND_TIP, &mut gesture, 0x22) {
                    l.blend_size = blend.clamp(0, 4096) as u32;
                    changed = Some("Change a link rule");
                }

                if gui::checkbox("is entry##lre", &mut l.is_entry) {
                    changed = Some("Change a link rule");
                }

                if gui::is_item_hovered() {
                    gui::set_tooltip(ENTRY_TIP);
                }

                gui::same_line();
                if gui::checkbox("no offline blend##lro", &mut l.no_offline_blend) {
                    changed = Some("Change a link rule");
                }

                if gui::is_item_hovered() {
                    gui::set_tooltip(OFFLINE_TIP);
                }

                if int_field("base x##lrbx", &mut l.base_x, BASE_TIP, &mut gesture, 0x23) | int_field("base y##lrby", &mut l.base_y, BASE_TIP, &mut gesture, 0x24) {
                    changed = Some("Change a link rule");
                }

                if gui::small_button(&format!("{}  Remove the rule", icons::TRASH_2)) {
                    links.remove(i);
                    self.link_pick = None;
                    changed = Some("Remove a link rule");
                }
            }

            gui::text_disabled(&format!("the tile is {width} x {height} cells"));
        }

        if let Some(name) = changed {
            let g = if gesture != 0 { gesture ^ (u64::from(id) << 8) } else { 0 };
            if let Err(err) = d.set_links(name, targets, links, g) {
                self.status = err;
            }

            self.link_typing |= gesture != 0;
        }

        // A number typed is one undo entry: the merging ends when the field is left.
        if self.link_typing && !gui::is_any_item_active() {
            self.link_typing = false;
            d.history_mut().break_merge();
        }

        look
    }
}

/// A combo of link set names; the picked one when it changed.
fn set_combo(label: &str, current: &str, names: &[String], tip: &str) -> Option<String> {
    let mut out = None;
    gui::set_next_item_width(200.0);
    if gui::begin_combo(label, current) {
        for n in names {
            if gui::selectable(n, n == current, 0) && n != current {
                out = Some(n.clone());
            }
        }

        gui::end_combo();
    }

    if gui::is_item_hovered() {
        gui::set_tooltip(tip);
    }

    out
}
