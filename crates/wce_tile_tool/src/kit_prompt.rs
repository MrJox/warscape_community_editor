//! The question which Assembly Kit "Save for Assembly Kit" writes to, asked when several kits fit (the settings list more than one).

use std::path::PathBuf;

use wce_editor::editor::Editor;
use wce_editor::gui;

use crate::tool::TileTool;

/// What is saved for the kit.
#[derive(Clone, Debug)]
pub(crate) enum KitSave {
    /// The open tile.
    Tile,

    /// A prefab, by its key.
    Prefab(String),
}

/// The save asked about and the kits to choose from (their raw_data folders).
#[derive(Clone, Debug)]
pub(crate) struct KitPrompt {
    what: KitSave,
    kits: Vec<PathBuf>,
}

impl TileTool {
    /// Opens the question; the answer saves (`draw_kit_prompt`).
    pub(crate) fn ask_kit(&mut self, what: KitSave, kits: Vec<PathBuf>) -> String {
        self.kit_prompt = Some(KitPrompt { what, kits });
        self.kit_prompt_open = true;
        "choose the Assembly Kit to save for".to_owned()
    }

    pub(crate) fn draw_kit_prompt(&mut self, e: &mut Editor) {
        const TITLE: &str = "Save for Assembly Kit";
        if std::mem::take(&mut self.kit_prompt_open) {
            gui::open_popup(TITLE);
        }

        if !gui::begin_popup_modal(TITLE, None, gui::WINDOW_ALWAYS_AUTO_RESIZE) {
            return;
        }

        let Some(p) = self.kit_prompt.clone() else {
            gui::close_current_popup();
            gui::end_popup();
            return;
        };

        gui::text(match p.what {
            KitSave::Tile => "Several Assembly Kits can take this tile. Save it for:",
            KitSave::Prefab(_) => "Several Assembly Kits can take this prefab. Save it for:",
        });

        let mut chosen = None;
        for (i, raw) in p.kits.iter().enumerate() {
            // The kit is the folder that holds raw_data.
            let kit = raw.parent().unwrap_or(raw);
            if gui::button(&format!("{}##kit{i}", kit.display())) {
                chosen = Some(raw.clone());
            }
        }

        gui::separator();
        let cancelled = gui::button("Cancel") || gui::is_key_pressed(gui::KEY_ESCAPE, true);
        if chosen.is_some() || cancelled {
            let msg = match chosen {
                Some(raw) => match p.what {
                    KitSave::Tile => self.save_for_kit(e, Some(raw)),
                    KitSave::Prefab(key) => self.save_prefab_for_kit(e, &key, Some(raw)),
                }
                .unwrap_or_else(|m| format!("save for the Assembly Kit failed: {m}")),

                None => "save for the Assembly Kit cancelled".to_owned(),
            };

            e.status(&msg, false);
            self.kit_prompt = None;
            gui::close_current_popup();
        }

        gui::end_popup();
    }
}
