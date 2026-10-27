//! Tile > Import height map...: a picture's brightness as the tile's heights (`TileDocument::import_height_map`), stretched over the whole
//! height map, black and white at the heights given, replacing the heights or added to them.

use std::path::Path;

use wce_editor::editor::Editor;
use wce_editor::steam_detect::file_dialog;
use wce_editor::{gui, theme};
use wce_tile_editor::tile_document::TileDocument;

use crate::tool::{TileTool, document};

/// What the dialog holds while it is open.
#[derive(Clone, Debug)]
pub(crate) struct HeightImportDialog {
    path: String,
    low: f32,
    high: f32,
    add: bool,
    error: String,
}

impl TileTool {
    /// Opens the dialog on the next frame, black and white at the tile's lowest and highest heights now.
    pub fn open_height_import_dialog(&mut self, e: &mut Editor) {
        let (low, high) = document(e).and_then(|d: &mut TileDocument| d.height_span()).unwrap_or((0.0, 50.0));
        self.height_import = Some(HeightImportDialog { path: String::new(), low, high, add: false, error: String::new() });
        self.height_import_open = true;
    }

    pub(crate) fn draw_height_import_dialog(&mut self, e: &mut Editor) {
        const TITLE: &str = "Import height map";
        if std::mem::take(&mut self.height_import_open) {
            gui::open_popup(TITLE);
        }

        if !gui::begin_popup_modal(TITLE, None, gui::WINDOW_ALWAYS_AUTO_RESIZE) {
            return;
        }

        let Some(mut d) = self.height_import.take() else {
            gui::close_current_popup();
            gui::end_popup();
            return;
        };

        gui::text_wrapped(
            "A picture's brightness as the tile's heights, stretched over the whole height map (the tile with its border, as height_map_0.png: an \\
             edited copy of it comes back where it was). Grey or colour PNG, 8 or 16 bit, or DDS.",
        );
        gui::set_next_item_width(360.0);
        gui::input_text("##heightfile", Some("picture"), &mut d.path, 1024, 0);
        gui::same_line();
        if gui::button("Browse...") {
            let filters = [("Picture (PNG, DDS)".to_owned(), "*.png *.dds".to_owned()), ("All files".to_owned(), "*.*".to_owned())];
            if let Some(p) = file_dialog(false, Path::new(&d.path), &filters) {
                d.path = p.display().to_string();
            }
        }

        gui::set_next_item_width(120.0);
        gui::drag_float("black (m)", &mut d.low, 0.1, -10_000.0, 10_000.0, "%.2f");
        if gui::is_item_hovered() {
            gui::set_tooltip("The height of the picture's black, in metres as the editor shows them (it starts at the tile's lowest point).");
        }

        gui::set_next_item_width(120.0);
        gui::drag_float("white (m)", &mut d.high, 0.1, -10_000.0, 10_000.0, "%.2f");
        if gui::is_item_hovered() {
            gui::set_tooltip("The height of the picture's white (it starts at the tile's highest point); between the two, the grey's share.");
        }

        gui::checkbox("add to the heights", &mut d.add);
        if gui::is_item_hovered() {
            gui::set_tooltip("Off: the picture's heights replace the tile's. On: they are added to them (black and white as offsets).");
        }

        if !d.error.is_empty() {
            gui::text_colored(theme::warning(), &d.error);
        }

        gui::separator();
        let import = gui::button("Import");
        gui::same_line();
        let cancel = gui::button("Cancel") || gui::is_key_pressed(gui::KEY_ESCAPE, true);
        if import {
            let result = document(e).ok_or_else(|| "no tile open".to_owned()).and_then(|t| t.import_height_map(Path::new(&d.path), d.low, d.high, d.add));
            match result {
                Ok(n) => {
                    e.status(&format!("height map imported: {n} samples changed (one undo step)"), false);
                    gui::close_current_popup();
                    gui::end_popup();
                    return;
                }

                Err(err) => d.error = err,
            }
        }

        if cancel {
            gui::close_current_popup();
        } else {
            self.height_import = Some(d);
        }

        gui::end_popup();
    }
}
