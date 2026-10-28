//! Tile > Resize tile... (Phase 6b of docs/plans/tile_ted_parity.md): TEd's Resize Tile. The new size, where the old tile goes in it
//! (centred along x and / or z, else at an offset of whole cells), and what the new area gets; the scene moves with the old tile, the maps
//! are resized on disk and the tile is opened again (`TileDocument::resize_tile`). Not undoable: each map keeps a `.resize.bak`.

use wce_editor::editor::Editor;
use wce_editor::gui;
use wce_scene::render_scene::DocumentId;
use wce_tile::resize::{Fill, Resize};
use wce_tile_editor::tile_document::TileDocument;

use crate::tool::{TileTool, document_id};

#[derive(Clone, Debug, Default)]
pub(crate) struct ResizeUi {
    pub opening: bool,
    pub old: (u32, u32),
    pub size: [i32; 2],
    pub centre: [bool; 2],
    pub offset: [i32; 2],
    pub extend: bool,
    pub error: String,
}

impl ResizeUi {
    pub(crate) fn resize(&self) -> Resize {
        let new = (self.size[0].clamp(1, 64) as u32, self.size[1].clamp(1, 64) as u32);
        let fill = if self.extend { Fill::Extend } else { Fill::Flat };
        let mut r = Resize::centred(self.old, new, (self.centre[0], self.centre[1]), fill);
        if !self.centre[0] {
            r.offset.0 = self.offset[0];
        }

        if !self.centre[1] {
            r.offset.1 = self.offset[1];
        }

        r
    }
}

impl TileTool {
    pub(crate) fn open_resize_tile(&mut self, e: &mut Editor) {
        let Some(d) = document_id(e).and_then(|id| e.docs.find_as::<TileDocument>(id)) else { return };
        let Some(en) = d.project().entry.as_ref() else {
            self.status = "the tile has no database entry: it can't be resized".to_owned();
            return;
        };

        let old = (en.width(), en.height());
        self.resize = Some(ResizeUi { opening: true, old, size: [old.0 as i32, old.1 as i32], centre: [true, true], extend: true, ..Default::default() });
    }

    /// Resizes the open tile and opens it again; what was done.
    pub(crate) fn resize_tile(&mut self, e: &mut Editor, id: DocumentId, r: &Resize) -> Result<String, String> {
        let d = e.docs.find_as_mut::<TileDocument>(id).ok_or("no tile")?;
        let (dir, msg) = d.resize_tile(r)?;
        e.close_document(id);
        self.open_tile(e, &dir.display().to_string(), "")?;
        self.status = msg.clone();
        Ok(msg)
    }

    pub(crate) fn draw_resize_tile(&mut self, e: &mut Editor) {
        if let Some(ui) = self.resize.as_mut()
            && std::mem::take(&mut ui.opening)
        {
            gui::open_popup("Resize tile");
        }

        if self.resize.is_none() || !gui::begin_popup_modal("Resize tile", None, gui::WINDOW_ALWAYS_AUTO_RESIZE) {
            return;
        }

        let mut ui = self.resize.take().unwrap_or_default();
        gui::text(&format!("now {} x {} cells (128 m each)", ui.old.0, ui.old.1));
        gui::set_next_item_width(160.0);
        gui::input_int("cells along x", &mut ui.size[0]);
        gui::set_next_item_width(160.0);
        gui::input_int("cells along z", &mut ui.size[1]);
        ui.size = ui.size.map(|v| v.clamp(1, 64));
        gui::checkbox("centre along x", &mut ui.centre[0]);
        gui::same_line();
        gui::checkbox("centre along z", &mut ui.centre[1]);
        for (i, label) in [(0, "old tile at x cell"), (1, "old tile at z cell")] {
            if !ui.centre[i] {
                gui::set_next_item_width(160.0);
                gui::input_int(label, &mut ui.offset[i]);
            }
        }

        if gui::is_item_hovered() {
            gui::set_tooltip("Where the old tile's first cell lands in the new size (negative: the old tile's first cells are cut off).");
        }

        gui::checkbox("carry the edges out", &mut ui.extend);
        if gui::is_item_hovered() {
            gui::set_tooltip("What the new area gets: the old edge's heights, paint and ground types carried out (no cliff at the old border); off, as TEd: a new terrain (the base height, the first texture group, no ground type).");
        }

        let r = ui.resize();
        let shrinks = r.cells.0 < ui.old.0 || r.cells.1 < ui.old.1 || r.offset.0 < 0 || r.offset.1 < 0
            || r.offset.0 + ui.old.0 as i32 > r.cells.0 as i32 || r.offset.1 + ui.old.1 as i32 > r.cells.1 as i32;
        if shrinks {
            gui::text_colored(wce_editor::theme::palette().warning, "Part of the tile's ground is cut off (TEd: \"data loss\"); the things standing there stay, outside the tile.");
        }

        gui::text_disabled("The tile is saved, resized on disk and opened again: no undo (each map keeps a .resize.bak).");
        if !ui.error.is_empty() {
            gui::text_colored(wce_editor::theme::palette().error, &ui.error);
        }

        let mut keep = true;
        if gui::button("Resize") {
            match document_id(e).map(|id| self.resize_tile(e, id, &r)) {
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
            self.resize = Some(ui);
        } else {
            gui::close_current_popup();
        }

        gui::end_popup();
    }
}
