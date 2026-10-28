//! Editing the tile's links (Phase 6a of docs/plans/tile_ted_parity.md): the database entry's link targets (what the tile offers at an
//! edge cell) and rules (what a neighbour's cell must or must not be), as the Tile panel's Tile links list and an editor of the picked one.
//! Each change is one undo entry (`TileDocument::set_links`; typing a number is one).

use wce_editor::editor::Editor;
use wce_editor::{gui, icons};
use wce_formats::tile_entry::{TileLink, TileLinkTarget};
use wce_editor_core::document::Document;
use wce_scene::render_scene::DocumentId;
use wce_tile::battle_map::context::{MapPlacement, PlacedMap};
use wce_tile::battle_map::link_candidates::{Candidate, candidates};
use wce_tile::battle_map::tile_db::TileDatabase;
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
const BASE_TIP: &str = "Only the export's edge blend reads it: an offset in the link set's linking tile (a 1 x 1 tile, turned as it may be) from the link's cell to the cell of this tile where that tile's terrain is blended in. (1, 0): one cell over, the kit's usual; (0, 0): the link's cell. A reading of the code, not checked in game.";

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

/// Which side of the open tile a candidate of size `bw` x `bh` (turned) is on (the entry's cells: y counts from the +z edge).
fn side_of(c: &Candidate, (bw, bh): (i32, i32), w: i32, h: i32) -> &'static str {
    if c.x + bw <= 0 {
        "-x side"
    } else if c.x >= w {
        "+x side"
    } else if c.y + bh <= 0 {
        "+z side"
    } else if c.y >= h {
        "-z side"
    } else {
        "inside"
    }
}

impl TileTool {
    /// The tiles that can follow the open tile at its joins (`wce_tile::battle_map::link_candidates`), found on asking and worked out
    /// again after an edit; a click shows one next to the tile in the map context.
    pub(crate) fn draw_link_candidates(&mut self, e: &mut Editor, id: DocumentId) {
        if !gui::tree_node("Tiles that can follow") {
            return;
        }

        let Some(d) = e.docs.find_as::<TileDocument>(id) else {
            gui::tree_pop();
            return;
        };

        let (version, raw) = (d.history().version(), d.project().raw_root.clone());
        let fresh = self.link_candidates.as_ref().is_some_and(|c| c.0 == (id, version));
        if !fresh && gui::small_button("Find them") {
            match raw.as_deref().map(|r| crate::context_panel::database(&mut self.context, r)) {
                Some(Ok(db)) => {
                    let d = e.docs.find_as::<TileDocument>(id).expect("a tile");
                    if let Some(entry) = d.project().entry.as_ref() {
                        let open = db.tile_from_entry(entry);
                        let found = candidates(&db, &open);
                        self.link_candidates = Some(((id, version), open, found, db));
                    }
                }

                Some(Err(err)) => self.status = err,
                None => self.status = "the tile is not in an assembly kit".to_owned(),
            }
        }

        if gui::is_item_hovered() {
            gui::set_tooltip("Every tile of the kit's database, at every turn it may take and every place along this tile's edges, whose links and this tile's agree as BOB's fill tests them, and that carries one of this tile's joins on (a target its rule asks for). Found again after an edit.");
        }

        let Some((stamp, open, found, db)) = self.link_candidates.clone() else {
            gui::tree_pop();
            return;
        };

        if stamp.0 != id {
            gui::tree_pop();
            return;
        }

        if stamp.1 != version {
            gui::text_disabled("the links changed since: find them again");
        }

        let mut show = None;
        for (ti, t) in open.link_targets.iter().enumerate() {
            let set = t.set.map(|s| db.sets[s].name.clone()).unwrap_or_default();
            let here = found.iter().filter(|c| c.joins.contains(&ti)).collect::<Vec<_>>();
            if gui::tree_node(&format!("{set} at ({}, {}): {} placement(s)##join{ti}", t.x, t.y, here.len())) {
                for (k, c) in here.iter().enumerate() {
                    let tile = &db.tiles[c.tile];
                    let name = tile.location.rsplit(['\\', '/']).take(2).collect::<Vec<_>>().into_iter().rev().collect::<Vec<_>>().join("/");
                    let turn = match c.rotation { 0x20 => 90, 0x40 => 180, 0x80 => 270, _ => 0 };
                    let size = if matches!(c.rotation, 0x20 | 0x80) { (tile.height, tile.width) } else { (tile.width, tile.height) };
                    let label = format!("{name}, turned {turn}, on the {} at ({}, {})##cand{ti}_{k}", side_of(c, size, open.width, open.height), c.x, c.y);
                    if gui::selectable(&label, false, 0) {
                        show = Some((*c).clone());
                    }

                    if gui::is_item_hovered() {
                        gui::set_tooltip("Shows it next to this tile in the Map context (its first variation)");
                    }
                }

                gui::tree_pop();
            }
        }

        gui::tree_pop();
        if let Some(c) = show {
            self.preview_candidate(e, id, &open, &db, &c);
        }
    }

    /// Script: finds the candidates (as "Find them"); their count per join of the open tile.
    pub(crate) fn find_link_candidates(&mut self, e: &mut Editor, id: DocumentId) -> Result<Vec<usize>, String> {
        let d = e.docs.find_as::<TileDocument>(id).ok_or("no tile")?;
        let version = d.history().version();
        let raw = d.project().raw_root.clone().ok_or("the tile is not in an assembly kit")?;
        let db = crate::context_panel::database(&mut self.context, &raw)?;
        let d = e.docs.find_as::<TileDocument>(id).ok_or("no tile")?;
        let entry = d.project().entry.as_ref().ok_or("no tile database entry")?;
        let open = db.tile_from_entry(entry);
        let found = candidates(&db, &open);
        let counts = (0..open.link_targets.len()).map(|ti| found.iter().filter(|c| c.joins.contains(&ti)).count()).collect();
        self.link_candidates = Some(((id, version), open, found, db));
        Ok(counts)
    }

    /// Script: shows the `k`-th candidate of join `join` in the map context.
    pub(crate) fn show_link_candidate(&mut self, e: &mut Editor, id: DocumentId, join: usize, k: usize) -> Result<String, String> {
        let (_, open, found, db) = self.link_candidates.clone().ok_or("no candidates found yet")?;
        let c = found.iter().filter(|c| c.joins.contains(&join)).nth(k).cloned().ok_or("no such candidate")?;
        self.preview_candidate(e, id, &open, &db, &c);
        Ok(db.tiles[c.tile].location.clone())
    }

    /// Shows a candidate next to the open tile in the map context: a map of the two alone.
    fn preview_candidate(&mut self, e: &mut Editor, id: DocumentId, open: &wce_tile::battle_map::tile_db::Tile, db: &TileDatabase, c: &Candidate) {
        let Some(d) = e.docs.find_as::<TileDocument>(id) else { return };
        let info = &d.project().info;
        let t = &db.tiles[c.tile];
        let (bw, bh) = if matches!(c.rotation, 0x20 | 0x80) { (t.height, t.width) } else { (t.width, t.height) };

        // The two on a map that holds both, a cell of margin around.
        let (x0, y0) = (c.x.min(0) - 1, c.y.min(0) - 1);
        let (x1, y1) = ((c.x + bw).max(open.width) + 1, (c.y + bh).max(open.height) + 1);
        let placements = vec![
            MapPlacement { location: info.location.clone(), variation: info.variation.clone(), x: -x0, y: -y0, w: open.width, h: open.height, rotation: 0x10, flipped: false, climate: 0 },
            MapPlacement {
                location: t.location.clone(), variation: t.variations.first().map(|v| v.location.clone()).unwrap_or_default(), x: c.x - x0, y: c.y - y0, w: t.width,
                h: t.height, rotation: c.rotation, flipped: false, climate: 0,
            },
        ];
        // A flat LF 100 m up: as on an inland map, above the sea (with none the tiles sit 20 m under it, where the game never puts them).
        let level = ((100.0 + wce_tile::battle_map::context::VERTICAL_OFFSET) / wce_tile::battle_map::context::LF_HEIGHT * 65535.0).round() as u16;
        let lf = wce_tile::battle_map::context::LfField { w: 1, h: 1, values: vec![level] };
        let map = PlacedMap {
            name: format!("link candidate {}", t.location), width: (x1 - x0) as u32, height: (y1 - y0) as u32, placements, lf: Some(lf), unknown: vec![],
            climates: db.climates.iter().map(|c| c.name.clone()).collect(),
        };

        self.context.show_map(e, id, map, 0);
        self.status = format!("{} shown next to the tile in the Map context", t.location);
    }
}
