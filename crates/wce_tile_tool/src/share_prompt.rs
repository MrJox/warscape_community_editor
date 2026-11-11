//! File > Share project and File > Import shared project (`wce_tile::share`): the open tile's project in a zip for another user, and such a
//! zip into the projects folder, asking before it replaces anything; the imported tile then opens.

use std::path::{Path, PathBuf};

use wce_editor::editor::Editor;
use wce_editor::gui;
use wce_editor::steam_detect::file_dialog;
use wce_editor_core::document::Document;
use wce_tile::share::{self, ImportPlan};

use crate::tool::{TileTool, document};

/// What the menu items do (their tooltips).
const SHARE_TIP: &str = "Writes the open tile's project into one zip to give to someone else: every variation of the tile, its database entry, and the \
     prefabs of your projects it uses (with those they use, and the road pieces of its roads). Prefabs of the Assembly Kit are not copied: the zip names them. \
     Save the tile first.";
const IMPORT_TIP: &str = "Puts a project shared with Share project into your projects folder and opens its tile. When the tile or one of its prefabs is \
     there already and differs, it asks first; what it replaces is kept as .bak.";

/// The most file names a list in the prompt shows.
const LISTED: usize = 12;

/// An import that would replace files, waiting for the user's answer.
#[derive(Clone, Debug)]
pub(crate) struct ImportPrompt {
    zip: PathBuf,
    plan: ImportPlan,
}

/// The projects' `raw_data`.
fn projects_raw(e: &mut Editor) -> PathBuf {
    TileTool::workspace_dir(e).join("raw_data")
}

/// What an import plan lacks, as a sentence, or nothing.
fn lacking(plan: &ImportPlan) -> String {
    if plan.lacking_prefabs.is_empty() {
        return String::new();
    }

    format!(" Prefabs it uses that you do not have (from another Assembly Kit or mod): {}.", plan.lacking_prefabs.join(", "))
}

impl TileTool {
    /// Writes the open tile's project into a zip (`share::collect`): `out`, or one the user picks. The tile has to be saved: what is shared
    /// is its files.
    pub(crate) fn share_project(&mut self, e: &mut Editor, out: Option<&Path>) -> Result<String, String> {
        let d = document(e).ok_or("no tile is open")?;
        if d.modified() {
            return Err("save the tile first: its files are what is shared".to_owned());
        }

        let p = d.project();
        let entry = p.entry.as_ref().ok_or("the tile has no database entry to share it with")?;
        let raw_root = p.raw_root.clone().ok_or("the tile is not in a kit's layout")?;
        let (entry_file, location) = (entry.file().to_path_buf(), entry.location_generic());
        let variation = p.dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let name = location.rsplit('/').next().unwrap_or("tile").to_owned();
        let lib = self.prefab_library(e);
        let projects = projects_raw(e);
        let share = share::collect(&raw_root, &entry_file, &location, &variation, &projects, &lib, e.game().key)?;
        let out = match out {
            Some(out) => out.to_path_buf(),
            None => {
                let start = dirs_start(e).join(format!("{name}.zip"));
                let Some(out) = file_dialog(true, &start, &[("Shared project".to_owned(), "*.zip".to_owned())]) else { return Ok("share cancelled".to_owned()) };
                out
            }
        };

        let out = if out.extension().is_none() { out.with_extension("zip") } else { out };
        share::write_zip(&share, &out)?;
        let m = &share.manifest;
        let mut msg = format!("shared {} file(s) into {}", share.files.len(), out.display());
        if !m.kit_prefabs.is_empty() {
            msg += &format!("; it uses prefabs of the Assembly Kit, not in the zip: {}", m.kit_prefabs.join(", "));
        }

        if !m.missing_prefabs.is_empty() {
            msg += &format!("; prefabs found nowhere: {}", m.missing_prefabs.join(", "));
        }

        Ok(msg)
    }

    /// Asks for a shared zip and imports it (`share::plan_import`): straight away when it replaces nothing, else after the prompt.
    pub(crate) fn import_shared(&mut self, e: &mut Editor) -> Result<String, String> {
        let filters = [("Shared project".to_owned(), "*.zip".to_owned()), ("All files".to_owned(), "*.*".to_owned())];
        let Some(zip) = file_dialog(false, &dirs_start(e), &filters) else { return Ok("import cancelled".to_owned()) };
        self.import_shared_from(e, &zip, false)
    }

    /// Imports a shared zip; `overwrite`: replace what differs without asking (the console's).
    pub(crate) fn import_shared_from(&mut self, e: &mut Editor, zip: &Path, overwrite: bool) -> Result<String, String> {
        let lib = self.prefab_library(e);
        let raw = projects_raw(e);
        let plan = share::plan_import(zip, &raw, &lib, e.game().key)?;

        // The tile open in the editor is not replaced under it.
        let location = raw.join(&plan.manifest.location);
        if plan.overwrites() && document(e).is_some_and(|d| d.project().dir.starts_with(&location)) {
            return Err("the shared tile is the one open here: close it first".to_owned());
        }

        if plan.overwrites() && !overwrite {
            self.import_prompt = Some(ImportPrompt { zip: zip.to_path_buf(), plan });
            self.import_prompt_open = true;
            return Ok("the shared project replaces files: answer the prompt".to_owned());
        }

        self.finish_import(e, zip, &plan)
    }

    fn finish_import(&mut self, e: &mut Editor, zip: &Path, plan: &ImportPlan) -> Result<String, String> {
        let raw = projects_raw(e);
        let report = share::import(zip, &raw, plan, true)?;
        self.prefabs = None;
        let mut msg = format!(
            "imported {}: {} new, {} replaced, {} the same, {} moved aside.{}",
            plan.manifest.location, report.written, report.replaced, report.skipped, report.moved_aside, lacking(plan)
        );

        let open = raw.join(plan.open_path());
        match self.open_tile(e, &open.display().to_string(), "") {
            Ok(_) => {}
            Err(why) => msg += &format!(" It did not open: {why}"),
        }

        Ok(msg)
    }

    /// File > Share project and File > Import shared project.
    pub(crate) fn share_menu_items(&mut self, e: &mut Editor) {
        if gui::menu_item("Share project...", None, false, document(e).is_some()) {
            let msg = self.share_project(e, None).unwrap_or_else(|m| format!("share failed: {m}"));
            e.status(&msg, false);
        }

        if gui::is_item_hovered_allow_disabled() {
            gui::set_tooltip(SHARE_TIP);
        }

        if gui::menu_item("Import shared project...", None, false, true) {
            let msg = self.import_shared(e).unwrap_or_else(|m| format!("import failed: {m}"));
            e.status(&msg, false);
        }

        if gui::is_item_hovered_allow_disabled() {
            gui::set_tooltip(IMPORT_TIP);
        }
    }

    /// The question before an import replaces files.
    pub(crate) fn draw_import_prompt(&mut self, e: &mut Editor) {
        const TITLE: &str = "Import shared project";
        if std::mem::take(&mut self.import_prompt_open) {
            gui::open_popup(TITLE);
        }

        if !gui::begin_popup_modal(TITLE, None, gui::WINDOW_ALWAYS_AUTO_RESIZE) {
            return;
        }

        let Some(p) = self.import_prompt.clone() else {
            gui::close_current_popup();
            gui::end_popup();
            return;
        };

        gui::text(&format!("{} is in your projects already, and differs.", p.plan.manifest.location));
        let list = |title: &str, names: &[String]| {
            if names.is_empty() {
                return;
            }

            gui::separator_text(title);
            for n in names.iter().take(LISTED) {
                gui::bullet_text(n);
            }

            if names.len() > LISTED {
                gui::text_disabled(&format!("... and {} more", names.len() - LISTED));
            }
        };

        list("Replaced by the shared ones", &p.plan.differs);
        list("Not in the shared project (moved aside)", &p.plan.stale);
        if !p.plan.new.is_empty() {
            gui::text_disabled(&format!("{} new file(s), {} the same", p.plan.new.len(), p.plan.same.len()));
        }

        if !p.plan.lacking_prefabs.is_empty() {
            gui::text_wrapped(&lacking(&p.plan));
        }

        gui::spacing();
        gui::text_disabled("Each file replaced or moved aside is kept as <name>.bak.");
        gui::separator();
        let overwrite = gui::button("Overwrite");
        gui::same_line();
        let cancelled = gui::button("Cancel") || gui::is_key_pressed(gui::KEY_ESCAPE, true);
        if overwrite || cancelled {
            let msg = if overwrite {
                self.finish_import(e, &p.zip, &p.plan).unwrap_or_else(|m| format!("import failed: {m}"))
            } else {
                "import cancelled: nothing was written".to_owned()
            };

            e.status(&msg, false);
            self.import_prompt = None;
            gui::close_current_popup();
        }

        gui::end_popup();
    }
}

/// Where the share dialogs start: the projects folder.
fn dirs_start(e: &mut Editor) -> PathBuf {
    e.projects_dir()
}
