//! Tile > Variations > Combine... and Separate (docs/plans/tile_variations.md, "Migration: Combine variations"): the open tile's
//! variations made one combined project in the projects, and one of a combined project made a project of its own again
//! (`wce_tile::variations::combine`). Nothing combines by itself: the dialog lists the entry's variations with where each comes from (a
//! project, the kit, the packs) and what it shares with the reference; none is ticked at first.

use std::path::{Path, PathBuf};
use std::sync::mpsc::Receiver;

use wce_editor::editor::Editor;
use wce_editor::gui;
use wce_formats::tile_entry::TileEntry;
use wce_tile::variations::combine::{Input, Origin, Report};
use wce_tile::variations::merge::Stats;
use wce_tile_editor::tile_document::TileDocument;
use wce_tile_editor::tile_document::context::TileProvider;

use crate::tool::{TileTool, document_id};

/// A variation of the entry in the dialog.
#[derive(Clone, Debug)]
pub(crate) struct Row {
    pub variation: String,

    /// Where it comes from, none: nowhere (no project, kit or pack has it).
    pub origin: Option<Origin>,

    /// Its folder: known for a project's or a kit's, the packs' once decompiled.
    pub dir: Option<PathBuf>,
    pub ticked: bool,
}

enum Msg {
    Read(Result<(Vec<Option<PathBuf>>, Stats), String>),
    Combined(Result<Report, String>),
}

/// The Combine dialog.
pub(crate) struct CombineUi {
    pub opening: bool,
    pub entry: TileEntry,
    pub projects_raw: PathBuf,
    pub rows: Vec<Row>,

    /// The row whose terrain is the base.
    pub reference: usize,

    /// What the rows with a folder share (`stats_rows`: which rows, in its order).
    pub stats: Option<Stats>,
    pub stats_rows: Vec<usize>,

    /// The variation to view after combining.
    pub view: String,

    /// *Align heights*: a level whose height map is another's stored with another range takes that one's heights.
    pub align: bool,

    /// Variations a script asked ticked once they are read (`tile_combine_dialog tick ...`).
    pub tick_when_read: Vec<String>,
    job: Option<Receiver<Msg>>,
    pub busy: String,
    pub error: String,
}

/// The entry a combination is made with: the projects' copy when there is one (it has the rows' ranges of the projects' variations),
/// else the tile's.
fn projects_entry(entry: &TileEntry, projects_raw: &Path) -> TileEntry {
    let copy = entry
        .file()
        .file_name()
        .map(|n| projects_raw.join("terrain/tiles").join(entry.kind()).join("_tile_database/TILES").join(n))
        .filter(|p| p.is_file());
    copy.and_then(|p| TileEntry::load(&p).ok()).unwrap_or_else(|| entry.clone())
}

/// The projects' copy of an entry (`<projects raw_data>/terrain/tiles/<kind>/_tile_database/TILES/<name>.xml`).
fn entry_in(entry: &TileEntry, projects_raw: &Path) -> PathBuf {
    projects_raw.join("terrain/tiles").join(entry.kind()).join("_tile_database/TILES").join(entry.file().file_name().unwrap_or_default())
}

impl TileTool {
    /// Opens a variation of the tile of `entry` in the projects (its own project, or the combined project viewing it).
    fn open_variation(&mut self, e: &mut Editor, entry: &TileEntry, projects_raw: &Path, variation: &str) -> Result<String, String> {
        self.open_tile(e, &entry_in(entry, projects_raw).display().to_string(), variation)
    }

    /// The open tile's entry, the projects' `raw_data`, and each of the entry's variations with where it comes from: a project of its own
    /// in the projects, else the first kit that has it, else the packs (when they can be read).
    fn combine_rows(&mut self, e: &mut Editor) -> Result<(TileEntry, PathBuf, Vec<Row>, Option<TileProvider>), String> {
        let id = document_id(e).ok_or("no tile is open")?;
        let d = e.docs.find_as::<TileDocument>(id).ok_or("no tile is open")?;
        let entry = d.project().entry.clone().ok_or("the tile has no database entry")?;
        let projects_raw = Self::workspace_dir(e).join("raw_data");
        let entry = projects_entry(&entry, &projects_raw);
        if entry.tile_folder(&projects_raw).join(wce_tile::project_files::WCE_FILE).is_file() {
            return Err("the tile is a combined project already (Separate a variation to change which are in it)".to_owned());
        }

        let kits = self.kit_roots(e).into_iter().filter(|r| *r != projects_raw).collect::<Vec<_>>();
        self.prepare_neighbours(e);
        let provider = self.context.neighbours.as_ref().map(|(_, p)| p.clone()).or_else(|| self.context.packs_fallback.clone());
        let rows = entry
            .variations()
            .iter()
            .enumerate()
            .map(|(i, v)| {
                let own = entry.variation_folder(&projects_raw, i);
                let kit = kits.iter().map(|k| entry.variation_folder(k, i)).find(|d| wce_tile::tile_project::has_tile(d));
                let (origin, dir) = if wce_tile::tile_project::has_tile(&own) {
                    (Some(Origin::Project), Some(own))
                } else if let Some(k) = kit {
                    (Some(Origin::Kit), Some(k))
                } else if provider.is_some() {
                    (Some(Origin::Packs), None)
                } else {
                    (None, None)
                };
                Row { variation: v.location.clone(), origin, dir, ticked: false }
            })
            .collect();
        Ok((entry, projects_raw, rows, provider))
    }

    /// Opens the Combine dialog for the open tile, its variations read (and what they share) on a thread.
    pub(crate) fn open_combine(&mut self, e: &mut Editor) {
        let (entry, projects_raw, rows, provider) = match self.combine_rows(e) {
            Ok(r) => r,
            Err(m) => {
                e.status(&format!("combine: {m}"), false);
                return;
            }
        };

        let view = document_id(e)
            .and_then(|id| e.docs.find_as::<TileDocument>(id))
            .and_then(|d| d.project().entry.as_ref().zip(d.project().variation).map(|(en, v)| en.variations()[v].location.clone()))
            .unwrap_or_default();
        let location = entry.location();
        let wanted = rows.iter().map(|r| (r.variation.clone(), r.origin, r.dir.clone())).collect::<Vec<_>>();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let read = (|| {
                let mut dirs = vec![];
                let mut inputs = vec![];
                for (v, origin, dir) in wanted {
                    let dir = match (origin, dir, &provider) {
                        (Some(_), Some(d), _) => Some(d),
                        (Some(Origin::Packs), None, Some(p)) => p(&location, &v).ok().filter(|d| wce_tile::tile_project::has_tile(d)),
                        _ => None,
                    };
                    if let (Some(d), Some(o)) = (&dir, origin) {
                        inputs.push(Input { variation: v.clone(), dir: d.clone(), origin: o });
                    }

                    dirs.push(dir);
                }

                if inputs.len() < 2 {
                    return Err("fewer than two of its variations can be read: nothing to combine".to_owned());
                }

                Ok((dirs, wce_tile::variations::combine::shares(&inputs)?))
            })();
            let _ = tx.send(Msg::Read(read));
        });

        self.combine = Some(CombineUi {
            opening: true,
            entry,
            projects_raw,
            rows,
            reference: 0,
            stats: None,
            stats_rows: vec![],
            view,
            align: false,
            tick_when_read: vec![],
            job: Some(rx),
            busy: "Reading the variations and what they share...".to_owned(),
            error: String::new(),
        });
    }

    /// The tile's documents (any variation of it) closed, after saving those modified: a combination or a separation changes their files.
    fn close_tile_documents(e: &mut Editor, location: &str, save: bool) -> Result<(), String> {
        let ids = e
            .docs
            .order()
            .iter()
            .copied()
            .filter(|&id| {
                e.docs
                    .find_as::<TileDocument>(id)
                    .is_some_and(|d| d.project().entry.as_ref().is_some_and(|en| en.location().eq_ignore_ascii_case(location)))
            })
            .collect::<Vec<_>>();
        for id in ids {
            if e.docs.find(id).is_some_and(|d| d.modified()) {
                if !save {
                    return Err("the tile has unsaved changes: save them first".to_owned());
                }

                e.docs.save_one(id)?;
            }

            e.close_document(id);
        }

        Ok(())
    }

    /// Combines now (the command; the dialog does it on a thread): the variations named, `reference` giving the terrain's base (none: the
    /// one sharing the most). Opens the combined project.
    ///
    /// # Errors
    ///
    /// Fewer than two variations, one that can't be read, the tile has unsaved changes, or what `combine` refuses.
    pub fn combine_now(&mut self, e: &mut Editor, names: &[String], reference: Option<&str>, align: bool) -> Result<String, String> {
        let (entry, projects_raw, rows, provider) = self.combine_rows(e)?;
        let location = entry.location();
        let mut inputs = vec![];
        for n in names {
            let r = rows.iter().find(|r| r.variation.eq_ignore_ascii_case(n)).ok_or_else(|| format!("the tile has no variation '{n}'"))?;
            let dir = match (&r.dir, r.origin, &provider) {
                (Some(d), _, _) => d.clone(),
                (None, Some(Origin::Packs), Some(p)) => p(&location, &r.variation)?,
                _ => return Err(format!("{n}: no project, kit or pack has it")),
            };
            inputs.push(Input { variation: r.variation.clone(), dir, origin: r.origin.unwrap_or(Origin::Kit) });
        }

        let reference = match reference {
            Some(v) => {
                inputs.iter().position(|i| i.variation.eq_ignore_ascii_case(v)).ok_or_else(|| format!("{v} is not among those combined"))?
            }
            None => wce_tile::variations::combine::shares(&inputs)?.reference(),
        };
        let view = inputs[reference].variation.clone();
        Self::close_tile_documents(e, &location, false)?;
        let report = wce_tile::variations::combine::combine_with(&inputs, reference, &entry, &projects_raw, align)?;
        self.browse_changed();
        wce_foundation::log::info(&report.message);
        self.open_variation(e, &entry, &projects_raw, &view)?;
        Ok(report.message)
    }

    /// Separate: a variation of the open combined project made a project of its own again (`combine::separate`), the tile saved first and
    /// opened again after: the variation's own project when it was the one viewed (or the last), else the combined project.
    ///
    /// # Errors
    ///
    /// No combined project is open, it does not save, or what `separate` refuses.
    pub fn separate_variation(&mut self, e: &mut Editor, variation: &str) -> Result<String, String> {
        let id = document_id(e).ok_or("no tile is open")?;
        let d = e.docs.find_as::<TileDocument>(id).ok_or("no tile is open")?;
        if !d.project().is_combined() {
            return Err("the tile is not a combined project".to_owned());
        }

        let tile_dir = d.project().dir.clone();
        let entry = d.project().entry.clone().ok_or("the combined project has no tile database entry")?;
        let projects_raw = d.project().raw_root.clone().ok_or("the combined project is not in a kit layout")?;
        let location = entry.location();
        let viewed = d.tile().viewed_variation().unwrap_or_default().to_owned();
        let all = d.tile().variations().to_vec();
        let name = all
            .iter()
            .find(|v| v.eq_ignore_ascii_case(variation))
            .cloned()
            .ok_or_else(|| format!("the project has no variation '{variation}'"))?;
        Self::close_tile_documents(e, &location, true)?;
        let result = wce_tile::variations::combine::separate(&tile_dir, &name);
        self.browse_changed();
        let open = if result.is_err() || (viewed != name && all.len() > 1) { viewed } else { name };
        let reopened = self.open_variation(e, &entry, &projects_raw, &open);
        let msg = result?;
        wce_foundation::log::info(&msg);
        reopened?;
        Ok(msg)
    }

    /// Separate's question (Tile > Variations > Separate > <v>...): what it does, then Separate or Cancel. A click on the menu does nothing
    /// by itself (it sat next to Launch in game's list of the same variations, and a launch became a separation).
    pub(crate) fn draw_separate_confirm(&mut self, e: &mut Editor) {
        const TITLE: &str = "Separate a variation";
        let Some(v) = self.separate_ask.clone() else { return };
        if std::mem::take(&mut self.separate_opening) {
            gui::open_popup(TITLE);
        }

        if !gui::begin_popup_modal(TITLE, None, gui::WINDOW_ALWAYS_AUTO_RESIZE) {
            return;
        }

        gui::text(&format!("Separate {v} from the combined project?"));
        gui::text_disabled(&format!(
            "{v} becomes a project of its own again (its folder in the tile's), and the combined project keeps the others."
        ));
        gui::text_disabled("The tile is saved first; the combined project's old files go to the projects' backup folder.");
        if gui::button(&format!("Separate {v}")) {
            self.separate_ask = None;
            gui::close_current_popup();
            let msg = self.separate_variation(e, &v).unwrap_or_else(|m| format!("separate: {m}"));
            e.status(&msg, false);
        }

        gui::same_line();
        if gui::button("Cancel") || gui::is_key_pressed(gui::KEY_ESCAPE, true) {
            self.separate_ask = None;
            gui::close_current_popup();
        }

        gui::end_popup();
    }

    /// The Combine dialog, while it is open.
    pub(crate) fn draw_combine(&mut self, e: &mut Editor) {
        const TITLE: &str = "Combine variations";
        if let Some(ui) = self.combine.as_mut()
            && std::mem::take(&mut ui.opening)
        {
            gui::open_popup(TITLE);
        }

        if self.combine.is_none() || !gui::begin_popup_modal(TITLE, None, gui::WINDOW_ALWAYS_AUTO_RESIZE) {
            return;
        }

        let Some(mut ui) = self.combine.take() else { return };
        let mut keep = true;

        // What the thread brought.
        let mut combined = None;
        if let Some(msg) = ui.job.as_ref().and_then(|rx| rx.try_recv().ok()) {
            ui.job = None;
            ui.busy.clear();
            match msg {
                Msg::Read(Ok((dirs, stats))) => {
                    ui.stats_rows = dirs.iter().enumerate().filter(|(_, d)| d.is_some()).map(|(i, _)| i).collect();
                    for (row, dir) in ui.rows.iter_mut().zip(dirs) {
                        if dir.is_none() {
                            row.origin = None;
                        }

                        row.dir = dir;
                    }

                    ui.reference = ui.stats_rows.get(stats.reference()).copied().unwrap_or(0);
                    for row in ui.rows.iter_mut().filter(|r| r.dir.is_some()) {
                        row.ticked |= ui.tick_when_read.iter().any(|v| v.eq_ignore_ascii_case(&row.variation));
                    }

                    ui.stats = Some(stats);
                }

                Msg::Read(Err(m)) | Msg::Combined(Err(m)) => ui.error = m,
                Msg::Combined(Ok(r)) => combined = Some(r),
            }
        }

        gui::text(&format!("{}: its variations made one project in the projects.", ui.entry.location()));
        gui::text_disabled("Tick those to combine (none is ticked at first). What each shares with the reference helps to choose: random");
        gui::text_disabled("alternatives share little and gain little. The reference's terrain is the base the others' differ from.");
        gui::separator();
        let at = |ui: &CombineUi, row: usize| ui.stats_rows.iter().position(|&r| r == row);
        let reference = ui.reference;
        for i in 0..ui.rows.len() {
            let ready = ui.rows[i].dir.is_some() && ui.job.is_none();
            let label = ui.rows[i].variation.clone();
            gui::begin_disabled(!ready);
            gui::checkbox(&format!("{label}##combine_tick"), &mut ui.rows[i].ticked);
            gui::end_disabled();
            gui::same_line_at(150.0);
            gui::text_disabled(ui.rows[i].origin.map_or("nowhere", Origin::label));
            gui::same_line_at(230.0);
            let share = match (&ui.stats, at(&ui, i), at(&ui, reference)) {
                (Some(s), Some(a), Some(r)) if a == r => format!("{} instances, the reference", s.instances[a]),
                (Some(s), Some(a), Some(r)) => {
                    format!(
                        "{} instances, {:.0}% of them shared with {}",
                        s.instances[a],
                        100.0 * s.share(a, r),
                        ui.rows[reference].variation
                    )
                }
                _ => String::new(),
            };
            gui::text(&share);
            gui::same_line_at(560.0);
            gui::begin_disabled(!ready);
            if gui::radio_button(&format!("reference##combine_ref{i}"), reference == i) {
                ui.reference = i;
            }

            gui::end_disabled();
        }

        gui::separator();
        gui::checkbox("Align heights", &mut ui.align);
        if gui::is_item_hovered() {
            gui::set_tooltip(
                "For a level copied from another and saved again with another height range (its ground then stands a little higher or lower): it takes the other's heights where its stored samples are the same, and its range, and its buildings' own heights that only followed that shift go. Its export changes there (the report says how much); the others come back as they are.",
            );
        }

        gui::text_disabled("Checked before anything is written: each variation must come back from the combined project as it is.");
        gui::text_disabled("The projects that go in are moved to the projects' backup folder; the kit and the packs are not touched.");
        if !ui.busy.is_empty() {
            gui::text(&ui.busy);
        }

        if !ui.error.is_empty() {
            gui::text_colored(wce_editor::theme::palette().error, &ui.error);
        }

        let ticked = ui.rows.iter().enumerate().filter(|(_, r)| r.ticked && r.dir.is_some()).map(|(i, _)| i).collect::<Vec<_>>();
        gui::begin_disabled(ticked.len() < 2 || ui.job.is_some());
        let go = gui::button("Combine");
        gui::end_disabled();
        if gui::is_item_hovered_allow_disabled() {
            gui::set_tooltip(
                "Combines the ticked variations (at least two), checks each comes back as it is, then writes the combined project. The tile is closed and opened again combined.",
            );
        }

        if go {
            let location = ui.entry.location();
            match Self::close_tile_documents(e, &location, false) {
                Err(m) => ui.error = m,
                Ok(()) => {
                    let inputs = ticked
                        .iter()
                        .map(|&i| {
                            let r = &ui.rows[i];
                            Input {
                                variation: r.variation.clone(),
                                dir: r.dir.clone().unwrap_or_default(),
                                origin: r.origin.unwrap_or(Origin::Kit),
                            }
                        })
                        .collect::<Vec<_>>();
                    let reference = ticked.iter().position(|&i| i == ui.reference).unwrap_or(0);
                    if !inputs.iter().any(|i| i.variation == ui.view) {
                        ui.view = inputs[reference].variation.clone();
                    }

                    let (entry, raw, align) = (ui.entry.clone(), ui.projects_raw.clone(), ui.align);
                    let (tx, rx) = std::sync::mpsc::channel();
                    std::thread::spawn(move || {
                        let _ =
                            tx.send(Msg::Combined(wce_tile::variations::combine::combine_with(&inputs, reference, &entry, &raw, align)));
                    });
                    ui.job = Some(rx);
                    ui.error.clear();
                    ui.busy = "Combining and checking (the tile is closed meanwhile)...".to_owned();
                }
            }
        }

        gui::same_line();
        gui::begin_disabled(ui.job.is_some() && ui.stats.is_some());
        if gui::button("Cancel") {
            keep = false;
        }

        gui::end_disabled();
        if let Some(r) = combined {
            wce_foundation::log::info(&r.message);
            self.browse_changed();
            let (entry, raw, view) = (ui.entry.clone(), ui.projects_raw.clone(), ui.view.clone());
            let opened = self.open_variation(e, &entry, &raw, &view);
            e.status(&opened.map_or_else(|m| format!("combined, but it did not open: {m}"), |_| r.message.clone()), false);
            keep = false;
        }

        if keep {
            self.combine = Some(ui);
        } else {
            gui::close_current_popup();
        }

        gui::end_popup();
    }
}
