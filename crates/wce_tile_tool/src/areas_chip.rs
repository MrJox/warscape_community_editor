//! The Areas chip (docs/editor.md): beside the building technology chip at the top right of the viewport, a popup with how the polygons are
//! drawn: one opacity for every outline, one for every fill, and a factor per type of area that multiplies the fill. Kept in the tool's
//! settings, so it holds for every tile and the next session; the tool hands it to the document on show.

use wce_editor::editor::Editor;
use wce_editor::{gui, icons};
use wce_tile_editor::tile_document::{FILLED_TYPES, PolygonLook, TileDocument, polygon_colour};

use crate::entity_types::{type_icon, type_name};
use crate::ground_views::{begin_chip_popup, chip};
use crate::settings::{SETTINGS_ID, TileSettings};
use crate::tool::{TileTool, document_id};

const OUTLINES_TIP: &str = "Every polygon's and line's outline. 0 hides them.";
const FILLS_TIP: &str = "Every area's fill. Multiplies the sliders below, so this dims them all and keeps their balance. 0: outlines only.";
const TYPE_TIP: &str = "This type's fill, as a part of the All fills value: 1 follows it, 0 hides this type's fill.";

impl TileTool {
    /// The look the polygons are drawn with, read from the settings once.
    pub(crate) fn polygon_look(&mut self, e: &Editor) -> &PolygonLook {
        self.areas.get_or_insert_with(|| {
            let s = e.tool_settings::<TileSettings>(SETTINGS_ID);
            PolygonLook { fill: s.polygon_fill, outline: s.polygon_outline, type_fill: s.polygon_type_fill }
        })
    }

    /// Hands the look to the tile in view when it differs, whichever tile that is: its next update writes it into the overlay colour table,
    /// so a slider's drag touches no polygon.
    pub(crate) fn sync_polygon_look(&mut self, e: &mut Editor) {
        let look = self.polygon_look(e).clone();
        let Some(id) = document_id(e) else { return };
        let Some(d) = e.docs.find_as_mut::<TileDocument>(id) else { return };
        if d.tile().display().polygon_look != look {
            d.tile_mut().display_mut().polygon_look = look;
        }
    }

    /// Keeps the look in the settings.
    pub(crate) fn save_polygon_look(&mut self, e: &mut Editor) {
        let look = self.polygon_look(e).clone();
        e.update_tool_settings(SETTINGS_ID, |s: &mut TileSettings| {
            (s.polygon_fill, s.polygon_outline) = (look.fill, look.outline);
            s.polygon_type_fill = look.type_fill;
        });
    }

    /// The Areas chip at the top right of the viewport, left of the building technology chip; its popup.
    pub(crate) fn draw_areas_chip(&mut self, e: &mut Editor, origin: gui::Vec2, size: gui::Vec2, top: f32) {
        let Some(id) = document_id(e) else { return };
        let look = self.polygon_look(e).clone();
        let shown = e.docs.find_as::<TileDocument>(id).is_some_and(|d| d.tile().display().show_polygons);
        let changed = !shown || look != PolygonLook::default();
        let tip = format!(
            "Areas: outlines {:.2}, fills {:.2}\nClick: how the polygons' outlines and fills are drawn, for all of them and per type",
            look.outline, look.fill
        );
        gui::set_cursor_screen_pos(gui::v2(
            origin.x + size.x - self.ground_chip_width - self.tech_chip_width - self.areas_chip_width - 16.0,
            origin.y + top,
        ));
        gui::push_style_color(gui::COL_CHILD_BG, gui::v4(0.08, 0.08, 0.09, 0.78));
        if gui::begin_pill("##areaschip") {
            if chip(changed, icons::LAND_PLOT, "areaviews", &tip) {
                gui::open_popup("areaviews");
            }

            if begin_chip_popup("areaviews") {
                self.draw_areas_popup(e, id);
                gui::end_popup();
            }
        }

        gui::end_child();
        let (lo, hi) = gui::item_rect();
        self.areas_chip_width = hi.x - lo.x;
        gui::pop_style_color(1);
    }

    fn draw_areas_popup(&mut self, e: &mut Editor, id: wce_scene::render_scene::DocumentId) {
        let mut look = self.polygon_look(e).clone();
        let mut ended = false;
        let mut row = |label: &str, tip: &str, v: &mut f32| {
            gui::set_next_item_width(160.0);
            gui::slider_float(label, v, 0.0, 1.0, "%.2f");
            ended |= gui::is_item_deactivated_after_edit();
            if gui::is_item_hovered() {
                gui::set_tooltip(tip);
            }
        };

        gui::text_disabled("Areas");
        row("Outlines##areas", OUTLINES_TIP, &mut look.outline);
        row("All fills##areas", FILLS_TIP, &mut look.fill);

        let d = e.docs.find_as_mut::<TileDocument>(id).expect("a tile");
        let types = FILLED_TYPES.iter().copied().filter(|t| d.tile().type_counts().contains_key(*t)).collect::<Vec<_>>();
        if !types.is_empty() {
            gui::separator_text("Fill by type");
        }

        for ty in types {
            let [r, g, b] = polygon_colour(ty);
            gui::align_text_to_frame_padding();
            gui::text_colored([r, g, b, 1.0], type_icon(ty));
            gui::same_line();
            let mut v = look.type_factor(ty);
            row(&format!("{}##area{ty}", type_name(ty)), TYPE_TIP, &mut v);
            look.set_type_factor(ty, v);
        }

        gui::separator();
        let t = d.tile_mut();
        let v = t.display_mut();
        if gui::checkbox("Show polygons##areas", &mut v.show_polygons) {
            t.display_changed();
        }

        let v = t.display_mut();
        if gui::checkbox("Over buildings##areas", &mut v.polygon_over_buildings) {
            t.polygons_changed();
        }

        if gui::is_item_hovered() {
            gui::set_tooltip("The polygons' fill and edges go smoothly over the buildings; off: they stay on the terrain.");
        }

        gui::begin_disabled(look == PolygonLook::default());
        if gui::small_button("Reset##areas") {
            look = PolygonLook::default();
            ended = true;
        }

        gui::end_disabled();
        self.areas = Some(look);
        if ended {
            self.save_polygon_look(e);
        }
    }
}
