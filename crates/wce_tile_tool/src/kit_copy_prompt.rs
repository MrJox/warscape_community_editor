//! The question when a tile is opened from an Assembly Kit (the Tiles panel, the recent tiles, the favourites, File > Open tile...) and the
//! projects have their own copy of it (saved before): open the project's (the edits saved there) or the kit's (as TEd has it; saving it
//! replaces the project's, as File > Revert > To the Assembly Kit's version does). A script gets the project's, as before.

use std::path::{Path, PathBuf};

use wce_editor::editor::Editor;
use wce_editor::{gui, theme};
use wce_tile_editor::tile_document::TileDocument;

use crate::tool::{TILE_TOOL_ID, TileTool};

/// The tile asked about.
#[derive(Clone, Debug)]
pub(crate) struct CopyPrompt {
    /// What was opened (as `open_tile` took it), and its variation.
    target: String,
    variation: String,

    /// The kit's variation folder and the project's.
    kit: PathBuf,
    project: PathBuf,
}

/// The same folder, whatever the case and the separators.
pub(crate) fn same_folder(a: &Path, b: &Path) -> bool {
    let key = |p: &Path| {
        let p = std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
        p.to_string_lossy().replace('\\', "/").trim_end_matches('/').to_lowercase()
    };

    key(a) == key(b)
}

/// When a folder's files were last written (the newest of them), for the prompt.
fn modified(dir: &Path) -> String {
    let newest = std::fs::read_dir(dir).ok().into_iter().flatten().flatten().filter_map(|f| f.metadata().ok()?.modified().ok()).max();
    match newest.and_then(|t| t.elapsed().ok()) {
        Some(d) if d.as_secs() < 3600 => format!("changed {} min ago", d.as_secs() / 60),
        Some(d) if d.as_secs() < 86_400 => format!("changed {} h ago", d.as_secs() / 3600),
        Some(d) => format!("changed {} days ago", d.as_secs() / 86_400),
        None => String::new(),
    }
}

impl TileTool {
    /// Before `open_tile` opens `ted` (a kit's tile file): when the projects have a copy of the tile, the user is asked which one opens
    /// (`Some`: the message, nothing opened yet); the kit's copy chosen, the project's document is closed first (asked about its unsaved
    /// changes), as it would be taken for this one. `None`: go on.
    ///
    /// # Errors
    ///
    /// The project's document could not be closed (its save failed, the prompt was cancelled).
    pub(crate) fn kit_or_project(&mut self, e: &mut Editor, ted: &Path, target: &str, variation: &str) -> Result<Option<String>, String> {
        let projects = Self::workspace_dir(e);
        let Some(kit) = ted.parent().map(Path::to_path_buf) else { return Ok(None) };
        if kit.starts_with(&projects) || same_folder(&kit, &projects) {
            return Ok(None);
        }

        let Some(folder) = wce_tile_editor::tile_document::kit::tile_folder(&kit) else { return Ok(None) };
        let project = projects.join("raw_data").join(&folder);
        if !wce_tile::tile_project::has_tile(&project) {
            return Ok(None);
        }

        let chosen = self.kit_choice.as_deref().is_some_and(|k| same_folder(k, &kit));
        if !chosen {
            // A script, or a session that asks nothing: the project's, as the open did before.
            if e.in_script() || !e.interactive() {
                return Ok(None);
            }

            self.copy_prompt = Some(CopyPrompt { target: target.to_owned(), variation: variation.to_owned(), kit, project });
            self.copy_prompt_open = true;
            return Ok(Some(format!("{folder} is in the projects too: which copy opens?")));
        }

        // The project's document would be taken for the kit's tile (it is "opened from" the kit's folder): it goes first.
        let open = e
            .docs
            .of_tool(TILE_TOOL_ID)
            .filter(|&id| e.docs.find_as::<TileDocument>(id).is_some_and(|d| same_folder(&d.project().dir, &project)));
        if let Some(id) = open {
            let (again, again_variation) = (target.to_owned(), variation.to_owned());
            let retry = Box::new(move |e: &mut Editor| {
                e.with_tool::<Self, _>(|t, e| t.open_tile(e, &again, &again_variation))
                    .unwrap_or_else(|| Err("the Tile tool is busy".to_owned()))
            });
            if !e.clear_to_replace(TILE_TOOL_ID, retry)? {
                return Ok(Some(format!("asking about the project's unsaved changes before opening the kit's {folder}")));
            }

            e.close_document(id);
        }

        Ok(None)
    }

    pub(crate) fn draw_copy_prompt(&mut self, e: &mut Editor) {
        const TITLE: &str = "Open tile";
        if std::mem::take(&mut self.copy_prompt_open) {
            gui::open_popup(TITLE);
        }

        if !gui::begin_popup_modal(TITLE, None, gui::WINDOW_ALWAYS_AUTO_RESIZE) {
            return;
        }

        let Some(p) = self.copy_prompt.clone() else {
            gui::close_current_popup();
            gui::end_popup();
            return;
        };

        gui::text("This tile is in the projects folder too (saved from the editor):");
        gui::text_disabled(&format!("Project: {}  ({})", p.project.display(), modified(&p.project)));
        gui::text_disabled(&format!("Assembly Kit: {}  ({})", p.kit.display(), modified(&p.kit)));
        gui::text_colored(theme::warning(), "Saving the Assembly Kit's copy replaces the project's.");
        gui::separator();
        let mut done = None;
        if gui::button("Open the project's") {
            self.kit_choice = None;
            done = Some(self.open_tile(e, &p.project.display().to_string(), ""));
        }

        if gui::is_item_hovered() {
            gui::set_tooltip("Opens the project, with the edits saved in it.");
        }

        gui::same_line();
        if gui::button("Open the Assembly Kit's") {
            self.kit_choice = Some(p.kit.clone());
            done = Some(self.open_tile(e, &p.target, &p.variation));
        }

        if gui::is_item_hovered() {
            gui::set_tooltip(
                "Opens the kit's tile as TEd has it. The project stays on disk until this tile is saved: saving writes over it.",
            );
        }

        gui::same_line();
        if gui::button("Cancel") || gui::is_key_pressed(gui::KEY_ESCAPE, true) {
            done = Some(Ok("open cancelled".to_owned()));
        }

        if let Some(r) = done {
            e.status(&r.unwrap_or_else(|m| format!("open failed: {m}")), false);
            self.copy_prompt = None;
            gui::close_current_popup();
        }

        gui::end_popup();
    }
}
