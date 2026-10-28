//! Tile > Save tile as... (Phase 6b of docs/plans/tile_ted_parity.md): TEd's Save Tile As, the open tile as a battle tile of another tile
//! set, name and variation in the projects folder (`TileDocument::save_tile_as`).

use wce_editor::editor::Editor;
use wce_editor::gui;
use wce_scene::render_scene::DocumentId;
use wce_tile_editor::tile_document::TileDocument;

use crate::tool::{TileTool, document_id};

/// What the dialog holds: tile set, name, variation, and the last error.
#[derive(Clone, Debug, Default)]
pub(crate) struct SaveAsUi {
    /// The popup is to be opened (where it is drawn: its id scope).
    pub opening: bool,
    pub set: String,
    pub name: String,
    pub variation: String,
    pub error: String,
}

impl TileTool {
    pub(crate) fn open_save_tile_as(&mut self, e: &mut Editor) {
        let Some(id) = document_id(e) else { return };
        let Some(d) = e.docs.find_as::<TileDocument>(id) else { return };
        let info = &d.project().info;

        // The current set and name from the location (terrain\tiles\battle\<set>\<name>), the variation as it is.
        let parts = info.location.split(['\\', '/']).collect::<Vec<_>>();
        let set = parts.get(3).map_or("assembly_kit", |s| *s).to_owned();
        let name = parts.get(4..).map(|p| p.join("_")).unwrap_or_default();
        self.save_as = Some(SaveAsUi { opening: true, set, name, variation: info.variation.clone(), error: String::new() });
    }

    /// Saves the open tile as `set/name/variation`; the status says how it went.
    pub(crate) fn save_tile_as(&mut self, e: &mut Editor, id: DocumentId, set: &str, name: &str, variation: &str) -> Result<String, String> {
        let kit = self.battle_database_root(e);
        let d = e.docs.find_as_mut::<TileDocument>(id).ok_or("no tile")?;
        let msg = d.save_tile_as(set, name, variation, kit.as_deref())?;
        self.status = msg.clone();
        Ok(msg)
    }

    pub(crate) fn draw_save_tile_as(&mut self, e: &mut Editor) {
        if let Some(ui) = self.save_as.as_mut()
            && std::mem::take(&mut ui.opening)
        {
            gui::open_popup("Save tile as");
        }

        if self.save_as.is_none() || !gui::begin_popup_modal("Save tile as", None, gui::WINDOW_ALWAYS_AUTO_RESIZE) {
            return;
        }

        let mut ui = self.save_as.take().unwrap_or_default();
        gui::text_wrapped("The tile as a battle tile of the projects folder: terrain/tiles/battle/<tile set>/<name>/<variation>, with an entry of its own. A tile of that name and size takes it as one more variation (keeping its other properties); one of another size is refused, as TEd does.");
        for (label, v) in [("tile set", &mut ui.set), ("name", &mut ui.name), ("variation", &mut ui.variation)] {
            gui::set_next_item_width(240.0);
            gui::input_text(label, None, v, 64, 0);
        }

        if !ui.error.is_empty() {
            gui::text_colored(wce_editor::theme::palette().error, &ui.error);
        }

        let mut keep = true;
        if gui::button("Save") {
            let id = document_id(e);
            match id.map(|id| self.save_tile_as(e, id, &ui.set, &ui.name, &ui.variation)) {
                Some(Ok(msg)) => {
                    e.status(&msg, false);
                    keep = false;
                }

                Some(Err(err)) => ui.error = err,
                None => keep = false,
            }
        }

        gui::same_line();
        if gui::button("Cancel") {
            keep = false;
        }

        if keep {
            self.save_as = Some(ui);
        } else {
            gui::close_current_popup();
        }

        gui::end_popup();
    }
}
