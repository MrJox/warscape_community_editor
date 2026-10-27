//! The weather selection database (warscape_asset_variation_db): which .environment file the game picks for a set of battle conditions, and
//! an editor for the whole file (see docs/re/variation_db.md). Loads the .bin the game uses from the packs, or any .bin / .xml file; Save
//! writes both forms as loose files under the output folder (as the packs ship them), Save As one file whose extension picks the format, and
//! Convert turns a .xml into a .bin or back without opening it.

use std::collections::BTreeMap;
use std::io::Cursor;
use std::path::{Path, PathBuf};

use rpfm_lib::files::asset_variation_db::{AssetVariationDb, VariationQuery};
use rpfm_lib::files::{Decodeable, Encodeable};
use wce_editor::editor::Editor;
use wce_editor::steam_detect::file_dialog;
use wce_editor::{gui, theme};
use wce_vfs::{Vfs, normalize};
use wce_weather::variation_rules::GameFacts;

use crate::variation_editor::{VariationEditState, blocks_saving, draw_variation_editor, variation};

const VDB_BIN: &str = "warscape_asset_variation_db/warscape_asset_variation_db.bin";
const VDB_XML: &str = "warscape_asset_variation_db/warscape_asset_variation_db.xml";

/// Undo steps kept.
const UNDO_STEPS: usize = 100;

/// A candidate environment file: its path as stored and how many values it allows (0 for an unconstrained variation).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Candidate {
    pub filename: String,
    pub flags: u32,
    pub allowed_values: u32,
}

/// The candidate list the game builds for a query.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Candidates {
    pub candidates: Vec<Candidate>,

    /// Nothing matched the chosen map, so the game asked again with "default_map".
    pub used_default_map: bool,
}

fn candidates_of(db: &AssetVariationDb, query: &VariationQuery) -> Candidates {
    let m = db.matches("environment", query);
    Candidates {
        candidates: m.variations.iter().map(|v| Candidate { filename: v.filename().clone(), flags: *v.flags(), allowed_values: v.flags().count_ones() }).collect(),
        used_default_map: m.used_default_map,
    }
}

fn parse_bin(bytes: &[u8]) -> Result<AssetVariationDb, String> {
    AssetVariationDb::decode(&mut Cursor::new(bytes), &None).map_err(|e| e.to_string())
}

fn parse_xml(bytes: &[u8]) -> Result<AssetVariationDb, String> {
    AssetVariationDb::from_xml(&String::from_utf8_lossy(bytes)).map_err(|e| e.to_string())
}

fn to_bin(db: &AssetVariationDb) -> Result<Vec<u8>, String> {
    let mut out = vec![];
    db.clone().encode(&mut out, &None).map_err(|e| e.to_string())?;
    Ok(out)
}

/// True for a .xml path, false for a .bin one; other extensions are refused.
fn is_xml(path: &Path) -> Result<bool, String> {
    match path.extension().map(|e| e.to_string_lossy().to_lowercase()).as_deref() {
        Some("xml") => Ok(true),
        Some("bin") => Ok(false),
        _ => Err(format!("{}: the name must end in .xml or .bin", path.display())),
    }
}

/// Reads a database from a .bin (FASTBIN0) or .xml file.
pub fn read_db(path: &Path) -> Result<AssetVariationDb, String> {
    let xml = is_xml(path)?;
    let bytes = std::fs::read(path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    if xml { parse_xml(&bytes) } else { parse_bin(&bytes) }
}

/// Writes a database as a .bin (FASTBIN0) or .xml file, creating its folder.
pub fn write_db(db: &AssetVariationDb, path: &Path) -> Result<(), String> {
    let bytes = if is_xml(path)? { db.to_xml().into_bytes() } else { to_bin(db)? };
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    }

    std::fs::write(path, bytes).map_err(|e| format!("cannot write {}: {e}", path.display()))
}

/// The other format's name: x.bin for x.xml and back.
fn other_format(path: &Path) -> PathBuf {
    path.with_extension(if is_xml(path).unwrap_or(false) { "bin" } else { "xml" })
}

fn db_filters() -> Vec<(String, String)> {
    vec![("Weather database (FASTBIN)".to_owned(), "*.bin".to_owned()), ("Weather database (XML)".to_owned(), "*.xml".to_owned())]
}

struct Source {
    label: String,
    pack: usize,
    xml: bool,
}

#[derive(Default)]
pub struct WeatherPicker {
    /// The "Weather picker" window is shown.
    pub open: bool,
    db: Option<AssetVariationDb>,

    /// As loaded or last saved, to tell whether the working copy was edited.
    orig: AssetVariationDb,
    undo: Vec<AssetVariationDb>,
    redo: Vec<AssetVariationDb>,
    error: String,
    source: String,
    message: String,
    sources: Vec<Source>,
    source_index: usize,
    save_anyway: bool,
    edit: VariationEditState,
    script: String,

    /// What the packs say about the climates and climate change phases the game sends (read with the database).
    facts: GameFacts,

    /// The Pick tab is selected when the window first shows, unless the Edit tab was asked for first (`select_pick` is the C++ flag).
    pick_shown: bool,
    edit_asked: bool,
    loaded: bool,
    query: VariationQuery,

    /// Command line / command, applied when the database loads.
    overrides: VariationQuery,
}

impl WeatherPicker {
    /// Run once after the database loaded.
    pub fn set_script(&mut self, s: &str) {
        self.script = s.to_owned();
    }

    pub fn set_pick(&mut self, category: &str, value: &str) {
        self.overrides.insert(category.to_owned(), value.to_owned());
        if self.db.is_some() {
            self.query.insert(category.to_owned(), value.to_owned());
        }
    }

    pub fn show_edit_tab(&mut self) {
        self.edit_asked = true;
    }

    fn select_pick(&self) -> bool {
        !(self.pick_shown || self.edit_asked)
    }

    pub fn message(&self) -> &str {
        &self.message
    }

    pub fn db(&self) -> Option<&AssetVariationDb> {
        self.db.as_ref()
    }

    /// The environment files the game would choose for the current query.
    pub fn candidates(&self) -> Candidates {
        self.db.as_ref().map(|db| candidates_of(db, &self.query)).unwrap_or_default()
    }

    /// Lists every .bin and .xml of the database in the packs and loads the one the game uses (the winning pack's .bin); reads the climates of
    /// the facts about the game files for the checks.
    fn load(&mut self, e: &Editor) {
        self.loaded = true;
        self.db = None;
        self.error.clear();
        self.message.clear();
        self.sources.clear();
        let Some(vfs) = e.vfs() else {
            self.error = "Packs are not mounted.".to_owned();
            return;
        };

        self.facts = GameFacts::read(vfs);
        let winner = vfs.owner(VDB_BIN);
        let mut use_source = None;
        for &pack in vfs.owners(VDB_BIN) {
            if winner == Some(pack) {
                use_source = Some(self.sources.len());
            }

            self.sources.push(Source { label: format!("{} : .bin", vfs.packs()[pack].name), pack, xml: false });
        }

        for &pack in vfs.owners(VDB_XML) {
            self.sources.push(Source { label: format!("{} : .xml", vfs.packs()[pack].name), pack, xml: true });
        }

        match use_source {
            Some(i) => self.load_source(vfs, i),
            None => self.error = format!("{VDB_BIN} is in no pack"),
        }
    }

    fn reset_query(&mut self) {
        self.query.clear();
        if let Some(db) = &self.db {
            for m in db.categories() {
                if let Some(first) = m.values().first() {
                    self.query.insert(m.name().clone(), first.clone());
                }
            }
        }
    }

    /// Makes `db` the working copy, as loaded from `source`.
    fn set_db(&mut self, db: AssetVariationDb, source: String) {
        self.source = source;
        self.orig = db.clone();
        self.db = Some(db);
        self.undo.clear();
        self.redo.clear();
        self.edit.reset();
        self.reset_query();
        for (k, v) in &self.overrides {
            self.query.insert(k.clone(), v.clone());
        }
    }

    fn load_source(&mut self, vfs: &Vfs, index: usize) {
        self.db = None;
        self.error.clear();
        self.message.clear();
        let Some(src) = self.sources.get(index) else { return };
        self.source_index = index;
        let (pack, xml, label) = (src.pack, src.xml, src.label.clone());
        let read = |path: &str| vfs.read_from(pack, path).map_err(|_| format!("cannot extract the file from {}", vfs.packs()[pack].name));
        let loaded = read(if xml { VDB_XML } else { VDB_BIN }).and_then(|bytes| if xml { parse_xml(&bytes) } else { parse_bin(&bytes) });
        let db = match loaded {
            Ok(db) => db,
            Err(e) => {
                self.error = e;
                return;
            }
        };

        // The other format of the same pack should hold the same data.
        if let Ok(other) = vfs.read_from(pack, if xml { VDB_BIN } else { VDB_XML }) {
            let o = if xml { parse_bin(&other) } else { parse_xml(&other) };
            match o {
                Ok(o) if o == db => {}
                Ok(_) => self.message = format!("The .bin and the .xml in {} hold different data.", vfs.packs()[pack].name),
                Err(e) => {
                    self.error = e;
                    return;
                }
            }
        }

        self.set_db(db, label);
    }

    fn load_file(&mut self, path: &Path) {
        match read_db(path) {
            Ok(db) => {
                self.error.clear();
                self.message = format!("Opened {}", path.display());
                self.set_db(db, path.display().to_string());
            }

            Err(e) => self.message = format!("Cannot open: {e}"),
        }
    }

    /// Why the working copy must not be written, unless "save anyway" is ticked.
    fn refuse_saving(&self, db: &AssetVariationDb) -> Option<String> {
        (blocks_saving(db, &self.facts) && !self.save_anyway)
            .then(|| "Not saved: the game would lose weather with this database (see the problems in the Edit tab). Tick \"save anyway\" to write it regardless.".to_owned())
    }

    /// Commands separated by ';' for automated tests: add-value:<category>=<value>, remove-value:<category>=<value>,
    /// rename-value:<category>=<from>><to>, clone-value:<category>=<from>><to>, add-variation:<file>, save:xml|bin|both, save-as:<file>,
    /// open:<file>, convert:<in>><out>. The result is shown in the picker window.
    fn run_script(&mut self, output_dir: &Path) {
        let script = std::mem::take(&mut self.script);
        let mut log = String::new();
        for cmd in script.split(';').filter(|c| !c.is_empty()) {
            let (op, arg) = cmd.split_once(':').unwrap_or((cmd, ""));
            let eq = arg.find('=');
            let arrow = |from: usize| arg[from..].find('>').map(|i| from + i);
            let result = match (op, eq) {
                ("save", _) | ("save-as", _) | ("open", _) | ("convert", _) => {
                    match op {
                        "save" => self.save(output_dir, arg == "xml" || arg == "both", arg == "bin" || arg == "both"),
                        "save-as" => self.save_as(Path::new(arg)),
                        "open" => self.load_file(Path::new(arg)),
                        _ => match arrow(0) {
                            Some(gt) => self.convert(Path::new(&arg[..gt]), Path::new(&arg[gt + 1..])),
                            None => self.message = "convert:<in>><out>".to_owned(),
                        },
                    }

                    log += &self.message;
                    log.push(' ');
                    continue;
                }

                _ if self.db.is_none() => Err("no database".to_owned()),
                ("add-value", Some(eq)) => self.db.as_mut().expect("checked").add_value(&arg[..eq], &arg[eq + 1..]).map_err(|e| e.to_string()),
                ("remove-value", Some(eq)) => self.db.as_mut().expect("checked").remove_value(&arg[..eq], &arg[eq + 1..]).map_err(|e| e.to_string()),
                ("rename-value" | "clone-value", Some(eq)) if arrow(eq).is_some() => {
                    let gt = arrow(eq).expect("checked");
                    let db = self.db.as_mut().expect("checked");
                    let (category, from, to) = (&arg[..eq], &arg[eq + 1..gt], &arg[gt + 1..]);
                    let result = if op == "rename-value" { db.rename_value(category, from, to) } else { db.clone_value(category, from, to) };
                    result.map_err(|e| e.to_string())
                }

                ("add-variation", _) if !self.db.as_ref().expect("checked").entries().is_empty() => {
                    let db = self.db.as_mut().expect("checked");
                    let flags = db.flags_for(&BTreeMap::new());
                    db.entries_mut()[0].variations_mut().push(variation(arg, flags));
                    Ok(())
                }

                _ => Err("unknown command".to_owned()),
            };

            log += &match result {
                Ok(()) => format!("{op} ok "),
                Err(why) => format!("{op} FAILED ({why}) "),
            };
        }

        self.message = log;
    }

    /// Writes the working copy as loose files under the output folder, at the in-pack path of the database.
    fn save(&mut self, output_dir: &Path, xml: bool, bin: bool) {
        let Some(db) = &self.db else { return };
        if let Some(why) = self.refuse_saving(db) {
            self.message = why;
            return;
        }

        let dir = output_dir.join("warscape_asset_variation_db");
        let written = (|| -> Result<String, String> {
            let mut done = String::new();
            if xml {
                write_db(db, &dir.join("warscape_asset_variation_db.xml"))?;
                done += "warscape_asset_variation_db.xml ";
            }

            if bin {
                write_db(db, &dir.join("warscape_asset_variation_db.bin"))?;
                done += "warscape_asset_variation_db.bin ";
            }

            Ok(done)
        })();

        match written {
            Ok(done) => {
                self.message = format!("Saved {done}to {}", dir.display());
                self.orig = db.clone();
            }

            Err(e) => self.message = format!("Cannot save: {e}"),
        }
    }

    /// Writes the working copy to one file; .xml or .bin by its extension.
    fn save_as(&mut self, path: &Path) {
        let Some(db) = &self.db else { return };
        if let Some(why) = self.refuse_saving(db) {
            self.message = why;
            return;
        }

        match write_db(db, path) {
            Ok(()) => {
                self.message = format!("Saved {}", path.display());
                self.orig = db.clone();
            }

            Err(e) => self.message = format!("Cannot save: {e}"),
        }
    }

    /// Converts a database file to the other format (by the extensions), without touching the working copy.
    fn convert(&mut self, from: &Path, to: &Path) {
        let converted = read_db(from).and_then(|db| match self.refuse_saving(&db) {
            Some(_) => Err(format!("{} has problems for the game; open it to see them, or tick \"save anyway\"", from.display())),
            None => write_db(&db, to),
        });

        self.message = match converted {
            Ok(()) => format!("Converted {} to {}", from.display(), to.display()),
            Err(e) => format!("Cannot convert: {e}"),
        };
    }

    /// Loads the database now (otherwise on first draw) and runs a pending script; true when a database is loaded.
    pub fn ensure_loaded(&mut self, e: &Editor, output_dir: &Path) -> bool {
        if !self.loaded {
            self.load(e);
        }

        if !self.script.is_empty() {
            self.run_script(output_dir);
        }

        self.db.is_some()
    }

    /// Source, Open, Save, Save As, Convert, Undo / Redo / Revert; true when the window must stop drawing this frame (a new database was loaded).
    fn draw_file_bar(&mut self, e: &mut Editor, output_dir: &Path) -> bool {
        gui::set_next_item_width(420.0);
        if gui::begin_combo("##source", &self.source) {
            let mut chosen = None;
            for (i, s) in self.sources.iter().enumerate() {
                if gui::selectable(&s.label, i == self.source_index, 0) {
                    chosen = Some(i);
                }
            }

            gui::end_combo();
            if let Some(i) = chosen {
                if let Some(vfs) = e.vfs().cloned() {
                    self.load_source(&vfs, i);
                }

                return true;
            }
        }

        if gui::is_item_hovered() {
            gui::set_tooltip("The database in each pack; the game uses the .bin of the pack that wins");
        }

        gui::same_line();
        if gui::button("Reload") {
            self.loaded = false;
            return true;
        }

        gui::same_line();
        if gui::button("Open...")
            && let Some(path) = file_dialog(false, &output_dir.join(VDB_BIN), &[("Weather database".to_owned(), "*.bin *.xml".to_owned())])
        {
            self.load_file(&path);
            return true;
        }

        let dirty = self.db.as_ref() != Some(&self.orig);
        if dirty {
            gui::text_colored(theme::warning(), "edited, not saved");
        } else {
            gui::text_colored(theme::dim(), "unchanged");
        }

        gui::same_line_at(140.0);
        if gui::button("Save") {
            self.save(output_dir, true, true);
        }

        if gui::is_item_hovered() {
            gui::set_tooltip(&format!(
                "Writes warscape_asset_variation_db.xml and .bin (the game reads the .bin) to\n{}",
                output_dir.join("warscape_asset_variation_db").display()
            ));
        }

        gui::same_line();
        if gui::button("Save As...")
            && let Some(path) = file_dialog(true, &output_dir.join(VDB_BIN), &db_filters())
        {
            self.save_as(&path);
        }

        if gui::is_item_hovered() {
            gui::set_tooltip("One file: a name ending in .bin writes FASTBIN (what the game reads), .xml the XML form");
        }

        gui::same_line();
        if gui::button("Convert...")
            && let Some(from) = file_dialog(false, &output_dir.join(VDB_XML), &[("Weather database".to_owned(), "*.bin *.xml".to_owned())])
            && let Some(to) = file_dialog(true, &other_format(&from), &if is_xml(&from).unwrap_or(false) { db_filters() } else { db_filters().into_iter().rev().collect() })
        {
            self.convert(&from, &to);
        }

        if gui::is_item_hovered() {
            gui::set_tooltip("Turns a .xml into a .bin or a .bin into a .xml, without opening it");
        }

        gui::same_line();
        gui::begin_disabled(self.undo.is_empty());
        if gui::button("Undo")
            && let Some(prev) = self.undo.pop()
        {
            self.redo.extend(self.db.replace(prev));
        }

        gui::end_disabled();
        gui::same_line();
        gui::begin_disabled(self.redo.is_empty());
        if gui::button("Redo")
            && let Some(next) = self.redo.pop()
        {
            self.undo.extend(self.db.replace(next));
        }

        gui::end_disabled();
        if dirty {
            gui::same_line();
            if gui::button("Revert") {
                self.undo.extend(self.db.replace(self.orig.clone()));
                self.edit.reset();
            }
        }

        gui::same_line();
        gui::checkbox("save anyway", &mut self.save_anyway);
        if gui::is_item_hovered() {
            gui::set_tooltip("Writes the database even when the game would lose weather with it");
        }

        if !self.message.is_empty() {
            gui::text_colored(theme::warning(), &self.message.clone());
        }

        false
    }

    /// The candidate list the game builds for a battle; `active_environment` is the in-pack path of the open environment (reverse lookup).
    /// Returns the in-pack path of a candidate to open (its Open button).
    pub fn draw(&mut self, e: &mut Editor, output_dir: &Path, active_environment: &str) -> Option<String> {
        if !self.open {
            return None;
        }

        gui::set_next_window_size(gui::v2(820.0, 860.0), gui::COND_FIRST_USE_EVER);
        gui::set_next_window_bg_alpha(1.0);
        if !gui::begin("Weather picker", Some(&mut self.open), 0) {
            gui::end();
            return None;
        }

        if !self.loaded {
            self.load(e);
        }

        if self.db.is_none() {
            gui::text_colored(theme::error(), &format!("Cannot read the variation database: {}", self.error));
            if gui::button("Retry") {
                self.loaded = false;
            }

            gui::same_line();
            if gui::button("Open...")
                && let Some(path) = file_dialog(false, &output_dir.join(VDB_BIN), &[("Weather database".to_owned(), "*.bin *.xml".to_owned())])
            {
                self.load_file(&path);
            }

            gui::end();
            return None;
        }

        if !self.script.is_empty() {
            self.run_script(output_dir);
        }

        if self.draw_file_bar(e, output_dir) || !gui::begin_tab_bar("vdbtabs") {
            gui::end();
            return None;
        }

        let vfs = e.vfs().cloned();
        if gui::begin_tab_item("Edit", 0) {
            if let Some(db) = self.db.as_mut() {
                let before = db.clone();
                if draw_variation_editor(db, &mut self.edit, vfs.as_deref(), &self.facts) {
                    self.undo.push(before);
                    if self.undo.len() > UNDO_STEPS {
                        self.undo.remove(0);
                    }

                    self.redo.clear();
                }
            }

            gui::end_tab_item();
        }

        if !gui::begin_tab_item("Pick", if self.select_pick() { gui::TAB_SET_SELECTED } else { 0 }) {
            gui::end_tab_bar();
            gui::end();
            return None;
        }

        self.pick_shown = true;

        let db = self.db.as_ref()?;
        gui::separator();
        gui::text("Battle conditions");
        for m in db.categories() {
            let cur = self.query.entry(m.name().clone()).or_default();
            gui::set_next_item_width(260.0);
            if gui::begin_combo(m.name(), cur) {
                for v in m.values() {
                    if gui::selectable(v, v == cur, 0) {
                        *cur = v.clone();
                    }
                }

                gui::end_combo();
            }
        }

        match db.query_mask(&self.query) {
            Some(bits) => gui::text_colored(theme::dim(), &format!("query bits 0x{bits:08X}")),
            None => gui::text_colored(theme::warning(), "A value of these conditions is in no list: the game's first query matches nothing."),
        }

        gui::separator();

        let total_values = db.value_count();
        let r = candidates_of(db, &self.query);
        if r.used_default_map {
            gui::text_colored(
                theme::warning(), &format!("Nothing matched map '{}', so the game asks again with map 'default_map'.", self.query.get("map").map_or("", String::as_str)),
            );
        }

        let mut open = None;
        if r.candidates.is_empty() {
            gui::text_colored(theme::error(), "The database has no environment file for these conditions.");
        } else {
            gui::text(&format!(
                "{} candidate(s); the game picks one uniformly at random ({:.1}% each).", r.candidates.len(), 100.0 / r.candidates.len() as f64,
            ));
            if gui::begin_table("cands", 4, gui::TABLE_ROW_BG | gui::TABLE_BORDERS | gui::TABLE_SCROLL_Y, gui::v2(0.0, 220.0)) {
                gui::table_setup_column("File", gui::COLUMN_WIDTH_STRETCH, 0.0);
                gui::table_setup_column("Allowed values", gui::COLUMN_WIDTH_FIXED, 100.0);
                gui::table_setup_column("Pack", gui::COLUMN_WIDTH_FIXED, 170.0);
                gui::table_setup_column("##open", gui::COLUMN_WIDTH_FIXED, 50.0);
                gui::table_headers_row();
                for (i, c) in r.candidates.iter().enumerate() {
                    let path = normalize(&c.filename);
                    let owner = vfs.as_ref().and_then(|v| v.owner(&path));
                    gui::push_id_int(i as i32);
                    gui::table_next_row();
                    gui::table_set_column_index(0);
                    gui::text(&path);
                    gui::table_set_column_index(1);
                    if c.flags == 0 {
                        gui::text("any");
                    } else {
                        gui::text(&format!("{} of {total_values}", c.allowed_values));
                    }

                    gui::table_set_column_index(2);
                    match (owner, vfs.as_ref()) {
                        (Some(o), Some(vfs)) => gui::text(&vfs.packs()[o].name),
                        _ => gui::text_colored(theme::error(), "missing in all packs"),
                    }

                    gui::table_set_column_index(3);
                    if owner.is_some() && gui::small_button("Open") {
                        open = Some(path.clone());
                    }

                    gui::pop_id();
                }

                gui::end_table();
            }
        }

        // Reverse lookup for the open document.
        gui::separator();
        if active_environment.is_empty() {
            gui::text_colored(theme::dim(), "Open an .environment file from the pack list to see the conditions under which the game picks it.");
        } else {
            gui::text(&format!("Open file: {active_environment}"));
            let mut found = 0;
            if let Some(env) = db.entry("environment") {
                for v in env.variations() {
                    if normalize(v.filename()) != active_environment {
                        continue;
                    }

                    found += 1;
                    gui::bullet_text("selected when:");
                    for (cat, allowed) in db.allowed_values(*v.flags()) {
                        let s = if allowed.is_empty() {
                            "(nothing - never matches)".to_owned()
                        } else if db.category(cat).is_some_and(|m| allowed.len() == m.values().len()) {
                            "any".to_owned()
                        } else {
                            allowed.join(", ")
                        };

                        gui::text(&format!("    {cat:<22} {s}"));
                    }
                }
            }

            if found == 0 {
                gui::text_colored(theme::dim(), "No variation refers to this file: weather selection never picks it.");
            }
        }

        gui::end_tab_item();
        gui::end_tab_bar();
        gui::end();
        open
    }
}
