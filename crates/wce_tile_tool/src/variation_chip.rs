//! The Variation chip (docs/plans/tile_variations.md): at the top right of the viewport, left of the Areas chip, for a combined project
//! only: the variation in view, and a popup that views another (`tile_variation`). Its scene and its terrain show; the others' do not.

use wce_editor::editor::Editor;
use wce_editor::{gui, icons};
use wce_tile_editor::tile_document::TileDocument;

use crate::ground_views::{begin_chip_popup, chip};
use crate::tool::{TileTool, document_id};

impl TileTool {
    /// The Variation chip, `right` pixels left of the viewport's right edge (`top` down from its top); its width is 0 without a combined
    /// project.
    pub(crate) fn draw_variation_chip(&mut self, e: &mut Editor, origin: gui::Vec2, size: gui::Vec2, top: f32, right: f32) {
        let Some(id) = document_id(e) else {
            self.variation_chip_width = 0.0;
            return;
        };

        let d = e.docs.find_as::<TileDocument>(id).expect("a tile");
        let vars = d.tile().variations().to_vec();
        if vars.is_empty() {
            self.variation_chip_width = 0.0;
            return;
        }

        let viewed = d.tile().viewed_variation().unwrap_or_default().to_owned();
        let tip =
            format!("Variation: {viewed} (of {})\nThe scene and the terrain of this variation show. Click: view another", vars.join(", "));
        gui::set_cursor_screen_pos(gui::v2(origin.x + size.x - right - self.variation_chip_width - 4.0, origin.y + top));
        gui::push_style_color(gui::COL_CHILD_BG, gui::v4(0.08, 0.08, 0.09, 0.78));
        let mut pick = None;
        if gui::begin_pill("##variationchip") {
            if chip(true, icons::LAYERS, "variations", &tip) {
                gui::open_popup("variations");
            }

            gui::same_line();
            gui::align_text_to_frame_padding();
            gui::text(&viewed);
            if begin_chip_popup("variations") {
                gui::text_disabled("Variation in view");
                for v in &vars {
                    if gui::radio_button(&format!("{v}##variation"), *v == viewed) && *v != viewed {
                        pick = Some(v.clone());
                    }
                }

                gui::end_popup();
            }
        }

        gui::end_child();
        let (lo, hi) = gui::item_rect();
        self.variation_chip_width = hi.x - lo.x;
        gui::pop_style_color(1);
        if let Some(v) = pick {
            let d = e.docs.find_as_mut::<TileDocument>(id).expect("a tile");
            let msg = match d.tile_mut().view_variation(&mut e.scene, &v) {
                Ok(()) => format!("viewing the variation {v}"),
                Err(m) => m,
            };
            e.status(&msg, false);
        }
    }
}
