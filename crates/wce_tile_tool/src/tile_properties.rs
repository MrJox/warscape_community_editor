//! The Tile panel's tile properties, in its Tile database entry section (Phase 6b of docs/plans/tile_ted_parity.md; TEd's Tile Properties): the database entry's colour, the
//! variation's normal strength, the flags, the mask of valid cells (TEd's subtile grid; its links are edited in Tile links), and the height
//! range (read-only: every save fits it to the heights, as TEd's). Each change is one undo entry (`TileDocument::set_entry_attr`).

use wce_editor::editor::Editor;
use wce_editor::{gui, theme};
use wce_editor_core::document::Document;
use wce_scene::render_scene::DocumentId;
use wce_tile_editor::tile_document::TileDocument;

use crate::tool::TileTool;

/// The flags of the entry and what each does (their tooltips), with the kit's counts (battle, 626 entries).
const FLAGS: [(&str, &str); 4] = [
    ("random_rotatable", "The map fill may turn the tile by 90, 180 or 270 degrees. Off for tiles that must keep their orientation (the wonders, Hadrian's wall, Rome: 8 of the kit's 626)."),
    ("encampable", "An army may set up its encampment on it (the kit's 9: plains, marsh and dunes of 2 x 2 and 2 x 3 cells). The game reads it; not traced further."),
    ("scalable", "The kit's 10: the encampments, a few custom and settlement tiles. The game reads it; what it does was not traced."),
    ("requires_infield_lodding", "TEd turns it on for a new tile (583 of the kit's 626 have it). Not traced."),
];

const NORMAL_TIP: &str = "The strength of the tile's exported normal map (the export bakes tile_normal from the heights with it; TEd's slider, 0.01 .. 50; the kit: 1, 2 or 0.25). The editor's view does not use it.";
const COLOUR_TIP: &str = "The tile's colour in the battle maps' pictures (a map paints a cell with it to ask for this tile): it must differ from every other tile's.";
const MASK_TIP: &str = "The tile's valid cells (TEd's subtile grid): a click turns a cell on or off. The map fill only puts the tile where its valid cells fit, and only those count as the tile's (an irregular tile: a coast, a wonder). All on: no mask.";

impl TileTool {
    /// Inside the Tile database entry section.
    pub(crate) fn draw_tile_properties(&mut self, e: &mut Editor, id: DocumentId) {
        let Some(d) = e.docs.find_as_mut::<TileDocument>(id) else { return };
        let Some(entry) = d.project().entry.as_ref() else {
            gui::text_disabled("no tile database entry: no properties");
            return;
        };

        let (w, h) = (entry.width() as usize, entry.height() as usize);
        let num = |v: Option<String>, def: f32| v.and_then(|s| s.trim().parse::<f32>().ok()).unwrap_or(def);
        let mut colour = ["red", "green", "blue"].map(|a| num(entry.tile_attr(a), 0.0) / 255.0);
        let v = d.project().variation.unwrap_or(0);
        let mut normal = num(entry.variation_attr(v, "normal_strength"), 1.0);
        let flags = FLAGS.map(|(a, _)| entry.tile_attr(a).is_some_and(|x| x == "true"));
        let mask = entry.tile_attr("mask").unwrap_or_default();
        let info = &d.project().info;
        gui::text_disabled(&format!("heights {:.2} .. {:.2} m (fitted to the heights at every save)", info.min_height, info.min_height + info.scale));

        let mut edits: Vec<(bool, &str, String, u64)> = vec![];
        if gui::color_edit3("colour", &mut colour, 0) {
            for (a, c) in ["red", "green", "blue"].into_iter().zip(colour) {
                edits.push((false, a, format!("{:.6}", (c * 255.0).round().clamp(0.0, 255.0)), 0x11));
            }
        }

        if gui::is_item_hovered() {
            gui::set_tooltip(COLOUR_TIP);
        }

        gui::set_next_item_width(200.0);
        if gui::drag_float("normal strength", &mut normal, 0.01, 0.01, 50.0, "%.2f") {
            edits.push((true, "normal_strength", format!("{:.6}", normal.clamp(0.01, 50.0)), 0x12));
        }

        if gui::is_item_hovered() {
            gui::set_tooltip(NORMAL_TIP);
        }

        for (i, (a, tip)) in FLAGS.iter().enumerate() {
            let mut on = flags[i];
            if gui::checkbox(a, &mut on) {
                edits.push((false, a, if on { "true" } else { "false" }.to_owned(), 0));
            }

            if gui::is_item_hovered() {
                gui::set_tooltip(tip);
            }
        }

        // The mask: a cell a square, rows from the +z edge as the entry counts them.
        gui::text("valid cells");
        if gui::is_item_hovered() {
            gui::set_tooltip(MASK_TIP);
        }

        let cells = if mask.len() == w * h { mask.bytes().map(|c| c == b'1').collect::<Vec<_>>() } else { vec![true; w * h] };
        let side = (240.0 / w.max(h) as f32).clamp(4.0, 18.0);
        let origin = gui::cursor_screen_pos();
        let dl = gui::window_draw_list();
        let mut toggled = None;
        for y in 0..h {
            for x in 0..w {
                let lo = gui::v2(origin.x + x as f32 * side, origin.y + y as f32 * side);
                let hi = gui::v2(lo.x + side - 1.0, lo.y + side - 1.0);
                let on = cells[y * w + x];
                let col = if on { theme::u32(theme::palette().highlight) } else { gui::col32(60, 60, 60, 255) };
                dl.rect_filled(lo, hi, col, 0.0);
                gui::set_cursor_screen_pos(lo);
                if gui::invisible_button(&format!("##mask{x}_{y}"), gui::v2(side - 1.0, side - 1.0), 0) {
                    toggled = Some(y * w + x);
                }

                if gui::is_item_hovered() {
                    gui::set_tooltip(&format!("cell ({x}, {y}): {}", if on { "valid" } else { "not the tile's" }));
                }
            }
        }

        gui::set_cursor_screen_pos(gui::v2(origin.x, origin.y + h as f32 * side + 4.0));
        gui::dummy(gui::v2(w as f32 * side, 1.0));
        if let Some(i) = toggled {
            let mut c = cells.clone();
            c[i] = !c[i];
            let text = if c.iter().all(|&b| b) { String::new() } else { c.iter().map(|&b| if b { '1' } else { '0' }).collect() };
            edits.push((false, "mask", text, 0));
        }

        let d = e.docs.find_as_mut::<TileDocument>(id).expect("a tile");
        for (variation, attr, value, g) in edits {
            self.prop_typing |= g != 0;
            let g = if g != 0 { g ^ (u64::from(id) << 8) } else { 0 };
            if let Err(err) = d.set_entry_attr(variation, attr, &value, g) {
                self.status = err;
            }
        }

        // A drag of the colour or the strength is one undo entry: the merging ends when it is let go.
        if self.prop_typing && !gui::is_any_item_active() {
            self.prop_typing = false;
            d.history_mut().break_merge();
        }
    }
}
