//! The vegetation sets in the Tile tool (`wce_tile::vegetation_sets`): read from the projects' `raw_data/art/vegetation_sets`, placed and
//! painted from the Create panel (a key picked by the weights for each tree), made from the selected trees or from scratch (Ctrl + click on
//! trees: the new set, not saved), and edited in the Inspector while one is in use (a saved one is saved at once). What is placed points to
//! the set (`SetInUse`), read again for every tree.

use std::sync::Arc;

use wce_editor::editor::Editor;
use wce_editor::{gui, icons, theme};
use wce_tile::vegetation_sets::{SetEntry, VegetationSet, scan, sets_root};

use crate::tool::{SetInUse, TileTool, document, selected};

impl TileTool {
    /// The folder of the sets: the projects'.
    fn sets_folder(e: &mut Editor) -> std::path::PathBuf {
        sets_root(&Self::workspace_dir(e).join("raw_data"))
    }

    /// The sets, read on first use and again after a change.
    pub(crate) fn vegetation_sets(&mut self, e: &mut Editor) -> Arc<Vec<VegetationSet>> {
        if let Some(sets) = &self.veg_sets {
            return sets.clone();
        }

        let sets = Arc::new(scan(&Self::sets_folder(e)));
        self.veg_sets = Some(sets.clone());
        sets
    }

    /// The set being placed or painted, as it is now.
    pub(crate) fn set_in_use(&mut self, e: &mut Editor) -> Option<VegetationSet> {
        match self.veg_set.clone()? {
            SetInUse::New => self.scratch_set.clone(),
            SetInUse::Saved(name) => self.vegetation_sets(e).iter().find(|s| s.name == name).cloned(),
        }
    }

    /// The sets changed: read them again (the Create panel's entries follow).
    pub(crate) fn rescan_vegetation_sets(&mut self) {
        self.veg_sets = None;
        self.veg_sets_version += 1;
    }

    /// The keys of the selected trees (in the selected groups and layers too), once per tree.
    fn selected_tree_keys(&self, e: &mut Editor) -> Vec<String> {
        let sel = selected(e);
        let Some(d) = document(e) else { return vec![] };
        let sc = d.tile().tscene();
        d.tile().instances_below(&sel).into_iter().filter(|&el| sc.type_of(el) == "VEGETATION_INSTANCE").filter_map(|el| sc.doc().get(el, "key")).collect()
    }

    /// A new set from the selected trees (each key weighted by how many there are), in the projects' sets.
    ///
    /// # Errors
    ///
    /// No name, no tree selected, a set of that name already, or the file can't be written.
    pub fn new_vegetation_set(&mut self, e: &mut Editor, name: &str) -> Result<String, String> {
        let name = set_file_name(name)?;
        if self.vegetation_sets(e).iter().any(|s| s.name.eq_ignore_ascii_case(name)) {
            return Err(format!("a vegetation set '{name}' already exists"));
        }

        let keys = self.selected_tree_keys(e);
        if keys.is_empty() {
            return Err("select some trees first".to_owned());
        }

        let mut set = VegetationSet::from_keys(name, keys.iter().map(String::as_str));
        set.path = Self::sets_folder(e).join(format!("{name}.wce"));
        set.save()?;
        self.rescan_vegetation_sets();
        Ok(format!("vegetation set {name}: {} kind(s) of tree from {} tree(s) ({})", set.entries.len(), keys.len(), set.path.display()))
    }

    /// Asks for the name of a new set from the selected trees (the dialog opens on the next frame).
    pub(crate) fn request_new_vegetation_set(&mut self) {
        self.new_set_name = Some(String::new());
        self.new_set_open = true;
    }

    /// Whether some trees are selected (the "New vegetation set" action).
    pub(crate) fn trees_selected(&self, e: &mut Editor) -> bool {
        !self.selected_tree_keys(e).is_empty()
    }

    /// The "New vegetation set" dialog.
    pub(crate) fn draw_new_set_popup(&mut self, e: &mut Editor) {
        if std::mem::take(&mut self.new_set_open) {
            gui::open_popup("New vegetation set");
        }

        if !gui::begin_popup_modal("New vegetation set", None, gui::WINDOW_ALWAYS_AUTO_RESIZE) {
            return;
        }

        let Some(mut name) = self.new_set_name.clone() else {
            gui::close_current_popup();
            gui::end_popup();
            return;
        };

        gui::text_wrapped("A set of the selected trees (each kind weighted by how many there are), saved in the projects' vegetation sets.");
        gui::set_next_item_width(320.0);
        let enter = gui::input_text("Name", Some("the set's file name"), &mut name, 128, gui::INPUT_ENTER_RETURNS_TRUE);
        self.new_set_name = Some(name.clone());
        if gui::button("Save") || enter {
            self.status = self.new_vegetation_set(e, &name).unwrap_or_else(|err| err);
            self.new_set_name = None;
            gui::close_current_popup();
        }

        gui::same_line();
        if gui::button("Cancel") || gui::is_key_pressed(gui::KEY_ESCAPE, true) {
            self.new_set_name = None;
            gui::close_current_popup();
        }

        gui::end_popup();
    }

    /// The saved set in use, in the Inspector (its box): its trees, their weights and scale ranges, remove; saved at once (the new one has
    /// its own box, `inspect_new_set`).
    pub(crate) fn inspect_veg_set(&mut self, e: &mut Editor) {
        if !matches!(self.veg_set, Some(SetInUse::Saved(_))) {
            return;
        }

        let Some(mut set) = self.set_in_use(e) else { return };
        let before = set.clone();
        if begin_set_box("##savedset", &format!("Vegetation set {}", set.name), "saved at once") {
            set_rows(&mut set);
        }

        end_set_box();
        if set == before {
            return;
        }

        match set.save() {
            Ok(()) => self.rescan_vegetation_sets(),
            Err(err) => self.status = err,
        }
    }

    /// Adds trees to the new set (Ctrl + click in the Create panel): a key, or every tree of a set; one there already weighs one more. Then
    /// places or paints with it, if that is not being done already.
    pub(crate) fn add_to_new_set(&mut self, e: &mut Editor, keys: &[SetEntry]) {
        let set = self.scratch_set.get_or_insert_with(VegetationSet::default);
        for k in keys {
            match set.entries.iter_mut().find(|en| en.key == k.key) {
                Some(en) => en.weight += k.weight,
                None => set.entries.push(k.clone()),
            }
        }

        let kinds = set.entries.len();
        if self.veg_set != Some(SetInUse::New) {
            self.start_new_set(e);
        }

        self.status = format!("the new vegetation set has {kinds} kind(s) of tree (the Inspector); click the terrain to place, Esc stops");
    }

    /// The new set (not saved), in its box at the top of the Inspector: its trees, a name to save it with, placing with it, dropping it.
    pub(crate) fn inspect_new_set(&mut self, e: &mut Editor) {
        let Some(mut set) = self.scratch_set.clone() else { return };
        let (mut save, mut use_it, mut drop) = (false, false, false);
        if begin_set_box("##newset", "New vegetation set", "not saved") {
            set_rows(&mut set);
            gui::spacing();
            let mut name = std::mem::take(&mut self.scratch_name);
            gui::set_next_item_width(-1.0);
            let enter = gui::input_text("##setname", Some("its name, to save it"), &mut name, 128, gui::INPUT_ENTER_RETURNS_TRUE);
            let can_save = !name.trim().is_empty() && !set.entries.is_empty();
            gui::begin_disabled(!can_save);
            save = gui::small_button(&format!("{}  Save", icons::SAVE)) || (enter && can_save);
            gui::end_disabled();

            // Placing with it already from the first tree; again after Esc.
            if self.veg_set != Some(SetInUse::New) {
                gui::same_line();
                gui::begin_disabled(set.entries.is_empty());
                use_it = gui::small_button(&format!("{}  Place / paint", icons::MOUSE_POINTER_2));
                gui::end_disabled();
            }

            gui::same_line();
            drop = gui::small_button(&format!("{}  Discard", icons::X));
            self.scratch_name = name;
        }

        end_set_box();
        self.scratch_set = Some(set);

        if save {
            self.status = self.save_new_set(e).unwrap_or_else(|err| err);
        } else if use_it {
            self.start_new_set(e);
        } else if drop {
            // Placed or painted with: that stops too.
            if self.veg_set == Some(SetInUse::New) {
                self.stop_making();
            }

            self.scratch_set = None;
            self.scratch_name.clear();
        }
    }

    /// Saves the new set under its name, in the projects' sets; placing or painting with it goes on with the saved one.
    ///
    /// # Errors
    ///
    /// No name or trees, a set of that name already, or the file can't be written.
    pub fn save_new_set(&mut self, e: &mut Editor) -> Result<String, String> {
        let name = set_file_name(&self.scratch_name)?.to_owned();
        let mut set = self.scratch_set.clone().filter(|s| !s.entries.is_empty()).ok_or("the new set has no trees")?;
        if self.vegetation_sets(e).iter().any(|s| s.name.eq_ignore_ascii_case(&name)) {
            return Err(format!("a vegetation set '{name}' already exists"));
        }

        set.name.clone_from(&name);
        set.path = Self::sets_folder(e).join(format!("{name}.wce"));
        set.save()?;
        self.scratch_set = None;
        self.scratch_name.clear();
        self.rescan_vegetation_sets();
        if self.veg_set == Some(SetInUse::New) {
            self.veg_set = Some(SetInUse::Saved(name.clone()));
        }

        Ok(format!("vegetation set {name} saved ({})", set.path.display()))
    }

    /// Places or paints with the new set (not saved).
    pub(crate) fn start_new_set(&mut self, e: &mut Editor) {
        let en = crate::create_panel::Entry::new_set_entry();
        self.start_making(e, &en);
    }
}

/// A set's name as its file's: trimmed, not empty, with no character or name a file system refuses (Windows' the strictest).
fn set_file_name(name: &str) -> Result<&str, String> {
    const RESERVED: [&str; 22] = ["CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8", "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9"];
    let name = name.trim();
    let bad = name.is_empty()
        || name.ends_with('.')
        || name.chars().any(|c| c.is_control() || ['/', '\\', ':', '*', '?', '"', '<', '>', '|'].contains(&c))
        || RESERVED.iter().any(|r| name.split('.').next().is_some_and(|stem| stem.eq_ignore_ascii_case(r)));
    if bad {
        return Err(format!("'{name}' can't be a file name: give the set another"));
    }

    Ok(name)
}

/// A set's box in the Inspector: framed, so that it reads as one thing (not more of the Inspector's sections), with its title and a note.
fn begin_set_box(id: &str, title: &str, note: &str) -> bool {
    gui::push_style_color(gui::COL_CHILD_BG, gui::v4(1.0, 1.0, 1.0, 0.035));
    let open = gui::begin_child(id, gui::v2(0.0, 0.0), gui::CHILD_BORDERS | gui::CHILD_AUTO_RESIZE_Y, 0);
    gui::pop_style_color(1);
    if open {
        gui::text_colored(theme::palette().highlight, &format!("{}  {title}", icons::TREES));
        gui::same_line();
        gui::text_disabled(&format!("({note})"));
    }

    open
}

fn end_set_box() {
    gui::end_child();
    gui::spacing();
}

/// A set's trees, one row each: the key, its weight, its own scale range (else the brush's), remove; and how to add more.
fn set_rows(set: &mut VegetationSet) {
    let mut remove = None;
    if !set.entries.is_empty() && gui::begin_table("trees", 4, gui::TABLE_ROW_BG | gui::TABLE_SIZING_FIXED_FIT, gui::v2(0.0, 0.0)) {
        gui::table_setup_column("tree", gui::COLUMN_WIDTH_STRETCH, 0.0);
        gui::table_setup_column("weight", gui::COLUMN_WIDTH_FIXED, 56.0);
        gui::table_setup_column("own scale", gui::COLUMN_WIDTH_FIXED, 150.0);
        gui::table_setup_column("", gui::COLUMN_WIDTH_FIXED, 0.0);
        gui::table_headers_row();
        for (i, en) in set.entries.iter_mut().enumerate() {
            gui::push_id_int(i as i32);
            gui::table_next_row();
            gui::table_next_column();
            gui::text(&en.key);
            gui::table_next_column();
            gui::set_next_item_width(-1.0);
            gui::drag_float("##weight", &mut en.weight, 0.05, 0.0, 10.0, "%.2f");
            if gui::is_item_hovered() {
                gui::set_tooltip("How often it is picked, against the others' weights");
            }

            gui::table_next_column();
            let mut own = en.scale.is_some();
            if gui::checkbox("##own", &mut own) {
                en.scale = own.then_some((0.85, 1.15));
            }

            if gui::is_item_hovered() {
                gui::set_tooltip("A scale range of its own (else the brush's)");
            }

            gui::same_line();
            if let Some((lo, hi)) = en.scale.as_mut() {
                let mut v = [*lo, *hi];
                gui::set_next_item_width(-1.0);
                if gui::drag_float2("##scale", &mut v, 0.01, "%.2f") {
                    (*lo, *hi) = (v[0].clamp(0.1, 3.0), v[1].clamp(0.1, 3.0));
                }
            } else {
                gui::text_disabled("brush's");
            }

            gui::table_next_column();
            if gui::small_button(icons::TRASH_2) {
                remove = Some(i);
            }

            if gui::is_item_hovered() {
                gui::set_tooltip(&format!("Take {} out of the set", en.key));
            }

            gui::pop_id();
        }

        gui::end_table();
    }

    if let Some(i) = remove {
        set.entries.remove(i);
    }

    gui::push_style_color(gui::COL_TEXT, gui::style_color(gui::COL_TEXT_DISABLED));
    gui::text_wrapped("Ctrl + click or Ctrl + Enter on a tree in Create or Shift+A adds it here.");
    gui::pop_style_color(1);
}
