//! The question before an exported tile is extracted over a project of the same folder (one extracted before, or the kit's tile of that
//! name saved): open the project, extract the tile over it (its edits are lost), or neither.

use std::path::Path;

use wce_editor::editor::Editor;
use wce_editor::{gui, theme};

use crate::tool::TileTool;

/// The tile asked about.
#[derive(Clone, Debug)]
pub(crate) struct ExtractPrompt {
    /// The exported tile (a folder on disk, or the packs' folder).
    target: String,

    /// The project it would replace, and whether that is an extracted tile.
    project: String,
    extracted: bool,
}

impl ExtractPrompt {
    pub(crate) fn new(target: &str, project: &Path) -> Self {
        Self { target: target.to_owned(), project: project.display().to_string(), extracted: wce_tile::project_files::is_extracted_dir(project) }
    }
}

impl TileTool {
    pub(crate) fn draw_extract_prompt(&mut self, e: &mut Editor) {
        const TITLE: &str = "Extract tile";
        if std::mem::take(&mut self.extract_prompt_open) {
            gui::open_popup(TITLE);
        }

        if !gui::begin_popup_modal(TITLE, None, gui::WINDOW_ALWAYS_AUTO_RESIZE) {
            return;
        }

        let Some(p) = self.extract_prompt.clone() else {
            gui::close_current_popup();
            gui::end_popup();
            return;
        };

        let what = if p.extracted { "an extracted tile" } else { "a project of the kit's tile" };
        gui::text(&format!("The projects folder has this tile already ({what}):"));
        gui::text_disabled(&p.project);
        gui::text_colored(theme::warning(), "Extracting it again replaces that project: its edits are lost.");
        gui::separator();
        let mut done = None;
        if gui::button("Open the project") {
            done = Some(self.open_exported(e, &p.target, None, false));
        }

        gui::same_line();
        if gui::button("Overwrite") {
            done = Some(self.open_exported(e, &p.target, None, true));
        }

        gui::same_line();
        if gui::button("Cancel") || gui::is_key_pressed(gui::KEY_ESCAPE, true) {
            done = Some(Ok("extraction cancelled".to_owned()));
        }

        if let Some(r) = done {
            e.status(&r.unwrap_or_else(|m| format!("open failed: {m}")), false);
            self.extract_prompt = None;
            gui::close_current_popup();
        }

        gui::end_popup();
    }
}
