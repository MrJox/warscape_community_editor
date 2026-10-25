//! The vegetation sets in the Tile tool (`wce_tile::vegetation_sets`): read from the projects' `raw_data/art/vegetation_sets`, placed and
//! painted from the Create panel (a key picked by the weights for each tree), made from the selected trees or from scratch (Ctrl + click on
//! trees: the new set, not saved), and edited in the Inspector while one is in use (a saved one is saved at once). What is placed points to
//! the set (`SetInUse`), read again for every tree.

use std::sync::Arc;

use wce_editor::editor::Editor;
use wce_editor::{gui, icons};
use wce_editor_core::inspector::Inspector;
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

    /// The rows of a set's trees: weight, own scale range, remove; and a key to add (typed in the new set's field or the saved one's).
    /// Returns the set as edited.
    fn edit_set_entries(&mut self, e: &mut Editor, ins: &mut dyn Inspector, mut set: VegetationSet, new: bool) -> VegetationSet {
        let mut remove = None;
        for (i, en) in set.entries.iter_mut().enumerate() {
            gui::push_id_int(i as i32);
            ins.text("key", "tree", &en.key);
            ins.edit_slider("weight", "weight", &mut en.weight, 0.0, 10.0);
            let mut own = en.scale.is_some();
            if ins.edit_bool("own_scale", "own scale range", &mut own) {
                en.scale = own.then_some((0.85, 1.15));
            }

            if let Some((lo, hi)) = en.scale.as_mut() {
                ins.edit_slider("scale_min", "scale from", lo, 0.1, 3.0);
                ins.edit_slider("scale_max", "scale to", hi, 0.1, 3.0);
            }

            if gui::small_button(&format!("{}  Remove {}", icons::TRASH_2, en.key)) {
                remove = Some(i);
            }

            gui::separator();
            gui::pop_id();
        }

        if let Some(i) = remove {
            set.entries.remove(i);
        }

        // Another tree: its key (a tree of the game's files; Ctrl + click in the Create panel adds one too).
        let mut key = std::mem::take(if new { &mut self.scratch_key } else { &mut self.new_set_key });
        ins.edit_string("new_key", "add a tree (species.variant)", &mut key);
        let known = self.catalogue(e, "VEGETATION_INSTANCE").iter().any(|k| *k == key.trim());
        gui::begin_disabled(!known || set.entries.iter().any(|en| en.key == key.trim()));
        if gui::small_button(&format!("{}  Add", icons::PLUS)) {
            set.entries.push(SetEntry { key: key.trim().to_owned(), weight: 1.0, scale: None });
            key.clear();
        }

        gui::end_disabled();
        *(if new { &mut self.scratch_key } else { &mut self.new_set_key }) = key;
        set
    }

    /// The saved set in use, in the Inspector: its trees, their weights and scale ranges, add and remove; saved at once (the new one is
    /// edited at the top of the Inspector, `inspect_new_set`).
    pub(crate) fn inspect_veg_set(&mut self, e: &mut Editor, ins: &mut dyn Inspector) {
        if !matches!(self.veg_set, Some(SetInUse::Saved(_))) {
            return;
        }

        let Some(set) = self.set_in_use(e) else { return };
        if let Some(d) = document(e) {
            ins.begin(wce_editor_core::document::Document::id(d), u64::MAX - 12);
        }

        let before = set.clone();
        ins.section(&format!("Vegetation set {}", set.name));
        let set = self.edit_set_entries(e, ins, set, false);
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

    /// The new set (not saved), at the top of the Inspector: its trees, a name to save it with, using it, dropping it (and placing with it).
    pub(crate) fn inspect_new_set(&mut self, e: &mut Editor, ins: &mut dyn Inspector) {
        let Some(set) = self.scratch_set.clone() else { return };
        if let Some(d) = document(e) {
            ins.begin(wce_editor_core::document::Document::id(d), u64::MAX - 11);
        }

        ins.section("New vegetation set (not saved)");
        let set = self.edit_set_entries(e, ins, set, true);
        let mut name = std::mem::take(&mut self.scratch_name);
        ins.edit_string("set_name", "name", &mut name);
        gui::begin_disabled(name.trim().is_empty() || set.entries.is_empty());
        let save = gui::small_button(&format!("{}  Save", icons::SAVE));
        gui::end_disabled();
        gui::same_line();
        gui::begin_disabled(set.entries.is_empty());
        let use_it = gui::small_button(&format!("{}  Place / paint with it", icons::TREES));
        gui::end_disabled();
        gui::same_line();
        let drop = gui::small_button(&format!("{}  Discard", icons::X));
        gui::separator();
        self.scratch_name = name;
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
            self.scratch_key.clear();
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
        self.scratch_key.clear();
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
