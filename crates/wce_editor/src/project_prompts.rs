//! The projects folder's prompts (docs/plans/save_system.md): **Revert to game's version** (the project's loose copy of a game file
//! deleted, the packs' file read again).

use wce_scene::render_scene::DocumentId;

use crate::editor::Editor;
use crate::gui;

#[derive(Default)]
pub struct ProjectPrompts {
    /// The document Revert to game's version asks about.
    revert: Option<DocumentId>,
    open_revert: bool,
}

impl Editor {
    /// Asks before Revert to game's version deletes the document's loose copy.
    pub fn ask_revert(&mut self, id: DocumentId) {
        self.project_prompts.revert = Some(id);
        self.project_prompts.open_revert = true;
    }

    pub(crate) fn draw_project_prompts(&mut self) {
        self.draw_revert_prompt();
    }

    fn draw_revert_prompt(&mut self) {
        const TITLE: &str = "Revert to game's version";
        if std::mem::take(&mut self.project_prompts.open_revert) {
            gui::open_popup(TITLE);
        }

        if !gui::begin_popup_modal(TITLE, None, gui::WINDOW_ALWAYS_AUTO_RESIZE) {
            return;
        }

        let id = self.project_prompts.revert;
        let doc = id.and_then(|id| self.docs.find(id)).map(|d| (d.title(), d.source_key(), d.modified()));
        match &doc {
            Some((title, key, modified)) => {
                gui::text(&format!("Revert {title} to the game's version?"));
                if let Some(copy) = self.loose_copy(key) {
                    gui::text_disabled(&format!("Deletes the project's copy: {}", copy.display()));
                }

                if *modified {
                    gui::text_colored(crate::theme::warning(), "Its unsaved changes are lost too.");
                }
            }

            None => gui::text("The document is closed."),
        }

        gui::spacing();
        if gui::button("Revert") {
            if let Some(id) = id.filter(|_| doc.is_some()) {
                match self.revert_to_game(id) {
                    Ok(m) => self.status(&m, false),
                    Err(m) => self.status(&m, true),
                }
            }

            self.project_prompts.revert = None;
            gui::close_current_popup();
        }

        gui::same_line();
        if gui::button("Cancel") {
            self.project_prompts.revert = None;
            gui::close_current_popup();
        }

        gui::end_popup();
    }
}
