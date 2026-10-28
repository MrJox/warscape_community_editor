//! The building technology chip (docs/editor.md): beside the ground views' chip at the top right of the viewport, a popup that picks what of
//! the buildings' logic files (`*_tech.cs2.parsed`) and `.meta` files is drawn over them: collision hulls, hard outlines, platforms, pipes,
//! formation lines, emitters, attached buildings, bounding boxes, and the buildings whose footprints overlap (wce_tile_editor's `tech.rs`).
//! Nothing is drawn until something is ticked, so the viewport stays clean; the settings last for the session (they are a view setting of
//! the tile, like the polygons', not kept in the settings file).

use wce_editor::editor::Editor;
use wce_editor::{gui, icons};
use wce_tile_editor::tile_document::{GROUPS, KINDS, TechKind, TechScope, TechSettings, TileDocument};

use crate::ground_views::chip;
use crate::tool::{TileTool, document_id};

const SCOPE_TIP: &str = "All buildings: every building within the range of the camera (the nearest first; a limit keeps a whole city from being drawn).\n\
Selected: only the selected buildings, or the buildings in a selected layer or group, however far.";

const XRAY_TIP: &str = "Draw the overlays over the buildings' models, as the collision hulls and the platforms are inside them. Off: what is behind a model is hidden by it.";

const HIGHEST_TIP: &str = "TEd's force_show_highest_destruct_level: draw the technology of every piece's most destroyed state (the rubble, the breach) instead of the \
state the building shows. The models are not changed.";

const DETECT_TIP: &str = "TEd's detect_building_collisions: buildings whose footprints (their hard outlines, else their boxes) overlap are outlined in red with a pole \
over them, and a line joins each pair. Footprints that only touch do not count. With a selection, what stands against it is tested too.";

impl TileTool {
    /// The building technology chip at the top right of the viewport, left of the ground views' (`top` down from the edge); its popup.
    pub(crate) fn draw_tech_chip(&mut self, e: &mut Editor, origin: gui::Vec2, size: gui::Vec2, top: f32) {
        let Some(id) = document_id(e) else { return };
        let d = e.docs.find_as::<TileDocument>(id).expect("a tile");
        let (s, stats) = (d.tile().display().tech.clone(), d.tile().tech_stats());
        let kinds = KINDS.iter().filter(|k| s.on(**k)).map(|k| k.info().name.to_lowercase()).collect::<Vec<_>>();
        let mut tip = if s.any() {
            let mut what = kinds.join(", ");
            if s.detect {
                what.push_str(if what.is_empty() { "overlapping buildings" } else { ", overlapping buildings" });
            }

            format!("Building technology: {what}\n{} building(s), {} line(s)", stats.buildings, stats.lines)
        } else {
            "Building technology: off".to_owned()
        };

        tip.push_str("\nClick: choose what of the buildings' logic files to draw (collision, platforms, pipes, formation lines, emitters ...)");
        gui::set_cursor_screen_pos(gui::v2(origin.x + size.x - self.ground_chip_width - self.tech_chip_width - 12.0, origin.y + top));
        gui::push_style_color(gui::COL_CHILD_BG, gui::v4(0.08, 0.08, 0.09, 0.78));
        if gui::begin_child("##techchip", gui::v2(0.0, 0.0), gui::CHILD_AUTO_RESIZE, gui::WINDOW_NO_SCROLLBAR) {
            if chip(s.any(), icons::SQUARE_DASHED, "techviews", &tip) {
                gui::open_popup("techviews");
            }

            if gui::begin_popup("techviews") {
                self.draw_tech_popup(e, id);
                gui::end_popup();
            }
        }

        gui::end_child();
        let (lo, hi) = gui::item_rect();
        self.tech_chip_width = hi.x - lo.x;
        gui::pop_style_color(1);
    }

    fn draw_tech_popup(&mut self, e: &mut Editor, id: wce_scene::render_scene::DocumentId) {
        let game_view = e.viewport.game_view;
        let d = e.docs.find_as_mut::<TileDocument>(id).expect("a tile");
        let before = d.tile().display().tech.clone();
        let stats = d.tile().tech_stats();
        let mut s = before.clone();

        gui::text_disabled("Building technology");
        gui::same_line();
        if gui::small_button("All##tech") {
            s.set_all(true);
        }

        gui::same_line();
        if gui::small_button("None##tech") {
            s.set_all(false);
            s.detect = false;
        }

        if game_view {
            gui::text_disabled("The Game view hides these (View > Editor view).");
        }

        gui::separator_text("Buildings");
        if gui::radio_button("In range##tech", s.scope == TechScope::All) {
            s.scope = TechScope::All;
        }

        if gui::is_item_hovered() {
            gui::set_tooltip(SCOPE_TIP);
        }

        if s.scope == TechScope::All {
            gui::same_line();
            gui::set_next_item_width(140.0);
            gui::slider_float("##techrange", &mut s.distance, 30.0, 1000.0, "%.0f m");
        }

        if gui::radio_button("Selected##tech", s.scope == TechScope::Selected) {
            s.scope = TechScope::Selected;
        }

        if gui::is_item_hovered() {
            gui::set_tooltip(SCOPE_TIP);
        }

        gui::checkbox("Draw through the models##tech", &mut s.xray);
        if gui::is_item_hovered() {
            gui::set_tooltip(XRAY_TIP);
        }

        for (g, group) in GROUPS.iter().enumerate() {
            gui::separator_text(group);
            for kind in KINDS.iter().filter(|k| k.info().group == g) {
                let info = kind.info();
                let mut on = s.on(*kind);
                gui::color_swatch(&format!("##swatch{}", info.name), [info.rgb[0], info.rgb[1], info.rgb[2], 1.0], gui::v2(10.0, 10.0));
                gui::same_line();
                if gui::checkbox(&format!("{}##tech", info.name), &mut on) {
                    s.set(*kind, on);
                }

                if gui::is_item_hovered() {
                    gui::set_tooltip(info.tip);
                }
            }
        }

        if s.on(TechKind::Platforms) {
            gui::separator();
            gui::set_next_item_width(140.0);
            gui::slider_float("platform fill##tech", &mut s.fill, 0.0, 1.0, "%.2f");
        }

        gui::separator_text("Checks");
        gui::checkbox("Overlapping buildings##tech", &mut s.detect);
        if gui::is_item_hovered() {
            gui::set_tooltip(DETECT_TIP);
        }

        if s.detect {
            gui::same_line();
            gui::text_disabled(&format!("{} pair(s)", stats.collisions));
        }

        gui::checkbox("Most destroyed state##tech", &mut s.highest);
        if gui::is_item_hovered() {
            gui::set_tooltip(HIGHEST_TIP);
        }

        if s.any() {
            gui::separator();
            gui::text_disabled(&format!("{} building(s), {} line(s)", stats.buildings, stats.lines));
            if stats.capped {
                gui::text_disabled("Limit reached: the farthest buildings are left out.");
            }
        }

        if s != before {
            apply(e, id, s);
        }
    }
}

/// Puts the settings in the tile and has the overlay drawn again.
fn apply(e: &mut Editor, id: wce_scene::render_scene::DocumentId, s: TechSettings) {
    let d = e.docs.find_as_mut::<TileDocument>(id).expect("a tile");
    d.tile_mut().display_mut().tech = s;
    d.tile_mut().tech_changed();
}
