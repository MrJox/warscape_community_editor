//! The Tile panel's Terrain layers section and the `terrain_layer` command (docs/plans/procedural_roads.md). The list reads as a stack: the top
//! row applies last, and the base is always at the bottom. A click on a row makes it the working layer: brushes of its type then edit it.

use wce_editor::editor::Editor;
use wce_editor::{gui, icons, theme};
use wce_scene::render_scene::DocumentId;
use wce_tile_editor::tile_document::TileDocument;
use wce_tile_editor::tile_document::layers::{LayerRow, LayerType};

use crate::tool::TileTool;

const SECTION_TIP: &str = "Terrain made by composition: what you paint goes into the working layer and leaves the base alone, so it can be hidden, faded, reordered or removed. \
    Height layers add to what is below, texture layers go over it, ground type layers replace it. Save for Assembly Kit writes the terrain they make, as TEd has no layers.";

/// The section's state.
#[derive(Default)]
pub struct LayersUi {
    kind: usize,
    name: String,
}

impl TileTool {
    pub(crate) fn draw_layers_section(&mut self, e: &mut Editor, id: DocumentId) {
        if !gui::collapsing_header("Terrain layers", 0) {
            return;
        }

        if gui::is_item_hovered() {
            gui::set_tooltip(SECTION_TIP);
        }

        if gui::begin_tab_bar("##layer_types") {
            for (k, kind) in LayerType::ALL.into_iter().enumerate() {
                if gui::begin_tab_item(kind.title(), 0) {
                    self.layers.kind = k;
                    let d = e.docs.find_as_mut::<TileDocument>(id).expect("a tile");
                    self.draw_layer_list(d, kind);
                    gui::end_tab_item();
                }
            }

            gui::end_tab_bar();
        }
    }

    fn draw_layer_list(&mut self, d: &mut TileDocument, kind: LayerType) {
        let (rows, working) = d.terrain_layers(kind);
        let can_add = gui::button(&format!("{} New layer", icons::PLUS));
        if can_add {
            let name = format!("Layer {}", rows.len() + 1);
            if let Err(why) = d.add_terrain_layer(kind, &name, "") {
                self.status = why;
            }

            return;
        }

        let selected = working.and_then(|id| rows.iter().find(|r| r.id == id)).cloned();
        if let Some(row) = &selected {
            self.draw_layer_actions(d, kind, row, &rows);
        }

        gui::spacing();
        for row in rows.iter().rev() {
            gui::push_id(&format!("layer{}", row.id));
            let eye = if row.visible { icons::EYE } else { icons::EYE_OFF };
            if gui::small_button(eye) {
                d.set_terrain_layer_look(kind, row.id, !row.visible, row.opacity);
            }

            gui::same_line();
            let label = if row.owner.is_empty() { row.name.clone() } else { format!("{} {}", icons::ROUTE, row.name) };
            if gui::selectable(&label, working == Some(row.id), 0) {
                d.set_working_layer(kind, Some(row.id));
            }

            gui::pop_id();
        }

        gui::push_id("layer_base");
        gui::small_button(icons::LAYERS);
        gui::same_line();
        if gui::selectable("Base", working.is_none(), 0) {
            d.set_working_layer(kind, None);
        }

        gui::pop_id();
        match &selected {
            Some(row) if !row.visible => {
                gui::push_style_color(gui::COL_TEXT, theme::v4(theme::warning()));
                gui::text_wrapped("Hidden: what you paint goes to the base until it is shown.");
                gui::pop_style_color(1);
            }

            _ => {}
        }
    }

    /// The working layer's name, opacity and actions.
    fn draw_layer_actions(&mut self, d: &mut TileDocument, kind: LayerType, row: &LayerRow, rows: &[LayerRow]) {
        gui::label_column();
        self.layers.name.clone_from(&row.name);
        gui::row("Name");
        if gui::input_text("##layer_name", None, &mut self.layers.name, 64, 0) {
            d.rename_terrain_layer(kind, row.id, &self.layers.name);
        }

        let mut opacity = row.opacity;
        gui::row("Opacity");
        if gui::slider_float("##layer_opacity", &mut opacity, 0.0, 1.0, "%.2f") {
            d.set_terrain_layer_look(kind, row.id, row.visible, opacity);
        }

        let at = rows.iter().position(|r| r.id == row.id).unwrap_or(0);
        gui::begin_disabled(at + 1 >= rows.len());
        if gui::small_button(icons::ARROW_BIG_UP) {
            d.move_terrain_layer(kind, row.id, 1);
        }

        gui::end_disabled();
        gui::same_line();
        gui::begin_disabled(at == 0);
        if gui::small_button(icons::ARROW_BIG_DOWN) {
            d.move_terrain_layer(kind, row.id, -1);
        }

        gui::end_disabled();
        gui::same_line();
        if gui::small_button("Merge down") {
            d.merge_terrain_layer(kind, row.id);
        }

        gui::same_line();
        if gui::small_button(icons::TRASH_2) {
            d.remove_terrain_layer(kind, row.id);
        }
    }
}
