//! File > Share project and File > Import shared project (`wce_tile::share`): the open tile's project in a zip for another user, and such a
//! zip into the projects folder; the imported tile then opens. The Prefab tool's File > Share prefab and File > Import shared prefab do the
//! same for a prefab (`prefab_tool.rs`), through its Tile tool.
//!
//! An import asks first when it would replace files, or when the shared scenes use what the receiver's setup lacks (prefabs of another kit,
//! buildings, props, trees, decals or effects their game files do not have): nothing is written until they answer.

use std::path::{Path, PathBuf};

use wce_editor::editor::Editor;
use wce_editor::gui;
use wce_editor::steam_detect::file_dialog;
use wce_editor_core::document::Document;
use wce_foundation::log;
use wce_tile::share::{self, ImportPlan, Share, ShareKind};

use crate::tool::{TileTool, document};

/// What the menu items do (their tooltips).
const SHARE_TIP: &str = "Writes the open tile's project into one zip to give to someone else: every variation of the tile, its database entry, and the \
     prefabs of your projects it uses (with those they use, and the road pieces of its roads). Prefabs of the Assembly Kit are not copied: the zip names them. \
     Save the tile first.";
const IMPORT_TIP: &str = "Puts a project shared with Share project into your projects folder and opens its tile. When the tile or one of its prefabs is \
     there already and differs, or when it uses prefabs, buildings, props, trees, decals or effects you do not have, it asks first; what it replaces is \
     kept as .bak.";
pub(crate) const SHARE_PREFAB_TIP: &str = "Writes the prefab on the stage into one zip to give to someone else: its file (with its ground), and the prefabs \
     of your projects it uses (with those they use). Prefabs of the Assembly Kit are not copied: the zip names them. Save the prefab first.";
pub(crate) const IMPORT_PREFAB_TIP: &str = "Puts a prefab shared with Share prefab into your projects' prefabs and opens it on the stage. When it or one of \
     the prefabs it uses is there already and differs, or when it uses prefabs, buildings, props, trees, decals or effects you do not have, it asks \
     first; what it replaces is kept as .bak.";

/// The most file names a list in the prompt shows.
const LISTED: usize = 12;

/// An import that would replace files or lacks something, waiting for the user's answer.
#[derive(Clone, Debug)]
pub(crate) struct ImportPrompt {
    zip: PathBuf,
    plan: ImportPlan,
}

/// What a share or an import has to tell, in a dialog (docs/plans/editor_todo.md, "Share / Import shared project: visible errors"): why it
/// failed and what to do about it, or the prefabs it left out.
#[derive(Clone, Debug)]
pub(crate) struct ShareNotice {
    title: String,
    error: Option<String>,
    hint: String,
    text: String,
    lists: Vec<(String, Vec<String>)>,
}

/// What to do about a share's or an import's failure, from its reason; empty when the reason says it.
fn hint(why: &str) -> &'static str {
    let has = |s: &str| why.contains(s);
    if has("save the tile first") {
        "Save the tile (Ctrl + S), then share it again."
    } else if has("save the prefab first") {
        "Save the prefab (Ctrl + S), then share it again."
    } else if has("an Assembly Kit's prefab") {
        "Save the prefab on the stage (Ctrl + S): a save makes your projects' copy of it, which is what is shared."
    } else if has("no database entry") {
        "Open the tile through the Tiles panel (from a kit or your projects), so that it has its tile database entry, then share it."
    } else if has("not in a kit's layout") {
        "Save it as a project (Save as project, in the Tile panel), so that it is in your projects folder, then share that."
    } else if has("is not there") || has("is not in") {
        "The tile's files have moved or are gone: open it again from the Tiles panel."
    } else if has("is a shared") {
        "Import a shared tile in the Tile tool (File > Import shared project), a shared prefab in the Prefab tool (File > Import shared prefab)."
    } else if has("is not a zip") || has("is not a shared project") || has("manifest") || has("outside its folder") {
        "Pick a zip made with File > Share project or File > Share prefab (as it was given to you, not unpacked and packed again)."
    } else if has("the one open here") {
        "Close the tile (File > Close), then import the project again."
    } else if has("on the stage") {
        "Save or revert the prefab on the stage (File > Save, File > Revert), then import it again."
    } else if has("newer editor") || has("switch the game") {
        ""
    } else if has("cannot create") || has("cannot move") || has("os error") {
        "Check that the folder can be written to (not read-only, not open in another program) and that the disk has room."
    } else {
        ""
    }
}

/// The projects' `raw_data`.
pub(crate) fn projects_raw(e: &mut Editor) -> PathBuf {
    TileTool::workspace_dir(e).join("raw_data")
}

/// What an import plan lacks, as a sentence, or nothing.
fn lacking(plan: &ImportPlan) -> String {
    let mut parts = vec![];
    if !plan.lacking_prefabs.is_empty() {
        parts.push(format!("prefabs {}", plan.lacking_prefabs.join(", ")));
    }

    for (kind, keys) in plan.lacking_by_kind() {
        parts.push(format!("{} {}", kind.label().to_lowercase(), keys.join(", ")));
    }

    if parts.is_empty() {
        return String::new();
    }

    format!(" It uses what you do not have (from another Assembly Kit or a mod): {}.", parts.join("; "))
}

/// The titles of a kind's share and import.
fn titles(kind: ShareKind) -> (&'static str, &'static str) {
    match kind {
        ShareKind::Tile => ("Share project", "Import shared project"),
        ShareKind::Prefab => ("Share prefab", "Import shared prefab"),
    }
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
        self.write_share(e, &share, &name, out)
    }

    /// Writes a prefab of the projects into a zip (`share::collect_prefab`): `out`, or one the user picks. The caller checks it is saved.
    pub(crate) fn share_prefab(&mut self, e: &mut Editor, key: &str, out: Option<&Path>) -> Result<String, String> {
        let lib = self.prefab_library(e);
        let projects = projects_raw(e);
        let share = share::collect_prefab(key, &projects, &lib, e.game().key)?;
        self.write_share(e, &share, key, out)
    }

    /// Writes a share into `out` or a zip the user picks (`name`.zip to start with), and says what it left out.
    fn write_share(&mut self, e: &mut Editor, share: &Share, name: &str, out: Option<&Path>) -> Result<String, String> {
        let m = &share.manifest;
        let out = match out {
            Some(out) => out.to_path_buf(),
            None => {
                let start = dirs_start(e).join(format!("{name}.zip"));
                let filter = format!("Shared {}", m.kind.noun());
                let Some(out) = file_dialog(true, &start, &[(filter, "*.zip".to_owned())]) else {
                    return Ok("share cancelled".to_owned());
                };
                out
            }
        };

        let out = if out.extension().is_none() { out.with_extension("zip") } else { out };
        share::write_zip(share, &out)?;
        let mut msg = format!("shared {} file(s) into {}", share.files.len(), out.display());
        if !m.kit_prefabs.is_empty() {
            msg += &format!("; it uses prefabs of the Assembly Kit, not in the zip: {}", m.kit_prefabs.join(", "));
        }

        if !m.missing_prefabs.is_empty() {
            let noun = m.kind.noun();
            msg += &format!("; prefabs found nowhere: {}", m.missing_prefabs.join(", "));
            log::warn(format!(
                "share: {} prefab(s) the {noun} uses were found nowhere and are not in the zip: {}",
                m.missing_prefabs.len(),
                m.missing_prefabs.join(", ")
            ));
            let mut lists = vec![("Found nowhere (not in the zip)".to_owned(), m.missing_prefabs.clone())];
            if !m.kit_prefabs.is_empty() {
                lists.push(("The Assembly Kit's (not in the zip: the other user needs that kit)".to_owned(), m.kit_prefabs.clone()));
            }

            self.share_notice(ShareNotice {
                title: titles(m.kind).0.to_owned(),
                error: None,
                hint: format!(
                    "The {noun} uses prefabs that are neither in your projects nor in an Assembly Kit (deleted or renamed since): their references \
                    will show nothing for the other user. Place them again or remove those references, then share again."
                ),
                text: format!("Shared into {}, without some of its prefabs.", out.display()),
                lists,
            });
        }

        Ok(msg)
    }

    /// Asks for a shared zip of this kind and imports it: the Tile tool's tile, the Prefab tool's prefab (`prefab_tool.rs` checks its stage
    /// and opens what came in).
    pub(crate) fn pick_shared(&mut self, e: &mut Editor, kind: ShareKind) -> Option<PathBuf> {
        let filters = [(format!("Shared {}", kind.noun()), "*.zip".to_owned()), ("All files".to_owned(), "*.*".to_owned())];
        file_dialog(false, &dirs_start(e), &filters)
    }

    /// Asks for a shared project and imports it.
    pub(crate) fn import_shared(&mut self, e: &mut Editor) -> Result<String, String> {
        let Some(zip) = self.pick_shared(e, ShareKind::Tile) else { return Ok("import cancelled".to_owned()) };
        self.import_shared_from(e, &zip, false)
    }

    /// What importing a shared zip would do (`share::plan_import`, the assets checked against the game's files); one of another kind than
    /// `kind` is refused.
    pub(crate) fn plan_shared(&mut self, e: &mut Editor, zip: &Path, kind: ShareKind) -> Result<ImportPlan, String> {
        let lib = self.prefab_library(e);
        let raw = projects_raw(e);
        let files = e.files().map(GameFiles);
        let plan = share::plan_import(zip, &raw, &lib, files.as_ref().map(|f| f as &dyn share::AssetFiles), e.game().key)?;
        let got = plan.manifest.kind;
        if got != kind {
            return Err(format!("{} is a shared {}, not a {}", zip.display(), got.noun(), kind.noun()));
        }

        Ok(plan)
    }

    /// Imports a shared project; `overwrite`: replace what differs, and import what lacks something, without asking (the console's).
    pub(crate) fn import_shared_from(&mut self, e: &mut Editor, zip: &Path, overwrite: bool) -> Result<String, String> {
        let plan = self.plan_shared(e, zip, ShareKind::Tile)?;

        // The tile open in the editor is not replaced under it.
        let location = projects_raw(e).join(&plan.manifest.location);
        if plan.overwrites() && document(e).is_some_and(|d| d.project().dir.starts_with(&location)) {
            return Err("the shared tile is the one open here: close it first".to_owned());
        }

        self.import_or_ask(e, zip, plan, overwrite)
    }

    /// Imports a planned zip at once, or asks first when it replaces files or lacks something (a script is asked only about what it replaces:
    /// what it lacks is in its message and the log).
    pub(crate) fn import_or_ask(&mut self, e: &mut Editor, zip: &Path, plan: ImportPlan, overwrite: bool) -> Result<String, String> {
        let ask = !overwrite && (plan.overwrites() || (plan.lacks() && !e.in_script()));
        if ask {
            let why = if plan.overwrites() { "replaces files" } else { "uses what you do not have" };
            let msg = format!("the shared {} {why}: answer the prompt", plan.manifest.kind.noun());
            self.import_prompt = Some(ImportPrompt { zip: zip.to_path_buf(), plan });
            self.import_prompt_open = true;
            return Ok(msg);
        }

        self.finish_import(e, zip, &plan)
    }

    fn finish_import(&mut self, e: &mut Editor, zip: &Path, plan: &ImportPlan) -> Result<String, String> {
        let raw = projects_raw(e);
        let report = share::import(zip, &raw, plan, true)?;

        // Every library reads the prefabs again (one of them lent out, this tool's Prefab tool, does it when it opens what came in).
        self.rescan_prefabs(e);
        e.with_tool::<TileTool, _>(|t, e| t.rescan_prefabs(e));
        e.with_tool::<crate::prefab_tool::PrefabTool, _>(|p, e| p.tile.rescan_prefabs(e));
        let m = &plan.manifest;
        let mut msg = format!(
            "imported {}: {} new, {} replaced, {} the same, {} moved aside.{}",
            m.name(),
            report.written,
            report.replaced,
            report.skipped,
            report.moved_aside,
            lacking(plan)
        );

        if plan.lacks() {
            log::warn(format!("import of {}:{}", m.name(), lacking(plan)));
        }

        if !plan.assets_checked {
            msg += " The game's files are not mounted: its buildings, props, trees, decals and effects were not checked.";
        }

        match m.kind {
            ShareKind::Tile => {
                let open = raw.join(plan.open_path());
                if let Err(why) = self.open_tile(e, &open.display().to_string(), "") {
                    msg += &format!(" It did not open: {why}");
                }
            }

            ShareKind::Prefab => self.imported_prefab = Some(m.prefab.clone()),
        }

        Ok(msg)
    }

    fn share_notice(&mut self, notice: ShareNotice) {
        self.share_notice = Some(notice);
        self.share_notice_open = true;
    }

    /// A share's or an import's failure: in a dialog with what to do about it, and as an error in the log.
    pub(crate) fn share_failed(&mut self, e: &mut Editor, title: &str, why: &str) {
        let what = if title.starts_with("Share") { "share" } else { "import" };
        // An error status: the log's red line too.
        e.status(&format!("{what} failed: {why}"), true);
        self.share_notice(ShareNotice {
            title: title.to_owned(),
            error: Some(why.to_owned()),
            hint: hint(why).to_owned(),
            text: String::new(),
            lists: vec![],
        });
    }

    /// File > Share project and File > Import shared project.
    pub(crate) fn share_menu_items(&mut self, e: &mut Editor) {
        if gui::menu_item("Share project...", None, false, document(e).is_some()) {
            match self.share_project(e, None) {
                Ok(msg) => e.status(&msg, false),
                Err(why) => self.share_failed(e, "Share project", &why),
            }
        }

        if gui::is_item_hovered_allow_disabled() {
            gui::set_tooltip(SHARE_TIP);
        }

        if gui::menu_item("Import shared project...", None, false, true) {
            match self.import_shared(e) {
                Ok(msg) => e.status(&msg, false),
                Err(why) => self.share_failed(e, "Import shared project", &why),
            }
        }

        if gui::is_item_hovered_allow_disabled() {
            gui::set_tooltip(IMPORT_TIP);
        }
    }

    /// The question before an import replaces files or brings in what uses things the user lacks.
    pub(crate) fn draw_import_prompt(&mut self, e: &mut Editor) {
        const ID: &str = "##import_shared";
        if std::mem::take(&mut self.import_prompt_open) {
            gui::open_popup(ID);
        }

        gui::set_next_window_size_max(gui::v2(760.0, 680.0));
        if !gui::begin_popup_modal(ID, None, gui::WINDOW_ALWAYS_AUTO_RESIZE | gui::WINDOW_NO_TITLE_BAR) {
            return;
        }

        let Some(p) = self.import_prompt.clone() else {
            gui::close_current_popup();
            gui::end_popup();
            return;
        };

        let (kind, name) = (p.plan.manifest.kind, p.plan.manifest.name().to_owned());
        let title = titles(kind).1;
        gui::text(title);
        gui::separator();
        gui::push_text_wrap_pos(720.0);
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

        if p.plan.overwrites() {
            gui::text(&format!("{name} is in your projects already, and differs."));
            list("Replaced by the shared ones", &p.plan.differs);
            list("Not in the shared project (moved aside)", &p.plan.stale);
            if !p.plan.new.is_empty() {
                gui::text_disabled(&format!("{} new file(s), {} the same", p.plan.new.len(), p.plan.same.len()));
            }
        }

        if p.plan.lacks() {
            gui::spacing();
            gui::text_colored(
                wce_editor::theme::error(),
                &format!(
                    "The shared {} uses what your setup does not have (your projects, Assembly Kits and game files): it shows nothing, or a \
                    stand-in, where those are. Get them from whoever shared it (the mod or the kit they come from), or replace them after.",
                    kind.noun()
                ),
            );
            list("Prefabs", &p.plan.lacking_prefabs);
            for (k, keys) in p.plan.lacking_by_kind() {
                list(k.label(), &keys);
            }
        }

        if !p.plan.assets_checked {
            gui::spacing();
            gui::text_disabled("The game's files are not mounted: its buildings, props, trees, decals and effects were not checked.");
        }

        gui::pop_text_wrap_pos();
        gui::spacing();
        if p.plan.overwrites() {
            gui::text_disabled("Each file replaced or moved aside is kept as <name>.bak.");
        }

        gui::separator();
        let go = gui::button(if p.plan.overwrites() { "Overwrite" } else { "Import anyway" });
        if gui::is_item_hovered() {
            gui::set_tooltip(if p.plan.overwrites() {
                "Imports the shared files over yours; each file replaced or moved aside is kept as <name>.bak."
            } else {
                "Imports it as it is: what you lack shows nothing until you have it."
            });
        }

        gui::same_line();
        let cancelled = gui::button("Cancel") || gui::is_key_pressed(gui::KEY_ESCAPE, true);
        if go || cancelled {
            self.import_prompt = None;
            gui::close_current_popup();
            if go {
                match self.finish_import(e, &p.zip, &p.plan) {
                    Ok(msg) => e.status(&msg, false),
                    Err(why) => self.share_failed(e, title, &why),
                }
            } else {
                e.status("import cancelled: nothing was written", false);
            }
        }

        gui::end_popup();
    }

    /// The dialog of a share's or an import's notice.
    pub(crate) fn draw_share_notice(&mut self) {
        const ID: &str = "##share_notice";
        if std::mem::take(&mut self.share_notice_open) {
            gui::open_popup(ID);
        }

        let Some(n) = self.share_notice.clone() else { return };
        gui::set_next_window_size_max(gui::v2(720.0, 600.0));
        let vp = gui::main_viewport();
        gui::set_next_window_pos(
            gui::v2(vp.WorkPos.x + vp.WorkSize.x * 0.5, vp.WorkPos.y + vp.WorkSize.y * 0.5),
            gui::COND_APPEARING,
            gui::v2(0.5, 0.5),
        );
        if !gui::begin_popup_modal(ID, None, gui::WINDOW_ALWAYS_AUTO_RESIZE | gui::WINDOW_NO_TITLE_BAR) {
            return;
        }

        gui::text(&n.title);
        gui::separator();
        gui::push_text_wrap_pos(680.0);
        if let Some(why) = &n.error {
            gui::text_colored(wce_editor::theme::error(), &format!("It failed: {why}"));
        }

        if !n.text.is_empty() {
            gui::text(&n.text);
        }

        if !n.hint.is_empty() {
            gui::spacing();
            gui::text(&n.hint);
        }

        gui::pop_text_wrap_pos();
        for (title, names) in &n.lists {
            gui::separator_text(title);
            for name in names.iter().take(LISTED) {
                gui::bullet_text(name);
            }

            if names.len() > LISTED {
                gui::text_disabled(&format!("... and {} more (all of them are in the log)", names.len() - LISTED));
            }
        }

        gui::spacing();
        gui::separator();
        if gui::button("OK") || gui::is_key_pressed(gui::KEY_ESCAPE, true) || gui::is_key_pressed(gui::KEY_ENTER, true) {
            self.share_notice = None;
            gui::close_current_popup();
        }

        gui::end_popup();
    }
}

/// The game's mounted files, as an import's asset check reads them.
struct GameFiles(std::sync::Arc<dyn wce_vfs::files::Files>);

impl share::AssetFiles for GameFiles {
    fn exists(&self, path: &str) -> bool {
        self.0.exists(path)
    }

    fn list(&self, prefix: &str, extension: &str) -> Vec<String> {
        self.0.list(prefix, extension)
    }
}

/// Where the share dialogs start: the projects folder.
fn dirs_start(e: &mut Editor) -> PathBuf {
    e.projects_dir()
}
