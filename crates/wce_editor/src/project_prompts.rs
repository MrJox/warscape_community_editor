//! The projects folder's prompts (docs/plans/save_system.md): **Revert to game's version** (the project's loose copy of a game file
//! deleted, the packs' file read again), and, once per game until the release, the old output folder's files offered to the project's
//! loose folder, where Save writes them now.

use std::path::{Path, PathBuf};

use wce_foundation::log;
use wce_scene::render_scene::DocumentId;
use wce_vfs::normalize;

use crate::editor::Editor;
use crate::gui;

#[derive(Default)]
pub struct ProjectPrompts {
    /// The document Revert to game's version asks about.
    revert: Option<DocumentId>,
    open_revert: bool,

    /// The old output folder and the files in it that would move (paths in the packs), while the offer is open.
    old_output: Option<(PathBuf, Vec<String>)>,
    open_old_output: bool,

    /// The game whose old output folder was looked at this session.
    checked_game: String,
}

/// TODO(pre-release): the files of an old output folder that move into the project's loose folder: edits of game files Save wrote there
/// (weather files, the weather database, UI layouts: paths the packs have), not the build files (`launch/`, `dry_*`), the UI tool's
/// XML exports (`ui_xml/`), nor exports in the game's layout (`terrain/`: exported tiles and maps, which would hide the game's own).
pub fn movable_old_output_files(dir: &Path, in_packs: impl Fn(&str) -> bool) -> Vec<String> {
    let mut out = vec![];
    let mut pending = vec![dir.to_path_buf()];
    while let Some(d) = pending.pop() {
        let Ok(entries) = std::fs::read_dir(&d) else { continue };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                pending.push(path);
                continue;
            }

            let Ok(rel) = path.strip_prefix(dir) else { continue };
            let rel = normalize(&rel.to_string_lossy());
            let skipped = rel.starts_with("launch/") || rel.starts_with("dry_") || rel.starts_with("ui_xml/") || rel.starts_with("terrain/");
            if !skipped && in_packs(&rel) {
                out.push(rel);
            }
        }
    }

    out.sort();
    out
}

impl Editor {
    /// Asks before Revert to game's version deletes the document's loose copy.
    pub fn ask_revert(&mut self, id: DocumentId) {
        self.project_prompts.revert = Some(id);
        self.project_prompts.open_revert = true;
    }

    /// TODO(pre-release): looks once per game and session (interactive runs only) for files of the old output folder to offer.
    pub(crate) fn check_old_output(&mut self) {
        let key = self.game().key.to_owned();
        if self.project_prompts.checked_game == key || !self.interactive() || self.vfs().is_none() {
            return;
        }

        self.project_prompts.checked_game = key;
        let Some(dir) = self.settings_ref().old_output_dir() else { return };
        let vfs = self.vfs().cloned();
        let in_packs = |p: &str| vfs.as_ref().is_some_and(|v| v.owners(p).iter().any(|&i| !v.packs()[i].loose));
        let files = movable_old_output_files(&dir, in_packs);
        if files.is_empty() {
            self.old_output_offered();
            return;
        }

        self.project_prompts.old_output = Some((dir, files));
        self.project_prompts.open_old_output = true;
    }

    /// The old output folder is not asked about again (for this game).
    fn old_output_offered(&mut self) {
        let g = self.settings().game_mut();
        g.output_dir = None;
        g.output_dir_offered = true;
        self.save_settings();
    }

    /// TODO(pre-release): moves the old output folder's files into the project's loose folder (a file the project has already stays); the
    /// message says what moved and what stayed.
    ///
    /// # Errors
    ///
    /// None so far: a file that can't move is listed in the log and stays.
    pub fn move_old_output(&mut self, dir: &Path, files: &[String]) -> Result<String, String> {
        let (mut moved, mut kept) = (0, vec![]);
        for rel in files {
            let from = dir.join(rel);
            let to = self.loose_path(rel);
            if to.exists() {
                kept.push(format!("{rel} (the project has it)"));
                continue;
            }

            let r = to.parent().map_or(Ok(()), std::fs::create_dir_all).and_then(|()| {
                std::fs::rename(&from, &to).or_else(|_| std::fs::copy(&from, &to).and_then(|_| std::fs::remove_file(&from)))
            });

            match r {
                Ok(()) => {
                    moved += 1;
                    self.loose_changed(rel);
                }

                Err(e) => kept.push(format!("{rel} ({e})")),
            }
        }

        for k in &kept {
            log::warn(format!("old output folder: {k} stays in {}", dir.display()));
        }

        Ok(format!("{moved} file(s) of the old output folder moved into the project's loose folder{}", if kept.is_empty() { String::new() } else { format!(", {} stayed (see the log)", kept.len()) }))
    }

    pub(crate) fn draw_project_prompts(&mut self) {
        self.draw_revert_prompt();
        self.draw_old_output_prompt();
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

    fn draw_old_output_prompt(&mut self) {
        const TITLE: &str = "Files of the old output folder";
        if std::mem::take(&mut self.project_prompts.open_old_output) {
            gui::open_popup(TITLE);
        }

        if !gui::begin_popup_modal(TITLE, None, gui::WINDOW_ALWAYS_AUTO_RESIZE) {
            return;
        }

        let Some((dir, files)) = self.project_prompts.old_output.clone() else {
            gui::close_current_popup();
            gui::end_popup();
            return;
        };

        gui::text_wrapped(&format!(
            "Save used to write edited game files into the output folder ({}); it writes them into the project's loose folder now, where every tool reads them before the packs. Move these {} file(s) there?",
            dir.display(),
            files.len()
        ));

        for f in files.iter().take(12) {
            gui::bullet_text(f);
        }

        if files.len() > 12 {
            gui::text_disabled(&format!("... and {} more", files.len() - 12));
        }

        gui::spacing();
        let mut done = false;
        if gui::button("Move them into the project") {
            match self.move_old_output(&dir, &files) {
                Ok(m) => self.status(&m, false),
                Err(m) => self.status(&m, true),
            }

            done = true;
        }

        gui::same_line();
        if gui::button("Leave them where they are") {
            self.status(&format!("the old output folder's files stay in {}", dir.display()), false);
            done = true;
        }

        if done {
            self.project_prompts.old_output = None;
            self.old_output_offered();
            gui::close_current_popup();
        }

        gui::end_popup();
    }
}
