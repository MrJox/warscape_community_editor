//! The vegetation sets in the Tile tool (`wce_tile::vegetation_sets`): read from the projects' `raw_data/art/vegetation_sets`, placed and
//! painted from the Create panel (a key picked by the weights for each tree), made from the selected trees, and edited in the Inspector while
//! one is in use (saved at once).

use std::sync::Arc;

use wce_editor::editor::Editor;
use wce_editor::{gui, icons};
use wce_editor_core::inspector::Inspector;
use wce_tile::vegetation_sets::{SetEntry, VegetationSet, scan, sets_root};

use crate::tool::{TileTool, document, selected};

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
        let name = name.trim();
        if name.is_empty() || name.contains(['/', '\\', ':', '*', '?', '"', '<', '>', '|']) {
            return Err("give the set a file name".to_owned());
        }

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

    /// The set in use, in the Inspector: its trees, their weights and scale ranges, add and remove; saved at once.
    pub(crate) fn inspect_veg_set(&mut self, e: &mut Editor, ins: &mut dyn Inspector) {
        let Some(mut set) = self.veg_set.clone() else { return };
        let before = set.clone();
        ins.section(&format!("Vegetation set {}", set.name));
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

        // Another tree: its key (a tree of the game's files).
        let mut key = std::mem::take(&mut self.new_set_key);
        ins.edit_string("new_key", "add a tree (species.variant)", &mut key);
        let known = self.catalogue(e, "VEGETATION_INSTANCE").iter().any(|k| *k == key.trim());
        gui::begin_disabled(!known || set.entries.iter().any(|en| en.key == key.trim()));
        if gui::small_button(&format!("{}  Add", icons::PLUS)) {
            set.entries.push(SetEntry { key: key.trim().to_owned(), weight: 1.0, scale: None });
            key.clear();
        }

        gui::end_disabled();
        self.new_set_key = key;
        if set != before {
            match set.save() {
                Ok(()) => {
                    self.veg_set = Some(set);
                    self.rescan_vegetation_sets();
                }

                Err(err) => self.status = err,
            }
        }
    }
}
