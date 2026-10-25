//! Prefabs in the Tile tool (`wce_tile::prefabs`): the library of TEd's saved groups (`<kit raw_data>/art/prefabs` of the kits and of the
//! tile), the Prefabs panel to place them, and the copies in the tile: an instance group named after a prefab's key is a copy, shown as one
//! Prefab row (its contents are not listed, and a click on them selects the copy). "Edit prefab" opens a copy for editing; Apply writes the
//! prefab file and gives every other copy in the tile the new contents (one undo entry), Cancel gives the copy the file's contents back.
//! "Unpack prefab" renames a copy so that it is a plain group; "Save as prefab" writes a group as a new prefab and names it after it.

use std::sync::Arc;

use wce_editor::editor::Editor;
use wce_editor::{gui, icons, theme};
use wce_editor_core::document::Document;
use wce_foundation::log;
use wce_foundation::math::Float3;
use wce_scene::render_scene::SourceRef;
use wce_tile::prefabs::{self, PrefabLibrary};
use wce_tile_editor::tile_document::{TileDocument, item_of};

use crate::scene_panel::{layer_by_key, layer_key};
use crate::tool::{TileTool, document, document_id, selected};

impl TileTool {
    /// The folders the library is read from: the projects' `raw_data/art/prefabs` (new prefabs go there), then `art/prefabs` of the open
    /// tile's kit and of the kits the Tiles panel lists (the Assembly Kit).
    pub(crate) fn prefab_folders(&self, e: &mut Editor) -> Vec<std::path::PathBuf> {
        let mut folders = vec![prefabs::prefab_root(&Self::workspace_dir(e).join("raw_data"))];
        let mut kits = self.kit_roots(e);
        if let Some(raw) = document(e).and_then(|d| d.project().raw_root.clone()) {
            kits.insert(0, raw);
        }

        for k in kits {
            let f = prefabs::prefab_root(&k);
            if !folders.contains(&f) {
                folders.push(f);
            }
        }

        folders
    }

    /// The prefab library (`prefab_folders`), scanned on first use and again for another tile.
    pub fn prefab_library(&mut self, e: &mut Editor) -> Arc<PrefabLibrary> {
        if let Some(lib) = &self.prefabs {
            return lib.clone();
        }

        let folders = self.prefab_folders(e);
        let lib = Arc::new(PrefabLibrary::scan_folders(&folders));
        self.prefabs = Some(lib.clone());
        self.prefab_version += 1;
        lib
    }

    /// Scans the prefab folders again.
    pub fn rescan_prefabs(&mut self, e: &mut Editor) -> usize {
        self.prefabs = None;
        self.prefab_library(e).len()
    }

    /// The prefab key of an element when it is a copy of a prefab of the library (an instance group named after the key).
    pub(crate) fn prefab_of(&self, d: &TileDocument, elem: usize) -> Option<String> {
        let key = d.prefab_key(elem)?;
        self.prefabs.as_ref()?.get(&key).map(|p| p.key.clone())
    }

    /// The copy being edited in this tile, if there is one.
    pub(crate) fn edited_copy(&self, d: &TileDocument) -> Option<usize> {
        let (doc, key, _) = self.prefab_edit.as_ref().filter(|(doc, _, _)| *doc == d.id())?;
        let _ = doc;
        layer_by_key(d.tile().tscene(), key)
    }

    /// A copy whose contents the Scene tree does not list and whose contents a click does not select: every copy but the edited one.
    pub(crate) fn closed_prefab(&self, d: &TileDocument, elem: usize) -> bool {
        self.prefab_of(d, elem).is_some() && self.edited_copy(d) != Some(elem)
    }

    /// The selection with the contents of closed copies replaced by the copies (once per frame, after the clicks).
    pub(crate) fn select_whole_prefabs(&mut self, e: &mut Editor) {
        let Some(id) = document_id(e) else { return };
        let Some(d) = e.docs.find_as::<TileDocument>(id) else { return };
        if self.prefabs.is_none() || e.selection.is_empty() {
            return;
        }

        let sc = d.tile().tscene();
        let mut changed = false;
        let mut items = vec![];
        for &el in &selected(e) {
            // An element that is gone (an undone addition) leaves the selection.
            let Some(element) = sc.doc().elements().get(el) else {
                changed = true;
                continue;
            };

            // The outermost closed copy above (or at) the element.
            let mut copy = None;
            let mut p = element.parent;
            while let Some(q) = p {
                if self.closed_prefab(d, q) {
                    copy = Some(q);
                }

                p = sc.doc().elements()[q].parent;
            }

            match copy {
                Some(c) => {
                    changed = true;
                    items.push(c);
                }

                None => items.push(el),
            }
        }

        if changed {
            items.sort_unstable();
            items.dedup();
            e.selection.clear();
            for el in items {
                e.selection.add(SourceRef { doc: id, item: item_of(el) });
            }
        }
    }

    /// Places a prefab in the working layer, on the ground at `at` (world metres).
    ///
    /// # Errors
    ///
    /// No tile, no such prefab, or its file can't be read.
    pub fn place_prefab(&mut self, e: &mut Editor, key: &str, at: Float3) -> Result<String, String> {
        let lib = self.prefab_library(e);
        let entry = lib.get(key).ok_or_else(|| format!("no prefab '{key}'"))?.clone();
        let p = lib.load(&entry.key)?;
        let d = document(e).ok_or("no tile open")?;
        let into = self.new_items_layer(d).ok_or("this tile has no scene graph")?;
        d.place_prefab(&entry.key, &p.group_text, into, at)?;
        Ok(format!("placed {}", entry.key))
    }

    /// Where a prefab placed from the panel goes: the terrain under the mouse, else the point the camera looks at.
    pub(crate) fn placement_point(&self, e: &Editor) -> Float3 {
        if self.terrain_hit { self.cursor } else { e.viewport.camera().target }
    }

    /// Opens a copy for editing (the one selected).
    ///
    /// # Errors
    ///
    /// Not a copy of a prefab, or another copy is being edited.
    pub fn edit_prefab(&mut self, e: &mut Editor, copy: usize) -> Result<String, String> {
        let d = document(e).ok_or("no tile open")?;
        let key = self.prefab_of(d, copy).ok_or("not a copy of a prefab")?;
        if self.edited_copy(d).is_some() {
            return Err("another copy is being edited: apply or cancel it first".to_owned());
        }

        let path = layer_key(d.tile().tscene(), copy).ok_or("the copy is not in the scene graph")?;
        self.prefab_edit = Some((d.id(), path, key.clone()));
        self.prefab_version += 1;
        Ok(format!("editing {key}: Apply writes the prefab and updates its copies in this tile"))
    }

    /// Ends the edit: writes the prefab file from the edited copy, and gives every other copy in the tile its contents (one undo entry).
    ///
    /// # Errors
    ///
    /// No edit, or the file can't be written.
    pub fn apply_prefab_edit(&mut self, e: &mut Editor) -> Result<String, String> {
        let lib = self.prefab_library(e);
        let d = document(e).ok_or("no tile open")?;
        let copy = self.edited_copy(d).ok_or("no prefab is being edited")?;
        let key = self.prefab_edit.as_ref().map(|p| p.2.clone()).unwrap_or_default();
        let entry = lib.get(&key).ok_or_else(|| format!("no prefab '{key}'"))?.clone();
        let children = d.children_text(copy);

        // The edit goes into the projects' prefab (its .wce), never into the kit.
        let projects = prefabs::prefab_root(&Self::workspace_dir(e).join("raw_data"));
        let saved = lib.save_edit(&entry.key, &projects, &children)?;
        let d = document(e).ok_or("no tile open")?;
        let others = d.prefab_copies(&key).into_iter().filter(|&c| c != copy).collect::<Vec<_>>();
        let n = d.set_groups_children(&others, &children, &format!("Update the copies of {key}"))?;
        self.end_prefab_edit(e);
        self.rescan_prefabs(e);
        Ok(format!("{key} saved to {}; {n} other cop{} updated", saved.display(), if n == 1 { "y" } else { "ies" }))
    }

    /// Ends the edit without saving: the copy gets the prefab file's contents back.
    ///
    /// # Errors
    ///
    /// No edit, or the file can't be read.
    pub fn cancel_prefab_edit(&mut self, e: &mut Editor) -> Result<String, String> {
        let lib = self.prefab_library(e);
        let d = document(e).ok_or("no tile open")?;
        let copy = self.edited_copy(d).ok_or("no prefab is being edited")?;
        let key = self.prefab_edit.as_ref().map(|p| p.2.clone()).unwrap_or_default();
        let p = lib.load(&key)?;
        d.set_groups_children(&[copy], &p.children_text, &format!("Cancel the edit of {key}"))?;
        self.end_prefab_edit(e);
        Ok(format!("{key}: edit cancelled"))
    }

    fn end_prefab_edit(&mut self, e: &mut Editor) {
        // The copy stays selected (as a whole again).
        let copy = document(e).and_then(|d| self.edited_copy(d));
        self.prefab_edit = None;
        self.prefab_version += 1;
        if let (Some(c), Some(id)) = (copy, document_id(e)) {
            e.selection.set(SourceRef { doc: id, item: item_of(c) });
        }
    }

    /// Makes a copy a plain group (renamed `<key> (unpacked)`).
    ///
    /// # Errors
    ///
    /// Not a copy of a prefab.
    pub fn unpack_prefab(&mut self, e: &mut Editor, copy: usize) -> Result<String, String> {
        let d = document(e).ok_or("no tile open")?;
        let key = self.prefab_of(d, copy).ok_or("not a copy of a prefab")?;
        if self.edited_copy(d) == Some(copy) {
            return Err("the copy is being edited: apply or cancel it first".to_owned());
        }

        d.unpack_prefab_copy(copy, &format!("{key} (unpacked)"));
        self.prefab_version += 1;
        Ok(format!("{key} unpacked into a plain group"))
    }

    /// Writes an instance group as a new prefab `<prefab folder>/<folder>/<name>.xml` and names the group after it (so it is a copy of it).
    ///
    /// # Errors
    ///
    /// Not an instance group, no name, no prefab folder, or a prefab of that name already exists.
    pub fn save_as_prefab(&mut self, e: &mut Editor, group: usize, name: &str, folder: &str) -> Result<String, String> {
        let name = name.trim();
        if name.is_empty() || name.contains(['/', '\\', ':', '*', '?', '"', '<', '>', '|']) {
            return Err("give the prefab a file name".to_owned());
        }

        let lib = self.prefab_library(e);
        if lib.get(name).is_some() {
            return Err(format!("a prefab '{name}' already exists"));
        }

        let d = document(e).ok_or("no tile open")?;
        if d.tile().tscene().type_of(group) != "UserInstanceGroup" {
            return Err("only an instance group can be saved as a prefab".to_owned());
        }

        let root = lib.roots().first().cloned().or_else(|| d.project().raw_root.as_deref().map(prefabs::prefab_root)).ok_or("no kit to put the prefab in")?;
        let folder = folder.trim().trim_matches(['/', '\\']);
        let path = if folder.is_empty() { root.join(format!("{name}.wce")) } else { root.join(folder).join(format!("{name}.wce")) };
        d.rename(group, name);
        prefabs::write(&path, &d.element_text(group))?;

        // The group is a reference to it now.
        d.make_prefab_reference(group, name);
        self.rescan_prefabs(e);
        Ok(format!("saved {}", path.display()))
    }

    /// "Save for Assembly Kit" of a prefab: its file as TEd reads it in the kit's art/prefabs (the prefabs placed in it baked).
    ///
    /// # Errors
    ///
    /// No such prefab, no kit, or the file can't be written.
    pub fn save_prefab_for_kit(&mut self, e: &mut Editor, key: &str) -> Result<String, String> {
        let lib = self.prefab_library(e);
        let projects = Self::workspace_dir(e).join("raw_data");
        let kit = self.kit_roots(e).into_iter().find(|r| *r != projects).ok_or("no Assembly Kit (set it in the game's settings)")?;
        let (path, report) = lib.save_for_kit(key, &prefabs::prefab_root(&kit))?;
        for line in report.polyfilled.iter().chain(&report.dropped) {
            log::info(format!("Save prefab for Assembly Kit: {line}"));
        }

        Ok(format!("{key} saved for the Assembly Kit: {} ({})", path.display(), report.summary()))
    }

    /// The "Save as prefab" dialog (opened from a group's menu in the Scene panel).
    pub(crate) fn draw_save_prefab_popup(&mut self, e: &mut Editor) {
        if self.prefab_save.as_ref().is_some_and(|s| s.0.starts_with('\u{1}')) {
            // Opened this frame.
            if let Some(s) = &mut self.prefab_save {
                s.0.remove(0);
            }

            gui::open_popup("Save as prefab");
        }

        if !gui::begin_popup_modal("Save as prefab", None, gui::WINDOW_ALWAYS_AUTO_RESIZE) {
            return;
        }

        let roots = self.prefab_library(e).roots().to_vec();
        let Some((group_key, name, folder)) = self.prefab_save.as_mut() else {
            gui::close_current_popup();
            gui::end_popup();
            return;
        };

        gui::text_wrapped("The group is written as a prefab file and renamed after it, so that it is a copy of it.");
        gui::set_next_item_width(320.0);
        gui::input_text("Name", Some("the prefab's key (file name)"), name, 128, 0);
        gui::set_next_item_width(320.0);
        gui::input_text("Folder", Some("below art/prefabs, e.g. global/rocks"), folder, 256, 0);
        if let Some(r) = roots.first() {
            gui::text_disabled(&format!("in {}", r.display()));
        }

        let (group_key, name, folder) = (group_key.clone(), name.clone(), folder.clone());
        if gui::button("Save") {
            let group = document(e).and_then(|d| layer_by_key(d.tile().tscene(), &group_key));
            self.status = match group {
                Some(g) => self.save_as_prefab(e, g, &name, &folder).unwrap_or_else(|err| err),
                None => "the group is gone".to_owned(),
            };

            self.prefab_save = None;
            gui::close_current_popup();
        }

        gui::same_line();
        if gui::button("Cancel") || gui::is_key_pressed(gui::KEY_ESCAPE, true) {
            self.prefab_save = None;
            gui::close_current_popup();
        }

        gui::end_popup();
    }

    /// Asks for "Save as prefab" on a group (the dialog opens on the next frame).
    pub(crate) fn request_save_prefab(&mut self, d: &TileDocument, group: usize) {
        if let Some(key) = layer_key(d.tile().tscene(), group) {
            let name = d.tile().tscene().name(group);
            self.prefab_save = Some((format!("\u{1}{key}"), name, String::new()));
        }
    }
}

/// The icon and type name a row of the Scene panel shows for a prefab copy.
pub(crate) const PREFAB_ICON: &str = icons::COMPONENT;

/// The colour of an edited copy's row marks.
pub(crate) fn edited_colour() -> [f32; 4] {
    theme::palette().highlight
}
