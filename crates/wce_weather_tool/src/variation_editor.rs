//! Editing UI for the battle weather selection database (rpfm_lib's asset variation db): the flag bits, the categories and their values (the values
//! the game's code sends are locked unless Expert is ticked, see wce_weather::variation_rules), a coverage grid of how many files the game picks from
//! per season, precipitation and time of day, the variations with their conditions, and what the game will make of it all.

use std::collections::{BTreeMap, HashMap};

use rpfm_lib::files::asset_variation_db::{AssetVariationDb, Variation, VariationQuery};
use wce_editor::{gui, theme};
use wce_vfs::{Vfs, normalize};
use wce_weather::variation_rules::{self, DEFAULT_MAP, Role, Severity};

/// The coverage grid: a row per value of `ROWS`, a column per pair of values of `COLUMNS`.
const ROWS: &str = "season";
const COLUMNS: [&str; 2] = ["precipitation", "time_of_day"];

const FLAG_BITS: usize = 32;
const ENGINE_COLOUR: [f32; 4] = [0.45, 0.62, 0.95, 1.0];

#[derive(Clone, Debug, Default)]
pub struct VariationEditState {
    /// Which entry ("environment") is shown.
    pub entry: usize,

    /// Variation row being edited.
    pub selected: Option<usize>,
    pub filter: String,

    /// Text typed into each category's "new value" box.
    pub new_value: HashMap<String, String>,
    pub new_file: String,

    /// Result of the last operation that can fail.
    pub message: String,

    /// Unlocks the values and categories the game's code sends.
    pub expert: bool,

    /// The value being cloned (category, value) and the clone's name.
    clone: Option<(String, String)>,
    clone_name: String,
    new_category: String,
    category_name: HashMap<String, String>,

    /// Coverage grid: the value of every category that is not an axis, and the picked cell (season, precipitation, time of day).
    coverage: VariationQuery,
    cell: Option<[String; 3]>,
    cell_file: String,
}

impl VariationEditState {
    /// Forgets everything about the previous database but the Expert tick.
    pub fn reset(&mut self) {
        *self = Self { expert: self.expert, ..Default::default() };
    }
}

/// A variation of a file with these flags.
pub fn variation(filename: &str, flags: u32) -> Variation {
    let mut v = Variation::default();
    v.set_filename(filename.to_owned());
    v.set_flags(flags);
    v
}

/// "any" or "category: a, b; category: c" for the categories a variation restricts.
fn summary(db: &AssetVariationDb, flags: u32) -> String {
    let mut s = String::new();
    for (cat, allowed) in db.allowed_values(flags) {
        if db.category(cat).is_some_and(|m| allowed.len() == m.values().len()) {
            continue;
        }

        if !s.is_empty() {
            s += "; ";
        }

        s += &format!("{cat}: ");
        if allowed.is_empty() {
            s += "(nothing)";
        }

        s += &allowed.join(", ");
    }

    if s.is_empty() { "any".to_owned() } else { s }
}

fn role_label(role: Role) -> (&'static str, [f32; 4]) {
    match role {
        Role::Engine => ("game", ENGINE_COLOUR),
        Role::Data => ("data", theme::success()),
        Role::Unused => ("unused", theme::warning()),
    }
}

fn role_tooltip(category: &str, value: &str, role: Role) -> String {
    match role {
        Role::Engine => variation_rules::engine_reason(category, Some(value)),
        Role::Data => "A name from the game files (a campaign map or a battle climate): free to add, rename and remove.".to_owned(),
        Role::Unused => "The game never sends this value: it only takes a flag bit.".to_owned(),
    }
}

/// Shows the result of an edit; true when it was made.
fn report<E: ToString>(st: &mut VariationEditState, result: Result<(), E>, done: String) -> bool {
    match result {
        Ok(()) => {
            st.message = done;
            true
        }

        Err(why) => {
            st.message = why.to_string();
            false
        }
    }
}

/// Draws the editor; true when the database changed this frame.
pub fn draw_variation_editor(db: &mut AssetVariationDb, st: &mut VariationEditState, vfs: Option<&Vfs>, tile_climates: Option<&[String]>) -> bool {
    let mut changed = false;
    draw_bits(db);
    gui::same_line();
    gui::checkbox("Expert", &mut st.expert);
    if gui::is_item_hovered() {
        gui::set_tooltip("Unlocks the values and categories the game's code sends (season, precipitation, ...). Changing them costs battles their weather.");
    }

    if !st.message.is_empty() {
        gui::text_colored(theme::warning(), &st.message);
    }

    if db.entries().is_empty() {
        gui::text_colored(theme::error(), "The database has no entries.");
    } else {
        st.entry = st.entry.min(db.entries().len() - 1);
        if db.entries().len() > 1 {
            gui::set_next_item_width(200.0);
            if gui::begin_combo("Entry", db.entries()[st.entry].key()) {
                for i in 0..db.entries().len() {
                    if gui::selectable(db.entries()[i].key(), i == st.entry, 0) {
                        st.entry = i;
                        st.selected = None;
                    }
                }

                gui::end_combo();
            }
        }
    }

    if gui::collapsing_header("Categories and values", gui::TREE_DEFAULT_OPEN) {
        changed |= draw_categories(db, st);
    }

    if !db.entries().is_empty() {
        if gui::collapsing_header("Coverage", 0) {
            changed |= draw_coverage(db, st);
        }

        if gui::collapsing_header("Variations", gui::TREE_DEFAULT_OPEN) {
            changed |= draw_variations(db, st, vfs);
        }
    }

    draw_problems(db, tile_climates);
    changed
}

/// One swatch per flag bit, coloured by what its value is to the game.
fn draw_bits(db: &AssetVariationDb) {
    gui::align_text_to_frame_padding();
    gui::text(&format!("Flag bits: {} of {FLAG_BITS} used", db.value_count()));
    let mut free = theme::dim();
    free[3] = 0.35;
    let values = db.categories().iter().flat_map(|c| c.values().iter().map(move |v| (c.name(), v))).collect::<Vec<_>>();
    for bit in 0..FLAG_BITS.max(values.len()) {
        let (colour, tip) = match values.get(bit) {
            Some((category, value)) => {
                let (label, colour) = role_label(variation_rules::value_role(category, value));
                (colour, format!("bit {bit}: {category} = {value} ({label})"))
            }

            None => (free, format!("bit {bit}: free")),
        };

        gui::same_line();
        gui::color_swatch(&format!("##bit{bit}"), colour, gui::v2(11.0, gui::text_line_height()));
        if gui::is_item_hovered() {
            gui::set_tooltip(&tip);
        }
    }
}

fn draw_categories(db: &mut AssetVariationDb, st: &mut VariationEditState) -> bool {
    let mut changed = false;
    let entry = st.entry.min(db.entries().len().saturating_sub(1));
    let mut first_bit = 0;
    for mi in 0..db.categories().len() {
        let name = db.categories()[mi].name().clone();
        let values = db.categories()[mi].values().clone();
        let engine = variation_rules::engine_category(&name).is_some();
        let data = variation_rules::is_data_category(&name);
        let kind = match (engine, data) {
            (true, true) => "names from the game files",
            (true, false) => "values the game sends",
            (false, _) => "never asked for by the game",
        };

        gui::push_id_int(mi as i32);
        if gui::tree_node_id(&name, &format!("{name} ({}) - {kind}", values.len())) {
            let mut rename = None;
            let mut remove = None;
            if gui::begin_table("values", 5, gui::TABLE_ROW_BG | gui::TABLE_BORDERS_INNER_V | gui::TABLE_SIZING_FIXED_FIT, gui::v2(0.0, 0.0)) {
                gui::table_setup_column("Value", gui::COLUMN_WIDTH_FIXED, 240.0);
                gui::table_setup_column("Bit", gui::COLUMN_WIDTH_FIXED, 30.0);
                gui::table_setup_column("Allowed by", gui::COLUMN_WIDTH_FIXED, 90.0);
                gui::table_setup_column("Role", gui::COLUMN_WIDTH_FIXED, 60.0);
                gui::table_setup_column("##actions", gui::COLUMN_WIDTH_FIXED, 130.0);
                gui::table_headers_row();
                for (vi, value) in values.iter().enumerate() {
                    let role = variation_rules::value_role(&name, value);
                    let editable = role != Role::Engine || st.expert;
                    gui::push_id_int(vi as i32);
                    gui::table_next_row();
                    gui::table_set_column_index(0);
                    if editable {
                        let mut buf = value.clone();
                        gui::set_next_item_width(-1.0);
                        if gui::input_text("##v", None, &mut buf, 128, gui::INPUT_ENTER_RETURNS_TRUE) && buf != *value {
                            rename = Some((value.clone(), buf));
                        }

                        if gui::is_item_hovered() {
                            gui::set_tooltip("Type a new name and press Enter to rename");
                        }
                    } else {
                        gui::align_text_to_frame_padding();
                        gui::text(value);
                    }

                    gui::table_set_column_index(1);
                    gui::text(&(first_bit + vi).to_string());
                    gui::table_set_column_index(2);
                    if let Some(entry) = db.entries().get(entry) {
                        let bit = db.bit(&name, value);
                        let n = entry.variations().iter().filter(|v| *v.flags() == 0 || v.flags() & bit != 0).count();
                        gui::text(&format!("{n} of {}", entry.variations().len()));
                    }

                    gui::table_set_column_index(3);
                    let (label, colour) = role_label(role);
                    gui::text_colored(colour, label);
                    if gui::is_item_hovered() {
                        gui::set_tooltip(&role_tooltip(&name, value, role));
                    }

                    gui::table_set_column_index(4);
                    if data && gui::small_button("Clone") {
                        st.clone = Some((name.clone(), value.clone()));
                        st.clone_name.clear();
                    }

                    if data && gui::is_item_hovered() {
                        gui::set_tooltip(&format!("A new {name} with the weather of '{value}': every variation that allows '{value}' allows it too"));
                    }

                    if data {
                        gui::same_line();
                    }

                    gui::begin_disabled(!editable);
                    if gui::small_button("Remove") {
                        remove = Some(value.clone());
                    }

                    gui::end_disabled();
                    if !editable && gui::is_item_hovered_allow_disabled() {
                        gui::set_tooltip(&format!("Locked. {} Tick Expert to change it anyway.", variation_rules::engine_reason(&name, Some(value))));
                    }

                    gui::pop_id();
                }

                gui::end_table();
            }

            if let Some((from, to)) = rename {
                changed |= report(st, db.rename_value(&name, &from, &to), format!("Renamed {from} to {to}"));
            }

            if let Some(value) = remove {
                let warn = if variation_rules::value_role(&name, &value) == Role::Engine { " - the game still sends it, see Problems" } else { "" };
                changed |= report(st, db.remove_value(&name, &value), format!("Removed {value}{warn}"));
            }

            changed |= draw_clone(db, st, &name);

            // New values: free for data and the categories the game never asks for, expert-only where the game's code decides the values.
            if data || !engine || st.expert {
                let typed = st.new_value.entry(name.clone()).or_default();
                gui::set_next_item_width(240.0);
                let enter = gui::input_text("##new", Some("new value"), typed, 128, gui::INPUT_ENTER_RETURNS_TRUE);
                gui::same_line();
                if (gui::button("Add value") || enter) && !typed.is_empty() {
                    let typed = std::mem::take(typed);
                    let result = db.add_value(&name, &typed);
                    if result.is_err() {
                        st.new_value.insert(name.clone(), typed.clone());
                    }

                    let note = if engine && !data { " - the game never sends it" } else { "" };
                    changed |= report(st, result, format!("Added {typed}{note}"));
                }
            } else {
                gui::text_colored(theme::dim(), "The game only sends these values; a new one would never be asked for.");
            }

            if !engine || st.expert {
                let typed = st.category_name.entry(name.clone()).or_insert_with(|| name.clone());
                gui::set_next_item_width(240.0);
                let enter = gui::input_text("##rename_category", None, typed, 128, gui::INPUT_ENTER_RETURNS_TRUE);
                gui::same_line();
                if (gui::button("Rename category") || enter) && *typed != name {
                    let to = typed.clone();
                    changed |= report(st, db.rename_category(&name, &to), format!("Renamed category {name} to {to}"));
                }

                gui::same_line();
                if gui::button("Remove category") {
                    changed |= report(st, db.remove_category(&name), format!("Removed category {name}"));
                }
            }

            gui::tree_pop();
        }

        gui::pop_id();
        first_bit += values.len();
    }

    if st.expert {
        gui::set_next_item_width(240.0);
        let enter = gui::input_text("##newcat", Some("new category"), &mut st.new_category, 128, gui::INPUT_ENTER_RETURNS_TRUE);
        gui::same_line();
        if (gui::button("Add category") || enter) && !st.new_category.is_empty() {
            let name = std::mem::take(&mut st.new_category);
            changed |= report(st, db.add_category(&name), format!("Added category {name} - the game never asks for it"));
        }
    }

    gui::text_colored(theme::dim(), "Adding or removing a value moves the flag bits of later values; every variation keeps allowing the same values.");
    changed
}

/// The clone row under a category, while one of its values is being cloned.
fn draw_clone(db: &mut AssetVariationDb, st: &mut VariationEditState, category: &str) -> bool {
    let Some((_, from)) = st.clone.clone().filter(|(c, _)| c == category) else { return false };
    gui::align_text_to_frame_padding();
    gui::text(&format!("New {category} with the weather of '{from}':"));
    gui::same_line();
    gui::set_next_item_width(200.0);
    let enter = gui::input_text("##clone", Some("name"), &mut st.clone_name, 128, gui::INPUT_ENTER_RETURNS_TRUE);
    gui::same_line();
    let mut changed = false;
    if (gui::button("Create") || enter) && !st.clone_name.is_empty() {
        let name = st.clone_name.clone();
        let hint = if category == "climate" { " (the game asks for it once the battle tile database has a climate of that name)" } else { "" };
        changed = report(st, db.clone_value(category, &from, &name), format!("Added {name} with the weather of {from}{hint}"));
        if changed {
            st.clone = None;
        }
    }

    gui::same_line();
    if gui::button("Cancel") {
        st.clone = None;
    }

    changed
}

/// Grid of the number of files the game picks from per season, precipitation and time of day, for one value of every other category.
fn draw_coverage(db: &mut AssetVariationDb, st: &mut VariationEditState) -> bool {
    let (Some(rows), Some(c0), Some(c1)) = (db.category(ROWS), db.category(COLUMNS[0]), db.category(COLUMNS[1])) else {
        gui::text_colored(theme::dim(), &format!("The grid needs the categories {ROWS}, {} and {}.", COLUMNS[0], COLUMNS[1]));
        return false;
    };

    let (rows, c0, c1) = (rows.values().clone(), c0.values().clone(), c1.values().clone());
    let key = db.entries()[st.entry].key().clone();

    // The other categories: one value each.
    let mut on_line = 0;
    for category in db.categories().iter().filter(|c| c.name() != ROWS && !COLUMNS.contains(&c.name().as_str()) && !c.values().is_empty()) {
        let name = category.name();
        let values = category.values();
        let default = if values.iter().any(|v| v == DEFAULT_MAP) { DEFAULT_MAP.to_owned() } else { values[0].clone() };
        let current = st.coverage.entry(name.clone()).or_insert(default);
        if !values.contains(current) {
            *current = values[0].clone();
        }

        if on_line > 0 {
            gui::same_line();
        }

        gui::set_next_item_width(170.0);
        if gui::begin_combo(&format!("{name}##cov"), current) {
            for v in values {
                if gui::selectable(v, v == current, 0) {
                    *current = v.clone();
                }
            }

            gui::end_combo();
        }

        on_line = (on_line + 1) % 2;
    }

    st.coverage.retain(|name, _| db.category(name).is_some());
    let query = |cell: &[String; 3]| {
        let mut q = st.coverage.clone();
        q.insert(ROWS.to_owned(), cell[0].clone());
        q.insert(COLUMNS[0].to_owned(), cell[1].clone());
        q.insert(COLUMNS[1].to_owned(), cell[2].clone());
        q
    };

    let map = st.coverage.get("map").cloned().unwrap_or_default();
    let columns = c0.iter().flat_map(|a| c1.iter().map(move |b| (a.clone(), b.clone()))).collect::<Vec<_>>();
    if gui::begin_table("coverage", 1 + columns.len() as i32, gui::TABLE_BORDERS | gui::TABLE_SIZING_FIXED_FIT, gui::v2(0.0, 0.0)) {
        gui::table_setup_column(ROWS, gui::COLUMN_WIDTH_FIXED, 40.0);
        for (a, b) in &columns {
            gui::table_setup_column(&format!("{a}\n{b}"), gui::COLUMN_WIDTH_FIXED, 44.0);
        }

        gui::table_headers_row();
        for row in &rows {
            gui::table_next_row();
            gui::table_set_column_index(0);
            gui::text(row);
            for (ci, (a, b)) in columns.iter().enumerate() {
                gui::table_set_column_index(1 + ci as i32);
                let cell = [row.clone(), a.clone(), b.clone()];
                let m = db.matches(&key, &query(&cell));
                let fallback = m.used_default_map && map != DEFAULT_MAP && !m.variations.is_empty();
                let label = format!("{}{}##{row}{ci}", m.variations.len(), if fallback { "*" } else { "" });
                let colour = if m.variations.is_empty() { theme::error() } else if fallback { theme::warning() } else { theme::text() };
                gui::push_style_color(gui::COL_TEXT, theme::v4(colour));
                if gui::selectable(&label, st.cell.as_ref() == Some(&cell), 0) {
                    st.cell = Some(cell.clone());
                }

                gui::pop_style_color(1);
                if gui::is_item_hovered() {
                    let mut tip = format!("{row}, {a}, {b}: ");
                    tip += &match m.variations.len() {
                        0 => "no weather from the database".to_owned(),
                        n => format!("{n} file(s), each picked 1 in {n}{}", if fallback { ", after asking again with default_map" } else { "" }),
                    };

                    for v in m.variations.iter().take(12) {
                        tip += &format!("\n{}", v.filename());
                    }

                    gui::set_tooltip(&tip);
                }
            }
        }

        gui::end_table();
    }

    gui::text_colored(theme::dim(), "Files the game picks from. * only after asking again with default_map; red: no weather. Click a cell for its files.");

    // The picked cell: its files, and a new one for exactly its conditions.
    let Some(cell) = st.cell.clone().filter(|c| rows.contains(&c[0]) && c0.contains(&c[1]) && c1.contains(&c[2])) else { return false };
    let q = query(&cell);
    gui::separator();
    gui::text(&format!("{} - {} - {}:", cell[0], cell[1], cell[2]));
    let entry = &db.entries()[st.entry];
    let picked = db.matches(&key, &q).variations.iter().map(|v| *v as *const Variation).collect::<Vec<_>>();
    for (i, v) in entry.variations().iter().enumerate().filter(|(_, v)| picked.contains(&(*v as *const Variation))) {
        gui::push_id_int(i as i32);
        if gui::small_button("edit") {
            st.selected = Some(i);
        }

        gui::same_line();
        gui::text(v.filename());
        gui::pop_id();
    }

    gui::set_next_item_width(420.0);
    gui::input_text("##cellfile", Some("weather\\folder\\file.environment"), &mut st.cell_file, 300, 0);
    gui::same_line();
    if gui::button("Add file here") && !st.cell_file.is_empty() {
        let allowed: BTreeMap<String, Vec<String>> = q.iter().map(|(k, v)| (k.clone(), vec![v.clone()])).collect();
        let flags = db.flags_for(&allowed);
        let variations = db.entries_mut()[st.entry].variations_mut();
        variations.push(variation(&std::mem::take(&mut st.cell_file), flags));
        st.selected = Some(variations.len() - 1);
        st.message = "Added a file for exactly these conditions; widen them under Variations".to_owned();
        return true;
    }

    if gui::is_item_hovered() {
        gui::set_tooltip("The new variation allows exactly the values shown above the grid and this cell's; widen its conditions under Variations.");
    }

    false
}

fn draw_variations(db: &mut AssetVariationDb, st: &mut VariationEditState, vfs: Option<&Vfs>) -> bool {
    let mut changed = false;
    gui::set_next_item_width(260.0);
    gui::input_text("##filter", Some("filter variations"), &mut st.filter, 128, 0);
    gui::same_line();
    gui::text(&format!("{} variations", db.entries()[st.entry].variations().len()));

    if gui::begin_table("vars", 3, gui::TABLE_ROW_BG | gui::TABLE_BORDERS | gui::TABLE_SCROLL_Y | gui::TABLE_RESIZABLE, gui::v2(0.0, 260.0)) {
        gui::table_setup_scroll_freeze(0, 1);
        gui::table_setup_column("File", gui::COLUMN_WIDTH_STRETCH, 3.0);
        gui::table_setup_column("Allowed when", gui::COLUMN_WIDTH_STRETCH, 2.0);
        gui::table_setup_column("##sel", gui::COLUMN_WIDTH_FIXED, 56.0);
        gui::table_headers_row();
        let filter = st.filter.to_lowercase();
        for i in 0..db.entries()[st.entry].variations().len() {
            let (filename, flags) = {
                let v = &db.entries()[st.entry].variations()[i];
                (v.filename().clone(), *v.flags())
            };

            if !filter.is_empty() && !filename.to_lowercase().contains(&filter) {
                continue;
            }

            gui::push_id_int(i as i32);
            gui::table_next_row();
            gui::table_set_column_index(0);
            let mut buf = filename.clone();
            let missing = vfs.is_some_and(|vfs| vfs.owner(&normalize(&filename)).is_none());
            if missing {
                gui::push_style_color(gui::COL_FRAME_BG, gui::v4(0.45, 0.15, 0.15, 1.0));
            }

            gui::set_next_item_width(-1.0);
            if gui::input_text("##f", None, &mut buf, 300, 0) && buf != filename {
                db.entries_mut()[st.entry].variations_mut()[i].set_filename(buf);
                changed = true;
            }

            if missing {
                gui::pop_style_color(1);
                if gui::is_item_hovered() {
                    gui::set_tooltip("This file is in no pack");
                }
            }

            gui::table_set_column_index(1);
            gui::text(&summary(db, flags));
            gui::table_set_column_index(2);
            if gui::small_button(if st.selected == Some(i) { "editing" } else { "edit" }) {
                st.selected = Some(i);
            }

            gui::pop_id();
        }

        gui::end_table();
    }

    gui::set_next_item_width(520.0);
    gui::input_text("##newfile", Some("weather\\folder\\file.environment"), &mut st.new_file, 300, 0);
    gui::same_line();
    if gui::button("Add variation") && !st.new_file.is_empty() {
        let flags = db.flags_for(&BTreeMap::new());
        let variations = db.entries_mut()[st.entry].variations_mut();
        variations.push(variation(&std::mem::take(&mut st.new_file), flags));
        st.selected = Some(variations.len() - 1);
        changed = true;
    }

    if let Some(sel) = st.selected.filter(|&s| s < db.entries()[st.entry].variations().len()) {
        changed |= draw_conditions(db, st, sel);
    }

    changed
}

/// The selected variation's conditions: a row per category with a checkbox per value.
fn draw_conditions(db: &mut AssetVariationDb, st: &mut VariationEditState, sel: usize) -> bool {
    let (filename, flags) = {
        let v = &db.entries()[st.entry].variations()[sel];
        (v.filename().clone(), *v.flags())
    };

    gui::separator_text(&format!("Conditions of {filename}"));
    let mut allowed: BTreeMap<String, Vec<String>> =
        db.allowed_values(flags).into_iter().map(|(cat, values)| (cat.to_owned(), values.into_iter().map(str::to_owned).collect())).collect();
    let mut edited = false;
    if gui::begin_table("conditions", 2, gui::TABLE_ROW_BG | gui::TABLE_BORDERS_INNER_V, gui::v2(0.0, 0.0)) {
        gui::table_setup_column("Category", gui::COLUMN_WIDTH_FIXED, 230.0);
        gui::table_setup_column("Allowed values", gui::COLUMN_WIDTH_STRETCH, 1.0);
        for m in db.categories() {
            let list = allowed.entry(m.name().clone()).or_default();
            gui::push_id(m.name());
            gui::table_next_row();
            gui::table_set_column_index(0);
            gui::align_text_to_frame_padding();
            gui::text(m.name());
            gui::same_line_at(150.0);
            if gui::small_button("All") {
                *list = m.values().clone();
                edited = true;
            }

            gui::same_line();
            if gui::small_button("None") {
                list.clear();
                edited = true;
            }

            gui::table_set_column_index(1);
            let width = gui::content_region_avail().x;
            let mut used = 0.0;
            for value in m.values() {
                let w = gui::calc_text_width(value) + gui::text_line_height() + 3.0 * gui::style_frame_padding().x + gui::item_spacing().x;
                if used > 0.0 && used + w <= width {
                    gui::same_line();
                } else {
                    used = 0.0;
                }

                used += w;
                let mut on = list.contains(value);
                if gui::checkbox(value, &mut on) {
                    if on {
                        list.push(value.clone());
                    } else {
                        list.retain(|x| x != value);
                    }

                    edited = true;
                }
            }

            gui::pop_id();
        }

        gui::end_table();
    }

    let mut changed = false;
    let mut flags = flags;
    if edited {
        flags = db.flags_for(&allowed);
        db.entries_mut()[st.entry].variations_mut()[sel].set_flags(flags);
        changed = true;
    }

    gui::text_colored(theme::dim(), &format!("flags {flags} (0x{flags:08X})"));
    gui::same_line();
    let variations = db.entries_mut()[st.entry].variations_mut();
    if gui::small_button("Any (flags 0)") {
        variations[sel].set_flags(0);
        changed = true;
    }

    gui::same_line();
    if gui::small_button("Duplicate") {
        let copy = variations[sel].clone();
        variations.insert(sel + 1, copy);
        st.selected = Some(sel + 1);
        changed = true;
    }

    gui::same_line();
    if gui::small_button("Delete") {
        variations.remove(sel);
        st.selected = None;
        changed = true;
    }

    changed
}

/// The file's own problems, then what the game will make of it (variation_rules::check).
fn draw_problems(db: &AssetVariationDb, tile_climates: Option<&[String]>) {
    let problems = db.validate();
    let findings = variation_rules::check(db, tile_climates);
    let notes = findings.iter().filter(|f| f.severity == Severity::Info).collect::<Vec<_>>();
    gui::separator();
    if problems.is_empty() && notes.len() == findings.len() {
        gui::text_colored(theme::success(), "No problems for the game.");
    }

    for p in &problems {
        gui::text_colored(theme::error(), "!");
        gui::same_line();
        gui::text_wrapped(p);
    }

    for f in findings.iter().filter(|f| f.severity != Severity::Info) {
        gui::text_colored(if f.severity == Severity::Error { theme::error() } else { theme::warning() }, "!");
        gui::same_line();
        gui::text_wrapped(&f.text);
    }

    if !notes.is_empty() && gui::tree_node(&format!("Notes ({})", notes.len())) {
        for f in notes {
            gui::bullet_text(&f.text);
        }

        gui::tree_pop();
    }
}

/// True when the game would lose weather with this database: problems of the file or engine values it lacks.
pub fn blocks_saving(db: &AssetVariationDb, tile_climates: Option<&[String]>) -> bool {
    !db.validate().is_empty() || variation_rules::check(db, tile_climates).iter().any(|f| f.severity == Severity::Error)
}
