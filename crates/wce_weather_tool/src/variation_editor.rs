//! Editing UI for the battle weather selection database (rpfm_lib's asset variation db): value lists per category and the variations
//! (environment file + the conditions it is allowed for).

use std::collections::{BTreeMap, HashMap};

use rpfm_lib::files::asset_variation_db::{AssetVariationDb, Variation};
use wce_editor::{gui, theme};
use wce_vfs::{Vfs, normalize};


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

/// Draws the editor; true when the database changed this frame.
pub fn draw_variation_editor(db: &mut AssetVariationDb, st: &mut VariationEditState, vfs: Option<&Vfs>) -> bool {
    let mut changed = false;

    // ---- Categories and their values
    gui::text(&format!("Values: {} of 32 flag bits in use", db.value_count()));
    if gui::tree_node("Categories and values") {
        for mi in 0..db.categories().len() {
            let category = db.categories()[mi].name().clone();
            gui::push_id_int(mi as i32);
            if gui::tree_node_id(&category, &format!("{category} ({})", db.categories()[mi].values().len())) {
                let mut remove = None;
                let mut rename = None;
                for (vi, value) in db.categories()[mi].values().iter().enumerate() {
                    gui::push_id_int(vi as i32);
                    let mut buf = value.clone();
                    gui::set_next_item_width(260.0);
                    if gui::input_text("##v", None, &mut buf, 128, gui::INPUT_ENTER_RETURNS_TRUE) && buf != *value {
                        rename = Some((value.clone(), buf));
                    }

                    gui::same_line();
                    if gui::small_button("Remove") {
                        remove = Some(value.clone());
                    }

                    gui::pop_id();
                }

                if let Some((from, to)) = rename {
                    match db.rename_value(&category, &from, &to) {
                        Ok(()) => {
                            st.message = format!("Renamed {from} to {to}");
                            changed = true;
                        }

                        Err(why) => st.message = why.to_string(),
                    }
                }

                if let Some(value) = remove {
                    match db.remove_value(&category, &value) {
                        Ok(()) => {
                            st.message = format!("Removed {value}");
                            changed = true;
                        }

                        Err(why) => st.message = why.to_string(),
                    }
                }

                let typed = st.new_value.entry(category.clone()).or_default();
                gui::set_next_item_width(260.0);
                let enter = gui::input_text("##new", Some("new value"), typed, 128, gui::INPUT_ENTER_RETURNS_TRUE);
                gui::same_line();
                if (gui::button("Add value") || enter) && !typed.is_empty() {
                    match db.add_value(&category, typed) {
                        Ok(()) => {
                            st.message = format!("Added {typed}");
                            typed.clear();
                            changed = true;
                        }

                        Err(why) => st.message = why.to_string(),
                    }
                }

                gui::tree_pop();
            }

            gui::pop_id();
        }

        gui::text_colored(theme::dim(), "Removing or adding a value moves the flag bits of later values; every variation keeps allowing the same values.");
        gui::tree_pop();
    }

    // ---- Variations
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
            let filter = st.filter.clone();
            for i in 0..db.entries()[st.entry].variations().len() {
                let (filename, flags) = {
                    let v = &db.entries()[st.entry].variations()[i];
                    (v.filename().clone(), *v.flags())
                };

                if !filter.is_empty() && !filename.contains(&filter) {
                    continue;
                }

                gui::push_id_int(i as i32);
                gui::table_next_row();
                gui::table_set_column_index(0);
                let mut buf = filename.clone();
                let missing = vfs.is_some_and(|vfs| vfs.owner(&normalize(&filename)).is_none());
                if missing {
                    gui::push_style_color(gui::COL_FRAME_BG, theme::v4(theme::palette().error_bg));
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

        // Add
        gui::set_next_item_width(520.0);
        gui::input_text("##newfile", Some("weather\\folder\\file.environment"), &mut st.new_file, 300, 0);
        gui::same_line();
        if gui::button("Add variation") && !st.new_file.is_empty() {
            let flags = db.flags_for(&BTreeMap::new());
            let variations = db.entries_mut()[st.entry].variations_mut();
            variations.push(variation(&st.new_file, flags));
            st.selected = Some(variations.len() - 1);
            st.new_file.clear();
            changed = true;
        }

        // Conditions of the selected variation
        if let Some(sel) = st.selected.filter(|&s| s < db.entries()[st.entry].variations().len()) {
            let (filename, flags) = {
                let v = &db.entries()[st.entry].variations()[sel];
                (v.filename().clone(), *v.flags())
            };

            gui::separator();
            gui::text(&format!("Conditions of {filename}"));
            let mut allowed: BTreeMap<String, Vec<String>> =
                db.allowed_values(flags).into_iter().map(|(cat, values)| (cat.to_owned(), values.into_iter().map(str::to_owned).collect())).collect();
            let mut edited = false;
            for m in db.categories() {
                let list = allowed.entry(m.name().clone()).or_default();
                gui::push_id(m.name());
                gui::text(m.name());
                gui::same_line_at(180.0);
                for value in m.values() {
                    let mut on = list.contains(value);
                    if gui::checkbox(value, &mut on) {
                        if on {
                            list.push(value.clone());
                        } else {
                            list.retain(|x| x != value);
                        }

                        edited = true;
                    }

                    gui::same_line();
                }

                gui::new_line();
                gui::pop_id();
            }

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
                variations.remove(st.selected.unwrap_or(sel));
                st.selected = None;
                changed = true;
            }
        }
    }

    // ---- Problems
    let problems = db.validate();
    if !problems.is_empty() {
        gui::separator();
        gui::text_colored(theme::error(), &format!("{} problem(s) for the game:", problems.len()));
        for p in &problems {
            gui::bullet_text(p);
        }
    }

    if !st.message.is_empty() {
        gui::text(&st.message);
    }

    changed
}
